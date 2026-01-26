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

//! dYdX credential storage and wallet-based transaction signing helpers.
//!
//! dYdX v4 uses Cosmos SDK-style wallet signing rather than API key authentication.
//! Trading operations require signing transactions with a secp256k1 private key.
//!
//! # Credential Resolution
//!
//! Credentials are resolved in the following priority order:
//!
//! 1. `private_key` from config
//! 2. `DYDX_PRIVATE_KEY` / `DYDX_TESTNET_PRIVATE_KEY` env var
//!
//! Wallet address env vars: `DYDX_WALLET_ADDRESS` / `DYDX_TESTNET_WALLET_ADDRESS`

#![allow(unused_assignments)] // Fields are accessed externally, false positive from nightly

use std::fmt::Debug;

use anyhow::Context;
use cosmrs::{AccountId, crypto::secp256k1::SigningKey, tx::SignDoc};
use nautilus_core::env::get_or_env_var_opt;

use crate::common::consts::DYDX_BECH32_PREFIX;

/// dYdX wallet credentials for signing blockchain transactions.
///
/// Uses secp256k1 for signing as per Cosmos SDK specifications.
///
/// # Security
///
/// The underlying `SigningKey` from cosmrs (backed by k256) securely zeroizes
/// private key material from memory on drop.
pub struct DydxCredential {
    /// The secp256k1 signing key.
    signing_key: SigningKey,
    /// Bech32-encoded account address (e.g., dydx1...).
    pub address: String,
    /// Optional authenticator IDs for permissioned key trading.
    pub authenticator_ids: Vec<u64>,
}

impl Debug for DydxCredential {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DydxCredential))
            .field("address", &self.address)
            .field("authenticator_ids", &self.authenticator_ids)
            .field("signing_key", &"<redacted>")
            .finish()
    }
}

