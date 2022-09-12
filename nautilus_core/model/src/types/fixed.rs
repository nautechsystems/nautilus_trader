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

use nautilus_core::correctness;

pub const FIXED_PRECISION: u8 = 9;
pub const FIXED_SCALAR: f64 = 1000000000.0; // 10.0**FIXED_PRECISION

pub fn f64_to_fixed_i64(value: f64, precision: u8) -> i64 {
    correctness::u8_in_range_inclusive(precision, 0, 9, "precision");

    let pow1 = 10_i64.pow(precision as u32);
    let pow2 = 10_i64.pow((FIXED_PRECISION - precision) as u32);
    let rounded = (value * pow1 as f64).round() as i64;
    rounded * pow2
}

pub fn f64_to_fixed_u64(value: f64, precision: u8) -> u64 {
    correctness::u8_in_range_inclusive(precision, 0, 9, "precision");

    let pow1 = 10_u64.pow(precision as u32);
    let pow2 = 10_u64.pow((FIXED_PRECISION - precision) as u32);
    let rounded = (value * pow1 as f64).round() as u64;
    rounded * pow2
}

pub fn fixed_i64_to_f64(value: i64) -> f64 {
    (value as f64) * 0.000000001
}

pub fn fixed_u64_to_f64(value: u64) -> f64 {
    (value as f64) * 0.000000001
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use crate::types::fixed::{
        f64_to_fixed_i64, f64_to_fixed_u64, fixed_i64_to_f64, fixed_u64_to_f64,
    };
    use rstest::*;

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1)]
    #[case(1.1, 1)]
    #[case(0.000000001, 9)]
    #[case(-0.0, 0)]
    #[case(-1.0, 1)]
    #[case(-1.1, 1)]
    #[case(-0.000000001, 9)]
    fn test_f64_to_fixed_i64_to_fixed(#[case] value: f64, #[case] precision: u8) {
        let fixed = f64_to_fixed_i64(value, precision);
        let result = fixed_i64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1)]
    #[case(1.1, 1)]
    #[case(0.000000001, 9)]
    fn test_f64_to_fixed_u64_to_fixed(#[case] value: f64, #[case] precision: u8) {
        let fixed = f64_to_fixed_u64(value, precision);
        let result = fixed_u64_to_f64(fixed);
        assert_eq!(result, value);
    }
}
