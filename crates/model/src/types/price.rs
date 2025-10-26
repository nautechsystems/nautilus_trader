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

//! Represents a price in a market with a specified precision.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Deref, Mul, Neg, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::correctness::{FAILED, check_in_range_inclusive_f64, check_predicate_true};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

use super::fixed::{FIXED_PRECISION, FIXED_SCALAR, check_fixed_precision};
#[cfg(feature = "high-precision")]
use super::fixed::{PRECISION_DIFF_SCALAR, f64_to_fixed_i128, fixed_i128_to_f64};
#[cfg(not(feature = "high-precision"))]
use super::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
#[cfg(feature = "defi")]
use crate::types::fixed::MAX_FLOAT_PRECISION;

// -----------------------------------------------------------------------------
// PriceRaw
// -----------------------------------------------------------------------------

// Use 128-bit integers when either `high-precision` or `defi` features are enabled. This is
// required for the extended 18-decimal wei precision used in DeFi contexts.

#[cfg(feature = "high-precision")]
pub type PriceRaw = i128;

#[cfg(not(feature = "high-precision"))]
pub type PriceRaw = i64;

// -----------------------------------------------------------------------------

/// The maximum raw price integer value.
///
/// # Safety
///
/// This value is computed at compile time from PRICE_MAX * FIXED_SCALAR.
/// The multiplication is guaranteed not to overflow because PRICE_MAX and FIXED_SCALAR
/// are chosen such that their product fits within PriceRaw's range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static PRICE_RAW_MAX: PriceRaw = (PRICE_MAX * FIXED_SCALAR) as PriceRaw;

/// The minimum raw price integer value.
///
/// # Safety
///
/// This value is computed at compile time from PRICE_MIN * FIXED_SCALAR.
/// The multiplication is guaranteed not to overflow because PRICE_MIN and FIXED_SCALAR
/// are chosen such that their product fits within PriceRaw's range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static PRICE_RAW_MIN: PriceRaw = (PRICE_MIN * FIXED_SCALAR) as PriceRaw;

/// The sentinel value for an unset or null price.
pub const PRICE_UNDEF: PriceRaw = PriceRaw::MAX;

/// The sentinel value for an error or invalid price.
pub const PRICE_ERROR: PriceRaw = PriceRaw::MIN;

// -----------------------------------------------------------------------------
// PRICE_MAX
// -----------------------------------------------------------------------------

/// The maximum valid price value that can be represented.
#[cfg(feature = "high-precision")]
pub const PRICE_MAX: f64 = 17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The maximum valid price value that can be represented.
pub const PRICE_MAX: f64 = 9_223_372_036.0;

// -----------------------------------------------------------------------------
// PRICE_MIN
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The minimum valid price value that can be represented.
pub const PRICE_MIN: f64 = -17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The minimum valid price value that can be represented.
pub const PRICE_MIN: f64 = -9_223_372_036.0;

// -----------------------------------------------------------------------------

/// The sentinel `Price` representing errors (this will be removed when Cython is gone).
pub const ERROR_PRICE: Price = Price {
    raw: 0,
    precision: 255,
};

/// Represents a price in a market with a specified precision.
///
/// The number of decimal places may vary. For certain asset classes, prices may
/// have negative values. For example, prices for options instruments can be
/// negative under certain conditions.
///
/// Handles up to [`FIXED_PRECISION`] decimals of precision.
///
/// - [`PRICE_MAX`] - Maximum representable price value.
/// - [`PRICE_MIN`] - Minimum representable price value.
#[repr(C)]
#[derive(Clone, Copy, Default, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", frozen)
)]
pub struct Price {
    /// Represents the raw fixed-point value, with `precision` defining the number of decimal places.
    pub raw: PriceRaw,
    /// The number of decimal places, with a maximum of [`FIXED_PRECISION`].
    pub precision: u8,
}

impl Price {
    /// Creates a new [`Price`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is invalid outside the representable range [`PRICE_MIN`, `PRICE_MAX`].
    /// - `precision` is invalid outside the representable range [0, `FIXED_PRECISION``].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(value: f64, precision: u8) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(value, PRICE_MIN, PRICE_MAX, "value")?;

        #[cfg(feature = "defi")]
        if precision > MAX_FLOAT_PRECISION {
            // Floats are only reliable up to ~16 decimal digits of precision regardless of feature flags
            anyhow::bail!(
                "`precision` exceeded maximum float precision ({MAX_FLOAT_PRECISION}), use `Price::from_wei()` for wei values instead"
            );
        }

