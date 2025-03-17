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

//! Represents a quantity with a non-negative value.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Deref, Mul, MulAssign, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::{
    correctness::{FAILED, check_in_range_inclusive_f64},
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

#[cfg(feature = "high-precision")]
pub type QuantityRaw = u128;
#[cfg(not(feature = "high-precision"))]
pub type QuantityRaw = u64;

/// The maximum raw quantity integer value.
#[unsafe(no_mangle)]
pub static QUANTITY_RAW_MAX: QuantityRaw = (QUANTITY_MAX * FIXED_SCALAR) as QuantityRaw;

/// The sentinel value for an unset or null quantity.
pub const QUANTITY_UNDEF: QuantityRaw = QuantityRaw::MAX;

/// The maximum valid quantity value which can be represented.
#[cfg(feature = "high-precision")]
pub const QUANTITY_MAX: f64 = 34_028_236_692_093.0;
#[cfg(not(feature = "high-precision"))]
pub const QUANTITY_MAX: f64 = 18_446_744_073.0;

/// The minimum valid quantity value which can be represented.
pub const QUANTITY_MIN: f64 = 0.0;

/// Represents a quantity with a non-negative value.
///
/// Capable of storing either a whole number (no decimal places) of 'contracts'
/// or 'shares' (instruments denominated in whole units) or a decimal value
/// containing decimal places for instruments denominated in fractional units.
///
/// Handles up to {FIXED_PRECISION} decimals of precision.
///
/// - `QUANTITY_MAX` = {QUANTITY_MAX}
/// - `QUANTITY_MIN` = 0
#[repr(C)]
#[derive(Clone, Copy, Default, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Quantity {
    /// Represents the raw fixed-point value, with `precision` defining the number of decimal places.
    pub raw: QuantityRaw,
    /// The number of decimal places, with a maximum of {FIXED_PRECISION}.
    pub precision: u8,
}

impl Quantity {
    /// Creates a new [`Quantity`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `value` is invalid outside the representable range [0, {QUANTITY_MAX}].
    /// - If `precision` is invalid outside the representable range [0, {FIXED_PRECISION}].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(value: f64, precision: u8) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(value, QUANTITY_MIN, QUANTITY_MAX, "value")?;
        check_fixed_precision(precision)?;

        #[cfg(feature = "high-precision")]
        let raw = f64_to_fixed_u128(value, precision);
        #[cfg(not(feature = "high-precision"))]
        let raw = f64_to_fixed_u64(value, precision);

        Ok(Self { raw, precision })
    }

    /// Creates a new [`Quantity`] instance.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If a correctness check fails. See [`Quantity::new_checked`] for more details.
    pub fn new(value: f64, precision: u8) -> Self {
        Self::new_checked(value, precision).expect(FAILED)
    }

    /// Creates a new [`Quantity`] instance from the given `raw` fixed-point value and `precision`.
    pub fn from_raw(raw: QuantityRaw, precision: u8) -> Self {
        check_fixed_precision(precision).expect(FAILED);
        Self { raw, precision }
    }

    /// Creates a new [`Quantity`] instance with a value of zero with the given `precision`.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If a correctness check fails. See [`Quantity::new_checked`] for more details.
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
        self.raw > 0
    }

    /// Returns the value of this instance as an `f64`.
    #[must_use]
    #[cfg(feature = "high-precision")]
    pub fn as_f64(&self) -> f64 {
        fixed_u128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let rescaled_raw =
            self.raw / QuantityRaw::pow(10, u32::from(FIXED_PRECISION - self.precision));
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
        anyhow::bail!("{FAILED}: invalid `Quantity` for '{param}' not positive, was {value}")
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
    #[should_panic(expected = "Condition failed: invalid `Quantity` for 'qty' not positive, was 0")]
    fn test_check_quantity_positive() {
        let qty = Quantity::new(0.0, 0);
        check_positive_quantity(qty, "qty").unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_new() {
        // Precision out of range for fixed
        let _ = Quantity::new(1.0, FIXED_PRECISION + 1);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` exceeded maximum `FIXED_PRECISION`")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Quantity::from_raw(1, FIXED_PRECISION + 1);
    }

    #[rstest]
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

    #[test]
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
