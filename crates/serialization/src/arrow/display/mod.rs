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

//! Display-mode Arrow encoders for Nautilus types.
//!
//! These encoders emit schemas compatible with display pipelines that cannot
//! consume `FixedSizeBinary` columns.
//! Prices and quantities render as `Float64` via `.as_f64()`, `instrument_id` becomes a
//! `Utf8` column rather than batch metadata (so mixed-instrument batches work), and
//! timestamps render as `Timestamp(Nanosecond, None)` rather than `UInt64`.
//!
//! The conversion is lossy: precision metadata is discarded when values cast to `f64`.
//! For catalog storage that must round-trip, use the `FixedSizeBinary` encoders in
//! the parent [`crate::arrow`] module instead.

pub mod account_state;
pub mod bar;
pub mod close;
pub mod delta;
pub mod depth;
pub mod index_price;
pub mod instrument;
pub mod mark_price;
pub mod order_filled;
pub mod position;
pub mod quote;
pub mod report;
pub mod trade;

use arrow::datatypes::{DataType, Field, TimeUnit};
use nautilus_model::types::{
    Money, Price, Quantity, fixed::MAX_FLOAT_PRECISION, price::PRICE_ERROR,
};
use rust_decimal::prelude::ToPrimitive;

/// Upper bound on precision the display encoders accept. Values above this are
/// treated as pathological sentinels (most notably `ERROR_PRICE`, which carries
/// `precision: 255`) and emit `NaN`. Legitimate high-precision inputs top out at
/// `nautilus_model::defi::WEI_PRECISION` (18).
const DISPLAY_MAX_PRECISION: u8 = 18;

/// Builds a non-nullable `Utf8` field with the given name.
pub(super) fn utf8_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Utf8, nullable)
}

/// Builds a `Boolean` field with the given name and nullability.
pub(super) fn bool_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Boolean, nullable)
}

/// Builds a `Float64` field with the given name and nullability.
pub(super) fn float64_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::Float64, nullable)
}

/// Builds a `UInt8` field with the given name and nullability.
pub(super) fn uint8_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::UInt8, nullable)
}

/// Builds a `UInt32` field with the given name and nullability.
pub(super) fn uint32_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::UInt32, nullable)
}

/// Builds a `UInt64` field with the given name and nullability.
pub(super) fn uint64_field(name: &str, nullable: bool) -> Field {
    Field::new(name, DataType::UInt64, nullable)
}

/// Builds a `Timestamp(Nanosecond, None)` field with the given name and nullability.
pub(super) fn timestamp_field(name: &str, nullable: bool) -> Field {
    Field::new(
        name,
        DataType::Timestamp(TimeUnit::Nanosecond, None),
        nullable,
    )
}