        check_fixed_precision(precision)?;

        #[cfg(feature = "high-precision")]
        let raw = f64_to_fixed_i128(value, precision);

        #[cfg(not(feature = "high-precision"))]
        let raw = f64_to_fixed_i64(value, precision);

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Price`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    pub fn new(value: f64, precision: u8) -> Self {
        Self::new_checked(value, precision).expect(FAILED)
    }

    /// Creates a new [`Price`] instance from the given `raw` fixed-point value and `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    pub fn from_raw(raw: PriceRaw, precision: u8) -> Self {
        if raw == PRICE_UNDEF {
            check_predicate_true(
                precision == 0,
                "`precision` must be 0 when `raw` is PRICE_UNDEF",
            )
            .expect(FAILED);
        }
        check_predicate_true(
            raw == PRICE_ERROR
                || raw == PRICE_UNDEF
                || (raw >= PRICE_RAW_MIN && raw <= PRICE_RAW_MAX),
            &format!("raw value outside valid range, was {raw}"),
        )
        .expect(FAILED);
        check_fixed_precision(precision).expect(FAILED);
        Self { raw, precision }
    }

    /// Creates a new [`Price`] instance with a value of zero with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn zero(precision: u8) -> Self {
        check_fixed_precision(precision).expect(FAILED);
        Self { raw: 0, precision }
    }

    /// Creates a new [`Price`] instance with the maximum representable value with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn max(precision: u8) -> Self {
        check_fixed_precision(precision).expect(FAILED);
        Self {
            raw: PRICE_RAW_MAX,
            precision,
        }
    }

    /// Creates a new [`Price`] instance with the minimum representable value with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Price::new_checked`] for more details.
    #[must_use]
    pub fn min(precision: u8) -> Self {
        check_fixed_precision(precision).expect(FAILED);
        Self {
            raw: PRICE_RAW_MIN,
            precision,
        }
    }

    /// Returns `true` if the value of this instance is undefined.
    #[must_use]
    pub fn is_undefined(&self) -> bool {
        self.raw == PRICE_UNDEF
    }

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Returns `true` if the value of this instance is position (> 0).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.raw != PRICE_UNDEF && self.raw > 0
    }

    #[cfg(feature = "high-precision")]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond [`MAX_FLOAT_PRECISION`] (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        if self.precision > MAX_FLOAT_PRECISION {
            panic!("Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)");
        }

        fixed_i128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond [`MAX_FLOAT_PRECISION`] (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        if self.precision > MAX_FLOAT_PRECISION {
            panic!("Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)");
        }

        fixed_i64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let precision_diff = FIXED_PRECISION.saturating_sub(self.precision);
        let rescaled_raw = self.raw / PriceRaw::pow(10, u32::from(precision_diff));
        #[allow(clippy::unnecessary_cast, reason = "Required for precision modes")]
        Decimal::from_i128_with_scale(rescaled_raw as i128, u32::from(self.precision))
    }

    /// Returns a formatted string representation of this instance.
    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        format!("{self}").separate_with_underscores()
    }

    /// Creates a new [`Price`] from a `Decimal` value with specified precision.
    ///
    /// This method provides more reliable parsing by using Decimal arithmetic
    /// to avoid floating-point precision issues during conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `precision` exceeds [`FIXED_PRECISION`].
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    pub fn from_decimal(decimal: Decimal, precision: u8) -> anyhow::Result<Self> {
        check_fixed_precision(precision)?;

        // Scale the decimal to the target precision
        let scale_factor = Decimal::from(10_i64.pow(precision as u32));
        let scaled = decimal * scale_factor;
        let rounded = scaled.round();

        #[cfg(feature = "high-precision")]
        let raw_at_precision: PriceRaw = rounded.to_i128().ok_or_else(|| {
            anyhow::anyhow!("Decimal value '{decimal}' cannot be converted to i128")
        })?;
        #[cfg(not(feature = "high-precision"))]
        let raw_at_precision: PriceRaw = rounded.to_i64().ok_or_else(|| {
            anyhow::anyhow!("Decimal value '{decimal}' cannot be converted to i64")
        })?;

        let scale_up = 10_i64.pow((FIXED_PRECISION - precision) as u32) as PriceRaw;
        let raw = raw_at_precision
            .checked_mul(scale_up)
            .ok_or_else(|| anyhow::anyhow!("Overflow when scaling to fixed precision"))?;

        check_predicate_true(
            raw == PRICE_UNDEF || (raw >= PRICE_RAW_MIN && raw <= PRICE_RAW_MAX),
            &format!("raw value outside valid range, was {raw}"),
        )?;

        Ok(Self { raw, precision })
    }
}

