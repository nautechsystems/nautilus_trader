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

//! Represents a quantity with a non-negative value and specified precision.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Deref, Mul, MulAssign, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::{
    correctness::{FAILED, check_in_range_inclusive_f64, check_predicate_true},
    parsing::precision_from_str,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

use super::fixed::{FIXED_PRECISION, FIXED_SCALAR, check_fixed_precision};
#[cfg(not(feature = "high-precision"))]
use super::fixed::{f64_to_fixed_u64, fixed_u64_to_f64};
#[cfg(feature = "high-precision")]
use super::fixed::{f64_to_fixed_u128, fixed_u128_to_f64};
#[cfg(feature = "defi")]
use crate::types::fixed::MAX_FLOAT_PRECISION;

// -----------------------------------------------------------------------------
// QuantityRaw
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
pub type QuantityRaw = u128;

#[cfg(not(feature = "high-precision"))]
pub type QuantityRaw = u64;

// -----------------------------------------------------------------------------

/// The maximum raw quantity integer value.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static QUANTITY_RAW_MAX: QuantityRaw = (QUANTITY_MAX * FIXED_SCALAR) as QuantityRaw;

/// The sentinel value for an unset or null quantity.
pub const QUANTITY_UNDEF: QuantityRaw = QuantityRaw::MAX;

// -----------------------------------------------------------------------------
// QUANTITY_MAX
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The maximum valid quantity value that can be represented.
pub const QUANTITY_MAX: f64 = 34_028_236_692_093.0;

#[cfg(not(feature = "high-precision"))]
/// The maximum valid quantity value that can be represented.
pub const QUANTITY_MAX: f64 = 18_446_744_073.0;

// -----------------------------------------------------------------------------

/// The minimum valid quantity value that can be represented.
pub const QUANTITY_MIN: f64 = 0.0;

/// Represents a quantity with a non-negative value and specified precision.
///
/// Capable of storing either a whole number (no decimal places) of 'contracts'
/// or 'shares' (instruments denominated in whole units) or a decimal value
/// containing decimal places for instruments denominated in fractional units.
///
/// Handles up to [`FIXED_PRECISION`] decimals of precision.
///
/// - [`QUANTITY_MAX`] - Maximum representable quantity value.
/// - [`QUANTITY_MIN`] - 0 (non-negative values only).
#[repr(C)]
#[derive(Clone, Copy, Default, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Quantity {
    /// Represents the raw fixed-point value, with `precision` defining the number of decimal places.
    pub raw: QuantityRaw,
    /// The number of decimal places, with a maximum of [`FIXED_PRECISION`].
    pub precision: u8,
}

impl Quantity {
    /// Creates a new [`Quantity`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is invalid outside the representable range [0, `QUANTITY_MAX`].
    /// - `precision` is invalid outside the representable range [0, `FIXED_PRECISION`].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(value: f64, precision: u8) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(value, QUANTITY_MIN, QUANTITY_MAX, "value")?;

        #[cfg(feature = "defi")]
        if precision > MAX_FLOAT_PRECISION {
            // Floats are only reliable up to ~16 decimal digits of precision regardless of feature flags
            anyhow::bail!(
                "`precision` exceeded maximum float precision ({MAX_FLOAT_PRECISION}), use `Quantity::from_wei()` for WEI values instead"
            );
        }

        check_fixed_precision(precision)?;

