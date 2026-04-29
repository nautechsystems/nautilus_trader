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

//! Represents an amount of money in a specified currency denomination.
//!
//! [`Money`] is an immutable value type for representing monetary amounts with an associated
//! currency. It supports both positive and negative values (for debits, losses, etc.) and
//! enforces currency consistency in arithmetic operations.
//!
//! # Arithmetic behavior
//!
//! | Operation         | Result    | Notes                             |
//! |-------------------|-----------|-----------------------------------|
//! | `Money + Money`   | `Money`   | Panics if currencies don't match. |
//! | `Money - Money`   | `Money`   | Panics if currencies don't match. |
//! | `Money + Decimal` | `Decimal` |                                   |
//! | `Money - Decimal` | `Decimal` |                                   |
//! | `Money * Decimal` | `Decimal` |                                   |
//! | `Money / Decimal` | `Decimal` |                                   |
//! | `Money + f64`     | `f64`     |                                   |
//! | `Money - f64`     | `f64`     |                                   |
//! | `Money * f64`     | `f64`     |                                   |
//! | `Money / f64`     | `f64`     |                                   |
//! | `-Money`          | `Money`   |                                   |
//!
//! # Currency constraints
//!
//! When performing arithmetic between two `Money` values, both must have the same currency.
//! Attempting to add or subtract money with different currencies raises an error.
//!
//! # Immutability
//!
//! `Money` is immutable. All arithmetic operations return new instances.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, Div, Mul, Neg, Sub},
    str::FromStr,
};

use nautilus_core::{
    correctness::{
        CorrectnessError, CorrectnessResult, CorrectnessResultExt, FAILED,
        check_in_range_inclusive_f64,
    },
    string::formatting::Separable,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};

#[cfg(not(any(feature = "defi", feature = "high-precision")))]
use super::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
#[cfg(any(feature = "defi", feature = "high-precision"))]
use super::fixed::{f64_to_fixed_i128, fixed_i128_to_f64};
#[cfg(feature = "defi")]
use crate::types::fixed::MAX_FLOAT_PRECISION;
use crate::types::{
    Currency,
    fixed::{
        FIXED_PRECISION, FIXED_SCALAR, check_fixed_precision, mantissa_exponent_to_fixed_i128,
        raw_scales_match,
    },
};

// -----------------------------------------------------------------------------
// MoneyRaw
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
pub type MoneyRaw = i128;

#[cfg(not(feature = "high-precision"))]
pub type MoneyRaw = i64;

// -----------------------------------------------------------------------------

/// The maximum raw money integer value.
///
/// # Safety
///
/// This value is computed at compile time from `MONEY_MAX` * `FIXED_SCALAR`.
/// The multiplication is guaranteed not to overflow because `MONEY_MAX` and `FIXED_SCALAR`
/// are chosen such that their product fits within `MoneyRaw`'s range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static MONEY_RAW_MAX: MoneyRaw = (MONEY_MAX * FIXED_SCALAR) as MoneyRaw;

/// The minimum raw money integer value.
///
/// # Safety
///
/// This value is computed at compile time from `MONEY_MIN` * `FIXED_SCALAR`.
/// The multiplication is guaranteed not to overflow because `MONEY_MIN` and `FIXED_SCALAR`
/// are chosen such that their product fits within `MoneyRaw`'s range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static MONEY_RAW_MIN: MoneyRaw = (MONEY_MIN * FIXED_SCALAR) as MoneyRaw;

// -----------------------------------------------------------------------------
// MONEY_MAX
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The maximum valid money amount that can be represented.
pub const MONEY_MAX: f64 = 17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The maximum valid money amount that can be represented.
pub const MONEY_MAX: f64 = 9_223_372_036.0;

// -----------------------------------------------------------------------------
// MONEY_MIN
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The minimum valid money amount that can be represented.
pub const MONEY_MIN: f64 = -17_014_118_346_046.0;

#[cfg(not(feature = "high-precision"))]
/// The minimum valid money amount that can be represented.
pub const MONEY_MIN: f64 = -9_223_372_036.0;

// -----------------------------------------------------------------------------

/// Represents an amount of money in a specified currency denomination.
///
/// - [`MONEY_MAX`] - Maximum representable money amount
/// - [`MONEY_MIN`] - Minimum representable money amount
#[repr(C)]
#[derive(Clone, Copy, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.model",
        frozen,
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct Money {
    /// Represents the raw fixed-point amount, with `currency.precision` defining the number of decimal places.
    pub raw: MoneyRaw,
    /// The currency denomination associated with the monetary amount.
    pub currency: Currency,
}

impl Money {
    /// Creates a new [`Money`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `amount` is invalid outside the representable range [`MONEY_MIN`, `MONEY_MAX`].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(amount: f64, currency: Currency) -> CorrectnessResult<Self> {
        // check_in_range_inclusive_f64 already validates that amount is finite
        // (not NaN or infinite) as part of its range validation logic, so no additional
        // infinity checks are needed here.
        check_in_range_inclusive_f64(amount, MONEY_MIN, MONEY_MAX, "amount")?;

