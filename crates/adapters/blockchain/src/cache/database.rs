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

use std::pin::Pin;

use alloy::primitives::{Address, U256};
use futures_util::{Stream, StreamExt};
use nautilus_model::{
    defi::{
        Block, Chain, DexType, Pool, PoolLiquidityUpdate, PoolSwap, SharedChain, SharedDex, Token,
        data::{DexPoolData, PoolFeeCollect, PoolFlash, block::BlockPosition},
        pool_analysis::{
            position::PoolPosition,
            snapshot::{PoolAnalytics, PoolSnapshot, PoolState},
        },
        tick_map::tick::PoolTick,
        validation::validate_address,
    },
    identifiers::InstrumentId,
};
use sqlx::{PgPool, Row, postgres::PgConnectOptions};

use crate::{
    cache::{
        consistency::CachedBlocksConsistencyStatus,
        copy::PostgresCopyHandler,
        rows::{BlockTimestampRow, PoolRow, TokenRow, transform_row_to_dex_pool_data},
        types::{U128Pg, U256Pg},
    },
    events::initialize::InitializeEvent,
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
    /// by calling the existing PostgreSQL function `create_block_partition`.
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
    /// by calling the existing PostgreSQL function `create_token_partition`.
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
            r"
            SELECT
                COALESCE((SELECT number FROM block WHERE chain_id = $1 ORDER BY number DESC LIMIT 1), 0) as max_block,
                get_last_continuous_block($1) as last_continuous_block
            "
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

    /// Inserts tokens using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn add_tokens_copy(&self, chain_id: u32, tokens: &[Token]) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler.copy_tokens(chain_id, tokens).await
    }

    /// Inserts pools using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn add_pools_copy(&self, chain_id: u32, pools: &[Pool]) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler.copy_pools(chain_id, pools).await
    }

    /// Inserts pool swaps using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// This method is significantly faster than INSERT for bulk operations as it bypasses
    /// SQL parsing and uses PostgreSQL's native binary protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn add_pool_swaps_copy(
        &self,
        chain_id: u32,
        swaps: &[PoolSwap],
    ) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler.copy_pool_swaps(chain_id, swaps).await
    }

    /// Inserts pool liquidity updates using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// This method is significantly faster than INSERT for bulk operations as it bypasses
    /// SQL parsing and uses PostgreSQL's native binary protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn add_pool_liquidity_updates_copy(
        &self,
        chain_id: u32,
        updates: &[PoolLiquidityUpdate],
    ) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler
            .copy_pool_liquidity_updates(chain_id, updates)
            .await
    }

    /// Inserts pool fee collect events using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// This method is significantly faster than INSERT for bulk operations as it bypasses
    /// SQL parsing and most database validation checks.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_pool_fee_collects_batch(
        &self,
        chain_id: u32,
        collects: &[PoolFeeCollect],
    ) -> anyhow::Result<()> {
        let copy_handler = PostgresCopyHandler::new(&self.pool);
        copy_handler.copy_pool_collects(chain_id, collects).await
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
        .bind(dex.factory.to_string())
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
                fee, tick_spacing, initial_tick, initial_sqrt_price_x96
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12)
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
                tick_spacing = $10,
                initial_tick = $11,
                initial_sqrt_price_x96 = $12
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
        .bind(pool.fee.map(|fee| fee as i32))
        .bind(pool.tick_spacing.map(|tick_spacing| tick_spacing as i32))
        .bind(pool.initial_tick)
        .bind(pool.initial_sqrt_price_x96.as_ref().map(|p| p.to_string()))
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
        let len = pools.len();
        let mut addresses: Vec<String> = Vec::with_capacity(len);
        let mut dex_names: Vec<String> = Vec::with_capacity(len);
        let mut creation_blocks: Vec<i64> = Vec::with_capacity(len);
        let mut token0_chains: Vec<i32> = Vec::with_capacity(len);
        let mut token0_addresses: Vec<String> = Vec::with_capacity(len);
        let mut token1_chains: Vec<i32> = Vec::with_capacity(len);
        let mut token1_addresses: Vec<String> = Vec::with_capacity(len);
        let mut fees: Vec<Option<i32>> = Vec::with_capacity(len);
        let mut tick_spacings: Vec<Option<i32>> = Vec::with_capacity(len);
        let mut initial_ticks: Vec<Option<i32>> = Vec::with_capacity(len);
        let mut initial_sqrt_price_x96s: Vec<Option<String>> = Vec::with_capacity(len);
        let mut chain_ids: Vec<i32> = Vec::with_capacity(len);

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
            fees.push(pool.fee.map(|fee| fee as i32));
            tick_spacings.push(pool.tick_spacing.map(|tick_spacing| tick_spacing as i32));
            initial_ticks.push(pool.initial_tick);
            initial_sqrt_price_x96s
                .push(pool.initial_sqrt_price_x96.as_ref().map(|p| p.to_string()));
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool (
                chain_id, address, dex_name, creation_block,
                token0_chain, token0_address,
                token1_chain, token1_address,
                fee, tick_spacing, initial_tick, initial_sqrt_price_x96
            )
            SELECT *
            FROM UNNEST(
                $1::int4[], $2::text[], $3::text[], $4::int8[],
                $5::int4[], $6::text[], $7::int4[], $8::text[],
                $9::int4[], $10::int4[], $11::int4[], $12::text[]
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
        .bind(&initial_ticks[..])
        .bind(&initial_sqrt_price_x96s[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool table: {e}"))
    }

    /// Inserts multiple pool swaps in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_swaps_batch(
        &self,
        chain_id: u32,
        swaps: &[PoolSwap],
    ) -> anyhow::Result<()> {
        if swaps.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = swaps.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut blocks: Vec<i64> = Vec::with_capacity(len);
        let mut transaction_hashes: Vec<String> = Vec::with_capacity(len);
        let mut transaction_indices: Vec<i32> = Vec::with_capacity(len);
        let mut log_indices: Vec<i32> = Vec::with_capacity(len);
        let mut senders: Vec<String> = Vec::with_capacity(len);
        let mut recipients: Vec<String> = Vec::with_capacity(len);
        let mut sides: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sizes: Vec<Option<String>> = Vec::with_capacity(len);
        let mut prices: Vec<Option<String>> = Vec::with_capacity(len);
        let mut sqrt_price_x96s: Vec<String> = Vec::with_capacity(len);
        let mut liquidities: Vec<String> = Vec::with_capacity(len);
        let mut ticks: Vec<i32> = Vec::with_capacity(len);
        let mut amount0s: Vec<String> = Vec::with_capacity(len);
        let mut amount1s: Vec<String> = Vec::with_capacity(len);
        let mut chain_ids: Vec<i32> = Vec::with_capacity(len);

        // Fill vectors from swaps
        for swap in swaps {
            chain_ids.push(chain_id as i32);
            pool_addresses.push(swap.pool_address.to_string());
            blocks.push(swap.block as i64);
            transaction_hashes.push(swap.transaction_hash.clone());
            transaction_indices.push(swap.transaction_index as i32);
            log_indices.push(swap.log_index as i32);
            senders.push(swap.sender.to_string());
            recipients.push(swap.recipient.to_string());
            sides.push(swap.side.map(|side| side.to_string()));
            sizes.push(swap.size.map(|size| size.to_string()));
            prices.push(swap.price.map(|price| price.to_string()));
            sqrt_price_x96s.push(swap.sqrt_price_x96.to_string());
            liquidities.push(swap.liquidity.to_string());
            ticks.push(swap.tick);
            amount0s.push(swap.amount0.to_string());
            amount1s.push(swap.amount1.to_string());
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_swap_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, recipient, side, size, price, sqrt_price_x96, liquidity, tick, amount0, amount1
            )
            SELECT
                chain_id, pool_address, block, transaction_hash, transaction_index, log_index, sender, recipient,
                side, size, price, sqrt_price_x96::U160, liquidity::U128, tick, amount0::I256, amount1::I256
            FROM UNNEST(
                $1::INT[], $2::TEXT[], $3::INT[], $4::TEXT[], $5::INT[], $6::INT[],
                $7::TEXT[], $8::TEXT[], $9::TEXT[], $10::TEXT[], $11::TEXT[],
                $12::TEXT[], $13::TEXT[], $14::INT[], $15::TEXT[], $16::TEXT[]
            ) AS t(chain_id, pool_address, block, transaction_hash, transaction_index,
                   log_index, sender, recipient, side, size, price, sqrt_price_x96, liquidity, tick, amount0, amount1)
            ON CONFLICT (chain_id, transaction_hash, log_index) DO NOTHING
           ",
        )
        .bind(&chain_ids[..])
        .bind(&pool_addresses[..])
        .bind(&blocks[..])
        .bind(&transaction_hashes[..])
        .bind(&transaction_indices[..])
        .bind(&log_indices[..])
        .bind(&senders[..])
        .bind(&recipients[..])
        .bind(&sides[..])
        .bind(&sizes[..])
        .bind(&prices[..])
        .bind(&sqrt_price_x96s[..])
        .bind(&liquidities[..])
        .bind(&ticks[..])
        .bind(&amount0s[..])
        .bind(&amount1s[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_swap_event table: {e}"))
    }

    /// Inserts multiple pool liquidity updates in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_liquidity_updates_batch(
        &self,
        chain_id: u32,
        updates: &[PoolLiquidityUpdate],
    ) -> anyhow::Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = updates.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut blocks: Vec<i64> = Vec::with_capacity(len);
        let mut transaction_hashes: Vec<String> = Vec::with_capacity(len);
        let mut transaction_indices: Vec<i32> = Vec::with_capacity(len);
        let mut log_indices: Vec<i32> = Vec::with_capacity(len);
        let mut event_types: Vec<String> = Vec::with_capacity(len);
        let mut senders: Vec<Option<String>> = Vec::with_capacity(len);
        let mut owners: Vec<String> = Vec::with_capacity(len);
        let mut position_liquidities: Vec<String> = Vec::with_capacity(len);
        let mut amount0s: Vec<String> = Vec::with_capacity(len);
        let mut amount1s: Vec<String> = Vec::with_capacity(len);
        let mut tick_lowers: Vec<i32> = Vec::with_capacity(len);
        let mut tick_uppers: Vec<i32> = Vec::with_capacity(len);
        let mut chain_ids: Vec<i32> = Vec::with_capacity(len);

        // Fill vectors from updates
        for update in updates {
            chain_ids.push(chain_id as i32);
            pool_addresses.push(update.pool_address.to_string());
            blocks.push(update.block as i64);
            transaction_hashes.push(update.transaction_hash.clone());
            transaction_indices.push(update.transaction_index as i32);
            log_indices.push(update.log_index as i32);
            event_types.push(update.kind.to_string());
            senders.push(update.sender.map(|s| s.to_string()));
            owners.push(update.owner.to_string());
            position_liquidities.push(update.position_liquidity.to_string());
            amount0s.push(update.amount0.to_string());
            amount1s.push(update.amount1.to_string());
            tick_lowers.push(update.tick_lower);
            tick_uppers.push(update.tick_upper);
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_liquidity_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, event_type, sender, owner, position_liquidity,
                amount0, amount1, tick_lower, tick_upper
            )
            SELECT
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, event_type, sender, owner, position_liquidity::u128,
                amount0::U256, amount1::U256, tick_lower, tick_upper
            FROM UNNEST(
                $1::INT[], $2::TEXT[], $3::INT[], $4::TEXT[], $5::INT[],
                $6::INT[], $7::TEXT[], $8::TEXT[], $9::TEXT[], $10::TEXT[],
                $11::TEXT[], $12::TEXT[], $13::INT[], $14::INT[]
            ) AS t(chain_id, pool_address, block, transaction_hash, transaction_index,
                   log_index, event_type, sender, owner, position_liquidity,
                   amount0, amount1, tick_lower, tick_upper)
            ON CONFLICT (chain_id, transaction_hash, log_index) DO NOTHING
           ",
        )
        .bind(&chain_ids[..])
        .bind(&pool_addresses[..])
        .bind(&blocks[..])
        .bind(&transaction_hashes[..])
        .bind(&transaction_indices[..])
        .bind(&log_indices[..])
        .bind(&event_types[..])
        .bind(&senders[..])
        .bind(&owners[..])
        .bind(&position_liquidities[..])
        .bind(&amount0s[..])
        .bind(&amount1s[..])
        .bind(&tick_lowers[..])
        .bind(&tick_uppers[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_liquidity_event table: {e}"))
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
            INSERT INTO pool_swap_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, recipient, side, size, price, sqrt_price_x96, amount0, amount1
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, $13, $14)
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
        .bind(swap.recipient.to_string())
        .bind(swap.side.map(|side| side.to_string()))
        .bind(swap.size.map(|size| size.to_string()))
        .bind(swap.price.map(|price| price.to_string()))
        .bind(swap.sqrt_price_x96.to_string())
        .bind(swap.amount0.to_string())
        .bind(swap.amount1.to_string())
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
            INSERT INTO pool_liquidity_event (
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
        .bind(U128Pg(liquidity_update.position_liquidity))
        .bind(U256Pg(liquidity_update.amount0))
        .bind(U256Pg(liquidity_update.amount1))
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
                tick_spacing,
                initial_tick,
                initial_sqrt_price_x96
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
    /// - `synchronous_commit` = OFF
    /// - `work_mem` increased for bulk operations
    ///
    /// When disabled (false), restores default safe settings:
    /// - `synchronous_commit` = ON (data safety)
    /// - `work_mem` back to default
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
        dex: &DexType,
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
        .bind(dex.to_string())
        .bind(block_number as i64)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to update dex last synced block: {e}"))
    }

    pub async fn update_pool_last_synced_block(
        &self,
        chain_id: u32,
        dex: &DexType,
        pool_address: &Address,
        block_number: u64,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            UPDATE pool
            SET last_full_sync_block_number = $4
            WHERE chain_id = $1
            AND dex_name = $2
            AND address = $3
            ",
        )
        .bind(chain_id as i32)
        .bind(dex.to_string())
        .bind(pool_address.to_string())
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
        dex: &DexType,
    ) -> anyhow::Result<Option<u64>> {
        let result = sqlx::query_as::<_, (Option<i64>,)>(
            r#"
            SELECT
                last_full_sync_pools_block_number
            FROM dex
            WHERE chain_id = $1
            AND name = $2
            "#,
        )
        .bind(chain_id as i32)
        .bind(dex.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get dex last synced block: {e}"))?;

        Ok(result.and_then(|(block_number,)| block_number.map(|b| b as u64)))
    }

    pub async fn get_pool_last_synced_block(
        &self,
        chain_id: u32,
        dex: &DexType,
        pool_address: &Address,
    ) -> anyhow::Result<Option<u64>> {
        let result = sqlx::query_as::<_, (Option<i64>,)>(
            r#"
            SELECT
                last_full_sync_block_number
            FROM pool
            WHERE chain_id = $1
            AND dex_name = $2
            AND address = $3
            "#,
        )
        .bind(chain_id as i32)
        .bind(dex.to_string())
        .bind(pool_address.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to get pool last synced block: {e}"))?;

        Ok(result.and_then(|(block_number,)| block_number.map(|b| b as u64)))
    }

    /// Retrieves the maximum block number from a specific table for a given pool.
    /// This is useful to detect orphaned data where events were inserted but progress wasn't updated.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn get_table_last_block(
        &self,
        chain_id: u32,
        table_name: &str,
        pool_address: &Address,
    ) -> anyhow::Result<Option<u64>> {
        let query = format!(
            "SELECT MAX(block) FROM {} WHERE chain_id = $1 AND pool_address = $2",
            table_name
        );
        let result = sqlx::query_as::<_, (Option<i64>,)>(query.as_str())
            .bind(chain_id as i32)
            .bind(pool_address.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to get table last block for {}: {e}", table_name)
            })?;

        Ok(result.and_then(|(block_number,)| block_number.map(|b| b as u64)))
    }

    /// Adds a batch of pool fee collect events to the database using batch operations.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_collects_batch(
        &self,
        chain_id: u32,
        collects: &[PoolFeeCollect],
    ) -> anyhow::Result<()> {
        if collects.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = collects.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut blocks: Vec<i64> = Vec::with_capacity(len);
        let mut transaction_hashes: Vec<String> = Vec::with_capacity(len);
        let mut transaction_indices: Vec<i32> = Vec::with_capacity(len);
        let mut log_indices: Vec<i32> = Vec::with_capacity(len);
        let mut owners: Vec<String> = Vec::with_capacity(len);
        let mut amount0s: Vec<String> = Vec::with_capacity(len);
        let mut amount1s: Vec<String> = Vec::with_capacity(len);
        let mut tick_lowers: Vec<i32> = Vec::with_capacity(len);
        let mut tick_uppers: Vec<i32> = Vec::with_capacity(len);
        let mut chain_ids: Vec<i32> = Vec::with_capacity(len);

        // Fill vectors from collects
        for collect in collects {
            chain_ids.push(chain_id as i32);
            pool_addresses.push(collect.pool_address.to_string());
            blocks.push(collect.block as i64);
            transaction_hashes.push(collect.transaction_hash.clone());
            transaction_indices.push(collect.transaction_index as i32);
            log_indices.push(collect.log_index as i32);
            owners.push(collect.owner.to_string());
            amount0s.push(collect.amount0.to_string());
            amount1s.push(collect.amount1.to_string());
            tick_lowers.push(collect.tick_lower);
            tick_uppers.push(collect.tick_upper);
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_collect_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, owner, amount0, amount1, tick_lower, tick_upper
            )
            SELECT
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, owner, amount0::U256, amount1::U256, tick_lower, tick_upper
            FROM UNNEST(
                $1::INT[], $2::TEXT[], $3::INT[], $4::TEXT[], $5::INT[],
                $6::INT[], $7::TEXT[], $8::TEXT[], $9::TEXT[], $10::INT[], $11::INT[]
            ) AS t(chain_id, pool_address, block, transaction_hash, transaction_index,
                   log_index, owner, amount0, amount1, tick_lower, tick_upper)
            ON CONFLICT (chain_id, transaction_hash, log_index) DO NOTHING
           ",
        )
        .bind(&chain_ids[..])
        .bind(&pool_addresses[..])
        .bind(&blocks[..])
        .bind(&transaction_hashes[..])
        .bind(&transaction_indices[..])
        .bind(&log_indices[..])
        .bind(&owners[..])
        .bind(&amount0s[..])
        .bind(&amount1s[..])
        .bind(&tick_lowers[..])
        .bind(&tick_uppers[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_fee_collect table: {e}"))
    }

    /// Inserts multiple pool flash events in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_flash_batch(
        &self,
        chain_id: u32,
        flash_events: &[PoolFlash],
    ) -> anyhow::Result<()> {
        if flash_events.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = flash_events.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut blocks: Vec<i64> = Vec::with_capacity(len);
        let mut transaction_hashes: Vec<String> = Vec::with_capacity(len);
        let mut transaction_indices: Vec<i32> = Vec::with_capacity(len);
        let mut log_indices: Vec<i32> = Vec::with_capacity(len);
        let mut senders: Vec<String> = Vec::with_capacity(len);
        let mut recipients: Vec<String> = Vec::with_capacity(len);
        let mut amount0s: Vec<String> = Vec::with_capacity(len);
        let mut amount1s: Vec<String> = Vec::with_capacity(len);
        let mut paid0s: Vec<String> = Vec::with_capacity(len);
        let mut paid1s: Vec<String> = Vec::with_capacity(len);
        let mut chain_ids: Vec<i32> = Vec::with_capacity(len);

        // Fill vectors from flash events
        for flash in flash_events {
            chain_ids.push(chain_id as i32);
            pool_addresses.push(flash.pool_address.to_string());
            blocks.push(flash.block as i64);
            transaction_hashes.push(flash.transaction_hash.clone());
            transaction_indices.push(flash.transaction_index as i32);
            log_indices.push(flash.log_index as i32);
            senders.push(flash.sender.to_string());
            recipients.push(flash.recipient.to_string());
            amount0s.push(flash.amount0.to_string());
            amount1s.push(flash.amount1.to_string());
            paid0s.push(flash.paid0.to_string());
            paid1s.push(flash.paid1.to_string());
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_flash_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, recipient, amount0, amount1, paid0, paid1
            )
            SELECT
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, recipient, amount0::U256, amount1::U256, paid0::U256, paid1::U256
            FROM UNNEST(
                $1::INT[], $2::TEXT[], $3::INT[], $4::TEXT[], $5::INT[],
                $6::INT[], $7::TEXT[], $8::TEXT[], $9::TEXT[], $10::TEXT[], $11::TEXT[], $12::TEXT[]
            ) AS t(chain_id, pool_address, block, transaction_hash, transaction_index,
                   log_index, sender, recipient, amount0, amount1, paid0, paid1)
            ON CONFLICT (chain_id, transaction_hash, log_index) DO NOTHING
           ",
        )
        .bind(&chain_ids[..])
        .bind(&pool_addresses[..])
        .bind(&blocks[..])
        .bind(&transaction_hashes[..])
        .bind(&transaction_indices[..])
        .bind(&log_indices[..])
        .bind(&senders[..])
        .bind(&recipients[..])
        .bind(&amount0s[..])
        .bind(&amount1s[..])
        .bind(&paid0s[..])
        .bind(&paid1s[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_flash_event table: {e}"))
    }

    pub async fn add_pool_snapshot(
        &self,
        chain_id: u32,
        pool_address: &Address,
        snapshot: &PoolSnapshot,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO pool_snapshot (
                chain_id, pool_address, block, transaction_index, log_index, transaction_hash,
                current_tick, price_sqrt_ratio_x96, liquidity,
                protocol_fees_token0, protocol_fees_token1, fee_protocol,
                fee_growth_global_0, fee_growth_global_1,
                total_amount0_deposited, total_amount1_deposited,
                total_amount0_collected, total_amount1_collected,
                total_swaps, total_mints, total_burns, total_fee_collects, total_flashes,
                liquidity_utilization_rate
            ) VALUES (
                $1, $2, $3, $4, $5, $6,
                $7, $8::U160, $9::U128, $10::U256, $11::U256, $12,
                $13::U256, $14::U256, $15::U256, $16::U256, $17::U256, $18::U256,
                $19, $20, $21, $22, $23, $24
            )
            ON CONFLICT (chain_id, pool_address, block, transaction_index, log_index)
            DO NOTHING
            ",
        )
        .bind(chain_id as i32)
        .bind(pool_address.to_string())
        .bind(snapshot.block_position.number as i64)
        .bind(snapshot.block_position.transaction_index as i32)
        .bind(snapshot.block_position.log_index as i32)
        .bind(snapshot.block_position.transaction_hash.to_string())
        .bind(snapshot.state.current_tick)
        .bind(snapshot.state.price_sqrt_ratio_x96.to_string())
        .bind(snapshot.state.liquidity.to_string())
        .bind(snapshot.state.protocol_fees_token0.to_string())
        .bind(snapshot.state.protocol_fees_token1.to_string())
        .bind(snapshot.state.fee_protocol as i16)
        .bind(snapshot.state.fee_growth_global_0.to_string())
        .bind(snapshot.state.fee_growth_global_1.to_string())
        .bind(snapshot.analytics.total_amount0_deposited.to_string())
        .bind(snapshot.analytics.total_amount1_deposited.to_string())
        .bind(snapshot.analytics.total_amount0_collected.to_string())
        .bind(snapshot.analytics.total_amount1_collected.to_string())
        .bind(snapshot.analytics.total_swaps as i32)
        .bind(snapshot.analytics.total_mints as i32)
        .bind(snapshot.analytics.total_burns as i32)
        .bind(snapshot.analytics.total_fee_collects as i32)
        .bind(snapshot.analytics.total_flashes as i32)
        .bind(snapshot.analytics.liquidity_utilization_rate)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into pool_snapshot table: {e}"))
    }

    /// Inserts multiple pool positions in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_positions_batch(
        &self,
        chain_id: u32,
        snapshot_block: u64,
        snapshot_transaction_index: u32,
        snapshot_log_index: u32,
        positions: &[(Address, PoolPosition)],
    ) -> anyhow::Result<()> {
        if positions.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = positions.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut owners: Vec<String> = Vec::with_capacity(len);
        let mut tick_lowers: Vec<i32> = Vec::with_capacity(len);
        let mut tick_uppers: Vec<i32> = Vec::with_capacity(len);
        let mut liquidities: Vec<String> = Vec::with_capacity(len);
        let mut fee_growth_inside_0_lasts: Vec<String> = Vec::with_capacity(len);
        let mut fee_growth_inside_1_lasts: Vec<String> = Vec::with_capacity(len);
        let mut tokens_owed_0s: Vec<String> = Vec::with_capacity(len);
        let mut tokens_owed_1s: Vec<String> = Vec::with_capacity(len);
        let mut total_amount0_depositeds: Vec<Option<String>> = Vec::with_capacity(len);
        let mut total_amount1_depositeds: Vec<Option<String>> = Vec::with_capacity(len);
        let mut total_amount0_collecteds: Vec<Option<String>> = Vec::with_capacity(len);
        let mut total_amount1_collecteds: Vec<Option<String>> = Vec::with_capacity(len);

        // Fill vectors from positions
        for (pool_address, position) in positions {
            pool_addresses.push(pool_address.to_string());
            owners.push(position.owner.to_string());
            tick_lowers.push(position.tick_lower);
            tick_uppers.push(position.tick_upper);
            liquidities.push(position.liquidity.to_string());
            fee_growth_inside_0_lasts.push(position.fee_growth_inside_0_last.to_string());
            fee_growth_inside_1_lasts.push(position.fee_growth_inside_1_last.to_string());
            tokens_owed_0s.push(position.tokens_owed_0.to_string());
            tokens_owed_1s.push(position.tokens_owed_1.to_string());
            total_amount0_depositeds.push(Some(position.total_amount0_deposited.to_string()));
            total_amount1_depositeds.push(Some(position.total_amount1_deposited.to_string()));
            total_amount0_collecteds.push(Some(position.total_amount0_collected.to_string()));
            total_amount1_collecteds.push(Some(position.total_amount1_collected.to_string()));
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_position (
                chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index,
                owner, tick_lower, tick_upper,
                liquidity, fee_growth_inside_0_last, fee_growth_inside_1_last,
                tokens_owed_0, tokens_owed_1,
                total_amount0_deposited, total_amount1_deposited,
                total_amount0_collected, total_amount1_collected
            )
            SELECT
                $1, pool_address, $2, $3, $4,
                owner, tick_lower, tick_upper,
                liquidity::U128, fee_growth_inside_0_last::U256, fee_growth_inside_1_last::U256,
                tokens_owed_0::U128, tokens_owed_1::U128,
                total_amount0_deposited::U256, total_amount1_deposited::U256,
                total_amount0_collected::U128, total_amount1_collected::U128
            FROM UNNEST(
                $5::TEXT[], $6::TEXT[], $7::INT[], $8::INT[], $9::TEXT[], $10::TEXT[],
                $11::TEXT[], $12::TEXT[], $13::TEXT[], $14::TEXT[], $15::TEXT[],
                $16::TEXT[], $17::TEXT[]
            ) AS t(pool_address, owner, tick_lower, tick_upper,
                   liquidity, fee_growth_inside_0_last, fee_growth_inside_1_last,
                   tokens_owed_0, tokens_owed_1,
                   total_amount0_deposited, total_amount1_deposited,
                   total_amount0_collected, total_amount1_collected)
            ON CONFLICT (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index, owner, tick_lower, tick_upper)
            DO NOTHING
           ",
        )
        .bind(chain_id as i32)
        .bind(snapshot_block as i64)
        .bind(snapshot_transaction_index as i32)
        .bind(snapshot_log_index as i32)
        .bind(&pool_addresses[..])
        .bind(&owners[..])
        .bind(&tick_lowers[..])
        .bind(&tick_uppers[..])
        .bind(&liquidities[..])
        .bind(&fee_growth_inside_0_lasts[..])
        .bind(&fee_growth_inside_1_lasts[..])
        .bind(&tokens_owed_0s[..])
        .bind(&tokens_owed_1s[..])
        .bind(&total_amount0_depositeds as &[Option<String>])
        .bind(&total_amount1_depositeds as &[Option<String>])
        .bind(&total_amount0_collecteds as &[Option<String>])
        .bind(&total_amount1_collecteds as &[Option<String>])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_position table: {e}"))
    }

    /// Inserts multiple pool ticks in a single database operation using UNNEST for optimal performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn add_pool_ticks_batch(
        &self,
        chain_id: u32,
        snapshot_block: u64,
        snapshot_transaction_index: u32,
        snapshot_log_index: u32,
        ticks: &[(Address, &PoolTick)],
    ) -> anyhow::Result<()> {
        if ticks.is_empty() {
            return Ok(());
        }

        // Prepare vectors for each column
        let len = ticks.len();
        let mut pool_addresses: Vec<String> = Vec::with_capacity(len);
        let mut tick_values: Vec<i32> = Vec::with_capacity(len);
        let mut liquidity_grosses: Vec<String> = Vec::with_capacity(len);
        let mut liquidity_nets: Vec<String> = Vec::with_capacity(len);
        let mut fee_growth_outside_0s: Vec<String> = Vec::with_capacity(len);
        let mut fee_growth_outside_1s: Vec<String> = Vec::with_capacity(len);
        let mut initializeds: Vec<bool> = Vec::with_capacity(len);
        let mut last_updated_blocks: Vec<i64> = Vec::with_capacity(len);

        // Fill vectors from ticks
        for (pool_address, tick) in ticks {
            pool_addresses.push(pool_address.to_string());
            tick_values.push(tick.value);
            liquidity_grosses.push(tick.liquidity_gross.to_string());
            liquidity_nets.push(tick.liquidity_net.to_string());
            fee_growth_outside_0s.push(tick.fee_growth_outside_0.to_string());
            fee_growth_outside_1s.push(tick.fee_growth_outside_1.to_string());
            initializeds.push(tick.initialized);
            last_updated_blocks.push(tick.last_updated_block as i64);
        }

        // Execute batch insert with UNNEST
        sqlx::query(
            r"
            INSERT INTO pool_tick (
                chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index,
                tick_value, liquidity_gross, liquidity_net,
                fee_growth_outside_0, fee_growth_outside_1, initialized, last_updated_block
            )
            SELECT
                $1, pool_address, $2, $3, $4,
                tick_value, liquidity_gross::U128, liquidity_net::I128,
                fee_growth_outside_0::U256, fee_growth_outside_1::U256, initialized, last_updated_block
            FROM UNNEST(
                $5::TEXT[], $6::INT[], $7::TEXT[], $8::TEXT[], $9::TEXT[],
                $10::TEXT[], $11::BOOLEAN[], $12::BIGINT[]
            ) AS t(pool_address, tick_value, liquidity_gross, liquidity_net,
                   fee_growth_outside_0, fee_growth_outside_1, initialized, last_updated_block)
            ON CONFLICT (chain_id, pool_address, snapshot_block, snapshot_transaction_index, snapshot_log_index, tick_value)
            DO NOTHING
           ",
        )
        .bind(chain_id as i32)
        .bind(snapshot_block as i64)
        .bind(snapshot_transaction_index as i32)
        .bind(snapshot_log_index as i32)
        .bind(&pool_addresses[..])
        .bind(&tick_values[..])
        .bind(&liquidity_grosses[..])
        .bind(&liquidity_nets[..])
        .bind(&fee_growth_outside_0s[..])
        .bind(&fee_growth_outside_1s[..])
        .bind(&initializeds[..])
        .bind(&last_updated_blocks[..])
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to batch insert into pool_tick table: {e}"))
    }

    pub async fn update_pool_initial_price_tick(
        &self,
        chain_id: u32,
        initialize_event: &InitializeEvent,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            UPDATE pool
            SET
                initial_tick = $4,
                initial_sqrt_price_x96 = $5
            WHERE chain_id = $1
            AND dex_name = $2
            AND address = $3
            ",
        )
        .bind(chain_id as i32)
        .bind(initialize_event.dex.name.to_string())
        .bind(initialize_event.pool_address.to_string())
        .bind(initialize_event.tick)
        .bind(initialize_event.sqrt_price_x96.to_string())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to update dex last synced block: {e}"))
    }

    /// Loads the latest valid pool snapshot from the database.
    ///
    /// Returns the most recent snapshot that has been validated against on-chain state.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn load_latest_valid_pool_snapshot(
        &self,
        chain_id: u32,
        pool_address: &Address,
    ) -> anyhow::Result<Option<PoolSnapshot>> {
        let result = sqlx::query(
            r"
            SELECT
                block, transaction_index, log_index, transaction_hash,
                current_tick, price_sqrt_ratio_x96::TEXT, liquidity::TEXT,
                protocol_fees_token0::TEXT, protocol_fees_token1::TEXT, fee_protocol,
                fee_growth_global_0::TEXT, fee_growth_global_1::TEXT,
                total_amount0_deposited::TEXT, total_amount1_deposited::TEXT,
                total_amount0_collected::TEXT, total_amount1_collected::TEXT,
                total_swaps, total_mints, total_burns, total_fee_collects, total_flashes,
                liquidity_utilization_rate,
                (SELECT dex_name FROM pool WHERE chain_id = $1 AND address = $2) as dex_name
            FROM pool_snapshot
            WHERE chain_id = $1 AND pool_address = $2 AND is_valid = TRUE
            ORDER BY block DESC, transaction_index DESC, log_index DESC
            LIMIT 1
            ",
        )
        .bind(chain_id as i32)
        .bind(pool_address.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load latest valid pool snapshot: {}", e))?;

        if let Some(row) = result {
            // Parse snapshot state
            let block: i64 = row.get("block");
            let transaction_index: i32 = row.get("transaction_index");
            let log_index: i32 = row.get("log_index");
            let transaction_hash: String = row.get("transaction_hash");

            let block_position = nautilus_model::defi::data::block::BlockPosition::new(
                block as u64,
                transaction_hash,
                transaction_index as u32,
                log_index as u32,
            );

            let state = PoolState {
                current_tick: row.get("current_tick"),
                price_sqrt_ratio_x96: row.get::<String, _>("price_sqrt_ratio_x96").parse()?,
                liquidity: row.get::<String, _>("liquidity").parse()?,
                protocol_fees_token0: row.get::<String, _>("protocol_fees_token0").parse()?,
                protocol_fees_token1: row.get::<String, _>("protocol_fees_token1").parse()?,
                fee_protocol: row.get::<i16, _>("fee_protocol") as u8,
                fee_growth_global_0: row.get::<String, _>("fee_growth_global_0").parse()?,
                fee_growth_global_1: row.get::<String, _>("fee_growth_global_1").parse()?,
            };

            let analytics = PoolAnalytics {
                total_amount0_deposited: row.get::<String, _>("total_amount0_deposited").parse()?,
                total_amount1_deposited: row.get::<String, _>("total_amount1_deposited").parse()?,
                total_amount0_collected: row.get::<String, _>("total_amount0_collected").parse()?,
                total_amount1_collected: row.get::<String, _>("total_amount1_collected").parse()?,
                total_swaps: row.get::<i32, _>("total_swaps") as u64,
                total_mints: row.get::<i32, _>("total_mints") as u64,
                total_burns: row.get::<i32, _>("total_burns") as u64,
                total_fee_collects: row.get::<i32, _>("total_fee_collects") as u64,
                total_flashes: row.get::<i32, _>("total_flashes") as u64,
                liquidity_utilization_rate: row.get::<f64, _>("liquidity_utilization_rate"),
            };

            // Load positions and ticks
            let positions = self
                .load_pool_positions_for_snapshot(
                    chain_id,
                    pool_address,
                    block as u64,
                    transaction_index as u32,
                    log_index as u32,
                )
                .await?;

            let ticks = self
                .load_pool_ticks_for_snapshot(
                    chain_id,
                    pool_address,
                    block as u64,
                    transaction_index as u32,
                    log_index as u32,
                )
                .await?;

            let dex_name: String = row.get("dex_name");
            let chain = nautilus_model::defi::Chain::from_chain_id(chain_id)
                .ok_or_else(|| anyhow::anyhow!("Unknown chain_id: {}", chain_id))?;

            let dex_type = nautilus_model::defi::DexType::from_dex_name(&dex_name)
                .ok_or_else(|| anyhow::anyhow!("Unknown dex_name: {}", dex_name))?;

            let dex_extended = crate::exchanges::get_dex_extended(chain.name, &dex_type)
                .ok_or_else(|| {
                    anyhow::anyhow!("No DEX extended found for {} on {}", dex_name, chain.name)
                })?;

            let instrument_id =
                Pool::create_instrument_id(chain.name, &dex_extended.dex, pool_address);

            Ok(Some(PoolSnapshot::new(
                instrument_id,
                state,
                positions,
                ticks,
                analytics,
                block_position,
            )))
        } else {
            Ok(None)
        }
    }

    /// Marks a pool snapshot as valid after successful on-chain verification.
    ///
    /// # Errors
    ///
    /// Returns an error if the database operation fails.
    pub async fn mark_pool_snapshot_valid(
        &self,
        chain_id: u32,
        pool_address: &Address,
        block: u64,
        transaction_index: u32,
        log_index: u32,
    ) -> anyhow::Result<()> {
        sqlx::query(
            r"
            UPDATE pool_snapshot
            SET is_valid = TRUE
            WHERE chain_id = $1
            AND pool_address = $2
            AND block = $3
            AND transaction_index = $4
            AND log_index = $5
            ",
        )
        .bind(chain_id as i32)
        .bind(pool_address.to_string())
        .bind(block as i64)
        .bind(transaction_index as i32)
        .bind(log_index as i32)
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to mark pool snapshot as valid: {}", e))
    }

    /// Loads all positions for a specific snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn load_pool_positions_for_snapshot(
        &self,
        chain_id: u32,
        pool_address: &Address,
        snapshot_block: u64,
        snapshot_transaction_index: u32,
        snapshot_log_index: u32,
    ) -> anyhow::Result<Vec<PoolPosition>> {
        let rows = sqlx::query(
            r"
            SELECT
                owner, tick_lower, tick_upper,
                liquidity::TEXT, fee_growth_inside_0_last::TEXT, fee_growth_inside_1_last::TEXT,
                tokens_owed_0::TEXT, tokens_owed_1::TEXT,
                total_amount0_deposited::TEXT, total_amount1_deposited::TEXT,
                total_amount0_collected::TEXT, total_amount1_collected::TEXT
            FROM pool_position
            WHERE chain_id = $1
            AND pool_address = $2
            AND snapshot_block = $3
            AND snapshot_transaction_index = $4
            AND snapshot_log_index = $5
            ",
        )
        .bind(chain_id as i32)
        .bind(pool_address.to_string())
        .bind(snapshot_block as i64)
        .bind(snapshot_transaction_index as i32)
        .bind(snapshot_log_index as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load pool positions: {}", e))?;

        rows.iter()
            .map(|row| {
                let owner: String = row.get("owner");
                let position = PoolPosition {
                    owner: validate_address(&owner)?,
                    tick_lower: row.get("tick_lower"),
                    tick_upper: row.get("tick_upper"),
                    liquidity: row.get::<String, _>("liquidity").parse()?,
                    fee_growth_inside_0_last: row
                        .get::<String, _>("fee_growth_inside_0_last")
                        .parse()?,
                    fee_growth_inside_1_last: row
                        .get::<String, _>("fee_growth_inside_1_last")
                        .parse()?,
                    tokens_owed_0: row.get::<String, _>("tokens_owed_0").parse()?,
                    tokens_owed_1: row.get::<String, _>("tokens_owed_1").parse()?,
                    total_amount0_deposited: row
                        .get::<String, _>("total_amount0_deposited")
                        .parse()?,
                    total_amount1_deposited: row
                        .get::<String, _>("total_amount1_deposited")
                        .parse()?,
                    total_amount0_collected: row
                        .get::<String, _>("total_amount0_collected")
                        .parse()?,
                    total_amount1_collected: row
                        .get::<String, _>("total_amount1_collected")
                        .parse()?,
                };
                Ok(position)
            })
            .collect()
    }

    /// Loads all ticks for a specific snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails.
    pub async fn load_pool_ticks_for_snapshot(
        &self,
        chain_id: u32,
        pool_address: &Address,
        snapshot_block: u64,
        snapshot_transaction_index: u32,
        snapshot_log_index: u32,
    ) -> anyhow::Result<Vec<PoolTick>> {
        let rows = sqlx::query(
            r"
            SELECT
                tick_value, liquidity_gross::TEXT, liquidity_net::TEXT,
                fee_growth_outside_0::TEXT, fee_growth_outside_1::TEXT, initialized,
                last_updated_block
            FROM pool_tick
            WHERE chain_id = $1
            AND pool_address = $2
            AND snapshot_block = $3
            AND snapshot_transaction_index = $4
            AND snapshot_log_index = $5
            ",
        )
        .bind(chain_id as i32)
        .bind(pool_address.to_string())
        .bind(snapshot_block as i64)
        .bind(snapshot_transaction_index as i32)
        .bind(snapshot_log_index as i32)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| anyhow::anyhow!("Failed to load pool ticks: {}", e))?;

        rows.iter()
            .map(|row| {
                let tick = PoolTick::new(
                    row.get("tick_value"),
                    row.get::<String, _>("liquidity_gross").parse()?,
                    row.get::<String, _>("liquidity_net").parse()?,
                    row.get::<String, _>("fee_growth_outside_0").parse()?,
                    row.get::<String, _>("fee_growth_outside_1").parse()?,
                    row.get("initialized"),
                    row.get::<i64, _>("last_updated_block") as u64,
                );
                Ok(tick)
            })
            .collect()
    }

    /// Streams pool events from all event tables (swap, liquidity, collect) for a specific pool.
    ///
    /// Creates a unified stream of pool events from multiple tables, ordering them chronologically
    /// by block number, transaction index, and log index. Optionally resumes from a specific block position.
    ///
    /// # Returns
    ///
    /// A stream of `DexPoolData` events in chronological order.
    ///
    /// # Errors
    ///
    /// Returns an error if the database query fails or if event transformation fails.
    pub fn stream_pool_events<'a>(
        &'a self,
        chain: SharedChain,
        dex: SharedDex,
        instrument_id: InstrumentId,
        pool_address: &Address,
        from_position: Option<BlockPosition>,
    ) -> Pin<Box<dyn Stream<Item = Result<DexPoolData, anyhow::Error>> + Send + 'a>> {
        // Query without position filter (streams all events)
        const QUERY_ALL: &str = r"
            (SELECT
                'swap' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                recipient,
                NULL::TEXT as owner,
                side,
                size,
                price,
                sqrt_price_x96::TEXT,
                liquidity::TEXT as swap_liquidity,
                tick as swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                NULL::TEXT as amount0,
                NULL::TEXT as amount1,
                NULL::INT as tick_lower,
                NULL::INT as tick_upper,
                NULL::TEXT as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_swap_event
            WHERE chain_id = $1 AND pool_address = $2)
            UNION ALL
            (SELECT
                'liquidity' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                NULL::TEXT as recipient,
                owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::text as price,
                NULL::text as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT as swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                position_liquidity::TEXT,
                amount0::TEXT,
                amount1::TEXT,
                tick_lower::INT,
                tick_upper::INT,
                event_type as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_liquidity_event
            WHERE chain_id = $1 AND pool_address = $2)
            UNION ALL
            (SELECT
                'collect' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                NULL::TEXT as sender,
                NULL::TEXT as recipient,
                owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::TEXT as price,
                NULL::TEXT as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT AS swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                amount0::TEXT,
                amount1::TEXT,
                tick_lower::INT,
                tick_upper::INT,
                NULL::TEXT as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_collect_event
            WHERE chain_id = $1 AND pool_address = $2)
            UNION ALL
            (SELECT
                'flash' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                recipient,
                NULL::TEXT as owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::TEXT as price,
                NULL::TEXT as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT AS swap_tick,
                NULL::TEXT as swap_amount0,
                NULL::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                NULL::TEXT as amount0,
                NULL::TEXT as amount1,
                NULL::INT as tick_lower,
                NULL::INT as tick_upper,
                NULL::TEXT as liquidity_event_type,
                amount0::TEXT as flash_amount0,
                amount1::TEXT as flash_amount1,
                paid0::TEXT as flash_paid0,
                paid1::TEXT as flash_paid1
            FROM pool_flash_event
            WHERE chain_id = $1 AND pool_address = $2)
            ORDER BY block, transaction_index, log_index";

        // Query with position filter (resumes from specific block position)
        const QUERY_FROM_POSITION: &str = r"
            (SELECT
                'swap' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                recipient,
                NULL::TEXT as owner,
                side,
                size,
                price,
                sqrt_price_x96::TEXT,
                liquidity::TEXT as swap_liquidity,
                tick as swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                NULL::TEXT as amount0,
                NULL::TEXT as amount1,
                NULL::INT as tick_lower,
                NULL::INT as tick_upper,
                NULL::TEXT as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_swap_event
            WHERE chain_id = $1 AND pool_address = $2
            AND (block > $3 OR (block = $3 AND transaction_index > $4) OR (block = $3 AND transaction_index = $4 AND log_index > $5)))
            UNION ALL
            (SELECT
                'liquidity' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                NULL::TEXT as recipient,
                owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::text as price,
                NULL::text as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT as swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                position_liquidity::TEXT,
                amount0::TEXT,
                amount1::TEXT,
                tick_lower::INT,
                tick_upper::INT,
                event_type as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_liquidity_event
            WHERE chain_id = $1 AND pool_address = $2
            AND (block > $3 OR (block = $3 AND transaction_index > $4) OR (block = $3 AND transaction_index = $4 AND log_index > $5)))
            UNION ALL
            (SELECT
                'collect' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                NULL::TEXT as sender,
                NULL::TEXT as recipient,
                owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::TEXT as price,
                NULL::TEXT as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT AS swap_tick,
                amount0::TEXT as swap_amount0,
                amount1::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                amount0::TEXT,
                amount1::TEXT,
                tick_lower::INT,
                tick_upper::INT,
                NULL::TEXT as liquidity_event_type,
                NULL::TEXT as flash_amount0,
                NULL::TEXT as flash_amount1,
                NULL::TEXT as flash_paid0,
                NULL::TEXT as flash_paid1
            FROM pool_collect_event
            WHERE chain_id = $1 AND pool_address = $2
            AND (block > $3 OR (block = $3 AND transaction_index > $4) OR (block = $3 AND transaction_index = $4 AND log_index > $5)))
            UNION ALL
            (SELECT
                'flash' as event_type,
                chain_id,
                pool_address,
                block,
                transaction_hash,
                transaction_index,
                log_index,
                sender,
                recipient,
                NULL::TEXT as owner,
                NULL::TEXT as side,
                NULL::TEXT as size,
                NULL::TEXT as price,
                NULL::TEXT as sqrt_price_x96,
                NULL::TEXT as swap_liquidity,
                NULL::INT AS swap_tick,
                NULL::TEXT as swap_amount0,
                NULL::TEXT as swap_amount1,
                NULL::TEXT as position_liquidity,
                NULL::TEXT as amount0,
                NULL::TEXT as amount1,
                NULL::INT as tick_lower,
                NULL::INT as tick_upper,
                NULL::TEXT as liquidity_event_type,
                amount0::TEXT as flash_amount0,
                amount1::TEXT as flash_amount1,
                paid0::TEXT as flash_paid0,
                paid1::TEXT as flash_paid1
            FROM pool_flash_event
            WHERE chain_id = $1 AND pool_address = $2
            AND (block > $3 OR (block = $3 AND transaction_index > $4) OR (block = $3 AND transaction_index = $4 AND log_index > $5)))
            ORDER BY block, transaction_index, log_index";

        // Build query with appropriate bindings
        let query = if let Some(pos) = from_position {
            sqlx::query(QUERY_FROM_POSITION)
                .bind(chain.chain_id as i32)
                .bind(pool_address.to_string())
                .bind(pos.number as i64)
                .bind(pos.transaction_index as i32)
                .bind(pos.log_index as i32)
                .fetch(&self.pool)
        } else {
            sqlx::query(QUERY_ALL)
                .bind(chain.chain_id as i32)
                .bind(pool_address.to_string())
                .fetch(&self.pool)
        };

        // Transform rows to events
        let stream = query.map(move |row_result| match row_result {
            Ok(row) => {
                transform_row_to_dex_pool_data(&row, chain.clone(), dex.clone(), instrument_id)
                    .map_err(|e| anyhow::anyhow!("Steam pool event transform error: {}", e))
            }
            Err(e) => Err(anyhow::anyhow!("Stream pool events database error: {}", e)),
        });

        Box::pin(stream)
    }
}