        #[cfg(feature = "high-precision")]
        let raw = f64_to_fixed_u128(value, precision);
        #[cfg(not(feature = "high-precision"))]
        let raw = f64_to_fixed_u64(value, precision);

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Quantity`] instance with a guaranteed non zero value.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is zero.
    /// - `value` becomes zero after rounding to `precision`.
    /// - `value` is invalid outside the representable range [0, `QUANTITY_MAX`].
    /// - `precision` is invalid outside the representable range [0, `FIXED_PRECISION`].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn non_zero_checked(value: f64, precision: u8) -> anyhow::Result<Self> {
        check_predicate_true(value != 0.0, "value was zero")?;
        check_fixed_precision(precision)?;
        let rounded_value =
            (value * 10.0_f64.powi(precision as i32)).round() / 10.0_f64.powi(precision as i32);
        check_predicate_true(
            rounded_value != 0.0,
            &format!("value {value} was zero after rounding to precision {precision}"),
        )?;

        Self::new_checked(value, precision)
    }

    /// Creates a new [`Quantity`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Quantity::new_checked`] for more details.
    pub fn new(value: f64, precision: u8) -> Self {
        Self::new_checked(value, precision).expect(FAILED)
    }

    /// Creates a new [`Quantity`] instance with a guaranteed non zero value.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Quantity::non_zero_checked`] for more details.
    pub fn non_zero(value: f64, precision: u8) -> Self {
        Self::non_zero_checked(value, precision).expect(FAILED)
    }

    /// Creates a new [`Quantity`] instance from the given `raw` fixed-point value and `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Quantity::new_checked`] for more details.
    pub fn from_raw(raw: QuantityRaw, precision: u8) -> Self {
        if raw == QUANTITY_UNDEF {
            check_predicate_true(
                precision == 0,
                "`precision` must be 0 when `raw` is QUANTITY_UNDEF",
            )
            .expect(FAILED);
        }
        check_predicate_true(
            raw == QUANTITY_UNDEF || raw <= QUANTITY_RAW_MAX,
            &format!("raw outside valid range, was {raw}"),
        )
        .expect(FAILED);
        check_fixed_precision(precision).expect(FAILED);
        Self { raw, precision }
    }

    /// Creates a new [`Quantity`] instance with a value of zero with the given `precision`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Quantity::new_checked`] for more details.
    #[must_use]
    pub fn zero(precision: u8) -> Self {
        check_fixed_precision(precision).expect(FAILED);
        Self::new(0.0, precision)
    }

    /// Returns `true` if the value of this instance is undefined.
    #[must_use]
    pub fn is_undefined(&self) -> bool {
        self.raw == QUANTITY_UNDEF
    }

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Returns `true` if the value of this instance is position (> 0).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.raw != QUANTITY_UNDEF && self.raw > 0
    }

    #[cfg(feature = "high-precision")]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond [`MAX_FLOAT_PRECISION`] (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        if self.precision > MAX_FLOAT_PRECISION {
            panic!("Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)");
        }

        fixed_u128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond [`MAX_FLOAT_PRECISION`] (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let precision_diff = FIXED_PRECISION.saturating_sub(self.precision);
        let rescaled_raw = self.raw / QuantityRaw::pow(10, u32::from(precision_diff));

        // SAFETY: The raw value is guaranteed to be within i128 range after scaling
        // because our quantity constraints ensure the maximum raw value times the scaling
        // factor cannot exceed i128::MAX (high-precision) or i64::MAX (standard-precision).
        #[allow(clippy::useless_conversion)] // Required for precision modes
        Decimal::from_i128_with_scale(rescaled_raw as i128, u32::from(self.precision))
    }

    /// Returns a formatted string representation of this instance.
    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        format!("{self}").separate_with_underscores()
    }
}

impl From<Quantity> for f64 {
    fn from(qty: Quantity) -> Self {
        qty.as_f64()
    }
}

impl From<&Quantity> for f64 {
    fn from(qty: &Quantity) -> Self {
        qty.as_f64()
    }
}

impl From<i32> for Quantity {
    fn from(value: i32) -> Self {
        Self::new(value as f64, 0)
    }
}

impl From<i64> for Quantity {
    fn from(value: i64) -> Self {
        Self::new(value as f64, 0)
    }
}

impl From<u32> for Quantity {
    fn from(value: u32) -> Self {
        Self::new(value as f64, 0)
    }
}

impl From<u64> for Quantity {
    fn from(value: u64) -> Self {
        Self::new(value as f64, 0)
    }
}