/// Converts a `u64` nanosecond timestamp to the `i64` expected by Arrow.
///
/// Nautilus timestamps fit comfortably in `i64`, but this clamps defensively
/// to avoid an overflow panic on the cast.
pub(super) fn unix_nanos_to_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Converts a [`Price`] to `f64` for display without panicking.
///
/// Returns [`f64::NAN`] for sentinel values (`PRICE_UNDEF`, `PRICE_ERROR`,
/// and the `ERROR_PRICE` synthetic with `precision: 255`), so clear-style
/// order book deltas and error sentinels render as missing cells instead of
/// bogus numeric values. [`Price::as_f64`] panics when the `defi` feature is
/// enabled and precision exceeds [`MAX_FLOAT_PRECISION`] (16), so this helper
/// falls back to a [`rust_decimal::Decimal`] conversion in that range. The
/// decimal path returns [`f64::NAN`] if the value is outside `f64` range.
pub(super) fn price_to_f64(price: &Price) -> f64 {
    if price.is_undefined() || price.raw == PRICE_ERROR || price.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if price.precision <= MAX_FLOAT_PRECISION {
        price.as_f64()
    } else {
        price.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

/// Converts a [`Quantity`] to `f64` for display without panicking.
///
/// See [`price_to_f64`] for the rationale. Returns [`f64::NAN`] for the
/// `QUANTITY_UNDEF` sentinel and for pathological precisions.
pub(super) fn quantity_to_f64(quantity: &Quantity) -> f64 {
    if quantity.is_undefined() || quantity.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if quantity.precision <= MAX_FLOAT_PRECISION {
        quantity.as_f64()
    } else {
        quantity.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

/// Converts a [`Money`] amount to `f64` for display without panicking.
///
/// [`Money::as_f64`] panics under `feature = "defi"` when the currency
/// precision exceeds [`MAX_FLOAT_PRECISION`] (16); high-precision tokens
/// (e.g. 18-decimal ERC-20s) would otherwise abort an entire display batch.
/// This helper guards pathological precisions and falls back to the decimal
/// path for 17-18 decimal currencies. Returns [`f64::NAN`] if the value is
/// outside `f64` range.
pub(super) fn money_to_f64(money: &Money) -> f64 {
    if money.currency.precision > DISPLAY_MAX_PRECISION {
        return f64::NAN;
    }

    if money.currency.precision <= MAX_FLOAT_PRECISION {
        money.as_f64()
    } else {
        money.as_decimal().to_f64().unwrap_or(f64::NAN)
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::{
        Currency, Money, Price, Quantity,
        price::{ERROR_PRICE, PRICE_ERROR, PRICE_UNDEF},
        quantity::QUANTITY_UNDEF,
    };
    use rstest::rstest;

    use super::{money_to_f64, price_to_f64, quantity_to_f64};

    #[rstest]
    fn test_price_to_f64_normal_value() {
        let price = Price::from("100.10");
        assert!((price_to_f64(&price) - 100.10).abs() < 1e-9);
    }

    #[rstest]
    fn test_price_to_f64_undef_sentinel_is_nan() {
        let price = Price::from_raw(PRICE_UNDEF, 0);
        assert!(price_to_f64(&price).is_nan());
    }

    #[rstest]
    fn test_price_to_f64_error_sentinel_is_nan() {
        let price = Price::from_raw(PRICE_ERROR, 0);
        assert!(price_to_f64(&price).is_nan());
    }

    #[rstest]
    fn test_price_to_f64_error_price_constant_is_nan() {
        // ERROR_PRICE has precision = 255; must not panic and must emit NaN
        assert!(price_to_f64(&ERROR_PRICE).is_nan());
    }

    #[rstest]
    fn test_price_to_f64_wei_precision_boundary_is_finite() {
        // Precision 18 is the upper bound for legitimate wei-precision inputs
        // and must not be caught by the pathological-precision guard. Struct
        // literal bypasses `from_raw`'s `FIXED_PRECISION` assertion so the
        // test runs across all feature combinations.
        let price = Price {
            raw: 1_000_000_000_000_000_000,
            precision: 18,
        };
        let value = price_to_f64(&price);

        assert!(value.is_finite(), "precision 18 should not return NaN");
        assert!((value - 1.0).abs() < 1e-9);
    }

    #[rstest]
    fn test_quantity_to_f64_normal_value() {
        let quantity = Quantity::from(1_000);
        assert!((quantity_to_f64(&quantity) - 1_000.0).abs() < 1e-9);
    }

    #[rstest]
    fn test_quantity_to_f64_undef_sentinel_is_nan() {
        let quantity = Quantity::from_raw(QUANTITY_UNDEF, 0);
        assert!(quantity_to_f64(&quantity).is_nan());
    }

    #[rstest]
    fn test_quantity_to_f64_pathological_precision_is_nan() {
        // Mirrors the `ERROR_PRICE` guard for `price_to_f64`: any precision
        // beyond `DISPLAY_MAX_PRECISION` (18) must emit NaN rather than
        // panic or render a bogus value.
        let quantity = Quantity {
            raw: 0,
            precision: 200,
        };
        assert!(quantity_to_f64(&quantity).is_nan());
    }

    #[rstest]
    fn test_money_to_f64_normal_value() {
        let money = Money::new(123.45, Currency::USD());
        assert!((money_to_f64(&money) - 123.45).abs() < 1e-9);
    }

    #[rstest]
    fn test_money_to_f64_pathological_precision_is_nan() {
        // Simulates a DeFi currency whose precision exceeds DISPLAY_MAX_PRECISION
        // so Money::as_f64 would panic under feature = "defi"; we mutate precision
        // directly since Currency::new rejects values above FIXED_PRECISION.
        let mut money = Money::new(0.0, Currency::USD());
        money.currency.precision = 200;
        assert!(money_to_f64(&money).is_nan());
    }
}
