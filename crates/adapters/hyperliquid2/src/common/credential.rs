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

//! Hyperliquid credential and authentication implementation.

use ethers::{
    core::types::{Signature, H160},
    signers::{LocalWallet, Signer},
    utils::keccak256,
};
use serde_json::Value;
use std::str::FromStr;

/// Hyperliquid credentials for authentication
#[derive(Clone)]
pub struct HyperliquidCredentials {
    wallet: LocalWallet,
    address: H160,
}

impl HyperliquidCredentials {
    /// Creates a new [`HyperliquidCredentials`] instance from a private key
    ///
    /// # Parameters
    /// - `private_key`: Ethereum private key (hex string with or without 0x prefix)
    pub fn new(private_key: &str) -> anyhow::Result<Self> {
        let private_key = private_key.trim_start_matches("0x");
        let wallet = LocalWallet::from_str(private_key)?;
        let address = wallet.address();

        Ok(Self { wallet, address })
    }

    /// Returns the wallet address
    pub fn address(&self) -> H160 {
        self.address
    }

    /// Returns the wallet address as a hex string (with 0x prefix)
    pub fn address_str(&self) -> String {
        format!("{:?}", self.address)
    }

    /// Signs a message with the wallet's private key
    ///
    /// # Parameters
    /// - `message`: Message bytes to sign
    pub async fn sign_message(&self, message: &[u8]) -> anyhow::Result<Signature> {
        let signature = self.wallet.sign_message(message).await?;
        Ok(signature)
    }

    /// Signs a typed data payload (EIP-712)
    ///
    /// # Parameters
    /// - `action`: Action payload to sign (JSON object)
    /// - `nonce`: Nonce value for replay protection
    pub async fn sign_l1_action(
        &self,
        action: &Value,
        nonce: u64,
    ) -> anyhow::Result<(String, u64)> {
        // Construct the payload to sign
        let payload = serde_json::json!({
            "action": action,
            "nonce": nonce,
            "vaultAddress": null,
        });

        // Convert to canonical JSON string
        let payload_str = serde_json::to_string(&payload)?;
        let payload_bytes = payload_str.as_bytes();

        // Hash the payload
        let hash = keccak256(payload_bytes);

        // Sign the hash
        let signature = self.wallet.sign_message(&hash).await?;

        // Format signature as hex string
        // Convert U256 to bytes manually
        let mut r_bytes = [0u8; 32];
        signature.r.to_big_endian(&mut r_bytes);
        let mut s_bytes = [0u8; 32];
        signature.s.to_big_endian(&mut s_bytes);

        let sig_str = format!(
            "0x{}{}{}",
            hex::encode(r_bytes),
            hex::encode(s_bytes),
            hex::encode([signature.v as u8])
        );

        Ok((sig_str, nonce))
    }

    /// Signs an agent action (for sub-accounts)
    pub async fn sign_agent_action(
        &self,
        action: &Value,
        nonce: u64,
        vault_address: Option<&str>,
    ) -> anyhow::Result<(String, u64)> {
        let payload = serde_json::json!({
            "action": action,
            "nonce": nonce,
            "vaultAddress": vault_address,
        });

        let payload_str = serde_json::to_string(&payload)?;
        let payload_bytes = payload_str.as_bytes();
        let hash = keccak256(payload_bytes);

        let signature = self.wallet.sign_message(&hash).await?;

        let mut r_bytes = [0u8; 32];
        signature.r.to_big_endian(&mut r_bytes);
        let mut s_bytes = [0u8; 32];
        signature.s.to_big_endian(&mut s_bytes);

        let sig_str = format!(
            "0x{}{}{}",
            hex::encode(r_bytes),
            hex::encode(s_bytes),
            hex::encode([signature.v as u8])
        );

        Ok((sig_str, nonce))
    }

    /// Generates the next nonce based on current timestamp
    pub fn generate_nonce() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64
    }
}

// Implement Debug without exposing the private key
impl std::fmt::Debug for HyperliquidCredentials {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HyperliquidCredentials")
            .field("address", &self.address_str())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_credentials_creation() {
        // Test private key (DO NOT use in production)
        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let credentials = HyperliquidCredentials::new(private_key).unwrap();

        // Verify address is generated
        let address = credentials.address_str();
        assert!(address.starts_with("0x"));
        assert_eq!(address.len(), 42); // 0x + 40 hex chars
    }

    #[test]
    fn test_nonce_generation() {
        let nonce1 = HyperliquidCredentials::generate_nonce();
        std::thread::sleep(std::time::Duration::from_millis(10));
        let nonce2 = HyperliquidCredentials::generate_nonce();

        assert!(nonce2 > nonce1);
    }

    #[tokio::test]
    async fn test_sign_message() {
        let private_key = "0x1234567890123456789012345678901234567890123456789012345678901234";
        let credentials = HyperliquidCredentials::new(private_key).unwrap();

        let message = b"test message";
        let signature = credentials.sign_message(message).await.unwrap();

        // Verify signature has expected structure
        assert!(signature.r != ethers::core::types::U256::zero());
        assert!(signature.s != ethers::core::types::U256::zero());
    }
}
