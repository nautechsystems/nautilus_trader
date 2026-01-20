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
//! - Atomic sequence number tracking across all order operations
//! - Transaction building and signing
//! - Chain synchronization for sequence recovery
//!
//! # Sequence Management
//!
//! dYdX (Cosmos SDK) requires each transaction to have a unique, incrementing sequence number.
//! When multiple transactions are submitted in parallel, they can race for the same sequence,
//! causing "account sequence mismatch" errors.
//!
//! This module solves this by:
//! 1. Using `AtomicU64` for lock-free sequence allocation
//! 2. Initializing from chain on first use (lazy initialization)
//! 3. Providing `resync_sequence()` for recovery after errors
//! 4. Supporting batch sequence allocation for optimistic parallel broadcasts

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

// Note: Arc is still used for sequence_number (shared across components)
use cosmrs::Any;

use super::{types::PreparedTransaction, wallet::Wallet};
use crate::{
    error::DydxError,
    grpc::{DydxGrpcClient, TxBuilder, types::ChainId},
};

/// Default fee denomination for dYdX transactions.
const FEE_DENOM: &str = "adydx";

/// Transaction manager responsible for sequence tracking and transaction building.
///
/// This is the single source of truth for sequence numbers, ensuring that
/// concurrent order operations don't race for the same sequence.
///
/// # Thread Safety
///
/// All methods are safe to call from multiple tasks concurrently. Sequence
/// allocation uses atomic compare-exchange operations for lock-free performance.
#[derive(Debug)]
pub struct TransactionManager {
    /// gRPC client for chain queries.
    grpc_client: DydxGrpcClient,
    /// Wallet for transaction signing (owned, not shared).
    wallet: Wallet,
    /// Main account address (for account lookups).
    /// May differ from wallet address when using permissioned keys.
    wallet_address: String,
    /// Chain ID for transaction building.
    chain_id: ChainId,
    /// Authenticator IDs for permissioned key trading.
    authenticator_ids: Vec<u64>,
    /// Atomic sequence counter. Value 0 means uninitialized.
    sequence_number: Arc<AtomicU64>,
    /// Cached account number (never changes for a given address).
    /// Value 0 means uninitialized.
    account_number: AtomicU64,
}

impl TransactionManager {
    /// Creates a new transaction manager.
    ///
    /// # Arguments
    ///
    /// * `grpc_client` - gRPC client for chain queries
    /// * `wallet` - Wallet for signing (required)
    /// * `wallet_address` - Main account address (may differ from wallet address for permissioned keys)
    /// * `chain_id` - dYdX chain ID
    /// * `authenticator_ids` - Authenticator IDs for permissioned trading (empty for direct signing)
    /// * `sequence_number` - Shared atomic sequence counter
    #[must_use]
    pub fn new(
        grpc_client: DydxGrpcClient,
        wallet: Wallet,
        wallet_address: String,
        chain_id: ChainId,
        authenticator_ids: Vec<u64>,
        sequence_number: Arc<AtomicU64>,
    ) -> Self {
        Self {
            grpc_client,
            wallet,
            wallet_address,
            chain_id,
            authenticator_ids,
            sequence_number,
            account_number: AtomicU64::new(0),
        }
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
            if current == 0 {
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
            if current == 0 {
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
        // Only set if still 0 (another thread might have set it)
        if self
            .sequence_number
            .compare_exchange(0, chain_seq, Ordering::SeqCst, Ordering::SeqCst)
            .is_ok()
        {
            log::info!("Initialized sequence from chain: {chain_seq}");
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
    /// Useful for logging and debugging. Returns 0 if uninitialized.
    #[must_use]
    pub fn current_sequence(&self) -> u64 {
        self.sequence_number.load(Ordering::SeqCst)
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

        if !self.authenticator_ids.is_empty() {
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
        let expanded_auth_ids: Vec<u64> = if self.authenticator_ids.is_empty() {
            Vec::new()
        } else {
            // For each message, use the first authenticator ID
            // (typically there's only one configured for the trading key)
            std::iter::repeat_n(self.authenticator_ids[0], msgs.len()).collect()
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

    /// Returns a reference to the shared sequence counter.
    ///
    /// Useful for sharing with other components that need sequence access.
    #[must_use]
    pub fn sequence_number(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.sequence_number)
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
