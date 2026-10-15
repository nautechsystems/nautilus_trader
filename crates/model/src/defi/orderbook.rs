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

//! Order book depth analysis for native DeFi quantity scales.

use std::collections::BTreeMap;

use alloy_primitives::{I256, U256};
use rust_decimal::Decimal;

use crate::{
    defi::{WEI_PRECISION, tick_map::full_math::FullMath},
    orderbook::{BookLevel, BookPrice},
    types::{
        Price, Quantity,
        fixed::{FIXED_PRECISION, raw_scale},
        quantity::QuantityRaw,
    },
};

const DEPTH_SCALE_FACTORS: [u128; 3] = [1, 10, 100];

/// Calculates the estimated average price for a specified quantity from a set of
/// order book levels.
///
/// # Panics
///
/// Panics if the calculated average price cannot be parsed as an `f64`.
#[must_use]
pub fn get_avg_px_for_quantity(qty: Quantity, levels: &BTreeMap<BookPrice, BookLevel>) -> f64 {
    let precision = WEI_PRECISION;
    let target_size_raw = scaled_quantity_raw(qty, precision);
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut cumulative_value = I256::ZERO;

    for (book_price, level) in levels {
        let size_this_level =
            scaled_level_size_raw(level, precision).min(target_size_raw - cumulative_size_raw);
        cumulative_size_raw += size_this_level;
        let price_raw = book_price.value.raw
            * DEPTH_SCALE_FACTORS
                [usize::from(precision - book_price.value.precision.max(FIXED_PRECISION))]
                as i128;
        cumulative_value += I256::try_from(price_raw)
            .expect("Normalized raw price must fit in I256")
            * I256::from_raw(U256::from(size_this_level));

        if cumulative_size_raw >= target_size_raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        0.0
    } else {
        depth_average_as_f64(cumulative_value, cumulative_size_raw, precision)
    }
}

/// Calculates the worst (last-touched) price while filling a specified quantity
/// from order book levels.
///
/// For buy-side traversal this is the highest ask touched; for sell-side traversal
/// this is the lowest bid touched. Returns `None` when no quantity can be matched.
#[must_use]
pub fn get_worst_px_for_quantity(
    qty: Quantity,
    levels: &BTreeMap<BookPrice, BookLevel>,
) -> Option<Price> {
    let precision = WEI_PRECISION;
    let target_size_raw = scaled_quantity_raw(qty, precision);
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut worst_price: Option<Price> = None;

    for (book_price, level) in levels {
        let size_this_level =
            scaled_level_size_raw(level, precision).min(target_size_raw - cumulative_size_raw);

        if size_this_level == 0 {
            continue;
        }

        cumulative_size_raw += size_this_level;
        worst_price = Some(book_price.value);

        if cumulative_size_raw >= target_size_raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        None
    } else {
        worst_price
    }
}

/// Calculates the estimated average price for a specified exposure from a set of
/// order book levels.
#[must_use]
pub fn get_avg_px_qty_for_exposure(
    target_exposure: Quantity,
    levels: &BTreeMap<BookPrice, BookLevel>,
) -> (f64, f64, f64) {
    // Use the book's scale to preserve its fill quantum and exposure rounding
    let precision = depth_precision(levels);
    let mut cumulative_exposure = 0.0;
    let mut cumulative_size_raw: QuantityRaw = 0;
    let mut final_price = levels
        .first_key_value()
        .map_or(0.0, |(price, _)| price.value.as_f64());

    let target_exposure_raw = exposure_raw_as_f64(target_exposure, precision);

    for (book_price, level) in levels {
        let price = book_price.value.as_f64();

        if price == 0.0 {
            continue;
        }

        let level_size_raw = scaled_level_size_raw(level, precision);
        let level_exposure = price * level_size_raw as f64;
        let exposure_this_level = level_exposure.min(target_exposure_raw - cumulative_exposure);
        let size_this_level = (exposure_this_level / price).floor() as QuantityRaw;

        if size_this_level == 0 {
            continue;
        }

        final_price = price;
        cumulative_exposure += price * size_this_level as f64;
        cumulative_size_raw += size_this_level;

        if cumulative_exposure >= target_exposure_raw {
            break;
        }
    }

    if cumulative_size_raw == 0 {
        (0.0, 0.0, final_price)
    } else {
        let size_raw = cumulative_size_raw as f64;
        let avg_price = cumulative_exposure / size_raw;
        (
            avg_price,
            size_raw / raw_scale(precision) as f64,
            final_price,
        )
    }
}