        #[cfg(feature = "defi")]
        if currency.precision > MAX_FLOAT_PRECISION {
            // Floats are only reliable up to ~16 decimal digits of precision regardless of feature flags
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "`currency.precision` exceeded maximum float precision ({MAX_FLOAT_PRECISION}), use `Money::from_wei()` for wei values instead"
                ),
            });
        }

        #[cfg(feature = "high-precision")]
        let raw = f64_to_fixed_i128(amount, currency.precision);

        #[cfg(not(feature = "high-precision"))]
        let raw = f64_to_fixed_i64(amount, currency.precision);

        Ok(Self { raw, currency })
    }

    /// Creates a new [`Money`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Money::new_checked`] for more details.
    #[must_use]
    pub fn new(amount: f64, currency: Currency) -> Self {
        Self::new_checked(amount, currency).expect_display(FAILED)
    }

    /// Creates a new [`Money`] instance from the given `raw` fixed-point value and the specified `currency`.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Money::from_raw_checked`] for more details.
    #[must_use]
    pub fn from_raw(raw: MoneyRaw, currency: Currency) -> Self {
        Self::from_raw_checked(raw, currency).expect_display(FAILED)
    }

    /// Creates a new [`Money`] instance from the given `raw` fixed-point value and the specified
    /// `currency` with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `raw` is outside the representable range [`MONEY_RAW_MIN`, `MONEY_RAW_MAX`].
    /// - `currency.precision` exceeds the maximum fixed precision.
    pub fn from_raw_checked(raw: MoneyRaw, currency: Currency) -> CorrectnessResult<Self> {
        if raw < MONEY_RAW_MIN || raw > MONEY_RAW_MAX {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "`raw` value {raw} exceeded bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}] for Money"
                ),
            });
        }

        check_fixed_precision(currency.precision)?;

        // TODO: Enforce spurious bits validation in v2
        // Validate raw value has no spurious bits beyond the precision scale
        // if raw != 0 {
        //     #[cfg(feature = "high-precision")]
        //     super::fixed::check_fixed_raw_i128(raw, currency.precision)?;
        //     #[cfg(not(feature = "high-precision"))]
        //     super::fixed::check_fixed_raw_i64(raw, currency.precision)?;
        // }

        Ok(Self { raw, currency })
    }

    /// Creates a new [`Money`] from a mantissa/exponent pair using pure integer arithmetic.
    ///
    /// The value is `mantissa * 10^exponent`. This avoids all floating-point and Decimal
    /// operations, making it ideal for exchange data that arrives as mantissa/exponent pairs.
    ///
    /// # Panics
    ///
    /// Panics if the resulting raw value exceeds [`MONEY_RAW_MAX`] or [`MONEY_RAW_MIN`].
    #[must_use]
    pub fn from_mantissa_exponent(mantissa: i64, exponent: i8, currency: Currency) -> Self {
        check_fixed_precision(currency.precision).expect_display(FAILED);

        if mantissa == 0 {
            return Self { raw: 0, currency };
        }

        let raw_i128 =
            mantissa_exponent_to_fixed_i128(i128::from(mantissa), exponent, currency.precision)
                .expect("Overflow in Money::from_mantissa_exponent");

        #[allow(
            clippy::useless_conversion,
            reason = "i128 to MoneyRaw is real when not high-precision"
        )]
        let raw: MoneyRaw = raw_i128
            .try_into()
            .expect("Raw value exceeds MoneyRaw range in Money::from_mantissa_exponent");
        assert!(
            raw >= MONEY_RAW_MIN && raw <= MONEY_RAW_MAX,
            "`raw` value {raw} exceeded bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}] for Money"
        );

        Self { raw, currency }
    }

    /// Creates a new [`Money`] instance with a value of zero with the given [`Currency`].
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`Money::new_checked`] for more details.
    #[must_use]
    pub fn zero(currency: Currency) -> Self {
        Self::new(0.0, currency)
    }

    /// Returns a copy with raw value rounded to currency precision,
    /// stripping any sub-scale bits.
    #[must_use]
    pub fn normalized(&self) -> Self {
        #[cfg(feature = "high-precision")]
        let raw = super::fixed::correct_raw_i128(self.raw, self.currency.precision);

        #[cfg(not(feature = "high-precision"))]
        let raw = super::fixed::correct_raw_i64(self.raw, self.currency.precision);

        Self {
            raw,
            currency: self.currency,
        }
    }

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    /// Returns `true` if the value of this instance is positive (> 0).
    #[must_use]
    pub fn is_positive(&self) -> bool {
        self.raw > 0
    }

    /// Performs a checked addition, returning `None` on raw integer overflow, when
    /// the result falls outside `[MONEY_RAW_MIN, MONEY_RAW_MAX]`, or when the operands
    /// have mixed raw scales (e.g. a wei-scaled `Money` and a `FIXED_SCALAR`-scaled
    /// `Money`, even if their currency codes match).
    ///
    /// # Panics
    ///
    /// Panics if `self.currency` and `rhs.currency` differ by code (currency mismatch
    /// is a type-system invariant violation, not a recoverable arithmetic condition).
    #[must_use]
    pub fn checked_add(self, rhs: Self) -> Option<Self> {
        assert_eq!(
            self.currency, rhs.currency,
            "Currency mismatch: cannot add {} to {}",
            rhs.currency.code, self.currency.code
        );

        if !raw_scales_match(self.currency.precision, rhs.currency.precision) {
            return None;
        }
        let raw = self.raw.checked_add(rhs.raw)?;
        if raw < MONEY_RAW_MIN || raw > MONEY_RAW_MAX {
            return None;
        }
        Some(Self {
            raw,
            currency: self.currency,
        })
    }

    /// Performs a checked subtraction, returning `None` on raw integer underflow, when
    /// the result falls outside `[MONEY_RAW_MIN, MONEY_RAW_MAX]`, or when the operands
    /// have mixed raw scales (e.g. a wei-scaled `Money` and a `FIXED_SCALAR`-scaled
    /// `Money`, even if their currency codes match).
    ///
    /// # Panics
    ///
    /// Panics if `self.currency` and `rhs.currency` differ by code (currency mismatch
    /// is a type-system invariant violation, not a recoverable arithmetic condition).
    #[must_use]
    pub fn checked_sub(self, rhs: Self) -> Option<Self> {
        assert_eq!(
            self.currency, rhs.currency,
            "Currency mismatch: cannot subtract {} from {}",
            rhs.currency.code, self.currency.code
        );

        if !raw_scales_match(self.currency.precision, rhs.currency.precision) {
            return None;
        }
        let raw = self.raw.checked_sub(rhs.raw)?;
        if raw < MONEY_RAW_MIN || raw > MONEY_RAW_MAX {
            return None;
        }
        Some(Self {
            raw,
            currency: self.currency,
        })
    }

    #[cfg(feature = "high-precision")]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond `MAX_FLOAT_PRECISION` (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        assert!(
            self.currency.precision <= MAX_FLOAT_PRECISION,
            "Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)"
        );

        fixed_i128_to_f64(self.raw)
    }

    #[cfg(not(feature = "high-precision"))]
    /// Returns the value of this instance as an `f64`.
    ///
    /// # Panics
    ///
    /// Panics if precision is beyond `MAX_FLOAT_PRECISION` (16).
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        #[cfg(feature = "defi")]
        if self.currency.precision > MAX_FLOAT_PRECISION {
            panic!("Invalid f64 conversion beyond `MAX_FLOAT_PRECISION` (16)");
        }

        fixed_i64_to_f64(self.raw)
    }

    /// Returns the value of this instance as a `Decimal`.
    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let precision = self.currency.precision;
        let precision_diff = FIXED_PRECISION.saturating_sub(precision);

        // Money's raw value is stored at fixed precision scale, but needs to be adjusted
        // to the currency's actual precision for decimal conversion.
        let rescaled_raw = self.raw / MoneyRaw::pow(10, u32::from(precision_diff));

        #[allow(
            clippy::useless_conversion,
            reason = "i128::from is real when MoneyRaw is i64"
        )]
        Decimal::from_i128_with_scale(i128::from(rescaled_raw), u32::from(precision))
    }

    /// Returns a formatted string representation of this instance.
    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        let amount_str = format!("{:.*}", self.currency.precision as usize, self.as_f64())
            .separate_with_underscores();
        format!("{} {}", amount_str, self.currency.code)
    }

    /// Creates a new [`Money`] from a `Decimal` value with specified currency.
    ///
    /// This method provides more reliable parsing by using Decimal arithmetic
    /// to avoid floating-point precision issues during conversion.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The decimal value cannot be converted to the raw representation.
    /// - Overflow occurs during scaling.
    pub fn from_decimal(decimal: Decimal, currency: Currency) -> CorrectnessResult<Self> {
        let exponent = -(decimal.scale() as i8);
        let raw_i128 =
            mantissa_exponent_to_fixed_i128(decimal.mantissa(), exponent, currency.precision)?;

        #[allow(
            clippy::useless_conversion,
            reason = "i128 to MoneyRaw is real when not high-precision"
        )]
        let raw: MoneyRaw =
            raw_i128
                .try_into()
                .map_err(|_| CorrectnessError::PredicateViolation {
                    message: format!(
                        "Decimal value exceeds MoneyRaw range [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}]"
                    ),
                })?;

        if !(raw >= MONEY_RAW_MIN && raw <= MONEY_RAW_MAX) {
            return Err(CorrectnessError::PredicateViolation {
                message: format!(
                    "Raw value {raw} exceeded bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}] for Money"
                ),
            });
        }

        Ok(Self { raw, currency })
    }
}

