// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::cell::Cell;

use nautilus_core::UUID4;
use rstest::fixture;
use rust_decimal::prelude::ToPrimitive;

use crate::{
    data::order::BookOrder,
    enums::{BookType, LiquiditySide, OrderSide, OrderType},
    identifiers::InstrumentId,
    instruments::{CurrencyPair, Instrument, InstrumentAny, stubs::audusd_sim},
    orderbook::OrderBook,
    orders::{builder::OrderTestBuilder, stubs::OrderFilledTestBuilder},
    position::Position,
    types::{Money, Price, Quantity},
};

/// Seed used by [`test_uuid`] for deterministic UUIDs in test fixtures.
pub(crate) const TEST_UUID_SEED: u64 = 42;

thread_local! {
    static TEST_UUID_STATE: Cell<u64> = const { Cell::new(TEST_UUID_SEED) };
}

// SplitMix64 PRNG (Steele, Lea, Flood 2014): owning the algorithm here keeps the test UUID
// sequence stable regardless of upstream PRNG crate versions, with zero added dependencies.
fn splitmix64(state: &mut u64) -> u64 {
    *state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = *state;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// Returns the next [`UUID4`] in a per-thread deterministic sequence seeded with a fixed value.
///
/// The official test runner is `cargo nextest`, which spawns one process per test, so the
/// sequence resets at every test boundary without explicit teardown. Multiple events constructed
/// within a single test get distinct UUIDs, and re-running the same test produces the same
/// sequence.
///
/// Intended for use as a default in test specs and fixtures only.
#[must_use]
pub fn test_uuid() -> UUID4 {
    TEST_UUID_STATE.with(|cell| {
        let mut state = cell.get();
        let hi = splitmix64(&mut state).to_be_bytes();
        let lo = splitmix64(&mut state).to_be_bytes();
        cell.set(state);

        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&hi);
        bytes[8..].copy_from_slice(&lo);
        UUID4::from_bytes(bytes)
    })
}

/// Resets the per-thread test UUID state to its seed.
///
/// Only needed under runners that share a process across tests (e.g. plain `cargo test`); under
/// nextest each test starts with fresh thread-local state already.
pub fn reset_test_uuid_rng() {
    TEST_UUID_STATE.with(|cell| cell.set(TEST_UUID_SEED));
}

/// A trait for providing test-only default values.
///
/// This trait is intentionally separate from [`Default`] to make it clear
/// that these default values are only meaningful in testing contexts and should
/// not be used in production code.
pub trait TestDefault {
    /// Creates a new instance with test-appropriate default values.
    fn test_default() -> Self;
}

/// Calculate commission for testing.
///
/// # Panics
///
/// This function panics if:
/// - The liquidity side is `NoLiquiditySide`.
/// - `instrument.maker_fee()` or `instrument.taker_fee()` cannot be converted to `f64`.
#[must_use]
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
    let filled = OrderFilledTestBuilder::new(&order, &audusd_sim)
        .last_px(Price::from("1.0002"))
        .build();
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
    let filled = OrderFilledTestBuilder::new(&order, &audusd_sim)
        .last_px(Price::from("22000.0"))
        .build();
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

