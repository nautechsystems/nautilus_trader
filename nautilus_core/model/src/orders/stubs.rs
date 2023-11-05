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

use nautilus_core::uuid::UUID4;

use crate::{
    enums::{OrderSide, TimeInForce},
    identifiers::stubs::*,
    orders::market::MarketOrder,
    types::quantity::Quantity,
};

// ---- MarketOrder ----
pub fn market_order(quantity: Quantity, time_in_force: Option<TimeInForce>) -> MarketOrder {
    let trader = trader_test();
    let strategy = strategy_id_ema_cross();
    let instrument = instrument_id_eth_usdt_binance();
    let client_order_id = client_order_id();
    MarketOrder::new(
        trader,
        strategy,
        instrument,
        client_order_id,
        OrderSide::Buy,
        quantity,
        time_in_force.unwrap_or(TimeInForce::Gtc),
        UUID4::new(),
        12321312321312,
        false,
        false,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
    )
    .unwrap()
}