impl Hash for Quantity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Quantity {
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

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Quantity {
    type Target = QuantityRaw;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Add for Quantity {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        let precision = match self.precision {
            0 => rhs.precision,
            _ => self.precision,
        };
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
                .expect("Overflow occurred when adding `Quantity`"),
            precision,
        }
    }
}

impl Sub for Quantity {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        let precision = match self.precision {
            0 => rhs.precision,
            _ => self.precision,
        };
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
                .expect("Underflow occurred when subtracting `Quantity`"),
            precision,
        }
    }
}

#[allow(clippy::suspicious_arithmetic_impl)] // Can use division to scale back
impl Mul for Quantity {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        let precision = match self.precision {
            0 => rhs.precision,
            _ => self.precision,
        };
        assert!(
            self.precision >= rhs.precision,
            "Precision mismatch: cannot multiply precision {} with precision {} (precision loss)",
            rhs.precision,
            self.precision,
        );

        let result_raw = self
            .raw
            .checked_mul(rhs.raw)
            .expect("Overflow occurred when multiplying `Quantity`");

        Self {
            raw: result_raw / (FIXED_SCALAR as QuantityRaw),
            precision,
        }
    }
}

impl Mul<f64> for Quantity {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl From<Quantity> for QuantityRaw {
    fn from(value: Quantity) -> Self {
        value.raw
    }
}

impl From<&Quantity> for QuantityRaw {
    fn from(value: &Quantity) -> Self {
        value.raw
    }
}

impl FromStr for Quantity {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let float_from_input = value
            .replace('_', "")
            .parse::<f64>()
            .map_err(|e| format!("Error parsing `input` string '{value}' as f64: {e}"))?;

        Self::new_checked(float_from_input, precision_from_str(value)).map_err(|e| e.to_string())
    }
}

// Note: we can't implement `AsRef<str>` due overlapping traits (maybe there is a way)
impl From<&str> for Quantity {
    fn from(value: &str) -> Self {
        Self::from_str(value).expect("Valid string input for `Quantity`")
    }
}

impl From<String> for Quantity {
    fn from(value: String) -> Self {
        Self::from_str(&value).expect("Valid string input for `Quantity`")
    }
}

impl From<&String> for Quantity {
    fn from(value: &String) -> Self {
        Self::from_str(value).expect("Valid string input for `Quantity`")
    }
}

impl<T: Into<QuantityRaw>> AddAssign<T> for Quantity {
    fn add_assign(&mut self, other: T) {
        self.raw = self
            .raw
            .checked_add(other.into())
            .expect("Overflow occurred when adding `Quantity`");
    }
}

impl<T: Into<QuantityRaw>> SubAssign<T> for Quantity {
    fn sub_assign(&mut self, other: T) {
        self.raw = self
            .raw
            .checked_sub(other.into())
            .expect("Underflow occurred when subtracting `Quantity`");
    }
}

impl<T: Into<QuantityRaw>> MulAssign<T> for Quantity {
    fn mul_assign(&mut self, other: T) {
        self.raw = self
            .raw
            .checked_mul(other.into())
            .expect("Overflow occurred when multiplying `Quantity`");
    }
}

