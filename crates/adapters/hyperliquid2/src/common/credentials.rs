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

//! Hyperliquid authentication credentials and signing utilities.

use std::fmt;

use anyhow::Result;
use k256::{ecdsa::{SigningKey, signature::Signer, Signature}};
use sha2::{Digest, Sha256};

use super::consts::{
    HYPERLIQUID_PRIVATE_KEY_ENV_KEY, HYPERLIQUID_API_WALLET_ENV_KEY,
    HYPERLIQUID_TESTNET_PRIVATE_KEY_ENV_KEY, HYPERLIQUID_TESTNET_WALLET_ENV_KEY,
};

/// Hyperliquid credentials for wallet-based authentication.
#[derive(Debug, Clone)]
pub struct HyperliquidCredentials {
    pub private_key: String,
    pub wallet_address: Option<String>,
    pub testnet: bool,
}

impl HyperliquidCredentials {
    /// Create new credentials.
    pub fn new(
        private_key: String,
        wallet_address: Option<String>,
        testnet: bool,
    ) -> Self {
        Self {
            private_key,
            wallet_address,
            testnet,
        }
    }

    /// Create credentials from environment variables.
    pub fn from_env(testnet: bool) -> Result<Self> {
        let (key_env, wallet_env) = if testnet {
            (HYPERLIQUID_TESTNET_PRIVATE_KEY_ENV_KEY, HYPERLIQUID_TESTNET_WALLET_ENV_KEY)
        } else {
            (HYPERLIQUID_PRIVATE_KEY_ENV_KEY, HYPERLIQUID_API_WALLET_ENV_KEY)
        };

        let private_key = std::env::var(key_env)
            .map_err(|_| anyhow::anyhow!("Environment variable {} not set", key_env))?;
        
        let wallet_address = std::env::var(wallet_env).ok();

        Ok(Self::new(private_key, wallet_address, testnet))
    }

    /// Sign a message with the private key.
    pub fn sign_message(&self, message: &str) -> Result<String> {
        // Remove 0x prefix if present
        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        
        // Parse private key
        let key_bytes = hex::decode(key_hex)?;
        if key_bytes.len() != 32 {
            anyhow::bail!("Private key must be exactly 32 bytes");
        }
        let key_array: [u8; 32] = key_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Invalid private key length"))?;
        let signing_key = SigningKey::from_bytes(&key_array.into())?;
        
        // Hash the message
        let message_bytes = message.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(message_bytes);
        let message_hash = hasher.finalize();

        // Sign the hash
        let signature: Signature = signing_key.sign(&message_hash);
        
        // Return signature as hex string
        Ok(hex::encode(signature.to_bytes()))
    }

    /// Get the wallet address, deriving it from private key if not set.
    pub fn get_wallet_address(&self) -> Result<String> {
        if let Some(addr) = &self.wallet_address {
            Ok(addr.clone())
        } else {
            // Derive address from private key
            self.derive_wallet_address()
        }
    }

    /// Derive wallet address from private key.
    fn derive_wallet_address(&self) -> Result<String> {
        // Remove 0x prefix if present
        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        
        // Parse private key
        let key_bytes = hex::decode(key_hex)?;
        if key_bytes.len() != 32 {
            anyhow::bail!("Private key must be exactly 32 bytes");
        }
        let key_array: [u8; 32] = key_bytes.try_into()
            .map_err(|_| anyhow::anyhow!("Invalid private key length"))?;
        let signing_key = SigningKey::from_bytes(&key_array.into())?;
        
        // Get public key
        let public_key = signing_key.verifying_key();
        let public_key_bytes = public_key.to_encoded_point(false);
        let public_key_uncompressed = public_key_bytes.as_bytes();
        
        // Skip the first byte (0x04) and take the rest (64 bytes)
        let public_key_64 = &public_key_uncompressed[1..];
        
        // Hash the public key with Keccak-256
        use sha3::{Digest, Keccak256};
        let mut hasher = Keccak256::new();
        hasher.update(public_key_64);
        let hash = hasher.finalize();
        
        // Take the last 20 bytes and format as hex address
        let address_bytes = &hash[12..];
        let address = format!("0x{}", hex::encode(address_bytes));
        
        Ok(address)
    }

    /// Validate the credentials.
    pub fn validate(&self) -> Result<()> {
        // Check private key format
        let key_hex = self.private_key.strip_prefix("0x").unwrap_or(&self.private_key);
        if key_hex.len() != 64 {
            anyhow::bail!("Invalid private key length");
        }
        
        // Check if it's valid hex
        hex::decode(key_hex)?;
        
        // If wallet address is provided, validate it
        if let Some(addr) = &self.wallet_address {
            if !addr.starts_with("0x") || addr.len() != 42 {
                anyhow::bail!("Invalid wallet address format");
            }
        }
        
        Ok(())
    }
}

impl fmt::Display for HyperliquidCredentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "HyperliquidCredentials(wallet: {}, testnet: {})",
            self.wallet_address.as_deref().unwrap_or("derived"),
            self.testnet
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const TEST_PRIVATE_KEY: &str = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    const TEST_WALLET: &str = "0x1234567890123456789012345678901234567890";

    #[test]
    fn test_credentials_creation() {
        let creds = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            true,
        );

        assert_eq!(creds.private_key, TEST_PRIVATE_KEY);
        assert_eq!(creds.wallet_address, Some(TEST_WALLET.to_string()));
        assert!(creds.testnet);
    }

    #[test]
    fn test_credentials_validation() {
        // Valid credentials
        let creds = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            false,
        );
        assert!(creds.validate().is_ok());

        // Invalid private key length
        let invalid_creds = HyperliquidCredentials::new(
            "short".to_string(),
            None,
            false,
        );
        assert!(invalid_creds.validate().is_err());

        // Invalid wallet address
        let invalid_wallet_creds = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some("invalid_address".to_string()),
            false,
        );
        assert!(invalid_wallet_creds.validate().is_err());
    }

    #[test]
    fn test_sign_message() {
        let creds = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            true,
        );

        let message = "test message";
        let signature = creds.sign_message(message);
        
        assert!(signature.is_ok());
        let sig = signature.unwrap();
        assert_eq!(sig.len(), 128); // 64 bytes as hex string
        assert!(hex::decode(sig).is_ok());
    }

    #[test]
    fn test_private_key_with_0x_prefix() {
        let key_with_prefix = format!("0x{}", TEST_PRIVATE_KEY);
        let creds = HyperliquidCredentials::new(
            key_with_prefix,
            None,
            false,
        );

        assert!(creds.validate().is_ok());
        assert!(creds.sign_message("test").is_ok());
    }

    #[test]
    fn test_get_wallet_address() {
        // With explicit wallet address
        let creds_with_addr = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            false,
        );
        assert_eq!(creds_with_addr.get_wallet_address().unwrap(), TEST_WALLET);

        // Without explicit wallet address (should derive)
        let creds_without_addr = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            None,
            false,
        );
        let derived_addr = creds_without_addr.get_wallet_address();
        assert!(derived_addr.is_ok());
        let addr = derived_addr.unwrap();
        assert!(addr.starts_with("0x"));
        assert_eq!(addr.len(), 42);
    }

    #[test]
    fn test_display_format() {
        let creds = HyperliquidCredentials::new(
            TEST_PRIVATE_KEY.to_string(),
            Some(TEST_WALLET.to_string()),
            true,
        );

        let display_str = format!("{}", creds);
        assert!(display_str.contains("wallet: 0x1234567890123456789012345678901234567890"));
        assert!(display_str.contains("testnet: true"));
    }
}