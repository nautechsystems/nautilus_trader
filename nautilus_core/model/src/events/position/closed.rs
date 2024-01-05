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

use nautilus_core::time::{TimedeltaNanos, UnixNanos};

use crate::{
    enums::{OrderSide, PositionSide},
    identifiers::{
        account_id::AccountId, client_order_id::ClientOrderId, instrument_id::InstrumentId,
        position_id::PositionId, strategy_id::StrategyId, trader_id::TraderId,
    },
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
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
    pub signed_qty: f64,
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
