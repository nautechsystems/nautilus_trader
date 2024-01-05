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

use anyhow::{bail, Result};

const FAILED: &str = "Condition failed:";

/// Validates the content of a string `s`.
///
/// # Panics
///
/// - If `s` is an empty string.
/// - If `s` consists solely of whitespace characters.
/// - If `s` contains one or more non-ASCII characters.
pub fn check_valid_string(s: &str, desc: &str) -> Result<()> {
    if s.is_empty() {
        bail!("{FAILED} invalid string for {desc}, was empty")
    } else if s.chars().all(char::is_whitespace) {
        bail!("{FAILED} invalid string for {desc}, was all whitespace",)
    } else if !s.is_ascii() {
        bail!("{FAILED} invalid string for {desc} contained a non-ASCII char, was '{s}'",)
    } else {
        Ok(())
    }
}

/// Validates that the string `s` contains the pattern `pat`.
pub fn check_string_contains(s: &str, pat: &str, desc: &str) -> Result<()> {
    if !s.contains(pat) {
        bail!("{FAILED} invalid string for {desc} did not contain '{pat}', was '{s}'")
    }
    Ok(())
}

/// Validates that `u8` values are equal.
pub fn check_u8_equal(lhs: u8, rhs: u8, lhs_param: &str, rhs_param: &str) -> Result<()> {
    if lhs != rhs {
        bail!("{FAILED} '{lhs_param}' u8 of {lhs} was not equal to '{rhs_param}' u8 of {rhs}")
    }
    Ok(())
}

/// Validates that the `u8` value is in the inclusive range [`l`, `r`].
pub fn check_u8_in_range_inclusive(value: u8, l: u8, r: u8, desc: &str) -> Result<()> {
    if value < l || value > r {
        bail!("{FAILED} invalid u8 for {desc} not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates that the `u64` value is in the inclusive range [`l`, `r`].
pub fn check_u64_in_range_inclusive(value: u64, l: u64, r: u64, desc: &str) -> Result<()> {
    if value < l || value > r {
        bail!("{FAILED} invalid u64 for {desc} not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates that the `i64` value is in the inclusive range [`l`, `r`].
pub fn check_i64_in_range_inclusive(value: i64, l: i64, r: i64, desc: &str) -> Result<()> {
    if value < l || value > r {
        bail!("{FAILED} invalid i64 for {desc} not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates that the `f64` value is in the inclusive range [`l`, `r`].
pub fn check_f64_in_range_inclusive(value: f64, l: f64, r: f64, desc: &str) -> Result<()> {
    if value.is_nan() || value.is_infinite() {
        bail!("{FAILED} invalid f64 for {desc}, was {value}")
    }
    if value < l || value > r {
        bail!("{FAILED} invalid f64 for {desc} not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates that the `f64` value is non-negative.
pub fn check_f64_non_negative(value: f64, desc: &str) -> Result<()> {
    if value.is_nan() || value.is_infinite() {
        bail!("{FAILED} invalid f64 for {desc}, was {value}")
    }
    if value < 0.0 {
        bail!("{FAILED} invalid f64 for {desc} negative, was {value}")
    }
    Ok(())
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(" a")]
    #[case("a ")]
    #[case("a a")]
    #[case(" a ")]
    #[case("abc")]
    fn test_valid_string_with_valid_value(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // <-- empty string
    #[case(" ")] // <-- whitespace-only
    #[case("  ")] // <-- whitespace-only string
    #[case("🦀")] // <-- contains non-ASCII char
    fn test_valid_string_with_invalid_values(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_err());
    }

    #[rstest]
    #[case("a", "a")]
    fn test_string_contains_when_it_does_contain(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_ok());
    }

    #[rstest]
    #[case("a", "b")]
    fn test_string_contains_with_invalid_values(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_u8_in_range_inclusive_when_valid_values(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        assert!(check_u8_in_range_inclusive(value, l, r, desc).is_ok());
    }

    #[rstest]
    #[case(0, 1, "left param", "right param")]
    #[case(1, 0, "left param", "right param")]
    fn test_u8_equal_when_invalid_values(
        #[case] lhs: u8,
        #[case] rhs: u8,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
    ) {
        assert!(check_u8_equal(lhs, rhs, lhs_param, rhs_param).is_err());
    }

    #[rstest]
    #[case(0, 0, "left param", "right param")]
    fn test_u8_equal_when_valid_values(
        #[case] lhs: u8,
        #[case] rhs: u8,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
    ) {
        assert!(check_u8_equal(lhs, rhs, lhs_param, rhs_param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_u8_in_range_inclusive_when_invalid_values(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        assert!(check_u8_in_range_inclusive(value, l, r, desc).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_u64_in_range_inclusive_when_valid_values(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] desc: &str,
    ) {
        assert!(check_u64_in_range_inclusive(value, l, r, desc).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_u64_in_range_inclusive_when_invalid_values(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] desc: &str,
    ) {
        assert!(check_u64_in_range_inclusive(value, l, r, desc).is_err())
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_i64_in_range_inclusive_when_valid_values(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] desc: &str,
    ) {
        assert!(check_i64_in_range_inclusive(value, l, r, desc).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_i64_in_range_inclusive_when_invalid_values(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] desc: &str,
    ) {
        assert!(check_i64_in_range_inclusive(value, l, r, desc).is_err());
    }

    #[rstest]
    #[case(0.0, "value")]
    #[case(1.0, "value")]
    fn test_f64_non_negative_when_valid_values(#[case] value: f64, #[case] desc: &str) {
        assert!(check_f64_non_negative(value, desc).is_ok());
    }

    #[rstest]
    #[case(f64::NAN, "value")]
    #[case(f64::INFINITY, "value")]
    #[case(f64::NEG_INFINITY, "value")]
    #[case(-0.1, "value")]
    fn test_f64_non_negative_when_invalid_values(#[case] value: f64, #[case] desc: &str) {
        assert!(check_f64_non_negative(value, desc).is_err());
    }
}
