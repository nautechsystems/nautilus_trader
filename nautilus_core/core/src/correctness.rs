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

const FAILED: &str = "Condition failed:";

/// Validates the string `s` contains only ASCII characters and has semantic meaning.
///
/// # Errors
///
/// - If `s` is an empty string.
/// - If `s` consists solely of whitespace characters.
/// - If `s` contains one or more non-ASCII characters.
pub fn check_valid_string(s: &str, param: &str) -> anyhow::Result<()> {
    if s.is_empty() {
        anyhow::bail!("{FAILED} invalid string for '{param}', was empty")
    } else if s.chars().all(char::is_whitespace) {
        anyhow::bail!("{FAILED} invalid string for '{param}', was all whitespace",)
    } else if !s.is_ascii() {
        anyhow::bail!("{FAILED} invalid string for '{param}' contained a non-ASCII char, was '{s}'",)
    } else {
        Ok(())
    }
}

/// Validates the string `s` if Some, contains only ASCII characters and has semantic meaning.
///
/// # Errors
///
/// - If `s` is an empty string.
/// - If `s` consists solely of whitespace characters.
/// - If `s` contains one or more non-ASCII characters.
pub fn check_valid_string_optional(s: Option<&str>, param: &str) -> anyhow::Result<()> {
    if let Some(s) = s {
        check_valid_string(s, param)?;
    }
    Ok(())
}

/// Validates the string `s` contains the pattern `pat`.
pub fn check_string_contains(s: &str, pat: &str, param: &str) -> anyhow::Result<()> {
    if !s.contains(pat) {
        anyhow::bail!("{FAILED} invalid string for '{param}' did not contain '{pat}', was '{s}'")
    }
    Ok(())
}

/// Validates the `u8` values are equal.
pub fn check_equal_u8(lhs: u8, rhs: u8, lhs_param: &str, rhs_param: &str) -> anyhow::Result<()> {
    if lhs != rhs {
        anyhow::bail!(
            "{FAILED} '{lhs_param}' u8 of {lhs} was not equal to '{rhs_param}' u8 of {rhs}"
        )
    }
    Ok(())
}

