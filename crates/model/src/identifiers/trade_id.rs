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

//! Represents a valid trade match ID (assigned by a trading venue).

use std::{
    ffi::CStr,
    fmt::{Debug, Display, Formatter},
    hash::Hash,
};

use nautilus_core::correctness::{
    FAILED, check_predicate_false, check_predicate_true, check_slice_not_empty,
};
use serde::{Deserialize, Deserializer, Serialize};

/// The maximum length of ASCII characters for a `TradeId` string value (including null terminator).
pub const TRADE_ID_LEN: usize = 37;

/// Represents a valid trade match ID (assigned by a trading venue).
///
/// The unique ID assigned to the trade entity once it is received or matched by
/// the venue or central counterparty.
///
/// Can correspond to the `TradeID <1003> field` of the FIX protocol.
///
/// Maximum length is 36 characters.
#[repr(C)]
#[derive(Clone, Copy, Hash, PartialEq, Eq, PartialOrd, Ord)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct TradeId {
    /// The trade match ID value as a fixed-length C string byte array (includes null terminator).
    pub(crate) value: [u8; TRADE_ID_LEN],
}

impl TradeId {
    /// Creates a new [`TradeId`] instance with correctness checking.
    ///
    /// Maximum length is 36 characters.
    ///
    /// # Errors
    ///
    /// This function returns an error:
    /// - If `value` is an invalid string (e.g., is empty or contains non-ASCII characters).
    /// - If `value` length exceeds 36 characters.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        Self::from_bytes(value.as_ref().as_bytes())
    }

    /// Creates a new [`TradeId`] instance.
    ///
    /// Maximum length is 36 characters.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `value` is an invalid string (e.g., is empty or contains non-ASCII characters).
    /// - If `value` length exceeds 36 characters.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect(FAILED)
    }

    /// Creates a new [`TradeId`] instance.
    ///
    /// Maximum length is 36 characters plus a null terminator byte.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If `value` is empty or consists only of a single null byte.
    /// - If `value` exceeds 36 bytes and does not end with a null byte.
    /// - If `value` is exactly 37 bytes but the last byte is not null.
    /// - If `value` contains non-ASCII characters.
    pub fn from_bytes(value: &[u8]) -> anyhow::Result<Self> {
        check_slice_not_empty(value, "value")?;

        // Check for non-ASCII characters and capture last byte in single pass
        let mut last_byte = 0;
        let all_ascii = value
            .iter()
            .inspect(|&&b| last_byte = b)
            .all(|&b| b.is_ascii());

        check_predicate_true(all_ascii, "'value' contains non-ASCII characters")?;
        check_predicate_false(
            value.len() == 1 && last_byte == 0,
            "'value' was single null byte",
        )?;
        check_predicate_true(
            value.len() <= 36 || (value.len() == 37 && last_byte == 0),
            "'value' exceeds max length or invalid format",
        )?;

        let mut buf = [0; TRADE_ID_LEN];
        buf[..value.len()].copy_from_slice(value);

        Ok(Self { value: buf })
    }

    /// Returns a C string slice from the trade ID value.
    #[must_use]
    pub fn as_cstr(&self) -> &CStr {
        // SAFETY: Unwrap safe as we always store valid C strings
        // We use until nul because the values array may be padded with nul bytes
        CStr::from_bytes_until_nul(&self.value).unwrap()
    }
}

impl Debug for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}('{}')", stringify!(TradeId), self)
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_cstr().to_str().unwrap())
    }
}

impl Serialize for TradeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for TradeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value_str = String::deserialize(deserializer)?;
        Ok(Self::new(&value_str))
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{stubs::*, trade_id::TradeId};

    #[rstest]
    fn test_trade_id_new_valid() {
        let trade_id = TradeId::new("TRADE12345");
        assert_eq!(trade_id.to_string(), "TRADE12345");
    }

    #[rstest]
    #[should_panic(expected = "Condition failed: 'value' exceeds max length or invalid format")]
    fn test_trade_id_new_invalid_length() {
        let _ = TradeId::new("A".repeat(37).as_str());
    }

    #[rstest]
    #[case(b"1234567890", "1234567890")]
    #[case(
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"
    )]
    #[case(b"1234567890\0", "1234567890")]
    #[case(
        b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456\0",
        "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456"
    )]
    fn test_trade_id_from_valid_bytes(#[case] input: &[u8], #[case] expected: &str) {
        let trade_id = TradeId::from_bytes(input).unwrap();
        assert_eq!(trade_id.to_string(), expected);
    }

    #[rstest]
    #[should_panic(expected = "the 'value' slice `&[u8]` was empty")]
    fn test_trade_id_from_bytes_empty() {
        TradeId::from_bytes(&[] as &[u8]).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "'value' was single null byte")]
    fn test_trade_id_single_null_byte() {
        TradeId::from_bytes(&[0u8] as &[u8]).unwrap();
    }

    #[rstest]
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789012")] // 37 bytes, no null terminator
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456789012\0")] // 38 bytes, with null terminator
    #[should_panic(expected = "'value' exceeds max length or invalid format")]
    fn test_trade_id_exceeds_max_length(#[case] input: &[u8]) {
        TradeId::from_bytes(input).unwrap();
    }

    #[rstest]
    fn test_trade_id_with_null_terminator_at_max_length() {
        let input = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ123456\0" as &[u8];
        let trade_id = TradeId::from_bytes(input).unwrap();
        assert_eq!(trade_id.to_string(), "ABCDEFGHIJKLMNOPQRSTUVWXYZ123456");
    }

    #[rstest]
    fn test_trade_id_as_cstr() {
        let trade_id = TradeId::new("TRADE12345");
        assert_eq!(trade_id.as_cstr().to_str().unwrap(), "TRADE12345");
    }

    #[rstest]
    fn test_trade_id_equality() {
        let trade_id1 = TradeId::new("TRADE12345");
        let trade_id2 = TradeId::new("TRADE12345");
        assert_eq!(trade_id1, trade_id2);
    }

    #[rstest]
    fn test_string_reprs(trade_id: TradeId) {
        assert_eq!(trade_id.to_string(), "1234567890");
        assert_eq!(format!("{trade_id}"), "1234567890");
        assert_eq!(format!("{trade_id:?}"), "TradeId('1234567890')");
    }

    #[rstest]
    fn test_trade_id_ordering() {
        let trade_id1 = TradeId::new("TRADE12345");
        let trade_id2 = TradeId::new("TRADE12346");
        assert!(trade_id1 < trade_id2);
    }

    #[rstest]
    fn test_trade_id_serialization() {
        let trade_id = TradeId::new("TRADE12345");
        let json = serde_json::to_string(&trade_id).unwrap();
        assert_eq!(json, "\"TRADE12345\"");

        let deserialized: TradeId = serde_json::from_str(&json).unwrap();
        assert_eq!(trade_id, deserialized);
    }
}
