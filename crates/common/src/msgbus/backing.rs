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
//! - `external_egress_from_backing` adapts a backing into an egress surface.
//! - `external_io_from_backing` adapts one backing into shared egress and ingress surfaces.

use std::{
    cell::{Cell, RefCell},
    fmt::Debug,
    rc::Rc,
};

use nautilus_core::UUID4;
use nautilus_model::identifiers::TraderId;

use super::config::MessageBusConfig;
use crate::msgbus::BusMessage;

/// Receiver for external message bus ingress publications.
#[cfg(feature = "live")]
pub type MessageBusExternalReceiver = tokio::sync::mpsc::Receiver<BusMessage>;

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
/// surface used by the core bus. With the `live` feature, the same backing can also hand an
/// inbound receiver to the live bridge.
pub trait MessageBusBacking {
    /// Returns `true` if the backing has been closed.
    fn is_closed(&self) -> bool;

    /// Queues a serialized bus message for external egress.
    fn publish(&self, message: BusMessage);

    /// Takes the inbound message receiver for live bridge consumption.
    ///
    /// # Errors
    ///
    /// Returns an error if the receiver has already been taken or is unavailable.
    #[cfg(feature = "live")]
    fn take_receiver(&mut self) -> anyhow::Result<MessageBusExternalReceiver> {
        anyhow::bail!("external ingress receiver unavailable")
    }

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
    fn take_receiver(&mut self) -> anyhow::Result<MessageBusExternalReceiver>;

    /// Closes ingress and stops accepting inbound messages.
    fn close(&mut self);
}

type SharedMessageBusBacking = Rc<RefCell<Box<dyn MessageBusBacking>>>;
type SharedMessageBusCloseState = Rc<Cell<bool>>;

/// Wraps a message bus backing for external egress installation.
#[must_use]
pub fn external_egress_from_backing(
    backing: Box<dyn MessageBusBacking>,
) -> Box<dyn MessageBusExternalEgress> {
    Box::new(BackingExternalEgress {
        backing: Rc::new(RefCell::new(backing)),
        closed: Rc::new(Cell::new(false)),
    })
}

/// Splits a message bus backing into external egress and ingress surfaces.
#[cfg(feature = "live")]
#[must_use]
pub fn external_io_from_backing(
    backing: Box<dyn MessageBusBacking>,
) -> (
    Box<dyn MessageBusExternalEgress>,
    Box<dyn MessageBusExternalIngress>,
) {
    let backing = Rc::new(RefCell::new(backing));
    let closed = Rc::new(Cell::new(false));
    (
        Box::new(BackingExternalEgress {
            backing: backing.clone(),
            closed: closed.clone(),
        }),
        Box::new(BackingExternalIngress { backing, closed }),
    )
}

struct BackingExternalEgress {
    backing: SharedMessageBusBacking,
    closed: SharedMessageBusCloseState,
}

impl MessageBusExternalEgress for BackingExternalEgress {
    fn is_closed(&self) -> bool {
        self.backing.borrow().is_closed()
    }

    fn publish(&self, message: BusMessage) {
        self.backing.borrow().publish(message);
    }

    fn close(&mut self) {
        if !self.closed.replace(true) {
            self.backing.borrow_mut().close();
        }
    }
}

#[cfg(feature = "live")]
struct BackingExternalIngress {
    backing: SharedMessageBusBacking,
    closed: SharedMessageBusCloseState,
}

#[cfg(feature = "live")]
impl MessageBusExternalIngress for BackingExternalIngress {
    fn is_closed(&self) -> bool {
        self.backing.borrow().is_closed()
    }

    fn take_receiver(&mut self) -> anyhow::Result<MessageBusExternalReceiver> {
        self.backing.borrow_mut().take_receiver()
    }

    fn close(&mut self) {
        if !self.closed.replace(true) {
            self.backing.borrow_mut().close();
        }
    }
}

