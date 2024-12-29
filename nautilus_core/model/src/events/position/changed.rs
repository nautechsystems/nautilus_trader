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

use nautilus_core::{nanos::UnixNanos, uuid::UUID4};

use crate::{
    enums::{OrderSide, PositionSide},
    events::OrderFilled,
    identifiers::{AccountId, ClientOrderId, InstrumentId, PositionId, StrategyId, TraderId},
    position::Position,
    types::{Currency, Money, Price, Quantity},
};

#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionChanged {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub position_id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub signed_qty: f64,
    pub quantity: Quantity,
    pub peak_quantity: Quantity,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub avg_px_open: f64,
    pub avg_px_close: Option<f64>,
    pub realized_return: f64,
    pub realized_pnl: Option<Money>,
    pub unrealized_pnl: Money,
    pub event_id: UUID4,
    pub ts_opened: UnixNanos,
    pub ts_event: UnixNanos,
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
