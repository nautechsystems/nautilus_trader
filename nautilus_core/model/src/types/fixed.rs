// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
pub const FIXED_SCALAR: f64 = 1_000_000_000.0; // 10.0**FIXED_PRECISION

pub fn check_fixed_precision(precision: u8) -> anyhow::Result<()> {
    if precision > FIXED_PRECISION {
        anyhow::bail!("Condition failed: `precision` was greater than the maximum `FIXED_PRECISION` (9), was {precision}")
    }
    Ok(())
}

#[must_use]
pub fn f64_to_fixed_i64(value: f64, precision: u8) -> i64 {
    assert!(precision <= FIXED_PRECISION, "precision exceeded maximum 9");
    let pow1 = 10_i64.pow(u32::from(precision));
    let pow2 = 10_i64.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as i64;
    rounded * pow2
}

#[must_use]
pub fn f64_to_fixed_u64(value: f64, precision: u8) -> u64 {
    assert!(precision <= FIXED_PRECISION, "precision exceeded maximum 9");
    let pow1 = 10_u64.pow(u32::from(precision));
    let pow2 = 10_u64.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as u64;
    rounded * pow2
}

#[must_use]
pub fn fixed_i64_to_f64(value: i64) -> f64 {
    (value as f64) / FIXED_SCALAR
}

#[must_use]
pub fn fixed_u64_to_f64(value: u64) -> f64 {
    (value as f64) / FIXED_SCALAR
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0)]
    #[case(FIXED_PRECISION)]
    fn test_valid_precision(#[case] precision: u8) {
        let result = check_fixed_precision(precision);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_invalid_precision() {
        let precision = FIXED_PRECISION + 1;
        let result = check_fixed_precision(precision);
        assert!(result.is_err());
    }

    #[rstest]
    #[case(0, 0.0)]
    #[case(1, 1.0)]
    #[case(1, 1.1)]
    #[case(9, 0.000_000_001)]
    #[case(0, -0.0)]
    #[case(1, -1.0)]
    #[case(1, -1.1)]
    #[case(9, -0.000_000_001)]
    fn test_f64_to_fixed_i64_to_fixed(#[case] precision: u8, #[case] value: f64) {
        let fixed = f64_to_fixed_i64(value, precision);
        let result = fixed_i64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0, 0.0)]
    #[case(1, 1.0)]
    #[case(1, 1.1)]
    #[case(9, 0.000_000_001)]
    fn test_f64_to_fixed_u64_to_fixed(#[case] precision: u8, #[case] value: f64) {
        let fixed = f64_to_fixed_u64(value, precision);
        let result = fixed_u64_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0, 123_456.0, 123_456_000_000_000)]
    #[case(0, 123_456.7, 123_457_000_000_000)]
    #[case(0, 123_456.4, 123_456_000_000_000)]
    #[case(1, 123_456.0, 123_456_000_000_000)]
    #[case(1, 123_456.7, 123_456_700_000_000)]
    #[case(1, 123_456.4, 123_456_400_000_000)]
    #[case(2, 123_456.0, 123_456_000_000_000)]
    #[case(2, 123_456.7, 123_456_700_000_000)]
    #[case(2, 123_456.4, 123_456_400_000_000)]
    fn test_f64_to_fixed_i64_with_precision(
        #[case] precision: u8,
        #[case] value: f64,
        #[case] expected: i64,
    ) {
        assert_eq!(f64_to_fixed_i64(value, precision), expected);
    }

    #[rstest]
    #[case(0, 5.5, 6_000_000_000)]
    #[case(1, 5.55, 5_600_000_000)]
    #[case(2, 5.555, 5_560_000_000)]
    #[case(3, 5.5555, 5_556_000_000)]
    #[case(4, 5.55555, 5_555_600_000)]
    #[case(5, 5.555_555, 5_555_560_000)]
    #[case(6, 5.555_555_5, 5_555_556_000)]
    #[case(7, 5.555_555_55, 5_555_555_600)]
    #[case(8, 5.555_555_555, 5_555_555_560)]
    #[case(9, 5.555_555_555_5, 5_555_555_556)]
    #[case(0, -5.5, -6_000_000_000)]
    #[case(1, -5.55, -5_600_000_000)]
    #[case(2, -5.555, -5_560_000_000)]
    #[case(3, -5.5555, -5_556_000_000)]
    #[case(4, -5.55555, -5_555_600_000)]
    #[case(5, -5.555_555, -5_555_560_000)]
    #[case(6, -5.555_555_5, -5_555_556_000)]
    #[case(7, -5.555_555_55, -5_555_555_600)]
    #[case(8, -5.555_555_555, -5_555_555_560)]
    #[case(9, -5.555_555_555_5, -5_555_555_556)]
    fn test_f64_to_fixed_i64(#[case] precision: u8, #[case] value: f64, #[case] expected: i64) {
        assert_eq!(f64_to_fixed_i64(value, precision), expected);
    }

    #[rstest]
    #[case(0, 5.5, 6_000_000_000)]
    #[case(1, 5.55, 5_600_000_000)]
    #[case(2, 5.555, 5_560_000_000)]
    #[case(3, 5.5555, 5_556_000_000)]
    #[case(4, 5.55555, 5_555_600_000)]
    #[case(5, 5.555_555, 5_555_560_000)]
    #[case(6, 5.555_555_5, 5_555_556_000)]
    #[case(7, 5.555_555_55, 5_555_555_600)]
    #[case(8, 5.555_555_555, 5_555_555_560)]
    #[case(9, 5.555_555_555_5, 5_555_555_556)]
    fn test_f64_to_fixed_u64(#[case] precision: u8, #[case] value: f64, #[case] expected: u64) {
        assert_eq!(f64_to_fixed_u64(value, precision), expected);
    }

    #[rstest]
    fn test_fixed_i64_to_f64(
        #[values(1, -1, 2, -2, 10, -10, 100, -100, 1_000, -1_000)] value: i64,
    ) {
        assert_eq!(fixed_i64_to_f64(value), value as f64 / FIXED_SCALAR);
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
        assert_eq!(result, (value as f64) / FIXED_SCALAR);
    }
}
