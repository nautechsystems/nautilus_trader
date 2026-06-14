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

//! Caching layer for blockchain entities and domain objects.
//!
//! This module provides an in-memory cache with optional PostgreSQL persistence for storing
//! and retrieving blockchain-related data such as blocks, tokens, pools, swaps, and other
//! DeFi protocol events.

use std::{
    collections::{BTreeMap, HashMap, HashSet},
    sync::Arc,
};

use alloy::primitives::Address;
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    Block, DexType, Pool, PoolIdentifier, PoolLiquidityUpdate, PoolSwap, SharedChain, SharedDex,
    SharedPool, Token,
    data::{PoolFeeCollect, PoolFlash},
    pool_analysis::{position::PoolPosition, snapshot::PoolSnapshot},
    tick_map::tick::PoolTick,
};
use sqlx::postgres::PgConnectOptions;

use crate::{
    cache::{consistency::CachedBlocksConsistencyStatus, database::BlockchainCacheDatabase},
    events::initialize::InitializeEvent,
};

pub mod consistency;
pub mod copy;
pub mod database;
pub mod rows;
pub mod types;

/// Provides caching functionality for various blockchain domain objects.
#[derive(Debug)]
pub struct BlockchainCache {
    /// The blockchain chain this cache is associated with.
    chain: SharedChain,
    /// Map of block numbers to their corresponding timestamp
    block_timestamps: BTreeMap<u64, UnixNanos>,
    /// Map of DEX identifiers to their corresponding DEX objects.
    dexes: HashMap<DexType, SharedDex>,
    /// Map of token addresses to their corresponding `Token` objects.
    tokens: HashMap<Address, Token>,
    /// Cached set of invalid token addresses that failed validation or processing.
    invalid_tokens: HashSet<Address>,
    /// Map of pool identifiers to their corresponding `Pool` objects.
    pools: HashMap<PoolIdentifier, SharedPool>,
    /// Optional database connection for persistent storage.
    pub database: Option<BlockchainCacheDatabase>,
}

impl BlockchainCache {
    /// Creates a new in-memory blockchain cache for the specified chain.
    #[must_use]
    pub fn new(chain: SharedChain) -> Self {
        Self {
            chain,
            dexes: HashMap::new(),
            tokens: HashMap::new(),
            invalid_tokens: HashSet::new(),
            pools: HashMap::new(),
            block_timestamps: BTreeMap::new(),
            database: None,
        }
    }

    /// Returns the highest continuous block number currently cached, if any.
    pub async fn get_cache_block_consistency_status(
        &self,
    ) -> Option<CachedBlocksConsistencyStatus> {
        let database = self.database.as_ref()?;
        database
            .get_block_consistency_status(&self.chain)
            .await
            .map_err(|e| log::error!("Error getting block consistency status: {e}"))
            .ok()
    }

    /// Returns the earliest block number where any DEX in the cache was created on the blockchain.
    #[must_use]
    pub fn min_dex_creation_block(&self) -> Option<u64> {
        self.dexes
            .values()
            .map(|dex| dex.factory_creation_block)
            .min()
    }

    /// Returns the timestamp for the specified block number if it exists in the cache.
    #[must_use]
    pub fn get_block_timestamp(&self, block_number: u64) -> Option<&UnixNanos> {
        self.block_timestamps.get(&block_number)
    }

    /// Records a block timestamp in the in-memory cache without persisting it.
    ///
    /// Used while streaming pool events so event conversion can resolve `ts_event` for blocks
    /// that have not been persisted via [`Self::add_block`].
    pub fn cache_block_timestamp(&mut self, number: u64, timestamp: UnixNanos) {
        self.block_timestamps.insert(number, timestamp);
    }

    /// Initializes the database connection for persistent storage.
    pub async fn initialize_database(&mut self, pg_connect_options: PgConnectOptions) {
        let database = BlockchainCacheDatabase::init(pg_connect_options).await;
        self.database = Some(database);
    }

