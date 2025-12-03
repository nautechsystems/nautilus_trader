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

use std::{cmp::max, collections::HashSet};

use alloy::primitives::Address;
use futures_util::StreamExt;
use nautilus_model::defi::{
    SharedDex,
    amm::Pool,
    chain::SharedChain,
    reporting::{BlockchainSyncReportItems, BlockchainSyncReporter},
    token::Token,
};
use thousands::Separable;
use tokio_util::sync::CancellationToken;

use crate::{
    cache::BlockchainCache,
    config::BlockchainDataClientConfig,
    contracts::erc20::Erc20Contract,
    events::pool_created::PoolCreatedEvent,
    exchanges::extended::DexExtended,
    hypersync::{client::HyperSyncClient, helpers::extract_block_number},
};

const BLOCKS_PROCESS_IN_SYNC_REPORT: u64 = 50_000;
const POOL_DB_BATCH_SIZE: usize = 2000;

/// Sanitizes a string by removing null bytes and other invalid characters for PostgreSQL UTF-8.
///
/// This function strips null bytes (0x00) and other problematic control characters that are
/// invalid in PostgreSQL's UTF-8 text fields. Common with malformed on-chain token metadata.
/// Preserves printable characters and common whitespace (space, tab, newline).
fn sanitize_string(s: String) -> String {
    s.chars()
        .filter(|c| {
            // Keep printable characters and common whitespace, but filter null bytes
            // and other problematic control characters
            *c != '\0' && (*c >= ' ' || *c == '\t' || *c == '\n' || *c == '\r')
        })
        .collect()
}

/// Service responsible for discovering DEX liquidity pools from blockchain events.
///
/// This service handles the synchronization of pool creation events from various DEXes,
/// managing token metadata fetching, buffering strategies, and database persistence.
#[derive(Debug)]
pub struct PoolDiscoveryService<'a> {
    /// The blockchain network being synced
    chain: SharedChain,
    /// Cache for tokens and pools
    cache: &'a mut BlockchainCache,
    /// ERC20 contract interface for token metadata
    erc20_contract: &'a Erc20Contract,
    /// HyperSync client for event streaming
    hypersync_client: &'a HyperSyncClient,
    /// Cancellation token for graceful shutdown
    cancellation_token: CancellationToken,
    /// Configuration for sync operations
    config: BlockchainDataClientConfig,
}

impl<'a> PoolDiscoveryService<'a> {
    /// Creates a new [`PoolDiscoveryService`] instance.
    #[must_use]
    pub const fn new(
        chain: SharedChain,
        cache: &'a mut BlockchainCache,
        erc20_contract: &'a Erc20Contract,
        hypersync_client: &'a HyperSyncClient,
        cancellation_token: CancellationToken,
        config: BlockchainDataClientConfig,
    ) -> Self {
        Self {
            chain,
            cache,
            erc20_contract,
            hypersync_client,
            cancellation_token,
            config,
        }
    }

