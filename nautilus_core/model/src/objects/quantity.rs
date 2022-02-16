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

use crate::objects::{FIXED_EXPONENT, FIXED_PRECISION};
use nautilus_core::text::precision_from_str;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Result};
use std::ops::{AddAssign, Mul, MulAssign};

#[repr(C)]
#[derive(Copy, Clone, Default)]
pub struct Quantity {
    value: u64,
    pub precision: u8,
}

impl Quantity {
    pub fn new(value: f64, precision: u8) -> Self {
        assert!(precision <= 9);
        let diff = FIXED_EXPONENT - precision;
        Quantity {
            value: (value * 10_i32.pow(precision as u32) as f64) as u64 * 10_u64.pow(diff as u32),
            precision,
        }
    }

    pub fn new_from_str(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("Cannot parse `input` string '{}' as f64, {}", input, err),
        };
        Quantity::new(float_res, precision_from_str(input))
    }

    pub fn is_zero(&self) -> bool {
        self.value == 0
    }
    pub fn as_f64(&self) -> f64 {
        (self.value) as f64 * FIXED_PRECISION
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.value == other.value
    }
}

impl Eq for Quantity {}

impl PartialOrd for Quantity {
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

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.value.cmp(&other.value)
    }
}

impl AddAssign for Quantity {
    fn add_assign(&mut self, other: Self) {
        self.value += other.value;
    }
}

impl MulAssign<u64> for Quantity {
    fn mul_assign(&mut self, multiplier: u64) {
        self.value *= multiplier;
    }
}

impl Mul<u64> for Quantity {
    type Output = Self;
    fn mul(self, rhs: u64) -> Self {
        Quantity {
            value: self.value * rhs,
            precision: self.precision,
        }
    }
}

impl Debug for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

#[allow(unused_imports)] // warning: unused import: `std::fmt::Write as FmtWrite`
#[cfg(test)]
mod tests {
    use crate::objects::quantity::Quantity;

    #[test]
    fn qty_new() {
        let qty = Quantity::new(0.00812, 8);

        assert_eq!(qty, qty);
        assert_eq!(qty.value, 8120000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[test]
    fn qty_minimum() {
        let qty = Quantity::new(0.000000001, 9);

        assert_eq!(qty.value, 1);
        assert_eq!(qty.to_string(), "0.000000001");
    }
    #[test]
    fn qty_precision() {
        let qty = Quantity::new(1.001, 2);

        assert_eq!(qty.value, 1000000000);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[test]
    fn qty_new_from_str() {
        let qty = Quantity::new_from_str("0.00812000");

        assert_eq!(qty, qty);
        assert_eq!(qty.value, 8120000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[test]
    fn qty_equality() {
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 1));
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 2));
        assert_ne!(Quantity::new(1.1, 1), Quantity::new(1.0, 1));
        assert!(!(Quantity::new(1.0, 1) > Quantity::new(1.0, 2)));
        assert!(Quantity::new(1.1, 1) > Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 2));
        assert!(!(Quantity::new(1.0, 1) < Quantity::new(1.0, 2)));
        assert!(Quantity::new(0.9, 1) < Quantity::new(1.0, 1));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 2));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 1));
    }

    #[test]
    fn qty_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let qty = Quantity::new_from_str(&input_string);
        let mut res = String::new();

        write!(&mut res, "{}", qty).unwrap();
        assert_eq!(res, input_string);
        assert_eq!(qty.to_string(), input_string);
    }
}
