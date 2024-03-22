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

use rstest::fixture;
use rust_decimal::prelude::ToPrimitive;

use crate::{
    data::order::BookOrder,
    enums::{LiquiditySide, OrderSide},
    identifiers::instrument_id::InstrumentId,
    instruments::{currency_pair::CurrencyPair, stubs::audusd_sim, Instrument},
    orderbook::book_mbp::OrderBookMbp,
    orders::{
        market::MarketOrder,
        stubs::{TestOrderEventStubs, TestOrderStubs},
    },
    position::Position,
    types::{money::Money, price::Price, quantity::Quantity},
};

/// Calculate commission for testing
pub fn calculate_commission<T: Instrument>(
    instrument: T,
    last_qty: Quantity,
    last_px: Price,
    use_quote_for_inverse: Option<bool>,
) -> anyhow::Result<Money> {
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

#[must_use]
pub fn stub_order_book_mbp_appl_xnas() -> OrderBookMbp {
    stub_order_book_mbp(
        InstrumentId::from("AAPL.XNAS"),
        101.0,
        100.0,
        100.0,
        100.0,
        2,
        0.01,
        0,
        100.0,
        10,
    )
}

#[allow(clippy::too_many_arguments)]
#[must_use]
pub fn stub_order_book_mbp(
    instrument_id: InstrumentId,
    top_ask_price: f64,
    top_bid_price: f64,
    top_ask_size: f64,
    top_bid_size: f64,
    price_precision: u8,
    price_increment: f64,
    size_precision: u8,
    size_increment: f64,
    num_levels: usize,
) -> OrderBookMbp {
    let mut book = OrderBookMbp::new(instrument_id, false);

    // Generate bids
    for i in 0..num_levels {
        let price = Price::new(
            price_increment.mul_add(-(i as f64), top_bid_price),
            price_precision,
        )
        .unwrap();
        let size = Quantity::new(
            size_increment.mul_add(i as f64, top_bid_size),
            size_precision,
        )
        .unwrap();
        let order = BookOrder::new(
            OrderSide::Buy,
            price,
            size,
            0, // order_id not applicable for MBP (market by price) books
        );
        book.add(order, 0, 1);
    }

    // Generate asks
    for i in 0..num_levels {
        let price = Price::new(
            price_increment.mul_add(i as f64, top_ask_price),
            price_precision,
        )
        .unwrap();
        let size = Quantity::new(
            size_increment.mul_add(i as f64, top_ask_size),
            size_precision,
        )
        .unwrap();
        let order = BookOrder::new(
            OrderSide::Sell,
            price,
            size,
            0, // order_id not applicable for MBP (market by price) books
        );
        book.add(order, 0, 1);
    }

    book
}
