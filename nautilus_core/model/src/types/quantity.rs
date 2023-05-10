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
use std::ops::{Add, AddAssign, Deref, Mul, MulAssign, Sub, SubAssign};

use nautilus_core::correctness;
use nautilus_core::parsing::precision_from_str;

use crate::types::fixed::{f64_to_fixed_u64, fixed_u64_to_f64};

use super::fixed::FIXED_SCALAR;

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

        Self {
            raw: f64_to_fixed_u64(value, precision),
            precision,
        }
    }

    #[must_use]
    pub fn from_raw(raw: u64, precision: u8) -> Self {
        Self { raw, precision }
    }

    #[must_use]
    pub fn is_zero(&self) -> bool {
        self.raw == 0
    }
    #[must_use]
    pub fn as_f64(&self) -> f64 {
        fixed_u64_to_f64(self.raw)
    }
}

impl From<Quantity> for f64 {
    fn from(value: Quantity) -> Self {
        value.as_f64()
    }
}

impl From<&Quantity> for f64 {
    fn from(value: &Quantity) -> Self {
        value.as_f64()
    }
}

impl From<&str> for Quantity {
    fn from(input: &str) -> Self {
        let float_from_input = input.parse::<f64>();
        let float_res = match float_from_input {
            Ok(number) => number,
            Err(err) => panic!("cannot parse `input` string '{input}' as f64, {err}"),
        };
        Self::new(float_res, precision_from_str(input))
    }
}

impl From<i64> for Quantity {
    fn from(input: i64) -> Self {
        Self::new(input as f64, 0)
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
        Self {
            raw: self.raw + rhs.raw,
            precision: self.precision,
        }
    }
}

impl Sub for Quantity {
    type Output = Self;
    fn sub(self, rhs: Self) -> Self::Output {
        Self {
            raw: self.raw - rhs.raw,
            precision: self.precision,
        }
    }
}

impl Mul for Quantity {
    type Output = Self;
    fn mul(self, rhs: Self) -> Self::Output {
        Self {
            raw: (self.raw * rhs.raw) / (FIXED_SCALAR as u64),
            precision: self.precision,
        }
    }
}

impl From<Quantity> for u64 {
    fn from(value: Quantity) -> Self {
        value.raw
    }
}

impl From<&Quantity> for u64 {
    fn from(value: &Quantity) -> Self {
        value.raw
    }
}

impl<T: Into<u64>> AddAssign<T> for Quantity {
    fn add_assign(&mut self, other: T) {
        self.raw += other.into();
    }
}

impl<T: Into<u64>> SubAssign<T> for Quantity {
    fn sub_assign(&mut self, other: T) {
        self.raw -= other.into();
    }
}

impl<T: Into<u64>> MulAssign<T> for Quantity {
    fn mul_assign(&mut self, other: T) {
        self.raw *= other.into();
    }
}

impl Debug for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:.*}", self.precision as usize, self.as_f64())
    }
}

impl Display for Quantity {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
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
        let qty = Quantity::new(0.000_000_001, 9);
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
    fn test_add() {
        let quantity1 = Quantity::new(1.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        let quantity3 = quantity1 + quantity2;
        assert_eq!(quantity3.raw, 3_000_000_000);
    }

    #[test]
    fn test_sub() {
        let quantity1 = Quantity::new(3.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        let quantity3 = quantity1 - quantity2;
        assert_eq!(quantity3.raw, 1_000_000_000);
    }

    #[test]
    fn test_add_assign() {
        let mut quantity1 = Quantity::new(1.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        quantity1 += quantity2;
        assert_eq!(quantity1.raw, 3_000_000_000);
    }

    #[test]
    fn test_sub_assign() {
        let mut quantity1 = Quantity::new(3.0, 0);
        let quantity2 = Quantity::new(2.0, 0);
        quantity1 -= quantity2;
        assert_eq!(quantity1.raw, 1_000_000_000);
    }

    #[test]
    fn test_mul() {
        let quantity1 = Quantity::new(2.0, 1);
        let quantity2 = Quantity::new(2.0, 1);
        let quantity3 = quantity1 * quantity2;
        assert_eq!(quantity3.raw, 4_000_000_000);
    }

    #[test]
    fn test_quantity_mul_assign() {
        let mut q = Quantity::from_raw(100, 0);
        q *= 2u64;

        assert_eq!(q.raw, 200);
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
