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

use std::fmt::Display;

use nautilus_core::UnixNanos;
use nautilus_model::identifiers::{InstrumentId, StrategyId, TraderId};

/// Command replacing the external order claims owned by a strategy.
///
/// The instrument IDs are the complete desired claim set, not an additive update. A stale partial
/// set releases claims omitted from the command, and an empty set releases every claim owned by the
/// strategy. Conflicts and repeated instruments reject the entire update atomically, while claims
/// owned by other strategies remain unchanged.
///
/// Sending this command only confirms that it was queued, not that it was applied. The caller must
/// not infer that routing changed from a successful send.
///
/// This command is intended for low-rate control operations. Its unbounded channel provides no
/// backpressure.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetExternalOrderClaims {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_ids: Vec<InstrumentId>,
    pub ts_init: UnixNanos,
}

impl SetExternalOrderClaims {
    /// Creates a new [`SetExternalOrderClaims`] instance.
    #[must_use]
    pub const fn new(
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_ids: Vec<InstrumentId>,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            trader_id,
            strategy_id,
            instrument_ids,
            ts_init,
        }
    }
}

impl Display for SetExternalOrderClaims {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(trader_id={}, strategy_id={}, instrument_ids={:?})",
            stringify!(SetExternalOrderClaims),
            self.trader_id,
            self.strategy_id,
            self.instrument_ids,
        )
    }
}
