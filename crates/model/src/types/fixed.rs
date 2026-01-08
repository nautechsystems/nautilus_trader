// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
//!
//! # Raw Value Requirements
//!
//! When constructing value types like [`Price`] or [`Quantity`] using `from_raw`, the raw value
//! **must** be a valid multiple of the scale factor for the given precision. Valid raw values
//! should ideally come from:
//!
//! - Accessing the `.raw` field of an existing value (e.g., `price.raw`)
//! - Using the fixed-point conversion functions in this module
//! - Values from Nautilus-produced Arrow data
//!
//! Raw values that are not valid multiples will cause a panic on construction in debug builds,
//! and may result in incorrect values in release builds.
//!
//! # Legacy Catalog Data and Floating-Point Errors
//!
//! Data written to catalogs using V2 wranglers before 16th December 2025 may contain raw values with
//! floating-point precision errors. This occurred because the wranglers used:
//!
//! ```text
//! int(value * FIXED_SCALAR)  # Introduces floating-point errors
//! ```
//!
//! instead of the correct precision-aware approach:
//!
//! ```text
//! round(value * 10^precision) * scale  # Correct
//! ```
//!
//! # Raw Value Correction
//!
//! To handle legacy data with floating-point errors, the Arrow decode path uses correction
//! functions ([`correct_raw_i64`], [`correct_raw_i128`], etc.) to round raw values to the
//! nearest valid multiple. This ensures backward compatibility with existing catalogs.
//!
//! **Note:** This correction adds a small amount of overhead during decoding. In a future
//! version, once catalogs have been repaired or migrated, this correction will become opt-in.
//!
//! [`Price`]: crate::types::Price
//! [`Quantity`]: crate::types::Quantity

use std::fmt::Display;

use nautilus_core::correctness::FAILED;

use crate::types::{price::PriceRaw, quantity::QuantityRaw};

/// Indicates if high-precision mode is enabled.
///
/// # Safety
///
/// This static variable is initialized at compile time and never mutated,
/// making it safe to read from multiple threads without synchronization.
/// The value is determined by the "high-precision" feature flag.
#[unsafe(no_mangle)]
#[allow(unsafe_code)]
pub static HIGH_PRECISION_MODE: u8 = cfg!(feature = "high-precision") as u8;

// -----------------------------------------------------------------------------
// FIXED_PRECISION
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The maximum fixed-point precision.
pub const FIXED_PRECISION: u8 = 16;

#[cfg(not(feature = "high-precision"))]
/// The maximum fixed-point precision.
pub const FIXED_PRECISION: u8 = 9;

// -----------------------------------------------------------------------------
// PRECISION_BYTES (size of integer backing the fixed-point values)
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The width in bytes for fixed-point value types in high-precision mode (128-bit).
pub const PRECISION_BYTES: i32 = 16;

#[cfg(not(feature = "high-precision"))]
/// The width in bytes for fixed-point value types in standard-precision mode (64-bit).
pub const PRECISION_BYTES: i32 = 8;

// -----------------------------------------------------------------------------
// FIXED_BINARY_SIZE
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The data type name for the Arrow fixed-size binary representation.
pub const FIXED_SIZE_BINARY: &str = "FixedSizeBinary(16)";

#[cfg(not(feature = "high-precision"))]
/// The data type name for the Arrow fixed-size binary representation.
pub const FIXED_SIZE_BINARY: &str = "FixedSizeBinary(8)";

// -----------------------------------------------------------------------------
// FIXED_SCALAR
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The scalar value corresponding to the maximum precision (10^16).
pub const FIXED_SCALAR: f64 = 10_000_000_000_000_000.0;

#[cfg(not(feature = "high-precision"))]
/// The scalar value corresponding to the maximum precision (10^9).
pub const FIXED_SCALAR: f64 = 1_000_000_000.0;

// -----------------------------------------------------------------------------
// PRECISION_DIFF_SCALAR
// -----------------------------------------------------------------------------

#[cfg(feature = "high-precision")]
/// The scalar representing the difference between high-precision and standard-precision modes.
pub const PRECISION_DIFF_SCALAR: f64 = 10_000_000.0; // 10^(16-9)

