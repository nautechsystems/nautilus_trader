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

use alloy::primitives::{Address, U256};
use nautilus_model::defi::{
    Block, Chain, Pool, PoolLiquidityUpdate, PoolSwap, SharedChain, SharedDex, Token,
    validation::validate_address,
};
use sqlx::{PgPool, postgres::PgConnectOptions};

use crate::cache::{
    consistency::CachedBlocksConsistencyStatus,
    copy::PostgresCopyHandler,
    rows::{BlockTimestampRow, PoolRow, TokenRow},
};

/// Database interface for persisting and retrieving blockchain entities and domain objects.
#[derive(Debug)]
pub struct BlockchainCacheDatabase {
    /// PostgreSQL connection pool used for database operations.
    pool: PgPool,
}

impl BlockchainCacheDatabase {
    /// Initializes a new database instance by establishing a connection to PostgreSQL.
    ///
    /// # Panics
    ///
    /// Panics if unable to connect to PostgreSQL with the provided options.
    pub async fn init(pg_options: PgConnectOptions) -> Self {
        let pool = sqlx::postgres::PgPoolOptions::new()
            .max_connections(32) // Increased from default 10
            .min_connections(5) // Keep some connections warm
            .acquire_timeout(std::time::Duration::from_secs(3))
            .connect_with(pg_options)
            .await
            .expect("Error connecting to Postgres");
        Self { pool }
    }