#[cfg(all(test, feature = "live"))]
mod tests {
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };

    use bytes::Bytes;
    use rstest::*;

    use super::{MessageBusBacking, external_egress_from_backing, external_io_from_backing};
    use crate::{
        enums::SerializationEncoding,
        msgbus::{
            BusMessage, BusPayloadType,
            MessageBusExternalIngress as ReexportedMessageBusExternalIngress,
        },
    };

    struct CapturingBacking {
        rx: Option<tokio::sync::mpsc::Receiver<BusMessage>>,
        closed: bool,
    }

    impl MessageBusBacking for CapturingBacking {
        fn is_closed(&self) -> bool {
            self.closed
        }

        fn publish(&self, _message: BusMessage) {}

        fn take_receiver(&mut self) -> anyhow::Result<tokio::sync::mpsc::Receiver<BusMessage>> {
            self.rx
                .take()
                .ok_or_else(|| anyhow::anyhow!("Stream receiver already taken"))
        }

        fn close(&mut self) {
            self.closed = true;
        }
    }

    struct CapturingPublishBacking {
        publications: Rc<RefCell<Vec<BusMessage>>>,
        closed: Rc<Cell<bool>>,
        close_count: Rc<Cell<u32>>,
    }

    impl MessageBusBacking for CapturingPublishBacking {
        fn is_closed(&self) -> bool {
            self.closed.get()
        }

        fn publish(&self, message: BusMessage) {
            self.publications.borrow_mut().push(message);
        }

        fn close(&mut self) {
            self.close_count.set(self.close_count.get() + 1);
            self.closed.set(true);
        }
    }

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

    #[rstest]
    fn test_external_io_from_backing_shares_close_state() {
        let (tx, rx) = tokio::sync::mpsc::channel::<BusMessage>(1);
        let backing = CapturingBacking {
            rx: Some(rx),
            closed: false,
        };
        let message = BusMessage::with_str_topic(
            "events/data",
            BusPayloadType::QuoteTick,
            Bytes::from_static(b"payload"),
            SerializationEncoding::Json,
        );
        let (mut egress, mut ingress) = external_io_from_backing(Box::new(backing));

        tx.try_send(message.clone()).unwrap();
        let mut stream_rx = ingress.take_receiver().unwrap();
        let received = stream_rx.try_recv().unwrap();

        assert_eq!(received.topic, message.topic);
        assert!(!egress.is_closed());
        assert!(!ingress.is_closed());

        egress.close();

        assert!(egress.is_closed());
        assert!(ingress.is_closed());
    }

    #[rstest]
    fn test_external_egress_from_backing_forwards_publications() {
        let publications = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(false));
        let close_count = Rc::new(Cell::new(0));
        let backing = CapturingPublishBacking {
            publications: publications.clone(),
            closed: closed.clone(),
            close_count,
        };
        let mut egress = external_egress_from_backing(Box::new(backing));
        let message = BusMessage::with_str_topic(
            "events/data",
            BusPayloadType::QuoteTick,
            Bytes::from_static(b"payload"),
            SerializationEncoding::Json,
        );

        egress.publish(message.clone());
        egress.close();

        let publications = publications.borrow();
        assert_eq!(publications.len(), 1);
        assert_eq!(publications[0].topic, message.topic);
        assert!(closed.get());
    }

    #[rstest]
    fn test_external_io_from_backing_closes_shared_backing_once() {
        let publications = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(false));
        let close_count = Rc::new(Cell::new(0));
        let backing = CapturingPublishBacking {
            publications,
            closed,
            close_count: close_count.clone(),
        };
        let (mut egress, mut ingress) = external_io_from_backing(Box::new(backing));

        egress.close();
        ingress.close();

        assert_eq!(close_count.get(), 1);
    }

    #[rstest]
    fn test_external_io_from_backing_close_does_not_depend_on_backing_is_closed() {
        let publications = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(true));
        let close_count = Rc::new(Cell::new(0));
        let backing = CapturingPublishBacking {
            publications,
            closed,
            close_count: close_count.clone(),
        };
        let (mut egress, mut ingress) = external_io_from_backing(Box::new(backing));

        egress.close();
        ingress.close();

        assert_eq!(close_count.get(), 1);
    }

    #[rstest]
    fn test_external_io_from_backing_default_receiver_is_unavailable() {
        let publications = Rc::new(RefCell::new(Vec::new()));
        let closed = Rc::new(Cell::new(false));
        let close_count = Rc::new(Cell::new(0));
        let backing = CapturingPublishBacking {
            publications,
            closed,
            close_count,
        };
        let (_egress, mut ingress) = external_io_from_backing(Box::new(backing));

        let error = ingress
            .take_receiver()
            .expect_err("egress-only backing should not provide ingress receiver");

        assert_eq!(error.to_string(), "external ingress receiver unavailable");
    }
}
