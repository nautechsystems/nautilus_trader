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

use crate::types::fixed::{f64_to_fixed_i64, fixed_i64_to_f64};
use nautilus_core::string::precision_from_str;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Mul, MulAssign, Neg, Sub, SubAssign};

#[repr(C)]
#[derive(Eq, Clone, Default)]
pub struct Price {
    fixed: i64,
    pub precision: u8,
}

impl Price {
    pub fn new(value: f64, precision: u8) -> Self {
        Price {
            fixed: f64_to_fixed_i64(value, precision),
            precision,
        }
    }

    pub fn from_fixed(fixed: i64, precision: u8) -> Self {
        Price {
            fixed,
            precision,
        }
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
        self.fixed == 0
    }

    pub fn as_f64(&self) -> f64 {
        fixed_i64_to_f64(self.fixed)
    }
}

impl Hash for Price {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.fixed.hash(state)
    }
}

impl PartialEq for Price {
    fn eq(&self, other: &Self) -> bool {
        self.fixed == other.fixed
    }
}

impl PartialOrd for Price {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.fixed.partial_cmp(&other.fixed)
    }

    fn lt(&self, other: &Self) -> bool {
        self.fixed.lt(&other.fixed)
    }

    fn le(&self, other: &Self) -> bool {
        self.fixed.le(&other.fixed)
    }

    fn gt(&self, other: &Self) -> bool {
        self.fixed.gt(&other.fixed)
    }

    fn ge(&self, other: &Self) -> bool {
        self.fixed.ge(&other.fixed)
    }
}

impl Ord for Price {
    fn cmp(&self, other: &Self) -> Ordering {
        self.fixed.cmp(&other.fixed)
    }
}

impl Neg for Price {
    type Output = Self;
    fn neg(self) -> Self::Output {
        Price {
            fixed: -self.fixed,
            precision: self.precision,
        }
    }
}

impl Add for Price {
    type Output = Self;
    fn add(self, rhs: Price) -> Self::Output {
        Price {
            fixed: self.fixed + rhs.fixed,
            precision: self.precision,
        }
    }
}

impl Sub for Price {
    type Output = Self;
    fn sub(self, rhs: Price) -> Self::Output {
        Price {
            fixed: self.fixed - rhs.fixed,
            precision: self.precision,
        }
    }
}

impl Mul for Price {
    type Output = Self;
    fn mul(self, rhs: Price) -> Self {
        Price {
            fixed: self.fixed * rhs.fixed,
            precision: self.precision,
        }
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        self.fixed += other.fixed;
    }
}

impl SubAssign for Price {
    fn sub_assign(&mut self, other: Self) {
        self.fixed -= other.fixed;
    }
}

impl MulAssign for Price {
    fn mul_assign(&mut self, multiplier: Self) {
        self.fixed *= multiplier.fixed;
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
    use crate::types::price::Price;

    #[test]
    fn test_price_new() {
        let price = Price::new(0.00812, 8);

        assert_eq!(price, price);
        assert_eq!(price.fixed, 8120000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn test_price_minimum() {
        let price = Price::new(0.000000001, 9);

        assert_eq!(price.fixed, 1);
        assert_eq!(price.to_string(), "0.000000001");
    }

    #[test]
    fn test_price_is_zero() {
        let price = Price::new(0.0, 8);

        assert_eq!(price, price);
        assert_eq!(price.fixed, 0);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.0);
        assert_eq!(price.to_string(), "0.00000000");
        assert!(price.is_zero());
    }

    #[test]
    fn test_price_precision() {
        let price = Price::new(1.001, 2);

        assert_eq!(price.fixed, 1000000000);
        assert_eq!(price.to_string(), "1.00");
    }

    #[test]
    fn test_price_new_from_str() {
        let price = Price::from_str("0.00812000");

        assert_eq!(price, price);
        assert_eq!(price.fixed, 8120000);
        assert_eq!(price.precision, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.to_string(), "0.00812000");
    }

    #[test]
    fn test_price_equality() {
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
        assert_eq!(price3.fixed, 2011000000)
    }

    #[test]
    fn test_add_assign() {
        let mut price = Price::new(1.000, 3);
        price += Price::new(1.011, 3);

        assert_eq!(price.fixed, 2011000000)
    }

    #[test]
    fn test_sub_assign() {
        let mut price = Price::new(1.000, 3);
        price -= Price::new(0.011, 3);

        assert_eq!(price.fixed, 989000000)
    }

    #[test]
    fn test_price_display_works() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let price = Price::from_str(&input_string);
        let mut res = String::new();

        write!(&mut res, "{}", price).unwrap();
        assert_eq!(res, input_string);
    }

    #[test]
    fn test_price_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.123456";
        let price = Price::from_str(&input_string);

        assert_eq!(price.fixed, 44123456000);
        assert_eq!(price.precision, 6);
        assert_eq!(price.as_f64(), 44.123456000000004);
        assert_eq!(price.to_string(), "44.123456");
    }
}
