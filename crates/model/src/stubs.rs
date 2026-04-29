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
    orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
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
    use rstest::rstest;

    use super::*;

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
}
