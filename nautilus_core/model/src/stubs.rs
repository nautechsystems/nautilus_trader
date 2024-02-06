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

use crate::enums::{LiquiditySide, OrderSide};
use crate::instruments::currency_pair::CurrencyPair;
use crate::instruments::stubs::*;
use crate::instruments::Instrument;
use crate::orders::market::MarketOrder;
use crate::orders::stubs::{TestOrderEventStubs, TestOrderStubs};
use crate::position::Position;
use crate::types::money::Money;
use crate::types::price::Price;
use crate::types::quantity::Quantity;
use anyhow::Result;
use rstest::fixture;
use rust_decimal::prelude::ToPrimitive;

/// Calculate commission for testing
pub fn calculate_commission<T: Instrument>(
    instrument: T,
    last_qty: Quantity,
    last_px: Price,
    use_quote_for_inverse: Option<bool>,
) -> Result<Money> {
    let liquidity_side = LiquiditySide::Taker;
    assert_ne!(
        liquidity_side,
        LiquiditySide::NoLiquiditySide,
        "Invalid liquidity side"
    );
    let notional = instrument
        .calculate_notional_value(last_qty, last_px, use_quote_for_inverse)
        .as_f64();
    let commission = if liquidity_side == LiquiditySide::Maker {
        notional * instrument.maker_fee().to_f64().unwrap()
    } else if liquidity_side == LiquiditySide::Taker {
        notional * instrument.taker_fee().to_f64().unwrap()
    } else {
        panic!("Invalid liquid side {liquidity_side}")
    };
    if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
        Ok(Money::new(commission, instrument.base_currency().unwrap()).unwrap())
    } else {
        Ok(Money::new(commission, instrument.quote_currency()).unwrap())
    }
}

#[fixture]
pub fn test_position_long(audusd_sim: CurrencyPair) -> Position {
    let order =
        TestOrderStubs::market_order(audusd_sim.id, OrderSide::Buy, Quantity::from(1), None, None);
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
pub fn test_position_short(audusd_sim: CurrencyPair) -> Position {
    let order = TestOrderStubs::market_order(
        audusd_sim.id,
        OrderSide::Sell,
        Quantity::from(1),
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
