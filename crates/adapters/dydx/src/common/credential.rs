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

//! dYdX credential storage and wallet-based transaction signing helpers.
//!
//! dYdX v4 uses Cosmos SDK-style wallet signing rather than API key authentication.
//! Trading operations require signing transactions with a secp256k1 private key.

#![allow(unused_assignments)] // Fields are accessed externally, false positive from nightly

use std::fmt::{Debug, Formatter};

use anyhow::Context;
use cosmrs::{AccountId, crypto::secp256k1::SigningKey, tx::SignDoc};

use crate::common::consts::DYDX_BECH32_PREFIX;

/// dYdX wallet credentials for signing blockchain transactions.
///
/// Uses secp256k1 for signing as per Cosmos SDK specifications.
pub struct DydxCredential {
    /// The secp256k1 signing key.
    signing_key: SigningKey,
    /// Bech32-encoded account address (e.g., dydx1...).
    pub address: String,
    /// Optional authenticator IDs for permissioned key trading.
    pub authenticator_ids: Vec<u64>,
}

impl Drop for DydxCredential {
    fn drop(&mut self) {
        // Note: SigningKey doesn't implement Zeroize directly
        // Its memory will be securely cleared by cosmrs on drop
    }
}

impl Debug for DydxCredential {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(DydxCredential))
            .field("address", &self.address)
            .field("authenticator_ids", &self.authenticator_ids)
            .field("signing_key", &"<redacted>")
            .finish()
    }
}

impl DydxCredential {
    /// Creates a new [`DydxCredential`] from a mnemonic phrase.
    ///
    /// # Errors
    ///
    /// Returns an error if the mnemonic is invalid or key derivation fails.
    pub fn from_mnemonic(
        mnemonic_phrase: &str,
        account_index: u32,
        authenticator_ids: Vec<u64>,
    ) -> anyhow::Result<Self> {
        use std::str::FromStr;

        use bip32::{DerivationPath, Language, Mnemonic};

        // Derive seed from mnemonic
        let mnemonic =
            Mnemonic::new(mnemonic_phrase, Language::English).context("Invalid mnemonic phrase")?;
        let seed = mnemonic.to_seed("");

        // BIP-44 derivation path: m/44'/118'/0'/0/{account_index}
        // 118 is the Cosmos SLIP-0044 coin type
        let derivation_path = format!("m/44'/118'/0'/0/{account_index}");
        let path = DerivationPath::from_str(&derivation_path).context("Invalid derivation path")?;

        // Derive signing key
        let signing_key =
            SigningKey::derive_from_path(&seed, &path).context("Failed to derive signing key")?;

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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    // Test mnemonic from dYdX v4 client examples
    const TEST_MNEMONIC: &str = "mirror actor skill push coach wait confirm orchard lunch mobile athlete gossip awake miracle matter bus reopen team ladder lazy list timber render wait";

    #[rstest]
    fn test_from_mnemonic() {
        let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![])
            .expect("Failed to create credential");

        assert!(credential.address.starts_with("dydx"));
        assert!(credential.authenticator_ids.is_empty());
    }

    #[rstest]
    fn test_from_mnemonic_with_authenticators() {
        let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![1, 2, 3])
            .expect("Failed to create credential");

        assert_eq!(credential.authenticator_ids, vec![1, 2, 3]);
    }

    #[rstest]
    fn test_from_private_key() {
        // Use a valid test private key (small non-zero value)
        // This is a valid secp256k1 private key: 32 bytes with value 1
        let test_key = format!("{:0>64}", "1");

        let credential = DydxCredential::from_private_key(&test_key, vec![])
            .expect("Failed to create credential from private key");

        assert!(credential.address.starts_with("dydx"));
        assert!(credential.authenticator_ids.is_empty());
    }

    #[rstest]
    fn test_account_id() {
        let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![])
            .expect("Failed to create credential");

        let account_id = credential.account_id().expect("Failed to get account ID");
        assert_eq!(account_id.to_string(), credential.address);
    }

    #[rstest]
    fn test_sign_bytes() {
        let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![])
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
        let credential = DydxCredential::from_mnemonic(TEST_MNEMONIC, 0, vec![])
            .expect("Failed to create credential");

        let debug_str = format!("{credential:?}");
        // Should contain redacted marker
        assert!(debug_str.contains("<redacted>"));
        // Should contain the struct name
        assert!(debug_str.contains("DydxCredential"));
        // Should show address
        assert!(debug_str.contains(&credential.address));
    }
}
