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

use std::{
    collections::{BTreeMap, HashMap},
    sync::Arc,
};

use alloy::primitives::Address;
use nautilus_core::UnixNanos;
use nautilus_model::defi::{
    amm::{Pool, SharedPool},
    block::Block,
    chain::SharedChain,
    liquidity::PoolLiquidityUpdate,
    swap::Swap,
    token::Token,
};
use sqlx::postgres::PgConnectOptions;

use crate::{cache::database::BlockchainCacheDatabase, exchanges::extended::DexExtended};

pub mod database;
pub mod rows;

/// Provides caching functionality for various blockchain domain objects.
#[derive(Debug)]
pub struct BlockchainCache {
    /// The blockchain chain this cache is associated with.
    chain: SharedChain,
    /// Map of block numbers to their corresponding timestamp
    block_timestamps: BTreeMap<u64, UnixNanos>,
    /// Map of DEX identifiers to their corresponding extended DEX objects.
    dexes: HashMap<String, DexExtended>,
    /// Map of token addresses to their corresponding `Token` objects.
    tokens: HashMap<Address, Token>,
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
            pools: HashMap::new(),
            block_timestamps: BTreeMap::new(),
            database: None,
        }
    }

    /// Returns the highest block number currently cached, if any.
    #[must_use]
    pub fn last_cached_block_number(&self) -> Option<u64> {
        self.block_timestamps.last_key_value().map(|(k, _)| *k)
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

    /// Connects to the database and loads initial data.
    pub async fn connect(&mut self, from_block: u64) -> anyhow::Result<()> {
        // Seed target adapter chain in database
        if let Some(database) = &self.database {
            database.seed_chain(&self.chain).await?;
        }
        self.load_tokens().await?;
        if let Err(e) = self.load_blocks(from_block).await {
            log::error!("Error loading blocks from database: {e}");
        }
        Ok(())
    }

    /// Adds a block to the cache and persists it to the database if available.
    pub async fn add_block(&mut self, block: Block) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_block(self.chain.chain_id, &block).await?;
        }
        self.block_timestamps.insert(block.number, block.timestamp);
        Ok(())
    }

    /// Adds a DEX to the cache with the specified identifier.
    pub async fn add_dex(&mut self, dex_id: String, dex: DexExtended) -> anyhow::Result<()> {
        log::info!("Adding dex {dex_id} to the cache");
        if let Some(database) = &self.database {
            database.add_dex(&dex).await?;
        }
        self.dexes.insert(dex_id, dex);
        Ok(())
    }

    /// Adds a liquidity pool/pair to the cache.
    pub async fn add_pool(&mut self, pool: Pool) -> anyhow::Result<()> {
        let pool_address = pool.address;
        log::info!("Adding dex pool {pool_address} to the cache");
        if let Some(database) = &self.database {
            database.add_pool(&pool).await?;
        }
        self.pools.insert(pool_address, Arc::new(pool));
        Ok(())
    }

    /// Adds a token to the cache.
    pub async fn add_token(&mut self, token: Token) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_token(&token).await?;
        }
        self.tokens.insert(token.address, token);
        Ok(())
    }

    /// Loads tokens from the database into the in-memory cache.
    async fn load_tokens(&mut self) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            let tokens = database.load_tokens(self.chain.clone()).await?;
            log::info!("Loading {} tokens from cache database", tokens.len());
            for token in tokens {
                self.tokens.insert(token.address, token);
            }
        }
        Ok(())
    }

    /// Loads block timestamps from the database starting from the specified block number.
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

            log::info!(
                "Loading {} blocks timestamps from the cache database",
                block_timestamps.len()
            );
            for block in block_timestamps {
                self.block_timestamps.insert(block.number, block.timestamp);
            }
        }
        Ok(())
    }

    /// Adds a [`Swap`] to the cache database if available.
    pub async fn add_swap(&self, swap: Swap) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database.add_swap(self.chain.chain_id, &swap).await?;
        }

        Ok(())
    }

    /// Adds a [`PoolLiquidityUpdate`] to the cache database if available.
    pub async fn add_pool_liquidity_update(
        &self,
        liquidity_update: PoolLiquidityUpdate,
    ) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            database
                .add_pool_liquidity_update(self.chain.chain_id, &liquidity_update)
                .await?;
        }

        Ok(())
    }

    /// Returns a reference to the `DexExtended` associated with the given name.
    #[must_use]
    pub fn get_dex(&self, name: &str) -> Option<&DexExtended> {
        self.dexes.get(name)
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
}
