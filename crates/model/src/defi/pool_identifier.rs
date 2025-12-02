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

use std::{
    fmt::{Debug, Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use alloy_primitives::Address;
use nautilus_core::correctness::FAILED;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use ustr::Ustr;

/// Protocol-aware pool identifier for DeFi liquidity pools.
///
/// This enum distinguishes between two types of pool identifiers:
/// - **Address**: Used by V2/V3 protocols where pool identifier equals pool contract address (42 chars: "0x" + 40 hex)
/// - **PoolId**: Used by V4 protocols where pool identifier is a bytes32 hash (66 chars: "0x" + 64 hex)
///
/// The type implements case-insensitive equality and hashing for address comparison,
/// while preserving the original case for display purposes.
#[derive(Clone, Copy, PartialOrd, Ord)]
pub enum PoolIdentifier {
    /// V2/V3 pool identifier (checksummed Ethereum address)
    Address(Ustr),
    /// V4 pool identifier (32-byte pool ID as hex string)
    PoolId(Ustr),
}

impl PoolIdentifier {
    /// Creates a new [`PoolIdentifier`] instance with correctness checking.
    ///
    /// Automatically detects variant based on string length:
    /// - 42 characters (0x + 40 hex): Address variant
    /// - 66 characters (0x + 64 hex): PoolId variant
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - String doesn't start with "0x"
    /// - Length is neither 42 nor 66 characters
    /// - Contains invalid hex characters
    /// - Address checksum validation fails (for Address variant)
    pub fn new_checked<T: AsRef<str>>(value: T) -> anyhow::Result<Self> {
        let value = value.as_ref();

        if !value.starts_with("0x") {
            anyhow::bail!("Pool identifier must start with '0x', got: {value}");
        }

        match value.len() {
            42 => {
                validate_hex_string(value)?;

                // Parse without strict checksum validation, then normalize to checksummed format
                let addr = value
                    .parse::<Address>()
                    .map_err(|e| anyhow::anyhow!("Invalid address: {e}"))?;

                // Store the checksummed version
                Ok(Self::Address(Ustr::from(addr.to_checksum(None).as_str())))
            }
            66 => {
                // PoolId variant (32 bytes)
                validate_hex_string(value)?;

                // Store lowercase version for consistency
                Ok(Self::PoolId(Ustr::from(&value.to_lowercase())))
            }
            len => {
                anyhow::bail!(
                    "Pool identifier must be 42 chars (address) or 66 chars (pool ID), got {len} chars: {value}"
                )
            }
        }
    }

    /// Creates a new [`PoolIdentifier`] instance.
    ///
    /// # Panics
    ///
    /// Panics if validation fails.
    #[must_use]
    pub fn new<T: AsRef<str>>(value: T) -> Self {
        Self::new_checked(value).expect(FAILED)
    }

    /// Creates an Address variant from an alloy Address.
    ///
    /// Returns the checksummed representation.
    #[must_use]
    pub fn from_address(address: Address) -> Self {
        Self::Address(Ustr::from(address.to_checksum(None).as_str()))
    }

    /// Creates a PoolId variant from raw bytes (32 bytes).
    ///
    /// # Errors
    ///
    /// Returns an error if bytes length is not 32.
    pub fn from_pool_id_bytes(bytes: &[u8]) -> anyhow::Result<Self> {
        anyhow::ensure!(
            bytes.len() == 32,
            "Pool ID must be 32 bytes, got {}",
            bytes.len()
        );

        let hex_string = format!("0x{}", hex::encode(bytes));
        Ok(Self::PoolId(Ustr::from(&hex_string)))
    }

    /// Creates a PoolId variant from a hex string (with or without 0x prefix).
    ///
    /// # Errors
    ///
    /// Returns an error if the string is not valid 64-character hex.
    pub fn from_pool_id_hex<T: AsRef<str>>(hex: T) -> anyhow::Result<Self> {
        let hex = hex.as_ref();
        let hex_str = hex.strip_prefix("0x").unwrap_or(hex);

        anyhow::ensure!(
            hex_str.len() == 64,
            "Pool ID hex must be 64 characters (32 bytes), got {}",
            hex_str.len()
        );

        validate_hex_string(&format!("0x{hex_str}"))?;

        Ok(Self::PoolId(Ustr::from(&format!(
            "0x{}",
            hex_str.to_lowercase()
        ))))
    }

    /// Returns the inner identifier value as a Ustr.
    #[must_use]
    pub fn inner(&self) -> Ustr {
        match self {
            Self::Address(s) | Self::PoolId(s) => *s,
        }
    }

    /// Returns the inner identifier value as a string slice.
    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Address(s) | Self::PoolId(s) => s.as_str(),
        }
    }

    /// Returns true if this is an Address variant (V2/V3 pools).
    #[must_use]
    pub fn is_address(&self) -> bool {
        matches!(self, Self::Address(_))
    }

    /// Returns true if this is a PoolId variant (V4 pools).
    #[must_use]
    pub fn is_pool_id(&self) -> bool {
        matches!(self, Self::PoolId(_))
    }

    /// Converts to native Address type (V2/V3 pools only).
    ///
    /// Returns the underlying Address for use with alloy/ethers operations.
    ///
    /// # Errors
    ///
    /// Returns error if this is a PoolId variant or if parsing fails.
    pub fn to_address(&self) -> anyhow::Result<Address> {
        match self {
            Self::Address(s) => Address::parse_checksummed(s.as_str(), None)
                .map_err(|e| anyhow::anyhow!("Failed to parse address: {e}")),
            Self::PoolId(_) => anyhow::bail!("Cannot convert PoolId variant to Address"),
        }
    }

    /// Converts to native bytes array (V4 pools only).
    ///
    /// Returns the 32-byte pool ID for use in V4-specific operations.
    ///
    /// # Errors
    ///
    /// Returns error if this is an Address variant or if hex decoding fails.
    pub fn to_pool_id_bytes(&self) -> anyhow::Result<[u8; 32]> {
        match self {
            Self::PoolId(s) => {
                let hex = s.as_str().strip_prefix("0x").unwrap_or(s.as_str());
                let bytes = hex::decode(hex)
                    .map_err(|e| anyhow::anyhow!("Failed to decode pool ID hex: {e}",))?;

                bytes
                    .try_into()
                    .map_err(|_| anyhow::anyhow!("Pool ID must be exactly 32 bytes"))
            }
            Self::Address(_) => anyhow::bail!("Cannot convert Address variant to PoolId bytes"),
        }
    }
}

