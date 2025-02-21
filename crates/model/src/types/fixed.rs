// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Functions for handling fixed-point arithmetic.
//!
//! This module provides constants and functions that enforce a fixed-point precision strategy,
//! ensuring consistent precision and scaling across various types and calculations.

use nautilus_core::correctness::FAILED;

/// Indicates if high_precision mode is enabled.
#[unsafe(no_mangle)]
pub static HIGH_PRECISION_MODE: u8 = cfg!(feature = "high-precision") as u8;

#[cfg(feature = "high-precision")]
/// The maximum fixed-point precision.
pub const FIXED_PRECISION: u8 = 16;
#[cfg(not(feature = "high-precision"))]
/// The maximum fixed-point precision.
pub const FIXED_PRECISION: u8 = 9;

#[cfg(feature = "high-precision")]
/// The width in bytes for fixed-point value types in high-precision mode (128-bit).
#[unsafe(no_mangle)]
pub static PRECISION_BYTES: i32 = 16;
#[cfg(not(feature = "high-precision"))]
/// The width in bytes for fixed-point value types in standard-precision mode (64-bit).
#[unsafe(no_mangle)]
pub static PRECISION_BYTES: i32 = 8;

#[cfg(feature = "high-precision")]
/// The data type name for fixed size binary.
pub const FIXED_SIZE_BINARY: &str = "FixedSizeBinary(16)";
#[cfg(not(feature = "high-precision"))]
/// The data type name for fixed size binary.
pub const FIXED_SIZE_BINARY: &str = "FixedSizeBinary(8)";

#[cfg(feature = "high-precision")]
/// The scalar value corresponding to the maximum precision (10^16).
pub const FIXED_SCALAR: f64 = 10_000_000_000_000_000.0; // 10.0**FIXED_PRECISION
#[cfg(not(feature = "high-precision"))]
/// The scalar value corresponding to the maximum precision (10^9).
pub const FIXED_SCALAR: f64 = 1_000_000_000.0; // 10.0**FIXED_PRECISION

/// The scalar representing the difference between high-precision and standard-precision modes.
pub const PRECISION_DIFF_SCALAR: f64 = 10_000_000.0; // 10.0**(16-9)

/// Checks if a given `precision` value is within the allowed fixed-point precision range.
///
/// # Errors
///
/// This function returns an error:
/// - If `precision` exceeds [`FIXED_PRECISION`].
pub fn check_fixed_precision(precision: u8) -> anyhow::Result<()> {
    if precision > FIXED_PRECISION {
        anyhow::bail!(
            "`precision` exceeded maximum `FIXED_PRECISION` ({FIXED_PRECISION}), was {precision}"
        )
    }
    Ok(())
}

/// Converts an `f64` value to a raw fixed-point `i64` representation with a specified precision.
///
/// # Panics
///
/// This function panics:
/// - If `precision` exceeds [`FIXED_PRECISION`].
#[must_use]
pub fn f64_to_fixed_i64(value: f64, precision: u8) -> i64 {
    check_fixed_precision(precision).expect(FAILED);
    let pow1 = 10_i64.pow(u32::from(precision));
    let pow2 = 10_i64.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as i64;
    rounded * pow2
}

/// Converts an `f64` value to a raw fixed-point `i128` representation with a specified precision.
///
/// # Panics
///
/// This function panics:
/// - If `precision` exceeds [`FIXED_PRECISION`].
pub fn f64_to_fixed_i128(value: f64, precision: u8) -> i128 {
    check_fixed_precision(precision).expect(FAILED);
    let pow1 = 10_i128.pow(u32::from(precision));
    let pow2 = 10_i128.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as i128;
    rounded * pow2
}

/// Converts an `f64` value to a raw fixed-point `u64` representation with a specified precision.
///
/// # Panics
///
/// This function panics:
/// - If `precision` exceeds [`FIXED_PRECISION`].
#[must_use]
pub fn f64_to_fixed_u64(value: f64, precision: u8) -> u64 {
    check_fixed_precision(precision).expect(FAILED);
    let pow1 = 10_u64.pow(u32::from(precision));
    let pow2 = 10_u64.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as u64;
    rounded * pow2
}

