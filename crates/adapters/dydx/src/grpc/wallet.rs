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

//! Wallet and account management for dYdX v4.
//!
//! This module provides wallet functionality for deriving accounts from BIP-39 mnemonics
//! and managing signing keys for Cosmos SDK transactions.

use std::{
    fmt::{Debug, Formatter},
    str::FromStr,
};

use cosmrs::{
    AccountId,
    bip32::{DerivationPath, Language, Mnemonic, Seed},
    crypto::{PublicKey, secp256k1::SigningKey},
    tx,
};

/// Account prefix for dYdX addresses.
///
/// See [Cosmos accounts](https://docs.cosmos.network/main/learn/beginner/accounts).
const BECH32_PREFIX_DYDX: &str = "dydx";

/// Hierarchical Deterministic (HD) [wallet](https://dydx.exchange/crypto-learning/glossary?#wallet)
/// which allows multiple addresses and signing keys from one master seed.
///
/// [BIP-44](https://github.com/bitcoin/bips/blob/master/bip-0044.mediawiki) introduced a wallet
/// standard to derive multiple accounts for different chains from a single seed (which allows
/// recovery of the whole tree of keys).
///
/// This `Wallet` uses the Cosmos ATOM derivation path to generate dYdX addresses.
///
/// See also [Mastering Bitcoin](https://github.com/bitcoinbook/bitcoinbook/blob/develop/ch05_wallets.adoc).
pub struct Wallet {
    seed: Seed,
}

impl Debug for Wallet {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("seed", &"<redacted>")
            .finish()
    }
}

impl Wallet {
    /// Derive a seed from a 24-word English mnemonic phrase.
    ///
    /// # Errors
    ///
    /// Returns an error if the mnemonic is invalid or cannot be converted to a seed.
    pub fn from_mnemonic(mnemonic: &str) -> Result<Self, anyhow::Error> {
        let seed = Mnemonic::new(mnemonic, Language::English)?.to_seed("");
        Ok(Self { seed })
    }

    /// Derive a dYdX account with zero account and sequence numbers.
    ///
    /// Account and sequence numbers must be fetched from the chain before signing transactions.
    ///
    /// # Errors
    ///
    /// Returns an error if the account derivation fails.
    pub fn account_offline(&self, index: u32) -> Result<Account, anyhow::Error> {
        self.derive_account(index, BECH32_PREFIX_DYDX)
    }

    fn derive_account(&self, index: u32, prefix: &str) -> Result<Account, anyhow::Error> {
        // BIP-44 derivation path for Cosmos (coin type 118)
        // See https://github.com/satoshilabs/slips/blob/master/slip-0044.md
        let derivation_str = format!("m/44'/118'/0'/0/{index}");
        let derivation_path = DerivationPath::from_str(&derivation_str)?;
        let private_key = SigningKey::derive_from_path(&self.seed, &derivation_path)?;
        let public_key = private_key.public_key();
        let account_id = public_key.account_id(prefix).map_err(anyhow::Error::msg)?;
        let address = account_id.to_string();

        Ok(Account {
            index,
            address,
            account_id,
            key: private_key,
            account_number: 0,
            sequence_number: 0,
        })
    }
}

/// Represents a derived dYdX account.
///
/// An account contains the signing key and metadata needed to sign and broadcast transactions.
/// The `account_number` and `sequence_number` must be set from on-chain data before signing.
///
/// See also [`Wallet`].
pub struct Account {
    /// Derivation index of the account.
    pub index: u32,
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
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Account")
            .field("index", &self.index)
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