fn depth_average_as_f64(value: I256, size_raw: QuantityRaw, precision: u8) -> f64 {
    if value.is_zero() {
        return 0.0;
    }

    let mut numerator = value.unsigned_abs();
    let mut denominator = U256::from(size_raw) * U256::from(raw_scale(precision));
    let mut exponent = 0_i32;
    let ten = U256::from(10);
    while numerator < denominator {
        numerator *= ten;
        exponent -= 1;
    }

    while numerator >= denominator * ten {
        denominator *= ten;
        exponent += 1;
    }

    // Keep significant digits even when the average is below Decimal's smallest nonzero value
    let scale = Decimal::MAX_SCALE - 1;
    let mantissa = FullMath::mul_div(numerator, U256::from(10_u128.pow(scale)), denominator)
        .expect("Normalized average price mantissa must fit in U256");

    let sign = if value.is_negative() { "-" } else { "" };
    format!("{sign}{mantissa}e{}", exponent - scale as i32)
        .parse::<f64>()
        .expect("Average price must parse as f64")
}

fn depth_precision(levels: &BTreeMap<BookPrice, BookLevel>) -> u8 {
    levels
        .values()
        .flat_map(BookLevel::iter)
        .map(|order| order.size.precision)
        .max()
        .unwrap_or(FIXED_PRECISION)
        .max(FIXED_PRECISION)
}

fn scaled_quantity_raw(qty: Quantity, precision: u8) -> QuantityRaw {
    let precision_diff = precision - qty.precision.max(FIXED_PRECISION);
    if precision_diff == 0 {
        return qty.raw();
    }

    qty.raw() * DEPTH_SCALE_FACTORS[usize::from(precision_diff)]
}

fn scaled_level_size_raw(level: &BookLevel, precision: u8) -> QuantityRaw {
    level
        .iter()
        .try_fold(0_u128, |total, order| {
            total.checked_add(scaled_quantity_raw(order.size, precision))
        })
        .expect("Overflow occurred when summing scaled order book size")
}

