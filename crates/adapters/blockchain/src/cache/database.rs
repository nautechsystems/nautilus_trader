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

use nautilus_model::defi::{
    amm::Pool,
    block::Block,
    chain::{Chain, SharedChain},
    dex::Dex,
    liquidity::PoolLiquidityUpdate,
    swap::Swap,
    token::Token,
};
use sqlx::{PgPool, postgres::PgConnectOptions};

use crate::cache::rows::{BlockTimestampRow, TokenRow};

/// Database interface for persisting and retrieving blockchain entities and domain objects.
#[derive(Debug)]
pub struct BlockchainCacheDatabase {
    /// PostgreSQL connection pool used for database operations.
    pool: PgPool,
}

impl BlockchainCacheDatabase {
    /// Initializes a new database instance by establishing a connection to PostgreSQL.
    pub async fn init(pg_options: PgConnectOptions) -> Self {
        let pool = PgPool::connect_with(pg_options)
            .await
            .expect("Error connecting to Postgres");
        Self { pool }
    }

    /// Seeds the database with a blockchain chain record.
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

    /// Inserts or updates a block record in the database.
    pub async fn add_block(&self, chain_id: u32, block: &Block) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO block (
                chain_id, number, hash, parent_hash, miner, gas_limit, gas_used, timestamp
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8)
            ON CONFLICT (chain_id, number)
            DO UPDATE
            SET
                hash = $3,
                parent_hash = $4,
                miner = $5,
                gas_limit = $6,
                gas_used = $7,
                timestamp = $8
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
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into block table: {e}"))
    }

    /// Retrieves block timestamps for a given chain starting from a specific block number.
    pub async fn load_block_timestamps(
        &self,
        chain: SharedChain,
        from_block: u64,
    ) -> anyhow::Result<Vec<BlockTimestampRow>> {
        sqlx::query_as::<_, BlockTimestampRow>(
            r"
            SELECT DISTINCT ON (block.chain_id, number)
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
    pub async fn add_dex(&self, dex: &Dex) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO dex (
                chain_id, name, factory_address
            ) VALUES ($1, $2, $3)
            ON CONFLICT (chain_id, name)
            DO UPDATE
            SET
                factory_address = $3
        ",
        )
        .bind(dex.chain.chain_id as i32)
        .bind(dex.name.as_ref())
        .bind(dex.factory.as_ref())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into dex table: {e}"))
    }

    /// Adds or updates a liquidity pool/pair record in the database.
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
        .bind(pool.dex.name.as_ref())
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

    /// Adds or updates a token record in the database.
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

    /// Persists a token swap transaction event to the pool_swap table.
    pub async fn add_swap(&self, chain_id: u32, swap: &Swap) -> anyhow::Result<()> {
        sqlx::query(
            r"
            INSERT INTO pool_swap (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, side, quantity, price
            ) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10)
            ON CONFLICT (chain_id, transaction_hash, log_index)
            DO NOTHING
        ",
        )
        .bind(chain_id as i32)
        .bind(swap.pool.address.to_string())
        .bind(swap.block as i64)
        .bind(swap.transaction_hash.as_str())
        .bind(swap.transaction_index as i32)
        .bind(swap.log_index as i32)
        .bind(swap.sender.to_string())
        .bind(swap.side.to_string())
        .bind(swap.quantity.to_string())
        .bind(swap.price.to_string())
        .execute(&self.pool)
        .await
        .map(|_| ())
        .map_err(|e| anyhow::anyhow!("Failed to insert into pool_swap table: {e}"))
    }

    /// Persists a liquidity position change (mint/burn) event to the pool_liquidity table.
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
        .bind(liquidity_update.pool.address.to_string())
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

    /// Retrieves all token records for the given chain and converts them into `Token` domain objects.
    pub async fn load_tokens(&self, chain: SharedChain) -> anyhow::Result<Vec<Token>> {
        sqlx::query_as::<_, TokenRow>("SELECT * FROM token WHERE chain_id = $1")
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
}
