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

//! CRC32 checksum helpers shared by the Kraken Spot `book` and `level3` channels.
//!
//! Both channels hash the same shape of string: for each level, the price then the quantity, each
//! with its decimal point removed and leading zeros stripped, over the top ten asks ascending
//! followed by the top ten bids descending, with the IEEE CRC32.

use rust_decimal::Decimal;

/// Formats a decimal string per Kraken's checksum rules.
///
/// Removes the decimal point then strips leading zeros, so `"0.12730000"` becomes `"12730000"`
/// and `"79754.0"` becomes `"797540"`.
pub(crate) fn format_raw(raw: &str) -> String {
    let no_dot = raw.replace('.', "");
    let trimmed = no_dot.trim_start_matches('0');
    if trimmed.is_empty() {
        "0".to_string()
    } else {
        trimmed.to_string()
    }
}

/// Appends `value` at `scale` decimals to `out` per [`format_raw`], reusing `scratch`.
///
/// The venue hashes its wire representation, so a value held at another precision is rendered at
/// the wire scale first: a price held at six decimals on a pair quoted at seven gains its trailing
/// zero here. The checksum string covers up to forty values per message, so the rendering goes
/// through one reused buffer rather than a fresh allocation per value.
pub(crate) fn push_scaled(out: &mut String, scratch: &mut String, value: Decimal, scale: u8) {
    use std::fmt::Write;

    scratch.clear();
    // Writing a Decimal into a String cannot fail.
    let _ = write!(scratch, "{value:.prec$}", prec = usize::from(scale));

    let start = out.len();
    let mut leading = true;
    for c in scratch.chars() {
        if c == '.' || (leading && c == '0') {
            continue;
        }
        leading = false;
        out.push(c);
    }

    if out.len() == start {
        out.push('0');
    }
}

/// IEEE CRC32 polynomial (reflected), inline to avoid adding a new dependency.
pub(crate) fn crc32_ieee(data: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ 0xEDB8_8320;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    /// Renders `value` at exactly `scale` decimals through [`push_scaled`] into a fresh string.
    fn format_scaled(value: Decimal, scale: u8) -> String {
        let mut out = String::new();
        let mut scratch = String::new();
        push_scaled(&mut out, &mut scratch, value, scale);
        out
    }

    #[rstest]
    fn test_crc32_ieee_known_value() {
        assert_eq!(crc32_ieee(b"123456789"), 0xCBF4_3926);
    }

    #[rstest]
    #[case("42000", "42000")]
    #[case("79754.0", "797540")]
    #[case("0.12730000", "12730000")]
    #[case("0.0", "0")]
    fn test_format_raw(#[case] raw: &str, #[case] expected: &str) {
        assert_eq!(format_raw(raw), expected);
    }

    /// The wire scale decides the digits, not the value's own precision.
    #[rstest]
    #[case(dec!(45285.2), 1, "452852")]
    #[case(dec!(0.001), 8, "100000")]
    #[case(dec!(0.000123), 7, "1230")]
    #[case(dec!(0.000123), 6, "123")]
    fn test_format_scaled(#[case] value: Decimal, #[case] scale: u8, #[case] expected: &str) {
        assert_eq!(format_scaled(value, scale), expected);
    }
}
