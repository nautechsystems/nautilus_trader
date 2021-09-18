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

use std::fmt::{Display, Formatter, Result};
use std::ops::{AddAssign, Mul, MulAssign};
use nautilus_core::text::prec_from_str;

const FIXED_PREC: f64 = 0.000000001;

#[repr(C)]
#[derive(Copy, Clone, Debug, Default, Eq, Hash, PartialEq, PartialOrd, Ord)]
pub struct Price {
    pub value: i64,
    pub prec: usize,
}

impl Price {
    #[no_mangle]
    pub extern "C" fn new(value: f64, prec: usize) -> Self {
        Price {
            value: (value / FIXED_PREC).round() as i64,
            prec,
        }
    }

    pub fn new_from_str(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("Cannot parse `input` string '{}' as f64, {}", input, err),
        };
        Price::new(float_res, prec_from_str(input))
    }

    pub fn as_f64(self) -> f64 {
        (self.value) as f64 * FIXED_PREC
    }

    pub fn as_string(self) -> String {
        format!("{:.*}", self.prec, self.as_f64())
    }
}

impl AddAssign for Price {
    fn add_assign(&mut self, other: Self) {
        self.value += other.value;
    }
}

impl MulAssign<i64> for Price {
    fn mul_assign(&mut self, multiplier: i64) {
        self.value *= multiplier;
    }
}

impl Mul<i64> for Price {
    type Output = Self;
    fn mul(self, rhs: i64) -> Self {
        Price {
            value: self.value * rhs,
            prec: self.prec,
        }
    }
}

impl Display for Price {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        write!(f, "{:.*}", self.prec, self.as_f64())
    }
}

#[allow(unused_imports)] // warning: unused import: `std::fmt::Write as FmtWrite`
#[cfg(test)]
mod tests {
    use crate::objects::price::Price;

    #[test]
    fn price_new() {
        let price = Price::new(0.00812, 8);

        assert_eq!(price, price);
        assert_eq!(price.value, 8120000);
        assert_eq!(price.prec, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.as_string(), "0.00812000");
    }

    #[test]
    fn price_new_from_str() {
        let price = Price::new_from_str("0.00812000");

        assert_eq!(price, price);
        assert_eq!(price.value, 8120000);
        assert_eq!(price.prec, 8);
        assert_eq!(price.as_f64(), 0.00812);
        assert_eq!(price.as_string(), "0.00812000");
    }

    #[test]
    fn display_works() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let price = Price::new_from_str(&input_string);
        let mut res = String::new();

        write!(&mut res, "{}", price).unwrap();
        assert_eq!(res, input_string);
    }

    #[test]
    fn price_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.123456";
        let price = Price::new_from_str(&input_string);

        assert_eq!(price.value, 44123456000);
        assert_eq!(price.as_f64(), 44.123456000000004);
    }
}
