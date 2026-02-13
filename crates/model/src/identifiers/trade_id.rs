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

//! Represents a valid trade match ID (assigned by a trading venue).

use std::{
    ffi::CStr,
    fmt::{Debug, Display},
    hash::Hash,
};

use nautilus_core::StackStr;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

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
pub struct TradeId(StackStr);

impl TradeId {
    /// Creates a new [`TradeId`] instance with correctness checking.
    ///
    /// Maximum length is 36 characters.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `value` is an invalid string (e.g., is empty or contains non-ASCII characters).
    /// - `value` length exceeds 36 characters.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type for proper error handling and stacktrace printing in Python.
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        Ok(Self(StackStr::new_checked(value.as_ref())?))
    }

    /// Creates a new [`TradeId`] instance.
    ///
    /// Maximum length is 36 characters.
    ///
    /// # Panics
    ///
    /// This function panics if:
    /// - `value` is an invalid string (e.g., is empty or contains non-ASCII characters).
    /// - `value` length exceeds 36 characters.
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self(StackStr::new(value.as_ref()))
    }

    /// Creates a [`TradeId`] from a byte slice.
    ///
    /// # Errors
    ///
    /// Returns an error if `bytes` is empty, contains non-ASCII characters,
    /// or exceeds 36 bytes (excluding trailing null terminator).
    pub fn from_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        Ok(Self(StackStr::from_bytes(bytes)?))
    }

    /// Returns the inner string value.
    #[inline]
    #[must_use]
    pub fn as_str(&self) -> &str {
        self.0.as_str()
    }

    /// Returns a C string slice from the trade ID value.
    #[inline]
    #[must_use]
    pub fn as_cstr(&self) -> &CStr {
        self.0.as_cstr()
    }
}

impl Debug for TradeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}('{}')", stringify!(TradeId), self)
    }
}

impl Display for TradeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl Serialize for TradeId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.serialize(serializer)
    }
}

impl<'de> Deserialize<'de> for TradeId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let inner = StackStr::deserialize(deserializer)?;
        Ok(Self(inner))
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::identifiers::{TradeId, stubs::*};

    #[rstest]
    fn test_trade_id_new_valid() {
        let trade_id = TradeId::new("TRADE12345");
        assert_eq!(trade_id.to_string(), "TRADE12345");
    }

    #[rstest]
    #[should_panic(expected = "exceeds maximum length")]
    fn test_trade_id_new_invalid_length() {
        let _ = TradeId::new("A".repeat(37).as_str());
    }

    #[rstest]
    #[case(b"1234567890", "1234567890")]
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234", "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234")] // 30 chars
    #[case(b"1234567890\0", "1234567890")]
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234\0", "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234")] // 30 chars with null
    fn test_trade_id_from_valid_bytes(#[case] input: &[u8], #[case] expected: &str) {
        let trade_id = TradeId::from_bytes(input).unwrap();
        assert_eq!(trade_id.to_string(), expected);
    }

    #[rstest]
    #[should_panic(expected = "String is empty")]
    fn test_trade_id_from_bytes_empty() {
        TradeId::from_bytes(&[] as &[u8]).unwrap();
    }

    #[rstest]
    #[should_panic(expected = "String is empty")]
    fn test_trade_id_single_null_byte() {
        TradeId::from_bytes(&[0u8] as &[u8]).unwrap();
    }

    #[rstest]
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ12345678901")] // 37 bytes, no null
    #[case(b"ABCDEFGHIJKLMNOPQRSTUVWXYZ12345678901\0")] // 38 bytes, with null
    #[should_panic(expected = "exceeds maximum length")]
    fn test_trade_id_exceeds_max_length(#[case] input: &[u8]) {
        TradeId::from_bytes(input).unwrap();
    }

    #[rstest]
    fn test_trade_id_with_null_terminator_at_max_length() {
        let input = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890\0" as &[u8];
        let trade_id = TradeId::from_bytes(input).unwrap();
        assert_eq!(trade_id.to_string(), "ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890"); // 36 chars
    }

    #[rstest]
    fn test_trade_id_as_cstr() {
        let trade_id = TradeId::new("TRADE12345");
        assert_eq!(trade_id.as_cstr().to_str().unwrap(), "TRADE12345");
    }

    #[rstest]
    fn test_trade_id_as_str() {
        let trade_id = TradeId::new("TRADE12345");
        assert_eq!(trade_id.as_str(), "TRADE12345");
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
