// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
use nautilus_core::text::prec_from_str;
use std::cmp::Ordering;
use std::fmt::{Debug, Display, Formatter, Result};
use std::ops::{AddAssign, Mul, MulAssign};

#[repr(C)]
#[derive(Copy, Clone, Default, Hash)]
pub struct Quantity {
    pub mantissa: u64,
    pub precision: u8,
}

impl Quantity {
    pub fn new(value: f64, precision: u8) -> Self {
        assert!(precision <= 9);
        let diff = FIXED_EXPONENT - precision;
        Quantity {
            mantissa: (value * 10_i32.pow(precision as u32) as f64) as u64 * 10_u64.pow(diff as u32),
            precision,
        }
    }

    pub fn new_from_str(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("Cannot parse `input` string '{}' as f64, {}", input, err),
        };
        Quantity::new(float_res, prec_from_str(input))
    }

    pub fn as_f64(&self) -> f64 {
        (self.mantissa) as f64 * FIXED_PRECISION
    }

    //##########################################################################
    // C API
    //##########################################################################
    #[no_mangle]
    pub extern "C" fn quantity_new(value: f64, precision: u8) -> Self {
        Quantity::new(value, precision)
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.mantissa == other.mantissa
    }

    fn ne(&self, other: &Self) -> bool {
        self.mantissa != other.mantissa
    }
}

impl Eq for Quantity {}

impl PartialOrd for Quantity {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.mantissa.partial_cmp(&other.mantissa)
    }

    fn lt(&self, other: &Self) -> bool {
        self.mantissa.lt(&other.mantissa)
    }

    fn le(&self, other: &Self) -> bool {
        self.mantissa.le(&other.mantissa)
    }

    fn gt(&self, other: &Self) -> bool {
        self.mantissa.gt(&other.mantissa)
    }

    fn ge(&self, other: &Self) -> bool {
        self.mantissa.ge(&other.mantissa)
    }
}

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.mantissa.cmp(&other.mantissa)
    }
}

impl AddAssign for Quantity {
    fn add_assign(&mut self, other: Self) {
        self.mantissa += other.mantissa;
    }
}

impl MulAssign<u64> for Quantity {
    fn mul_assign(&mut self, multiplier: u64) {
        self.mantissa *= multiplier;
    }
}

impl Mul<u64> for Quantity {
    type Output = Self;
    fn mul(self, rhs: u64) -> Self {
        Quantity {
            mantissa: self.mantissa * rhs,
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
        assert_eq!(qty.mantissa, 8120000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[test]
    fn qty_minimum() {
        let qty = Quantity::new(0.000000001, 9);

        assert_eq!(qty.mantissa, 1);
        assert_eq!(qty.to_string(), "0.000000001");
    }
    #[test]
    fn qty_precision() {
        let qty = Quantity::new(1.001, 2);

        assert_eq!(qty.mantissa, 1000000000);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[test]
    fn qty_new_from_str() {
        let qty = Quantity::new_from_str("0.00812000");

        assert_eq!(qty, qty);
        assert_eq!(qty.mantissa, 8120000);
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
