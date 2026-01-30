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

//! Transaction broadcaster for dYdX v4 protocol.
//!
//! This module handles gRPC transmission of transactions with automatic retry
//! on sequence mismatch errors. It works in conjunction with `TransactionManager`
//! to provide reliable transaction delivery.
//!
//! # Retry Logic
//!
//! Uses the battle-tested [`RetryManager`] from `nautilus-network` with exponential
//! backoff. When a transaction fails with "account sequence mismatch" (Cosmos SDK
//! error code 32), the broadcaster:
//!
//! 1. Resyncs the sequence counter from chain
//! 2. Rebuilds the transaction with the new sequence
//! 3. Applies exponential backoff (500ms → 1s → 2s → 4s)
//! 4. Retries up to 5 times
//!
//! This handles the case where multiple transactions are submitted in parallel
//! and one succeeds before the other, invalidating the sequence.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use cosmrs::Any;
use nautilus_network::retry::{RetryConfig, RetryManager};

use super::{tx_manager::TransactionManager, types::PreparedTransaction};
use crate::{error::DydxError, grpc::DydxGrpcClient};

/// Maximum retries for sequence mismatch errors.
pub const MAX_SEQUENCE_RETRIES: u32 = 5;

/// Initial delay between retries in milliseconds.
/// Exponential backoff will increase this: 500 → 1000 → 2000 → 4000ms
const INITIAL_RETRY_DELAY_MS: u64 = 500;

/// Maximum delay between retries in milliseconds.
const MAX_RETRY_DELAY_MS: u64 = 4_000;

/// Maximum total time for all retries in milliseconds (10 seconds).
/// Prevents indefinite retry loops during chain congestion.
const MAX_ELAPSED_MS: u64 = 10_000;

/// Creates a retry manager configured for blockchain transaction broadcasting.
///
/// Configuration optimized for Cosmos SDK sequence management:
/// - 5 retries with exponential backoff (500ms → 4s max)
/// - Small jitter (100ms) to avoid thundering herd
/// - No operation timeout (chain responses can be slow)
/// - 10 second total budget to prevent indefinite waits
#[must_use]
pub fn create_tx_retry_manager() -> RetryManager<DydxError> {
    let config = RetryConfig {
        max_retries: MAX_SEQUENCE_RETRIES,
        initial_delay_ms: INITIAL_RETRY_DELAY_MS,
        max_delay_ms: MAX_RETRY_DELAY_MS,
        backoff_factor: 2.0,
        jitter_ms: 100,
        operation_timeout_ms: None, // Blockchain responses can be slow
        immediate_first: false,     // Always wait before retry (block needs time)
        max_elapsed_ms: Some(MAX_ELAPSED_MS),
    };
    RetryManager::new(config)
}

/// Transaction broadcaster responsible for gRPC transmission with retry logic.
///
/// Works with `TransactionManager` to handle sequence mismatch errors gracefully.
/// Uses [`RetryManager`] with exponential backoff for reliable delivery.
///
/// # Serialization
///
/// All broadcasts are serialized through a semaphore to prevent sequence races.
/// Cosmos SDK requires transactions to be broadcast in sequence order - if tx with
/// sequence 11 arrives before sequence 10, it will fail. The semaphore ensures
/// allocate → build → broadcast happens atomically for each operation.
///
/// # Retry Strategy
///
/// On sequence mismatch (Cosmos SDK error code 32):
/// 1. The `should_retry` callback sets a flag indicating resync is needed
/// 2. The `RetryManager` applies exponential backoff
/// 3. On next attempt, the operation checks the flag and resyncs sequence from chain
/// 4. A new transaction is built with the fresh sequence and broadcast
#[derive(Debug)]
pub struct TxBroadcaster {
    /// gRPC client for broadcasting transactions.
    grpc_client: DydxGrpcClient,
    /// Retry manager for handling transient failures.
    retry_manager: RetryManager<DydxError>,
    /// Semaphore for serializing broadcasts (permits=1 acts as mutex).
    /// Ensures sequence allocation → build → broadcast are atomic.
    broadcast_semaphore: Arc<tokio::sync::Semaphore>,
}

impl TxBroadcaster {
    /// Creates a new transaction broadcaster.
    #[must_use]
    pub fn new(grpc_client: DydxGrpcClient) -> Self {
        Self {
            grpc_client,
            retry_manager: create_tx_retry_manager(),
            broadcast_semaphore: Arc::new(tokio::sync::Semaphore::new(1)),
        }
    }