    /// Synchronizes pools for a specific DEX within a given block range.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - HyperSync streaming fails
    /// - Token RPC calls fail
    /// - Database operations fail
    /// - Sync is cancelled
    pub async fn sync_pools(
        &mut self,
        dex: &DexExtended,
        from_block: u64,
        to_block: Option<u64>,
        reset: bool,
    ) -> anyhow::Result<()> {
        // Determine effective sync range
        let (last_synced_block, effective_from_block) = if reset {
            (None, from_block)
        } else {
            let last_synced_block = self.cache.get_dex_last_synced_block(&dex.dex.name).await?;
            let effective_from_block = last_synced_block
                .map_or(from_block, |last_synced| max(from_block, last_synced + 1));
            (last_synced_block, effective_from_block)
        };

        let to_block = match to_block {
            Some(block) => block,
            None => self.hypersync_client.current_block().await,
        };

        // Skip sync if already up to date
        if effective_from_block > to_block {
            tracing::info!(
                "DEX {} already synced to block {} (current: {}), skipping sync",
                dex.dex.name,
                last_synced_block.unwrap_or(0).separate_with_commas(),
                to_block.separate_with_commas()
            );
            return Ok(());
        }

        let total_blocks = to_block.saturating_sub(effective_from_block) + 1;
        tracing::info!(
            "Syncing DEX exchange pools from {} to {} (total: {} blocks){}",
            effective_from_block.separate_with_commas(),
            to_block.separate_with_commas(),
            total_blocks.separate_with_commas(),
            if let Some(last_synced) = last_synced_block {
                format!(
                    " - resuming from last synced block {}",
                    last_synced.separate_with_commas()
                )
            } else {
                String::new()
            },
        );
        tracing::info!(
            "Syncing {} pool creation events from factory contract {} on chain {}",
            dex.dex.name,
            dex.factory,
            self.chain.name
        );

        // Enable performance settings for sync operations
        if let Err(e) = self.cache.toggle_performance_settings(true).await {
            tracing::warn!("Failed to enable performance settings: {e}");
        }

        let mut metrics = BlockchainSyncReporter::new(
            BlockchainSyncReportItems::PoolCreatedEvents,
            effective_from_block,
            total_blocks,
            BLOCKS_PROCESS_IN_SYNC_REPORT,
        );

        let factory_address = &dex.factory;
        let pair_created_event_signature = dex.pool_created_event.as_ref();
        let pools_stream = self
            .hypersync_client
            .request_contract_events_stream(
                effective_from_block,
                Some(to_block),
                factory_address,
                vec![pair_created_event_signature],
            )
            .await;

        tokio::pin!(pools_stream);

        // LEVEL 1: RPC buffers (small, constrained by rate limits)
        let token_rpc_batch_size = (self.config.multicall_calls_per_rpc_request / 3) as usize;
        let mut token_rpc_buffer: HashSet<Address> = HashSet::new();

        // LEVEL 2: DB buffers (large, optimize for throughput)
        let mut token_db_buffer: Vec<Token> = Vec::new();
        let mut pool_events_buffer: Vec<PoolCreatedEvent> = Vec::new();

        let mut last_block_saved = effective_from_block;

        // Tracking counters
        let mut total_discovered = 0;
        let mut total_skipped_exists = 0;
        let mut total_skipped_invalid_tokens = 0;
        let mut total_saved = 0;

        let cancellation_token = self.cancellation_token.clone();
        let sync_result = tokio::select! {
            () = cancellation_token.cancelled() => {
                tracing::info!("Exchange pool sync cancelled");
                Err(anyhow::anyhow!("Sync cancelled"))
            }

            result = async {
                while let Some(log) = pools_stream.next().await {
                    let block_number = extract_block_number(&log)?;
                    let blocks_progress = block_number - last_block_saved;
                    last_block_saved = block_number;

                    let pool = dex.parse_pool_created_event_hypersync(log)?;
                    total_discovered += 1;

                    if self.cache.get_pool(&pool.pool_identifier).is_some() {
                        // Pool is already initialized and cached.
                        total_skipped_exists += 1;
                        continue;
                    }

                    if self.cache.is_invalid_token(&pool.token0)
                        || self.cache.is_invalid_token(&pool.token1)
                    {
                        // Skip pools with invalid tokens as they cannot be properly processed or traded.
                        total_skipped_invalid_tokens += 1;
                        continue;
                    }

                    // Collect tokens needed for RPC fetch
                    if self.cache.get_token(&pool.token0).is_none() {
                        token_rpc_buffer.insert(pool.token0);
                    }
                    if self.cache.get_token(&pool.token1).is_none() {
                        token_rpc_buffer.insert(pool.token1);
                    }

                    // Buffer the pool for later processing
                    pool_events_buffer.push(pool);

                    // ==== RPC FLUSHING (small batches) ====
                    if token_rpc_buffer.len() >= token_rpc_batch_size {
                        let fetched_tokens = self
                            .fetch_and_cache_tokens_in_memory(&mut token_rpc_buffer)
                            .await?;

                        // Accumulate for later DB write
                        token_db_buffer.extend(fetched_tokens);
                    }

                    // ==== DB FLUSHING (large batches) ====
                    // Process pools when buffer is full
                    if pool_events_buffer.len() >= POOL_DB_BATCH_SIZE {
                        // 1. Fetch any remaining tokens in RPC buffer (needed for pool construction)
                        if !token_rpc_buffer.is_empty() {
                            let fetched_tokens = self
                                .fetch_and_cache_tokens_in_memory(&mut token_rpc_buffer)
                                .await?;
                            token_db_buffer.extend(fetched_tokens);
                        }

                        // 2. Flush ALL tokens to DB (satisfy foreign key constraints)
                        if !token_db_buffer.is_empty() {
                            self.cache
                                .add_tokens_batch(std::mem::take(&mut token_db_buffer))
                                .await?;
                        }

                        // 3. Now safe to construct and flush pools
                        let pools = self
                            .construct_pools_batch(&mut pool_events_buffer, &dex.dex)
                            .await?;
                        total_saved += pools.len();
                        self.cache.add_pools_batch(pools).await?;
                    }

                    metrics.update(blocks_progress as usize);
                    // Log progress if needed
                    if metrics.should_log_progress(block_number, to_block) {
                        metrics.log_progress(block_number);
                    }
                }

                // ==== FINAL FLUSH (all remaining data) ====
                // 1. Fetch any remaining tokens
                if !token_rpc_buffer.is_empty() {
                    let fetched_tokens = self
                        .fetch_and_cache_tokens_in_memory(&mut token_rpc_buffer)
                        .await?;
                    token_db_buffer.extend(fetched_tokens);
                }

                // 2. Flush all tokens to DB
                if !token_db_buffer.is_empty() {
                    self.cache
                        .add_tokens_batch(std::mem::take(&mut token_db_buffer))
                        .await?;
                }

                // 3. Process and flush all pools
                if !pool_events_buffer.is_empty() {
                    let pools = self
                        .construct_pools_batch(&mut pool_events_buffer, &dex.dex)
                        .await?;
                    total_saved += pools.len();
                    self.cache.add_pools_batch(pools).await?;
                }

                metrics.log_final_stats();

                // Update the last synced block after successful completion.
                self.cache
                    .update_dex_last_synced_block(&dex.dex.name, to_block)
                    .await?;

                tracing::info!(
                    "Successfully synced DEX {} pools up to block {} | Summary: discovered={}, saved={}, skipped_exists={}, skipped_invalid_tokens={}",
                    dex.dex.name,
                    to_block.separate_with_commas(),
                    total_discovered,
                    total_saved,
                    total_skipped_exists,
                    total_skipped_invalid_tokens
                );

                Ok(())
            } => result
        };

        sync_result?;

        // Restore default safe settings after sync completion
        if let Err(e) = self.cache.toggle_performance_settings(false).await {
            tracing::warn!("Failed to restore default settings: {e}");
        }

        Ok(())
    }

