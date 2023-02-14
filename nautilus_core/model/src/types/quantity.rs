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
use std::fmt::{Debug, Display, Formatter, Result};
use std::hash::{Hash, Hasher};
use std::ops::{Add, AddAssign, Deref, Mul, MulAssign, Sub, SubAssign};

use nautilus_core::correctness;
use nautilus_core::parsing::precision_from_str;

use crate::types::fixed::{f64_to_fixed_u64, fixed_u64_to_f64};

pub const QUANTITY_MAX: f64 = 18_446_744_073.0;
pub const QUANTITY_MIN: f64 = 0.0;

#[repr(C)]
#[derive(Eq, Clone, Default)]
pub struct Quantity {
    pub raw: u64,
    pub precision: u8,
}

impl Quantity {
    #[must_use]
    pub fn new(value: f64, precision: u8) -> Self {
        correctness::f64_in_range_inclusive(value, QUANTITY_MIN, QUANTITY_MAX, "`Quantity` value");

        Quantity {
            raw: f64_to_fixed_u64(value, precision),
            precision,
        }
    }

    pub fn from_raw(raw: u64, precision: u8) -> Self {
        Quantity { raw, precision }
    }

    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }
}

impl From<&str> for Quantity {
    fn from(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("cannot parse `input` string '{input}' as f64, {err}"),
        };
        Quantity::new(float_res, precision_from_str(input))
    }
}

impl From<i64> for Quantity {
    fn from(input: i64) -> Self {
        Quantity::new(input as f64, 0)
    }
}

impl Hash for Quantity {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.raw.hash(state)
    }
}

impl PartialEq for Quantity {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}

impl PartialOrd for Quantity {
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

impl Ord for Quantity {
    fn cmp(&self, other: &Self) -> Ordering {
        self.raw.cmp(&other.raw)
    }
}

impl Deref for Quantity {
    type Target = u64;

    fn deref(&self) -> &Self::Target {
        &self.raw
    }
}

impl Add for Quantity {
    type Output = Self;
    fn add(self, rhs: Self) -> Self::Output {
        Quantity {
            raw: self.raw + rhs.raw,
            precision: self.precision,
        }
    }
}

impl Sub for Quantity {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Quantity {
            raw: self.raw - rhs.raw,
            precision: self.precision,
        }
    }
}

impl Mul for Quantity {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Quantity {
            raw: self.raw * rhs.raw,
            precision: self.precision,
        }
    }
}

impl AddAssign for Quantity {
    fn add_assign(&mut self, other: Self) {
        self.raw += other.raw;
    }
}

impl AddAssign<u64> for Quantity {
    fn add_assign(&mut self, other: u64) {
        self.raw += other;
    }
}

impl SubAssign for Quantity {
    fn sub_assign(&mut self, other: Self) {
        self.raw -= other.raw;
    }
}

impl SubAssign<u64> for Quantity {
    fn sub_assign(&mut self, other: u64) {
        self.raw -= other;
    }
}

impl MulAssign<u64> for Quantity {
    fn mul_assign(&mut self, multiplier: u64) {
        self.raw *= multiplier;
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

////////////////////////////////////////////////////////////////////////////////
// C API
////////////////////////////////////////////////////////////////////////////////
#[no_mangle]
pub extern "C" fn quantity_new(value: f64, precision: u8) -> Quantity {
    Quantity::new(value, precision)
}

#[no_mangle]
pub extern "C" fn quantity_from_raw(raw: u64, precision: u8) -> Quantity {
    Quantity::from_raw(raw, precision)
}

#[no_mangle]
pub extern "C" fn quantity_as_f64(qty: &Quantity) -> f64 {
    qty.as_f64()
}

#[no_mangle]
pub extern "C" fn quantity_add_assign(mut a: Quantity, b: Quantity) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_add_assign_u64(mut a: Quantity, b: u64) {
    a.add_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_sub_assign(mut a: Quantity, b: Quantity) {
    a.sub_assign(b);
}

#[no_mangle]
pub extern "C" fn quantity_sub_assign_u64(mut a: Quantity, b: u64) {
    a.sub_assign(b);
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use super::Quantity;

    #[test]
    fn test_qty_new() {
        let qty = Quantity::new(0.00812, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[test]
    fn test_qty_from_i64() {
        let qty = Quantity::from(100_000);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 100_000_000_000_000);
        assert_eq!(qty.precision, 0);
    }

    #[test]
    fn test_qty_minimum() {
        let qty = Quantity::new(0.000000001, 9);
        assert_eq!(qty.raw, 1);
        assert_eq!(qty.to_string(), "0.000000001");
    }

    #[test]
    fn test_qty_is_zero() {
        let qty = Quantity::new(0.0, 8);
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 0);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.0);
        assert_eq!(qty.to_string(), "0.00000000");
        assert!(qty.is_zero());
    }

    #[test]
    fn test_qty_precision() {
        let qty = Quantity::new(1.001, 2);
        assert_eq!(qty.raw, 1_000_000_000);
        assert_eq!(qty.to_string(), "1.00");
    }

    #[test]
    fn test_qty_new_from_str() {
        let qty = Quantity::from("0.00812000");
        assert_eq!(qty, qty);
        assert_eq!(qty.raw, 8_120_000);
        assert_eq!(qty.precision, 8);
        assert_eq!(qty.as_f64(), 0.00812);
        assert_eq!(qty.to_string(), "0.00812000");
    }

    #[test]
    fn test_qty_equality() {
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 1));
        assert_eq!(Quantity::new(1.0, 1), Quantity::new(1.0, 2));
        assert_ne!(Quantity::new(1.1, 1), Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) <= Quantity::new(1.0, 2));
        assert!(Quantity::new(1.1, 1) > Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 1));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 2));
        assert!(Quantity::new(1.0, 1) >= Quantity::new(1.0, 2));
        assert!(Quantity::new(0.9, 1) < Quantity::new(1.0, 1));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 2));
        assert!(Quantity::new(0.9, 1) <= Quantity::new(1.0, 1));
    }

    #[test]
    fn test_qty_display() {
        use std::fmt::Write as FmtWrite;
        let input_string = "44.12";
        let qty = Quantity::from(input_string);
        let mut res = String::new();
        write!(&mut res, "{qty}").unwrap();
        assert_eq!(res, input_string);
        assert_eq!(qty.to_string(), input_string);
    }
}
