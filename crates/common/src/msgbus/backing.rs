// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
//  https://nautechsystems.io
//
//  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
//  You may not use this file except in compliance with the License.
//  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
//
//  Unless required by applicable law or agreed to in writing, software
//  distributed under the License is distributed on an "AS IS" BASIS,
//  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
//  See the License for the specific language governing permissions and
//  limitations under the License.
// -------------------------------------------------------------------------------------------------

//! External message bus backing traits.
//!
//! Ingress and egress are named from the local message bus boundary. Egress carries serialized
//! [`BusMessage`] values from the local bus to external streams. Ingress exposes serialized
//! [`BusMessage`] values read from external streams so a live bridge can republish them on the
//! local bus.
//!
//! - `MessageBusBackingFactory` creates concrete backing technology for a bus runtime.
//! - `MessageBusBacking` owns the runtime facade used by core bus wiring.
//! - `MessageBusExternalEgress` accepts outbound messages from the local bus.
//! - `MessageBusExternalIngress` exposes the inbound external stream receiver.

use std::fmt::Debug;

use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;

pub use super::config::MessageBusConfig;
use crate::msgbus::BusMessage;

/// Factory for constructing external message bus backings.
///
/// Implementations own concrete backing configuration and return the [`MessageBusBacking`] surface
/// used by the core bus runtime.
pub trait MessageBusBackingFactory: Debug + Send + Sync {
    /// Creates a message bus backing for the given bus runtime.
    ///
    /// # Errors
    ///
    /// Returns an error if backing construction or connection setup fails.
    fn create(
        &self,
        trader_id: TraderId,
        instance_id: UUID4,
        config: MessageBusConfig,
    ) -> anyhow::Result<Box<dyn MessageBusBacking>>;
}

/// External message bus backing facade.
///
/// Implementations own the concrete backing technology and provide the runtime-facing publication
/// surface used by the core bus.
pub trait MessageBusBacking {
    /// Returns `true` if the backing has been closed.
    fn is_closed(&self) -> bool;

    /// Queues a serialized bus message for external egress.
    fn publish(&self, message: BusMessage);

    /// Closes the backing and releases any owned resources.
    fn close(&mut self);
}

/// External egress surface for serialized message bus publications.
///
/// The core bus passes each outbound message as a [`BusMessage`] carrying the
/// `topic`, `payload_type`, and serialized `payload`. Implementations must not block the publishing
/// thread. If the underlying channel is full, drop the message in the implementation rather than
/// applying back-pressure to the node.
pub trait MessageBusExternalEgress {
    /// Returns `true` if egress has been closed.
    fn is_closed(&self) -> bool;

    /// Queues a serialized bus message for external egress.
    fn publish(&self, message: BusMessage);

    /// Closes egress and stops accepting outbound messages.
    fn close(&mut self);
}

/// External ingress surface for serialized message bus publications.
///
/// The live bridge consumes each inbound [`BusMessage`] as a topic and serialized
/// payload. The receiver can be taken only once so ingress can hand ownership of the external stream
/// to the bridge without exposing concrete backing details.
#[cfg(feature = "live")]
pub trait MessageBusExternalIngress {
    /// Returns `true` if ingress has been closed.
    fn is_closed(&self) -> bool;

    /// Takes the inbound message receiver for live bridge consumption.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver has already been taken or is unavailable.
    fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>>;

    /// Closes ingress and stops accepting inbound messages.
    fn close(&mut self);
}

#[cfg(all(test, feature = "live"))]
mod tests {
    use bytes::Bytes;
    use rstest::*;

    use crate::{
        enums::SerializationEncoding,
        msgbus::{
            BusMessage, BusPayloadType,
            MessageBusExternalIngress as ReexportedMessageBusExternalIngress,
        },
    };

    struct CapturingExternalIngress {
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: bool,
    }

    impl ReexportedMessageBusExternalIngress for CapturingExternalIngress {
        fn is_closed(&self) -> bool {
            self.closed
        }

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
        }

        fn close(&mut self) {
            self.closed = true;
        }
    }

    #[rstest]
    fn test_message_bus_external_ingress_reexport_accepts_bus_messages() {
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let mut ingress = CapturingExternalIngress {
            rx: Some(rx),
            closed: false,
        };
        let message = BusMessage::with_str_topic(
            "events/data",
            BusPayloadType::QuoteTick,
            Bytes::from_static(b"payload"),
            SerializationEncoding::Json,
        );

        tx.try_send(message.clone()).unwrap();
        let mut stream_rx =
            ReexportedMessageBusExternalIngress::take_receiver(&mut ingress).unwrap();
        let received = stream_rx.try_recv().unwrap();

        assert_eq!(received.topic, message.topic);
        assert_eq!(received.payload, message.payload);
        assert!(ReexportedMessageBusExternalIngress::take_receiver(&mut ingress).is_err());

        ReexportedMessageBusExternalIngress::close(&mut ingress);
        assert!(ReexportedMessageBusExternalIngress::is_closed(&ingress));
    }
}