/// Validates that a string contains only valid hexadecimal characters after "0x" prefix.
fn validate_hex_string(s: &str) -> anyhow::Result<()> {
    let hex_part = &s[2..];
    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        anyhow::bail!("Invalid hex characters in: {s}");
    }
    Ok(())
}

impl PartialEq for PoolIdentifier {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Address(a), Self::Address(b)) | (Self::PoolId(a), Self::PoolId(b)) => {
                // Case-insensitive comparison
                a.as_str().eq_ignore_ascii_case(b.as_str())
            }
            // Different variants are never equal
            _ => false,
        }
    }
}

impl Eq for PoolIdentifier {}

impl Hash for PoolIdentifier {
    fn hash<H: Hasher>(&self, state: &mut H) {
        // Hash the variant discriminant first
        std::mem::discriminant(self).hash(state);

        // Then hash the lowercase version of the string
        match self {
            Self::Address(s) | Self::PoolId(s) => {
                for byte in s.as_str().bytes() {
                    state.write_u8(byte.to_ascii_lowercase());
                }
            }
        }
    }
}

impl Display for PoolIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Address(s) | Self::PoolId(s) => write!(f, "{s}"),
        }
    }
}

impl Debug for PoolIdentifier {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Address(s) => write!(f, "Address({s:?})"),
            Self::PoolId(s) => write!(f, "PoolId({s:?})"),
        }
    }
}

impl Serialize for PoolIdentifier {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        // Serialize as plain string (same as current String behavior)
        match self {
            Self::Address(s) | Self::PoolId(s) => s.serialize(serializer),
        }
    }
}

impl<'de> Deserialize<'de> for PoolIdentifier {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value_str: &str = Deserialize::deserialize(deserializer)?;
        Self::new_checked(value_str).map_err(serde::de::Error::custom)
    }
}

impl FromStr for PoolIdentifier {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Self::new_checked(s)
    }
}