    /// Broadcasts a prepared transaction with automatic retry on sequence mismatch.
    ///
    /// **Serialization**: Acquires a semaphore permit before allocating sequence,
    /// building, and broadcasting. This ensures transactions are broadcast in
    /// sequence order, preventing "sequence mismatch" errors from concurrent calls.
    ///
    /// On sequence mismatch (code=32), resyncs from chain, allocates new sequence,
    /// rebuilds the transaction, and retries with exponential backoff.
    ///
    /// # Arguments
    ///
    /// * `tx_manager` - Transaction manager for sequence resync and rebuilding
    /// * `msgs` - Original messages to rebuild on retry
    /// * `operation` - Human-readable operation name for logging
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns error if all retries are exhausted or a non-retryable error occurs.
    pub async fn broadcast_with_retry(
        &self,
        tx_manager: &TransactionManager,
        msgs: Vec<Any>,
        operation_name: &str,
    ) -> Result<String, DydxError> {
        // Acquire semaphore to serialize broadcasts.
        // This ensures sequence N is fully broadcast before sequence N+1 is allocated.
        let _permit =
            self.broadcast_semaphore.acquire().await.map_err(|e| {
                DydxError::Nautilus(anyhow::anyhow!("Broadcast semaphore closed: {e}"))
            })?;

        log::debug!("Acquired broadcast permit for {operation_name}");

        // Flag to track if we need to resync sequence before the next attempt.
        // Set by should_retry when a sequence mismatch is detected.
        let needs_resync = Arc::new(AtomicBool::new(false));
        let needs_resync_for_retry = Arc::clone(&needs_resync);

        // Clone values that need to be moved into closures
        let grpc_client = self.grpc_client.clone();
        let op_name = operation_name.to_string();

        let operation = || {
            // Clone captures for the async block
            let needs_resync = Arc::clone(&needs_resync);
            let grpc_client = grpc_client.clone();
            let msgs = msgs.clone();
            let op_name = op_name.clone();

            async move {
                // Resync sequence if previous attempt failed with sequence mismatch
                if needs_resync.swap(false, Ordering::SeqCst) {
                    log::debug!("Resyncing sequence from chain before retry");
                    tx_manager.resync_sequence().await?;
                }

                // Prepare transaction (allocates new sequence)
                let prepared = tx_manager.prepare_transaction(msgs, &op_name).await?;

                // Broadcast
                let mut grpc = grpc_client;
                let tx_hash = grpc.broadcast_tx(prepared.tx_bytes).await.map_err(|e| {
                    log::error!("gRPC broadcast failed for {op_name}: {e}");
                    DydxError::Nautilus(e)
                })?;

                log::info!("{op_name} successfully: tx_hash={tx_hash}");
                Ok(tx_hash)
            }
        };

        let should_retry = move |e: &DydxError| -> bool {
            if e.is_sequence_mismatch() {
                // Set flag so next attempt will resync
                needs_resync_for_retry.store(true, Ordering::SeqCst);
                log::warn!("Sequence mismatch detected, will resync and retry");
                true
            } else if e.is_transient() {
                log::warn!("Transient error detected, will retry: {e}");
                true
            } else {
                false
            }
        };

        let create_error = |msg: String| -> DydxError { DydxError::Nautilus(anyhow::anyhow!(msg)) };

        // Permit is held throughout retry loop, released when _permit drops
        self.retry_manager
            .execute_with_retry(operation_name, operation, should_retry, create_error)
            .await
    }

    /// Broadcasts a prepared transaction without retry.
    ///
    /// Use this for optimistic batching where you handle failures externally,
    /// or when you've already prepared a transaction and want direct control.
    ///
    /// # Returns
    ///
    /// The transaction hash on success.
    ///
    /// # Errors
    ///
    /// Returns error if the gRPC broadcast fails.
    pub async fn broadcast_once(
        &self,
        prepared: &PreparedTransaction,
    ) -> Result<String, DydxError> {
        let mut grpc = self.grpc_client.clone();
        let operation = &prepared.operation;

        let tx_hash = grpc
            .broadcast_tx(prepared.tx_bytes.clone())
            .await
            .map_err(|e| {
                log::error!("gRPC broadcast failed for {operation}: {e}");
                DydxError::Nautilus(e)
            })?;

        log::info!("{operation} successfully: tx_hash={tx_hash}");
        Ok(tx_hash)
    }
}
