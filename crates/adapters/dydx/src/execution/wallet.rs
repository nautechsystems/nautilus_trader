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

//! Wallet and account management for dYdX v4.
//!
//! This module provides wallet functionality for managing signing keys for Cosmos SDK transactions.
//! Wallets are created from hex-encoded private keys.

use std::fmt::Debug;

use anyhow::Context;
use cosmrs::{
    AccountId,
    crypto::{PublicKey, secp256k1::SigningKey},
    tx,
};

/// Account prefix for dYdX addresses.
///
/// See [Cosmos accounts](https://docs.cosmos.network/main/learn/beginner/accounts).
const BECH32_PREFIX_DYDX: &str = "dydx";

/// Wallet for dYdX v4 transaction signing.
///
/// A wallet holds a secp256k1 private key used to sign Cosmos SDK transactions.
/// The private key bytes are stored to allow recreating SigningKey (which doesn't
/// implement Clone). Address and account_id are pre-computed during construction
/// to avoid repeated derivation.
///
/// # Security
///
/// Private key bytes should be treated as sensitive material.
pub struct Wallet {
    /// Raw private key bytes (32 bytes for secp256k1).
    /// Stored separately because SigningKey doesn't implement Clone or expose bytes.
    private_key_bytes: Box<[u8]>,
    /// Pre-computed dYdX address (bech32 encoded).
    address: String,
    /// Pre-computed Cosmos SDK account ID.
    account_id: AccountId,
}

impl Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Wallet))
            .field("private_key_bytes", &"<redacted>")
            .field("address", &self.address)
            .finish()
    }
}

impl Clone for Wallet {
    fn clone(&self) -> Self {
        Self {
            private_key_bytes: self.private_key_bytes.clone(),
            address: self.address.clone(),
            account_id: self.account_id.clone(),
        }
    }
}

impl Wallet {
    /// Create a wallet from a hex-encoded private key.
    ///
    /// The private key should be a 32-byte secp256k1 key encoded as hex,
    /// optionally with a `0x` prefix. Address and account ID are derived
    /// during construction.
    ///
    /// # Errors
    ///
    /// Returns an error if the private key is invalid hex or not a valid secp256k1 key.
    pub fn from_private_key(private_key_hex: &str) -> anyhow::Result<Self> {
        let key_bytes = hex::decode(private_key_hex.trim_start_matches("0x"))
            .context("Invalid hex private key")?;

        // Validate the key and derive address/account_id
        let signing_key = SigningKey::from_slice(&key_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid secp256k1 private key: {e}"))?;

        let public_key = signing_key.public_key();
        let account_id = public_key
            .account_id(BECH32_PREFIX_DYDX)
            .map_err(|e| anyhow::anyhow!("Failed to derive account ID: {e}"))?;
        let address = account_id.to_string();

        Ok(Self {
            private_key_bytes: key_bytes.into_boxed_slice(),
            address,
            account_id,
        })
    }

    /// Get a dYdX account with zero account and sequence numbers.
    ///
    /// Creates an account using the pre-computed address/account_id.
    /// SigningKey is recreated from stored bytes (it doesn't implement Clone).
    /// Account and sequence numbers must be set before signing.
    ///
    /// # Errors
    ///
    /// Returns an error if the signing key creation fails.
    pub fn account_offline(&self) -> Result<Account, anyhow::Error> {
        // SigningKey doesn't impl Clone, so recreate from stored bytes
        let key = SigningKey::from_slice(&self.private_key_bytes)
            .map_err(|e| anyhow::anyhow!("Failed to create signing key: {e}"))?;

        Ok(Account {
            address: self.address.clone(),
            account_id: self.account_id.clone(),
            key,
            account_number: 0,
            sequence_number: 0,
        })
    }

    /// Returns the pre-computed wallet address.
    #[must_use]
    pub fn address(&self) -> &str {
        &self.address
    }
}

/// Represents a dYdX account.
///
/// An account contains the signing key and metadata needed to sign and broadcast transactions.
/// The `account_number` and `sequence_number` must be set from on-chain data before signing.
///
/// See also [`Wallet`].
pub struct Account {
    /// dYdX address (bech32 encoded).
    pub address: String,
    /// Cosmos SDK account ID.
    pub account_id: AccountId,
    /// Private signing key.
    key: SigningKey,
    /// On-chain account number (must be fetched before signing).
    pub account_number: u64,
    /// Transaction sequence number (must be fetched before signing).
    pub sequence_number: u64,
}

impl Debug for Account {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(Account))
            .field("address", &self.address)
            .field("account_id", &self.account_id)
            .field("key", &"<redacted>")
            .field("account_number", &self.account_number)
            .field("sequence_number", &self.sequence_number)
            .finish()
    }
}

impl Account {
    /// Get the public key associated with this account.
    #[must_use]
    pub fn public_key(&self) -> PublicKey {
        self.key.public_key()
    }

    /// Sign a [`SignDoc`](tx::SignDoc) with the private key.
    ///
    /// # Errors
    ///
    /// Returns an error if signing fails.
    pub fn sign(&self, doc: tx::SignDoc) -> Result<tx::Raw, anyhow::Error> {
        doc.sign(&self.key)
            .map_err(|e| anyhow::anyhow!("Failed to sign transaction: {e}"))
    }

    /// Update account and sequence numbers from on-chain data.
    pub fn set_account_info(&mut self, account_number: u64, sequence_number: u64) {
        self.account_number = account_number;
        self.sequence_number = sequence_number;
    }

    /// Increment the sequence number (used after successful transaction broadcast).
    pub fn increment_sequence(&mut self) {
        self.sequence_number += 1;
    }

    /// Derive a subaccount for this account.
    ///
    /// # Errors
    ///
    /// Returns an error if the subaccount number is invalid.
    pub fn subaccount(&self, number: u32) -> Result<Subaccount, anyhow::Error> {
        Ok(Subaccount {
            address: self.address.clone(),
            number,
        })
    }
}

/// A subaccount within a dYdX account.
///
/// Each account can have multiple subaccounts for organizing positions and balances.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Subaccount {
    /// Parent account address.
    pub address: String,
    /// Subaccount number.
    pub number: u32,
}

impl Subaccount {
    /// Create a new subaccount.
    #[must_use]
    pub fn new(address: String, number: u32) -> Self {
        Self { address, number }
    }
}