/// Validates the `u64` value is positive (> 0).
pub fn check_positive_u64(value: u64, param: &str) -> anyhow::Result<()> {
    if value == 0 {
        anyhow::bail!("{FAILED} invalid u64 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Validates the `i64` value is positive (> 0).
pub fn check_positive_i64(value: i64, param: &str) -> anyhow::Result<()> {
    if value <= 0 {
        anyhow::bail!("{FAILED} invalid i64 for '{param}' not positive, was {value}")
    }
    Ok(())
}

/// Validates the `f64` value is non-negative (< 0).
pub fn check_non_negative_f64(value: f64, param: &str) -> anyhow::Result<()> {
    if value.is_nan() || value.is_infinite() {
        anyhow::bail!("{FAILED} invalid f64 for '{param}', was {value}")
    }
    if value < 0.0 {
        anyhow::bail!("{FAILED} invalid f64 for '{param}' negative, was {value}")
    }
    Ok(())
}

/// Validates the `u8` value is in range [`l`, `r`] (inclusive).
pub fn check_in_range_inclusive_u8(value: u8, l: u8, r: u8, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("{FAILED} invalid u8 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates the `u64` value is range [`l`, `r`] (inclusive).
pub fn check_in_range_inclusive_u64(value: u64, l: u64, r: u64, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("{FAILED} invalid u64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates the `i64` value is in range [`l`, `r`] (inclusive).
pub fn check_in_range_inclusive_i64(value: i64, l: i64, r: i64, param: &str) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("{FAILED} invalid i64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates the `f64` value is in range [`l`, `r`] (inclusive).
pub fn check_in_range_inclusive_f64(value: f64, l: f64, r: f64, param: &str) -> anyhow::Result<()> {
    if value.is_nan() || value.is_infinite() {
        anyhow::bail!("{FAILED} invalid f64 for '{param}', was {value}")
    }
    if value < l || value > r {
        anyhow::bail!("{FAILED} invalid f64 for '{param}' not in range [{l}, {r}], was {value}")
    }
    Ok(())
}

/// Validates the `usize` value is in range [`l`, `r`] (inclusive).
pub fn check_in_range_inclusive_usize(
    value: usize,
    l: usize,
    r: usize,
    param: &str,
) -> anyhow::Result<()> {
    if value < l || value > r {
        anyhow::bail!("{FAILED} invalid usize for '{param}' not in range [{l}, {r}], was {value}")
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
    fn test_check_valid_string_with_valid_value(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_ok());
    }

    #[rstest]
    #[case("")] // <-- empty string
    #[case(" ")] // <-- whitespace-only
    #[case("  ")] // <-- whitespace-only string
    #[case("🦀")] // <-- contains non-ASCII char
    fn test_check_valid_string_with_invalid_values(#[case] s: &str) {
        assert!(check_valid_string(s, "value").is_err());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(" a"))]
    #[case(Some("a "))]
    #[case(Some("a a"))]
    #[case(Some(" a "))]
    #[case(Some("abc"))]
    fn test_check_valid_string_optional_with_valid_value(#[case] s: Option<&str>) {
        assert!(check_valid_string_optional(s, "value").is_ok());
    }

    #[rstest]
    #[case("a", "a")]
    fn test_check_string_contains_when_does_contain(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_ok());
    }

    #[rstest]
    #[case("a", "b")]
    fn test_check_string_contains_when_does_not_contain(#[case] s: &str, #[case] pat: &str) {
        assert!(check_string_contains(s, pat, "value").is_err());
    }

    #[rstest]
    #[case(0, 0, "left", "right")]
    #[case(1, 1, "left", "right")]
    fn test_check_equal_u8_when_equal(
        #[case] lhs: u8,
        #[case] rhs: u8,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
    ) {
        assert!(check_equal_u8(lhs, rhs, lhs_param, rhs_param).is_ok());
    }

    #[rstest]
    #[case(0, 1, "left", "right")]
    #[case(1, 0, "left", "right")]
    fn test_check_equal_u8_when_not_equal(
        #[case] lhs: u8,
        #[case] rhs: u8,
        #[case] lhs_param: &str,
        #[case] rhs_param: &str,
    ) {
        assert!(check_equal_u8(lhs, rhs, lhs_param, rhs_param).is_err());
    }

    #[rstest]
    #[case(1, "value")]
    fn test_check_positive_u64_when_positive(#[case] value: u64, #[case] param: &str) {
        assert!(check_positive_u64(value, param).is_ok());
    }

    #[rstest]
    #[case(0, "value")]
    fn test_check_positive_u64_when_not_positive(#[case] value: u64, #[case] param: &str) {
        assert!(check_positive_u64(value, param).is_err());
    }

    #[rstest]
    #[case(1, "value")]
    fn test_check_positive_i64_when_positive(#[case] value: i64, #[case] param: &str) {
        assert!(check_positive_i64(value, param).is_ok());
    }

    #[rstest]
    #[case(0, "value")]
    #[case(-1, "value")]
    fn test_check_positive_i64_when_not_positive(#[case] value: i64, #[case] param: &str) {
        assert!(check_positive_i64(value, param).is_err());
    }

    #[rstest]
    #[case(0.0, "value")]
    #[case(1.0, "value")]
    fn test_check_non_negative_f64_when_not_negative(#[case] value: f64, #[case] param: &str) {
        assert!(check_non_negative_f64(value, param).is_ok());
    }

    #[rstest]
    #[case(f64::NAN, "value")]
    #[case(f64::INFINITY, "value")]
    #[case(f64::NEG_INFINITY, "value")]
    #[case(-0.1, "value")]
    fn test_check_non_negative_f64_when_negative(#[case] value: f64, #[case] param: &str) {
        assert!(check_non_negative_f64(value, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_u8_when_in_range(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] desc: &str,
    ) {
        assert!(check_in_range_inclusive_u8(value, l, r, desc).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_u8_when_out_of_range(
        #[case] value: u8,
        #[case] l: u8,
        #[case] r: u8,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u8(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_u64_when_in_range(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u64(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_u64_when_out_of_range(
        #[case] value: u64,
        #[case] l: u64,
        #[case] r: u64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_u64(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_i64_when_in_range(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_i64(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_i64_when_out_of_range(
        #[case] value: i64,
        #[case] l: i64,
        #[case] r: i64,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_i64(value, l, r, param).is_err());
    }

    #[rstest]
    #[case(0, 0, 0, "value")]
    #[case(0, 0, 1, "value")]
    #[case(1, 0, 1, "value")]
    fn test_check_in_range_inclusive_usize_when_in_range(
        #[case] value: usize,
        #[case] l: usize,
        #[case] r: usize,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_usize(value, l, r, param).is_ok());
    }

    #[rstest]
    #[case(0, 1, 2, "value")]
    #[case(3, 1, 2, "value")]
    fn test_check_in_range_inclusive_usize_when_out_of_range(
        #[case] value: usize,
        #[case] l: usize,
        #[case] r: usize,
        #[case] param: &str,
    ) {
        assert!(check_in_range_inclusive_usize(value, l, r, param).is_err());
    }
}
