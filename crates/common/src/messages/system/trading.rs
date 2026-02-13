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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
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
