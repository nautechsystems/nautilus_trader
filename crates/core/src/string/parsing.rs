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

//! Core parsing functions.

/// Clamps a length to `u8::MAX` with optional debug logging.
#[inline]
#[must_use]
#[expect(
    clippy::cast_possible_truncation,
    reason = "Intentional for parsing, value range validated"
)]
fn clamp_precision_with_log(len: usize, context: &str, input: &str) -> u8 {
    if len > u8::MAX as usize {
        log::debug!(
            "{} precision clamped from {} to {} for input: {}",
            context,
            len,
            u8::MAX,
            input
        );
    }
    len.min(u8::MAX as usize) as u8
}

/// Computes decimal precision from a scientific-notation string.
///
/// Precision is `max(0, mantissa_fractional_digits - exponent)`, clamped to
/// `u8::MAX` (255). When `trim_trailing_zeros` is true, trailing zeros in the
/// mantissa's fractional part are stripped before counting.
///
/// Absurd exponents that overflow `i64` clamp to 255 (negative) or 0 (positive)
/// without panicking.
///
/// # Panics
///
/// Panics when `strict` is true and the exponent is missing or non-numeric.
fn precision_from_scientific(s: &str, trim_trailing_zeros: bool, strict: bool) -> Option<u8> {
    let e_pos = s.find('e')?;
    let mantissa = &s[..e_pos];
    let exponent_str = &s[e_pos + 1..];

    let frac_digits = mantissa.split_once('.').map_or(0, |(_, frac)| {
        if trim_trailing_zeros {
            frac.trim_end_matches('0').len()
        } else {
            frac.len()
        }
    });

    let exponent: i64 = if let Ok(v) = exponent_str.parse::<i64>() {
        v
    } else {
        let (digits, is_negative) = exponent_str
            .strip_prefix('-')
            .map(|rest| (rest, true))
            .or_else(|| exponent_str.strip_prefix('+').map(|rest| (rest, false)))
            .unwrap_or((exponent_str, false));

        if digits.is_empty() {
            assert!(
                !strict,
                "Invalid scientific notation format: missing exponent value"
            );
            return None;
        }

        if digits.chars().all(|c| c.is_ascii_digit()) {
            return Some(if is_negative { u8::MAX } else { 0 });
        }

        assert!(
            !strict,
            "Invalid scientific notation exponent '{exponent_str}': must be a valid number"
        );
        return None;
    };

    let precision = i64::try_from(frac_digits)
        .unwrap_or(i64::MAX)
        .saturating_sub(exponent)
        .clamp(0, i64::from(u8::MAX));

    #[expect(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        reason = "clamped to 0..=u8::MAX above"
    )]
    let precision = precision as u8;

    Some(precision)
}

/// Returns the decimal precision inferred from the given string.
///
/// For scientific notation (e.g., "1e-300", "1.5e-2"), the precision accounts
/// for both the mantissa's fractional digits and the signed exponent:
/// `max(0, fractional_digits - exponent)`, clamped to `u8::MAX` (255).
///
/// # Panics
///
/// Panics if the input string is malformed (e.g., "1e-" with no exponent value, or non-numeric
/// exponents like "1e-abc").
#[must_use]
pub fn precision_from_str(s: &str) -> u8 {
    let s = s.trim().to_ascii_lowercase();

    if s.contains('e') {
        return precision_from_scientific(&s, false, true)
            .expect("precision_from_scientific should return Some in strict mode");
    }

    if let Some((_, decimal_part)) = s.split_once('.') {
        clamp_precision_with_log(decimal_part.len(), "Decimal", &s)
    } else {
        0
    }
}