impl FromStr for Price {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let clean_value = value.replace('_', "");

        let decimal = if clean_value.contains('e') || clean_value.contains('E') {
            Decimal::from_scientific(&clean_value)
                .map_err(|e| format!("Error parsing `input` string '{value}' as Decimal: {e}"))?
        } else {
            Decimal::from_str(&clean_value)
                .map_err(|e| format!("Error parsing `input` string '{value}' as Decimal: {e}"))?
        };

        // Determine precision from the final decimal result
        let decimal_str = decimal.to_string();
        let precision = if let Some(dot_pos) = decimal_str.find('.') {
            let decimal_part = &decimal_str[dot_pos + 1..];
            decimal_part.len().min(u8::MAX as usize) as u8
        } else {
            0
        };

        Self::from_decimal(decimal, precision).map_err(|e| e.to_string())
    }
}

impl<T: AsRef<str>> From<T> for Price {
    fn from(value: T) -> Self {
        Self::from_str(value.as_ref()).expect(FAILED)
    }
}

impl From<Price> for f64 {
    fn from(price: Price) -> Self {
        price.as_f64()
    }
}

impl From<&Price> for f64 {
    fn from(price: &Price) -> Self {
        price.as_f64()
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    fn lt(&self, other: &Self) -> bool {
        self.raw.lt(&other.raw)
    }

    fn le(&self, other: &Self) -> bool {
        self.raw.le(&other.raw)
    }

    fn gt(&self, other: &Self) -> bool {
        self.raw.gt(&other.raw)
    }

    fn ge(&self, other: &Self) -> bool {
        self.raw.ge(&other.raw)
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Price {
    type Target = PriceRaw;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Neg for Price {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            raw: -self.raw,
            precision: self.precision,
        }
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        // SAFETY: Current precision logic ensures only equal or higher precision operations
        // are allowed to prevent silent precision loss. When self.precision >= rhs.precision,
        // the rhs value is effectively scaled up internally by the fixed-point representation,
        // so no actual precision is lost in the addition. However, the result is limited
        // to self.precision decimal places.
        assert!(
            self.precision >= rhs.precision,
            "Precision mismatch: cannot add precision {} to precision {} (precision loss)",
            rhs.precision,
            self.precision,
        );
        Self {
            raw: self
                .raw
                .checked_add(rhs.raw)
                .expect("Overflow occurred when adding `Price`"),
            precision: self.precision,
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        // SAFETY: Current precision logic ensures only equal or higher precision operations
        // are allowed to prevent silent precision loss. When self.precision >= rhs.precision,
        // the rhs value is effectively scaled up internally by the fixed-point representation,
        // so no actual precision is lost in the subtraction. However, the result is limited
        // to self.precision decimal places.
        assert!(
            self.precision >= rhs.precision,
            "Precision mismatch: cannot subtract precision {} from precision {} (precision loss)",
            rhs.precision,
            self.precision,
        );
        Self {
            raw: self
                .raw
                .checked_sub(rhs.raw)
                .expect("Underflow occurred when subtracting `Price`"),
            precision: self.precision,
        }
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        assert!(
            self.precision >= other.precision,
            "Precision mismatch: cannot add precision {} to precision {} (precision loss)",
            other.precision,
            self.precision,
        );
        self.raw = self
            .raw
            .checked_add(other.raw)
            .expect("Overflow occurred when adding `Price`");
    }
}

impl SubAssign for Price {
    fn sub_assign(&mut self, other: Self) {
        assert!(
            self.precision >= other.precision,
            "Precision mismatch: cannot subtract precision {} from precision {} (precision loss)",
            other.precision,
            self.precision,
        );
        self.raw = self
            .raw
            .checked_sub(other.raw)
            .expect("Underflow occurred when subtracting `Price`");
    }
}

impl Add<f64> for Price {
    type Output = f64;
    fn add(self, rhs: f64) -> Self::Output {
        self.as_f64() + rhs
    }
}

impl Sub<f64> for Price {
    type Output = f64;
    fn sub(self, rhs: f64) -> Self::Output {
        self.as_f64() - rhs
    }
}

impl Mul<f64> for Price {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl Debug for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{}({})", stringify!(Price), self.raw)
        } else {
            write!(f, "{}({})", stringify!(Price), self.as_decimal())
        }
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{}", self.raw)
        } else {
            write!(f, "{}", self.as_decimal())
        }
    }
}