#[cfg(not(feature = "high-precision"))]
/// The scalar representing the difference between high-precision and standard-precision modes.
pub const PRECISION_DIFF_SCALAR: f64 = 1.0;

// -----------------------------------------------------------------------------
// POWERS_OF_10 (lookup table for fast validation)
// -----------------------------------------------------------------------------

/// Precomputed powers of 10 for fast scale lookup.
///
/// Index i contains 10^i. Table covers 10^0 through 10^16 (sufficient for FIXED_PRECISION).
/// Used by `check_fixed_raw_*` functions to avoid runtime exponentiation.
const POWERS_OF_10: [u64; 17] = [
    1,                      // 10^0
    10,                     // 10^1
    100,                    // 10^2
    1_000,                  // 10^3
    10_000,                 // 10^4
    100_000,                // 10^5
    1_000_000,              // 10^6
    10_000_000,             // 10^7
    100_000_000,            // 10^8
    1_000_000_000,          // 10^9
    10_000_000_000,         // 10^10
    100_000_000_000,        // 10^11
    1_000_000_000_000,      // 10^12
    10_000_000_000_000,     // 10^13
    100_000_000_000_000,    // 10^14
    1_000_000_000_000_000,  // 10^15
    10_000_000_000_000_000, // 10^16
];

// Compile-time verification that FIXED_PRECISION is within table bounds.
// We index POWERS_OF_10[FIXED_PRECISION] when precision=0, so need strict `<`.
const _: () = assert!(
    (FIXED_PRECISION as usize) < POWERS_OF_10.len(),
    "FIXED_PRECISION exceeds POWERS_OF_10 table size"
);

// -----------------------------------------------------------------------------

/// The maximum precision that can be safely used with f64-based constructors.
///
/// This is a hard limit imposed by IEEE 754 double-precision floating-point representation,
/// which has approximately 15-17 significant decimal digits. Beyond 16 decimal places,
/// floating-point arithmetic becomes unreliable due to rounding errors.
///
/// For higher precision values (such as 18-decimal wei values in DeFi), specialized
/// constructors that work with integer representations should be used instead.
pub const MAX_FLOAT_PRECISION: u8 = 16;

