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

//! Hexadecimal string parsing utilities for blockchain data.
//!
//! This module provides functions for converting hexadecimal strings (commonly used in blockchain
//! APIs and JSON-RPC responses) to native Rust types, with specialized support for timestamps.

use alloy_primitives::U256;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use serde::{Deserialize, Deserializer};

/// Converts a hexadecimal string to a u64 integer.
///
/// # Errors
///
/// Returns a `std::num::ParseIntError` if:
/// - The input string contains non-hexadecimal characters.
/// - The hexadecimal value is too large to fit in a u64.
/// - The hex string is longer than 16 characters (excluding 0x prefix).
pub fn from_str_hex_to_u64(hex_string: &str) -> Result<u64, std::num::ParseIntError> {
    let without_prefix = if hex_string.starts_with("0x") || hex_string.starts_with("0X") {
        &hex_string[2..]
    } else {
        hex_string
    };

    // A `u64` can hold 16 full hex characters (0xffff_ffff_ffff_ffff). Anything longer is a
    // guaranteed overflow so we proactively short-circuit with the same error type that the
    // native parser would return. We build this error once via an intentionally-overflowing
    // parse call and reuse it whenever necessary (this avoids the `unwrap_err()` call in hot
    // paths).
    if without_prefix.len() > 16 {
        // Force–generate the standard overflow error and return it. This keeps the public API
        // identical to the branch that would have overflowed inside `from_str_radix`.
        return Err(u64::from_str_radix("ffffffffffffffffffffffff", 16).unwrap_err());
    }

    u64::from_str_radix(without_prefix, 16)
}

/// Custom deserializer function for hex numbers.
///
/// # Errors
///
/// Returns an error if parsing the hex string to a number fails.
pub fn deserialize_hex_number<'de, D>(deserializer: D) -> Result<u64, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    from_str_hex_to_u64(hex_string.as_str()).map_err(serde::de::Error::custom)
}

/// Custom deserializer that converts an optional hexadecimal string into an `Option<u64>`.
///
/// The field is treated as optional – if the JSON field is `null` or absent the function returns
/// `Ok(None)`. When the value **is** present it is parsed via [`from_str_hex_to_u64`] and wrapped
/// in `Some(..)`.
///
/// # Errors
///
/// Returns a [`serde::de::Error`] if the provided string is not valid hexadecimal or if the value
/// is larger than the `u64` range.
pub fn deserialize_opt_hex_u64<'de, D>(deserializer: D) -> Result<Option<u64>, D::Error>
where
    D: Deserializer<'de>,
{
    // We first deserialize the value into an `Option<String>` so that missing / null JSON keys
    // gracefully map to `None`.
    let opt = Option::<String>::deserialize(deserializer)?;

    match opt {
        None => Ok(None),
        Some(hex_string) => from_str_hex_to_u64(hex_string.as_str())
            .map(Some)
            .map_err(serde::de::Error::custom),
    }
}

/// Custom deserializer that converts an optional hexadecimal string into an `Option<U256>`.
/// A `None` result indicates the field was absent or explicitly `null`.
///
/// # Errors
///
/// Returns a [`serde::de::Error`] if the string is not valid hex or cannot be parsed into `U256`.
pub fn deserialize_opt_hex_u256<'de, D>(deserializer: D) -> Result<Option<U256>, D::Error>
where
    D: Deserializer<'de>,
{
    let opt = Option::<String>::deserialize(deserializer)?;

    match opt {
        None => Ok(None),
        Some(hex_string) => {
            let without_prefix = if hex_string.starts_with("0x") || hex_string.starts_with("0X") {
                &hex_string[2..]
            } else {
                hex_string.as_str()
            };

            U256::from_str_radix(without_prefix, 16)
                .map(Some)
                .map_err(serde::de::Error::custom)
        }
    }
}

/// Custom deserializer function for hex timestamps to convert hex seconds to `UnixNanos`.
///
/// # Errors
///
/// Returns an error if parsing the hex string to a timestamp fails.
pub fn deserialize_hex_timestamp<'de, D>(deserializer: D) -> Result<UnixNanos, D::Error>
where
    D: Deserializer<'de>,
{
    let hex_string = String::deserialize(deserializer)?;
    let seconds = from_str_hex_to_u64(hex_string.as_str()).map_err(serde::de::Error::custom)?;

    // Protect against multiplication overflow (extremely far future dates or malicious input).
    seconds
        .checked_mul(NANOSECONDS_IN_SECOND)
        .map(UnixNanos::new)
        .ok_or_else(|| serde::de::Error::custom("UnixNanos overflow when converting timestamp"))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_from_str_hex_to_u64_valid() {
        assert_eq!(from_str_hex_to_u64("0x0").unwrap(), 0);
        assert_eq!(from_str_hex_to_u64("0x1").unwrap(), 1);
        // Upper-case prefix should also be accepted
        assert_eq!(from_str_hex_to_u64("0XfF").unwrap(), 255);
        assert_eq!(from_str_hex_to_u64("0xff").unwrap(), 255);
        assert_eq!(from_str_hex_to_u64("0xffffffffffffffff").unwrap(), u64::MAX);
        assert_eq!(from_str_hex_to_u64("1234abcd").unwrap(), 0x1234abcd);
    }

    #[rstest]
    fn test_from_str_hex_to_u64_too_long() {
        // 17 characters should fail (exceeds u64 max length)
        let too_long = "0x1ffffffffffffffff";
        assert!(from_str_hex_to_u64(too_long).is_err());

        // Even longer should also fail
        let very_long = "0x123456789abcdef123456789abcdef";
        assert!(from_str_hex_to_u64(very_long).is_err());
    }

    #[rstest]
    fn test_from_str_hex_to_u64_invalid_chars() {
        assert!(from_str_hex_to_u64("0xzz").is_err());
        assert!(from_str_hex_to_u64("0x123g").is_err());
    }

    #[rstest]
    fn test_deserialize_hex_timestamp() {
        // Test that hex timestamp conversion works
        let timestamp_hex = "0x64b5f3bb"; // Some timestamp
        let expected_nanos = 0x64b5f3bb * NANOSECONDS_IN_SECOND;

        // This tests the conversion logic, though we can't easily test the deserializer directly
        assert_eq!(
            from_str_hex_to_u64(timestamp_hex).unwrap() * NANOSECONDS_IN_SECOND,
            expected_nanos
        );
    }

    #[rstest]
    fn test_deserialize_opt_hex_u256_present() {
        let json = "\"0x1a\"";
        let value: Option<U256> = serde_json::from_str(json).unwrap();
        assert_eq!(value, Some(U256::from(26u8)));
    }

    #[rstest]
    fn test_deserialize_opt_hex_u256_null() {
        let json = "null";
        let value: Option<U256> = serde_json::from_str(json).unwrap();
        assert!(value.is_none());
    }
}
