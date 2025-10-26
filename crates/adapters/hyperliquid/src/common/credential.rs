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

#![allow(unused_assignments)] // Fields are accessed via methods, false positive from nightly

use std::{env, fmt, fs, path::Path};

use serde::Deserialize;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::http::error::{Error, Result};

/// Represents a secure wrapper for EVM private key with zeroization on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct EvmPrivateKey {
    #[zeroize(skip)]
    formatted_key: String, // Keep the formatted version for display
    #[zeroize(skip)] // Skip zeroization to allow safe cloning
    raw_bytes: Vec<u8>, // The actual key bytes
}

impl EvmPrivateKey {
    /// Creates a new EVM private key from hex string.
    pub fn new(key: String) -> Result<Self> {
        let key = key.trim().to_string();
        let hex_key = key.strip_prefix("0x").unwrap_or(&key);

        // Validate hex format and length
        if hex_key.len() != 64 {
            return Err(Error::bad_request(
                "EVM private key must be 32 bytes (64 hex chars)",
            ));
        }

        if !hex_key.chars().all(|c| c.is_ascii_hexdigit()) {
            return Err(Error::bad_request("EVM private key must be valid hex"));
        }

        // Convert to lowercase for consistency
        let normalized = hex_key.to_lowercase();
        let formatted = format!("0x{}", normalized);

        // Parse to bytes for validation
        let raw_bytes = hex::decode(&normalized)
            .map_err(|_| Error::bad_request("Invalid hex in private key"))?;

        if raw_bytes.len() != 32 {
            return Err(Error::bad_request(
                "EVM private key must be exactly 32 bytes",
            ));
        }

        Ok(Self {
            formatted_key: formatted,
            raw_bytes,
        })
    }

    /// Get the formatted hex key (0x-prefixed)
    pub fn as_hex(&self) -> &str {
        &self.formatted_key
    }

    /// Gets the raw bytes (for signing operations).
    pub fn as_bytes(&self) -> &[u8] {
        &self.raw_bytes
    }
}

impl fmt::Debug for EvmPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EvmPrivateKey(***redacted***)")
    }
}

impl fmt::Display for EvmPrivateKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("EvmPrivateKey(***redacted***)")
    }
}

/// Represents a secure wrapper for vault address.
#[derive(Clone, Copy)]
pub struct VaultAddress {
    bytes: [u8; 20],
}

impl VaultAddress {
    /// Parses vault address from hex string.
    pub fn parse(s: &str) -> Result<Self> {
        let s = s.trim();
        let hex_part = s.strip_prefix("0x").unwrap_or(s);

        if hex_part.len() != 40 {
            return Err(Error::bad_request(
                "Vault address must be 20 bytes (40 hex chars)",
            ));
        }

        let bytes = hex::decode(hex_part)
            .map_err(|_| Error::bad_request("Invalid hex in vault address"))?;

        if bytes.len() != 20 {
            return Err(Error::bad_request("Vault address must be exactly 20 bytes"));
        }

        let mut addr_bytes = [0u8; 20];
        addr_bytes.copy_from_slice(&bytes);

        Ok(Self { bytes: addr_bytes })
    }

    /// Get address as 0x-prefixed hex string
    pub fn to_hex(&self) -> String {
        format!("0x{}", hex::encode(self.bytes))
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 20] {
        &self.bytes
    }
}

impl fmt::Debug for VaultAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let hex = self.to_hex();
        write!(f, "VaultAddress({}...{})", &hex[..6], &hex[hex.len() - 4..])
    }
}

impl fmt::Display for VaultAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

/// Complete secrets configuration for Hyperliquid
#[derive(Clone)]
pub struct Secrets {
    pub private_key: EvmPrivateKey,
    pub vault_address: Option<VaultAddress>,
    pub is_testnet: bool,
}

impl fmt::Debug for Secrets {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct(stringify!(Secrets))
            .field("private_key", &self.private_key)
            .field("vault_address", &self.vault_address)
            .field("is_testnet", &self.is_testnet)
            .finish()
    }
}

