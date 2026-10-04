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

use std::{
    any::Any,
    fmt::{Debug, Display},
};

use indexmap::IndexMap;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{enums::TradingState, identifiers::TraderId};
use serde::{Deserialize, Serialize};

/// Represents an event where trading state has changed at the `RiskEngine`.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
pub struct TradingStateChanged {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The trading state.
    pub state: TradingState,
    /// The risk engine configuration.
    pub config: IndexMap<String, String>,
    /// The event ID.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the instance was initialized.
    pub ts_init: UnixNanos,
}

impl TradingStateChanged {
    /// Creates a new [`TradingStateChanged`] instance.
    #[must_use]
    pub fn new(
        trader_id: TraderId,
        state: TradingState,
        config: IndexMap<String, String>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            state,
            config,
            event_id,
            ts_event,
            ts_init,
        }
    }

    pub fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Display for TradingStateChanged {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, state={}, event_id={})",
            stringify!(TradingStateChanged),
            self.trader_id,
            self.state,
            self.event_id,
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    fn trading_state_changed() -> TradingStateChanged {
        let mut config = IndexMap::new();
        config.insert(
            "max_order_submit_rate".to_string(),
            "100/00:00:01".to_string(),
        );

        TradingStateChanged::new(
            TraderId::from("TESTER-001"),
            TradingState::Halted,
            config,
            UUID4::from("00000000-0000-4000-8000-000000000002"),
            UnixNanos::from(7),
            UnixNanos::from(11),
        )
    }

    #[rstest]
    fn test_new_assigns_every_field() {
        let event = trading_state_changed();

        assert_eq!(event.trader_id, TraderId::from("TESTER-001"));
        assert_eq!(event.state, TradingState::Halted);
        assert_eq!(
            event.config.get("max_order_submit_rate").unwrap(),
            "100/00:00:01"
        );
        assert_eq!(
            event.event_id,
            UUID4::from("00000000-0000-4000-8000-000000000002")
        );
        assert_eq!(event.ts_event, UnixNanos::from(7));
        assert_eq!(event.ts_init, UnixNanos::from(11));
    }

    #[rstest]
    fn test_display_reports_identity_state_and_event_id() {
        assert_eq!(
            trading_state_changed().to_string(),
            "TradingStateChanged(trader_id=TESTER-001, state=HALTED, \
             event_id=00000000-0000-4000-8000-000000000002)"
        );
    }

    #[rstest]
    fn test_as_any_downcasts_to_self() {
        let event = trading_state_changed();

        assert_eq!(
            event.as_any().downcast_ref::<TradingStateChanged>(),
            Some(&event)
        );
    }

    #[rstest]
    fn test_serde_round_trips() {
        let event = trading_state_changed();

        let json = serde_json::to_string(&event).unwrap();

        assert_eq!(
            serde_json::from_str::<TradingStateChanged>(&json).unwrap(),
            event
        );
    }
}
