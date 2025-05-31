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

use std::collections::HashMap;

use nautilus_model::defi::{amm::Pool, chain::SharedChain, token::Token};
use sqlx::postgres::PgConnectOptions;

use crate::{cache::database::BlockchainCacheDatabase, exchanges::extended::DexExtended};

pub mod database;
pub mod rows;

/// Provides caching functionality for various blockchain domain objects.
#[derive(Debug)]
pub struct BlockchainCache {
    /// The blockchain chain this cache is associated with.
    chain: SharedChain,
    /// Map of DEX identifiers to their corresponding extended DEX objects.
    dexes: HashMap<String, DexExtended>,
    /// Map of token addresses to their corresponding `Token` objects.
    tokens: HashMap<String, Token>,
    /// Map of pool addresses to their corresponding `Pool` objects.
    pools: HashMap<String, Pool>,
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
            database: None,
        }
    }

    /// Initializes the database connection for persistent storage.
    pub async fn initialize_database(&mut self, pg_connect_options: PgConnectOptions) {
        let database = BlockchainCacheDatabase::init(pg_connect_options).await;
        self.database = Some(database);
    }

    /// Connects to the database and loads initial data.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        // Seed target adapter chain in database
        if let Some(database) = &self.database {
            database.seed_chain(&self.chain).await?;
        }
        self.load_tokens().await?;
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
        let pool_address = pool.address.clone();
        log::info!("Adding dex pool {} to the cache", pool_address.as_str());
        if let Some(database) = &self.database {
            database.add_pool(&pool).await?;
        }
        self.pools.insert(pool_address, pool);
        Ok(())
    }

    /// Adds a token to the cache.
    pub async fn add_token(&mut self, token: Token) -> anyhow::Result<()> {
        let token_address = token.address.clone();
        if let Some(database) = &self.database {
            database.add_token(&token).await?;
        }
        self.tokens.insert(token_address, token);
        Ok(())
    }

    /// Loads tokens from the database into the in-memory cache.
    async fn load_tokens(&mut self) -> anyhow::Result<()> {
        if let Some(database) = &self.database {
            let tokens = database.load_tokens(self.chain.clone()).await?;
            log::info!("Loading {} tokens from cache database", tokens.len());
            for token in tokens {
                self.tokens.insert(token.address.clone(), token);
            }
        }
        Ok(())
    }

    /// Returns a reference to the `DexExtended` associated with the given name.
    #[must_use]
    pub fn get_dex(&self, name: &str) -> Option<&DexExtended> {
        self.dexes.get(name)
    }

    /// Returns a reference to the `Token` associated with the given address.
    #[must_use]
    pub fn get_token(&self, address: &str) -> Option<&Token> {
        self.tokens.get(address)
    }
}