fn exposure_raw_as_f64(exposure: Quantity, precision: u8) -> f64 {
    if exposure.precision > precision {
        let factor = raw_scale(exposure.precision) / raw_scale(precision);
        let raw = exposure.raw();
        return (raw / factor) as f64 + (raw % factor) as f64 / factor as f64;
    }

    scaled_quantity_raw(exposure, precision) as f64
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        data::order::BookOrder,
        enums::{BookType, OrderSide},
        identifiers::InstrumentId,
        orderbook::OrderBook,
        types::quantity::QUANTITY_RAW_MAX,
    };

    #[rstest]
    #[case(2, 18, 18)]
    #[case(18, 2, 2)]
    #[case(2, 17, 17)]
    #[case(17, 2, 2)]
    #[case(2, 2, 18)]
    #[case(18, 18, 2)]
    #[case(17, 18, 17)]
    fn test_book_depth_normalizes_quantity_scales(
        #[case] query_precision: u8,
        #[case] first_precision: u8,
        #[case] second_precision: u8,
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
        #[values(false, true)] mixed_orders: bool,
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L3_MBO);
        let order_side = side.opposite();

        let (first_price, second_price) = match side {
            OrderSide::Buy => (100, 200),
            OrderSide::Sell => (200, 100),
        };

        for (level, price, precision) in [
            (0, first_price, first_precision),
            (1, second_price, second_precision),
        ] {
            let sizes = if mixed_orders {
                [
                    (dec!(0.25), first_precision),
                    (dec!(0.75), second_precision),
                ]
            } else {
                [(dec!(0.25), precision), (dec!(0.75), precision)]
            };

            for (order, (size, size_precision)) in sizes.into_iter().enumerate() {
                book.add(
                    BookOrder::new(
                        order_side,
                        Price::from(price.to_string()),
                        Quantity::from_decimal_dp(size, size_precision).unwrap(),
                        level * 2 + order as u64,
                    ),
                    0,
                    0,
                    0.into(),
                );
            }
        }

        let query = Quantity::from_decimal_dp(dec!(1.5), query_precision).unwrap();
        let exposure = Quantity::from_decimal_dp(
            Decimal::from(first_price + second_price / 2),
            query_precision,
        )
        .unwrap();
        let expected_average = f64::from(first_price + second_price / 2) / 1.5;

        assert_eq!(book.get_avg_px_for_quantity(query, side), expected_average);
        assert_eq!(
            book.get_worst_px_for_quantity(query, side),
            Some(Price::from(second_price.to_string()))
        );
        assert_eq!(
            book.get_avg_px_qty_for_exposure(exposure, side),
            (expected_average, 1.5, f64::from(second_price))
        );
    }

    #[rstest]
    #[case(2, 2)]
    #[case(2, 17)]
    #[case(2, 18)]
    #[case(17, 2)]
    #[case(18, 2)]
    fn test_book_exposure_preserves_book_scale_rounding(
        #[case] size_precision: u8,
        #[case] target_precision: u8,
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        book.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from("99"),
                Quantity::from_decimal_dp(dec!(1), size_precision).unwrap(),
                1,
            ),
            0,
            0,
            0.into(),
        );
        let target = Quantity::from_decimal_dp(dec!(1), target_precision).unwrap();

        let expected = if size_precision <= FIXED_PRECISION {
            (99.000_000_000_000_01, 0.010_101_010_101_010_1, 99.0)
        } else if size_precision == 17 {
            (99.0, 0.010_101_010_101_010_1, 99.0)
        } else {
            (99.0, 0.010_101_010_101_010_102, 99.0)
        };

        assert_eq!(
            book.get_avg_px_qty_for_exposure(target, OrderSide::Buy),
            expected
        );
    }

    #[rstest]
    fn test_book_depth_preserves_native_units() {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        for (id, price) in [(1, 100), (2, 200)] {
            book.add(
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from(price.to_string()),
                    Quantity::from_raw(1, 18),
                    id,
                ),
                0,
                0,
                0.into(),
            );
        }

        assert_eq!(
            book.get_avg_px_for_quantity(Quantity::from(1), OrderSide::Buy),
            150.0
        );
        assert_eq!(
            book.get_worst_px_for_quantity(Quantity::from(1), OrderSide::Buy),
            Some(Price::from("200"))
        );
        assert_eq!(
            book.get_avg_px_qty_for_exposure(Quantity::from(1), OrderSide::Buy),
            (150.0, 2e-18, 200.0)
        );
    }

    #[rstest]
    #[case("1", "2", 1.9)]
    #[case("10000000000000", "11000000000000", 10_900_000_000_000.0)]
    fn test_book_depth_normalizes_max_raw_sizes(
        #[case] first_price: &str,
        #[case] second_price: &str,
        #[case] expected_average: f64,
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        book.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from(first_price),
                Quantity::from_raw(QUANTITY_RAW_MAX, 18),
                1,
            ),
            0,
            0,
            0.into(),
        );
        book.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from(second_price),
                Quantity::from_raw(QUANTITY_RAW_MAX, FIXED_PRECISION),
                2,
            ),
            0,
            0,
            0.into(),
        );
        let query = Quantity::from_raw(QUANTITY_RAW_MAX / 10, FIXED_PRECISION);

        assert_eq!(
            book.get_avg_px_for_quantity(query, OrderSide::Buy),
            expected_average
        );
        assert_eq!(
            book.get_worst_px_for_quantity(query, OrderSide::Buy),
            Some(Price::from(second_price))
        );
    }

    #[rstest]
    #[case("0.00000000001", 1e-11)]
    #[case("0.0000000000000001", 1e-16)]
    #[case("-0.00000000001", -1e-11)]
    #[case("0.000000000000000001", 1e-18)]
    fn test_book_depth_native_query_preserves_small_prices(
        #[case] price: &str,
        #[case] expected: f64,
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        book.add(
            BookOrder::new(OrderSide::Sell, Price::from(price), Quantity::from(1), 1),
            0,
            0,
            0.into(),
        );

        assert_eq!(
            book.get_avg_px_for_quantity(Quantity::from_raw(1, 18), OrderSide::Buy),
            expected
        );
    }

    #[rstest]
    fn test_book_depth_average_preserves_sub_price_units() {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        for (id, price, size) in [
            (1, "0", Quantity::from(1)),
            (2, "0.0000000000000001", Quantity::from_raw(1, 18)),
        ] {
            book.add(
                BookOrder::new(OrderSide::Sell, Price::from(price), size, id),
                0,
                0,
                0.into(),
            );
        }

        assert_eq!(
            book.get_avg_px_for_quantity(Quantity::from(2), OrderSide::Buy),
            1e-34
        );
    }

    #[rstest]
    #[case(0, 0.0, None)]
    #[case(1, 100.0, Some(Price::from("100")))]
    fn test_book_depth_native_query_against_fixed_sizes(
        #[case] query_raw: QuantityRaw,
        #[case] expected_average: f64,
        #[case] expected_worst: Option<Price>,
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        book.add(
            BookOrder::new(OrderSide::Sell, Price::from("100"), Quantity::from(1), 1),
            0,
            0,
            0.into(),
        );
        let query = Quantity::from_raw(query_raw, 18);

        assert_eq!(
            book.get_avg_px_for_quantity(query, OrderSide::Buy),
            expected_average
        );
        assert_eq!(
            book.get_worst_px_for_quantity(query, OrderSide::Buy),
            expected_worst
        );
    }

    #[rstest]
    #[case(199, (0.0, 0.0, 3.0))]
    #[case(300, (3.0, 1e-16, 3.0))]
    fn test_book_exposure_finer_target_preserves_fill_quantum(
        #[case] target_raw: QuantityRaw,
        #[case] expected: (f64, f64, f64),
    ) {
        let mut book = OrderBook::new(InstrumentId::from("ETHUSDT-PERP.BINANCE"), BookType::L2_MBP);
        book.add(
            BookOrder::new(OrderSide::Sell, Price::from("3"), Quantity::from(1), 1),
            0,
            0,
            0.into(),
        );
        let target = Quantity::from_raw(target_raw, 18);

        assert_eq!(
            book.get_avg_px_qty_for_exposure(target, OrderSide::Buy),
            expected
        );
    }

    #[rstest]
    fn test_book_worst_price_preserves_mixed_scale_traversal() {
        let mut book = OrderBook::new(InstrumentId::from("AAPL.XNAS"), BookType::L2_MBP);
        let price = Price::from("1.00");
        book.add(
            BookOrder::new(OrderSide::Sell, price, Quantity::from_raw(1, 18), 1),
            0,
            0,
            0.into(),
        );
        assert_eq!(
            book.get_worst_px_for_quantity(Quantity::from(2), OrderSide::Buy),
            Some(price)
        );
    }

    #[rstest]
    fn test_book_exposure_accepts_native_scale_quantities() {
        let mut book = OrderBook::new(InstrumentId::from("AAPL.XNAS"), BookType::L2_MBP);
        let target = Quantity::from_raw(1_000_000_000_000_000_000, 18);
        assert_eq!(
            book.get_avg_px_qty_for_exposure(target, OrderSide::Buy),
            (0.0, 0.0, 0.0)
        );
        book.add(
            BookOrder::new(
                OrderSide::Sell,
                Price::from("1.00"),
                Quantity::from_raw(2_000_000_000_000_000_000, 18),
                1,
            ),
            0,
            0,
            0.into(),
        );
        assert_eq!(
            book.get_avg_px_qty_for_exposure(target, OrderSide::Buy),
            (1.0, 1.0, 1.0)
        );
    }
}