    /// Toggles performance optimization settings in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database is not initialized or the operation fails.
    pub async fn toggle_performance_settings(&self, enable: bool) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.toggle_perf_sync_settings(enable).await
        } else {
            log::warn!("Database not initialized, skipping performance settings toggle");
            Ok(())
        }
    }

    /// Initializes the chain by seeding it in the database and creating necessary partitions.
    ///
    /// This method sets up the blockchain chain in the database, creates block and token
    /// partitions for optimal performance, and loads existing tokens into the cache.
    pub async fn initialize_chain(&mut self) {
        // Seed target adapter chain in database
        if let Some(database) = &self.database {
            if let Err(e) = database.seed_chain(&self.chain).await {
                log::error!(
                    "Error seeding chain in database: {e}. Continuing without database cache functionality"
                );
                return;
            }
            log::info!("Chain seeded in the database");

            match database.create_block_partition(&self.chain).await {
                Ok(message) => log::info!("Executing block partition creation: {message}"),
                Err(e) => log::error!(
                    "Error creating block partition for chain {}: {e}. Continuing without partition creation...",
                    self.chain.chain_id
                ),
            }

            match database.create_token_partition(&self.chain).await {
                Ok(message) => log::info!("Executing token partition creation: {message}"),
                Err(e) => log::error!(
                    "Error creating token partition for chain {}: {e}. Continuing without partition creation...",
                    self.chain.chain_id
                ),
            }
        }

        if let Err(e) = self.load_tokens().await {
            log::error!("Error loading tokens from the database: {e}");
        }
    }

    /// Connects to the database and loads initial data.
    ///
    /// # Errors
    ///
    /// Returns an error if database seeding, token loading, or block loading fails.
    pub async fn connect(&mut self, from_block: u64) -> anyhow::Result<()> {
        log::debug!("Connecting and loading from_block {from_block}");

        if let Err(e) = self.load_tokens().await {
            log::error!("Error loading tokens from the database: {e}");
        }

        // TODO disable block syncing for now as we don't have timestamps yet configured
        // if let Err(e) = self.load_blocks(from_block).await {
        //     log::error!("Error loading blocks from database: {e}");
        // }

        Ok(())
    }

    /// Loads tokens from the database into the in-memory cache.
    async fn load_tokens(&mut self) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            let (tokens, invalid_tokens) = tokio::try_join!(
                database.load_tokens(self.chain.clone()),
                database.load_invalid_token_addresses(self.chain.chain_id)
            )?;

            log::info!(
                "Loading {} valid tokens and {} invalid tokens from cache database",
                tokens.len(),
                invalid_tokens.len()
            );

            self.tokens
                .extend(tokens.into_iter().map(|token| (token.address, token)));
            self.invalid_tokens.extend(invalid_tokens);
        }
        Ok(())
    }

    /// Loads DEX exchange pools from the database into the in-memory cache.
    ///
    /// Returns the loaded pools.
    ///
    /// # Errors
    ///
    /// Returns an error if the DEX has not been registered or if database operations fail.
    pub async fn load_pools(&mut self, dex_id: &DexType) -> anyhow::Result<Vec<Pool>> {
        let mut loaded_pools = Vec::new();

        if let Some(database) = &self.database {
            let dex = self
                .get_dex(dex_id)
                .ok_or_else(|| anyhow::anyhow!("DEX {dex_id:?} has not been registered"))?;
            let pool_rows = database
                .load_pools(self.chain.clone(), &dex_id.to_string())
                .await?;
            log::info!(
                "Loading {} pools for DEX {} from cache database",
                pool_rows.len(),
                dex_id,
            );

            for pool_row in pool_rows {
                let token0 = if let Some(token) = self.tokens.get(&pool_row.token0_address) {
                    token
                } else {
                    log::error!(
                        "Failed to load pool {} for DEX {}: Token0 with address {} not found in cache. \
                             This may indicate the token was not properly loaded from the database or the pool references an unknown token",
                        pool_row.address,
                        dex_id,
                        pool_row.token0_address
                    );
                    continue;
                };

                let token1 = if let Some(token) = self.tokens.get(&pool_row.token1_address) {
                    token
                } else {
                    log::error!(
                        "Failed to load pool {} for DEX {}: Token1 with address {} not found in cache. \
                             This may indicate the token was not properly loaded from the database or the pool references an unknown token",
                        pool_row.address,
                        dex_id,
                        pool_row.token1_address
                    );
                    continue;
                };

                // Construct pool from row data and cached tokens
                let Some(pool_identifier) = pool_row.pool_identifier.parse().ok() else {
                    log::error!(
                        "Invalid pool identifier '{}' in database for pool {}, skipping",
                        pool_row.pool_identifier,
                        pool_row.address
                    );
                    continue;
                };
                let ts_init = pool_row.creation_block_timestamp.unwrap_or_default();
                let mut pool = Pool::new(
                    self.chain.clone(),
                    dex.clone(),
                    pool_row.address,
                    pool_identifier,
                    pool_row.creation_block as u64,
                    token0.clone(),
                    token1.clone(),
                    pool_row.fee.map(|fee| fee as u32),
                    pool_row
                        .tick_spacing
                        .map(|tick_spacing| tick_spacing as u32),
                    ts_init,
                );

                // Set hooks if available
                if let Some(ref hook_address_str) = pool_row.hook_address
                    && let Ok(hooks) = hook_address_str.parse()
                {
                    pool.set_hooks(hooks);
                }

                // Initialize pool with initial values if available
                if let Some(initial_sqrt_price_x96_str) = &pool_row.initial_sqrt_price_x96
                    && let Ok(initial_sqrt_price_x96) = initial_sqrt_price_x96_str.parse()
                    && let Some(initial_tick) = pool_row.initial_tick
                {
                    pool.initialize(initial_sqrt_price_x96, initial_tick);
                }

                // Add pool to cache and loaded pools list
                loaded_pools.push(pool.clone());
                self.pools.insert(pool.pool_identifier, Arc::new(pool));
            }
        }
        Ok(loaded_pools)
    }

    /// Loads block timestamps from the database starting `from_block` number
    /// into the in-memory cache.
    #[allow(dead_code)]
    async fn load_blocks(&mut self, from_block: u64) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            let block_timestamps = database
                .load_block_timestamps(self.chain.clone(), from_block)
                .await?;

            // Verify block number sequence consistency
            if !block_timestamps.is_empty() {
                let first = block_timestamps.first().unwrap().number;
                let last = block_timestamps.last().unwrap().number;
                let expected_len = (last - first + 1) as usize;
                if block_timestamps.len() != expected_len {
                    anyhow::bail!(
                        "Block timestamps are not consistent and sequential. Expected {expected_len} blocks but got {}",
                        block_timestamps.len()
                    );
                }
            }

            if block_timestamps.is_empty() {
                log::info!("No blocks found in database");
                return Ok(());
            }

            log::info!(
                "Loading {} blocks timestamps from the cache database with last block number {}",
                block_timestamps.len(),
                block_timestamps.last().unwrap().number,
            );

            for block in block_timestamps {
                self.block_timestamps.insert(block.number, block.timestamp);
            }
        }
        Ok(())
    }

    /// Adds a block to the cache and persists it to the database if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the block to the database fails.
    pub async fn add_block(&mut self, block: Block) -> anyhow::Result<()> {
        // Populate in-memory first so the timestamp resolves even if persistence fails
        self.block_timestamps.insert(block.number, block.timestamp);
        if let Some(database) = &self.database {
            database.add_block(self.chain.chain_id, &block).await?;
        }
        Ok(())
    }

    /// Adds multiple blocks to the cache and persists them to the database in batch if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the blocks to the database fails.
    pub async fn add_blocks_batch(
        &mut self,
        blocks: Vec<Block>,
        use_copy_command: bool,
    ) -> anyhow::Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        if let Some(database) = &self.database {
            if use_copy_command {
                database
                    .add_blocks_copy(self.chain.chain_id, &blocks)
                    .await?;
            } else {
                database
                    .add_blocks_batch(self.chain.chain_id, &blocks)
                    .await?;
            }
        }

        // Update in-memory cache
        for block in blocks {
            self.block_timestamps.insert(block.number, block.timestamp);
        }

        Ok(())
    }

    /// Adds block timestamps observed while streaming pool events.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the block timestamps to the database fails.
    pub async fn add_pool_event_blocks_batch(&mut self, blocks: Vec<Block>) -> anyhow::Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        if let Some(database) = &self.database {
            database
                .add_pool_event_blocks_batch(self.chain.chain_id, &blocks)
                .await?;
        }

        for block in blocks {
            self.block_timestamps.insert(block.number, block.timestamp);
        }

        Ok(())
    }

    /// Adds a DEX to the cache with the specified identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the DEX to the database fails.
    pub async fn add_dex(&mut self, dex: SharedDex) -> anyhow::Result<()> {
        log::info!("Adding dex {} to the cache", dex.name);

        if let Some(database) = &self.database {
            database.add_dex(dex.clone()).await?;
        }

        self.dexes.insert(dex.name, dex);
        Ok(())
    }

    /// Adds a liquidity pool/pair to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the pool to the database fails.
    pub async fn add_pool(&mut self, pool: Pool) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_pool(&pool).await?;
        }

        self.pools.insert(pool.pool_identifier, Arc::new(pool));
        Ok(())
    }

    /// Adds multiple pools to the cache and persists them to the database in batch if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the pools to the database fails.
    pub async fn add_pools_batch(&mut self, pools: Vec<Pool>) -> anyhow::Result<()> {
        if pools.is_empty() {
            return Ok(());
        }

        if let Some(database) = &self.database {
            database.add_pools_copy(self.chain.chain_id, &pools).await?;
        }
        self.pools.extend(
            pools
                .into_iter()
                .map(|pool| (pool.pool_identifier, Arc::new(pool))),
        );

        Ok(())
    }

    /// Adds a [`Token`] to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the token to the database fails.
    pub async fn add_token(&mut self, token: Token) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_token(&token).await?;
        }
        self.tokens.insert(token.address, token);
        Ok(())
    }

    /// Adds multiple tokens to the cache and persists them to the database in batch if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the tokens to the database fails.
    pub async fn add_tokens_batch(&mut self, tokens: Vec<Token>) -> anyhow::Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }

        if let Some(database) = &self.database {
            database
                .add_tokens_copy(self.chain.chain_id, &tokens)
                .await?;
        }

        self.tokens
            .extend(tokens.into_iter().map(|token| (token.address, token)));

        Ok(())
    }

    /// Updates the in-memory token cache without persisting to the database.
    pub fn insert_token_in_memory(&mut self, token: Token) {
        self.tokens.insert(token.address, token);
    }

    /// Marks a token address as invalid in the in-memory cache without persisting to the database.
    pub fn insert_invalid_token_in_memory(&mut self, address: Address) {
        self.invalid_tokens.insert(address);
    }

    /// Adds an invalid token address with associated error information to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the invalid token to the database fails.
    pub async fn add_invalid_token(
        &mut self,
        address: Address,
        error_string: &str,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .add_invalid_token(self.chain.chain_id, &address, error_string)
                .await?;
        }
        self.invalid_tokens.insert(address);
        Ok(())
    }

    /// Adds a [`PoolSwap`] to the cache database if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the swap to the database fails.
    pub async fn add_pool_swap(&self, swap: &PoolSwap) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_swap(self.chain.chain_id, swap).await?;
        }

        Ok(())
    }

    /// Adds a [`PoolLiquidityUpdate`] to the cache database if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the liquidity update to the database fails.
    pub async fn add_liquidity_update(
        &self,
        liquidity_update: &PoolLiquidityUpdate,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .add_pool_liquidity_update(self.chain.chain_id, liquidity_update)
                .await?;
        }

        Ok(())
    }

    /// Adds multiple [`PoolSwap`]s to the cache database in a single batch operation if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the swaps to the database fails.
    pub async fn add_pool_swaps_batch(
        &self,
        swaps: &[PoolSwap],
        use_copy_command: bool,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            if use_copy_command {
                database
                    .add_pool_swaps_copy(self.chain.chain_id, swaps)
                    .await?;
            } else {
                database
                    .add_pool_swaps_batch(self.chain.chain_id, swaps)
                    .await?;
            }
        }

        Ok(())
    }

    /// Adds multiple [`PoolLiquidityUpdate`]s to the cache database in a single batch operation if available.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the liquidity updates to the database fails.
    pub async fn add_pool_liquidity_updates_batch(
        &self,
        updates: &[PoolLiquidityUpdate],
        use_copy_command: bool,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            if use_copy_command {
                database
                    .add_pool_liquidity_updates_copy(self.chain.chain_id, updates)
                    .await?;
            } else {
                database
                    .add_pool_liquidity_updates_batch(self.chain.chain_id, updates)
                    .await?;
            }
        }

        Ok(())
    }

    /// Adds a batch of pool fee collect events to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the fee collects to the database fails.
    pub async fn add_pool_fee_collects_batch(
        &self,
        collects: &[PoolFeeCollect],
        use_copy_command: bool,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            if use_copy_command {
                database
                    .copy_pool_fee_collects_batch(self.chain.chain_id, collects)
                    .await?;
            } else {
                database
                    .add_pool_collects_batch(self.chain.chain_id, collects)
                    .await?;
            }
        }

        Ok(())
    }

    /// Adds a batch of pool flash events to the cache.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the flash events to the database fails.
    pub async fn add_pool_flash_batch(&self, flash_events: &[PoolFlash]) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .add_pool_flash_batch(self.chain.chain_id, flash_events)
                .await?;
        }

        Ok(())
    }

    /// Adds a pool snapshot to the cache database.
    ///
    /// This method saves the complete snapshot including:
    /// - Pool state and analytics (pool_snapshot table)
    /// - All positions at this snapshot (pool_position table)
    /// - All ticks at this snapshot (pool_tick table)
    ///
    /// # Errors
    ///
    /// Returns an error if adding the snapshot to the database fails.
    pub async fn add_pool_snapshot(
        &self,
        dex: &DexType,
        pool_identifier: &PoolIdentifier,
        snapshot: &PoolSnapshot,
    ) -> anyhow::Result<()> {
        // Reject stub snapshots at the pool's creation block: empty positions, empty ticks,
        // and the snapshot block matching pool creation indicates a bootstrap that bailed
        // before any liquidity events landed. A legitimately empty pool (e.g., fully burned)
        // would have its last_processed_event at the burn block, not at creation, so the
        // creation-block check preserves those valid checkpoints.
        if snapshot.positions.is_empty()
            && snapshot.ticks.is_empty()
            && let Some(pool) = self.pools.get(pool_identifier)
            && snapshot.block_position.number == pool.creation_block
        {
            log::warn!(
                "Refusing to persist empty stub snapshot for {} at pool creation block {}",
                snapshot.instrument_id,
                snapshot.block_position.number,
            );
            return Ok(());
        }

        if let Some(database) = &self.database {
            // Save snapshot first (required for foreign key constraints)
            database
                .add_pool_snapshot(self.chain.chain_id, dex, pool_identifier, snapshot)
                .await?;

            let positions: Vec<(PoolIdentifier, PoolPosition)> = snapshot
                .positions
                .iter()
                .map(|pos| (*pool_identifier, pos.clone()))
                .collect();

            if !positions.is_empty() {
                database
                    .add_pool_positions_batch(
                        self.chain.chain_id,
                        snapshot.block_position.number,
                        snapshot.block_position.transaction_index,
                        snapshot.block_position.log_index,
                        &positions,
                    )
                    .await?;
            }

            let ticks: Vec<(PoolIdentifier, &PoolTick)> = snapshot
                .ticks
                .iter()
                .map(|tick| (*pool_identifier, tick))
                .collect();

            if !ticks.is_empty() {
                database
                    .add_pool_ticks_batch(
                        self.chain.chain_id,
                        snapshot.block_position.number,
                        snapshot.block_position.transaction_index,
                        snapshot.block_position.log_index,
                        &ticks,
                    )
                    .await?;
            }
        }

        Ok(())
    }

    /// Updates the initial price and tick for a pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn update_pool_initialize_price_tick(
        &mut self,
        initialize_event: &InitializeEvent,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .update_pool_initial_price_tick(self.chain.chain_id, initialize_event)
                .await?;
        }

        // Update the cached pool if it exists
        let pool_identifier = initialize_event.pool_identifier;
        if let Some(cached_pool) = self.pools.get(&pool_identifier) {
            let mut updated_pool = (**cached_pool).clone();
            updated_pool.initialize(initialize_event.sqrt_price_x96, initialize_event.tick);

            self.pools.insert(pool_identifier, Arc::new(updated_pool));
        }

        Ok(())
    }

    /// Returns a reference to the `DexExtended` associated with the given name.
    #[must_use]
    pub fn get_dex(&self, dex_id: &DexType) -> Option<SharedDex> {
        self.dexes.get(dex_id).cloned()
    }

    /// Returns a list of registered `DexType` in the cache.
    #[must_use]
    pub fn get_registered_dexes(&self) -> HashSet<DexType> {
        self.dexes.keys().copied().collect()
    }

    /// Returns a reference to the pool associated with the given address.
    #[must_use]
    pub fn get_pool(&self, pool_identifier: &PoolIdentifier) -> Option<&SharedPool> {
        self.pools.get(pool_identifier)
    }

    /// Returns a reference to the `Token` associated with the given address.
    #[must_use]
    pub fn get_token(&self, address: &Address) -> Option<&Token> {
        self.tokens.get(address)
    }

    /// Checks if a token address is marked as invalid in the cache.
    ///
    /// Returns `true` if the address was previously recorded as invalid due to
    /// validation or processing failures.
    #[must_use]
    pub fn is_invalid_token(&self, address: &Address) -> bool {
        self.invalid_tokens.contains(address)
    }

    /// Saves the checkpoint block number indicating the last completed pool synchronization for a specific DEX.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn update_dex_last_synced_block(
        &self,
        dex: &DexType,
        block_number: u64,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .update_dex_last_synced_block(self.chain.chain_id, dex, block_number)
                .await
        } else {
            Ok(())
        }
    }

    /// Updates the last synced block number for a pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the database update fails.
    pub async fn update_pool_last_synced_block(
        &self,
        dex: &DexType,
        pool_identifier: &PoolIdentifier,
        block_number: u64,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .update_pool_last_synced_block(
                    self.chain.chain_id,
                    dex,
                    pool_identifier,
                    block_number,
                )
                .await
        } else {
            Ok(())
        }
    }

    /// Retrieves the saved checkpoint block number from the last completed pool synchronization for a specific DEX.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_dex_last_synced_block(&self, dex: &DexType) -> anyhow::Result<Option<u64>> {
        if let Some(database) = &self.database {
            database
                .get_dex_last_synced_block(self.chain.chain_id, dex)
                .await
        } else {
            Ok(None)
        }
    }

    /// Retrieves the last synced block number for a pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_pool_last_synced_block(
        &self,
        dex: &DexType,
        pool_identifier: &PoolIdentifier,
    ) -> anyhow::Result<Option<u64>> {
        if let Some(database) = &self.database {
            database
                .get_pool_last_synced_block(self.chain.chain_id, dex, pool_identifier)
                .await
        } else {
            Ok(None)
        }
    }

    /// Retrieves the maximum block number across all pool event tables for a given pool.
    ///
    /// # Errors
    ///
    /// Returns an error if any of the database queries fail.
    pub async fn get_pool_event_tables_last_block(
        &self,
        pool_identifier: &PoolIdentifier,
    ) -> anyhow::Result<Option<u64>> {
        if let Some(database) = &self.database {
            let (swaps_last_block, liquidity_last_block, collect_last_block, flash_last_block) = tokio::try_join!(
                database.get_table_last_block(
                    self.chain.chain_id,
                    "pool_swap_event",
                    pool_identifier
                ),
                database.get_table_last_block(
                    self.chain.chain_id,
                    "pool_liquidity_event",
                    pool_identifier
                ),
                database.get_table_last_block(
                    self.chain.chain_id,
                    "pool_collect_event",
                    pool_identifier
                ),
                database.get_table_last_block(
                    self.chain.chain_id,
                    "pool_flash_event",
                    pool_identifier
                ),
            )?;

            let max_block = [
                swaps_last_block,
                liquidity_last_block,
                collect_last_block,
                flash_last_block,
            ]
            .into_iter()
            .flatten()
            .max();
            Ok(max_block)
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::Arc,
        time::{SystemTime, UNIX_EPOCH},
    };

    use alloy::primitives::address;
    use futures_util::TryStreamExt;
    use nautilus_core::UnixNanos;
    use nautilus_infrastructure::sql::pg::get_postgres_connect_options;
    use nautilus_model::defi::{
        AmmType, Block, Blockchain, Chain, Dex, SharedChain, SharedDex, Token, data::DexPoolData,
    };
    use rstest::rstest;
    use sqlx::{
        AssertSqlSafe, PgPool,
        postgres::{PgConnectOptions, PgPoolOptions},
    };
    use ustr::Ustr;

    use super::*;

    fn test_cache() -> BlockchainCache {
        BlockchainCache::new(Arc::new(Chain::new(Blockchain::Ethereum, 1)))
    }

    #[rstest]
    fn cache_block_timestamp_records_in_memory() {
        let mut cache = test_cache();
        assert_eq!(cache.get_block_timestamp(100), None);

        cache.cache_block_timestamp(100, UnixNanos::from(1_700_000_000_000_000_000));

        assert_eq!(
            cache.get_block_timestamp(100),
            Some(&UnixNanos::from(1_700_000_000_000_000_000))
        );
    }

    #[tokio::test]
    async fn add_block_populates_timestamp_without_database() {
        let mut cache = test_cache();
        let block = Block::new(
            "0x1".to_string(),
            "0x0".to_string(),
            42,
            Ustr::from("miner"),
            30_000_000,
            21_000,
            UnixNanos::from(1_700_000_000_000_000_000),
            Some(Blockchain::Ethereum),
        );

        cache.add_block(block).await.unwrap();

        assert_eq!(
            cache.get_block_timestamp(42),
            Some(&UnixNanos::from(1_700_000_000_000_000_000))
        );
    }

    #[tokio::test]
    async fn stream_pool_events_uses_pool_event_block_timestamp_without_full_block()
    -> anyhow::Result<()> {
        let Some((database, schema)) = connect_cache_test_database().await? else {
            return Ok(());
        };
        let chain = arbitrum();
        let dex = uniswap_v3(&chain);
        let pool_address = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
        let pool_identifier = PoolIdentifier::from_address(pool_address);
        let instrument_id = Pool::create_instrument_id(chain.name, &dex, pool_identifier.as_str());
        let expected_ts = UnixNanos::from(1_700_000_000_123_456_789);

        database
            .add_pool_event_blocks_batch(chain.chain_id, &[test_block(12, expected_ts)])
            .await?;
        insert_pool_swap_event(
            &schema.admin_pool,
            &schema.name,
            chain.chain_id,
            &pool_identifier,
            12,
        )
        .await?;
        let events_result = database
            .stream_pool_events(chain, dex, instrument_id, pool_identifier, None, Some(12))
            .try_collect::<Vec<_>>()
            .await;

        drop(database);
        schema.cleanup().await?;

        let events = events_result?;
        let observed_timestamps = match events.as_slice() {
            [DexPoolData::Swap(swap)] => Some((swap.ts_event, swap.ts_init)),
            _ => None,
        };

        let expected_timestamps = Some((expected_ts, expected_ts));
        if observed_timestamps != expected_timestamps {
            anyhow::bail!(
                "unexpected stream timestamps: expected {expected_timestamps:?}, observed {observed_timestamps:?}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn load_block_timestamps_prefers_full_block_over_pool_event_block() -> anyhow::Result<()>
    {
        let Some((database, schema)) = connect_cache_test_database().await? else {
            return Ok(());
        };
        let chain = arbitrum();
        let fallback_ts = UnixNanos::from(1_700_000_000_000_000_000);
        let pool_event_ts = UnixNanos::from(1_700_000_002_000_000_000);
        let full_block_ts = UnixNanos::from(1_700_000_001_000_000_000);

        database
            .add_pool_event_blocks_batch(
                chain.chain_id,
                &[test_block(20, fallback_ts), test_block(21, pool_event_ts)],
            )
            .await?;
        database
            .add_block(chain.chain_id, &test_block(21, full_block_ts))
            .await?;

        let rows_result = database.load_block_timestamps(chain, 20).await;

        drop(database);
        schema.cleanup().await?;

        let rows = rows_result?;
        let observed = rows
            .into_iter()
            .map(|row| (row.number, row.timestamp))
            .collect::<Vec<_>>();

        let expected = vec![(20, fallback_ts), (21, full_block_ts)];
        if observed != expected {
            anyhow::bail!(
                "unexpected block timestamps: expected {expected:?}, observed {observed:?}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn load_block_timestamps_uses_pool_event_block_when_full_block_timestamp_is_null()
    -> anyhow::Result<()> {
        let Some((database, schema)) = connect_cache_test_database().await? else {
            return Ok(());
        };
        let chain = arbitrum();
        let fallback_ts = UnixNanos::from(1_700_000_004_000_000_000);

        database
            .add_pool_event_blocks_batch(chain.chain_id, &[test_block(22, fallback_ts)])
            .await?;
        insert_block_without_timestamp(&schema.admin_pool, &schema.name, chain.chain_id, 22)
            .await?;

        let rows_result = database.load_block_timestamps(chain, 22).await;

        drop(database);
        schema.cleanup().await?;

        let rows = rows_result?;
        let observed = rows
            .into_iter()
            .map(|row| (row.number, row.timestamp))
            .collect::<Vec<_>>();

        let expected = vec![(22, fallback_ts)];
        if observed != expected {
            anyhow::bail!(
                "unexpected block timestamps: expected {expected:?}, observed {observed:?}"
            );
        }
        Ok(())
    }

    #[tokio::test]
    async fn load_pools_sets_pool_timestamps_from_pool_event_block() -> anyhow::Result<()> {
        let Some((database, schema)) = connect_cache_test_database().await? else {
            return Ok(());
        };
        let chain = arbitrum();
        let dex = uniswap_v3(&chain);
        let token0 = weth(&chain);
        let token1 = usdc(&chain);
        let pool_address = address!("0xd13040d4fe917EE704158CfCB3338dCd2838B245");
        let pool_identifier = PoolIdentifier::from_address(pool_address);
        let creation_block = 30;
        let creation_ts = UnixNanos::from(1_700_000_003_000_000_000);
        let pool = Pool::new(
            chain.clone(),
            dex.clone(),
            pool_address,
            pool_identifier,
            creation_block,
            token0.clone(),
            token1.clone(),
            Some(500),
            Some(10),
            UnixNanos::default(),
        );
        let mut cache = BlockchainCache::new(chain.clone());
        cache.database = Some(database);

        cache.add_dex(dex).await?;
        cache.add_token(token0).await?;
        cache.add_token(token1).await?;
        cache.add_pool(pool).await?;
        let Some(database) = cache.database.as_ref() else {
            anyhow::bail!("cache database must be set");
        };
        database
            .add_pool_event_blocks_batch(chain.chain_id, &[test_block(creation_block, creation_ts)])
            .await?;

        let pools_result = cache.load_pools(&DexType::UniswapV3).await;

        cache.database = None;
        schema.cleanup().await?;

        let pools = pools_result?;
        let observed_timestamps = pools
            .first()
            .map(|pool| (pool.ts_event, pool.ts_init, pools.len()));

        let expected_timestamps = Some((creation_ts, creation_ts, 1));
        if observed_timestamps != expected_timestamps {
            anyhow::bail!(
                "unexpected pool timestamps: expected {expected_timestamps:?}, observed {observed_timestamps:?}"
            );
        }
        Ok(())
    }

    async fn connect_cache_test_database()
    -> anyhow::Result<Option<(BlockchainCacheDatabase, TestSchema)>> {
        let connect_options: PgConnectOptions =
            get_postgres_connect_options(None, None, None, None, None).into();
        let admin_pool = match PgPoolOptions::new()
            .max_connections(1)
            .connect_with(connect_options.clone())
            .await
        {
            Ok(pool) => pool,
            Err(e) => {
                eprintln!("Postgres service not available; skipping blockchain cache DB test: {e}");
                return Ok(None);
            }
        };
        let schema_name = cache_test_schema_name();

        create_cache_test_schema(&admin_pool, &schema_name).await?;
        let database = BlockchainCacheDatabase::connect(
            connect_options.options([("search_path", format!("{schema_name},public"))]),
        )
        .await?;

        Ok(Some((
            database,
            TestSchema {
                admin_pool,
                name: schema_name,
            },
        )))
    }

    struct TestSchema {
        admin_pool: PgPool,
        name: String,
    }

    impl TestSchema {
        async fn cleanup(self) -> anyhow::Result<()> {
            drop_cache_test_schema(&self.admin_pool, &self.name).await?;
            self.admin_pool.close().await;
            Ok(())
        }
    }

    #[expect(
        clippy::too_many_lines,
        reason = "test schema declares the narrow table set used by cache SQL"
    )]
    async fn create_cache_test_schema(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
        execute_schema_statement(pool, format!("CREATE SCHEMA {schema}")).await?;

        let statements = [
            format!(
                r#"
                CREATE TABLE {schema}."chain" (
                    chain_id INTEGER PRIMARY KEY,
                    name TEXT NOT NULL
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."block" (
                    chain_id INTEGER NOT NULL,
                    number BIGINT NOT NULL,
                    hash TEXT,
                    parent_hash TEXT,
                    miner TEXT,
                    gas_limit BIGINT,
                    gas_used BIGINT,
                    timestamp TEXT,
                    base_fee_per_gas TEXT,
                    blob_gas_used TEXT,
                    excess_blob_gas TEXT,
                    l1_gas_price TEXT,
                    l1_gas_used BIGINT,
                    l1_fee_scalar BIGINT,
                    PRIMARY KEY (chain_id, number)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool_event_block" (
                    chain_id INTEGER NOT NULL,
                    number BIGINT NOT NULL,
                    timestamp TEXT NOT NULL,
                    PRIMARY KEY (chain_id, number)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."token" (
                    chain_id INTEGER NOT NULL,
                    address TEXT NOT NULL,
                    symbol TEXT,
                    name TEXT,
                    decimals INTEGER,
                    error TEXT,
                    PRIMARY KEY (chain_id, address)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."dex" (
                    chain_id INTEGER NOT NULL,
                    name TEXT NOT NULL,
                    factory_address TEXT NOT NULL,
                    creation_block BIGINT NOT NULL,
                    last_full_sync_pools_block_number BIGINT,
                    PRIMARY KEY (chain_id, name),
                    UNIQUE (chain_id, factory_address)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool" (
                    chain_id INTEGER NOT NULL,
                    dex_name TEXT NOT NULL,
                    address TEXT NOT NULL,
                    pool_identifier TEXT NOT NULL,
                    creation_block BIGINT NOT NULL,
                    token0_chain INTEGER NOT NULL,
                    token0_address TEXT NOT NULL,
                    token1_chain INTEGER NOT NULL,
                    token1_address TEXT NOT NULL,
                    fee INTEGER,
                    tick_spacing INTEGER,
                    initial_tick INTEGER,
                    initial_sqrt_price_x96 TEXT,
                    hook_address TEXT,
                    last_full_sync_block_number BIGINT,
                    PRIMARY KEY (chain_id, dex_name, pool_identifier)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool_swap_event" (
                    chain_id INTEGER NOT NULL,
                    pool_identifier TEXT NOT NULL,
                    dex_name TEXT NOT NULL,
                    block BIGINT NOT NULL,
                    transaction_hash TEXT NOT NULL,
                    transaction_index INTEGER NOT NULL,
                    log_index INTEGER NOT NULL,
                    sender TEXT NOT NULL,
                    recipient TEXT NOT NULL,
                    sqrt_price_x96 TEXT NOT NULL,
                    liquidity TEXT NOT NULL,
                    tick INTEGER NOT NULL,
                    amount0 TEXT NOT NULL,
                    amount1 TEXT NOT NULL,
                    order_side TEXT,
                    base_quantity NUMERIC,
                    quote_quantity NUMERIC,
                    spot_price NUMERIC,
                    execution_price NUMERIC,
                    UNIQUE(chain_id, transaction_hash, log_index)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool_liquidity_event" (
                    chain_id INTEGER NOT NULL,
                    pool_identifier TEXT NOT NULL,
                    dex_name TEXT NOT NULL,
                    block BIGINT NOT NULL,
                    transaction_hash TEXT NOT NULL,
                    transaction_index INTEGER NOT NULL,
                    log_index INTEGER NOT NULL,
                    event_type TEXT NOT NULL,
                    sender TEXT,
                    owner TEXT NOT NULL,
                    position_liquidity TEXT NOT NULL,
                    amount0 TEXT NOT NULL,
                    amount1 TEXT NOT NULL,
                    tick_lower INTEGER NOT NULL,
                    tick_upper INTEGER NOT NULL,
                    UNIQUE(chain_id, transaction_hash, log_index)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool_collect_event" (
                    chain_id INTEGER NOT NULL,
                    pool_identifier TEXT NOT NULL,
                    dex_name TEXT NOT NULL,
                    block BIGINT NOT NULL,
                    transaction_hash TEXT NOT NULL,
                    transaction_index INTEGER NOT NULL,
                    log_index INTEGER NOT NULL,
                    owner TEXT NOT NULL,
                    amount0 TEXT NOT NULL,
                    amount1 TEXT NOT NULL,
                    tick_lower INTEGER NOT NULL,
                    tick_upper INTEGER NOT NULL,
                    UNIQUE(chain_id, transaction_hash, log_index)
                )
                "#
            ),
            format!(
                r#"
                CREATE TABLE {schema}."pool_flash_event" (
                    chain_id INTEGER NOT NULL,
                    pool_identifier TEXT NOT NULL,
                    dex_name TEXT NOT NULL,
                    block BIGINT NOT NULL,
                    transaction_hash TEXT NOT NULL,
                    transaction_index INTEGER NOT NULL,
                    log_index INTEGER NOT NULL,
                    sender TEXT NOT NULL,
                    recipient TEXT NOT NULL,
                    amount0 TEXT NOT NULL,
                    amount1 TEXT NOT NULL,
                    paid0 TEXT NOT NULL,
                    paid1 TEXT NOT NULL,
                    UNIQUE(chain_id, transaction_hash, log_index)
                )
                "#
            ),
        ];

        for statement in statements {
            execute_schema_statement(pool, statement).await?;
        }

        Ok(())
    }

    async fn insert_pool_swap_event(
        pool: &PgPool,
        schema: &str,
        chain_id: u32,
        pool_identifier: &PoolIdentifier,
        block: u64,
    ) -> anyhow::Result<()> {
        sqlx::query(AssertSqlSafe(format!(
            r#"
            INSERT INTO {schema}."pool_swap_event" (
                chain_id, pool_identifier, dex_name, block, transaction_hash, transaction_index,
                log_index, sender, recipient, sqrt_price_x96, liquidity, tick, amount0, amount1
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            "#
        )))
        .bind(chain_id as i32)
        .bind(pool_identifier.to_string())
        .bind(DexType::UniswapV3.to_string())
        .bind(block as i64)
        .bind("0x000000000000000000000000000000000000000000000000000000000000000c")
        .bind(0_i32)
        .bind(0_i32)
        .bind("0x1111111111111111111111111111111111111111")
        .bind("0x2222222222222222222222222222222222222222")
        .bind("79228162514264337593543950336")
        .bind("1000000")
        .bind(0_i32)
        .bind("-1000000000000000000")
        .bind("2000000")
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn insert_block_without_timestamp(
        pool: &PgPool,
        schema: &str,
        chain_id: u32,
        number: u64,
    ) -> anyhow::Result<()> {
        sqlx::query(AssertSqlSafe(format!(
            r#"
            INSERT INTO {schema}."block" (
                chain_id, number, hash, parent_hash, miner, gas_limit, gas_used, timestamp
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, NULL)
            "#
        )))
        .bind(chain_id as i32)
        .bind(number as i64)
        .bind(format!("0x{number:064x}"))
        .bind("0x0")
        .bind("0x0000000000000000000000000000000000000000")
        .bind(30_000_000_i64)
        .bind(21_000_i64)
        .execute(pool)
        .await?;
        Ok(())
    }

    async fn drop_cache_test_schema(pool: &PgPool, schema: &str) -> anyhow::Result<()> {
        execute_schema_statement(pool, format!("DROP SCHEMA IF EXISTS {schema} CASCADE")).await
    }

    async fn execute_schema_statement(pool: &PgPool, statement: String) -> anyhow::Result<()> {
        sqlx::query(AssertSqlSafe(statement)).execute(pool).await?;
        Ok(())
    }

    fn cache_test_schema_name() -> String {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system clock must be after UNIX epoch")
            .as_nanos();

        format!("nt_blockchain_cache_test_{}_{}", std::process::id(), nanos)
    }

    fn arbitrum() -> SharedChain {
        let Some(chain) = Chain::from_chain_id(42161) else {
            panic!("Arbitrum chain must exist in model definitions");
        };

        Arc::new(chain.clone())
    }

    fn uniswap_v3(chain: &SharedChain) -> SharedDex {
        Arc::new(Dex::new(
            (**chain).clone(),
            DexType::UniswapV3,
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            0,
            AmmType::CLAMM,
            "PoolCreated",
            "Swap",
            "Mint",
            "Burn",
            "Collect",
        ))
    }

    fn weth(chain: &SharedChain) -> Token {
        Token::new(
            chain.clone(),
            address!("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        )
    }

    fn usdc(chain: &SharedChain) -> Token {
        Token::new(
            chain.clone(),
            address!("0xFF970A61A04b1cA14834A43f5dE4533eBDDB5CC8"),
            "USD Coin".to_string(),
            "USDC".to_string(),
            6,
        )
    }

    fn test_block(number: u64, timestamp: UnixNanos) -> Block {
        Block::new(
            format!("0x{number:064x}"),
            String::from("0x0"),
            number,
            Ustr::from("0x0000000000000000000000000000000000000000"),
            30_000_000,
            21_000,
            timestamp,
            Some(Blockchain::Arbitrum),
        )
    }
}
