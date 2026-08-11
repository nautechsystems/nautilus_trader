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

use std::{any::Any, fmt::Display};

use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::identifiers::{ClientId, TraderId, Venue};
use ustr::Ustr;

/// Represents the availability state of a socket transport.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.common",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.common")
)]
pub enum SocketState {
    /// The transport is available.
    Connected,
    /// An active transport was lost.
    Disconnected,
}

/// Transport-neutral notification that a socket state changed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SocketStateChange {
    /// The client ID associated with the transport.
    pub client_id: ClientId,
    /// The venue associated with the transport, if any.
    pub venue: Option<Venue>,
    /// Stable, non-secret label identifying the endpoint.
    pub endpoint: Ustr,
    /// The current transport state.
    pub state: SocketState,
}

impl SocketStateChange {
    /// Creates a new [`SocketStateChange`] instance.
    #[must_use]
    pub const fn new(
        client_id: ClientId,
        venue: Option<Venue>,
        endpoint: Ustr,
        state: SocketState,
    ) -> Self {
        Self {
            client_id,
            venue,
            endpoint,
            state,
        }
    }
}

impl Display for SocketStateChange {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(client_id={}, venue={:?}, endpoint={}, state={:?})",
            stringify!(SocketStateChange),
            self.client_id,
            self.venue,
            self.endpoint,
            self.state,
        )
    }
}

/// Represents an event where a socket transport state has changed.
#[repr(C)]
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
pub struct SocketStateChanged {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The client ID associated with the transport.
    pub client_id: ClientId,
    /// The venue associated with the transport, if any.
    pub venue: Option<Venue>,
    /// Stable, non-secret label identifying the endpoint.
    pub endpoint: Ustr,
    /// The current transport state.
    pub state: SocketState,
    /// The event ID.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl SocketStateChanged {
    /// Creates a new [`SocketStateChanged`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        client_id: ClientId,
        venue: Option<Venue>,
        endpoint: Ustr,
        state: SocketState,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            endpoint,
            state,
            event_id,
            ts_event,
            ts_init,
        }
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Display for SocketStateChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, client_id={}, venue={:?}, endpoint={}, state={:?}, event_id={})",
            stringify!(SocketStateChanged),
            self.trader_id,
            self.client_id,
            self.venue,
            self.endpoint,
            self.state,
            self.event_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(
        Some("BINANCE"),
        SocketState::Disconnected,
        "binance-futures-market-streams"
    )]
    #[case(None, SocketState::Connected, "direct-feed")]
    fn test_socket_state_changed_new_assigns_all_fields(
        #[case] venue: Option<&str>,
        #[case] state: SocketState,
        #[case] endpoint: &str,
    ) {
        let trader_id = TraderId::from("TRADER-001");
        let client_id = ClientId::from("BINANCE");
        let venue = venue.map(Venue::from);
        let endpoint = Ustr::from(endpoint);
        let event_id = UUID4::from("00000000-0000-4000-8000-000000000001");
        let ts_event = UnixNanos::from(29);
        let ts_init = UnixNanos::from(31);

        let event = SocketStateChanged::new(
            trader_id, client_id, venue, endpoint, state, event_id, ts_event, ts_init,
        );

        assert_eq!(event.trader_id, trader_id);
        assert_eq!(event.client_id, client_id);
        assert_eq!(event.venue, venue);
        assert_eq!(event.endpoint, endpoint);
        assert_eq!(event.state, state);
        assert_eq!(event.event_id, event_id);
        assert_eq!(event.ts_event, ts_event);
        assert_eq!(event.ts_init, ts_init);
    }
}
