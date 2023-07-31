// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
    ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign},
    str::FromStr,
};

use nautilus_core::correctness;
use pyo3::prelude::*;
use serde::{Deserialize, Deserializer, Serialize};

use crate::types::{
    currency::Currency,
    fixed::{f64_to_fixed_i64, fixed_i64_to_f64},
};

pub const MONEY_MAX: f64 = 9_223_372_036.0;
pub const MONEY_MIN: f64 = -9_223_372_036.0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Eq)]
#[pyclass]
pub struct Money {
    raw: i64,
    pub currency: Currency,
}

impl Money {
    #[must_use]
    pub fn new(amount: f64, currency: Currency) -> Self {
        correctness::f64_in_range_inclusive(amount, MONEY_MIN, MONEY_MAX, "`Money` amount");

        Self {
            raw: f64_to_fixed_i64(amount, currency.precision),
            currency,
        }
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

impl Mul for Money {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        assert_eq!(self.currency, rhs.currency);
        Self {
            raw: self.raw * rhs.raw,
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

impl MulAssign for Money {
    fn mul_assign(&mut self, multiplier: Self) {
        self.raw *= multiplier.raw;
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

impl Display for Money {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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

        Ok(Money::new(amount, currency))
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn money_new(amount: f64, currency: Currency) -> Money {
    Money::new(amount, currency)
}

#[no_mangle]
pub extern "C" fn money_from_raw(raw: i64, currency: Currency) -> Money {
    Money::from_raw(raw, currency)
}

#[no_mangle]
pub extern "C" fn money_as_f64(money: &Money) -> f64 {
    money.as_f64()
}

#[no_mangle]
pub extern "C" fn money_add_assign(mut a: Money, b: Money) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn money_sub_assign(mut a: Money, b: Money) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::*;
    use crate::currencies::{BTC, USD};

    #[test]
    fn test_money_new_usd() {
        let money = Money::new(1000.0, USD.clone());
        assert_eq!(money.currency.code.as_str(), "USD");
        assert_eq!(money.currency.precision, 2);
        assert_eq!(money.to_string(), "1000.00 USD");
    }

    #[test]
    fn test_money_new_btc() {
        let money = Money::new(10.3, BTC.clone());
        assert_eq!(money.currency.code.as_str(), "BTC");
        assert_eq!(money.currency.precision, 8);
        assert_eq!(money.to_string(), "10.30000000 BTC");
    }

    // #[test]
    // fn test_account_balance() {
    //     let usd = Currency {
    //         code: String::from("USD"),
    //         precision: 2,
    //         currency_type: CurrencyType::Fiat,
    //     };
    //     let balance = AccountBalance {
    //         currency: usd,
    //         total: Money {
    //             amount: Decimal::new(103, 1),
    //             currency: usd,
    //         },
    //         locked: Money {
    //             amount: Decimal::new(0, 0),
    //             currency: usd,
    //         },
    //         free: Money {
    //             amount: Decimal::new(103, 1),
    //             currency: usd,
    //         },
    //     };
    //
    //     assert_eq!(balance.to_string(), "10.30000000 BTC");
    // }
}