/// Converts an `f64` value to a raw fixed-point `u128` representation with a specified precision.
///
/// # Panics
///
/// This function panics:
/// - If `precision` exceeds [`FIXED_PRECISION`].
#[must_use]
pub fn f64_to_fixed_u128(value: f64, precision: u8) -> u128 {
    check_fixed_precision(precision).expect(FAILED);
    let pow1 = 10_u128.pow(u32::from(precision));
    let pow2 = 10_u128.pow(u32::from(FIXED_PRECISION - precision));
    let rounded = (value * pow1 as f64).round() as u128;
    rounded * pow2
}

/// Converts a raw fixed-point `i64` value back to an `f64` value.
#[must_use]
pub fn fixed_i64_to_f64(value: i64) -> f64 {
    (value as f64) / FIXED_SCALAR
}

/// Converts a raw fixed-point `i128` value back to an `f64` value.
#[must_use]
pub fn fixed_i128_to_f64(value: i128) -> f64 {
    (value as f64) / FIXED_SCALAR
}

/// Converts a raw fixed-point `u64` value back to an `f64` value.
#[must_use]
pub fn fixed_u64_to_f64(value: u64) -> f64 {
    (value as f64) / FIXED_SCALAR
}

/// Converts a raw fixed-point `u128` value back to an `f64` value.
#[must_use]
pub fn fixed_u128_to_f64(value: u128) -> f64 {
    (value as f64) / FIXED_SCALAR
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "high-precision")]
#[cfg(test)]
mod tests {
    use float_cmp::approx_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_precision_boundaries() {
        assert!(check_fixed_precision(0).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION + 1).is_err());
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(-1.0)]
    fn test_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_i128(value, precision);
            let result = fixed_i128_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001, ulps = 16));
        }
    }

    #[rstest]
    #[case(1000000.0)]
    #[case(-1000000.0)]
    fn test_large_value_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_i128(value, precision);
            let result = fixed_i128_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.000_1));
        }
    }

    #[rstest]
    #[case(0, 123456.0, 123456_0000000000000000)]
    #[case(0, 123456.7, 123457_0000000000000000)]
    #[case(1, 123456.7, 123456_7000000000000000)]
    #[case(2, 123456.78, 123456_7800000000000000)]
    #[case(8, 123456.12345678, 123456_1234567800000000)]
    #[case(16, 123456.1234567890123456, 123456_1234567889944576)]
    fn test_precision_specific_values(
        #[case] precision: u8,
        #[case] value: f64,
        #[case] expected: i128,
    ) {
        assert_eq!(f64_to_fixed_i128(value, precision), expected);
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(1000000.0)]
    fn test_unsigned_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_u128(value, precision);
            let result = fixed_u128_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001, ulps = 16));
        }
    }

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
    #[case(16, 0.000_000_000_000_000_1)]
    #[case(0, -0.0)]
    #[case(1, -1.0)]
    #[case(1, -1.1)]
    #[case(9, -0.000_000_001)]
    #[case(16, -0.000_000_000_000_000_1)]
    fn test_f64_to_fixed_i128_to_fixed(#[case] precision: u8, #[case] value: f64) {
        let fixed = f64_to_fixed_i128(value, precision);
        let result = fixed_i128_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0, 0.0)]
    #[case(1, 1.0)]
    #[case(1, 1.1)]
    #[case(9, 0.000_000_001)]
    #[case(16, 0.000_000_000_000_000_1)]
    fn test_f64_to_fixed_u128_to_fixed(#[case] precision: u8, #[case] value: f64) {
        let fixed = f64_to_fixed_u128(value, precision);
        let result = fixed_u128_to_f64(fixed);
        assert_eq!(result, value);
    }

    #[rstest]
    #[case(0, 123_456.0, 1_234_560_000_000_000_000_000)]
    #[case(0, 123_456.7, 1_234_570_000_000_000_000_000)]
    #[case(0, 123_456.4, 1_234_560_000_000_000_000_000)]
    #[case(1, 123_456.0, 1_234_560_000_000_000_000_000)]
    #[case(1, 123_456.7, 1_234_567_000_000_000_000_000)]
    #[case(1, 123_456.4, 1_234_564_000_000_000_000_000)]
    #[case(2, 123_456.0, 1_234_560_000_000_000_000_000)]
    #[case(2, 123_456.7, 1_234_567_000_000_000_000_000)]
    #[case(2, 123_456.4, 1_234_564_000_000_000_000_000)]
    fn test_f64_to_fixed_i128_with_precision(
        #[case] precision: u8,
        #[case] value: f64,
        #[case] expected: i128,
    ) {
        assert_eq!(f64_to_fixed_i128(value, precision), expected);
    }

    #[rstest]
    #[case(0, 5.555555555555555, 60_000_000_000_000_000)]
    #[case(1, 5.555555555555555, 56_000_000_000_000_000)]
    #[case(2, 5.555555555555555, 55_600_000_000_000_000)]
    #[case(3, 5.555555555555555, 55_560_000_000_000_000)]
    #[case(4, 5.555555555555555, 55_556_000_000_000_000)]
    #[case(5, 5.555555555555555, 55_555_600_000_000_000)]
    #[case(6, 5.555555555555555, 55_555_560_000_000_000)]
    #[case(7, 5.555555555555555, 55_555_556_000_000_000)]
    #[case(8, 5.555555555555555, 55_555_555_600_000_000)]
    #[case(9, 5.555555555555555, 55_555_555_560_000_000)]
    #[case(10, 5.555555555555555, 55_555_555_556_000_000)]
    #[case(11, 5.555555555555555, 55_555_555_555_600_000)]
    #[case(12, 5.555555555555555, 55_555_555_555_560_000)]
    #[case(13, 5.555555555555555, 55_555_555_555_556_000)]
    #[case(14, 5.555555555555555, 55_555_555_555_555_600)]
    #[case(15, 5.555555555555555, 55_555_555_555_555_550)]
    #[case(16, 5.555555555555555, 55_555_555_555_555_552)]
    #[case(0, -5.555555555555555, -60_000_000_000_000_000)]
    #[case(1, -5.555555555555555, -56_000_000_000_000_000)]
    #[case(2, -5.555555555555555, -55_600_000_000_000_000)]
    #[case(3, -5.555555555555555, -55_560_000_000_000_000)]
    #[case(4, -5.555555555555555, -55_556_000_000_000_000)]
    #[case(5, -5.555555555555555, -55_555_600_000_000_000)]
    #[case(6, -5.555555555555555, -55_555_560_000_000_000)]
    #[case(7, -5.555555555555555, -55_555_556_000_000_000)]
    #[case(8, -5.555555555555555, -55_555_555_600_000_000)]
    #[case(9, -5.555555555555555, -55_555_555_560_000_000)]
    #[case(10, -5.555555555555555, -55_555_555_556_000_000)]
    #[case(11, -5.555555555555555, -55_555_555_555_600_000)]
    #[case(12, -5.555555555555555, -55_555_555_555_560_000)]
    #[case(13, -5.555555555555555, -55_555_555_555_556_000)]
    #[case(14, -5.555555555555555, -55_555_555_555_555_600)]
    #[case(15, -5.555555555555555, -55_555_555_555_555_550)]
    #[case(16, -5.555555555555555, -55_555_555_555_555_552)]
    fn test_f64_to_fixed_i128(#[case] precision: u8, #[case] value: f64, #[case] expected: i128) {
        assert_eq!(f64_to_fixed_i128(value, precision), expected);
    }

    #[rstest]
    #[case(0, 5.555555555555555, 60_000_000_000_000_000)]
    #[case(1, 5.555555555555555, 56_000_000_000_000_000)]
    #[case(2, 5.555555555555555, 55_600_000_000_000_000)]
    #[case(3, 5.555555555555555, 55_560_000_000_000_000)]
    #[case(4, 5.555555555555555, 55_556_000_000_000_000)]
    #[case(5, 5.555555555555555, 55_555_600_000_000_000)]
    #[case(6, 5.555555555555555, 55_555_560_000_000_000)]
    #[case(7, 5.555555555555555, 55_555_556_000_000_000)]
    #[case(8, 5.555555555555555, 55_555_555_600_000_000)]
    #[case(9, 5.555555555555555, 55_555_555_560_000_000)]
    #[case(10, 5.555555555555555, 55_555_555_556_000_000)]
    #[case(11, 5.555555555555555, 55_555_555_555_600_000)]
    #[case(12, 5.555555555555555, 55_555_555_555_560_000)]
    #[case(13, 5.555555555555555, 55_555_555_555_556_000)]
    #[case(14, 5.555555555555555, 55_555_555_555_555_600)]
    #[case(15, 5.555555555555555, 55_555_555_555_555_550)]
    #[case(16, 5.555555555555555, 55_555_555_555_555_552)]
    fn test_f64_to_fixed_u64(#[case] precision: u8, #[case] value: f64, #[case] expected: u128) {
        assert_eq!(f64_to_fixed_u128(value, precision), expected);
    }

    #[rstest]
    fn test_fixed_i128_to_f64(
        #[values(1, -1, 2, -2, 10, -10, 100, -100, 1_000, -1_000, -10_000, -100_000)] value: i128,
    ) {
        assert_eq!(fixed_i128_to_f64(value), value as f64 / FIXED_SCALAR);
    }

    #[rstest]
    fn test_fixed_u128_to_f64(
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
            1_000_000_000_000_000,
            10_000_000_000_000_000,
            100_000_000_000_000_000,
            1_000_000_000_000_000_000,
            10_000_000_000_000_000_000,
            100_000_000_000_000_000_000
        )]
        value: u128,
    ) {
        let result = fixed_u128_to_f64(value);
        assert_eq!(result, (value as f64) / FIXED_SCALAR);
    }
}