impl Serialize for Price {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Price {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let price_str: &str = Deserialize::deserialize(_deserializer)?;
        let price: Self = price_str.into();
        Ok(price)
    }
}

/// Checks the price `value` is positive.
///
/// # Errors
///
/// Returns an error if `value` is `PRICE_UNDEF` or not positive.
pub fn check_positive_price(value: Price, param: &str) -> anyhow::Result<()> {
    if value.raw == PRICE_UNDEF {
        anyhow::bail!("invalid `Price` for '{param}', was PRICE_UNDEF")
    }
    if !value.is_positive() {
        anyhow::bail!("invalid `Price` for '{param}' not positive, was {value}")
    }
    Ok(())
}

#[cfg(feature = "high-precision")]
/// The raw i64 price has already been scaled by 10^9. Further scale it by the difference to
/// `FIXED_PRECISION` to make it high/defi-precision raw price.
pub fn decode_raw_price_i64(value: i64) -> PriceRaw {
    value as PriceRaw * PRECISION_DIFF_SCALAR as PriceRaw
}

#[cfg(not(feature = "high-precision"))]
pub fn decode_raw_price_i64(value: i64) -> PriceRaw {
    value
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::approx_eq;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[cfg(all(not(feature = "defi"), not(feature = "high-precision")))]
    #[should_panic(expected = "`precision` exceeded maximum `FIXED_PRECISION` (9), was 50")]
    fn test_invalid_precision_new() {
        // Precision exceeds float precision limit
        let _ = Price::new(1.0, 50);
    }

    #[rstest]
    #[cfg(all(not(feature = "defi"), feature = "high-precision"))]
    #[should_panic(expected = "`precision` exceeded maximum `FIXED_PRECISION` (16), was 50")]
    fn test_invalid_precision_new() {
        // Precision exceeds float precision limit
        let _ = Price::new(1.0, 50);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Price::from_raw(1, FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_max() {
        // Precision out of range for fixed
        let _ = Price::max(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_min() {
        // Precision out of range for fixed
        let _ = Price::min(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Price::zero(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid f64 for 'value' not in range")]
    fn test_max_value_exceeded() {
        Price::new(PRICE_MAX + 0.1, FIXED_PRECISION);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: invalid f64 for 'value' not in range")]
    fn test_min_value_exceeded() {
        Price::new(PRICE_MIN - 0.1, FIXED_PRECISION);
    }

    #[rstest]
    fn test_is_positive_ok() {
        // A normal, non‑zero price should be positive.
        let price = Price::new(42.0, 2);
        assert!(price.is_positive());

        // `check_positive_price` should accept it without error.
        check_positive_price(price, "price").unwrap();
    }

    #[rstest]
    #[should_panic(expected = "invalid `Price` for 'price' not positive")]
    fn test_is_positive_rejects_non_positive() {
        // Zero is NOT positive.
        let zero = Price::zero(2);
        check_positive_price(zero, "price").unwrap();
    }

    #[rstest]
    #[should_panic(expected = "invalid `Price` for 'price', was PRICE_UNDEF")]
    fn test_is_positive_rejects_undefined() {
        // PRICE_UNDEF must also be rejected.
        let undef = Price::from_raw(PRICE_UNDEF, 0);
        check_positive_price(undef, "price").unwrap();
    }

    #[rstest]
    fn test_construction() {
        let price = Price::new_checked(1.23456, 4);
        assert!(price.is_ok());
        let price = price.unwrap();
        assert_eq!(price.precision, 4);
        assert!(approx_eq!(f64, price.as_f64(), 1.23456, epsilon = 0.0001));
    }

    #[rstest]
    fn test_negative_price_in_range() {
        // Use max fixed precision which varies based on feature flags
        let neg_price = Price::new(PRICE_MIN / 2.0, FIXED_PRECISION);
        assert!(neg_price.raw < 0);
    }

    #[rstest]
    fn test_new_checked() {
        // Use max fixed precision which varies based on feature flags
        assert!(Price::new_checked(1.0, FIXED_PRECISION).is_ok());
        assert!(Price::new_checked(f64::NAN, FIXED_PRECISION).is_err());
        assert!(Price::new_checked(f64::INFINITY, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_from_raw() {
        let raw = 100 * FIXED_SCALAR as PriceRaw;
        let price = Price::from_raw(raw, 2);
        assert_eq!(price.raw, raw);
        assert_eq!(price.precision, 2);
    }

    #[rstest]
    fn test_zero_constructor() {
        let zero = Price::zero(3);
        assert!(zero.is_zero());
        assert_eq!(zero.precision, 3);
    }

    #[rstest]
    fn test_max_constructor() {
        let max = Price::max(4);
        assert_eq!(max.raw, PRICE_RAW_MAX);
        assert_eq!(max.precision, 4);
    }

    #[rstest]
    fn test_min_constructor() {
        let min = Price::min(4);
        assert_eq!(min.raw, PRICE_RAW_MIN);
        assert_eq!(min.precision, 4);
    }

    #[rstest]
    fn test_nan_validation() {
        assert!(Price::new_checked(f64::NAN, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_infinity_validation() {
        assert!(Price::new_checked(f64::INFINITY, FIXED_PRECISION).is_err());
        assert!(Price::new_checked(f64::NEG_INFINITY, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_special_values() {
        let zero = Price::zero(5);
        assert!(zero.is_zero());
        assert_eq!(zero.to_string(), "0.00000");

        let undef = Price::from_raw(PRICE_UNDEF, 0);
        assert!(undef.is_undefined());

        let error = ERROR_PRICE;
        assert_eq!(error.precision, 255);
    }

    #[rstest]
    fn test_string_parsing() {
        let price: Price = "123.456".into();
        assert_eq!(price.precision, 3);
        assert_eq!(price, Price::from("123.456"));
    }

    #[rstest]
    fn test_negative_price_from_str() {
        let price: Price = "-123.45".parse().unwrap();
        assert_eq!(price.precision, 2);
        assert!(approx_eq!(f64, price.as_f64(), -123.45, epsilon = 1e-9));
    }

    #[rstest]
    fn test_string_parsing_errors() {
        assert!(Price::from_str("invalid").is_err());
    }

    #[rstest]
    #[case("1e7", 0, 10_000_000.0)]
    #[case("1.5e3", 0, 1_500.0)]
    #[case("1.234e-2", 5, 0.01234)]
    #[case("5E-3", 3, 0.005)]
    fn test_from_str_scientific_notation(
        #[case] input: &str,
        #[case] expected_precision: u8,
        #[case] expected_value: f64,
    ) {
        let price = Price::from_str(input).unwrap();
        assert_eq!(price.precision, expected_precision);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    #[case("1_234.56", 2, 1234.56)]
    #[case("1_000_000", 0, 1_000_000.0)]
    #[case("99_999.999_99", 5, 99_999.999_99)]
    fn test_from_str_with_underscores(
        #[case] input: &str,
        #[case] expected_precision: u8,
        #[case] expected_value: f64,
    ) {
        let price = Price::from_str(input).unwrap();
        assert_eq!(price.precision, expected_precision);
        assert!(approx_eq!(
            f64,
            price.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    fn test_from_decimal_precision_preservation() {
        use rust_decimal::Decimal;

        // Test that decimal conversion preserves exact values
        let decimal = Decimal::from_str("123.456789").unwrap();
        let price = Price::from_decimal(decimal, 6).unwrap();
        assert_eq!(price.precision, 6);
        assert!(approx_eq!(f64, price.as_f64(), 123.456789, epsilon = 1e-10));

        // Verify raw value is exact
        let expected_raw = 123456789 * 10_i64.pow((FIXED_PRECISION - 6) as u32);
        assert_eq!(price.raw, expected_raw as PriceRaw);
    }

    #[rstest]
    fn test_from_decimal_rounding() {
        use rust_decimal::Decimal;

        // Test banker's rounding (round half to even)
        let decimal = Decimal::from_str("1.005").unwrap();
        let price = Price::from_decimal(decimal, 2).unwrap();
        assert_eq!(price.as_f64(), 1.0); // 1.005 rounds to 1.00 (even)

        let decimal = Decimal::from_str("1.015").unwrap();
        let price = Price::from_decimal(decimal, 2).unwrap();
        assert_eq!(price.as_f64(), 1.02); // 1.015 rounds to 1.02 (even)
    }

    #[rstest]
    fn test_string_formatting() {
        assert_eq!(format!("{}", Price::new(1234.5678, 4)), "1234.5678");
        assert_eq!(
            format!("{:?}", Price::new(1234.5678, 4)),
            "Price(1234.5678)"
        );
        assert_eq!(Price::new(1234.5678, 4).to_formatted_string(), "1_234.5678");
    }

    #[rstest]
    #[case(1234.5678, 4, "Price(1234.5678)", "1234.5678")] // Normal precision
    #[case(123.456789012345, 8, "Price(123.45678901)", "123.45678901")] // At max normal precision
    #[cfg_attr(
        feature = "defi",
        case(
            2_000_000_000_000_000_000.0,
            18,
            "Price(2000000000000000000)",
            "2000000000000000000"
        )
    )] // High precision
    fn test_string_formatting_precision_handling(
        #[case] value: f64,
        #[case] precision: u8,
        #[case] expected_debug: &str,
        #[case] expected_display: &str,
    ) {
        let price = if precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            Price::from_raw(value as PriceRaw, precision)
        } else {
            Price::new(value, precision)
        };

        assert_eq!(format!("{price:?}"), expected_debug);
        assert_eq!(format!("{price}"), expected_display);
        assert_eq!(
            price.to_formatted_string().replace("_", ""),
            expected_display
        );
    }

    #[rstest]
    fn test_decimal_conversions() {
        let price = Price::new(123.456, 3);
        assert_eq!(price.as_decimal(), dec!(123.456));

        let price = Price::new(0.000001, 6);
        assert_eq!(price.as_decimal(), dec!(0.000001));
    }

    #[rstest]
    fn test_basic_arithmetic() {
        let p1 = Price::new(10.5, 2);
        let p2 = Price::new(5.25, 2);
        assert_eq!(p1 + p2, Price::from("15.75"));
        assert_eq!(p1 - p2, Price::from("5.25"));
        assert_eq!(-p1, Price::from("-10.5"));
    }

    #[rstest]
    #[should_panic(expected = "Precision mismatch: cannot add precision 2 to precision 1")]
    fn test_precision_mismatch_add() {
        let p1 = Price::new(10.5, 1);
        let p2 = Price::new(5.25, 2);
        let _ = p1 + p2;
    }

    #[rstest]
    #[should_panic(expected = "Precision mismatch: cannot subtract precision 2 from precision 1")]
    fn test_precision_mismatch_sub() {
        let p1 = Price::new(10.5, 1);
        let p2 = Price::new(5.25, 2);
        let _ = p1 - p2;
    }

    #[rstest]
    fn test_f64_operations() {
        let p = Price::new(10.5, 2);
        assert_eq!(p + 1.0, 11.5);
        assert_eq!(p - 1.0, 9.5);
        assert_eq!(p * 2.0, 21.0);
    }

    #[rstest]
    fn test_assignment_operators() {
        let mut p = Price::new(10.5, 2);
        p += Price::new(5.25, 2);
        assert_eq!(p, Price::from("15.75"));
        p -= Price::new(5.25, 2);
        assert_eq!(p, Price::from("10.5"));
    }

    #[rstest]
    fn test_equality_and_comparisons() {
        let p1 = Price::new(10.0, 1);
        let p2 = Price::new(20.0, 1);
        let p3 = Price::new(10.0, 1);

        assert!(p1 < p2);
        assert!(p2 > p1);
        assert!(p1 <= p3);
        assert!(p1 >= p3);
        assert_eq!(p1, p3);
        assert_ne!(p1, p2);

        assert_eq!(Price::from("1.0"), Price::from("1.0"));
        assert_ne!(Price::from("1.1"), Price::from("1.0"));
        assert!(Price::from("1.0") <= Price::from("1.0"));
        assert!(Price::from("1.1") > Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("1.0") >= Price::from("1.0"));
        assert!(Price::from("0.9") < Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
        assert!(Price::from("0.9") <= Price::from("1.0"));
    }

    #[rstest]
    fn test_deref() {
        let price = Price::new(10.0, 1);
        assert_eq!(*price, price.raw);
    }

    #[rstest]
    fn test_decode_raw_price_i64() {
        let raw_scaled_by_1e9 = 42_000_000_000i64; // 42.0 * 10^9
        let decoded = decode_raw_price_i64(raw_scaled_by_1e9);
        let price = Price::from_raw(decoded, FIXED_PRECISION);
        assert!(
            approx_eq!(f64, price.as_f64(), 42.0, epsilon = 1e-9),
            "Expected 42.0 f64, was {} (precision = {})",
            price.as_f64(),
            price.precision
        );
    }

    #[rstest]
    fn test_hash() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let price1 = Price::new(1.0, 2);
        let price2 = Price::new(1.0, 2);
        let price3 = Price::new(1.1, 2);

        let mut hasher1 = DefaultHasher::new();
        let mut hasher2 = DefaultHasher::new();
        let mut hasher3 = DefaultHasher::new();

        price1.hash(&mut hasher1);
        price2.hash(&mut hasher2);
        price3.hash(&mut hasher3);

        assert_eq!(hasher1.finish(), hasher2.finish());
        assert_ne!(hasher1.finish(), hasher3.finish());
    }

    #[rstest]
    fn test_price_serde_json_round_trip() {
        let price = Price::new(1.0500, 4);
        let json = serde_json::to_string(&price).unwrap();
        let deserialized: Price = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, price);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Property-based tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    /// Strategy to generate valid price values within the allowed range.
    fn price_value_strategy() -> impl Strategy<Value = f64> {
        // Use a reasonable range that's well within PRICE_MIN/PRICE_MAX
        // but still tests edge cases with various scales
        prop_oneof![
            // Small positive values
            0.00001..1.0,
            // Normal trading range
            1.0..100_000.0,
            // Large values (but safe)
            100_000.0..1_000_000.0,
            // Small negative values (for spreads, etc.)
            -1_000.0..0.0,
            // Boundary values close to the extremes
            Just(PRICE_MIN / 2.0),
            Just(PRICE_MAX / 2.0),
        ]
    }

    fn float_precision_upper_bound() -> u8 {
        FIXED_PRECISION.min(crate::types::fixed::MAX_FLOAT_PRECISION)
    }

    /// Strategy to exercise both typical and extreme precision values.
    fn precision_strategy() -> impl Strategy<Value = u8> {
        let upper = float_precision_upper_bound();
        prop_oneof![Just(0u8), 0u8..=upper, Just(FIXED_PRECISION),]
    }

    fn precision_strategy_non_zero() -> impl Strategy<Value = u8> {
        let upper = float_precision_upper_bound().max(1);
        prop_oneof![Just(upper), Just(FIXED_PRECISION.max(1)), 1u8..=upper,]
    }

    fn price_raw_strategy() -> impl Strategy<Value = PriceRaw> {
        prop_oneof![
            Just(PRICE_RAW_MIN),
            Just(PRICE_RAW_MAX),
            PRICE_RAW_MIN..=PRICE_RAW_MAX,
        ]
    }

    /// Strategy to generate valid precision values for float-based constructors.
    fn float_precision_strategy() -> impl Strategy<Value = u8> {
        precision_strategy()
    }

    proptest! {
        /// Property: Price string serialization round-trip should preserve value and precision
        #[rstest]
        fn prop_price_serde_round_trip(
            value in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let original = Price::new(value, precision);

            // String round-trip (this should be exact and is the most important)
            let string_repr = original.to_string();
            let from_string: Price = string_repr.parse().unwrap();
            prop_assert_eq!(from_string.raw, original.raw);
            prop_assert_eq!(from_string.precision, original.precision);

            // JSON round-trip basic validation (just ensure it doesn't crash and preserves precision)
            let json = serde_json::to_string(&original).unwrap();
            let from_json: Price = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(from_json.precision, original.precision);
            // Note: JSON may have minor floating-point precision differences due to f64 limitations
        }

        /// Property: Price arithmetic should be associative for same precision
        #[rstest]
        fn prop_price_arithmetic_associative(
            a in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            b in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            c in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let p_c = Price::new(c, precision);

            // Check if we can perform the operations without overflow using raw arithmetic
            let ab_raw = p_a.raw.checked_add(p_b.raw);
            let bc_raw = p_b.raw.checked_add(p_c.raw);

            if let (Some(ab_raw), Some(bc_raw)) = (ab_raw, bc_raw) {
                let ab_c_raw = ab_raw.checked_add(p_c.raw);
                let a_bc_raw = p_a.raw.checked_add(bc_raw);

                if let (Some(ab_c_raw), Some(a_bc_raw)) = (ab_c_raw, a_bc_raw) {
                    // (a + b) + c == a + (b + c) using raw arithmetic (exact)
                    prop_assert_eq!(ab_c_raw, a_bc_raw, "Associativity failed in raw arithmetic");
                }
            }
        }

        /// Property: Price addition/subtraction should be inverse operations
        #[rstest]
        fn prop_price_addition_subtraction_inverse(
            base in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            delta in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() > 1e-3 && x.abs() < 1e6),
            precision in precision_strategy()
        ) {
            let p_base = Price::new(base, precision);
            let p_delta = Price::new(delta, precision);

            // Use raw arithmetic to avoid floating-point precision issues
            if let Some(added_raw) = p_base.raw.checked_add(p_delta.raw)
                && let Some(result_raw) = added_raw.checked_sub(p_delta.raw) {
                    // (base + delta) - delta should equal base exactly using raw arithmetic
                    prop_assert_eq!(result_raw, p_base.raw, "Inverse operation failed in raw arithmetic");
                }
        }

        /// Property: Price ordering should be transitive
        #[rstest]
        fn prop_price_ordering_transitive(
            a in price_value_strategy(),
            b in price_value_strategy(),
            c in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);
            let p_c = Price::new(c, precision);

            // If a <= b and b <= c, then a <= c
            if p_a <= p_b && p_b <= p_c {
                prop_assert!(p_a <= p_c, "Transitivity failed: {} <= {} <= {} but {} > {}",
                    p_a.as_f64(), p_b.as_f64(), p_c.as_f64(), p_a.as_f64(), p_c.as_f64());
            }
        }

        /// Property: String parsing should be consistent with precision inference
        #[rstest]
        fn prop_price_string_parsing_precision(
            integral in 0u32..1000000,
            fractional in 0u32..1000000,
            precision in precision_strategy_non_zero()
        ) {
            // Create a decimal string with exactly 'precision' decimal places
            let pow = 10u128.pow(u32::from(precision));
            let fractional_mod = (fractional as u128) % pow;
            let fractional_str = format!("{:0width$}", fractional_mod, width = precision as usize);
            let price_str = format!("{integral}.{fractional_str}");

            let parsed: Price = price_str.parse().unwrap();
            prop_assert_eq!(parsed.precision, precision);

            // Round-trip should preserve the original string (after normalization)
            let round_trip = parsed.to_string();
            let expected_value = format!("{integral}.{fractional_str}");
            prop_assert_eq!(round_trip, expected_value);
        }

        /// Property: Price with higher precision should contain more or equal information
        #[rstest]
        fn prop_price_precision_information_preservation(
            value in price_value_strategy().prop_filter("Reasonable values", |&x| x.abs() < 1e6),
            precision1 in precision_strategy_non_zero(),
            precision2 in precision_strategy_non_zero()
        ) {
            // Skip cases where precisions are equal (trivial case)
            prop_assume!(precision1 != precision2);

            let _p1 = Price::new(value, precision1);
            let _p2 = Price::new(value, precision2);

            // When both prices are created from the same value with different precisions,
            // converting both to the lower precision should yield the same result
            let min_precision = precision1.min(precision2);

            // Round the original value to the minimum precision first
            let scale = 10.0_f64.powi(min_precision as i32);
            let rounded_value = (value * scale).round() / scale;

            let p1_reduced = Price::new(rounded_value, min_precision);
            let p2_reduced = Price::new(rounded_value, min_precision);

            // They should be exactly equal when created from the same rounded value
            prop_assert_eq!(p1_reduced.raw, p2_reduced.raw, "Precision reduction inconsistent");
        }

        /// Property: Price arithmetic should never produce invalid values
        #[rstest]
        fn prop_price_arithmetic_bounds(
            a in price_value_strategy(),
            b in price_value_strategy(),
            precision in float_precision_strategy()
        ) {
            let p_a = Price::new(a, precision);
            let p_b = Price::new(b, precision);

            // Addition should either succeed or fail predictably
            let sum_f64 = p_a.as_f64() + p_b.as_f64();
            if sum_f64.is_finite() && (PRICE_MIN..=PRICE_MAX).contains(&sum_f64) {
                let sum = p_a + p_b;
                prop_assert!(sum.as_f64().is_finite());
                prop_assert!(!sum.is_undefined());
            }

            // Subtraction should either succeed or fail predictably
            let diff_f64 = p_a.as_f64() - p_b.as_f64();
            if diff_f64.is_finite() && (PRICE_MIN..=PRICE_MAX).contains(&diff_f64) {
                let diff = p_a - p_b;
                prop_assert!(diff.as_f64().is_finite());
                prop_assert!(!diff.is_undefined());
            }
        }
    }

    proptest! {
        /// Property: constructing from raw bounds preserves raw/precision fields
        #[rstest]
        fn prop_price_from_raw_round_trip(
            raw in price_raw_strategy(),
            precision in precision_strategy()
        ) {
            let price = Price::from_raw(raw, precision);
            prop_assert_eq!(price.raw, raw);
            prop_assert_eq!(price.precision, precision);
        }
    }
}
