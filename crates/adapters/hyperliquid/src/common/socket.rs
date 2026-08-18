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

//! Socket state publication and reconnect control for the Hyperliquid adapter.

use nautilus_common::{
    clients::{
        SocketReconnectHandle, SocketReconnectRegistration, SocketReconnectRegistry,
        SocketReconnectRequestOutcome,
    },
    live::runner::try_get_system_event_sender,
    messages::{
        SystemEvent,
        system::{SocketState as SystemSocketState, SocketStateChange},
    },
};
use nautilus_model::identifiers::ClientId;
use nautilus_network::{
    SocketState, SocketStateSink, mode::ReconnectRequestOutcome,
    websocket::WebSocketReconnectHandle,
};
use ustr::Ustr;

use super::consts::HYPERLIQUID_VENUE;

pub(crate) const DATA_STREAMS_ENDPOINT: &str = "hyperliquid-data-streams";
pub(crate) const USER_STREAMS_ENDPOINT: &str = "hyperliquid-user-streams";

#[derive(Clone, Debug)]
pub(crate) struct SocketStatePublisher {
    client_id: ClientId,
    sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
    registry: SocketReconnectRegistry,
}

impl SocketStatePublisher {
    pub(crate) fn new(client_id: ClientId, registry: SocketReconnectRegistry) -> Option<Self> {
        try_get_system_event_sender().map(|sender| Self {
            client_id,
            sender,
            registry,
        })
    }

    pub(crate) fn control(&self, endpoint: impl AsRef<str>) -> SocketControl {
        SocketControl {
            client_id: self.client_id,
            endpoint: Ustr::from(endpoint.as_ref()),
            sender: self.sender.clone(),
            registry: self.registry.clone(),
        }
    }
}

/// Observation and reconnect control for one logical socket endpoint.
#[derive(Clone, Debug)]
pub(crate) struct SocketControl {
    client_id: ClientId,
    endpoint: Ustr,
    sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
    registry: SocketReconnectRegistry,
}

impl SocketControl {
    pub(crate) fn sink(&self) -> SocketStateSink {
        let client_id = self.client_id;
        let endpoint = self.endpoint;
        let sender = self.sender.clone();

        SocketStateSink::new(move |state| {
            let state = match state {
                SocketState::Connected => SystemSocketState::Connected,
                SocketState::Disconnected => SystemSocketState::Disconnected,
            };
            let change =
                SocketStateChange::new(client_id, Some(*HYPERLIQUID_VENUE), endpoint, state);
            if let Err(e) = sender.send(SystemEvent::SocketState(change)) {
                log::error!("Failed to emit socket state change: {e}");
            }
        })
    }

    pub(crate) fn register(&self, handle: WebSocketReconnectHandle) -> SocketReconnectRegistration {
        let handle = SocketReconnectHandle::new(move || match handle.request_reconnect() {
            ReconnectRequestOutcome::Accepted => SocketReconnectRequestOutcome::Accepted,
            ReconnectRequestOutcome::AlreadyReconnecting => {
                SocketReconnectRequestOutcome::AlreadyReconnecting
            }
            ReconnectRequestOutcome::Disconnected => SocketReconnectRequestOutcome::Disconnected,
            ReconnectRequestOutcome::Closed => SocketReconnectRequestOutcome::Closed,
            ReconnectRequestOutcome::Unsupported => SocketReconnectRequestOutcome::Unsupported,
        });
        self.registry.register(self.endpoint, handle)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::clients::SocketReconnectRegistry;
    use nautilus_model::identifiers::ClientId;
    use rstest::rstest;

    use super::SocketStatePublisher;

    #[rstest]
    fn socket_state_publisher_absent_without_system_sender() {
        assert!(
            SocketStatePublisher::new(
                ClientId::from("HYPERLIQUID"),
                SocketReconnectRegistry::default(),
            )
            .is_none()
        );
    }
}
