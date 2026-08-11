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

//! Socket state publication for the Polymarket adapter.

use nautilus_common::{
    live::runner::try_get_system_event_sender,
    messages::{
        SystemEvent,
        system::{SocketState as SystemSocketState, SocketStateChange},
    },
};
use nautilus_model::identifiers::ClientId;
use nautilus_network::{SocketState, SocketStateSink};
use ustr::Ustr;

use super::consts::POLYMARKET_VENUE;

pub(crate) const MARKET_STREAMS_ENDPOINT: &str = "polymarket-market-streams";
pub(crate) const RTDS_STREAMS_ENDPOINT: &str = "polymarket-rtds-streams";
pub(crate) const USER_STREAMS_ENDPOINT: &str = "polymarket-user-streams";

#[derive(Clone, Debug)]
pub(crate) struct SocketStatePublisher {
    client_id: ClientId,
    sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
}

impl SocketStatePublisher {
    pub(crate) fn new(client_id: ClientId) -> Option<Self> {
        try_get_system_event_sender().map(|sender| Self { client_id, sender })
    }

    pub(crate) fn sink(&self, endpoint: &'static str) -> SocketStateSink {
        let client_id = self.client_id;
        let endpoint = Ustr::from(endpoint);
        let sender = self.sender.clone();

        SocketStateSink::new(move |state| {
            let state = match state {
                SocketState::Connected => SystemSocketState::Connected,
                SocketState::Disconnected => SystemSocketState::Disconnected,
            };
            let change =
                SocketStateChange::new(client_id, Some(*POLYMARKET_VENUE), endpoint, state);
            if let Err(e) = sender.send(SystemEvent::SocketState(change)) {
                log::error!("Failed to emit socket state change: {e}");
            }
        })
    }
}
