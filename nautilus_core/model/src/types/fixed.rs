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

pub const FIXED_PRECISION: u8 = 9;
pub const FIXED_SCALAR: f64 = 1000000000.0; // 10.0**FIXED_PRECISION

pub fn f64_to_fixed_i64(value: f64, precision: u8) -> i64 {
    assert!(precision <= FIXED_PRECISION, "precision exceeded maximum 9");
    let pow1 = 10_i64.pow(precision as u32);
    let pow2 = 10_i64.pow((FIXED_PRECISION - precision) as u32);
    let rounded = (value * pow1 as f64).round() as i64;
    rounded * pow2
}

pub fn f64_to_fixed_u64(value: f64, precision: u8) -> u64 {
    assert!(precision <= FIXED_PRECISION, "precision exceeded maximum 9");
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
    use rstest::*;

    use super::*;

    #[rstest(precision, value,
        case(0, 0.0),
        case(1, 1.0),
        case(1, 1.1),
        case(9, 0.000000001),
        case(0, -0.0),
        case(1, -1.0),
        case(1, -1.1),
        case(9, -0.000000001),
    )]
    fn test_f64_to_fixed_i64_to_fixed(precision: u8, value: f64) {
        let fixed = f64_to_fixed_i64(value, precision);
        let result = fixed_i64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest(
        precision,
        value,
        case(0, 0.0),
        case(1, 1.0),
        case(1, 1.1),
        case(9, 0.000000001)
    )]
    fn test_f64_to_fixed_u64_to_fixed(precision: u8, value: f64) {
        let fixed = f64_to_fixed_u64(value, precision);
        let result = fixed_u64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest(
        precision,
        value,
        expected,
        case(0, 123456.0, 123456000000000),
        case(0, 123456.7, 123457000000000),
        case(0, 123456.4, 123456000000000),
        case(1, 123456.0, 123456000000000),
        case(1, 123456.7, 123456700000000),
        case(1, 123456.4, 123456400000000),
        case(2, 123456.0, 123456000000000),
        case(2, 123456.7, 123456700000000),
        case(2, 123456.4, 123456400000000)
    )]
    fn test_f64_to_fixed_i64_with_precision(precision: u8, value: f64, expected: i64) {
        assert_eq!(f64_to_fixed_i64(value, precision), expected);
    }

    #[rstest(precision, value, expected,
        case(0, 5.5, 6000000000),
        case(1, 5.55, 5600000000),
        case(2, 5.555, 5560000000),
        case(3, 5.5555, 5556000000),
        case(4, 5.55555, 5555600000),
        case(5, 5.555555, 5555560000),
        case(6, 5.5555555, 5555556000),
        case(7, 5.55555555, 5555555600),
        case(8, 5.555555555, 5555555560),
        case(9, 5.5555555555, 5555555556),
        case(0, -5.5, -6000000000),
        case(1, -5.55, -5600000000),
        case(2, -5.555, -5560000000),
        case(3, -5.5555, -5556000000),
        case(4, -5.55555, -5555600000),
        case(5, -5.555555, -5555560000),
        case(6, -5.5555555, -5555556000),
        case(7, -5.55555555, -5555555600),
        case(8, -5.555555555, -5555555560),
        case(9, -5.5555555555, -5555555556),
    )]
    fn test_f64_to_fixed_i64(precision: u8, value: f64, expected: i64) {
        assert_eq!(f64_to_fixed_i64(value, precision), expected);
    }

    #[rstest(
        precision,
        value,
        expected,
        case(0, 5.5, 6000000000),
        case(1, 5.55, 5600000000),
        case(2, 5.555, 5560000000),
        case(3, 5.5555, 5556000000),
        case(4, 5.55555, 5555600000),
        case(5, 5.555555, 5555560000),
        case(6, 5.5555555, 5555556000),
        case(7, 5.55555555, 5555555600),
        case(8, 5.555555555, 5555555560),
        case(9, 5.5555555555, 5555555556)
    )]
    fn test_f64_to_fixed_u64(precision: u8, value: f64, expected: u64) {
        assert_eq!(f64_to_fixed_u64(value, precision), expected);
    }

    #[rstest]
    fn test_fixed_i64_to_f64(
        #[values(1, -1, 2, -2, 10, -10, 100, -100, 1_000, -1_000)] value: i64,
    ) {
        assert_eq!(fixed_i64_to_f64(value), value as f64 * 0.000000001);
    }

    #[rstest]
    fn test_fixed_u64_to_f64(
        #[values(
            0,
            1,
            2,
            3,
            10,
            100,
            1_000,
            10_000,
            100_000,
            1_000_000,
            10_000_000,
            100_000_000,
            1_000_000_000,
            10_000_000_000,
            100_000_000_000,
            1_000_000_000_000,
            10_000_000_000_000,
            100_000_000_000_000,
            1_000_000_000_000_000
        )]
        value: u64,
    ) {
        let result = fixed_u64_to_f64(value);
        assert_eq!(result, (value as f64) * 0.000000001);
    }
}
