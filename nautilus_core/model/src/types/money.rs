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

use std::{
    cmp::Ordering,
    fmt::{Display, Formatter, Result as FmtResult},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::correctness::check_in_range_inclusive_f64;
use rust_decimal::Decimal;
use serde::{Deserialize, Deserializer, Serialize};
use thousands::Separable;

use super::fixed::FIXED_PRECISION;
use crate::types::{
    currency::Currency,
    fixed::{f64_to_fixed_i64, fixed_i64_to_f64},
};

pub const MONEY_MAX: f64 = 9_223_372_036.0;
pub const MONEY_MIN: f64 = -9_223_372_036.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct Money {
    pub raw: i64,
    pub currency: Currency,
}

impl Money {
    pub fn new(amount: f64, currency: Currency) -> anyhow::Result<Self> {
        check_in_range_inclusive_f64(amount, MONEY_MIN, MONEY_MAX, "amount")?;

        Ok(Self {
            raw: f64_to_fixed_i64(amount, currency.precision),
            currency,
        })
    }

    #[must_use]
    pub fn from_raw(raw: i64, currency: Currency) -> Self {
        Self { raw, currency }
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }

    #[must_use]
    pub fn as_f64(&self) -> f64 {
        fixed_i64_to_f64(self.raw)
    }

    #[must_use]
    pub fn as_decimal(&self) -> Decimal {
        // Scale down the raw value to match the precision
        let precision = self.currency.precision;
        let rescaled_raw = self.raw / i64::pow(10, u32::from(FIXED_PRECISION - precision));
        Decimal::from_i128_with_scale(i128::from(rescaled_raw), u32::from(precision))
    }

    #[must_use]
    pub fn to_formatted_string(&self) -> String {
        let amount_str = format!("{:.*}", self.currency.precision as usize, self.as_f64())
            .separate_with_underscores();
        format!("{} {}", amount_str, self.currency.code)
    }
}

impl FromStr for Money {
    type Err = String;

    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = input.split_whitespace().collect();

        // Ensure we have both the amount and currency
        if parts.len() != 2 {
            return Err(format!(
                "Invalid input format: '{input}'. Expected '<amount> <currency>'"
            ));
        }

        // Parse amount
        let amount = parts[0]
            .parse::<f64>()
            .map_err(|e| format!("Cannot parse amount '{}' as `f64`: {:?}", parts[0], e))?;

        // Parse currency
        let currency = Currency::from_str(parts[1]).map_err(|e: anyhow::Error| e.to_string())?;

        Self::new(amount, currency).map_err(|e: anyhow::Error| e.to_string())
    }
}

impl From<&str> for Money {
    fn from(input: &str) -> Self {
        Self::from_str(input).unwrap()
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
        assert_eq!(self.currency, rhs.currency);
        Self {
            raw: self.raw + rhs.raw,
            currency: self.currency,
        }
    }
}

impl Sub for Money {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        assert_eq!(self.currency, rhs.currency);
        Self {
            raw: self.raw - rhs.raw,
            currency: self.currency,
        }
    }
}

impl AddAssign for Money {
    fn add_assign(&mut self, other: Self) {
        assert_eq!(self.currency, other.currency);
        self.raw += other.raw;
    }
}

