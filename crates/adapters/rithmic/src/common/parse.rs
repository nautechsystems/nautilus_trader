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

//! Parsing helpers for Rithmic messages.
//!
//! This module provides utilities for parsing Rithmic protocol buffer
//! messages into Nautilus domain types.

use crate::error::{Result, RithmicError};

/// Parses a price string from Rithmic to f64.
pub fn parse_price(price_str: &str) -> Result<f64> {
    price_str
        .parse::<f64>()
        .map_err(|e| RithmicError::Parse(format!("Invalid price '{price_str}': {e}")))
}

/// Parses a quantity string from Rithmic to f64.
pub fn parse_quantity(qty_str: &str) -> Result<f64> {
    qty_str
        .parse::<f64>()
        .map_err(|e| RithmicError::Parse(format!("Invalid quantity '{qty_str}': {e}")))
}

/// Parses a Rithmic timestamp to Unix nanoseconds.
///
/// Rithmic timestamps are typically in seconds with fractional part.
pub fn parse_timestamp_nanos(secs: f64) -> u64 {
    (secs * 1_000_000_000.0) as u64
}

/// Parses Unix timestamp in seconds to nanoseconds.
pub fn secs_to_nanos(secs: i64) -> u64 {
    (secs as u64) * 1_000_000_000
}

/// Parses Unix timestamp in milliseconds to nanoseconds.
pub fn millis_to_nanos(millis: i64) -> u64 {
    (millis as u64) * 1_000_000
}

/// Parses a tick size from display string (e.g., "0.25" -> 0.25).
pub fn parse_tick_size(tick_str: &str) -> Result<f64> {
    tick_str
        .parse::<f64>()
        .map_err(|e| RithmicError::Parse(format!("Invalid tick size '{tick_str}': {e}")))
}

/// Calculates price precision from tick size.
///
/// Example: tick_size=0.25 -> precision=2
pub fn tick_size_to_precision(tick_size: f64) -> u8 {
    if tick_size >= 1.0 {
        return 0;
    }

    let tick_str = format!("{tick_size}");
    if let Some(dot_pos) = tick_str.find('.') {
        let decimal_part = &tick_str[dot_pos + 1..];
        // Count significant decimal places
        decimal_part.trim_end_matches('0').len() as u8
    } else {
        0
    }
}

/// Normalizes a symbol by removing exchange prefix if present.
///
/// Example: "CME:ES" -> "ES"
pub fn normalize_symbol(symbol: &str) -> &str {
    if let Some(colon_pos) = symbol.find(':') {
        &symbol[colon_pos + 1..]
    } else {
        symbol
    }
}

/// Extracts exchange from a qualified symbol.
///
/// Example: "CME:ES" -> Some("CME")
pub fn extract_exchange(symbol: &str) -> Option<&str> {
    symbol.find(':').map(|pos| &symbol[..pos])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[rstest::rstest]
    fn test_parse_price() {
        assert_eq!(parse_price("1234.50").unwrap(), 1234.50);
        assert!(parse_price("invalid").is_err());
    }

    #[rstest::rstest]
    fn test_parse_timestamp() {
        let nanos = parse_timestamp_nanos(1_234_567_890.123_456_7);
        // f64 has ~15-16 significant digits, so we check approximate equality
        // The exact value 1234567890123456789 can't be represented precisely in f64
        let expected = 1234567890123456789_u64;
        let diff = (nanos as i64 - expected as i64).unsigned_abs();

        assert!(diff < 100, "Timestamp diff too large: {diff}");
    }

    #[rstest::rstest]
    fn test_tick_size_to_precision() {
        assert_eq!(tick_size_to_precision(0.25), 2);
        assert_eq!(tick_size_to_precision(0.01), 2);
        assert_eq!(tick_size_to_precision(0.0001), 4);
        assert_eq!(tick_size_to_precision(1.0), 0);
    }

    #[rstest::rstest]
    fn test_normalize_symbol() {
        assert_eq!(normalize_symbol("CME:ES"), "ES");
        assert_eq!(normalize_symbol("ES"), "ES");
    }

    #[rstest::rstest]
    fn test_extract_exchange() {
        assert_eq!(extract_exchange("CME:ES"), Some("CME"));
        assert_eq!(extract_exchange("ES"), None);
    }
}