impl DydxCredential {
    /// Creates a new [`DydxCredential`] from a raw private key.
    ///
    /// # Errors
    ///
    /// Returns an error if private key is invalid.
    pub fn from_private_key(
        private_key_hex: &str,
        authenticator_ids: Vec<u64>,
    ) -> anyhow::Result<Self> {
        // Decode hex private key
        let key_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
            .context("Invalid hex private key")?;

        let signing_key = SigningKey::from_slice(&key_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid secp256k1 private key: {e}"))?;

        // Derive bech32 address
        let public_key = signing_key.public_key();
        let account_id = public_key
            .account_id(DYDX_BECH32_PREFIX)
            .map_err(|e| anyhow::anyhow!("Failed to derive account ID: {e}"))?;
        let address = account_id.to_string();

        Ok(Self {
            signing_key,
            address,
            authenticator_ids,
        })
    }

    /// Creates a [`DydxCredential`] from environment variables.
    ///
    /// Checks for private key: `DYDX_PRIVATE_KEY` / `DYDX_TESTNET_PRIVATE_KEY`
    ///
    /// Returns `None` if no environment variable is set.
    ///
    /// # Errors
    ///
    /// Returns an error if a credential is set but invalid.
    pub fn from_env(is_testnet: bool, authenticator_ids: Vec<u64>) -> anyhow::Result<Option<Self>> {
        let private_key_env = if is_testnet {
            "DYDX_TESTNET_PRIVATE_KEY"
        } else {
            "DYDX_PRIVATE_KEY"
        };

        if let Some(private_key) = std::env::var(private_key_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(Some(Self::from_private_key(
                &private_key,
                authenticator_ids,
            )?));
        }

        Ok(None)
    }

    /// Resolves a [`DydxCredential`] from config values or environment variables.
    ///
    /// Priority:
    /// 1. `private_key` config value
    /// 2. `DYDX_PRIVATE_KEY` / `DYDX_TESTNET_PRIVATE_KEY` env var
    ///
    /// Returns `None` if no credential is available.
    ///
    /// # Errors
    ///
    /// Returns an error if a credential is provided but invalid.
    pub fn resolve(
        private_key: Option<String>,
        is_testnet: bool,
        authenticator_ids: Vec<u64>,
    ) -> anyhow::Result<Option<Self>> {
        // 1. Try private key from config
        if let Some(ref pk) = private_key
            && !pk.trim().is_empty()
        {
            return Ok(Some(Self::from_private_key(pk, authenticator_ids)?));
        }

        // 2. Try private key from env var
        let private_key_env = if is_testnet {
            "DYDX_TESTNET_PRIVATE_KEY"
        } else {
            "DYDX_PRIVATE_KEY"
        };
        if let Some(pk) = std::env::var(private_key_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            return Ok(Some(Self::from_private_key(&pk, authenticator_ids)?));
        }

        Ok(None)
    }

    /// Returns the account ID for this credential.
    ///
    /// # Errors
    ///
    /// Returns an error if the address cannot be parsed as a valid account ID.
    pub fn account_id(&self) -> anyhow::Result<AccountId> {
        self.address
            .parse()
            .map_err(|e| anyhow::anyhow!("Failed to parse account ID: {e}"))
    }

    /// Signs a transaction SignDoc.
    ///
    /// This produces the signature bytes that will be included in the transaction.
    ///
    /// # Errors
    ///
    /// Returns an error if SignDoc serialization or signing fails.
    pub fn sign(&self, sign_doc: &SignDoc) -> anyhow::Result<Vec<u8>> {
        let sign_bytes = sign_doc
            .clone()
            .into_bytes()
            .map_err(|e| anyhow::anyhow!("Failed to serialize SignDoc: {e}"))?;

        let signature = self
            .signing_key
            .sign(&sign_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to sign: {e}"))?;
        Ok(signature.to_bytes().to_vec())
    }

    /// Signs raw message bytes.
    ///
    /// Used for custom signing operations outside of standard transaction flow.
    ///
    /// # Errors
    ///
    /// Returns an error if signing fails.
    pub fn sign_bytes(&self, message: &[u8]) -> anyhow::Result<Vec<u8>> {
        let signature = self
            .signing_key
            .sign(message)
            .map_err(|e| anyhow::anyhow!("Failed to sign: {e}"))?;
        Ok(signature.to_bytes().to_vec())
    }

    /// Returns the public key for this credential.
    pub fn public_key(&self) -> cosmrs::crypto::PublicKey {
        self.signing_key.public_key()
    }
}

/// Resolves wallet address from config value or environment variable.
///
/// Priority:
/// 1. If `wallet_address` is `Some`, use it directly.
/// 2. Otherwise, try to read from environment variable.
///
/// Environment variables:
/// - Mainnet: `DYDX_WALLET_ADDRESS`
/// - Testnet: `DYDX_TESTNET_WALLET_ADDRESS`
///
/// Returns `None` if neither config nor env var provides a wallet address.
#[must_use]
pub fn resolve_wallet_address(wallet_address: Option<String>, is_testnet: bool) -> Option<String> {
    let env_var = if is_testnet {
        "DYDX_TESTNET_WALLET_ADDRESS"
    } else {
        "DYDX_WALLET_ADDRESS"
    };

    get_or_env_var_opt(wallet_address, env_var).filter(|s| !s.trim().is_empty())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Valid test private key (32 bytes, value 1 - simplest valid secp256k1 key)
    const TEST_PRIVATE_KEY: &str =
        "0000000000000000000000000000000000000000000000000000000000000001";

    #[rstest]
    fn test_from_private_key() {
        let credential = DydxCredential::from_private_key(TEST_PRIVATE_KEY, vec![])
            .expect("Failed to create credential from private key");

        assert!(credential.address.starts_with("dydx"));
        assert!(credential.authenticator_ids.is_empty());
    }

    #[rstest]
    fn test_from_private_key_with_authenticators() {
        let credential = DydxCredential::from_private_key(TEST_PRIVATE_KEY, vec![1, 2, 3])
            .expect("Failed to create credential");

        assert_eq!(credential.authenticator_ids, vec![1, 2, 3]);
    }

    #[rstest]
    fn test_from_private_key_with_0x_prefix() {
        let key_with_prefix = format!("0x{TEST_PRIVATE_KEY}");
        let credential = DydxCredential::from_private_key(&key_with_prefix, vec![])
            .expect("Failed to create credential from private key with 0x prefix");

        assert!(credential.address.starts_with("dydx"));
    }

    #[rstest]
    fn test_account_id() {
        let credential = DydxCredential::from_private_key(TEST_PRIVATE_KEY, vec![])
            .expect("Failed to create credential");

        let account_id = credential.account_id().expect("Failed to get account ID");
        assert_eq!(account_id.to_string(), credential.address);
    }

    #[rstest]
    fn test_sign_bytes() {
        let credential = DydxCredential::from_private_key(TEST_PRIVATE_KEY, vec![])
            .expect("Failed to create credential");

        let message = b"test message";
        let signature = credential
            .sign_bytes(message)
            .expect("Failed to sign bytes");

        // secp256k1 signatures are 64 bytes
        assert_eq!(signature.len(), 64);
    }

    #[rstest]
    fn test_debug_redacts_key() {
        let credential = DydxCredential::from_private_key(TEST_PRIVATE_KEY, vec![])
            .expect("Failed to create credential");

        let debug_str = format!("{credential:?}");
        // Should contain redacted marker
        assert!(debug_str.contains("<redacted>"));
        // Should contain the struct name
        assert!(debug_str.contains("DydxCredential"));
        // Should show address
        assert!(debug_str.contains(&credential.address));
    }

    #[rstest]
    fn test_resolve_with_provided_private_key() {
        let result = DydxCredential::resolve(Some(TEST_PRIVATE_KEY.to_string()), false, vec![])
            .expect("Failed to resolve credential");

        assert!(result.is_some());
        let credential = result.unwrap();
        assert!(credential.address.starts_with("dydx"));
    }

    #[rstest]
    fn test_resolve_with_none_and_no_env_var() {
        // Use testnet env var which is unlikely to be set in dev environment
        let result = DydxCredential::resolve(None, true, vec![])
            .expect("Should not error when credential not available");

        // Will be None unless DYDX_TESTNET_PRIVATE_KEY is set
        if std::env::var("DYDX_TESTNET_PRIVATE_KEY").is_err() {
            assert!(result.is_none());
        }
    }

    #[rstest]
    fn test_resolve_wallet_address_with_provided_value() {
        let result = resolve_wallet_address(Some("dydx1abc123".to_string()), false);
        assert_eq!(result, Some("dydx1abc123".to_string()));
    }

    #[rstest]
    fn test_resolve_wallet_address_empty_string_returns_none() {
        let result = resolve_wallet_address(Some(String::new()), false);
        assert!(result.is_none());

        let result = resolve_wallet_address(Some("   ".to_string()), false);
        assert!(result.is_none());
    }

    #[rstest]
    fn test_resolve_wallet_address_with_none_and_no_env_var() {
        // Use testnet env var which is unlikely to be set in dev environment
        let result = resolve_wallet_address(None, true);

        // Will be None unless DYDX_TESTNET_WALLET_ADDRESS is set
        if std::env::var("DYDX_TESTNET_WALLET_ADDRESS").is_err() {
            assert!(result.is_none());
        }
    }
}
