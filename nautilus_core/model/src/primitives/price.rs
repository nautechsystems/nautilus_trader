// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

use crate::primitives::{FIXED_EXPONENT, FIXED_PRECISION};
use nautilus_core::text::precision_from_str;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

#[repr(C)]
#[derive(Clone, Default)]
pub struct Price {
    value: i64,
    pub precision: u8,
}

impl Price {
    pub fn new(value: f64, precision: u8) -> Self {
        assert!(precision <= 9);

        let pow1 = 10_i64.pow(precision as u32);
        let pow2 = 10_i64.pow((FIXED_EXPONENT - precision) as u32);
        let rounded = (value * pow1 as f64).round() as i64;
        let value = rounded * pow2;
        Price { value, precision }
    }

    pub fn from_str(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("Cannot parse `input` string '{}' as f64, {}", input, err),
        };
        Price::new(float_res, precision_from_str(input))
    }

    pub fn is_zero(&self) -> bool {
        self.value == 0
    }
    pub fn as_f64(&self) -> f64 {
        (self.value) as f64 * FIXED_PRECISION
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.value.hash(state)
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Price {}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.value.partial_cmp(&other.value)
    }

    fn lt(&self, other: &Self) -> bool {
        self.value.lt(&other.value)
    }

    fn le(&self, other: &Self) -> bool {
        self.value.le(&other.value)
    }

    fn gt(&self, other: &Self) -> bool {
        self.value.gt(&other.value)
    }

    fn ge(&self, other: &Self) -> bool {
        self.value.ge(&other.value)
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl Neg for Price {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Price {
            value: -self.value,
            precision: self.precision,
        }
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Price) -> Self::Output {
        Price {
            value: self.value + rhs.value,
            precision: self.precision,
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Price) -> Self::Output {
        Price {
            value: self.value - rhs.value,
            precision: self.precision,
        }
    }
}

impl Mul for Price {
    type Output = Self;
    fn mul(self, rhs: Price) -> Self {
        Price {
            value: self.value * rhs.value,
            precision: self.precision,
        }
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        self.value += other.value;
    }
}

impl SubAssign for Price {
    fn sub_assign(&mut self, other: Self) {
        self.value -= other.value;
    }
}

impl MulAssign for Price {
    fn mul_assign(&mut self, multiplier: Self) {
        self.value *= multiplier.value;
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
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

#[allow(unused_imports)] // warning: unused import: `std::fmt::Write as FmtWrite`
#[cfg(test)]
mod tests {
    use crate::primitives::price::Price;

    #[test]
    fn price_new() {
        let price = Price::new(0.00812, 8);

        assert_eq!(price, price);
        assert_eq!(price.value, 8120000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn price_minimum() {
        let price = Price::new(0.000000001, 9);

        assert_eq!(price.value, 1);
        assert_eq!(price.to_string(), "0.000000001");
    }

    #[test]
    fn price_precision() {
        let price = Price::new(1.001, 2);

        assert_eq!(price.value, 1000000000);
        assert_eq!(price.to_string(), "1.00");
    }

    #[test]
    fn price_new_from_str() {
        let price = Price::from_str("0.00812000");

        assert_eq!(price, price);
        assert_eq!(price.value, 8120000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn price_equality() {
        assert_eq!(Price::new(1.0, 1), Price::new(1.0, 1));
        assert_eq!(Price::new(1.0, 1), Price::new(1.0, 2));
        assert_ne!(Price::new(1.1, 1), Price::new(1.0, 1));
        assert!(!(Price::new(1.0, 1) > Price::new(1.0, 2)));
        assert!(Price::new(1.1, 1) > Price::new(1.0, 1));
        assert!(Price::new(1.0, 1) >= Price::new(1.0, 1));
        assert!(Price::new(1.0, 1) >= Price::new(1.0, 2));
        assert!(!(Price::new(1.0, 1) < Price::new(1.0, 2)));
        assert!(Price::new(0.9, 1) < Price::new(1.0, 1));
        assert!(Price::new(0.9, 1) <= Price::new(1.0, 2));
        assert!(Price::new(0.9, 1) <= Price::new(1.0, 1));
    }

    #[test]
    fn test_add() {
        let price1 = Price::new(1.000, 3);
        let price2 = Price::new(1.011, 3);

        let price3 = price1 + price2;
        assert_eq!(price3.value, 2011000000)
    }

    #[test]
    fn test_add_assign() {
        let mut price = Price::new(1.000, 3);
        price += Price::new(1.011, 3);

        assert_eq!(price.value, 2011000000)
    }

    #[test]
    fn price_display_works() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let price = Price::from_str(&input_string);
        let mut res = String::new();

        write!(&mut res, "{}", price).unwrap();
        assert_eq!(res, input_string);
    }

    #[test]
    fn price_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.123456";
        let price = Price::from_str(&input_string);

        assert_eq!(price.value, 44123456000);
        assert_eq!(price.precision, 6);
        assert_eq!(price.as_f64(), 44.123456000000004);
        assert_eq!(price.to_string(), "44.123456");
    }
}
