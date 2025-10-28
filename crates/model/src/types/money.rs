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

//! Represents an amount of money in a specified currency denomination.

use std::{
    cmp::Ordering,
    fmt::{Debug, Display},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::correctness::{FAILED, check_in_range_inclusive_f64, check_predicate_true};
use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

#[cfg(not(any(feature = "defi", feature = "high-precision")))]
use super::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
#[cfg(any(feature = "defi", feature = "high-precision"))]
use super::fixed::{f64_to_fixed_i128, fixed_i128_to_f64};
#[cfg(feature = "defi")]
use crate::types::fixed::MAX_FLOAT_PRECISION;
use crate::types::{
    Currency,
    fixed::{FIXED_PRECISION, FIXED_SCALAR, check_fixed_precision},
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
/// This value is computed at compile time from MONEY_MAX * FIXED_SCALAR.
/// The multiplication is guaranteed not to overflow because MONEY_MAX and FIXED_SCALAR
/// are chosen such that their product fits within MoneyRaw's range in both
/// high-precision (i128) and standard-precision (i64) modes.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static MONEY_RAW_MAX: MoneyRaw = (MONEY_MAX * FIXED_SCALAR) as MoneyRaw;

/// The minimum raw money integer value.
///
/// # Safety
///
/// This value is computed at compile time from MONEY_MIN * FIXED_SCALAR.
/// The multiplication is guaranteed not to overflow because MONEY_MIN and FIXED_SCALAR
/// are chosen such that their product fits within MoneyRaw's range in both
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", frozen)
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
    /// Returns an error if `amount` is invalid outside the representable range [MONEY_MIN, MONEY_MAX].
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked(amount: f64, currency: Currency) -> anyhow::Result<Self> {
        // SAFETY: check_in_range_inclusive_f64 already validates that amount is finite
        // (not NaN or infinite) as part of its range validation logic, so no additional
        // infinity checks are needed here.
        check_in_range_inclusive_f64(amount, MONEY_MIN, MONEY_MAX, "amount")?;

        #[cfg(feature = "defi")]
        if currency.precision > MAX_FLOAT_PRECISION {
            // Floats are only reliable up to ~16 decimal digits of precision regardless of feature flags
            anyhow::bail!(
                "`currency.precision` exceeded maximum float precision ({MAX_FLOAT_PRECISION}), use `Money::from_wei()` for wei values instead"
            );
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
    pub fn new(amount: f64, currency: Currency) -> Self {
        Self::new_checked(amount, currency).expect(FAILED)
    }

    /// Creates a new [`Money`] instance from the given `raw` fixed-point value and the specified `currency`.
    ///
    /// # Panics
    ///
    /// Panics if `raw` is outside the representable range [`MONEY_RAW_MIN`, `MONEY_RAW_MAX`].
    /// Panics if `currency.precision` exceeds [`FIXED_PRECISION`].
    #[must_use]
    pub fn from_raw(raw: MoneyRaw, currency: Currency) -> Self {
        check_predicate_true(
            raw >= MONEY_RAW_MIN && raw <= MONEY_RAW_MAX,
            &format!(
                "`raw` value {raw} exceeded bounds [{}, {}] for Money",
                MONEY_RAW_MIN, MONEY_RAW_MAX
            ),
        )
        .expect(FAILED);
        check_fixed_precision(currency.precision).expect(FAILED);
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

    /// Returns `true` if the value of this instance is zero.
    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
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
        if self.currency.precision > MAX_FLOAT_PRECISION {
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

        #[allow(clippy::useless_conversion, reason = "Required for precision modes")]
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
    pub fn from_decimal(decimal: Decimal, currency: Currency) -> anyhow::Result<Self> {
        let precision = currency.precision;

        let scale_factor = Decimal::from(10_i64.pow(precision as u32));
        let scaled = decimal * scale_factor;
        let rounded = scaled.round();

        #[cfg(feature = "high-precision")]
        let raw_at_precision: MoneyRaw = rounded.to_i128().ok_or_else(|| {
            anyhow::anyhow!("Decimal value '{decimal}' cannot be converted to i128")
        })?;
        #[cfg(not(feature = "high-precision"))]
        let raw_at_precision: MoneyRaw = rounded.to_i64().ok_or_else(|| {
            anyhow::anyhow!("Decimal value '{decimal}' cannot be converted to i64")
        })?;

        let scale_up = 10_i64.pow((FIXED_PRECISION - precision) as u32) as MoneyRaw;
        let raw = raw_at_precision
            .checked_mul(scale_up)
            .ok_or_else(|| anyhow::anyhow!("Overflow when scaling to fixed precision"))?;

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

impl AddAssign for Money {
    fn add_assign(&mut self, other: Self) {
        assert_eq!(
            self.currency, other.currency,
            "Currency mismatch: cannot add {} to {}",
            other.currency.code, self.currency.code
        );
        self.raw = self
            .raw
            .checked_add(other.raw)
            .expect("Overflow occurred when adding `Money`");
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, other: Self) {
        assert_eq!(
            self.currency, other.currency,
            "Currency mismatch: cannot subtract {} from {}",
            other.currency.code, self.currency.code
        );
        self.raw = self
            .raw
            .checked_sub(other.raw)
            .expect("Underflow occurred when subtracting `Money`");
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
pub fn check_positive_money(value: Money, param: &str) -> anyhow::Result<()> {
    if value.raw <= 0 {
        anyhow::bail!("invalid `Money` for '{param}' not positive, was {value}");
    }
    Ok(())
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
    #[case(123.456789, 8, "BTC", "Money(123.45678900, BTC)", "123.45678900 BTC")] // At max normal precision
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
    #[should_panic]
    fn test_money_different_currency_addition() {
        let usd = Money::new(1000.0, Currency::USD());
        let btc = Money::new(1.0, Currency::BTC());
        let _result = usd + btc; // This should panic since currencies are different
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
    fn test_money_is_zero() {
        let zero_usd = Money::new(0.0, Currency::USD());
        assert!(zero_usd.is_zero());
        assert_eq!(zero_usd, Money::from("0.0 USD"));

        let non_zero_usd = Money::new(100.0, Currency::USD());
        assert!(!non_zero_usd.is_zero());
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
    fn test_add_assign() {
        let usd = Currency::USD();
        let mut money = Money::new(100.0, usd);
        money += Money::new(50.0, usd);
        assert!(approx_eq!(f64, money.as_f64(), 150.0, epsilon = 1e-9));
        assert_eq!(money.currency, usd);
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
    fn test_sub_assign() {
        let usd = Currency::USD();
        let mut money = Money::new(100.0, usd);
        money -= Money::new(25.0, usd);
        assert!(approx_eq!(f64, money.as_f64(), 75.0, epsilon = 1e-9));
        assert_eq!(money.currency, usd);
    }

    #[rstest]
    fn test_money_negation() {
        let money = Money::new(100.0, Currency::USD());
        let result = -money;
        assert_eq!(result, Money::from("-100.0 USD"));
        assert_eq!(result.currency, Currency::USD().clone());
    }

    #[rstest]
    fn test_money_multiplication_by_f64() {
        let money = Money::new(100.0, Currency::USD());
        let result = money * 1.5;
        assert!(approx_eq!(f64, result, 150.0, epsilon = 1e-9));
    }

    #[rstest]
    fn test_money_division_by_f64() {
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
    #[should_panic]
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
        let expected_raw = 12345 * 10_i64.pow((FIXED_PRECISION - 2) as u32);
        assert_eq!(money.raw, expected_raw as MoneyRaw);
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

    ////////////////////////////////////////////////////////////////////////////////
    // Property-based testing
    ////////////////////////////////////////////////////////////////////////////////

    use proptest::prelude::*;

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
        // Generate amounts within valid range, avoiding edge cases that might cause precision issues
        prop_oneof![
            // Small amounts
            -1000.0..1000.0,
            // Medium amounts
            -100_000.0..100_000.0,
            // Large amounts within safe range (avoid max values that could overflow when added)
            -1_000_000.0..1_000_000.0,
            // Edge cases
            Just(0.0),
            // Use smaller values than MONEY_MAX to avoid overflow in arithmetic operations
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
            // Test that valid amounts can be constructed and round-trip through f64
            if let Ok(money) = Money::new_checked(amount, currency) {
                let roundtrip = money.as_f64();
                // Allow for precision loss based on currency precision and magnitude
                let precision_epsilon = if currency.precision == 0 {
                    1.0 // For JPY and other zero-precision currencies, allow rounding to nearest integer
                } else {
                    let currency_epsilon = 10.0_f64.powi(-(currency.precision as i32));
                    let magnitude_epsilon = amount.abs() * 1e-10; // Allow relative error for large numbers
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
            // Addition should be commutative for same currency
            if money1.currency == money2.currency {
                // Check if addition would overflow before performing it
                if let (Some(_), Some(_)) = (
                    money1.raw.checked_add(money2.raw),
                    money2.raw.checked_add(money1.raw)
                ) {
                    let sum1 = money1 + money2;
                    let sum2 = money2 + money1;
                    prop_assert_eq!(sum1, sum2, "Addition should be commutative");
                    prop_assert_eq!(sum1.currency, money1.currency);
                }
                // If overflow would occur, skip this test case - it's expected
            }
        }

        #[rstest]
        fn prop_money_addition_associative(
            money1 in money_strategy(),
            money2 in money_strategy(),
            money3 in money_strategy(),
        ) {
            // Addition should be associative for same currency
            if money1.currency == money2.currency && money2.currency == money3.currency {
                // Test (a + b) + c == a + (b + c)
                // Use checked arithmetic to avoid overflow in property tests
                if let (Some(sum1), Some(sum2)) = (
                    money1.raw.checked_add(money2.raw),
                    money2.raw.checked_add(money3.raw)
                )
                    && let (Some(left), Some(right)) = (
                        sum1.checked_add(money3.raw),
                        money1.raw.checked_add(sum2)
                    ) {
                        // Check if results are within bounds before constructing Money
                        if (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&left)
                            && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&right)
                        {
                            let left_result = Money::from_raw(left, money1.currency);
                            let right_result = Money::from_raw(right, money1.currency);
                            prop_assert_eq!(left_result, right_result, "Addition should be associative");
                        }
                    }
            }
        }

        #[rstest]
        fn prop_money_subtraction_inverse(
            money1 in money_strategy(),
            money2 in money_strategy(),
        ) {
            // Subtraction should be the inverse of addition for same currency
            if money1.currency == money2.currency {
                // Test (a + b) - b == a, avoiding overflow
                if let Some(sum_raw) = money1.raw.checked_add(money2.raw)
                    && (MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&sum_raw) {
                        let sum = Money::from_raw(sum_raw, money1.currency);
                        let diff = sum - money2;
                        prop_assert_eq!(diff, money1, "Subtraction should be inverse of addition");
                    }
            }
        }

        #[rstest]
        fn prop_money_zero_identity(money in money_strategy()) {
            // Zero should be additive identity
            let zero = Money::zero(money.currency);
            prop_assert_eq!(money + zero, money, "Zero should be additive identity");
            prop_assert_eq!(zero + money, money, "Zero should be additive identity (commutative)");
            prop_assert!(zero.is_zero(), "Zero should be recognized as zero");
        }

        #[rstest]
        fn prop_money_negation_inverse(money in money_strategy()) {
            // Negation should be its own inverse
            let negated = -money;
            let double_neg = -negated;
            prop_assert_eq!(money, double_neg, "Double negation should equal original");
            prop_assert_eq!(negated.currency, money.currency, "Negation preserves currency");

            // Test additive inverse property (if no overflow)
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
            // Comparison operations should be consistent for same currency
            if money1.currency == money2.currency {
                let eq = money1 == money2;
                let lt = money1 < money2;
                let gt = money1 > money2;
                let le = money1 <= money2;
                let ge = money1 >= money2;

                // Exactly one of eq, lt, gt should be true
                let exclusive_count = [eq, lt, gt].iter().filter(|&&x| x).count();
                prop_assert_eq!(exclusive_count, 1, "Exactly one of ==, <, > should be true");

                // Consistency checks
                prop_assert_eq!(le, eq || lt, "<= should equal == || <");
                prop_assert_eq!(ge, eq || gt, ">= should equal == || >");
                prop_assert_eq!(lt, money2 > money1, "< should be symmetric with >");
                prop_assert_eq!(le, money2 >= money1, "<= should be symmetric with >=");
            }
        }

        #[rstest]
        fn prop_money_string_roundtrip(money in money_strategy()) {
            // String serialization should round-trip correctly
            let string_repr = money.to_string();
            let parsed = Money::from_str(&string_repr);
            prop_assert!(parsed.is_ok(), "String parsing should succeed for valid money");
            if let Ok(parsed_money) = parsed {
                prop_assert_eq!(parsed_money.currency, money.currency, "Currency should round-trip");
                // Allow for small precision differences due to string formatting
                let diff = (parsed_money.as_f64() - money.as_f64()).abs();
                prop_assert!(diff < 0.01, "Amount should round-trip within precision: {} vs {}",
                    money.as_f64(), parsed_money.as_f64());
            }
        }

        #[rstest]
        fn prop_money_decimal_conversion(money in money_strategy()) {
            // Decimal conversion should preserve value within precision limits
            let decimal = money.as_decimal();

            #[cfg(feature = "defi")]
            {
                // In DeFi mode, as_f64() is unreliable for high-precision values
                // Just ensure decimal conversion doesn't panic and produces reasonable values
                let decimal_f64: f64 = decimal.try_into().unwrap_or(0.0);
                prop_assert!(decimal_f64.is_finite(), "Decimal should convert to finite f64");

                // For DeFi mode, we mainly care that decimal conversion preserves the currency precision
                prop_assert_eq!(decimal.scale(), u32::from(money.currency.precision));
            }
            #[cfg(not(feature = "defi"))]
            {
                let decimal_f64: f64 = decimal.try_into().unwrap_or(0.0);
                let original_f64 = money.as_f64();

                // Allow for precision differences based on currency precision and high-precision mode
                let base_epsilon = 10.0_f64.powi(-(money.currency.precision as i32));
                let precision_epsilon = if cfg!(feature = "high-precision") {
                    // More tolerant epsilon for high-precision modes due to f64 limitations
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
            // Arithmetic with f64 should produce reasonable results
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

    #[rstest]
    #[case(42.0, true, "positive value")]
    #[case(0.0, false, "zero value")]
    #[case( -13.5,  false, "negative value")]
    fn test_check_positive_money(
        #[case] amount: f64,
        #[case] should_succeed: bool,
        #[case] _case_name: &str,
    ) {
        let money = Money::new(amount, Currency::USD());

        let res = check_positive_money(money, "money");

        match should_succeed {
            true => assert!(res.is_ok(), "expected Ok(..) for {amount}"),
            false => {
                assert!(res.is_err(), "expected Err(..) for {amount}");
                let msg = res.unwrap_err().to_string();
                assert!(
                    msg.contains("not positive"),
                    "error message should mention positivity; got: {msg:?}"
                );
            }
        }
    }
}