impl From<&str> for PoolIdentifier {
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for PoolIdentifier {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl AsRef<str> for PoolIdentifier {
    fn as_ref(&self) -> &str {
        self.as_str()
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
    #[case("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2", true)] // Valid checksummed address
    #[case("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2", true)] // Lowercase address
    #[case(
        "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461",
        true
    )] // V4 Pool ID
    fn test_valid_pool_identifiers(#[case] input: &str, #[case] expected_valid: bool) {
        let result = PoolIdentifier::new_checked(input);
        assert_eq!(result.is_ok(), expected_valid, "Input: {input}");
    }

    #[rstest]
    #[case("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2")] // Missing 0x
    #[case("0xC02aaA39")] // Too short
    #[case("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2EXTRA")] // Too long
    #[case("0xGGGGGGGGb223FE8D0A0e5C4F27eAD9083C756Cc2")] // Invalid hex
    fn test_invalid_pool_identifiers(#[case] input: &str) {
        let result = PoolIdentifier::new_checked(input);
        assert!(result.is_err(), "Input should fail: {input}");
    }

    #[rstest]
    fn test_case_insensitive_equality() {
        let addr1 = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let addr2 = PoolIdentifier::new("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");
        let addr3 = PoolIdentifier::new("0xC02AAA39B223FE8D0A0E5C4F27EAD9083C756CC2");

        assert_eq!(addr1, addr2);
        assert_eq!(addr2, addr3);
        assert_eq!(addr1, addr3);
    }

    #[rstest]
    fn test_case_insensitive_hashing() {
        use std::collections::HashMap;

        let mut map = HashMap::new();
        let addr1 = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let addr2 = PoolIdentifier::new("0xc02aaa39b223fe8d0a0e5c4f27ead9083c756cc2");

        map.insert(addr1, "value1");

        // Should be able to retrieve using different case
        assert_eq!(map.get(&addr2), Some(&"value1"));
    }

    #[rstest]
    fn test_display_preserves_case() {
        let checksummed = "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2";
        let addr = PoolIdentifier::new_checked(checksummed).unwrap();

        // Display should show checksummed version
        assert_eq!(addr.to_string(), checksummed);
    }

    #[rstest]
    fn test_variant_detection() {
        let address = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let pool_id = PoolIdentifier::new(
            "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461",
        );

        assert!(address.is_address());
        assert!(!address.is_pool_id());

        assert!(pool_id.is_pool_id());
        assert!(!pool_id.is_address());
    }

    #[rstest]
    fn test_different_variants_not_equal() {
        let address = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let pool_id = PoolIdentifier::new(
            "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461",
        );

        assert_ne!(address, pool_id);
    }

    #[rstest]
    fn test_serialization_roundtrip() {
        let original = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");

        let json = serde_json::to_string(&original).unwrap();
        let deserialized: PoolIdentifier = serde_json::from_str(&json).unwrap();

        assert_eq!(original, deserialized);
    }

    #[rstest]
    fn test_from_address() {
        let addr = Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let pool_id = PoolIdentifier::from_address(addr);

        assert!(pool_id.is_address());
        assert_eq!(
            pool_id.to_string(),
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
        );
    }

    #[rstest]
    fn test_from_pool_id_bytes() {
        let bytes: [u8; 32] = [
            0xc9, 0xbc, 0x80, 0x43, 0x29, 0x41, 0x46, 0x42, 0x4a, 0x4e, 0x46, 0x07, 0xd8, 0xad,
            0x83, 0x7d, 0x6a, 0x65, 0x91, 0x42, 0x82, 0x2b, 0xba, 0xaa, 0xbc, 0x83, 0xbb, 0x57,
            0xe7, 0x44, 0x74, 0x61,
        ];

        let pool_id = PoolIdentifier::from_pool_id_bytes(&bytes).unwrap();

        assert!(pool_id.is_pool_id());
        assert_eq!(
            pool_id.to_string(),
            "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461"
        );
    }

    #[rstest]
    fn test_to_address() {
        let id = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let address = id.to_address().unwrap();

        assert_eq!(
            address.to_string(),
            "0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2"
        );
    }

    #[rstest]
    fn test_to_address_fails_for_pool_id() {
        let pool_id = PoolIdentifier::new(
            "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461",
        );
        let result = pool_id.to_address();

        assert!(result.is_err());
    }

    #[rstest]
    fn test_to_pool_id_bytes() {
        let pool_id = PoolIdentifier::new(
            "0xc9bc8043294146424a4e4607d8ad837d6a659142822bbaaabc83bb57e7447461",
        );
        let bytes = pool_id.to_pool_id_bytes().unwrap();

        assert_eq!(bytes.len(), 32);
        assert_eq!(bytes[0], 0xc9);
        assert_eq!(bytes[31], 0x61);
    }

    #[rstest]
    fn test_to_pool_id_bytes_fails_for_address() {
        let address = PoolIdentifier::new("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let result = address.to_pool_id_bytes();

        assert!(result.is_err());
    }

    #[rstest]
    fn test_conversion_roundtrip_address() {
        let original_addr =
            Address::from_str("0xC02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2").unwrap();
        let pool_id = PoolIdentifier::from_address(original_addr);
        let converted_addr = pool_id.to_address().unwrap();

        assert_eq!(original_addr, converted_addr);
    }

    #[rstest]
    fn test_conversion_roundtrip_pool_id() {
        let original_bytes: [u8; 32] = [
            0xc9, 0xbc, 0x80, 0x43, 0x29, 0x41, 0x46, 0x42, 0x4a, 0x4e, 0x46, 0x07, 0xd8, 0xad,
            0x83, 0x7d, 0x6a, 0x65, 0x91, 0x42, 0x82, 0x2b, 0xba, 0xaa, 0xbc, 0x83, 0xbb, 0x57,
            0xe7, 0x44, 0x74, 0x61,
        ];

        let pool_id = PoolIdentifier::from_pool_id_bytes(&original_bytes).unwrap();
        let converted_bytes = pool_id.to_pool_id_bytes().unwrap();

        assert_eq!(original_bytes, converted_bytes);
    }
}