impl FromStr for Money {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = value.split_whitespace().collect();

        // Ensure we have both the amount and currency
        if parts.len() != 2 {
            return Err(format!(
                "Error invalid input format '{value}'. Expected '<amount> <currency>'"
            ));
        }

        let clean_amount = parts[0].replace('_', "");

        let decimal = if clean_amount.contains('e') || clean_amount.contains('E') {
            Decimal::from_scientific(&clean_amount)
                .map_err(|e| format!("Error parsing amount '{}' as Decimal: {e}", parts[0]))?
        } else {
            Decimal::from_str(&clean_amount)
                .map_err(|e| format!("Error parsing amount '{}' as Decimal: {e}", parts[0]))?
        };

        let currency = Currency::from_str(parts[1]).map_err(|e: anyhow::Error| e.to_string())?;
        Self::from_decimal(decimal, currency).map_err(|e| e.to_string())
    }
}

impl<T: AsRef<str>> From<T> for Money {
    fn from(value: T) -> Self {
        Self::from_str(value.as_ref()).expect(FAILED)
    }
}

impl From<Money> for f64 {
    fn from(money: Money) -> Self {
        money.as_f64()
    }
}

impl From<&Money> for f64 {
    fn from(money: &Money) -> Self {
        money.as_f64()
    }
}

impl Hash for Money {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state);
        self.currency.hash(state);
    }
}

impl PartialEq for Money {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw && self.currency == other.currency
    }
}

impl PartialOrd for Money {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }

    fn lt(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.raw.lt(&other.raw)
    }

    fn le(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.raw.le(&other.raw)
    }

    fn gt(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.raw.gt(&other.raw)
    }

    fn ge(&self, other: &Self) -> bool {
        assert_eq!(self.currency, other.currency);
        self.raw.ge(&other.raw)
    }
}

impl Ord for Money {
    fn cmp(&self, other: &Self) -> Ordering {
        assert_eq!(self.currency, other.currency);
        self.raw.cmp(&other.raw)
    }
}

impl Neg for Money {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Self {
            raw: -self.raw,
            currency: self.currency,
        }
    }
}

impl Add for Money {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        assert_eq!(
            self.currency, rhs.currency,
            "Currency mismatch: cannot add {} to {}",
            rhs.currency.code, self.currency.code
        );
        Self {
            raw: self
                .raw
                .checked_add(rhs.raw)
                .expect("Overflow occurred when adding `Money`"),
            currency: self.currency,
        }
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        assert_eq!(
            self.currency, rhs.currency,
            "Currency mismatch: cannot subtract {} from {}",
            rhs.currency.code, self.currency.code
        );
        Self {
            raw: self
                .raw
                .checked_sub(rhs.raw)
                .expect("Underflow occurred when subtracting `Money`"),
            currency: self.currency,
        }
    }
}

impl Add<Decimal> for Money {
    type Output = Decimal;
    fn add(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() + rhs
    }
}

impl Sub<Decimal> for Money {
    type Output = Decimal;
    fn sub(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() - rhs
    }
}

impl Mul<Decimal> for Money {
    type Output = Decimal;
    fn mul(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() * rhs
    }
}

impl Div<Decimal> for Money {
    type Output = Decimal;
    fn div(self, rhs: Decimal) -> Self::Output {
        self.as_decimal() / rhs
    }
}

impl Add<f64> for Money {
    type Output = f64;
    fn add(self, rhs: f64) -> Self::Output {
        self.as_f64() + rhs
    }
}

impl Sub<f64> for Money {
    type Output = f64;
    fn sub(self, rhs: f64) -> Self::Output {
        self.as_f64() - rhs
    }
}

impl Mul<f64> for Money {
    type Output = f64;
    fn mul(self, rhs: f64) -> Self::Output {
        self.as_f64() * rhs
    }
}

impl Div<f64> for Money {
    type Output = f64;
    fn div(self, rhs: f64) -> Self::Output {
        self.as_f64() / rhs
    }
}

