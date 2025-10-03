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

//! Core parsing functions.

/// Returns the decimal precision inferred from the given string.
///
/// For scientific notation with large negative exponents (e.g., "1e-300", "1e-4294967296"),
/// the precision is clamped to `u8::MAX` (255) since that represents the maximum representable
/// precision in this system. This handles arbitrarily large exponents without panicking.
///
/// # Panics
///
/// Panics if the input string is malformed (e.g., "1e-" with no exponent value, or non-numeric
/// exponents like "1e-abc").
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn precision_from_str(s: &str) -> u8 {
    let s = s.trim().to_ascii_lowercase();

    // Check for scientific notation
    if s.contains("e-") {
        // Extract the exponent part after "e-"
        let exponent_str = s
            .split("e-")
            .nth(1)
            .expect("Invalid scientific notation format: missing exponent after 'e-'");

        // Parse the exponent as u64 to handle very large values, then clamp to u8::MAX
        // This handles exponents larger than u32::MAX (e.g., "1e-4294967296") gracefully
        return match exponent_str.parse::<u64>() {
            Ok(exp) => exp.min(u8::MAX as u64) as u8,
            Err(_) => {
                // Empty exponent (e.g., "1e-") should fail fast
                if exponent_str.is_empty() {
                    panic!("Invalid scientific notation format: missing exponent after 'e-'");
                }
                // If it doesn't parse as u64, it's either non-numeric or absurdly large
                // Check if it's numeric but overflows u64 by manual inspection
                if exponent_str.chars().all(|c| c.is_ascii_digit()) {
                    // It's numeric but > u64::MAX, clamp to u8::MAX
                    u8::MAX
                } else {
                    panic!(
                        "Invalid scientific notation exponent '{}': must be a valid number",
                        exponent_str
                    )
                }
            }
        };
    }

    // Check for decimal precision
    if let Some((_, decimal_part)) = s.split_once('.') {
        // Clamp decimal precision to u8::MAX for very long decimal strings
        decimal_part.len().min(u8::MAX as usize) as u8
    } else {
        0
    }
}

/// Returns the minimum increment precision inferred from the given string,
/// ignoring trailing zeros.
#[must_use]
#[allow(clippy::cast_possible_truncation)]
pub fn min_increment_precision_from_str(s: &str) -> u8 {
    let s = s.trim().to_ascii_lowercase();

    // Check for scientific notation
    if let Some(pos) = s.find('e')
        && s[pos + 1..].starts_with('-')
    {
        return s[pos + 2..].parse::<u8>().unwrap_or(0);
    }

    // Check for decimal precision
    if let Some(dot_pos) = s.find('.') {
        let decimal_part = &s[dot_pos + 1..];
        if decimal_part.chars().any(|c| c != '0') {
            let trimmed_len = decimal_part.trim_end_matches('0').len();
            return trimmed_len.min(u8::MAX as usize) as u8;
        }
        return decimal_part.len().min(u8::MAX as usize) as u8;
    }

    0
}

/// Returns a `usize` from the given bytes.
///
/// # Errors
///
/// Returns an error if there are not enough bytes to represent a `usize`.
pub fn bytes_to_usize(bytes: &[u8]) -> anyhow::Result<usize> {
    // Check bytes width
    if bytes.len() >= std::mem::size_of::<usize>() {
        let mut buffer = [0u8; std::mem::size_of::<usize>()];
        buffer.copy_from_slice(&bytes[..std::mem::size_of::<usize>()]);

        Ok(usize::from_le_bytes(buffer))
    } else {
        anyhow::bail!("Not enough bytes to represent a `usize`");
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
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
    fn test_bytes_to_usize_empty() {
        let payload: Vec<u8> = vec![];
        let result = bytes_to_usize(&payload);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Not enough bytes to represent a `usize`"
        );
    }

    #[rstest]
    fn test_bytes_to_usize_invalid() {
        let payload: Vec<u8> = vec![0x01, 0x02, 0x03];
        let result = bytes_to_usize(&payload);
        assert!(result.is_err());
        assert_eq!(
            result.err().unwrap().to_string(),
            "Not enough bytes to represent a `usize`"
        );
    }

    #[rstest]
    fn test_bytes_to_usize_valid() {
        let payload: Vec<u8> = vec![0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08];
        let result = bytes_to_usize(&payload).unwrap();
        assert_eq!(result, 0x0807_0605_0403_0201);
        assert_eq!(result, 578_437_695_752_307_201);
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
    #[should_panic(expected = "missing exponent after 'e-'")]
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
}