impl Debug for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}({:.*})",
            stringify!(Quantity),
            self.precision as usize,
            self.as_f64(),
        )
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Serialize for Quantity {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Quantity {
    fn deserialize<D>(_deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let qty_str: &str = Deserialize::deserialize(_deserializer)?;
        let qty: Self = qty_str.into();
        Ok(qty)
    }
}

/// Checks if the given quantity is positive.
///
/// # Errors
///
/// Returns an error if the quantity is not positive.
pub fn check_positive_quantity(value: Quantity, param: &str) -> anyhow::Result<()> {
    if !value.is_positive() {
        anyhow::bail!("invalid `Quantity` for '{param}' not positive, was {value}")
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use float_cmp::approx_eq;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[should_panic(expected = "invalid `Quantity` for 'qty' not positive, was 0")]
    fn test_check_quantity_positive() {
        let qty = Quantity::new(0.0, 0);
        check_positive_quantity(qty, "qty").unwrap();
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "`precision` exceeded maximum `FIXED_PRECISION` (16), was 17")]
    fn test_invalid_precision_new() {
        // Precision 17 should fail due to DeFi validation
        let _ = Quantity::new(1.0, 17);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Quantity::from_raw(1, FIXED_PRECISION + 1);
    }

    #[rstest]
    #[cfg(not(feature = "defi"))]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Quantity::zero(FIXED_PRECISION + 1);
    }

    #[rstest]
    #[should_panic(
        expected = "Precision mismatch: cannot add precision 2 to precision 1 (precision loss)"
    )]
    fn test_precision_mismatch_add() {
        let q1 = Quantity::new(1.0, 1);
        let q2 = Quantity::new(1.0, 2);
        let _ = q1 + q2;
    }

    #[rstest]
    #[should_panic(
        expected = "Precision mismatch: cannot subtract precision 2 from precision 1 (precision loss)"
    )]
    fn test_precision_mismatch_sub() {
        let q1 = Quantity::new(1.0, 1);
        let q2 = Quantity::new(1.0, 2);
        let _ = q1 - q2;
    }

    #[rstest]
    #[should_panic(
        expected = "Precision mismatch: cannot multiply precision 2 with precision 1 (precision loss)"
    )]
    fn test_precision_mismatch_mul() {
        let q1 = Quantity::new(2.0, 1);
        let q2 = Quantity::new(3.0, 2);
        let _ = q1 * q2;
    }

    #[rstest]
    fn test_new_non_zero_ok() {
        let qty = Quantity::non_zero_checked(123.456, 3).unwrap();
        assert_eq!(qty.raw, Quantity::new(123.456, 3).raw);
        assert!(qty.is_positive());
    }

    #[rstest]
    fn test_new_non_zero_zero_input() {
        assert!(Quantity::non_zero_checked(0.0, 0).is_err());
    }

    #[rstest]
    fn test_new_non_zero_rounds_to_zero() {
        // 0.0004 rounded to 3 dp ⇒ 0.000
        assert!(Quantity::non_zero_checked(0.0004, 3).is_err());
    }

    #[rstest]
    fn test_new_non_zero_negative() {
        assert!(Quantity::non_zero_checked(-1.0, 0).is_err());
    }

    #[rstest]
    fn test_new_non_zero_exceeds_max() {
        assert!(Quantity::non_zero_checked(QUANTITY_MAX * 10.0, 0).is_err());
    }

    #[rstest]
    fn test_new_non_zero_invalid_precision() {
        assert!(Quantity::non_zero_checked(1.0, FIXED_PRECISION + 1).is_err());
    }

    #[rstest]
    fn test_new() {
        let value = 0.00812;
        let qty = Quantity::new(value, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, Quantity::from(&format!("{value}")).raw);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.as_decimal(), dec!(0.00812000));
        assert_eq!(qty.to_string(), "0.00812000");
        assert!(!qty.is_zero());
        assert!(qty.is_positive());
        assert!(approx_eq!(f64, qty.as_f64(), 0.00812, epsilon = 0.000_001));
    }

    #[rstest]
    fn test_check_quantity_positive_ok() {
        let qty = Quantity::new(10.0, 0);
        check_positive_quantity(qty, "qty").unwrap();
    }

    #[rstest]
    fn test_negative_quantity_validation() {
        assert!(Quantity::new_checked(-1.0, FIXED_PRECISION).is_err());
    }

    #[rstest]
    fn test_undefined() {
        let qty = Quantity::from_raw(QUANTITY_UNDEF, 0);
        assert_eq!(qty.raw, QUANTITY_UNDEF);
        assert!(qty.is_undefined());
    }

    #[rstest]
    fn test_zero() {
        let qty = Quantity::zero(8);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.precision, 8);
        assert!(qty.is_zero());
        assert!(!qty.is_positive());
    }

    #[rstest]
    fn test_from_i32() {
        let value = 100_000i32;
        let qty = Quantity::from(value);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, Quantity::from(&format!("{value}")).raw);
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_from_u32() {
        let value: u32 = 5000;
        let qty = Quantity::from(value);
        assert_eq!(qty.raw, Quantity::from(format!("{value}")).raw);
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_from_i64() {
        let value = 100_000i64;
        let qty = Quantity::from(value);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, Quantity::from(&format!("{value}")).raw);
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_from_u64() {
        let value = 100_000u64;
        let qty = Quantity::from(value);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, Quantity::from(&format!("{value}")).raw);
        assert_eq!(qty.precision, 0);
    }

    #[rstest] // Test does not panic rather than exact value
    fn test_with_maximum_value() {
        let qty = Quantity::new_checked(QUANTITY_MAX, 0);
        assert!(qty.is_ok());
    }

    #[rstest]
    fn test_with_minimum_positive_value() {
        let value = 0.000_000_001;
        let qty = Quantity::new(value, 9);
        assert_eq!(qty.raw, Quantity::from("0.000000001").raw);
        assert_eq!(qty.as_decimal(), dec!(0.000000001));
        assert_eq!(qty.to_string(), "0.000000001");
    }

    #[rstest]
    fn test_with_minimum_value() {
        let qty = Quantity::new(QUANTITY_MIN, 9);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.as_decimal(), dec!(0));
        assert_eq!(qty.to_string(), "0.000000000");
    }

    #[rstest]
    fn test_is_zero() {
        let qty = Quantity::zero(8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.0);
        assert_eq!(qty.as_decimal(), dec!(0));
        assert_eq!(qty.to_string(), "0.00000000");
        assert!(qty.is_zero());
    }

    #[rstest]
    fn test_precision() {
        let value = 1.001;
        let qty = Quantity::new(value, 2);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[rstest]
    fn test_new_from_str() {
        let qty = Quantity::new(0.00812000, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[rstest]
    #[case("0", 0)]
    #[case("1.1", 1)]
    #[case("1.123456789", 9)]
    fn test_from_str_valid_input(#[case] input: &str, #[case] expected_prec: u8) {
        let qty = Quantity::from(input);
        assert_eq!(qty.precision, expected_prec);
        assert_eq!(qty.as_decimal(), Decimal::from_str(input).unwrap());
    }

    #[rstest]
    #[should_panic]
    fn test_from_str_invalid_input() {
        let input = "invalid";
        Quantity::new(f64::from_str(input).unwrap(), 8);
    }

    #[rstest]
    fn test_add() {
        let a = 1.0;
        let b = 2.0;
        let quantity1 = Quantity::new(1.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        let quantity3 = quantity1 + quantity2;
        assert_eq!(quantity3.raw, Quantity::new(a + b, 0).raw);
    }

    #[rstest]
    fn test_sub() {
        let a = 3.0;
        let b = 2.0;
        let quantity1 = Quantity::new(a, 0);
        let quantity2 = Quantity::new(b, 0);
        let quantity3 = quantity1 - quantity2;
        assert_eq!(quantity3.raw, Quantity::new(a - b, 0).raw);
    }

    #[rstest]
    fn test_add_assign() {
        let a = 1.0;
        let b = 2.0;
        let mut quantity1 = Quantity::new(a, 0);
        let quantity2 = Quantity::new(b, 0);
        quantity1 += quantity2;
        assert_eq!(quantity1.raw, Quantity::new(a + b, 0).raw);
    }

    #[rstest]
    fn test_sub_assign() {
        let a = 3.0;
        let b = 2.0;
        let mut quantity1 = Quantity::new(a, 0);
        let quantity2 = Quantity::new(b, 0);
        quantity1 -= quantity2;
        assert_eq!(quantity1.raw, Quantity::new(a - b, 0).raw);
    }

    #[rstest]
    fn test_mul() {
        let value = 2.0;
        let quantity1 = Quantity::new(value, 1);
        let quantity2 = Quantity::new(value, 1);
        let quantity3 = quantity1 * quantity2;
        assert_eq!(quantity3.raw, Quantity::new(value * value, 0).raw);
    }

    #[rstest]
    fn test_mul_assign() {
        let mut quantity = Quantity::new(2.0, 0);
        quantity *= 3u64; // calls MulAssign<T: Into<QuantityRaw>>
        assert_eq!(quantity.raw, Quantity::new(6.0, 0).raw);

        let mut fraction = Quantity::new(1.5, 2);
        fraction *= 2u64; // => 1.5 * 2 = 3.0 => raw=300, precision=2
        assert_eq!(fraction.raw, Quantity::new(3.0, 2).raw);
    }

    #[rstest]
    fn test_comparisons() {
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 1));
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 2));
        assert_ne!(Quantity::new(1.1, 1), Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) <= Quantity::new(1.0, 2));
        assert!(Quantity::new(1.1, 1) > Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 2));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 2));
        assert!(Quantity::new(0.9, 1) < Quantity::new(1.0, 1));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 2));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 1));
    }

    #[rstest]
    fn test_debug() {
        let quantity = Quantity::from_str("44.12").unwrap();
        let result = format!("{quantity:?}");
        assert_eq!(result, "Quantity(44.12)");
    }

    #[rstest]
    fn test_display() {
        let quantity = Quantity::from_str("44.12").unwrap();
        let result = format!("{quantity}");
        assert_eq!(result, "44.12");
    }

    #[rstest]
    fn test_to_formatted_string() {
        let qty = Quantity::new(1234.5678, 4);
        let formatted = qty.to_formatted_string();
        assert_eq!(formatted, "1_234.5678");
        assert_eq!(qty.to_string(), "1234.5678");
    }

    #[rstest]
    fn test_hash() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let q1 = Quantity::new(100.0, 1);
        let q2 = Quantity::new(100.0, 1);
        let q3 = Quantity::new(200.0, 1);

        let mut s1 = DefaultHasher::new();
        let mut s2 = DefaultHasher::new();
        let mut s3 = DefaultHasher::new();

        q1.hash(&mut s1);
        q2.hash(&mut s2);
        q3.hash(&mut s3);

        assert_eq!(
            s1.finish(),
            s2.finish(),
            "Equal quantities must hash equally"
        );
        assert_ne!(
            s1.finish(),
            s3.finish(),
            "Different quantities must hash differently"
        );
    }

    #[rstest]
    fn test_quantity_serde_json_round_trip() {
        let original = Quantity::new(123.456, 3);
        let json_str = serde_json::to_string(&original).unwrap();
        assert_eq!(json_str, "\"123.456\"");

        let deserialized: Quantity = serde_json::from_str(&json_str).unwrap();
        assert_eq!(deserialized, original);
        assert_eq!(deserialized.precision, 3);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Property-based tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;

    use super::*;

    /// Strategy to generate valid quantity values (non-negative).
    fn quantity_value_strategy() -> impl Strategy<Value = f64> {
        // Use a reasonable range for quantities - must be non-negative
        prop_oneof![
            // Small positive values
            0.00001..1.0,
            // Normal trading range
            1.0..100_000.0,
            // Large values (but safe)
            100_000.0..1_000_000.0,
            // Include zero
            Just(0.0),
        ]
    }

    /// Strategy to generate valid precision values.
    fn precision_strategy() -> impl Strategy<Value = u8> {
        #[cfg(feature = "defi")]
        {
            // In DeFi mode, exclude precision 17 (invalid) and 18 (f64 constructor incompatible)
            0..=16u8
        }
        #[cfg(not(feature = "defi"))]
        {
            0..=FIXED_PRECISION
        }
    }

    proptest! {
        /// Property: Quantity string serialization round-trip should preserve value and precision
        #[test]
        fn prop_quantity_serde_round_trip(
            value in quantity_value_strategy(),
            precision in 0u8..=6u8  // Limit precision to avoid extreme floating-point cases
        ) {
            let original = Quantity::new(value, precision);

            // String round-trip (this should be exact and is the most important)
            let string_repr = original.to_string();
            let from_string: Quantity = string_repr.parse().unwrap();
            prop_assert_eq!(from_string.raw, original.raw);
            prop_assert_eq!(from_string.precision, original.precision);

            // JSON round-trip basic validation (just ensure it doesn't crash and preserves precision)
            let json = serde_json::to_string(&original).unwrap();
            let from_json: Quantity = serde_json::from_str(&json).unwrap();
            prop_assert_eq!(from_json.precision, original.precision);
            // Note: JSON may have minor floating-point precision differences due to f64 limitations
        }

        /// Property: Quantity arithmetic should be associative for same precision
        #[test]
        fn prop_quantity_arithmetic_associative(
            a in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 1e-3 && x < 1e6),
            b in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 1e-3 && x < 1e6),
            c in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 1e-3 && x < 1e6),
            precision in 0u8..=6u8  // Limit precision to avoid extreme cases
        ) {
            let q_a = Quantity::new(a, precision);
            let q_b = Quantity::new(b, precision);
            let q_c = Quantity::new(c, precision);

            // Check if we can perform the operations without overflow using raw arithmetic
            let ab_raw = q_a.raw.checked_add(q_b.raw);
            let bc_raw = q_b.raw.checked_add(q_c.raw);

            if let (Some(ab_raw), Some(bc_raw)) = (ab_raw, bc_raw) {
                let ab_c_raw = ab_raw.checked_add(q_c.raw);
                let a_bc_raw = q_a.raw.checked_add(bc_raw);

                if let (Some(ab_c_raw), Some(a_bc_raw)) = (ab_c_raw, a_bc_raw) {
                    // (a + b) + c == a + (b + c) using raw arithmetic (exact)
                    prop_assert_eq!(ab_c_raw, a_bc_raw, "Associativity failed in raw arithmetic");
                }
            }
        }

        /// Property: Quantity addition/subtraction should be inverse operations (when valid)
        #[test]
        fn prop_quantity_addition_subtraction_inverse(
            base in quantity_value_strategy().prop_filter("Reasonable values", |&x| x < 1e6),
            delta in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 1e-3 && x < 1e6),
            precision in 0u8..=6u8  // Limit precision to avoid extreme cases
        ) {
            let q_base = Quantity::new(base, precision);
            let q_delta = Quantity::new(delta, precision);

            // Use raw arithmetic to avoid floating-point precision issues
            if let Some(added_raw) = q_base.raw.checked_add(q_delta.raw) {
                if let Some(result_raw) = added_raw.checked_sub(q_delta.raw) {
                    // (base + delta) - delta should equal base exactly using raw arithmetic
                    prop_assert_eq!(result_raw, q_base.raw, "Inverse operation failed in raw arithmetic");
                }
            }
        }

        /// Property: Quantity ordering should be transitive
        #[test]
        fn prop_quantity_ordering_transitive(
            a in quantity_value_strategy(),
            b in quantity_value_strategy(),
            c in quantity_value_strategy(),
            precision in precision_strategy()
        ) {
            let q_a = Quantity::new(a, precision);
            let q_b = Quantity::new(b, precision);
            let q_c = Quantity::new(c, precision);

            // If a <= b and b <= c, then a <= c
            if q_a <= q_b && q_b <= q_c {
                prop_assert!(q_a <= q_c, "Transitivity failed: {} <= {} <= {} but {} > {}",
                    q_a.as_f64(), q_b.as_f64(), q_c.as_f64(), q_a.as_f64(), q_c.as_f64());
            }
        }

        /// Property: String parsing should be consistent with precision inference
        #[test]
        fn prop_quantity_string_parsing_precision(
            integral in 0u32..1000000,
            fractional in 0u32..1000000,
            precision in 1u8..=6
        ) {
            // Create a decimal string with exactly 'precision' decimal places
            let fractional_str = format!("{:0width$}", fractional % 10_u32.pow(precision as u32), width = precision as usize);
            let quantity_str = format!("{}.{}", integral, fractional_str);

            let parsed: Quantity = quantity_str.parse().unwrap();
            prop_assert_eq!(parsed.precision, precision);

            // Round-trip should preserve the original string (after normalization)
            let round_trip = parsed.to_string();
            let expected_value = format!("{}.{}", integral, fractional_str);
            prop_assert_eq!(round_trip, expected_value);
        }

        /// Property: Quantity with higher precision should contain more or equal information
        #[test]
        fn prop_quantity_precision_information_preservation(
            value in quantity_value_strategy().prop_filter("Reasonable values", |&x| x < 1e6),
            precision1 in 1u8..=6u8,  // Limit precision range for more predictable behavior
            precision2 in 1u8..=6u8
        ) {
            // Skip cases where precisions are equal (trivial case)
            prop_assume!(precision1 != precision2);

            let _q1 = Quantity::new(value, precision1);
            let _q2 = Quantity::new(value, precision2);

            // When both quantities are created from the same value with different precisions,
            // converting both to the lower precision should yield the same result
            let min_precision = precision1.min(precision2);

            // Round the original value to the minimum precision first
            let scale = 10.0_f64.powi(min_precision as i32);
            let rounded_value = (value * scale).round() / scale;

            let q1_reduced = Quantity::new(rounded_value, min_precision);
            let q2_reduced = Quantity::new(rounded_value, min_precision);

            // They should be exactly equal when created from the same rounded value
            prop_assert_eq!(q1_reduced.raw, q2_reduced.raw, "Precision reduction inconsistent");
        }

        /// Property: Quantity arithmetic should never produce invalid values
        #[test]
        fn prop_quantity_arithmetic_bounds(
            a in quantity_value_strategy(),
            b in quantity_value_strategy(),
            precision in precision_strategy()
        ) {
            let q_a = Quantity::new(a, precision);
            let q_b = Quantity::new(b, precision);

            // Addition should either succeed or fail predictably
            let sum_f64 = q_a.as_f64() + q_b.as_f64();
            if sum_f64.is_finite() && sum_f64 >= QUANTITY_MIN && sum_f64 <= QUANTITY_MAX {
                let sum = q_a + q_b;
                prop_assert!(sum.as_f64().is_finite());
                prop_assert!(!sum.is_undefined());
            }

            // Subtraction should either succeed or fail predictably
            let diff_f64 = q_a.as_f64() - q_b.as_f64();
            if diff_f64.is_finite() && diff_f64 >= QUANTITY_MIN && diff_f64 <= QUANTITY_MAX {
                let diff = q_a - q_b;
                prop_assert!(diff.as_f64().is_finite());
                prop_assert!(!diff.is_undefined());
            }
        }

        /// Property: Multiplication should preserve non-negativity
        #[test]
        fn prop_quantity_multiplication_non_negative(
            a in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 0.0 && x < 100.0),
            b in quantity_value_strategy().prop_filter("Reasonable values", |&x| x > 0.0 && x < 100.0),
            precision in precision_strategy()
        ) {
            let q_a = Quantity::new(a, precision);
            let q_b = Quantity::new(b, precision);

            // Check if multiplication would overflow before performing it
            let product_f64 = q_a.as_f64() * q_b.as_f64();
            // More conservative overflow check for high-precision modes
            let safe_limit = if cfg!(any(feature = "defi", feature = "high-precision")) {
                QUANTITY_MAX / 10000.0  // Much more conservative in high-precision mode
            } else {
                QUANTITY_MAX
            };

            if product_f64.is_finite() && product_f64 <= safe_limit {
                // Multiplying two quantities should always result in a non-negative value
                let product = q_a * q_b;
                prop_assert!(product.as_f64() >= 0.0, "Quantity multiplication produced negative value: {}", product.as_f64());
            }
        }

        /// Property: Zero quantity should be identity for addition
        #[test]
        fn prop_quantity_zero_addition_identity(
            value in quantity_value_strategy(),
            precision in precision_strategy()
        ) {
            let q = Quantity::new(value, precision);
            let zero = Quantity::zero(precision);

            // q + 0 = q and 0 + q = q
            prop_assert_eq!(q + zero, q);
            prop_assert_eq!(zero + q, q);
        }
    }
}
