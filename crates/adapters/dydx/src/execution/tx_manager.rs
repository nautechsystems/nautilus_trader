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

//! Transaction manager for dYdX v4 protocol.
//!
//! This module provides centralized transaction management including:
//! - Atomic sequence number tracking for stateful (long-term/conditional) orders
//! - Transaction building and signing
//! - Chain synchronization for sequence recovery
//!
//! # Sequence Management
//!
//! dYdX has two transaction types with different sequence behavior:
//!
//! - **Stateful orders** (long-term, conditional): Use Cosmos SDK sequences for replay
//!   protection. Each transaction requires a unique, incrementing sequence number.
//! - **Short-term orders**: Use Good-Til-Block (GTB) for replay protection. The chain's
//!   `ClobDecorator` ante handler skips sequence checking, so sequences are not consumed.
//!   Use [`TransactionManager::get_cached_sequence`] for these — it returns the current value
//!   without incrementing.
//!
//! For stateful orders, this module provides:
//! 1. `AtomicU64` for lock-free sequence allocation via [`TransactionManager::allocate_sequence`]
//! 2. Lazy initialization from chain on first use
//! 3. [`TransactionManager::resync_sequence`] for recovery after mismatch errors
//! 4. Batch allocation via [`TransactionManager::allocate_sequences`] for parallel stateful
//!    broadcasts

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use cosmrs::Any;

use super::{types::PreparedTransaction, wallet::Wallet};
use crate::{
    error::DydxError,
    grpc::{DydxGrpcClient, TxBuilder, types::ChainId},
    proto::AccountAuthenticator,
};

/// Sentinel value indicating sequence is uninitialized.
pub const SEQUENCE_UNINITIALIZED: u64 = u64::MAX;

/// Default fee denomination for dYdX transactions.
const FEE_DENOM: &str = "adydx";

/// Transaction manager responsible for wallet, sequence tracking, and transaction building.
///
/// This is the single source of truth for:
/// - Wallet and signing operations
/// - Sequence numbers (ensuring concurrent order operations don't race)
/// - Authenticator resolution for permissioned key trading
///
/// # Thread Safety
///
/// All methods are safe to call from multiple tasks concurrently. Sequence
/// allocation uses atomic compare-exchange operations for lock-free performance.
#[derive(Debug)]
pub struct TransactionManager {
    /// gRPC client for chain queries.
    grpc_client: DydxGrpcClient,
    /// Wallet for transaction signing (created from private key).
    wallet: Wallet,
    /// Main account address (for account lookups).
    /// May differ from wallet's signing address when using permissioned keys.
    wallet_address: String,
    /// Chain ID for transaction building.
    chain_id: ChainId,
    /// Authenticator IDs for permissioned key trading.
    authenticator_ids: RwLock<Vec<u64>>,
    /// Atomic sequence counter. Value `SEQUENCE_UNINITIALIZED` means uninitialized.
    sequence_number: Arc<AtomicU64>,
    /// Cached account number (never changes for a given address).
    /// Value 0 means uninitialized.
    account_number: AtomicU64,
}

impl TransactionManager {
    /// Creates a new transaction manager.
    ///
    /// Creates wallet from private key internally. The sequence number is initialized
    /// to `SEQUENCE_UNINITIALIZED` and will be fetched from chain on first use, or
    /// can be proactively initialized by calling [`Self::initialize_sequence`].
    ///
    /// # Errors
    ///
    /// Returns error if wallet creation from private key fails.
    pub fn new(
        grpc_client: DydxGrpcClient,
        private_key: &str,
        wallet_address: String,
        chain_id: ChainId,
    ) -> Result<Self, DydxError> {
        let wallet = Wallet::from_private_key(private_key)
            .map_err(|e| DydxError::Wallet(format!("Failed to create wallet: {e}")))?;

        Ok(Self {
            grpc_client,
            wallet,
            wallet_address,
            chain_id,
            authenticator_ids: RwLock::new(Vec::new()),
            sequence_number: Arc::new(AtomicU64::new(SEQUENCE_UNINITIALIZED)),
            account_number: AtomicU64::new(0),
        })
    }