/// Returns the minimum increment precision inferred from the given string,
/// ignoring trailing zeros.
///
/// For scientific notation (e.g., "1e-300", "1.5e-2"), trailing zeros in the
/// mantissa are stripped before computing precision, matching the behavior of
/// [`precision_from_str`].
#[must_use]
pub fn min_increment_precision_from_str(s: &str) -> u8 {
    let s = s.trim().to_ascii_lowercase();

    if s.contains('e') {
        return precision_from_scientific(&s, true, false).unwrap_or(0);
    }

    if let Some(dot_pos) = s.find('.') {
        let decimal_part = &s[dot_pos + 1..];
        if decimal_part.chars().any(|c| c != '0') {
            let trimmed_len = decimal_part.trim_end_matches('0').len();
            return clamp_precision_with_log(trimmed_len, "Minimum increment", &s);
        }
        clamp_precision_with_log(decimal_part.len(), "Decimal", &s)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("", 0)]
    #[case("0", 0)]
    #[case("1.0", 1)]
    #[case("1.00", 2)]
    #[case("1.23456789", 8)]
    #[case("123456.789101112", 9)]
    #[case("0.000000001", 9)]
    #[case("1e-1", 1)]
    #[case("1e-2", 2)]
    #[case("1e-3", 3)]
    #[case("1e8", 0)]
    #[case("-1.23", 2)]
    #[case("-1e-2", 2)]
    #[case("1E-2", 2)]
    #[case("1.5e-2", 3)]
    #[case("1.23e-2", 4)]
    #[case("1.5e2", 0)]
    #[case("1.5E2", 0)]
    #[case("1.5e+2", 0)]
    #[case("1.5e0", 1)]
    #[case("1.5e-1", 2)]
    #[case("  1.23", 2)]
    #[case("1.23  ", 2)]
    fn test_precision_from_str(#[case] s: &str, #[case] expected: u8) {
        let result = precision_from_str(s);
        assert_eq!(result, expected);
    }

    #[rstest]
    #[case("", 0)]
    #[case("0", 0)]
    #[case("1.0", 1)]
    #[case("1.00", 2)]
    #[case("1.23456789", 8)]
    #[case("123456.789101112", 9)]
    #[case("0.000000001", 9)]
    #[case("1e-1", 1)]
    #[case("1e-2", 2)]
    #[case("1e-3", 3)]
    #[case("1e8", 0)]
    #[case("-1.23", 2)]
    #[case("-1e-2", 2)]
    #[case("1E-2", 2)]
    #[case("1.5e-2", 3)]
    #[case("1.23e-2", 4)]
    #[case("1.5e2", 0)]
    #[case("1.50e-2", 3)]
    #[case("1.0e-2", 2)]
    #[case("  1.23", 2)]
    #[case("1.23  ", 2)]
    #[case("1.010", 2)]
    #[case("1.00100", 3)]
    #[case("0.0001000", 4)]
    #[case("1.000000000", 9)]
    fn test_min_increment_precision_from_str(#[case] s: &str, #[case] expected: u8) {
        let result = min_increment_precision_from_str(s);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_precision_from_str_large_exponent_clamped() {
        // u8::MAX is 255, so 999 should be clamped to 255
        let result = precision_from_str("1e-999");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_precision_from_str_very_large_exponent_clamped() {
        // Very large exponents should also be clamped to u8::MAX
        let result = precision_from_str("1e-300");
        assert_eq!(result, 255);

        let result = precision_from_str("1e-1000000");
        assert_eq!(result, 255);
    }

    #[rstest]
    #[should_panic(expected = "Invalid scientific notation exponent")]
    fn test_precision_from_str_invalid_exponent_not_numeric() {
        let _ = precision_from_str("1e-abc");
    }

    #[rstest]
    #[should_panic(expected = "missing exponent value")]
    fn test_precision_from_str_malformed_scientific_notation() {
        // "1e-" with empty exponent should panic (fail fast on malformed input)
        let _ = precision_from_str("1e-");
    }

    #[rstest]
    fn test_precision_from_str_edge_case_max_u8() {
        // u8::MAX = 255, should work
        let result = precision_from_str("1e-255");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_precision_from_str_just_above_max_u8() {
        // 256 should be clamped to 255
        let result = precision_from_str("1e-256");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_precision_from_str_u32_overflow() {
        // Exponent > u32::MAX (4294967296) should be clamped to 255
        let result = precision_from_str("1e-4294967296");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_precision_from_str_u64_overflow() {
        // Exponent > u64::MAX should be clamped to 255
        let result = precision_from_str("1e-99999999999999999999");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_min_increment_precision_from_str_large_exponent() {
        // Large exponents should be clamped to u8::MAX (255), not return 0
        let result = min_increment_precision_from_str("1e-300");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_min_increment_precision_from_str_very_large_exponent() {
        // Very large exponents should also be clamped to 255
        let result = min_increment_precision_from_str("1e-99999999999999999999");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_min_increment_precision_from_str_consistency() {
        // Should match precision_from_str for large exponents
        let input = "1e-1000";
        let precision = precision_from_str(input);
        let min_precision = min_increment_precision_from_str(input);
        assert_eq!(precision, min_precision);
        assert_eq!(precision, 255);
    }

    #[rstest]
    fn test_precision_from_str_i64_min_exponent_clamped() {
        // Exponent equal to i64::MIN must saturate, not overflow the subtraction
        let result = precision_from_str("1e-9223372036854775808");
        assert_eq!(result, 255);
    }

    #[rstest]
    fn test_min_increment_precision_from_str_empty_exponent() {
        // Empty exponent should return 0, not u8::MAX
        let result = min_increment_precision_from_str("1e-");
        assert_eq!(result, 0);
    }
}
