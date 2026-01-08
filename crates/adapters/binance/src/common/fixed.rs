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

//! Fixed-point conversion utilities for Binance SBE mantissa/exponent values.
//!
//! Binance SBE responses encode numeric values as mantissa + exponent pairs.
//! These utilities convert directly to Nautilus fixed-point types using pure
//! integer arithmetic, avoiding floating-point precision loss.

use nautilus_model::types::{
    Price, Quantity, fixed::FIXED_PRECISION, price::PriceRaw, quantity::QuantityRaw,
};

/// Precomputed powers of 10 for efficient scaling (covers 0..=18).
const POWERS_OF_10: [i128; 19] = [
    1,
    10,
    100,
    1_000,
    10_000,
    100_000,
    1_000_000,
    10_000_000,
    100_000_000,
    1_000_000_000,
    10_000_000_000,
    100_000_000_000,
    1_000_000_000_000,
    10_000_000_000_000,
    100_000_000_000_000,
    1_000_000_000_000_000,
    10_000_000_000_000_000,
    100_000_000_000_000_000,
    1_000_000_000_000_000_000,
];

/// Returns 10^exp using precomputed table.
#[inline]
fn pow10(exp: u8) -> i128 {
    POWERS_OF_10[exp as usize]
}

/// Scales a mantissa by 10^(FIXED_PRECISION + exponent) to produce a Nautilus raw value.
///
/// Uses i128 arithmetic internally to handle high-precision mode (FIXED_PRECISION=16).
#[inline]
fn scale_mantissa(mantissa: i64, exponent: i8) -> PriceRaw {
    let scale_exp = FIXED_PRECISION as i8 + exponent;
    let mantissa_wide = mantissa as i128;

    let scaled = if scale_exp >= 0 {
        mantissa_wide * pow10(scale_exp as u8)
    } else {
        mantissa_wide / pow10((-scale_exp) as u8)
    };

    scaled as PriceRaw
}

/// Converts a mantissa/exponent pair to a Nautilus [`Price`].
///
/// Uses pure integer arithmetic: `raw = mantissa * 10^(FIXED_PRECISION + exponent)`.
#[must_use]
pub fn mantissa_to_price(mantissa: i64, exponent: i8, precision: u8) -> Price {
    let raw = scale_mantissa(mantissa, exponent);
    Price::from_raw(raw, precision)
}

/// Converts a mantissa/exponent pair to a Nautilus [`Quantity`].
///
/// Uses pure integer arithmetic: `raw = mantissa * 10^(FIXED_PRECISION + exponent)`.
///
/// # Panics
///
/// Panics if `mantissa` is negative.
#[must_use]
pub fn mantissa_to_quantity(mantissa: i64, exponent: i8, precision: u8) -> Quantity {
    assert!(mantissa >= 0, "Quantity cannot be negative: {mantissa}");
    let raw = scale_mantissa(mantissa, exponent);
    Quantity::from_raw(raw as QuantityRaw, precision)
}

/// Converts a mantissa/exponent pair to f64 for display/debugging only.
///
/// This should NOT be used for domain type conversion - use [`mantissa_to_price`]
/// or [`mantissa_to_quantity`] instead.
#[must_use]
#[inline]
pub fn mantissa_to_f64(mantissa: i64, exponent: i8) -> f64 {
    mantissa as f64 * 10_f64.powi(exponent as i32)
}

#[cfg(test)]
mod tests {
    use nautilus_core::approx_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(12345678, -8, 8, 0.12345678)]
    #[case(9876543210, -8, 8, 98.7654321)]
    #[case(100000000, -8, 8, 1.0)]
    #[case(50000, -2, 2, 500.0)]
    #[case(123, 0, 0, 123.0)]
    fn test_mantissa_to_price(
        #[case] mantissa: i64,
        #[case] exponent: i8,
        #[case] precision: u8,
        #[case] expected: f64,
    ) {
        let price = mantissa_to_price(mantissa, exponent, precision);
        assert!(
            approx_eq!(f64, price.as_f64(), expected, epsilon = 1e-10),
            "Expected {expected}, got {}",
            price.as_f64()
        );
        assert_eq!(price.precision, precision);
    }

    #[rstest]
    #[case(12345678, -8, 8, 0.12345678)]
    #[case(100000000, -8, 8, 1.0)]
    #[case(50000, -2, 2, 500.0)]
    fn test_mantissa_to_quantity(
        #[case] mantissa: i64,
        #[case] exponent: i8,
        #[case] precision: u8,
        #[case] expected: f64,
    ) {
        let qty = mantissa_to_quantity(mantissa, exponent, precision);
        assert!(
            approx_eq!(f64, qty.as_f64(), expected, epsilon = 1e-10),
            "Expected {expected}, got {}",
            qty.as_f64()
        );
        assert_eq!(qty.precision, precision);
    }

    #[rstest]
    fn test_mantissa_to_f64() {
        assert!(approx_eq!(
            f64,
            mantissa_to_f64(12345678, -8),
            0.12345678,
            epsilon = 1e-15
        ));
        assert!(approx_eq!(
            f64,
            mantissa_to_f64(100, 2),
            10000.0,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    #[should_panic(expected = "Quantity cannot be negative")]
    fn test_negative_quantity_panics() {
        let _ = mantissa_to_quantity(-100, 0, 0);
    }
}