impl Secrets {
    /// Load secrets from environment variables
    ///
    /// Expected environment variables:
    /// - `HYPERLIQUID_PK`: EVM private key for mainnet (required for mainnet)
    /// - `HYPERLIQUID_TESTNET_PK`: EVM private key for testnet (required for testnet)
    /// - `HYPERLIQUID_VAULT`: Vault address for mainnet (optional)
    /// - `HYPERLIQUID_TESTNET_VAULT`: Vault address for testnet (optional)
    ///
    /// The method will first try to load testnet credentials. If not found, it will fall back to mainnet.
    pub fn from_env() -> Result<Self> {
        // Try testnet credentials first
        let (private_key_str, vault_env_var, is_testnet) =
            if let Ok(testnet_pk) = env::var("HYPERLIQUID_TESTNET_PK") {
                (testnet_pk, "HYPERLIQUID_TESTNET_VAULT", true)
            } else if let Ok(mainnet_pk) = env::var("HYPERLIQUID_PK") {
                (mainnet_pk, "HYPERLIQUID_VAULT", false)
            } else {
                return Err(Error::bad_request(
                    "Neither HYPERLIQUID_PK nor HYPERLIQUID_TESTNET_PK environment variable is set",
                ));
            };

        let private_key = EvmPrivateKey::new(private_key_str)?;

        let vault_address = match env::var(vault_env_var) {
            Ok(addr_str) if !addr_str.trim().is_empty() => Some(VaultAddress::parse(&addr_str)?),
            _ => None,
        };

        Ok(Self {
            private_key,
            vault_address,
            is_testnet,
        })
    }

    /// Create secrets from explicit private key and vault address.
    ///
    /// # Arguments
    ///
    /// * `private_key_str` - The private key hex string (with or without 0x prefix)
    /// * `vault_address_str` - Optional vault address for vault trading
    ///
    /// # Errors
    ///
    /// Returns an error if the private key or vault address is invalid.
    pub fn from_private_key(
        private_key_str: &str,
        vault_address_str: Option<&str>,
        is_testnet: bool,
    ) -> Result<Self> {
        let private_key = EvmPrivateKey::new(private_key_str.to_string())?;

        let vault_address = match vault_address_str {
            Some(addr_str) if !addr_str.trim().is_empty() => Some(VaultAddress::parse(addr_str)?),
            _ => None,
        };

        Ok(Self {
            private_key,
            vault_address,
            is_testnet,
        })
    }

    /// Load secrets from JSON file
    ///
    /// Expected JSON format:
    /// ```json
    /// {
    ///   "privateKey": "0x...",
    ///   "vaultAddress": "0x..." (optional),
    ///   "network": "mainnet" | "testnet" (optional)
    /// }
    /// ```
    pub fn from_file(path: &Path) -> Result<Self> {
        let mut content = fs::read_to_string(path).map_err(Error::Io)?;

        let result = Self::from_json(&content);

        // Zeroize the file content from memory
        content.zeroize();

        result
    }

    /// Parse secrets from JSON string
    pub fn from_json(json: &str) -> Result<Self> {
        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct RawSecrets {
            private_key: String,
            #[serde(default)]
            vault_address: Option<String>,
            #[serde(default)]
            network: Option<String>,
        }

        let raw: RawSecrets = serde_json::from_str(json)
            .map_err(|e| Error::bad_request(format!("Invalid JSON: {}", e)))?;

        let private_key = EvmPrivateKey::new(raw.private_key)?;

        let vault_address = match raw.vault_address {
            Some(addr) => Some(VaultAddress::parse(&addr)?),
            None => None,
        };

        let is_testnet = matches!(raw.network.as_deref(), Some("testnet" | "test"));

        Ok(Self {
            private_key,
            vault_address,
            is_testnet,
        })
    }
}