/// Checks if a given `precision` value is within the allowed fixed-point precision range.
///
/// # Errors
///
/// Returns an error if `precision` exceeds the maximum allowed:
/// - With `defi` feature: [`WEI_PRECISION`](crate::defi::WEI_PRECISION) (18)
/// - Without `defi` feature: [`FIXED_PRECISION`]
pub fn check_fixed_precision(precision: u8) -> anyhow::Result<()> {
    #[cfg(feature = "defi")]
    if precision > crate::defi::WEI_PRECISION {
        anyhow::bail!("`precision` exceeded maximum `WEI_PRECISION` (18), was {precision}")
    }

    #[cfg(not(feature = "defi"))]
    if precision > FIXED_PRECISION {
        anyhow::bail!(
            "`precision` exceeded maximum `FIXED_PRECISION` ({FIXED_PRECISION}), was {precision}"
        )
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Raw value validation
// -----------------------------------------------------------------------------

/// Returns `Ok(true)` if validation should be skipped, `Ok(false)` to proceed.
///
/// Validation is skipped when precision >= FIXED_PRECISION because every bit of the raw
/// value is significant. For precision > FIXED_PRECISION without the defi feature,
/// a debug assertion fires to surface potential misuse during development.
#[inline(always)]
fn should_skip_validation(precision: u8) -> anyhow::Result<bool> {
    if precision == FIXED_PRECISION {
        return Ok(true);
    }

    if precision > FIXED_PRECISION {
        // Only assert when defi feature is disabled - with defi, 18dp is legitimate
        #[cfg(not(feature = "defi"))]
        debug_assert!(
            false,
            "precision {precision} exceeds FIXED_PRECISION {FIXED_PRECISION}: \
             raw value validation is not possible at this precision"
        );
        return Ok(true);
    }

    Ok(false)
}

/// Builds the error for invalid fixed-point raw values (cold path).
#[cold]
fn invalid_raw_error(
    raw: impl Display,
    precision: u8,
    remainder: impl Display,
    scale: impl Display,
) -> anyhow::Error {
    anyhow::anyhow!(
        "Invalid fixed-point raw value {raw} for precision {precision}: \
         remainder {remainder} when divided by scale {scale}. \
         Raw value should be a multiple of {scale}. \
         This indicates data corruption or incorrect precision/scaling upstream"
    )
}

/// Checks that a raw unsigned fixed-point value has no spurious bits beyond the precision scale.
///
/// For a given precision P where P < FIXED_PRECISION, valid raw values must be exact
/// multiples of 10^(FIXED_PRECISION - P). Any non-zero remainder indicates data corruption
/// or incorrect scaling upstream.
///
/// # Precision Limits
///
/// This check only validates when `precision < FIXED_PRECISION`:
/// - When `precision == FIXED_PRECISION`, every bit of the raw value is significant and
///   the check passes trivially (no "extra" bits to validate).
/// - When `precision > FIXED_PRECISION` (possible with defi feature allowing up to 18dp),
///   validation is not possible because the requested precision exceeds our internal
///   representation. A debug assertion will fire to surface this during development.
///
/// **Important**: For defi 18dp values, this check provides NO protection against incorrectly scaled
/// raw values. The inherent limitation is that we cannot detect if a 16dp raw is incorrectly
/// labeled as 18dp, since both would appear valid at full internal precision.
///
/// # Example
///
/// With FIXED_PRECISION=9 and precision=0:
/// - Valid: raw=120_000_000_000 (120 * 10^9, divisible by 10^9)
/// - Invalid: raw=119_582_001_968_421_736 (remainder 968_421_736 when divided by 10^9)
///
/// # Errors
///
/// Returns an error if the raw value has non-zero bits beyond the precision scale
/// (only when `precision < FIXED_PRECISION`).
#[inline(always)]
pub fn check_fixed_raw_u128(raw: u128, precision: u8) -> anyhow::Result<()> {
    if should_skip_validation(precision)? {
        return Ok(());
    }

    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = u128::from(POWERS_OF_10[exp]);
    let remainder = raw % scale;

    if remainder != 0 {
        return Err(invalid_raw_error(raw, precision, remainder, scale));
    }

    Ok(())
}

/// Checks that a raw unsigned fixed-point value (64-bit) has no spurious bits.
///
/// Uses direct u64 arithmetic for better performance than widening to u128.
/// See [`check_fixed_raw_u128`] for full documentation on precision limits and behavior.
///
/// # Errors
///
/// Returns an error if the raw value has non-zero bits beyond the precision scale.
#[inline(always)]
pub fn check_fixed_raw_u64(raw: u64, precision: u8) -> anyhow::Result<()> {
    if should_skip_validation(precision)? {
        return Ok(());
    }

    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = POWERS_OF_10[exp];
    let remainder = raw % scale;

    if remainder != 0 {
        return Err(invalid_raw_error(raw, precision, remainder, scale));
    }

    Ok(())
}

/// Checks that a raw signed fixed-point value has no spurious bits beyond the precision scale.
///
/// For a given precision P where P < FIXED_PRECISION, valid raw values must be exact
/// multiples of 10^(FIXED_PRECISION - P). Any non-zero remainder indicates data corruption
/// or incorrect scaling upstream.
///
/// # Precision Limits
///
/// This check only validates when `precision < FIXED_PRECISION`:
/// - When `precision == FIXED_PRECISION`, every bit of the raw value is significant and
///   the check passes trivially (no "extra" bits to validate).
/// - When `precision > FIXED_PRECISION` (possible with defi feature allowing up to 18dp),
///   validation is not possible because the requested precision exceeds our internal
///   representation. A debug assertion will fire to surface this during development.
///
/// **Important**: For defi 18dp values, this check provides NO protection against incorrectly scaled
/// raw values. The inherent limitation is that we cannot detect if a 16dp raw is incorrectly
/// labeled as 18dp, since both would appear valid at full internal precision.
///
/// # Example
///
/// With FIXED_PRECISION=9 and precision=0:
/// - Valid: raw=120_000_000_000 (120 * 10^9, divisible by 10^9)
/// - Invalid: raw=119_582_001_968_421_736 (remainder 968_421_736 when divided by 10^9)
///
/// # Errors
///
/// Returns an error if the raw value has non-zero bits beyond the precision scale
/// (only when `precision < FIXED_PRECISION`).
#[inline(always)]
pub fn check_fixed_raw_i128(raw: i128, precision: u8) -> anyhow::Result<()> {
    if should_skip_validation(precision)? {
        return Ok(());
    }

    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = i128::from(POWERS_OF_10[exp]);
    let remainder = raw % scale;

    if remainder != 0 {
        return Err(invalid_raw_error(raw, precision, remainder, scale));
    }

    Ok(())
}

/// Checks that a raw signed fixed-point value (64-bit) has no spurious bits.
///
/// Uses direct i64 arithmetic for better performance than widening to i128.
/// See [`check_fixed_raw_i128`] for full documentation on precision limits and behavior.
///
/// # Errors
///
/// Returns an error if the raw value has non-zero bits beyond the precision scale.
#[inline(always)]
pub fn check_fixed_raw_i64(raw: i64, precision: u8) -> anyhow::Result<()> {
    if should_skip_validation(precision)? {
        return Ok(());
    }

    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = POWERS_OF_10[exp] as i64;
    let remainder = raw % scale;

    if remainder != 0 {
        return Err(invalid_raw_error(raw, precision, remainder, scale));
    }

    Ok(())
}

// -----------------------------------------------------------------------------
// Raw value correction functions
// -----------------------------------------------------------------------------
// These functions round raw values to the nearest valid multiple of the scale
// factor for a given precision. This is needed when reading data from catalogs
// or other sources that may have been created with floating-point precision
// errors (e.g., `int(value * FIXED_SCALAR)` instead of the correct
// `round(value * 10^precision) * scale` approach).

/// Rounds a raw `u128` value to the nearest valid multiple of the scale for the given precision.
///
/// This corrects raw values that have spurious bits beyond the precision scale, which can occur
/// from floating-point conversion errors during data creation.
#[must_use]
pub fn correct_raw_u128(raw: u128, precision: u8) -> u128 {
    if precision >= FIXED_PRECISION {
        return raw;
    }
    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = u128::from(POWERS_OF_10[exp]);
    let half_scale = scale / 2;
    let remainder = raw % scale;
    if remainder == 0 {
        raw
    } else if remainder >= half_scale {
        raw + (scale - remainder)
    } else {
        raw - remainder
    }
}

/// Rounds a raw `u64` value to the nearest valid multiple of the scale for the given precision.
///
/// This corrects raw values that have spurious bits beyond the precision scale, which can occur
/// from floating-point conversion errors during data creation.
#[must_use]
pub fn correct_raw_u64(raw: u64, precision: u8) -> u64 {
    if precision >= FIXED_PRECISION {
        return raw;
    }
    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = POWERS_OF_10[exp];
    let half_scale = scale / 2;
    let remainder = raw % scale;
    if remainder == 0 {
        raw
    } else if remainder >= half_scale {
        raw + (scale - remainder)
    } else {
        raw - remainder
    }
}

/// Rounds a raw `i128` value to the nearest valid multiple of the scale for the given precision.
///
/// This corrects raw values that have spurious bits beyond the precision scale, which can occur
/// from floating-point conversion errors during data creation.
#[must_use]
pub fn correct_raw_i128(raw: i128, precision: u8) -> i128 {
    if precision >= FIXED_PRECISION {
        return raw;
    }
    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = i128::from(POWERS_OF_10[exp]);
    let half_scale = scale / 2;
    let remainder = raw % scale;
    if remainder == 0 {
        raw
    } else if raw >= 0 {
        if remainder >= half_scale {
            raw + (scale - remainder)
        } else {
            raw - remainder
        }
    } else {
        // For negative values, remainder is negative
        if remainder.abs() >= half_scale {
            raw - (scale + remainder)
        } else {
            raw - remainder
        }
    }
}

/// Rounds a raw `i64` value to the nearest valid multiple of the scale for the given precision.
///
/// This corrects raw values that have spurious bits beyond the precision scale, which can occur
/// from floating-point conversion errors during data creation.
#[must_use]
pub fn correct_raw_i64(raw: i64, precision: u8) -> i64 {
    if precision >= FIXED_PRECISION {
        return raw;
    }
    let exp = usize::from(FIXED_PRECISION - precision);
    let scale = POWERS_OF_10[exp] as i64;
    let half_scale = scale / 2;
    let remainder = raw % scale;
    if remainder == 0 {
        raw
    } else if raw >= 0 {
        if remainder >= half_scale {
            raw + (scale - remainder)
        } else {
            raw - remainder
        }
    } else {
        // For negative values, remainder is negative
        if remainder.abs() >= half_scale {
            raw - (scale + remainder)
        } else {
            raw - remainder
        }
    }
}

/// Rounds a raw price value to the nearest valid multiple of the scale for the given precision.
///
/// This is a type-aliased wrapper that calls the appropriate underlying function based on
/// whether the `high-precision` feature is enabled. Use this when working with `PriceRaw` values
/// to ensure consistent feature-flag handling.
#[must_use]
#[inline]
pub fn correct_price_raw(raw: PriceRaw, precision: u8) -> PriceRaw {
    #[cfg(feature = "high-precision")]
    {
        correct_raw_i128(raw, precision)
    }
    #[cfg(not(feature = "high-precision"))]
    {
        correct_raw_i64(raw, precision)
    }
}

/// Rounds a raw quantity value to the nearest valid multiple of the scale for the given precision.
///
/// This is a type-aliased wrapper that calls the appropriate underlying function based on
/// whether the `high-precision` feature is enabled. Use this when working with `QuantityRaw` values
/// to ensure consistent feature-flag handling.
#[must_use]
#[inline]
pub fn correct_quantity_raw(raw: QuantityRaw, precision: u8) -> QuantityRaw {
    #[cfg(feature = "high-precision")]
    {
        correct_raw_u128(raw, precision)
    }
    #[cfg(not(feature = "high-precision"))]
    {
        correct_raw_u64(raw, precision)
    }
}

/// Converts an `f64` value to a raw fixed-point `i64` representation with a specified precision.
///
/// # Precision and Rounding
///
/// This function performs IEEE 754 "round half to even" rounding at the specified precision
/// before scaling to the fixed-point representation. The rounding is intentionally applied
/// at the user-specified precision level to ensure values are correctly represented
/// without accumulating floating-point errors during scaling.
///
/// # Panics
///
/// Panics if `precision` exceeds [`FIXED_PRECISION`].
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
/// Panics if `precision` exceeds [`FIXED_PRECISION`].
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
/// Panics if `precision` exceeds [`FIXED_PRECISION`].
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
/// Panics if `precision` exceeds [`FIXED_PRECISION`].
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

#[cfg(feature = "high-precision")]
#[cfg(test)]
mod tests {
    use nautilus_core::approx_eq;
    use rstest::rstest;

    use super::*;

    #[cfg(not(feature = "high-precision"))]
    #[rstest]
    fn test_precision_boundaries() {
        assert!(check_fixed_precision(0).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION).is_ok());
        assert!(check_fixed_precision(FIXED_PRECISION + 1).is_err());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_precision_boundaries() {
        use crate::defi::WEI_PRECISION;

        assert!(check_fixed_precision(0).is_ok());
        assert!(check_fixed_precision(WEI_PRECISION).is_ok());
        assert!(check_fixed_precision(WEI_PRECISION + 1).is_err());
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(-1.0)]
    fn test_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_i128(value, precision);
            let result = fixed_i128_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001));
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
    #[case(0, 123456.0)]
    #[case(0, 123456.7)]
    #[case(1, 123456.7)]
    #[case(2, 123456.78)]
    #[case(8, 123456.12345678)]
    fn test_precision_specific_values_basic(#[case] precision: u8, #[case] value: f64) {
        let result = f64_to_fixed_i128(value, precision);
        let back_converted = fixed_i128_to_f64(result);
        // Round-trip should preserve the value up to the specified precision
        let scale = 10.0_f64.powi(precision as i32);
        let expected_rounded = (value * scale).round() / scale;
        assert!((back_converted - expected_rounded).abs() < 1e-10);
    }

    #[rstest]
    fn test_max_precision_values() {
        // Test with maximum precision that the current feature set supports
        let test_value = 123456.123456789;
        let result = f64_to_fixed_i128(test_value, FIXED_PRECISION);
        let back_converted = fixed_i128_to_f64(result);
        // For maximum precision, we expect some floating-point limitations
        assert!((back_converted - test_value).abs() < 1e-6);
    }

    #[rstest]
    #[case(0.0)]
    #[case(1.0)]
    #[case(1000000.0)]
    fn test_unsigned_basic_roundtrip(#[case] value: f64) {
        for precision in 0..=FIXED_PRECISION {
            let fixed = f64_to_fixed_u128(value, precision);
            let result = fixed_u128_to_f64(fixed);
            assert!(approx_eq!(f64, value, result, epsilon = 0.001));
        }
    }

    #[rstest]
    #[case(0)]
    #[case(FIXED_PRECISION)]
    fn test_valid_precision(#[case] precision: u8) {
        let result = check_fixed_precision(precision);
        assert!(result.is_ok());
    }

    #[cfg(not(feature = "defi"))]
    #[rstest]
    fn test_invalid_precision() {
        let precision = FIXED_PRECISION + 1;
        let result = check_fixed_precision(precision);
        assert!(result.is_err());
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_invalid_precision() {
        use crate::defi::WEI_PRECISION;
        let precision = WEI_PRECISION + 1;
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
    #[case(0, 123_456.0)]
    #[case(0, 123_456.7)]
    #[case(0, 123_456.4)]
    #[case(1, 123_456.0)]
    #[case(1, 123_456.7)]
    #[case(1, 123_456.4)]
    #[case(2, 123_456.0)]
    #[case(2, 123_456.7)]
    #[case(2, 123_456.4)]
    fn test_f64_to_fixed_i128_with_precision(#[case] precision: u8, #[case] value: f64) {
        let result = f64_to_fixed_i128(value, precision);

        // Calculate expected value dynamically based on current FIXED_PRECISION
        let pow1 = 10_i128.pow(u32::from(precision));
        let pow2 = 10_i128.pow(u32::from(FIXED_PRECISION - precision));
        let rounded = (value * pow1 as f64).round() as i128;
        let expected = rounded * pow2;

        assert_eq!(
            result, expected,
            "Failed for precision {precision}, value {value}: got {result}, expected {expected}"
        );
    }

    #[rstest]
    #[case(0, 5.555555555555555)]
    #[case(1, 5.555555555555555)]
    #[case(2, 5.555555555555555)]
    #[case(3, 5.555555555555555)]
    #[case(4, 5.555555555555555)]
    #[case(5, 5.555555555555555)]
    #[case(6, 5.555555555555555)]
    #[case(7, 5.555555555555555)]
    #[case(8, 5.555555555555555)]
    #[case(9, 5.555555555555555)]
    #[case(10, 5.555555555555555)]
    #[case(11, 5.555555555555555)]
    #[case(12, 5.555555555555555)]
    #[case(13, 5.555555555555555)]
    #[case(14, 5.555555555555555)]
    #[case(15, 5.555555555555555)]
    #[case(0, -5.555555555555555)]
    #[case(1, -5.555555555555555)]
    #[case(2, -5.555555555555555)]
    #[case(3, -5.555555555555555)]
    #[case(4, -5.555555555555555)]
    #[case(5, -5.555555555555555)]
    #[case(6, -5.555555555555555)]
    #[case(7, -5.555555555555555)]
    #[case(8, -5.555555555555555)]
    #[case(9, -5.555555555555555)]
    #[case(10, -5.555555555555555)]
    #[case(11, -5.555555555555555)]
    #[case(12, -5.555555555555555)]
    #[case(13, -5.555555555555555)]
    #[case(14, -5.555555555555555)]
    #[case(15, -5.555555555555555)]
    fn test_f64_to_fixed_i128(#[case] precision: u8, #[case] value: f64) {
        // Only test up to the current FIXED_PRECISION
        if precision > FIXED_PRECISION {
            return;
        }

        let result = f64_to_fixed_i128(value, precision);

        // Calculate expected value dynamically based on current FIXED_PRECISION
        let pow1 = 10_i128.pow(u32::from(precision));
        let pow2 = 10_i128.pow(u32::from(FIXED_PRECISION - precision));
        let rounded = (value * pow1 as f64).round() as i128;
        let expected = rounded * pow2;

        assert_eq!(
            result, expected,
            "Failed for precision {precision}, value {value}: got {result}, expected {expected}"
        );
    }

    #[rstest]
    #[case(0, 5.555555555555555)]
    #[case(1, 5.555555555555555)]
    #[case(2, 5.555555555555555)]
    #[case(3, 5.555555555555555)]
    #[case(4, 5.555555555555555)]
    #[case(5, 5.555555555555555)]
    #[case(6, 5.555555555555555)]
    #[case(7, 5.555555555555555)]
    #[case(8, 5.555555555555555)]
    #[case(9, 5.555555555555555)]
    #[case(10, 5.555555555555555)]
    #[case(11, 5.555555555555555)]
    #[case(12, 5.555555555555555)]
    #[case(13, 5.555555555555555)]
    #[case(14, 5.555555555555555)]
    #[case(15, 5.555555555555555)]
    #[case(16, 5.555555555555555)]
    fn test_f64_to_fixed_u64(#[case] precision: u8, #[case] value: f64) {
        // Only test up to the current FIXED_PRECISION
        if precision > FIXED_PRECISION {
            return;
        }

        let result = f64_to_fixed_u128(value, precision);

        // Calculate expected value dynamically based on current FIXED_PRECISION
        let pow1 = 10_u128.pow(u32::from(precision));
        let pow2 = 10_u128.pow(u32::from(FIXED_PRECISION - precision));
        let rounded = (value * pow1 as f64).round() as u128;
        let expected = rounded * pow2;

        assert_eq!(
            result, expected,
            "Failed for precision {precision}, value {value}: got {result}, expected {expected}"
        );
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

    // -------------------------------------------------------------------------
    // Raw value validation tests (high-precision: FIXED_PRECISION = 16)
    // -------------------------------------------------------------------------

    #[rstest]
    #[case(0, 0)] // Zero is always valid
    #[case(0, 10_000_000_000_000_000)] // 1 * 10^16 at precision 0
    #[case(0, 1_200_000_000_000_000_000)] // 120 * 10^16 at precision 0
    #[case(8, 12_345_678_900_000_000)] // 123456789 * 10^8 at precision 8
    #[case(15, 1_234_567_890_123_450)] // Multiple of 10 at precision 15
    fn test_check_fixed_raw_u128_valid(#[case] precision: u8, #[case] raw: u128) {
        assert!(check_fixed_raw_u128(raw, precision).is_ok());
    }

    #[rstest]
    #[case(0, 1)] // Not multiple of 10^16
    #[case(0, 9_999_999_999_999_999)] // One less than scale
    #[case(0, 10_000_000_000_000_001)] // One more than 10^16
    #[case(8, 12_345_678_900_000_001)] // Not multiple of 10^8
    #[case(15, 1_234_567_890_123_451)] // Not multiple of 10
    fn test_check_fixed_raw_u128_invalid(#[case] precision: u8, #[case] raw: u128) {
        assert!(check_fixed_raw_u128(raw, precision).is_err());
    }

    #[rstest]
    fn test_check_fixed_raw_u128_at_max_precision() {
        // At FIXED_PRECISION (16), validation is skipped
        assert!(check_fixed_raw_u128(0, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u128(1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u128(123_456_789, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u128(u128::MAX, FIXED_PRECISION).is_ok());
    }

    #[rstest]
    #[case(0, 0)]
    #[case(0, 10_000_000_000_000_000)]
    #[case(0, -10_000_000_000_000_000)]
    #[case(8, 12_345_678_900_000_000)]
    #[case(8, -12_345_678_900_000_000)]
    fn test_check_fixed_raw_i128_valid(#[case] precision: u8, #[case] raw: i128) {
        assert!(check_fixed_raw_i128(raw, precision).is_ok());
    }

    #[rstest]
    #[case(0, 1)]
    #[case(0, -1)]
    #[case(0, 9_999_999_999_999_999)]
    #[case(0, -9_999_999_999_999_999)]
    fn test_check_fixed_raw_i128_invalid(#[case] precision: u8, #[case] raw: i128) {
        assert!(check_fixed_raw_i128(raw, precision).is_err());
    }

    #[rstest]
    fn test_check_fixed_raw_i128_at_max_precision() {
        assert!(check_fixed_raw_i128(0, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i128(1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i128(-1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i128(i128::MAX, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i128(i128::MIN, FIXED_PRECISION).is_ok());
    }
}

#[cfg(not(feature = "high-precision"))]
#[cfg(test)]
mod tests {
    use nautilus_core::approx_eq;
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
            assert!(approx_eq!(f64, value, result, epsilon = 0.001));
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
            assert!(approx_eq!(f64, value, result, epsilon = 0.001));
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

    #[rstest]
    #[case(0, 0)] // Zero is always valid
    #[case(0, 1_000_000_000)] // 1 * 10^9 at precision 0
    #[case(0, 120_000_000_000)] // 120 * 10^9 at precision 0
    #[case(2, 123_450_000_000)] // 12345 * 10^7 at precision 2
    #[case(8, 1_234_567_890)] // 123456789 * 10 at precision 8
    fn test_check_fixed_raw_u64_valid(#[case] precision: u8, #[case] raw: u64) {
        assert!(check_fixed_raw_u64(raw, precision).is_ok());
    }

    #[rstest]
    #[case(0, 1)] // Not multiple of 10^9
    #[case(0, 999_999_999)] // One less than scale
    #[case(0, 1_000_000_001)] // One more than 10^9
    #[case(0, 119_582_001_968_421_736)] // The original bug case
    #[case(2, 123_456_789_000)] // Not multiple of 10^7
    #[case(8, 1_234_567_891)] // Not multiple of 10
    fn test_check_fixed_raw_u64_invalid(#[case] precision: u8, #[case] raw: u64) {
        assert!(check_fixed_raw_u64(raw, precision).is_err());
    }

    #[rstest]
    fn test_check_fixed_raw_u64_at_max_precision() {
        // At FIXED_PRECISION, validation is skipped - any value is valid
        assert!(check_fixed_raw_u64(0, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u64(1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u64(123_456_789, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_u64(u64::MAX, FIXED_PRECISION).is_ok());
    }

    #[rstest]
    #[case(0, 0)]
    #[case(0, 1_000_000_000)]
    #[case(0, -1_000_000_000)]
    #[case(2, 123_450_000_000)]
    #[case(2, -123_450_000_000)]
    fn test_check_fixed_raw_i64_valid(#[case] precision: u8, #[case] raw: i64) {
        assert!(check_fixed_raw_i64(raw, precision).is_ok());
    }

    #[rstest]
    #[case(0, 1)]
    #[case(0, -1)]
    #[case(0, 999_999_999)]
    #[case(0, -999_999_999)]
    fn test_check_fixed_raw_i64_invalid(#[case] precision: u8, #[case] raw: i64) {
        assert!(check_fixed_raw_i64(raw, precision).is_err());
    }

    #[rstest]
    fn test_check_fixed_raw_i64_at_max_precision() {
        assert!(check_fixed_raw_i64(0, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i64(1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i64(-1, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i64(i64::MAX, FIXED_PRECISION).is_ok());
        assert!(check_fixed_raw_i64(i64::MIN, FIXED_PRECISION).is_ok());
    }
}