    /// Fetches token metadata via RPC and updates in-memory cache immediately.
    ///
    /// This method fetches token information using multicall, updates the in-memory cache right away
    /// (so pool construction can proceed), and returns valid tokens for later batch DB writes.
    ///
    /// # Errors
    ///
    /// Returns an error if the RPC multicall fails or database operations fail.
    async fn fetch_and_cache_tokens_in_memory(
        &mut self,
        token_buffer: &mut HashSet<Address>,
    ) -> anyhow::Result<Vec<Token>> {
        let batch_addresses: Vec<Address> = token_buffer.drain().collect();
        let token_infos = self
            .erc20_contract
            .batch_fetch_token_info(&batch_addresses)
            .await?;

        let mut valid_tokens = Vec::new();

        for (token_address, token_info) in token_infos {
            match token_info {
                Ok(token_info) => {
                    // Sanitize token metadata to remove null bytes and invalid UTF-8 characters
                    let sanitized_name = sanitize_string(token_info.name);
                    let sanitized_symbol = sanitize_string(token_info.symbol);

                    let token = Token::new(
                        self.chain.clone(),
                        token_address,
                        sanitized_name,
                        sanitized_symbol,
                        token_info.decimals,
                    );

                    // Update in-memory cache IMMEDIATELY (so construct_pool can read it)
                    self.cache.insert_token_in_memory(token.clone());

                    // Collect for LATER DB write
                    valid_tokens.push(token);
                }
                Err(token_info_error) => {
                    self.cache.insert_invalid_token_in_memory(token_address);
                    if let Some(database) = &self.cache.database {
                        let sanitized_error = sanitize_string(token_info_error.to_string());
                        database
                            .add_invalid_token(
                                self.chain.chain_id,
                                &token_address,
                                &sanitized_error,
                            )
                            .await?;
                    }
                }
            }
        }

        Ok(valid_tokens)
    }

    /// Constructs multiple pools from pool creation events.
    ///
    /// Assumes all required tokens are already in the in-memory cache.
    ///
    /// # Errors
    ///
    /// Logs errors for pools that cannot be constructed (missing tokens),
    /// but does not fail the entire batch.
    async fn construct_pools_batch(
        &mut self,
        pool_events: &mut Vec<PoolCreatedEvent>,
        dex: &SharedDex,
    ) -> anyhow::Result<Vec<Pool>> {
        let mut pools = Vec::with_capacity(pool_events.len());

        for pool_event in pool_events.drain(..) {
            // Both tokens should be in cache now
            let token0 = match self.cache.get_token(&pool_event.token0) {
                Some(token) => token.clone(),
                None => {
                    if !self.cache.is_invalid_token(&pool_event.token0) {
                        tracing::warn!(
                            "Skipping pool {}: Token0 {} not in cache and not marked as invalid",
                            pool_event.pool_address,
                            pool_event.token0
                        );
                    }
                    continue;
                }
            };

            let token1 = match self.cache.get_token(&pool_event.token1) {
                Some(token) => token.clone(),
                None => {
                    if !self.cache.is_invalid_token(&pool_event.token1) {
                        tracing::warn!(
                            "Skipping pool {}: Token1 {} not in cache and not marked as invalid",
                            pool_event.pool_address,
                            pool_event.token1
                        );
                    }
                    continue;
                }
            };

            let mut pool = Pool::new(
                self.chain.clone(),
                dex.clone(),
                pool_event.pool_address,
                pool_event.pool_identifier,
                pool_event.block_number,
                token0,
                token1,
                pool_event.fee,
                pool_event.tick_spacing,
                nautilus_core::UnixNanos::default(),
            );

            // Set hooks if available (UniswapV4)
            if let Some(hooks) = pool_event.hooks {
                pool.set_hooks(hooks);
            }

            // Initialize pool with sqrt_price_x96 and tick if available (UniswapV4)
            if let (Some(sqrt_price_x96), Some(tick)) = (pool_event.sqrt_price_x96, pool_event.tick)
            {
                pool.initialize(sqrt_price_x96, tick);
            }

            pools.push(pool);
        }

        Ok(pools)
    }
}