impl SubAssign for Money {
    fn sub_assign(&mut self, other: Self) {
        assert_eq!(self.currency, other.currency);
        self.raw -= other.raw;
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

// DIVIDE
impl Div<f64> for Money {
    type Output = f64;
    fn div(self, rhs: f64) -> Self::Output {
        self.as_f64() / rhs
    }
}

impl Display for Money {
    fn fmt(&self, f: &mut Formatter<'_>) -> FmtResult {
        write!(
            f,
            "{:.*} {}",
            self.currency.precision as usize,
            self.as_f64(),
            self.currency.code
        )
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
        let money_str: &str = Deserialize::deserialize(deserializer)?;

        let parts: Vec<&str> = money_str.splitn(2, ' ').collect();
        if parts.len() != 2 {
            return Err(serde::de::Error::custom("Invalid Money format"));
        }

        let amount_str = parts[0];
        let currency_str = parts[1];

        let amount = amount_str
            .parse::<f64>()
            .map_err(|_| serde::de::Error::custom("Failed to parse Money amount"))?;

        let currency = Currency::from_str(currency_str)
            .map_err(|_| serde::de::Error::custom("Invalid currency"))?;

        Ok(Self::new(amount, currency).unwrap()) // TODO: Properly handle the error
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use float_cmp::approx_eq;
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    #[should_panic]
    fn test_money_different_currency_addition() {
        let usd = Money::new(1000.0, Currency::USD()).unwrap();
        let btc = Money::new(1.0, Currency::BTC()).unwrap();
        let _result = usd + btc; // This should panic since currencies are different
    }

    #[rstest]
    fn test_money_min_max_values() {
        let min_money = Money::new(MONEY_MIN, Currency::USD()).unwrap();
        let max_money = Money::new(MONEY_MAX, Currency::USD()).unwrap();
        assert_eq!(
            min_money.raw,
            f64_to_fixed_i64(MONEY_MIN, Currency::USD().precision)
        );
        assert_eq!(
            max_money.raw,
            f64_to_fixed_i64(MONEY_MAX, Currency::USD().precision)
        );
    }

    #[rstest]
    fn test_money_addition_f64() {
        let money = Money::new(1000.0, Currency::USD()).unwrap();
        let result = money + 500.0;
        assert_eq!(result, 1500.0);
    }

    #[rstest]
    fn test_money_negation() {
        let money = Money::new(100.0, Currency::USD()).unwrap();
        let result = -money;
        assert_eq!(result.as_f64(), -100.0);
        assert_eq!(result.currency, Currency::USD().clone());
    }

    #[rstest]
    fn test_money_new_usd() {
        let money = Money::new(1000.0, Currency::USD()).unwrap();
        assert_eq!(money.currency.code.as_str(), "USD");
        assert_eq!(money.currency.precision, 2);
        assert_eq!(money.to_string(), "1000.00 USD");
        assert_eq!(money.to_formatted_string(), "1_000.00 USD");
        assert_eq!(money.as_decimal(), dec!(1000.00));
        assert!(approx_eq!(f64, money.as_f64(), 1000.0, epsilon = 0.001));
    }

    #[rstest]
    fn test_money_new_btc() {
        let money = Money::new(10.3, Currency::BTC()).unwrap();
        assert_eq!(money.currency.code.as_str(), "BTC");
        assert_eq!(money.currency.precision, 8);
        assert_eq!(money.to_string(), "10.30000000 BTC");
        assert_eq!(money.to_formatted_string(), "10.30000000 BTC");
    }

    #[rstest]
    fn test_money_serialization_deserialization() {
        let money = Money::new(123.45, Currency::USD()).unwrap();
        let serialized = serde_json::to_string(&money).unwrap();
        let deserialized: Money = serde_json::from_str(&serialized).unwrap();
        assert_eq!(money, deserialized);
    }

    #[rstest]
    #[case("0USD")] // <-- No whitespace separator
    #[case("0x00 USD")] // <-- Invalid float
    #[case("0 US")] // <-- Invalid currency
    #[case("0 USD USD")] // <-- Too many parts
    fn test_from_str_invalid_input(#[case] input: &str) {
        let result = Money::from_str(input);
        assert!(result.is_err());
    }

    #[rstest]
    #[case("0 USD", Currency::USD(), dec!(0.00))]
    #[case("1.1 AUD", Currency::AUD(), dec!(1.10))]
    #[case("1.12345678 BTC", Currency::BTC(), dec!(1.12345678))]
    fn test_from_str_valid_input(
        #[case] input: &str,
        #[case] expected_currency: Currency,
        #[case] expected_dec: Decimal,
    ) {
        let money = Money::from_str(input).unwrap();
        assert_eq!(money.currency, expected_currency);
        assert_eq!(money.as_decimal(), expected_dec);
    }
}