    /// Proactively initializes the sequence number from chain.
    ///
    /// Call this during connect() to ensure orders can be submitted immediately
    /// without first-transaction latency penalty. Also catches auth errors early.
    ///
    /// Returns the initialized sequence number.
    ///
    /// # Errors
    ///
    /// Returns error if chain query fails.
    pub async fn initialize_sequence(&self) -> Result<u64, DydxError> {
        let mut grpc = self.grpc_client.clone();
        let base_account = grpc.get_account(&self.wallet_address).await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to fetch account for sequence init: {e}"
            ))))
        })?;

        let chain_seq = base_account.sequence;
        self.sequence_number.store(chain_seq, Ordering::SeqCst);
        log::debug!("Initialized sequence from chain: {chain_seq}");
        Ok(chain_seq)
    }

    /// Resolves authenticator IDs if using permissioned keys (API wallet).
    ///
    /// Compares the wallet's signing address with the main account address.
    /// If they differ, fetches authenticators from chain and finds the one
    /// matching this wallet's public key.
    ///
    /// Call this during connect() after creating the TransactionManager.
    ///
    /// # Errors
    ///
    /// Returns error if:
    /// - Using permissioned key but no authenticators found for main account
    /// - No authenticator matches the wallet's public key
    /// - gRPC query fails
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    pub async fn resolve_authenticators(&self) -> Result<(), DydxError> {
        // Check if we already have authenticator IDs configured
        {
            let ids = self.authenticator_ids.read().expect("RwLock poisoned");
            if !ids.is_empty() {
                log::debug!("Using pre-configured authenticator IDs: {:?}", *ids);
                return Ok(());
            }
        }

        // Get the wallet's address (derived from private key)
        let account = self
            .wallet
            .account_offline()
            .map_err(|e| DydxError::Wallet(format!("Failed to derive account: {e}")))?;
        let signing_address = account.address.clone();
        let signing_pubkey = account.public_key();

        // Check if we're using an API wallet (signing address != main account)
        if signing_address == self.wallet_address {
            log::debug!(
                "Signing wallet matches main account {}, no authenticator needed",
                self.wallet_address
            );
            return Ok(());
        }

        log::info!(
            "Detected permissioned key setup: signing with {} for main account {}",
            signing_address,
            self.wallet_address
        );

        // Fetch authenticators for the main account
        let mut grpc = self.grpc_client.clone();
        let authenticators = grpc
            .get_authenticators(&self.wallet_address)
            .await
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to fetch authenticators from chain: {e}"
                ))))
            })?;

        if authenticators.is_empty() {
            return Err(DydxError::Config(format!(
                "No authenticators found for {}. \
                 Please create an API Trading Key in the dYdX UI first.",
                self.wallet_address
            )));
        }

        log::debug!(
            "Found {} authenticator(s) for {}",
            authenticators.len(),
            self.wallet_address
        );

        // Find authenticators matching the API wallet's public key
        let signing_pubkey_bytes = signing_pubkey.to_bytes();
        let signing_pubkey_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &signing_pubkey_bytes,
        );

        let mut matching_ids = Vec::new();
        for auth in &authenticators {
            if Self::authenticator_matches_pubkey(auth, &signing_pubkey_b64) {
                matching_ids.push(auth.id);
                log::info!("Found matching authenticator: id={}", auth.id);
            }
        }

        if matching_ids.is_empty() {
            return Err(DydxError::Config(format!(
                "No authenticator matches the API wallet's public key. \
                 Ensure the API Trading Key was created for wallet {}. \
                 Available authenticators: {:?}",
                signing_address,
                authenticators.iter().map(|a| a.id).collect::<Vec<_>>()
            )));
        }

        // Store the resolved authenticator IDs
        {
            let mut ids = self.authenticator_ids.write().expect("RwLock poisoned");
            *ids = matching_ids.clone();
        }
        log::info!("Resolved authenticator IDs: {matching_ids:?}");

        Ok(())
    }

    /// Checks if an authenticator contains a SignatureVerification matching the public key.
    ///
    /// Expected authenticator config format (JSON array of sub-authenticators):
    /// ```json
    /// [{"type": "SignatureVerification", "config": "<base64-pubkey>"}, ...]
    /// ```
    fn authenticator_matches_pubkey(auth: &AccountAuthenticator, pubkey_b64: &str) -> bool {
        #[derive(serde::Deserialize)]
        struct SubAuth {
            #[serde(rename = "type")]
            auth_type: String,
            config: String,
        }

        // auth.config is raw bytes (Vec<u8>) containing JSON
        let config_str = match String::from_utf8(auth.config.clone()) {
            Ok(s) => s,
            Err(e) => {
                log::warn!(
                    "Authenticator id={} has invalid UTF-8 config (len={}): {}",
                    auth.id,
                    auth.config.len(),
                    e
                );
                return false;
            }
        };

        log::debug!(
            "Checking authenticator id={}, type={}, config={}",
            auth.id,
            auth.r#type,
            config_str
        );

        match serde_json::from_str::<Vec<SubAuth>>(&config_str) {
            Ok(sub_auths) => {
                for sub in sub_auths {
                    log::debug!(
                        "  Sub-authenticator: type={}, config={}",
                        sub.auth_type,
                        sub.config
                    );

                    if sub.auth_type == "SignatureVerification" && sub.config == pubkey_b64 {
                        log::debug!("  -> MATCH! pubkey_b64={pubkey_b64}");
                        return true;
                    }
                }
            }
            Err(e) => {
                log::warn!(
                    "Authenticator id={} config is not in expected JSON array format: {} (config={})",
                    auth.id,
                    e,
                    config_str
                );
            }
        }

        false
    }

    /// Allocates the next sequence number atomically.
    ///
    /// If the sequence is uninitialized (0), fetches from chain first.
    /// Uses compare-exchange for lock-free concurrent access.
    ///
    /// # Errors
    ///
    /// Returns error if chain query fails during initialization.
    pub async fn allocate_sequence(&self) -> Result<u64, DydxError> {
        loop {
            let current = self.sequence_number.load(Ordering::SeqCst);
            if current == SEQUENCE_UNINITIALIZED {
                // Initialize from chain
                self.initialize_sequence_from_chain().await?;
                continue;
            }
            // Atomic get-and-increment
            if self
                .sequence_number
                .compare_exchange(current, current + 1, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok(current);
            }
            // Another thread modified it, retry
        }
    }

    /// Allocates N sequence numbers for optimistic parallel broadcast.
    ///
    /// Returns a vector of consecutive sequences that can be used concurrently.
    /// The caller is responsible for handling partial failures by resyncing.
    ///
    /// # Arguments
    ///
    /// * `count` - Number of sequences to allocate
    ///
    /// # Errors
    ///
    /// Returns error if chain query fails during initialization.
    ///
    /// # Example
    ///
    /// ```ignore
    /// let sequences = tx_manager.allocate_sequences(3).await?;
    /// // sequences = [10, 11, 12] - three consecutive sequence numbers
    /// ```
    pub async fn allocate_sequences(&self, count: usize) -> Result<Vec<u64>, DydxError> {
        if count == 0 {
            return Ok(Vec::new());
        }

        loop {
            let current = self.sequence_number.load(Ordering::SeqCst);
            if current == SEQUENCE_UNINITIALIZED {
                self.initialize_sequence_from_chain().await?;
                continue;
            }
            let new_value = current + count as u64;

            if self
                .sequence_number
                .compare_exchange(current, new_value, Ordering::SeqCst, Ordering::SeqCst)
                .is_ok()
            {
                return Ok((current..new_value).collect());
            }
            // Another thread modified it, retry
        }
    }

    /// Initializes the sequence counter from chain state.
    ///
    /// Only sets the value if it's still 0 (another thread might have set it).
    async fn initialize_sequence_from_chain(&self) -> Result<(), DydxError> {
        let mut grpc = self.grpc_client.clone();
        let base_account = grpc.get_account(&self.wallet_address).await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to fetch account for sequence init: {e}"
            ))))
        })?;

        let chain_seq = base_account.sequence;
        // Only set if still uninitialized (another thread might have set it)
        if self
            .sequence_number
            .compare_exchange(
                SEQUENCE_UNINITIALIZED,
                chain_seq,
                Ordering::SeqCst,
                Ordering::SeqCst,
            )
            .is_ok()
        {
            log::debug!("Initialized sequence from chain: {chain_seq}");
        }
        Ok(())
    }

    /// Resyncs the sequence counter from chain after a mismatch error.
    ///
    /// Called by the broadcaster's retry logic when a sequence mismatch is detected.
    /// Unconditionally stores the chain's current sequence.
    ///
    /// # Errors
    ///
    /// Returns error if chain query fails.
    pub async fn resync_sequence(&self) -> Result<(), DydxError> {
        let mut grpc = self.grpc_client.clone();
        let base_account = grpc.get_account(&self.wallet_address).await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to fetch account for resync: {e}"
            ))))
        })?;

        let chain_seq = base_account.sequence;
        self.sequence_number.store(chain_seq, Ordering::SeqCst);
        log::info!("Resynced sequence from chain: {chain_seq}");
        Ok(())
    }

    /// Returns the current sequence value without allocation.
    ///
    /// Useful for logging and debugging. Returns `SEQUENCE_UNINITIALIZED` if not yet initialized.
    #[must_use]
    pub fn current_sequence(&self) -> u64 {
        self.sequence_number.load(Ordering::SeqCst)
    }

    /// Returns the cached sequence for short-term orders without incrementing.
    ///
    /// # Errors
    ///
    /// Returns error if chain query fails during initialization.
    pub async fn get_cached_sequence(&self) -> Result<u64, DydxError> {
        let current = self.sequence_number.load(Ordering::SeqCst);
        if current == SEQUENCE_UNINITIALIZED {
            self.initialize_sequence_from_chain().await?;
            return Ok(self.sequence_number.load(Ordering::SeqCst));
        }
        Ok(current)
    }

    /// Builds and signs a transaction with the given messages and sequence.
    ///
    /// Uses cached account_number (fetched once from chain) to avoid repeated queries.
    ///
    /// # Arguments
    ///
    /// * `msgs` - Proto messages to include in transaction
    /// * `sequence` - Pre-allocated sequence number
    /// * `operation` - Human-readable name for logging
    ///
    /// # Errors
    ///
    /// Returns error if account lookup fails or transaction building fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal `RwLock` is poisoned.
    pub async fn build_transaction(
        &self,
        msgs: Vec<Any>,
        sequence: u64,
        operation: &str,
    ) -> Result<PreparedTransaction, DydxError> {
        // Derive account for signing (address/account_id are cached in wallet)
        let mut account = self
            .wallet
            .account_offline()
            .map_err(|e| DydxError::Wallet(format!("Failed to derive account: {e}")))?;

        // Read authenticator IDs (resolved during connect if using permissioned keys)
        let auth_ids_snapshot: Vec<u64> = {
            let ids = self.authenticator_ids.read().expect("RwLock poisoned");
            ids.clone()
        };

        if !auth_ids_snapshot.is_empty() {
            log::debug!(
                "Using permissioned key mode: signing with {} for main account {}",
                account.address,
                self.wallet_address
            );
        }

        // Get or cache account number (it never changes for a given address)
        let account_num = self.get_or_fetch_account_number().await?;

        // Set account info for signing
        account.set_account_info(account_num, sequence);

        // Build transaction
        let tx_builder =
            TxBuilder::new(self.chain_id.clone(), FEE_DENOM.to_string()).map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "TxBuilder init failed: {e}"
                ))))
            })?;

        // For permissioned key trading, each message needs an authenticator ID.
        // Repeat the configured authenticator ID(s) for each message in the batch.
        let expanded_auth_ids: Vec<u64> = if auth_ids_snapshot.is_empty() {
            Vec::new()
        } else {
            // For each message, use the first authenticator ID
            // (typically there's only one configured for the trading key)
            std::iter::repeat_n(auth_ids_snapshot[0], msgs.len()).collect()
        };

        let auth_ids = if expanded_auth_ids.is_empty() {
            None
        } else {
            Some(expanded_auth_ids.as_slice())
        };

        let tx_raw = tx_builder
            .build_transaction(&account, msgs, None, auth_ids)
            .map_err(|e| {
                DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                    "Failed to build tx: {e}"
                ))))
            })?;

        let tx_bytes = tx_raw.to_bytes().map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to serialize tx: {e}"
            ))))
        })?;

        log::debug!(
            "Built {} with {} bytes, sequence={}",
            operation,
            tx_bytes.len(),
            sequence
        );

        Ok(PreparedTransaction {
            tx_bytes,
            sequence,
            operation: operation.to_string(),
        })
    }

    /// Gets the cached account number, or fetches it from chain if not yet cached.
    ///
    /// Account numbers are immutable on-chain, so we only need to fetch once.
    async fn get_or_fetch_account_number(&self) -> Result<u64, DydxError> {
        let cached = self.account_number.load(Ordering::SeqCst);
        if cached != 0 {
            return Ok(cached);
        }

        // Fetch from chain
        let mut grpc = self.grpc_client.clone();
        let base_account = grpc.get_account(&self.wallet_address).await.map_err(|e| {
            DydxError::Grpc(Box::new(tonic::Status::internal(format!(
                "Failed to fetch account: {e}"
            ))))
        })?;

        let account_num = base_account.account_number;

        // Cache it (CAS to handle concurrent fetches)
        let _ = self.account_number.compare_exchange(
            0,
            account_num,
            Ordering::SeqCst,
            Ordering::SeqCst,
        );

        log::debug!("Cached account_number from chain: {account_num}");
        Ok(account_num)
    }

    /// Convenience method: allocate sequence, build, and return prepared transaction.
    ///
    /// This is the typical flow for single transaction submission.
    ///
    /// # Arguments
    ///
    /// * `msgs` - Proto messages to include in transaction
    /// * `operation` - Human-readable name for logging
    ///
    /// # Errors
    ///
    /// Returns error if sequence allocation or transaction building fails.
    pub async fn prepare_transaction(
        &self,
        msgs: Vec<Any>,
        operation: &str,
    ) -> Result<PreparedTransaction, DydxError> {
        let sequence = self.allocate_sequence().await?;
        self.build_transaction(msgs, sequence, operation).await
    }

    /// Returns the wallet address.
    #[must_use]
    pub fn wallet_address(&self) -> &str {
        &self.wallet_address
    }

    /// Returns the chain ID.
    #[must_use]
    pub fn chain_id(&self) -> &ChainId {
        &self.chain_id
    }
}
