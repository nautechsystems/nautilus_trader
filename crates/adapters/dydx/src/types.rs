// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Custom data types specific to the dYdX adapter.

use nautilus_core::UnixNanos;
use nautilus_model::{identifiers::InstrumentId, types::Price};

/// dYdX oracle price update.
///
/// Oracle prices on dYdX are used for:
/// - Mark price calculations for perpetual markets
/// - Liquidation price determination
/// - Funding rate calculations
///
/// Oracle prices are streamed via the v4_markets WebSocket channel and represent
/// off-chain price feeds aggregated by dYdX validators.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx")
)]
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct DydxOraclePrice {
    /// The instrument ID for the oracle price.
    pub instrument_id: InstrumentId,
    /// The oracle price reported by dYdX validators.
    pub oracle_price: Price,
    /// UNIX timestamp (nanoseconds) when the oracle price was updated.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl DydxOraclePrice {
    /// Creates a new [`DydxOraclePrice`] instance.
    #[must_use]
    pub const fn new(
        instrument_id: InstrumentId,
        oracle_price: Price,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            oracle_price,
            ts_event,
            ts_init,
        }
    }
}