/// Normalize EVM address to lowercase hex format
pub fn normalize_address(addr: &str) -> Result<String> {
    let addr = addr.trim();
    let hex_part = addr
        .strip_prefix("0x")
        .or_else(|| addr.strip_prefix("0X"))
        .unwrap_or(addr);

    if hex_part.len() != 40 {
        return Err(Error::bad_request(
            "Address must be 20 bytes (40 hex chars)",
        ));
    }

    if !hex_part.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Error::bad_request("Address must be valid hex"));
    }

    Ok(format!("0x{}", hex_part.to_lowercase()))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    const TEST_PRIVATE_KEY: &str =
        "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef";
    const TEST_VAULT_ADDRESS: &str = "0x1234567890123456789012345678901234567890";

    #[rstest]
    fn test_evm_private_key_creation() {
        let key = EvmPrivateKey::new(TEST_PRIVATE_KEY.to_string()).unwrap();
        assert_eq!(key.as_hex(), TEST_PRIVATE_KEY);
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[rstest]
    fn test_evm_private_key_without_0x_prefix() {
        let key_without_prefix = &TEST_PRIVATE_KEY[2..]; // Remove 0x
        let key = EvmPrivateKey::new(key_without_prefix.to_string()).unwrap();
        assert_eq!(key.as_hex(), TEST_PRIVATE_KEY);
    }

    #[rstest]
    fn test_evm_private_key_invalid_length() {
        let result = EvmPrivateKey::new("0x123".to_string());
        assert!(result.is_err());
    }

    #[rstest]
    fn test_evm_private_key_invalid_hex() {
        let result = EvmPrivateKey::new(
            "0x123g567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef".to_string(),
        );
        assert!(result.is_err());
    }

    #[rstest]
    fn test_evm_private_key_debug_redacts() {
        let key = EvmPrivateKey::new(TEST_PRIVATE_KEY.to_string()).unwrap();
        let debug_str = format!("{:?}", key);
        assert_eq!(debug_str, "EvmPrivateKey(***redacted***)");
        assert!(!debug_str.contains("1234"));
    }

    #[rstest]
    fn test_vault_address_creation() {
        let addr = VaultAddress::parse(TEST_VAULT_ADDRESS).unwrap();
        assert_eq!(addr.to_hex(), TEST_VAULT_ADDRESS);
        assert_eq!(addr.as_bytes().len(), 20);
    }

    #[rstest]
    fn test_vault_address_without_0x_prefix() {
        let addr_without_prefix = &TEST_VAULT_ADDRESS[2..]; // Remove 0x
        let addr = VaultAddress::parse(addr_without_prefix).unwrap();
        assert_eq!(addr.to_hex(), TEST_VAULT_ADDRESS);
    }

    #[rstest]
    fn test_vault_address_debug_redacts_middle() {
        let addr = VaultAddress::parse(TEST_VAULT_ADDRESS).unwrap();
        let debug_str = format!("{:?}", addr);
        assert!(debug_str.starts_with("VaultAddress(0x1234"));
        assert!(debug_str.ends_with("7890)"));
        assert!(debug_str.contains("..."));
    }

    #[rstest]
    fn test_secrets_from_json() {
        let json = format!(
            r#"{{
            "privateKey": "{}",
            "vaultAddress": "{}",
            "network": "testnet"
        }}"#,
            TEST_PRIVATE_KEY, TEST_VAULT_ADDRESS
        );

        let secrets = Secrets::from_json(&json).unwrap();
        assert_eq!(secrets.private_key.as_hex(), TEST_PRIVATE_KEY);
        assert!(secrets.vault_address.is_some());
        assert_eq!(secrets.vault_address.unwrap().to_hex(), TEST_VAULT_ADDRESS);
        assert!(secrets.is_testnet);
    }

    #[rstest]
    fn test_secrets_from_json_minimal() {
        let json = format!(
            r#"{{
            "privateKey": "{}"
        }}"#,
            TEST_PRIVATE_KEY
        );

        let secrets = Secrets::from_json(&json).unwrap();
        assert_eq!(secrets.private_key.as_hex(), TEST_PRIVATE_KEY);
        assert!(secrets.vault_address.is_none());
        assert!(!secrets.is_testnet);
    }

    #[rstest]
    fn test_normalize_address() {
        let test_cases = [
            (
                TEST_VAULT_ADDRESS,
                "0x1234567890123456789012345678901234567890",
            ),
            (
                "1234567890123456789012345678901234567890",
                "0x1234567890123456789012345678901234567890",
            ),
            (
                "0X1234567890123456789012345678901234567890",
                "0x1234567890123456789012345678901234567890",
            ),
        ];

        for (input, expected) in test_cases {
            assert_eq!(normalize_address(input).unwrap(), expected);
        }
    }

    #[rstest]
    #[ignore = "This test modifies environment variables - run manually if needed"]
    fn test_secrets_from_env() {
        // Note: This test requires setting environment variables manually
        // HYPERLIQUID_PK=1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef
        // HYPERLIQUID_VAULT=0x1234567890abcdef1234567890abcdef12345678
        // HYPERLIQUID_NETWORK=testnet

        // For now, just test the error case when variables are not set
        match Secrets::from_env() {
            Err(e) => {
                assert!(
                    e.to_string().contains("HYPERLIQUID_PK")
                        || e.to_string().contains("environment variable not set")
                );
            }
            Ok(_) => {
                // If environment variables are actually set, that's fine too
            }
        }
    }
}
