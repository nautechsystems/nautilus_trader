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

//! Represents a quantity with a non-negative value.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Deref, Mul, MulAssign, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::{
    correctness::{check_in_range_inclusive_f64, FAILED},
    parsing::precision_from_str,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

use super::fixed::{check_fixed_precision, FIXED_PRECISION, FIXED_SCALAR};
use crate::types::fixed::{f64_to_fixed_u64, fixed_u64_to_f64};

/// The sentinel value for an unset or null quantity.
pub const QUANTITY_UNDEF: u64 = u64::MAX;

/// The maximum valid quantity value which can be represented.
pub const QUANTITY_MAX: f64 = 18_446_744_073.0;

/// The minimum valid quantity value which can be represented.
pub const QUANTITY_MIN: f64 = 0.0;

/// Represents a quantity with a non-negative value.
///
/// Capable of storing either a whole number (no decimal places) of 'contracts'
/// or 'shares' (instruments denominated in whole units) or a decimal value
/// containing decimal places for instruments denominated in fractional units.
///
/// Handles up to 9 decimals of precision.
///
/// - `QUANTITY_MAX` = 18_446_744_073
/// - `QUANTITY_MIN` = 0
#[repr(C)]
#[derive(Clone, Copy, Default, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Quantity {
    /// The raw quantity as an unsigned 64-bit integer.
    /// Represents the unscaled value, with `precision` defining the number of decimal places.
    pub raw: u64,
    /// The number of decimal places, with a maximum precision of 9.
    pub precision: u8,
}

impl Quantity {
    /// Creates a new [`Quantity`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `value` is invalid outside the representable range [0, 18_446_744_073].
    /// - If `precision` is invalid outside the representable range [0, 9].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(value: f64, precision: u8) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(value, QUANTITY_MIN, QUANTITY_MAX, "value")?;
        check_fixed_precision(precision)?;

        Ok(Self {
            raw: f64_to_fixed_u64(value, precision),
            precision,
        })
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
    pub fn from_raw(raw: u64, precision: u8) -> Self {
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
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let rescaled_raw = self.raw / u64::pow(10, u32::from(FIXED_PRECISION - self.precision));
        Decimal::from_i128_with_scale(i128::from(rescaled_raw), u32::from(self.precision))
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

impl From<i64> for Quantity {
    fn from(input: i64) -> Self {
        Self::new(input as f64, 0)
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
    type Target = u64;

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
            raw: result_raw / (FIXED_SCALAR as u64),
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

impl From<Quantity> for u64 {
    fn from(value: Quantity) -> Self {
        value.raw
    }
}

impl From<&Quantity> for u64 {
    fn from(value: &Quantity) -> Self {
        value.raw
    }
}

impl FromStr for Quantity {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let float_from_input = input
            .replace('_', "")
            .parse::<f64>()
            .map_err(|e| format!("Error parsing `input` string '{input}' as f64: {e}"))?;

        Ok(Self::new(float_from_input, precision_from_str(input)))
    }
}

impl From<&str> for Quantity {
    fn from(input: &str) -> Self {
        Self::from_str(input).expect("Valid string input for `Quantity`")
    }
}

impl<T: Into<u64>> AddAssign<T> for Quantity {
    fn add_assign(&mut self, other: T) {
        self.raw = self
            .raw
            .checked_add(other.into())
            .expect("Overflow occurred when adding `Quantity`");
    }
}

impl<T: Into<u64>> SubAssign<T> for Quantity {
    fn sub_assign(&mut self, other: T) {
        self.raw = self
            .raw
            .checked_sub(other.into())
            .expect("Underflow occurred when subtracting `Quantity`");
    }
}

impl<T: Into<u64>> MulAssign<T> for Quantity {
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

pub fn check_quantity_positive(value: Quantity) -> anyhow::Result<()> {
    if !value.is_positive() {
        anyhow::bail!("{FAILED}: invalid `Quantity`, should be positive and was {value}")
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
    #[should_panic(expected = "Condition failed: invalid `Quantity`, should be positive and was 0")]
    fn test_check_quantity_positive() {
        let qty = Quantity::new(0.0, 0);
        check_quantity_positive(qty).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_new() {
        // Precision out of range for fixed
        let _ = Quantity::new(1.0, 10);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_from_raw() {
        // Precision out of range for fixed
        let _ = Quantity::from_raw(1, 10);
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: `precision` was greater than the maximum ")]
    fn test_invalid_precision_zero() {
        // Precision out of range for fixed
        let _ = Quantity::zero(10);
    }

    #[rstest]
    fn test_new() {
        let qty = Quantity::new(0.00812, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
        assert!(!qty.is_zero());
        assert!(qty.is_positive());
        assert_eq!(qty.as_decimal(), dec!(0.00812000));
        assert!(approx_eq!(f64, qty.as_f64(), 0.00812, epsilon = 0.000_001));
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
    fn test_from_i64() {
        let qty = Quantity::from(100_000);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 100_000_000_000_000);
        assert_eq!(qty.precision, 0);
    }

    #[rstest]
    fn test_with_maximum_value() {
        let qty = Quantity::new(QUANTITY_MAX, 8);
        assert_eq!(qty.raw, 18_446_744_073_000_000_000);
        assert_eq!(qty.as_decimal(), dec!(18_446_744_073));
        assert_eq!(qty.to_string(), "18446744073.00000000");
        assert_eq!(qty.to_formatted_string(), "18_446_744_073.00000000");
    }

    #[rstest]
    fn test_with_minimum_positive_value() {
        let qty = Quantity::new(0.000_000_001, 9);
        assert_eq!(qty.raw, 1);
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
        let qty = Quantity::new(1.001, 2);
        assert_eq!(qty.raw, 1_000_000_000);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[rstest]
    fn test_new_from_str() {
        let qty = Quantity::new(0.00812000, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
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
        let quantity1 = Quantity::new(1.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        let quantity3 = quantity1 + quantity2;
        assert_eq!(quantity3.raw, 3_000_000_000);
    }

    #[rstest]
    fn test_sub() {
        let quantity1 = Quantity::new(3.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        let quantity3 = quantity1 - quantity2;
        assert_eq!(quantity3.raw, 1_000_000_000);
    }

    #[rstest]
    fn test_add_assign() {
        let mut quantity1 = Quantity::new(1.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        quantity1 += quantity2;
        assert_eq!(quantity1.raw, 3_000_000_000);
    }

    #[rstest]
    fn test_sub_assign() {
        let mut quantity1 = Quantity::new(3.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        quantity1 -= quantity2;
        assert_eq!(quantity1.raw, 1_000_000_000);
    }

    #[rstest]
    fn test_mul() {
        let quantity1 = Quantity::new(2.0, 1);
        let quantity2 = Quantity::new(2.0, 1);
        let quantity3 = quantity1 * quantity2;
        assert_eq!(quantity3.raw, 4_000_000_000);
    }

    #[rstest]
    fn test_equality() {
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
}
