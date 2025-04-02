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

//! Type stubs to facilitate testing.

use rstest::fixture;
use rust_decimal::prelude::ToPrimitive;

use crate::{
    data::order::BookOrder,
    enums::{BookType, LiquiditySide, OrderSide, OrderType},
    identifiers::InstrumentId,
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::audusd_sim},
    orderbook::OrderBook,
    orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
    position::Position,
    types::{Money, Price, Quantity},
};

/// Calculate commission for testing
pub fn calculate_commission(
    instrument: &InstrumentAny,
    last_qty: Quantity,
    last_px: Price,
    use_quote_for_inverse: Option<bool>,
) -> Money {
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
        panic!("Invalid liquidity side {liquidity_side}")
    };
    if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
        Money::new(commission, instrument.base_currency().unwrap())
    } else {
        Money::new(commission, instrument.quote_currency())
    }
}

#[fixture]
pub fn stub_position_long(audusd_sim: CurrencyPair) -> Position {
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Buy)
        .quantity(Quantity::from(1))
        .build();
    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        None,
        Some(Price::from("1.0002")),
        None,
        None,
        None,
        None,
        None,
    );
    Position::new(&audusd_sim, filled.into())
}

#[fixture]
pub fn stub_position_short(audusd_sim: CurrencyPair) -> Position {
    let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
    let order = OrderTestBuilder::new(OrderType::Market)
        .instrument_id(audusd_sim.id())
        .side(OrderSide::Sell)
        .quantity(Quantity::from(1))
        .build();
    let filled = TestOrderEventStubs::filled(
        &order,
        &audusd_sim,
        None,
        None,
        Some(Price::from("22000.0")),
        None,
        None,
        None,
        None,
        None,
    );
    Position::new(&audusd_sim, filled.into())
}

#[must_use]
pub fn stub_order_book_mbp_appl_xnas() -> OrderBook {
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
) -> OrderBook {
    let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

    // Generate bids
    for i in 0..num_levels {
        let price = Price::new(
            price_increment.mul_add(-(i as f64), top_bid_price),
            price_precision,
        );
        let size = Quantity::new(
            size_increment.mul_add(i as f64, top_bid_size),
            size_precision,
        );
        let order = BookOrder::new(
            OrderSide::Buy,
            price,
            size,
            0, // order_id not applicable for MBP (market by price) books
        );
        book.add(order, 0, 1, 2.into());
    }

    // Generate asks
    for i in 0..num_levels {
        let price = Price::new(
            price_increment.mul_add(i as f64, top_ask_price),
            price_precision,
        );
        let size = Quantity::new(
            size_increment.mul_add(i as f64, top_ask_size),
            size_precision,
        );
        let order = BookOrder::new(
            OrderSide::Sell,
            price,
            size,
            0, // order_id not applicable for MBP (market by price) books
        );
        book.add(order, 0, 1, 2.into());
    }

    book
}