    /// Seeds the database with a blockchain chain record.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn seed_chain(&self, chain: &Chain) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO chain (
                chain_id, name
            ) VALUES ($1,$2)
            ON CONFLICT (chain_id)
            DO NOTHING
        ",
        )
        .bind(chain.chain_id as i32)
        .bind(chain.name.to_string())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to seed chain table: {e}"))
    }

    /// Creates a table partition for the block table specific to the given chain
    /// by calling the existing PostgreSQL function create_block_partition.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn create_block_partition(&self, chain: &Chain) -> anyhow::Result<String> {
        let result: (String,) = sqlx::query_as("SELECT create_block_partition($1)")
            .bind(chain.chain_id as i32)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to call create_block_partition for chain {}: {e}",
                    chain.chain_id
                )
            })?;

        Ok(result.0)
    }

    /// Creates a table partition for the token table specific to the given chain
    /// by calling the existing PostgreSQL function create_token_partition.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn create_token_partition(&self, chain: &Chain) -> anyhow::Result<String> {
        let result: (String,) = sqlx::query_as("SELECT create_token_partition($1)")
            .bind(chain.chain_id as i32)
            .fetch_one(&self.pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!(
                    "Failed to call create_token_partition for chain {}: {e}",
                    chain.chain_id
                )
            })?;

        Ok(result.0)
    }

    /// Returns the highest block number that maintains data continuity in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_block_consistency_status(
        &self,
        chain: &Chain,
    ) -> anyhow::Result<CachedBlocksConsistencyStatus> {
        tracing::info!("Fetching block consistency status");

        let result: (i64, i64) = sqlx::query_as(
            r#"
            SELECT
                COALESCE((SELECT number FROM block WHERE chain_id = $1 ORDER BY number DESC LIMIT 1), 0) as max_block,
                get_last_continuous_block($1) as last_continuous_block
            "#
        )
        .bind(chain.chain_id as i32)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| {
            anyhow::anyhow!(
                "Failed to get block info for chain {}: {}",
                chain.chain_id,
                e
            )
        })?;

        Ok(CachedBlocksConsistencyStatus::new(
            result.0 as u64,
            result.1 as u64,
        ))
    }

    /// Inserts or updates a block record in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_block(&self, chain_id: u32, block: &Block) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO block (
                chain_id, number, hash, parent_hash, miner, gas_limit, gas_used, timestamp,
                base_fee_per_gas, blob_gas_used, excess_blob_gas,
                l1_gas_price, l1_gas_used, l1_fee_scalar
            ) VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14
            )
            ON CONFLICT (chain_id, number)
            DO UPDATE
            SET
                hash = $3,
                parent_hash = $4,
                miner = $5,
                gas_limit = $6,
                gas_used = $7,
                timestamp = $8,
                base_fee_per_gas = $9,
                blob_gas_used = $10,
                excess_blob_gas = $11,
                l1_gas_price = $12,
                l1_gas_used = $13,
                l1_fee_scalar = $14
        ",
        )
        .bind(chain_id as i32)
        .bind(block.number as i64)
        .bind(block.hash.as_str())
        .bind(block.parent_hash.as_str())
        .bind(block.miner.as_str())
        .bind(block.gas_limit as i64)
        .bind(block.gas_used as i64)
        .bind(block.timestamp.to_string())
        .bind(block.base_fee_per_gas.as_ref().map(U256::to_string))
        .bind(block.blob_gas_used.as_ref().map(U256::to_string))
        .bind(block.excess_blob_gas.as_ref().map(U256::to_string))
        .bind(block.l1_gas_price.as_ref().map(U256::to_string))
        .bind(block.l1_gas_used.map(|v| v as i64))
        .bind(block.l1_fee_scalar.map(|v| v as i64))
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into block table: {e}"))
    }

    /// Inserts multiple blocks in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_blocks_batch(&self, chain_id: u32, blocks: &[Block]) -> anyhow::Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let mut numbers: Vec<i64> = Vec::with_capacity(blocks.len());
        let mut hashes: Vec<String> = Vec::with_capacity(blocks.len());
        let mut parent_hashes: Vec<String> = Vec::with_capacity(blocks.len());
        let mut miners: Vec<String> = Vec::with_capacity(blocks.len());
        let mut gas_limits: Vec<i64> = Vec::with_capacity(blocks.len());
        let mut gas_useds: Vec<i64> = Vec::with_capacity(blocks.len());
        let mut timestamps: Vec<String> = Vec::with_capacity(blocks.len());
        let mut base_fee_per_gases: Vec<Option<String>> = Vec::with_capacity(blocks.len());
        let mut blob_gas_useds: Vec<Option<String>> = Vec::with_capacity(blocks.len());
        let mut excess_blob_gases: Vec<Option<String>> = Vec::with_capacity(blocks.len());
        let mut l1_gas_prices: Vec<Option<String>> = Vec::with_capacity(blocks.len());
        let mut l1_gas_useds: Vec<Option<i64>> = Vec::with_capacity(blocks.len());
        let mut l1_fee_scalars: Vec<Option<i64>> = Vec::with_capacity(blocks.len());

        // Fill vectors from blocks
        for block in blocks {
            numbers.push(block.number as i64);
            hashes.push(block.hash.clone());
            parent_hashes.push(block.parent_hash.clone());
            miners.push(block.miner.to_string());
            gas_limits.push(block.gas_limit as i64);
            gas_useds.push(block.gas_used as i64);
            timestamps.push(block.timestamp.to_string());
            base_fee_per_gases.push(block.base_fee_per_gas.as_ref().map(U256::to_string));
            blob_gas_useds.push(block.blob_gas_used.as_ref().map(U256::to_string));
            excess_blob_gases.push(block.excess_blob_gas.as_ref().map(U256::to_string));
            l1_gas_prices.push(block.l1_gas_price.as_ref().map(U256::to_string));
            l1_gas_useds.push(block.l1_gas_used.map(|v| v as i64));
            l1_fee_scalars.push(block.l1_fee_scalar.map(|v| v as i64));
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO block (
                chain_id, number, hash, parent_hash, miner, gas_limit, gas_used, timestamp,
                base_fee_per_gas, blob_gas_used, excess_blob_gas,
                l1_gas_price, l1_gas_used, l1_fee_scalar
            )
            SELECT
                $1, *
            FROM UNNEST(
                $2::int8[], $3::text[], $4::text[], $5::text[],
                $6::int8[], $7::int8[], $8::text[],
                $9::text[], $10::text[], $11::text[],
                $12::text[], $13::int8[], $14::int8[]
            )
            ON CONFLICT (chain_id, number) DO NOTHING
           ",
        )
        .bind(chain_id as i32)
        .bind(&numbers[..])
        .bind(&hashes[..])
        .bind(&parent_hashes[..])
        .bind(&miners[..])
        .bind(&gas_limits[..])
        .bind(&gas_useds[..])
        .bind(&timestamps[..])
        .bind(&base_fee_per_gases as &[Option<String>])
        .bind(&blob_gas_useds as &[Option<String>])
        .bind(&excess_blob_gases as &[Option<String>])
        .bind(&l1_gas_prices as &[Option<String>])
        .bind(&l1_gas_useds as &[Option<i64>])
        .bind(&l1_fee_scalars as &[Option<i64>])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into block table: {e}"))
    }

    /// Inserts blocks using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// This method is significantly faster than INSERT for bulk operations as it bypasses
    /// SQL parsing and uses PostgreSQL's native binary protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn add_blocks_copy(&self, chain_id: u32, blocks: &[Block]) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler.copy_blocks(chain_id, blocks).await
    }

    /// Retrieves block timestamps for a given chain starting from a specific block number.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn load_block_timestamps(
        &self,
        chain: SharedChain,
        from_block: u64,
    ) -> anyhow::Result<Vec<BlockTimestampRow>> {
        sqlx::query_as::<_, BlockTimestampRow>(
            r"
            SELECT
                number,
                timestamp
            FROM block
            WHERE chain_id = $1 AND number >= $2
            ORDER BY number ASC
            ",
        )
        .bind(chain.chain_id as i32)
        .bind(from_block as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load block timestamps: {e}"))
    }

    /// Adds or updates a DEX (Decentralized Exchange) record in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_dex(&self, dex: SharedDex) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO dex (
                chain_id, name, factory_address, creation_block
            ) VALUES ($1, $2, $3, $4)
            ON CONFLICT (chain_id, name)
            DO UPDATE
            SET
                factory_address = $3,
                creation_block = $4
        ",
        )
        .bind(dex.chain.chain_id as i32)
        .bind(dex.name.to_string())
        .bind(dex.factory.as_ref())
        .bind(dex.factory_creation_block as i64)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into dex table: {e}"))
    }

    /// Adds or updates a liquidity pool/pair record in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool(&self, pool: &Pool) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO pool (
                chain_id, address, dex_name, creation_block,
                token0_chain, token0_address,
                token1_chain, token1_address,
                fee, tick_spacing
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (chain_id, address)
            DO UPDATE
            SET
                dex_name = $3,
                creation_block = $4,
                token0_chain = $5,
                token0_address = $6,
                token1_chain = $7,
                token1_address = $8,
                fee = $9,
                tick_spacing = $10
        ",
        )
        .bind(pool.chain.chain_id as i32)
        .bind(pool.address.to_string())
        .bind(pool.dex.name.to_string())
        .bind(pool.creation_block as i64)
        .bind(pool.token0.chain.chain_id as i32)
        .bind(pool.token0.address.to_string())
        .bind(pool.token1.chain.chain_id as i32)
        .bind(pool.token1.address.to_string())
        .bind(pool.fee as i32)
        .bind(pool.tick_spacing as i32)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into pool table: {e}"))
    }

    /// Inserts multiple pools in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pools_batch(&self, pools: &[Pool]) -> anyhow::Result<()> {
        if pools.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let mut addresses: Vec<String> = Vec::with_capacity(pools.len());
        let mut dex_names: Vec<String> = Vec::with_capacity(pools.len());
        let mut creation_blocks: Vec<i64> = Vec::with_capacity(pools.len());
        let mut token0_chains: Vec<i32> = Vec::with_capacity(pools.len());
        let mut token0_addresses: Vec<String> = Vec::with_capacity(pools.len());
        let mut token1_chains: Vec<i32> = Vec::with_capacity(pools.len());
        let mut token1_addresses: Vec<String> = Vec::with_capacity(pools.len());
        let mut fees: Vec<i32> = Vec::with_capacity(pools.len());
        let mut tick_spacings: Vec<i32> = Vec::with_capacity(pools.len());
        let mut chain_ids: Vec<i32> = Vec::with_capacity(pools.len());

        // Fill vectors from pools
        for pool in pools {
            chain_ids.push(pool.chain.chain_id as i32);
            addresses.push(pool.address.to_string());
            dex_names.push(pool.dex.name.to_string());
            creation_blocks.push(pool.creation_block as i64);
            token0_chains.push(pool.token0.chain.chain_id as i32);
            token0_addresses.push(pool.token0.address.to_string());
            token1_chains.push(pool.token1.chain.chain_id as i32);
            token1_addresses.push(pool.token1.address.to_string());
            fees.push(pool.fee as i32);
            tick_spacings.push(pool.tick_spacing as i32);
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool (
                chain_id, address, dex_name, creation_block,
                token0_chain, token0_address,
                token1_chain, token1_address,
                fee, tick_spacing
            )
            SELECT *
            FROM UNNEST(
                $1::int4[], $2::text[], $3::text[], $4::int8[],
                $5::int4[], $6::text[], $7::int4[], $8::text[],
                $9::int4[], $10::int4[]
            )
            ON CONFLICT (chain_id, address) DO NOTHING
           ",
        )
        .bind(&chain_ids[..])
        .bind(&addresses[..])
        .bind(&dex_names[..])
        .bind(&creation_blocks[..])
        .bind(&token0_chains[..])
        .bind(&token0_addresses[..])
        .bind(&token1_chains[..])
        .bind(&token1_addresses[..])
        .bind(&fees[..])
        .bind(&tick_spacings[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool table: {e}"))
    }

    /// Adds or updates a token record in the database.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_token(&self, token: &Token) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO token (
                chain_id, address, name, symbol, decimals
            ) VALUES ($1, $2, $3, $4, $5)
            ON CONFLICT (chain_id, address)
            DO UPDATE
            SET
                name = $3,
                symbol = $4,
                decimals = $5
        ",
        )
        .bind(token.chain.chain_id as i32)
        .bind(token.address.to_string())
        .bind(token.name.as_str())
        .bind(token.symbol.as_str())
        .bind(i32::from(token.decimals))
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into token table: {e}"))
    }

    /// Records an invalid token address with associated error information.
    ///
    /// # Errors
    ///
    /// Returns an error if the database insertion fails.
    pub async fn add_invalid_token(
        &self,
        chain_id: u32,
        address: &Address,
        error_string: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO token (
                chain_id, address, error
            ) VALUES ($1, $2, $3)
            ON CONFLICT (chain_id, address)
            DO NOTHING;
        ",
        )
        .bind(chain_id as i32)
        .bind(address.to_string())
        .bind(error_string)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into token table: {e}"))
    }

    /// Persists a token swap transaction event to the `pool_swap` table.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_swap(&self, chain_id: u32, swap: &PoolSwap) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO pool_swap (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, side, size, price
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (chain_id, transaction_hash, log_index)
            DO NOTHING
        ",
        )
        .bind(chain_id as i32)
        .bind(swap.pool_address.to_string())
        .bind(swap.block as i64)
        .bind(swap.transaction_hash.as_str())
        .bind(swap.transaction_index as i32)
        .bind(swap.log_index as i32)
        .bind(swap.sender.to_string())
        .bind(swap.side.to_string())
        .bind(swap.size.to_string())
        .bind(swap.price.to_string())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into pool_swap table: {e}"))
    }

    /// Persists a liquidity position change (mint/burn) event to the `pool_liquidity` table.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_liquidity_update(
        &self,
        chain_id: u32,
        liquidity_update: &PoolLiquidityUpdate,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO pool_liquidity (
                chain_id, pool_address, block, transaction_hash, transaction_index, log_index,
                event_type, sender, owner, position_liquidity, amount0, amount1, tick_lower, tick_upper
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
            ON CONFLICT (chain_id, transaction_hash, log_index)
            DO NOTHING
        ",
        )
        .bind(chain_id as i32)
        .bind(liquidity_update.pool_address.to_string())
        .bind(liquidity_update.block as i64)
        .bind(liquidity_update.transaction_hash.as_str())
        .bind(liquidity_update.transaction_index as i32)
        .bind(liquidity_update.log_index as i32)
        .bind(liquidity_update.kind.to_string())
        .bind(liquidity_update.sender.map(|sender| sender.to_string()))
        .bind(liquidity_update.owner.to_string())
        .bind(liquidity_update.position_liquidity.to_string())
        .bind(liquidity_update.amount0.to_string())
        .bind(liquidity_update.amount1.to_string())
        .bind(liquidity_update.tick_lower)
        .bind(liquidity_update.tick_upper)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into pool_liquidity table: {e}"))
    }

    /// Retrieves all valid token records for the given chain and converts them into `Token` domain objects.
    ///
    /// Only returns tokens that do not contain error information, filtering out invalid tokens
    /// that were previously recorded with error details.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn load_tokens(&self, chain: SharedChain) -> anyhow::Result<Vec<Token>> {
        sqlx::query_as::<_, TokenRow>("SELECT * FROM token WHERE chain_id = $1 AND error IS NULL")
            .bind(chain.chain_id as i32)
            .fetch_all(&self.pool)
            .await
            .map(|rows| {
                rows.into_iter()
                    .map(|token_row| {
                        Token::new(
                            chain.clone(),
                            token_row.address,
                            token_row.name,
                            token_row.symbol,
                            token_row.decimals as u8,
                        )
                    })
                    .collect::<Vec<_>>()
            })
            .map_err(|e| anyhow::anyhow!("Failed to load tokens: {e}"))
    }

    /// Retrieves all invalid token addresses for a given chain.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or address validation fails.
    pub async fn load_invalid_token_addresses(
        &self,
        chain_id: u32,
    ) -> anyhow::Result<Vec<Address>> {
        sqlx::query_as::<_, (String,)>(
            "SELECT address FROM token WHERE chain_id = $1 AND error IS NOT NULL",
        )
        .bind(chain_id as i32)
        .fetch_all(&self.pool)
        .await?
        .into_iter()
        .map(|(address,)| validate_address(&address))
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| anyhow::anyhow!("Failed to load invalid token addresses: {e}"))
    }

    /// Loads pool data from the database for the specified chain and DEX.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails, the connection to the database is lost, or the query parameters are invalid.
    pub async fn load_pools(
        &self,
        chain: SharedChain,
        dex_id: &str,
    ) -> anyhow::Result<Vec<PoolRow>> {
        sqlx::query_as::<_, PoolRow>(
            r"
            SELECT
                address,
                dex_name,
                creation_block,
                token0_chain,
                token0_address,
                token1_chain,
                token1_address,
                fee,
                tick_spacing
            FROM pool
            WHERE chain_id = $1 AND dex_name = $2
            ORDER BY creation_block ASC
        ",
        )
        .bind(chain.chain_id as i32)
        .bind(dex_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load pools: {e}"))
    }

    /// Toggles performance optimization settings for sync operations.
    ///
    /// When enabled (true), applies settings for maximum write performance:
    /// - synchronous_commit = OFF
    /// - work_mem increased for bulk operations
    ///
    /// When disabled (false), restores default safe settings:
    /// - synchronous_commit = ON (data safety)
    /// - work_mem back to default
    ///
    /// # Errors
    ///
    /// Returns an error if the database operations fail.
    pub async fn toggle_perf_sync_settings(&self, enable: bool) -> anyhow::Result<()> {
        if enable {
            tracing::info!("Enabling performance sync settings for bulk operations");

            // Set synchronous_commit to OFF for maximum write performance
            sqlx::query("SET synchronous_commit = OFF")
                .execute(&self.pool)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set synchronous_commit OFF: {e}"))?;

            // Increase work_mem for bulk operations
            sqlx::query("SET work_mem = '256MB'")
                .execute(&self.pool)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set work_mem: {e}"))?;

            tracing::debug!("Performance settings enabled: synchronous_commit=OFF, work_mem=256MB");
        } else {
            tracing::info!("Restoring default safe database performance settings");

            // Restore synchronous_commit to ON for data safety
            sqlx::query("SET synchronous_commit = ON")
                .execute(&self.pool)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to set synchronous_commit ON: {e}"))?;

            // Reset work_mem to default
            sqlx::query("RESET work_mem")
                .execute(&self.pool)
                .await
                .map_err(|e| anyhow::anyhow!("Failed to reset work_mem: {e}"))?;
        }

        Ok(())
    }

    /// Saves the checkpoint block number indicating the last completed pool synchronization for a specific DEX.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn update_dex_last_synced_block(
        &self,
        chain_id: u32,
        dex_name: &str,
        block_number: u64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            UPDATE dex
            SET last_full_sync_pools_block_number = $3
            WHERE chain_id = $1 AND name = $2
            ",
        )
        .bind(chain_id as i32)
        .bind(dex_name)
        .bind(block_number as i64)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to update dex last synced block: {e}"))
    }

    /// Retrieves the saved checkpoint block number from the last completed pool synchronization for a specific DEX.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_dex_last_synced_block(
        &self,
        chain_id: u32,
        dex_name: &str,
    ) -> anyhow::Result<Option<u64>> {
        let result = sqlx::query_as::<_, (Option<i64>,)>(
            "SELECT last_full_sync_pools_block_number FROM dex WHERE chain_id = $1 AND name = $2",
        )
        .bind(chain_id as i32)
        .bind(dex_name)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get dex last synced block: {e}"))?;

        Ok(result.and_then(|(block_number,)| block_number.map(|b| b as u64)))
    }
}
