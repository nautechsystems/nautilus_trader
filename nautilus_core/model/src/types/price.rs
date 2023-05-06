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

use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Deref, Mul, MulAssign, Neg, Sub, SubAssign};

use nautilus_core::correctness;
use nautilus_core::parsing::precision_from_str;

use crate::types::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};

use super::fixed::FIXED_SCALAR;

pub const PRICE_MAX: f64 = 9_223_372_036.0;
pub const PRICE_MIN: f64 = -9_223_372_036.0;

#[repr(C)]
#[derive(Eq, Clone, Default)]
pub struct Price {
    pub raw: i64,
    pub precision: u8,
}

impl Price {
    #[must_use]
    pub fn new(value: f64, precision: u8) -> Self {
        correctness::f64_in_range_inclusive(value, PRICE_MIN, PRICE_MAX, "`Price` value");

        Self {
            raw: f64_to_fixed_i64(value, precision),
            precision,
        }
    }

    #[must_use]
    pub fn from_raw(raw: i64, precision: u8) -> Self {
        Self { raw, precision }
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

impl From<&str> for Price {
    fn from(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("Cannot parse `input` string '{input}' as f64, {err}"),
        };
        Self::new(float_res, precision_from_str(input))
    }
}

impl From<Price> for f64 {
    fn from(value: Price) -> Self {
        value.as_f64()
    }
}

impl From<&Price> for f64 {
    fn from(value: &Price) -> Self {
        value.as_f64()
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state)
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.raw.partial_cmp(&other.raw)
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
    type Target = i64;

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
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision,
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision,
        }
    }
}

impl Mul for Price {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self {
        Self {
            raw: (self.raw * rhs.raw) / (FIXED_SCALAR as i64),
            precision: self.precision,
        }
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        self.raw += other.raw;
    }
}

impl SubAssign for Price {
    fn sub_assign(&mut self, other: Self) {
        self.raw -= other.raw;
    }
}

impl MulAssign for Price {
    fn mul_assign(&mut self, multiplier: Self) {
        self.raw *= multiplier.raw;
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn price_new(value: f64, precision: u8) -> Price {
    Price::new(value, precision)
}

#[no_mangle]
pub extern "C" fn price_from_raw(raw: i64, precision: u8) -> Price {
    Price::from_raw(raw, precision)
}

#[no_mangle]
pub extern "C" fn price_as_f64(price: &Price) -> f64 {
    price.as_f64()
}

#[no_mangle]
pub extern "C" fn price_add_assign(mut a: Price, b: Price) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn price_sub_assign(mut a: Price, b: Price) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::Price;

    #[test]
    fn test_price_new() {
        let price = Price::new(0.00812, 8);
        assert_eq!(price, price);
        assert_eq!(price.raw, 8_120_000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn test_price_minimum() {
        let price = Price::new(0.000_000_001, 9);
        assert_eq!(price.raw, 1);
        assert_eq!(price.to_string(), "0.000000001");
    }

    #[test]
    fn test_price_is_zero() {
        let price = Price::new(0.0, 8);
        assert_eq!(price, price);
        assert_eq!(price.raw, 0);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.0);
        assert_eq!(price.to_string(), "0.00000000");
        assert!(price.is_zero());
    }

    #[test]
    fn test_price_precision() {
        let price = Price::new(1.001, 2);
        assert_eq!(price.raw, 1_000_000_000);
        assert_eq!(price.to_string(), "1.00");
    }

    #[test]
    fn test_price_new_from_str() {
        let price = Price::from("0.00812000");
        assert_eq!(price, price);
        assert_eq!(price.raw, 8_120_000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn test_price_equality() {
        assert_eq!(Price::new(1.0, 1), Price::new(1.0, 1));
        assert_eq!(Price::new(1.0, 1), Price::new(1.0, 2));
        assert_ne!(Price::new(1.1, 1), Price::new(1.0, 1));
        assert!(Price::new(1.0, 1) <= Price::new(1.0, 2));
        assert!(Price::new(1.1, 1) > Price::new(1.0, 1));
        assert!(Price::new(1.0, 1) >= Price::new(1.0, 1));
        assert!(Price::new(1.0, 1) >= Price::new(1.0, 2));
        assert!(Price::new(1.0, 1) >= Price::new(1.0, 2));
        assert!(Price::new(0.9, 1) < Price::new(1.0, 1));
        assert!(Price::new(0.9, 1) <= Price::new(1.0, 2));
        assert!(Price::new(0.9, 1) <= Price::new(1.0, 1));
    }

    #[test]
    fn test_add() {
        let price1 = Price::new(1.000, 3);
        let price2 = Price::new(1.011, 3);
        let price3 = price1 + price2;
        assert_eq!(price3.raw, 2_011_000_000)
    }

    #[test]
    fn test_sub() {
        let price1 = Price::new(1.011, 3);
        let price2 = Price::new(1.000, 3);
        let price3 = price1 - price2;
        assert_eq!(price3.raw, 11_000_000);
    }

    #[test]
    fn test_add_assign() {
        let mut price = Price::new(1.000, 3);
        price += Price::new(1.011, 3);
        assert_eq!(price.raw, 2_011_000_000)
    }

    #[test]
    fn test_sub_assign() {
        let mut price = Price::new(1.000, 3);
        price -= Price::new(0.011, 3);
        assert_eq!(price.raw, 989_000_000)
    }

    #[test]
    fn test_mul() {
        let price1 = Price::new(1.000, 3);
        let price2 = Price::new(1.011, 3);
        let price3 = price1 * price2;
        assert_eq!(price3.raw, 1_011_000_000);
    }

    #[test]
    fn test_mul_assign() {
        let mut price1 = Price::new(1.000, 3);
        let price2 = Price::new(1.011, 3);
        price1 *= price2;
        assert_eq!(price1.raw, 1_011_000_000_000_000_000);
    }

    #[test]
    fn test_price_display_works() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let price = Price::from(input_string);
        let mut res = String::new();
        write!(&mut res, "{price}").unwrap();
        assert_eq!(res, input_string);
    }

    #[test]
    fn test_price_display() {
        let input_string = "44.123456";
        let price = Price::from(input_string);
        assert_eq!(price.raw, 44_123_456_000);
        assert_eq!(price.precision, 6);
        assert_eq!(price.as_f64(), 44.123_456_000_000_004);
        assert_eq!(price.to_string(), "44.123456");
    }
}
