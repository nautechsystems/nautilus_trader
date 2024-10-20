// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_core::nanos::UnixNanos;
use serde::{Deserialize, Serialize};

use crate::{
    enums::{OrderSide, PositionSide},
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    types::{currency::Currency, money::Money, quantity::Quantity},
};

/// Represents a position state snapshot as a certain instant.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct PositionSnapshot {
    /// The trader ID associated with the snapshot.
    pub trader_id: TraderId,
    /// The strategy ID associated with the snapshot.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the snapshot.
    pub instrument_id: InstrumentId,
    /// The position ID associated with the snapshot.
    pub position_id: PositionId,
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The client order ID for the order which opened the position.
    pub opening_order_id: ClientOrderId,
    /// The client order ID for the order which closed the position.
    pub closing_order_id: Option<ClientOrderId>,
    /// The entry direction from open.
    pub entry: OrderSide,
    /// The position side.
    pub side: PositionSide,
    /// The position signed quantity (positive for LONG, negative for SHOT).
    pub signed_qty: f64,
    /// The position open quantity.
    pub quantity: Quantity,
    /// The peak directional quantity reached by the position.
    pub peak_qty: Quantity,
    /// The last fill quantity for the position.
    // pub last_qty: Quantity,  TODO: Implement
    // /// The last fill price for the position.
    // pub last_px: Price,  TODO: Implement
    /// The position quote currency.
    pub quote_currency: Currency,
    /// The position base currency.
    pub base_currency: Option<Currency>,
    /// The position settlement currency.
    pub settlement_currency: Currency,
    /// The average open price.
    pub avg_px_open: f64,
    /// The average closing price.
    pub avg_px_close: Option<f64>,
    /// The realized return for the position.
    pub realized_return: Option<f64>,
    /// The realized PnL for the position (including commissions).
    pub realized_pnl: Money,
    /// The unrealized PnL for the position (including commissions).
    pub unrealized_pnl: Option<Money>,
    /// The commissions for the position.
    pub commissions: Vec<Money>,
    /// The open duration for the position (nanoseconds).
    pub duration_ns: Option<u64>,
    /// UNIX timestamp (nanoseconds) when the position opened.
    pub ts_opened: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the position closed.
    pub ts_closed: Option<UnixNanos>,
    /// UNIX timestamp (nanoseconds) when the last position event occurred.
    pub ts_last: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the snapshot was initialized.
    pub ts_init: UnixNanos,
}
