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

use nautilus_core::{UUID4, UnixNanos};

use crate::{
    enums::{OrderSide, PositionSide},
    events::OrderFilled,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};

/// Represents an event where a position has changed.
#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionChanged {
    /// The trader ID associated with the event.
    pub trader_id: TraderId,
    /// The strategy ID associated with the event.
    pub strategy_id: StrategyId,
    /// The instrument ID associated with the event.
    pub instrument_id: InstrumentId,
    /// The position ID associated with the event.
    pub position_id: PositionId,
    /// The account ID associated with the position.
    pub account_id: AccountId,
    /// The client order ID for the order which opened the position.
    pub opening_order_id: ClientOrderId,
    /// The position entry order side.
    pub entry: OrderSide,
    /// The position side.
    pub side: PositionSide,
    /// The current signed quantity (positive for position side `LONG`, negative for `SHORT`).
    pub signed_qty: f64,
    /// The current open quantity.
    pub quantity: Quantity,
    /// The peak directional quantity reached by the position.
    pub peak_quantity: Quantity,
    /// The last fill quantity for the position.
    pub last_qty: Quantity,
    /// The last fill price for the position.
    pub last_px: Price,
    /// The position quote currency.
    pub currency: Currency,
    /// The average open price.
    pub avg_px_open: f64,
    /// The average close price.
    pub avg_px_close: Option<f64>,
    /// The realized return for the position.
    pub realized_return: f64,
    /// The realized PnL for the position (including commissions).
    pub realized_pnl: Option<Money>,
    /// The unrealized PnL for the position (including commissions).
    pub unrealized_pnl: Money,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the position was opened.
    pub ts_opened: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PositionChanged {
    pub fn create(
        position: &Position,
        fill: &OrderFilled,
        event_id: UUID4,
        ts_init: UnixNanos,
    ) -> PositionChanged {
        PositionChanged {
            trader_id: position.trader_id,
            strategy_id: position.strategy_id,
            instrument_id: position.instrument_id,
            position_id: position.id,
            account_id: position.account_id,
            opening_order_id: position.opening_order_id,
            entry: position.entry,
            side: position.side,
            signed_qty: position.signed_qty,
            quantity: position.quantity,
            peak_quantity: position.peak_qty,
            last_qty: fill.last_qty,
            last_px: fill.last_px,
            currency: position.quote_currency,
            avg_px_open: position.avg_px_open,
            avg_px_close: position.avg_px_close,
            realized_return: position.realized_return,
            realized_pnl: position.realized_pnl,
            unrealized_pnl: Money::new(0.0, position.quote_currency),
            event_id,
            ts_opened: position.ts_opened,
            ts_event: fill.ts_event,
            ts_init,
        }
    }
}
