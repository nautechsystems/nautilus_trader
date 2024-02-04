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

use nautilus_common::factories::OrderFactory;
use nautilus_common::stubs::*;
use nautilus_model::enums::OrderSide;
use nautilus_model::identifiers::instrument_id::InstrumentId;
use nautilus_model::instruments::currency_pair::CurrencyPair;
use nautilus_model::instruments::stubs::audusd_sim;
use nautilus_model::orders::market::MarketOrder;
use nautilus_model::orders::stubs::TestOrderEventStubs;
use nautilus_model::position::Position;
use nautilus_model::types::price::Price;
use nautilus_model::types::quantity::Quantity;
use rstest::fixture;

#[fixture]
pub fn test_position_long(mut order_factory: OrderFactory, audusd_sim: CurrencyPair) -> Position {
    let order = order_factory.market(
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Buy,
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let order_filled = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
        &order,
        &audusd_sim,
        None,
        None,
        None,
        Some(Price::from("1.0002")),
        None,
        None,
        None,
    );
    Position::new(audusd_sim, order_filled).unwrap()
}

#[fixture]
pub fn test_position_short(mut order_factory: OrderFactory, audusd_sim: CurrencyPair) -> Position {
    let order = order_factory.market(
        InstrumentId::from("AUD/USD.SIM"),
        OrderSide::Sell,
        Quantity::from(1),
        None,
        None,
        None,
        None,
        None,
        None,
    );
    let order_filled = TestOrderEventStubs::order_filled::<MarketOrder, CurrencyPair>(
        &order,
        &audusd_sim,
        None,
        None,
        None,
        Some(Price::from("22000.0")),
        None,
        None,
        None,
    );
    Position::new(audusd_sim, order_filled).unwrap()
}