impl Debug for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.currency.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{}({}, {})", stringify!(Money), self.raw, self.currency)
        } else {
            write!(
                f,
                "{}({:.*}, {})",
                stringify!(Money),
                self.currency.precision as usize,
                self.as_f64(),
                self.currency
            )
        }
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.currency.precision > crate::types::fixed::MAX_FLOAT_PRECISION {
            write!(f, "{} {}", self.raw, self.currency)
        } else {
            write!(f, "{} {}", self.as_decimal(), self.currency)
        }
    }
}

impl Serialize for Money {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for Money {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let money_str: String = Deserialize::deserialize(deserializer)?;
        Ok(Self::from(money_str.as_str()))
    }
}

/// Checks if the money `value` is positive.
///
/// # Errors
///
/// Returns an error if `value` is not positive.
#[inline(always)]
pub fn check_positive_money(value: Money, param: &str) -> CorrectnessResult<()> {
    if value.raw <= 0 {
        return Err(CorrectnessError::NotPositive {
            param: param.to_string(),
            value: value.to_string(),
            type_name: "`Money`",
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use nautilus_core::{approx_eq, correctness::CorrectnessError};
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_debug() {
        let money = Money::new(1010.12, Currency::USD());
        let result = format!("{money:?}");
        let expected = "Money(1010.12, USD)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_display() {
        let money = Money::new(1010.12, Currency::USD());
        let result = format!("{money}");
        let expected = "1010.12 USD";
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case(1010.12, 2, "USD", "Money(1010.12, USD)", "1010.12 USD")] // Normal precision
    #[case(123.456_789, 8, "BTC", "Money(123.45678900, BTC)", "123.45678900 BTC")] // At max normal precision
    fn test_formatting_normal_precision(
        #[case] value: f64,
        #[case] precision: u8,
        #[case] currency_code: &str,
        #[case] expected_debug: &str,
        #[case] expected_display: &str,
    ) {
        use crate::enums::CurrencyType;
        let currency = Currency::new(
            currency_code,
            precision,
            0,
            currency_code,
            CurrencyType::Fiat,
        );
        let money = Money::new(value, currency);

        assert_eq!(format!("{money:?}"), expected_debug);
        assert_eq!(format!("{money}"), expected_display);
    }

    #[rstest]
    #[cfg(feature = "defi")]
    #[case(
        1_000_000_000_000_000_000_i128,
        18,
        "wei",
        "Money(1000000000000000000, wei)",
        "1000000000000000000 wei"
    )] // High precision
    #[case(
        2_500_000_000_000_000_000_i128,
        18,
        "ETH",
        "Money(2500000000000000000, ETH)",
        "2500000000000000000 ETH"
    )] // High precision
    fn test_formatting_high_precision(
        #[case] raw_value: i128,
        #[case] precision: u8,
        #[case] currency_code: &str,
        #[case] expected_debug: &str,
        #[case] expected_display: &str,
    ) {
        use crate::enums::CurrencyType;
        let currency = Currency::new(
            currency_code,
            precision,
            0,
            currency_code,
            CurrencyType::Crypto,
        );
        let money = Money::from_raw(raw_value, currency);

        assert_eq!(format!("{money:?}"), expected_debug);
        assert_eq!(format!("{money}"), expected_display);
    }

    #[rstest]
    fn test_zero_constructor() {
        let usd = Currency::USD();
        let money = Money::zero(usd);
        assert_eq!(money.raw, 0);
        assert_eq!(money.currency, usd);
    }

    #[rstest]
    #[should_panic(expected = "Currency mismatch")]
    fn test_money_different_currency_addition() {
        let usd = Money::new(1000.0, Currency::USD());
        let btc = Money::new(1.0, Currency::BTC());
        let _ = usd + btc; // This should panic since currencies are different
    }

    #[rstest] // Test does not panic rather than exact value
    fn test_with_maximum_value() {
        let money = Money::new_checked(MONEY_MAX, Currency::USD());
        assert!(money.is_ok());
    }

    #[rstest] // Test does not panic rather than exact value
    fn test_with_minimum_value() {
        let money = Money::new_checked(MONEY_MIN, Currency::USD());
        assert!(money.is_ok());
    }

    #[rstest]
    fn test_new_checked_returns_typed_error_with_stable_display() {
        let error = Money::new_checked(MONEY_MAX + 1.0, Currency::USD()).unwrap_err();

        assert!(matches!(error, CorrectnessError::OutOfRange { .. }));
        assert_eq!(
            error.to_string(),
            format!(
                "invalid f64 for 'amount' not in range [{MONEY_MIN}, {MONEY_MAX}], was {}",
                MONEY_MAX + 1.0
            )
        );
    }

    #[rstest]
    fn test_money_is_zero() {
        let zero_usd = Money::new(0.0, Currency::USD());
        assert!(zero_usd.is_zero());
        assert_eq!(zero_usd, Money::from("0.0 USD"));

        let non_zero_usd = Money::new(100.0, Currency::USD());
        assert!(!non_zero_usd.is_zero());
    }

    #[rstest]
    fn test_money_is_positive() {
        let usd = Currency::USD();
        assert!(Money::new(100.0, usd).is_positive());
        assert!(!Money::new(0.0, usd).is_positive());
        assert!(!Money::new(-100.0, usd).is_positive());
    }

    #[rstest]
    fn test_money_comparisons() {
        let usd = Currency::USD();
        let m1 = Money::new(100.0, usd);
        let m2 = Money::new(200.0, usd);

        assert!(m1 < m2);
        assert!(m2 > m1);
        assert!(m1 <= m2);
        assert!(m2 >= m1);

        // Equality
        let m3 = Money::new(100.0, usd);
        assert!(m1 == m3);
    }

    #[rstest]
    fn test_add() {
        let a = 1000.0;
        let b = 500.0;
        let money1 = Money::new(a, Currency::USD());
        let money2 = Money::new(b, Currency::USD());
        let money3 = money1 + money2;
        assert_eq!(money3.raw, Money::new(a + b, Currency::USD()).raw);
    }

    #[rstest]
    fn test_sub() {
        let usd = Currency::USD();
        let money1 = Money::new(1000.0, usd);
        let money2 = Money::new(250.0, usd);
        let result = money1 - money2;
        assert!(approx_eq!(f64, result.as_f64(), 750.0, epsilon = 1e-9));
        assert_eq!(result.currency, usd);
    }

    #[rstest]
    fn test_money_checked_add_within_bounds() {
        let usd = Currency::USD();
        let a = Money::new(100.0, usd);
        let b = Money::new(50.0, usd);
        assert_eq!(a.checked_add(b), Some(Money::new(150.0, usd)));
    }

    #[rstest]
    fn test_money_checked_add_above_max_returns_none() {
        let usd = Currency::USD();
        let near_max = Money::from_raw(MONEY_RAW_MAX, usd);
        let one = Money::new(1.0, usd);
        assert_eq!(near_max.checked_add(one), None);
    }

    #[rstest]
    fn test_money_checked_sub_within_bounds() {
        let usd = Currency::USD();
        let a = Money::new(100.0, usd);
        let b = Money::new(40.0, usd);
        assert_eq!(a.checked_sub(b), Some(Money::new(60.0, usd)));
    }

    #[rstest]
    fn test_money_checked_sub_below_min_returns_none() {
        let usd = Currency::USD();
        let near_min = Money::from_raw(MONEY_RAW_MIN, usd);
        let one = Money::new(1.0, usd);
        assert_eq!(near_min.checked_sub(one), None);
    }

    #[rstest]
    #[should_panic(expected = "Currency mismatch")]
    fn test_money_checked_add_currency_mismatch_panics() {
        let usd = Money::new(100.0, Currency::USD());
        let aud = Money::new(50.0, Currency::AUD());
        let _ = usd.checked_add(aud);
    }

    #[rstest]
    #[should_panic(expected = "Currency mismatch")]
    fn test_money_checked_sub_currency_mismatch_panics() {
        let usd = Money::new(100.0, Currency::USD());
        let aud = Money::new(50.0, Currency::AUD());
        let _ = usd.checked_sub(aud);
    }

    #[rstest]
    fn test_money_checked_add_at_exact_max_returns_some() {
        let usd = Currency::USD();
        let near_max = Money::from_raw(MONEY_RAW_MAX - 1, usd);
        let one_unit = Money::from_raw(1, usd);
        assert_eq!(
            near_max.checked_add(one_unit),
            Some(Money::from_raw(MONEY_RAW_MAX, usd)),
        );
    }

    #[rstest]
    fn test_money_checked_sub_at_exact_min_returns_some() {
        let usd = Currency::USD();
        let near_min = Money::from_raw(MONEY_RAW_MIN + 1, usd);
        let one_unit = Money::from_raw(1, usd);
        assert_eq!(
            near_min.checked_sub(one_unit),
            Some(Money::from_raw(MONEY_RAW_MIN, usd)),
        );
    }

    #[rstest]
    fn test_money_negation() {
        let money = Money::new(100.0, Currency::USD());
        let result = -money;
        assert_eq!(result, Money::from("-100.0 USD"));
        assert_eq!(result.currency, Currency::USD().clone());
    }

    #[rstest]
    fn test_money_addition_decimal() {
        let money = Money::new(100.0, Currency::USD());
        let result = money + dec!(50.25);
        assert_eq!(result, dec!(150.25));
    }

    #[rstest]
    fn test_money_subtraction_decimal() {
        let money = Money::new(100.0, Currency::USD());
        let result = money - dec!(30.50);
        assert_eq!(result, dec!(69.50));
    }

    #[rstest]
    fn test_money_multiplication_decimal() {
        let money = Money::new(100.0, Currency::USD());
        let result = money * dec!(1.5);
        assert_eq!(result, dec!(150.00));
    }

    #[rstest]
    fn test_money_division_decimal() {
        let money = Money::new(100.0, Currency::USD());
        let result = money / dec!(4);
        assert_eq!(result, dec!(25.00));
    }

    #[rstest]
    fn test_money_addition_f64() {
        let money = Money::new(100.0, Currency::USD());
        let result = money + 50.25;
        assert!(approx_eq!(f64, result, 150.25, epsilon = 1e-9));
    }

    #[rstest]
    fn test_money_subtraction_f64() {
        let money = Money::new(100.0, Currency::USD());
        let result = money - 30.50;
        assert!(approx_eq!(f64, result, 69.50, epsilon = 1e-9));
    }

    #[rstest]
    fn test_money_multiplication_f64() {
        let money = Money::new(100.0, Currency::USD());
        let result = money * 1.5;
        assert!(approx_eq!(f64, result, 150.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_money_division_f64() {
        let money = Money::new(100.0, Currency::USD());
        let result = money / 4.0;
        assert!(approx_eq!(f64, result, 25.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_money_new_usd() {
        let money = Money::new(1000.0, Currency::USD());
        assert_eq!(money.currency.code.as_str(), "USD");
        assert_eq!(money.currency.precision, 2);
        assert_eq!(money.to_string(), "1000.00 USD");
        assert_eq!(money.to_formatted_string(), "1_000.00 USD");
        assert_eq!(money.as_decimal(), dec!(1000.00));
        assert!(approx_eq!(f64, money.as_f64(), 1000.0, epsilon = 0.001));
    }

    #[rstest]
    fn test_money_new_btc() {
        let money = Money::new(10.3, Currency::BTC());
        assert_eq!(money.currency.code.as_str(), "BTC");
        assert_eq!(money.currency.precision, 8);
        assert_eq!(money.to_string(), "10.30000000 BTC");
        assert_eq!(money.to_formatted_string(), "10.30000000 BTC");
    }

    #[rstest]
    #[case("0USD")] // <-- No whitespace separator
    #[case("0x00 USD")] // <-- Invalid float
    #[case("0 US")] // <-- Invalid currency
    #[case("0 USD USD")] // <-- Too many parts
    #[should_panic(expected = "Condition failed")]
    fn test_from_str_invalid_input(#[case] input: &str) {
        let _ = Money::from(input);
    }

    #[rstest]
    #[case("0 USD", Currency::USD(), dec!(0.00))]
    #[case("1.1 AUD", Currency::AUD(), dec!(1.10))]
    #[case("1.12345678 BTC", Currency::BTC(), dec!(1.12345678))]
    #[case("10_000.10 USD", Currency::USD(), dec!(10000.10))]
    fn test_from_str_valid_input(
        #[case] input: &str,
        #[case] expected_currency: Currency,
        #[case] expected_dec: Decimal,
    ) {
        let money = Money::from(input);
        assert_eq!(money.currency, expected_currency);
        assert_eq!(money.as_decimal(), expected_dec);
    }

    #[rstest]
    fn test_money_from_str_negative() {
        let money = Money::from("-123.45 USD");
        assert!(approx_eq!(f64, money.as_f64(), -123.45, epsilon = 1e-9));
        assert_eq!(money.currency, Currency::USD());
    }

    #[rstest]
    #[case("1e7 USD", 10_000_000.0)]
    #[case("2.5e3 EUR", 2_500.0)]
    #[case("1.234e-2 GBP", 0.01)] // GBP has 2 decimal places, so 0.01234 becomes 0.01
    #[case("5E-3 JPY", 0.0)] // JPY has 0 decimal places, so 0.005 becomes 0
    fn test_from_str_scientific_notation(#[case] input: &str, #[case] expected_value: f64) {
        let money = Money::from_str(input).unwrap();
        assert!(approx_eq!(
            f64,
            money.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    #[case("1_234.56 USD", 1234.56)]
    #[case("1_000_000 EUR", 1_000_000.0)]
    #[case("99_999.99 GBP", 99_999.99)]
    fn test_from_str_with_underscores(#[case] input: &str, #[case] expected_value: f64) {
        let money = Money::from_str(input).unwrap();
        assert!(approx_eq!(
            f64,
            money.as_f64(),
            expected_value,
            epsilon = 1e-10
        ));
    }

    #[rstest]
    fn test_from_decimal_precision_preservation() {
        use rust_decimal::Decimal;

        let decimal = Decimal::from_str("123.45").unwrap();
        let money = Money::from_decimal(decimal, Currency::USD()).unwrap();
        assert_eq!(money.currency.precision, 2);
        assert!(approx_eq!(f64, money.as_f64(), 123.45, epsilon = 1e-10));

        // Verify raw value is exact for USD (2 decimal places)
        let expected_raw = 12345 * 10_i64.pow(u32::from(FIXED_PRECISION - 2));
        assert_eq!(money.raw, MoneyRaw::from(expected_raw));
    }

    #[rstest]
    fn test_from_decimal_rounding() {
        use rust_decimal::Decimal;

        // Test banker's rounding with USD (2 decimal places)
        let decimal = Decimal::from_str("1.005").unwrap();
        let money = Money::from_decimal(decimal, Currency::USD()).unwrap();
        assert_eq!(money.as_f64(), 1.0); // 1.005 rounds to 1.00 (even)

        let decimal = Decimal::from_str("1.015").unwrap();
        let money = Money::from_decimal(decimal, Currency::USD()).unwrap();
        assert_eq!(money.as_f64(), 1.02); // 1.015 rounds to 1.02 (even)
    }

    #[rstest]
    fn test_money_hash() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let m1 = Money::new(100.0, Currency::USD());
        let m2 = Money::new(100.0, Currency::USD());
        let m3 = Money::new(100.0, Currency::AUD());

        let mut s1 = DefaultHasher::new();
        let mut s2 = DefaultHasher::new();
        let mut s3 = DefaultHasher::new();

        m1.hash(&mut s1);
        m2.hash(&mut s2);
        m3.hash(&mut s3);

        assert_eq!(
            s1.finish(),
            s2.finish(),
            "Same amount + same currency => same hash"
        );
        assert_ne!(
            s1.finish(),
            s3.finish(),
            "Same amount + different currency => different hash"
        );
    }

    #[rstest]
    fn test_money_serialization_deserialization() {
        let money = Money::new(123.45, Currency::USD());
        let serialized = serde_json::to_string(&money);
        let deserialized: Money = serde_json::from_str(&serialized.unwrap()).unwrap();
        assert_eq!(money, deserialized);
    }

    #[rstest]
    #[should_panic(expected = "`raw` value")]
    fn test_money_from_raw_out_of_range_panics() {
        let usd = Currency::USD();
        let raw = MONEY_RAW_MAX.saturating_add(1);
        let _ = Money::from_raw(raw, usd);
    }

    #[rstest]
    fn test_money_from_raw_checked_valid() {
        let usd = Currency::USD();
        let money = Money::from_raw_checked(123_450_000_000, usd).unwrap();
        assert_eq!(money.currency, usd);
    }

    #[rstest]
    fn test_money_from_raw_checked_above_max_returns_error() {
        let usd = Currency::USD();
        let raw = MONEY_RAW_MAX.saturating_add(1);
        let error = Money::from_raw_checked(raw, usd).unwrap_err();
        assert!(matches!(error, CorrectnessError::PredicateViolation { .. }));
    }

    #[rstest]
    fn test_money_from_raw_checked_below_min_returns_error() {
        let usd = Currency::USD();
        let raw = MONEY_RAW_MIN.saturating_sub(1);
        let error = Money::from_raw_checked(raw, usd).unwrap_err();
        assert!(matches!(error, CorrectnessError::PredicateViolation { .. }));
    }

    #[rstest]
    fn test_from_decimal_rejects_out_of_range() {
        let huge = Decimal::from_str("99999999999999999999.99").unwrap();
        let result = Money::from_decimal(huge, Currency::USD());
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_decimal_out_of_range_returns_typed_error_with_stable_display() {
        let huge = Decimal::from_str("99999999999999999999.99").unwrap();
        let error = Money::from_decimal(huge, Currency::USD()).unwrap_err();
        match error {
            CorrectnessError::PredicateViolation { ref message } => {
                assert!(
                    message.contains("MoneyRaw range") || message.contains("Money"),
                    "unexpected message: {message:?}",
                );
            }
            _ => panic!("expected PredicateViolation, was {error:?}"),
        }
    }

    #[rstest]
    fn test_from_mantissa_exponent_exact_precision() {
        let money = Money::from_mantissa_exponent(12345, -2, Currency::USD());
        assert_eq!(money.as_f64(), 123.45);
    }

    #[rstest]
    fn test_from_mantissa_exponent_excess_rounds_down() {
        // 12.345 rounds to 12.34 (4 is even, banker's rounding)
        let money = Money::from_mantissa_exponent(12345, -3, Currency::USD());
        assert_eq!(money.as_f64(), 12.34);
    }

    #[rstest]
    fn test_from_mantissa_exponent_excess_rounds_up() {
        // 12.355 rounds to 12.36 (5 is odd, banker's rounding)
        let money = Money::from_mantissa_exponent(12355, -3, Currency::USD());
        assert_eq!(money.as_f64(), 12.36);
    }

    #[rstest]
    fn test_from_mantissa_exponent_positive_exponent() {
        let money = Money::from_mantissa_exponent(5, 2, Currency::USD());
        assert_eq!(money.as_f64(), 500.0);
    }

    #[rstest]
    #[should_panic(expected = "Overflow")]
    fn test_from_mantissa_exponent_overflow_panics() {
        let _ = Money::from_mantissa_exponent(i64::MAX, 9, Currency::USD());
    }

    #[rstest]
    #[should_panic(expected = "exceeds i128 range")]
    fn test_from_mantissa_exponent_large_exponent_panics() {
        let _ = Money::from_mantissa_exponent(1, 119, Currency::USD());
    }

    #[rstest]
    fn test_from_mantissa_exponent_zero_with_large_exponent() {
        let money = Money::from_mantissa_exponent(0, 119, Currency::USD());
        assert_eq!(money.as_f64(), 0.0);
    }

    #[rstest]
    fn test_from_mantissa_exponent_very_negative_exponent_rounds_to_zero() {
        // exponent=-120, frac_digits=120, excess=118 for USD (precision 2)
        let money = Money::from_mantissa_exponent(12345, -120, Currency::USD());
        assert_eq!(money.as_f64(), 0.0);
    }

    #[rstest]
    #[case(42.0, true, "positive value")]
    #[case(0.0, false, "zero value")]
    #[case( -13.5,  false, "negative value")]
    #[allow(clippy::used_underscore_binding)]
    fn test_check_positive_money(
        #[case] amount: f64,
        #[case] should_succeed: bool,
        #[case] _case_name: &str,
    ) {
        let money = Money::new(amount, Currency::USD());

        let res = check_positive_money(money, "money");

        if should_succeed {
            assert!(res.is_ok(), "expected Ok(..) for {amount}");
        } else {
            assert!(res.is_err(), "expected Err(..) for {amount}");
            let msg = res.unwrap_err().to_string();
            assert!(
                msg.contains("not positive"),
                "error message should mention positivity; got: {msg:?}"
            );
        }
    }

    #[rstest]
    fn test_check_positive_money_returns_typed_error_with_stable_display() {
        let error = check_positive_money(Money::new(0.0, Currency::USD()), "money").unwrap_err();

        assert_eq!(
            error,
            CorrectnessError::NotPositive {
                param: "money".to_string(),
                value: "0.00 USD".to_string(),
                type_name: "`Money`",
            }
        );
        assert_eq!(
            error.to_string(),
            "invalid `Money` for 'money' not positive, was 0.00 USD"
        );
    }
}

#[cfg(test)]
mod property_tests {
    use proptest::prelude::*;
    use rstest::rstest;

    use super::*;

    fn currency_strategy() -> impl Strategy<Value = Currency> {
        prop_oneof![
            Just(Currency::USD()),
            Just(Currency::EUR()),
            Just(Currency::GBP()),
            Just(Currency::JPY()),
            Just(Currency::AUD()),
            Just(Currency::CAD()),
            Just(Currency::CHF()),
            Just(Currency::BTC()),
            Just(Currency::ETH()),
            Just(Currency::USDT()),
        ]
    }

    fn money_amount_strategy() -> impl Strategy<Value = f64> {
        prop_oneof![
            -1000.0..1000.0,
            -100_000.0..100_000.0,
            -1_000_000.0..1_000_000.0,
            Just(0.0),
            Just(MONEY_MIN / 2.0),
            Just(MONEY_MAX / 2.0),
            Just(MONEY_MIN + 1.0),
            Just(MONEY_MAX - 1.0),
            Just(MONEY_MIN),
            Just(MONEY_MAX),
        ]
    }

    fn money_strategy() -> impl Strategy<Value = Money> {
        (money_amount_strategy(), currency_strategy())
            .prop_filter_map("constructible money", |(amount, currency)| {
                Money::new_checked(amount, currency).ok()
            })
    }

    proptest! {
        #[rstest]
        fn prop_money_construction_roundtrip(
            amount in money_amount_strategy(),
            currency in currency_strategy()
        ) {
            if let Ok(money) = Money::new_checked(amount, currency) {
                let roundtrip = money.as_f64();
                let precision_epsilon = if currency.precision == 0 {
                    1.0
                } else {
                    let currency_epsilon = 10.0_f64.powi(-i32::from(currency.precision));
                    let magnitude_epsilon = amount.abs() * 1e-10;
                    currency_epsilon.max(magnitude_epsilon)
                };
                prop_assert!((roundtrip - amount).abs() <= precision_epsilon,
                    "Roundtrip failed: {} -> {} -> {} (precision: {}, epsilon: {})",
                    amount, money.raw, roundtrip, currency.precision, precision_epsilon);
                prop_assert_eq!(money.currency, currency);
            }
        }

        #[rstest]
        fn prop_money_addition_commutative(
            money1 in money_strategy(),
            money2 in money_strategy(),
        ) {
            if money1.currency == money2.currency
                && let (Some(_), Some(_)) = (
                    money1.raw.checked_add(money2.raw),
                    money2.raw.checked_add(money1.raw)
                )
            {
                let sum1 = money1 + money2;
                let sum2 = money2 + money1;
                prop_assert_eq!(sum1, sum2, "Addition should be commutative");
                prop_assert_eq!(sum1.currency, money1.currency);
            }
        }

        #[rstest]
        fn prop_money_addition_associative(
            money1 in money_strategy(),
            money2 in money_strategy(),
            money3 in money_strategy(),
        ) {
            if money1.currency == money2.currency
                && money2.currency == money3.currency
                && let (Some(sum1), Some(sum2)) = (
                    money1.raw.checked_add(money2.raw),
                    money2.raw.checked_add(money3.raw)
                )
                && let (Some(left), Some(right)) = (
                    sum1.checked_add(money3.raw),
                    money1.raw.checked_add(sum2)
                )
                && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&left)
                && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&right)
            {
                let left_result = Money::from_raw(left, money1.currency);
                let right_result = Money::from_raw(right, money1.currency);
                prop_assert_eq!(left_result, right_result, "Addition should be associative");
            }
        }

        #[rstest]
        fn prop_money_subtraction_inverse(
            money1 in money_strategy(),
            money2 in money_strategy(),
        ) {
            if money1.currency == money2.currency
                && let Some(sum_raw) = money1.raw.checked_add(money2.raw)
                && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&sum_raw)
            {
                let sum = Money::from_raw(sum_raw, money1.currency);
                let diff = sum - money2;
                prop_assert_eq!(diff, money1, "Subtraction should be inverse of addition");
            }
        }

        /// Property: checked_add agrees with raw checked_add when result is in bounds and
        /// currencies match; returns None when out of bounds.
        #[rstest]
        fn prop_money_checked_add_matches_spec(
            raw1 in MONEY_RAW_MIN..=MONEY_RAW_MAX,
            raw2 in MONEY_RAW_MIN..=MONEY_RAW_MAX,
            currency in currency_strategy(),
        ) {
            let m1 = Money::from_raw(raw1, currency);
            let m2 = Money::from_raw(raw2, currency);
            let expected = m1.raw
                .checked_add(m2.raw)
                .filter(|r| (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(r))
                .map(|raw| Money { raw, currency });
            prop_assert_eq!(m1.checked_add(m2), expected);
        }

        /// Property: checked_sub agrees with raw checked_sub when result is in bounds and
        /// currencies match; returns None when out of bounds.
        #[rstest]
        fn prop_money_checked_sub_matches_spec(
            raw1 in MONEY_RAW_MIN..=MONEY_RAW_MAX,
            raw2 in MONEY_RAW_MIN..=MONEY_RAW_MAX,
            currency in currency_strategy(),
        ) {
            let m1 = Money::from_raw(raw1, currency);
            let m2 = Money::from_raw(raw2, currency);
            let expected = m1.raw
                .checked_sub(m2.raw)
                .filter(|r| (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(r))
                .map(|raw| Money { raw, currency });
            prop_assert_eq!(m1.checked_sub(m2), expected);
        }

        #[rstest]
        fn prop_money_zero_identity(money in money_strategy()) {
            let zero = Money::zero(money.currency);
            prop_assert_eq!(money + zero, money, "Zero should be additive identity");
            prop_assert_eq!(zero + money, money, "Zero should be additive identity (commutative)");
            prop_assert!(zero.is_zero(), "Zero should be recognized as zero");
        }

        #[rstest]
        fn prop_money_negation_inverse(money in money_strategy()) {
            let negated = -money;
            let double_neg = -negated;
            prop_assert_eq!(money, double_neg, "Double negation should equal original");
            prop_assert_eq!(negated.currency, money.currency, "Negation preserves currency");

            if let Some(sum_raw) = money.raw.checked_add(negated.raw)
                && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&sum_raw) {
                    let sum = Money::from_raw(sum_raw, money.currency);
                    prop_assert!(sum.is_zero(), "Money + (-Money) should equal zero");
                }
        }

        #[rstest]
        fn prop_money_comparison_consistency(
            money1 in money_strategy(),
            money2 in money_strategy(),
        ) {
            if money1.currency == money2.currency {
                let eq = money1 == money2;
                let lt = money1 < money2;
                let gt = money1 > money2;
                let le = money1 <= money2;
                let ge = money1 >= money2;

                let exclusive_count = [eq, lt, gt].iter().filter(|&&x| x).count();
                prop_assert_eq!(exclusive_count, 1, "Exactly one of ==, <, > should be true");

                prop_assert_eq!(le, eq || lt, "<= should equal == || <");
                prop_assert_eq!(ge, eq || gt, ">= should equal == || >");
                prop_assert_eq!(lt, money2 > money1, "< should be symmetric with >");
                prop_assert_eq!(le, money2 >= money1, "<= should be symmetric with >=");
            }
        }

        #[rstest]
        fn prop_money_decimal_conversion(money in money_strategy()) {
            let decimal = money.as_decimal();

            // Scale must always match currency precision
            prop_assert_eq!(decimal.scale(), u32::from(money.currency.precision));

            #[cfg(feature = "defi")]
            {
                let decimal_f64: f64 = decimal.try_into().unwrap_or(0.0);
                prop_assert!(decimal_f64.is_finite(), "Decimal should convert to finite f64");
            }
            #[cfg(not(feature = "defi"))]
            {
                let decimal_f64: f64 = decimal.try_into().unwrap_or(0.0);
                let original_f64 = money.as_f64();

                let base_epsilon = 10.0_f64.powi(-(money.currency.precision as i32));
                let precision_epsilon = if cfg!(feature = "high-precision") {
                    base_epsilon.max(1e-10)
                } else {
                    base_epsilon
                };
                let diff = (decimal_f64 - original_f64).abs();
                prop_assert!(diff <= precision_epsilon,
                    "Decimal conversion should preserve value within currency precision: {} vs {} (diff: {}, epsilon: {})",
                    original_f64, decimal_f64, diff, precision_epsilon);
            }
        }

        #[rstest]
        fn prop_money_arithmetic_with_f64(
            money in money_strategy(),
            factor in -1000.0..1000.0_f64,
        ) {
            if factor != 0.0 {
                let original_f64 = money.as_f64();

                let mul_result = money * factor;
                let expected_mul = original_f64 * factor;
                prop_assert!((mul_result - expected_mul).abs() < 0.01,
                    "Multiplication with f64 should be accurate");

                let div_result = money / factor;
                let expected_div = original_f64 / factor;
                if expected_div.is_finite() {
                    prop_assert!((div_result - expected_div).abs() < 0.01,
                        "Division with f64 should be accurate");
                }

                let add_result = money + factor;
                let expected_add = original_f64 + factor;
                prop_assert!((add_result - expected_add).abs() < 0.01,
                    "Addition with f64 should be accurate");

                let sub_result = money - factor;
                let expected_sub = original_f64 - factor;
                prop_assert!((sub_result - expected_sub).abs() < 0.01,
                    "Subtraction with f64 should be accurate");
            }
        }
    }
}
