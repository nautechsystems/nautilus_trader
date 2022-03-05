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

use math::round;

pub const FIXED_POWER: f64 = 1000000000.0;  // fixed power 10.0**9
pub const FIXED_UNIT: f64 = 0.000000001; // fixed unit 10.0**-9

pub fn f64_to_fixed_i64(value: f64, precision: i8) -> i64 {
    assert!(precision <= 9);
    let rounded = round::half_to_even(value, precision);
    (rounded * FIXED_POWER) as i64
}

pub fn f64_to_fixed_u64(value: f64, precision: i8) -> u64 {
    assert!(precision <= 9);
    let rounded = round::half_to_even(value, precision);
    (rounded * FIXED_POWER) as u64
}

pub fn fixed_i64_to_f64(value: i64) -> f64 {
    (value as f64) * FIXED_UNIT
}

pub fn fixed_u64_to_f64(value: u64) -> f64 {
    (value as f64) * FIXED_UNIT
}

#[cfg(test)]
mod tests {
    use crate::primitives::fixed::{
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
    fn test_f64_to_fixed_i64_to_fixed(#[case] value: f64, #[case] precision: i8) {
        let fixed = f64_to_fixed_i64(value, precision);
        let result = fixed_i64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0.0, 0)]
    #[case(1.0, 1)]
    #[case(1.1, 1)]
    #[case(0.000000001, 9)]
    fn test_f64_to_fixed_u64_to_fixed(#[case] value: f64, #[case] precision: i8) {
        let fixed = f64_to_fixed_u64(value, precision);
        let result = fixed_u64_to_f64(fixed);
        assert_eq!(result, value);
    }
}
