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
    Block, DexType, Pool, PoolLiquidityUpdate, PoolSwap, SharedChain, SharedDex, SharedPool, Token,
};
use sqlx::postgres::PgConnectOptions;

use crate::cache::{consistency::CachedBlocksConsistencyStatus, database::BlockchainCacheDatabase};

pub mod consistency;
pub mod copy;
pub mod database;
pub mod rows;

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
    /// Map of pool addresses to their corresponding `Pool` objects.
    pools: HashMap<Address, SharedPool>,
    /// Optional database connection for persistent storage.
    database: Option<BlockchainCacheDatabase>,
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
            .map_err(|e| tracing::error!("Error getting block consistency status: {e}"))
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
            tracing::warn!("Database not initialized, skipping performance settings toggle");
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
                tracing::error!(
                    "Error seeding chain in database: {e}. Continuing without database cache functionality"
                );
                return;
            }
            tracing::info!("Chain seeded in the database");

            match database.create_block_partition(&self.chain).await {
                Ok(message) => tracing::info!("Executing block partition creation: {}", message),
                Err(e) => tracing::error!(
                    "Error creating block partition for chain {}: {e}. Continuing without partition creation...",
                    self.chain.chain_id
                ),
            }

            match database.create_token_partition(&self.chain).await {
                Ok(message) => tracing::info!("Executing token partition creation: {}", message),
                Err(e) => tracing::error!(
                    "Error creating token partition for chain {}: {e}. Continuing without partition creation...",
                    self.chain.chain_id
                ),
            }
        }

        if let Err(e) = self.load_tokens().await {
            tracing::error!("Error loading tokens from the database: {e}");
        }
    }

    /// Connects to the database and loads initial data.
    ///
    /// # Errors
    ///
    /// Returns an error if database seeding, token loading, or block loading fails.
    pub async fn connect(&mut self, from_block: u64) -> anyhow::Result<()> {
        tracing::debug!("Connecting and loading from_block {from_block}");

        if let Err(e) = self.load_tokens().await {
            tracing::error!("Error loading tokens from the database: {e}");
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

            tracing::info!(
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
    /// # Errors
    ///
    /// Returns an error if the DEX has not been registered or if database operations fail.
    pub async fn load_pools(&mut self, dex_id: &DexType) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            let dex = self
                .get_dex(dex_id)
                .ok_or_else(|| anyhow::anyhow!("DEX {:?} has not been registered", dex_id))?;
            let pool_rows = database
                .load_pools(self.chain.clone(), &dex_id.to_string())
                .await?;
            tracing::info!(
                "Loading {} pools for DEX {} from cache database",
                pool_rows.len(),
                dex_id,
            );

            for pool_row in pool_rows {
                let token0 = if let Some(token) = self.tokens.get(&pool_row.token0_address) {
                    token
                } else {
                    tracing::error!(
                        "Failed to load pool {} for DEX {}: Token0 with address {} not found in cache. \
                             This may indicate the token was not properly loaded from the database or the pool references an unknown token.",
                        pool_row.address,
                        dex_id,
                        pool_row.token0_address
                    );
                    continue;
                };

                let token1 = if let Some(token) = self.tokens.get(&pool_row.token1_address) {
                    token
                } else {
                    tracing::error!(
                        "Failed to load pool {} for DEX {}: Token1 with address {} not found in cache. \
                             This may indicate the token was not properly loaded from the database or the pool references an unknown token.",
                        pool_row.address,
                        dex_id,
                        pool_row.token1_address
                    );
                    continue;
                };

                // Construct pool from row data and cached tokens
                let pool = Pool::new(
                    self.chain.clone(),
                    dex.clone(),
                    pool_row.address,
                    pool_row.creation_block as u64,
                    token0.clone(),
                    token1.clone(),
                    pool_row.fee.map(|fee| fee as u32),
                    pool_row
                        .tick_spacing
                        .map(|tick_spacing| tick_spacing as u32),
                    UnixNanos::default(), // TODO use default for now
                );

                // Add pool to cache
                self.pools.insert(pool.address, Arc::new(pool));
            }
        }
        Ok(())
    }

    /// Loads block timestamps from the database starting `from_block` number
    /// into the in-memory cache.
    #[allow(dead_code)] // TODO: Under development
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
                tracing::info!("No blocks found in database");
                return Ok(());
            }

            tracing::info!(
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
        if let Some(database) = &self.database {
            database.add_block(self.chain.chain_id, &block).await?;
        }
        self.block_timestamps.insert(block.number, block.timestamp);
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

    /// Adds a DEX to the cache with the specified identifier.
    ///
    /// # Errors
    ///
    /// Returns an error if adding the DEX to the database fails.
    pub async fn add_dex(&mut self, dex: SharedDex) -> anyhow::Result<()> {
        tracing::info!("Adding dex {} to the cache", dex.name);

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
        let pool_address = pool.address;
        if let Some(database) = &self.database {
            database.add_pool(&pool).await?;
        }

        self.pools.insert(pool_address, Arc::new(pool));
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
            database.add_pools_batch(&pools).await?;
        }

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
    pub fn get_pool(&self, address: &Address) -> Option<&SharedPool> {
        self.pools.get(address)
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
        dex_name: &str,
        block_number: u64,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .update_dex_last_synced_block(self.chain.chain_id, dex_name, block_number)
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
    pub async fn get_dex_last_synced_block(&self, dex_name: &str) -> anyhow::Result<Option<u64>> {
        if let Some(database) = &self.database {
            database
                .get_dex_last_synced_block(self.chain.chain_id, dex_name)
                .await
        } else {
            Ok(None)
        }
    }
}