#[expect(clippy::too_many_arguments)]
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        instruments::{
            CryptoPerpetual,
            stubs::{crypto_perpetual_ethusdt, xbtusd_bitmex},
        },
        orderbook::BookLevel,
        types::Currency,
    };

    #[rstest]
    fn test_uuid_is_valid_v4_rfc4122() {
        reset_test_uuid_rng();
        let s = test_uuid().to_string();
        // Format invariants per RFC 4122: position 14 is the version digit, position 19 the variant.
        assert_eq!(s.len(), 36);
        assert_eq!(&s[14..15], "4", "version digit must be 4, was {s}");
        let variant = s.chars().nth(19).unwrap();
        assert!(
            matches!(variant, '8' | '9' | 'a' | 'b'),
            "variant nibble must be one of 8/9/a/b, was {variant} in {s}",
        );
    }

    #[rstest]
    fn test_uuid_sequence_is_deterministic_and_distinct() {
        reset_test_uuid_rng();
        let first: Vec<UUID4> = (0..8).map(|_| test_uuid()).collect();
        reset_test_uuid_rng();
        let second: Vec<UUID4> = (0..8).map(|_| test_uuid()).collect();

        assert_eq!(first, second, "the same seed must replay the same sequence");
        assert_eq!(
            first.iter().collect::<HashSet<_>>().len(),
            first.len(),
            "each call must yield a distinct UUID",
        );
    }

    #[rstest]
    fn test_uuid_advances_without_reset() {
        reset_test_uuid_rng();
        let first = test_uuid();
        let second = test_uuid();

        assert_ne!(first, second);

        reset_test_uuid_rng();

        assert_eq!(
            test_uuid(),
            first,
            "reset must return to the seeded sequence"
        );
    }

    #[rstest]
    fn test_calculate_commission_applies_taker_fee_in_quote_currency(
        crypto_perpetual_ethusdt: CryptoPerpetual,
    ) {
        // ETHUSDT-PERP has distinct fees (maker 0.0002, taker 0.0004), so a maker/taker swap
        // changes the result: 10 @ 2000.00 = 20,000 notional -> 8.00 USDT taker, 4.00 maker.
        let instrument = InstrumentAny::CryptoPerpetual(crypto_perpetual_ethusdt);

        let commission = calculate_commission(
            &instrument,
            Quantity::from("10.000"),
            Price::from("2000.00"),
            None,
        );

        assert_eq!(commission, Money::new(8.0, Currency::from("USDT")));
        assert_eq!(commission.currency, instrument.quote_currency());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(false))]
    fn test_calculate_commission_charges_inverse_instruments_in_base_currency(
        xbtusd_bitmex: CryptoPerpetual,
        #[case] use_quote_for_inverse: Option<bool>,
    ) {
        // Inverse: 100,000 USD @ 50,000.00 = 2 BTC notional, taker 0.00075 -> 0.0015 BTC.
        // `Some(false)` must behave like `None`, not like `Some(true)`.
        let instrument = InstrumentAny::CryptoPerpetual(xbtusd_bitmex);

        let commission = calculate_commission(
            &instrument,
            Quantity::from(100_000),
            Price::from("50000.0"),
            use_quote_for_inverse,
        );

        assert_eq!(commission, Money::new(0.0015, Currency::BTC()));
        assert_eq!(commission.currency, instrument.base_currency().unwrap());
    }

    #[rstest]
    fn test_calculate_commission_uses_quote_currency_when_requested_for_inverse(
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let instrument = InstrumentAny::CryptoPerpetual(xbtusd_bitmex);

        let commission = calculate_commission(
            &instrument,
            Quantity::from(100_000),
            Price::from("50000.0"),
            Some(true),
        );

        assert_eq!(commission, Money::new(75.0, Currency::USD()));
        assert_eq!(commission.currency, instrument.quote_currency());
    }

    #[rstest]
    fn test_stub_order_book_mbp_appl_xnas_levels() {
        let book = stub_order_book_mbp_appl_xnas();

        assert_eq!(book.instrument_id, InstrumentId::from("AAPL.XNAS"));
        assert_eq!(book.book_type, BookType::L2_MBP);
        assert_eq!(book.bids(None).count(), 10);
        assert_eq!(book.asks(None).count(), 10);
        assert_eq!(book.best_bid_price(), Some(Price::new(100.0, 2)));
        assert_eq!(book.best_ask_price(), Some(Price::new(101.0, 2)));
        assert_eq!(book.best_bid_size(), Some(Quantity::new(100.0, 0)));
        assert_eq!(book.best_ask_size(), Some(Quantity::new(100.0, 0)));
        // `Price` and `Quantity` compare on raw value alone, so precision needs its own assertion.
        assert_eq!(book.best_bid_price().unwrap().precision, 2);
        assert_eq!(book.best_ask_price().unwrap().precision, 2);
        assert_eq!(book.best_bid_size().unwrap().precision, 0);
        assert_eq!(book.best_ask_size().unwrap().precision, 0);

        // Assert past the touch so the wrapper's own increments are pinned, not just its top level.
        let bids: Vec<&BookLevel> = book.bids(None).collect();
        let asks: Vec<&BookLevel> = book.asks(None).collect();

        assert_eq!(bids[1].price.value.as_decimal(), dec!(99.99));
        assert_eq!(asks[1].price.value.as_decimal(), dec!(101.01));
        assert_eq!(
            bids[1]
                .first()
                .expect("level must hold an order")
                .size
                .as_decimal(),
            dec!(200),
        );
        assert_eq!(
            asks[1]
                .first()
                .expect("level must hold an order")
                .size
                .as_decimal(),
            dec!(200),
        );
    }

    #[rstest]
    fn test_stub_order_book_mbp_walks_prices_away_from_the_touch() {
        // Every argument differs from `stub_order_book_mbp_appl_xnas`, so a hardcoded precision,
        // increment, or touch price inside the builder fails here even if the wrapper test passes.
        let book = stub_order_book_mbp(
            InstrumentId::from("ESH5.XCME"),
            200.500,
            200.000,
            7.5,
            4.5,
            3,
            0.005,
            1,
            2.5,
            3,
        );

        assert_eq!(book.instrument_id, InstrumentId::from("ESH5.XCME"));

        let bids: Vec<&BookLevel> = book.bids(None).collect();
        let asks: Vec<&BookLevel> = book.asks(None).collect();

        // `Price` and `Quantity` compare on raw value alone, so precision needs its own assertion.
        assert_eq!(
            bids.iter()
                .map(|l| l.price.value.as_decimal())
                .collect::<Vec<_>>(),
            vec![dec!(200.000), dec!(199.995), dec!(199.990)],
        );
        assert_eq!(
            asks.iter()
                .map(|l| l.price.value.as_decimal())
                .collect::<Vec<_>>(),
            vec![dec!(200.500), dec!(200.505), dec!(200.510)],
        );
        assert_eq!(
            bids.iter()
                .map(|l| l.price.value.precision)
                .collect::<Vec<_>>(),
            vec![3, 3, 3],
        );
        assert_eq!(
            asks.iter()
                .map(|l| l.price.value.precision)
                .collect::<Vec<_>>(),
            vec![3, 3, 3],
        );

        let bid_sizes: Vec<Quantity> = bids
            .iter()
            .map(|l| l.first().expect("level must hold an order").size)
            .collect();
        let ask_sizes: Vec<Quantity> = asks
            .iter()
            .map(|l| l.first().expect("level must hold an order").size)
            .collect();

        assert_eq!(
            bid_sizes
                .iter()
                .map(Quantity::as_decimal)
                .collect::<Vec<_>>(),
            vec![dec!(4.5), dec!(7.0), dec!(9.5)],
        );
        assert_eq!(
            ask_sizes
                .iter()
                .map(Quantity::as_decimal)
                .collect::<Vec<_>>(),
            vec![dec!(7.5), dec!(10.0), dec!(12.5)],
        );
        assert_eq!(
            bid_sizes.iter().map(|q| q.precision).collect::<Vec<_>>(),
            vec![1, 1, 1],
        );
        assert_eq!(
            ask_sizes.iter().map(|q| q.precision).collect::<Vec<_>>(),
            vec![1, 1, 1],
        );
    }
}