#[cfg(not(feature = "high-precision"))]
#[cfg(test)]
mod tests {
    use float_cmp::approx_eq;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_precision_boundaries() {
        assert!(check_fixed_precision(0).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION + 1).is_err());
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(-1.0)]
    fn test_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_i64(value, precision);
            let result = fixed_i64_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001, ulps = 16));
        }
    }

    #[rstest]
    #[case(1000000.0)]
    #[case(-1000000.0)]
    fn test_large_value_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_i64(value, precision);
            let result = fixed_i64_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.000_1));
        }
    }

    #[rstest]
    #[case(0, 123456.0, 123456_000000000)]
    #[case(0, 123456.7, 123457_000000000)]
    #[case(1, 123456.7, 123456_700000000)]
    #[case(2, 123456.78, 123456_780000000)]
    #[case(8, 123456.12345678, 123456_123456780)]
    #[case(9, 123456.123456789, 123456_123456789)]
    fn test_precision_specific_values(
        #[case] precision: u8,
        #[case] value: f64,
        #[case] expected: i64,
    ) {
        assert_eq!(f64_to_fixed_i64(value, precision), expected);
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(1000000.0)]
    fn test_unsigned_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_u64(value, precision);
            let result = fixed_u64_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001, ulps = 16));
        }
    }

    #[rstest]
    #[case(0, 1.4, 1.0)]
    #[case(0, 1.5, 2.0)]
    #[case(0, 1.6, 2.0)]
    #[case(1, 1.44, 1.4)]
    #[case(1, 1.45, 1.5)]
    #[case(1, 1.46, 1.5)]
    #[case(2, 1.444, 1.44)]
    #[case(2, 1.445, 1.45)]
    #[case(2, 1.446, 1.45)]
    fn test_rounding(#[case] precision: u8, #[case] input: f64, #[case] expected: f64) {
        let fixed = f64_to_fixed_i128(input, precision);
        assert!(approx_eq!(
            f64,
            fixed_i128_to_f64(fixed),
            expected,
            epsilon = 0.000_000_001
        ));
    }

    #[rstest]
    fn test_special_values() {
        // Zero handling
        assert_eq!(f64_to_fixed_i128(0.0, FIXED_PRECISION), 0);
        assert_eq!(f64_to_fixed_i128(-0.0, FIXED_PRECISION), 0);

        // Small values
        let smallest_positive = 1.0 / FIXED_SCALAR;
        let fixed_smallest = f64_to_fixed_i128(smallest_positive, FIXED_PRECISION);
        assert_eq!(fixed_smallest, 1);

        // Large integers
        let large_int = 1_000_000_000.0;
        let fixed_large = f64_to_fixed_i128(large_int, 0);
        assert_eq!(fixed_i128_to_f64(fixed_large), large_int);
    }

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
