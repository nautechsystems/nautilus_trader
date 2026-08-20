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

//! Socket state publication for the Lighter adapter.

use nautilus_common::messages::{
    SystemEvent,
    system::{SocketState as SystemSocketState, SocketStateChange},
};
use nautilus_model::identifiers::ClientId;
use nautilus_network::{SocketState, SocketStateSink};
use ustr::Ustr;

use super::consts::LIGHTER_VENUE;

pub(crate) const DATA_STREAMS_ENDPOINT: &str = "lighter-data-streams";
pub(crate) const USER_STREAMS_ENDPOINT: &str = "lighter-user-streams";

/// Returns a sink republishing transport state changes for `endpoint` as system events.
///
/// Callers resolve `sender` once on the thread owning the system event sender, because
/// [`nautilus_common::live::runner::try_get_system_event_sender`] reads a thread-local and a
/// later lookup from a spawned task returns `None`.
pub(crate) fn socket_state_sink(
    client_id: ClientId,
    endpoint: &'static str,
    sender: tokio::sync::mpsc::UnboundedSender<SystemEvent>,
) -> SocketStateSink {
    let endpoint = Ustr::from(endpoint);

    SocketStateSink::new(move |state| {
        let state = match state {
            SocketState::Connected => SystemSocketState::Connected,
            SocketState::Disconnected => SystemSocketState::Disconnected,
        };
        let change = SocketStateChange::new(client_id, Some(*LIGHTER_VENUE), endpoint, state);
        if let Err(e) = sender.send(SystemEvent::SocketState(change)) {
            log::error!("Failed to emit socket state change: {e}");
        }
    })
}
