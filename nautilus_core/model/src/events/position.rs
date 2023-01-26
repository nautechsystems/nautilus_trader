// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::enums::{OrderSide, PositionSide};
use crate::identifiers::account_id::AccountId;
use crate::identifiers::client_order_id::ClientOrderId;
use crate::identifiers::instrument_id::InstrumentId;
use crate::identifiers::position_id::PositionId;
use crate::identifiers::strategy_id::StrategyId;
use crate::identifiers::trader_id::TraderId;
use crate::types::currency::Currency;
use crate::types::money::Money;
use crate::types::price::Price;
use crate::types::quantity::Quantity;
use nautilus_core::time::{TimedeltaNanos, UnixNanos};

pub enum PositionEvent {
    PositionOpened(PositionOpened),
    PositionChanged(PositionChanged),
    PositionClosed(PositionClosed),
}

#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionOpened {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub position_id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub net_qty: f64,
    pub quantity: Quantity,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub avg_px_open: f64,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

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
    pub net_qty: f64,
    pub quantity: Quantity,
    pub peak_quantity: Quantity,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub avg_px_open: f64,
    pub avg_px_closed: f64,
    pub realized_return: f64,
    pub realized_pnl: Money,
    pub unrealized_pnl: Money,
    pub ts_opened: UnixNanos,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

#[repr(C)]
#[derive(Clone, PartialEq, Debug)]
pub struct PositionClosed {
    pub trader_id: TraderId,
    pub strategy_id: StrategyId,
    pub instrument_id: InstrumentId,
    pub position_id: PositionId,
    pub account_id: AccountId,
    pub opening_order_id: ClientOrderId,
    pub closing_order_id: ClientOrderId,
    pub entry: OrderSide,
    pub side: PositionSide,
    pub net_qty: f64,
    pub quantity: Quantity,
    pub peak_quantity: Quantity,
    pub last_qty: Quantity,
    pub last_px: Price,
    pub currency: Currency,
    pub avg_px_open: f64,
    pub avg_px_closed: f64,
    pub realized_return: f64,
    pub realized_pnl: Money,
    pub unrealized_pnl: Money,
    pub duration: TimedeltaNanos,
    pub ts_opened: UnixNanos,
    pub ts_closed: UnixNanos,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}
