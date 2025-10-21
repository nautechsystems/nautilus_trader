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

//! PostgreSQL COPY BINARY operations for high-performance bulk data loading.
//!
//! This module provides utilities for using PostgreSQL's COPY command with binary format,
//! which offers significantly better performance than standard INSERT operations for bulk data loading.

use nautilus_model::defi::{
    Block, Pool, PoolLiquidityUpdate, PoolSwap, Token, data::PoolFeeCollect,
};
use sqlx::{PgPool, postgres::PgPoolCopyExt};

// Helper to convert scientific notation to decimal
fn format_scientific_to_decimal(s: &str) -> String {
    use std::str::FromStr;

    use rust_decimal::Decimal;

    match Decimal::from_str(s) {
        Ok(decimal) => decimal.to_string(),
        Err(_) => s.to_string(), // Fallback
    }
}

/// Formats a numeric value for PostgreSQL NUMERIC type
fn format_numeric<T: ToString>(value: &T) -> String {
    let s = value.to_string();

    // Remove any '+' prefix
    let s = s.trim_start_matches('+');

    // Handle scientific notation by converting to decimal
    if s.contains('e') || s.contains('E') {
        return format_scientific_to_decimal(s);
    }

    // For very large numbers that rust_decimal can't handle,
    // just return the cleaned string since alloy_primitives already
    // produces clean decimal format
    s.to_string()
}

/// Handles PostgreSQL COPY BINARY operations for blockchain data.
#[derive(Debug)]
pub struct PostgresCopyHandler<'a> {
    pool: &'a PgPool,
}

impl<'a> PostgresCopyHandler<'a> {
    /// Creates a new COPY handler with a reference to the database pool.
    #[must_use]
    pub const fn new(pool: &'a PgPool) -> Self {
        Self { pool }
    }

    /// Inserts blocks using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// This method is significantly faster than INSERT for bulk operations as it bypasses
    /// SQL parsing and uses PostgreSQL's native binary protocol.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_blocks(&self, chain_id: u32, blocks: &[Block]) -> anyhow::Result<()> {
        if blocks.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY block (
                chain_id, number, hash, parent_hash, miner, gas_limit, gas_used, timestamp,
                base_fee_per_gas, blob_gas_used, excess_blob_gas,
                l1_gas_price, l1_gas_used, l1_fee_scalar
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        // Write binary header
        self.write_copy_header(&mut copy_in).await?;

        // Write each block as binary data
        for block in blocks {
            self.write_block_binary(&mut copy_in, chain_id, block)
                .await?;
        }

        // Write binary trailer
        self.write_copy_trailer(&mut copy_in).await?;

        // Finish the COPY operation
        copy_in
            .finish()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to finish COPY operation: {e}"))?;

        Ok(())
    }

    /// Inserts tokens using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_tokens(&self, chain_id: u32, tokens: &[Token]) -> anyhow::Result<()> {
        if tokens.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY token (
                chain_id, address, name, symbol, decimals
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        self.write_copy_header(&mut copy_in).await?;
        for token in tokens {
            self.write_token_binary(&mut copy_in, chain_id, token)
                .await?;
        }
        self.write_copy_trailer(&mut copy_in).await?;
        copy_in
            .finish()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to finish COPY operation: {e}"))?;

        Ok(())
    }

    /// Inserts pools using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_pools(&self, chain_id: u32, pools: &[Pool]) -> anyhow::Result<()> {
        if pools.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY pool (
                chain_id, dex_name, address, creation_block,
                token0_chain, token0_address, token1_chain, token1_address,
                fee, tick_spacing, initial_tick, initial_sqrt_price_x96
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        self.write_copy_header(&mut copy_in).await?;
        for pool in pools {
            self.write_pool_binary(&mut copy_in, chain_id, pool).await?;
        }
        self.write_copy_trailer(&mut copy_in).await?;
        copy_in
            .finish()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to finish COPY operation: {e}"))?;

        Ok(())
    }

    /// Inserts pool swaps using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_pool_swaps(&self, chain_id: u32, swaps: &[PoolSwap]) -> anyhow::Result<()> {
        if swaps.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY pool_swap_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, sender, recipient, side, size, price, sqrt_price_x96, amount0, amount1
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        // Write binary header
        self.write_copy_header(&mut copy_in).await?;

        // Write each swap as binary data
        for swap in swaps {
            self.write_pool_swap_binary(&mut copy_in, chain_id, swap)
                .await?;
        }

        // Write binary trailer
        self.write_copy_trailer(&mut copy_in).await?;

        // Finish the COPY operation
        copy_in.finish().await.map_err(|e| {
            // Log detailed information about the failed batch
            tracing::error!("COPY operation failed for pool_swap batch:");
            tracing::error!("  Chain ID: {}", chain_id);
            tracing::error!("  Batch size: {}", swaps.len());

            if !swaps.is_empty() {
                tracing::error!(
                    "  Block range: {} to {}",
                    swaps.iter().map(|s| s.block).min().unwrap_or(0),
                    swaps.iter().map(|s| s.block).max().unwrap_or(0)
                );
            }

            // Log first few swaps with key details
            for (i, swap) in swaps.iter().take(5).enumerate() {
                tracing::error!(
                    "  Swap[{}]: tx={} log_idx={} block={} pool={}",
                    i,
                    swap.transaction_hash,
                    swap.log_index,
                    swap.block,
                    swap.pool_address
                );
            }

            if swaps.len() > 5 {
                tracing::error!("  ... and {} more swaps", swaps.len() - 5);
            }

            anyhow::anyhow!("Failed to finish COPY operation: {e}")
        })?;

        Ok(())
    }

    /// Inserts pool liquidity updates using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_pool_liquidity_updates(
        &self,
        chain_id: u32,
        updates: &[PoolLiquidityUpdate],
    ) -> anyhow::Result<()> {
        if updates.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY pool_liquidity_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, event_type, sender, owner, position_liquidity,
                amount0, amount1, tick_lower, tick_upper
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        // Write binary header
        self.write_copy_header(&mut copy_in).await?;

        // Write each liquidity update as binary data
        for update in updates {
            self.write_pool_liquidity_update_binary(&mut copy_in, chain_id, update)
                .await?;
        }

        // Write binary trailer
        self.write_copy_trailer(&mut copy_in).await?;

        // Finish the COPY operation
        copy_in.finish().await.map_err(|e| {
            // Log detailed information about the failed batch
            tracing::error!("COPY operation failed for pool_liquidity batch:");
            tracing::error!("  Chain ID: {}", chain_id);
            tracing::error!("  Batch size: {}", updates.len());

            if !updates.is_empty() {
                tracing::error!(
                    "  Block range: {} to {}",
                    updates.iter().map(|u| u.block).min().unwrap_or(0),
                    updates.iter().map(|u| u.block).max().unwrap_or(0)
                );
            }

            // Log first few liquidity updates with key details
            for (i, update) in updates.iter().take(5).enumerate() {
                tracing::error!(
                    "  Update[{}]: tx={} log_idx={} block={} pool={} type={}",
                    i,
                    update.transaction_hash,
                    update.log_index,
                    update.block,
                    update.pool_address,
                    update.kind
                );
            }

            if updates.len() > 5 {
                tracing::error!("  ... and {} more updates", updates.len() - 5);
            }

            anyhow::anyhow!("Failed to finish COPY operation: {e}")
        })?;

        Ok(())
    }

    /// Writes the PostgreSQL COPY binary format header.
    ///
    /// The header consists of:
    /// - 11-byte signature: "PGCOPY\n\xff\r\n\0"
    /// - 4-byte flags field (all zeros)
    /// - 4-byte header extension length (all zeros)
    async fn write_copy_header(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut header = Vec::new();

        // PostgreSQL binary copy header
        header.write_all(b"PGCOPY\n\xff\r\n\0")?; // Signature
        header.write_all(&[0, 0, 0, 0])?; // Flags field
        header.write_all(&[0, 0, 0, 0])?; // Header extension length

        copy_in
            .send(header)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write COPY header: {e}"))?;
        Ok(())
    }

    /// Writes a single block in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_block_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        block: &Block,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (14)
        row_data.write_all(&14u16.to_be_bytes())?;

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // Field 2: number (INT8)
        let number_bytes = (block.number as i64).to_be_bytes();
        row_data.write_all(&(number_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&number_bytes)?;

        // Field 3: hash (TEXT)
        let hash_bytes = block.hash.as_bytes();
        row_data.write_all(&(hash_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(hash_bytes)?;

        // Field 4: parent_hash (TEXT)
        let parent_hash_bytes = block.parent_hash.as_bytes();
        row_data.write_all(&(parent_hash_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(parent_hash_bytes)?;

        // Field 5: miner (TEXT)
        let miner_bytes = block.miner.to_string().as_bytes().to_vec();
        row_data.write_all(&(miner_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&miner_bytes)?;

        // Field 6: gas_limit (INT8)
        let gas_limit_bytes = (block.gas_limit as i64).to_be_bytes();
        row_data.write_all(&(gas_limit_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&gas_limit_bytes)?;

        // Field 7: gas_used (INT8)
        let gas_used_bytes = (block.gas_used as i64).to_be_bytes();
        row_data.write_all(&(gas_used_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&gas_used_bytes)?;

        // Field 8: timestamp (TEXT)
        let timestamp_bytes = block.timestamp.to_string().as_bytes().to_vec();
        row_data.write_all(&(timestamp_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&timestamp_bytes)?;

        // Field 9: base_fee_per_gas (TEXT, nullable)
        if let Some(ref base_fee) = block.base_fee_per_gas {
            let base_fee_bytes = base_fee.to_string().as_bytes().to_vec();
            row_data.write_all(&(base_fee_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&base_fee_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 10: blob_gas_used (TEXT, nullable)
        if let Some(ref blob_gas) = block.blob_gas_used {
            let blob_gas_bytes = blob_gas.to_string().as_bytes().to_vec();
            row_data.write_all(&(blob_gas_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&blob_gas_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 11: excess_blob_gas (TEXT, nullable)
        if let Some(ref excess_blob) = block.excess_blob_gas {
            let excess_blob_bytes = excess_blob.to_string().as_bytes().to_vec();
            row_data.write_all(&(excess_blob_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&excess_blob_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 12: l1_gas_price (TEXT, nullable)
        if let Some(ref l1_gas_price) = block.l1_gas_price {
            let l1_gas_price_bytes = l1_gas_price.to_string().as_bytes().to_vec();
            row_data.write_all(&(l1_gas_price_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&l1_gas_price_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 13: l1_gas_used (INT8, nullable)
        if let Some(l1_gas_used) = block.l1_gas_used {
            let l1_gas_used_bytes = (l1_gas_used as i64).to_be_bytes();
            row_data.write_all(&(l1_gas_used_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&l1_gas_used_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 14: l1_fee_scalar (INT8, nullable)
        if let Some(l1_fee_scalar) = block.l1_fee_scalar {
            let l1_fee_scalar_bytes = (l1_fee_scalar as i64).to_be_bytes();
            row_data.write_all(&(l1_fee_scalar_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&l1_fee_scalar_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write block data: {e}"))?;
        Ok(())
    }

    /// Writes a single pool swap in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_pool_swap_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        swap: &PoolSwap,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (14)
        row_data.write_all(&14u16.to_be_bytes())?;

        // chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // pool_address (TEXT)
        let pool_address_bytes = swap.pool_address.to_string().as_bytes().to_vec();
        row_data.write_all(&(pool_address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&pool_address_bytes)?;

        // block (INT8)
        let block_bytes = (swap.block as i64).to_be_bytes();
        row_data.write_all(&(block_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&block_bytes)?;

        // transaction_hash (TEXT)
        let tx_hash_bytes = swap.transaction_hash.as_bytes();
        row_data.write_all(&(tx_hash_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(tx_hash_bytes)?;

        // transaction_index (INT4)
        let tx_index_bytes = (swap.transaction_index as i32).to_be_bytes();
        row_data.write_all(&(tx_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tx_index_bytes)?;

        // log_index (INT4)
        let log_index_bytes = (swap.log_index as i32).to_be_bytes();
        row_data.write_all(&(log_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&log_index_bytes)?;

        // sender (TEXT)
        let sender_bytes = swap.sender.to_string().as_bytes().to_vec();
        row_data.write_all(&(sender_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&sender_bytes)?;

        // recipient (TEXT)
        let recipient_bytes = swap.recipient.to_string().as_bytes().to_vec();
        row_data.write_all(&(recipient_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&recipient_bytes)?;

        // side (TEXT or NULL)
        if let Some(side) = swap.side {
            let side_bytes = side.to_string().as_bytes().to_vec();
            row_data.write_all(&(side_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&side_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // size (TEXT or NULL)
        if let Some(size) = swap.size {
            let size_bytes = size.to_string().as_bytes().to_vec();
            row_data.write_all(&(size_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&size_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // price (TEXT or NULL)
        if let Some(price) = swap.price {
            let price_bytes = price.to_string().as_bytes().to_vec();
            row_data.write_all(&(price_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&price_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // sqrt_price_x96 (U160)
        let sqrt_price_bytes = format_numeric(&swap.sqrt_price_x96).as_bytes().to_vec();
        row_data.write_all(&(sqrt_price_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&sqrt_price_bytes)?;

        // amount0 (I256)
        let amount0_bytes = format_numeric(&swap.amount0).as_bytes().to_vec();
        row_data.write_all(&(amount0_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&amount0_bytes)?;

        // amount1 (I256)
        let amount1_bytes = format_numeric(&swap.amount1).as_bytes().to_vec();
        row_data.write_all(&(amount1_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&amount1_bytes)?;

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write pool swap data: {e}"))?;
        Ok(())
    }

    /// Writes a single pool liquidity update in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_pool_liquidity_update_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        update: &PoolLiquidityUpdate,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (14)
        row_data.write_all(&14u16.to_be_bytes())?;

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // Field 2: pool_address (TEXT)
        let pool_address_bytes = update.pool_address.to_string().as_bytes().to_vec();
        row_data.write_all(&(pool_address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&pool_address_bytes)?;

        // Field 3: block (INT8)
        let block_bytes = (update.block as i64).to_be_bytes();
        row_data.write_all(&(block_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&block_bytes)?;

        // Field 4: transaction_hash (TEXT)
        let tx_hash_bytes = update.transaction_hash.as_bytes();
        row_data.write_all(&(tx_hash_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(tx_hash_bytes)?;

        // Field 5: transaction_index (INT4)
        let tx_index_bytes = (update.transaction_index as i32).to_be_bytes();
        row_data.write_all(&(tx_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tx_index_bytes)?;

        // Field 6: log_index (INT4)
        let log_index_bytes = (update.log_index as i32).to_be_bytes();
        row_data.write_all(&(log_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&log_index_bytes)?;

        // Field 7: event_type (TEXT)
        let event_type_bytes = update.kind.to_string().as_bytes().to_vec();
        row_data.write_all(&(event_type_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&event_type_bytes)?;

        // Field 8: sender (TEXT, nullable)
        if let Some(sender) = update.sender {
            let sender_bytes = sender.to_string().as_bytes().to_vec();
            row_data.write_all(&(sender_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&sender_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL value
        }

        // Field 9: owner (TEXT)
        let owner_bytes = update.owner.to_string().as_bytes().to_vec();
        row_data.write_all(&(owner_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&owner_bytes)?;

        // Field 10: position_liquidity (U128)
        let position_liquidity_bytes = format_numeric(&update.position_liquidity)
            .as_bytes()
            .to_vec();
        row_data.write_all(&(position_liquidity_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&position_liquidity_bytes)?;

        // Field 11: amount0 (U256)
        let amount0_bytes = format_numeric(&update.amount0).as_bytes().to_vec();
        row_data.write_all(&(amount0_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&amount0_bytes)?;

        // Field 12: amount1 (U256)
        let amount1_bytes = format_numeric(&update.amount1).as_bytes().to_vec();
        row_data.write_all(&(amount1_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&amount1_bytes)?;

        // Field 13: tick_lower (INT4)
        let tick_lower_bytes = update.tick_lower.to_be_bytes();
        row_data.write_all(&(tick_lower_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tick_lower_bytes)?;

        // Field 14: tick_upper (INT4)
        let tick_upper_bytes = update.tick_upper.to_be_bytes();
        row_data.write_all(&(tick_upper_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tick_upper_bytes)?;

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write pool liquidity update data: {e}"))?;
        Ok(())
    }

    /// Inserts pool fee collect events using PostgreSQL COPY BINARY for maximum performance.
    ///
    /// # Errors
    ///
    /// Returns an error if the COPY operation fails.
    pub async fn copy_pool_collects(
        &self,
        chain_id: u32,
        collects: &[PoolFeeCollect],
    ) -> anyhow::Result<()> {
        if collects.is_empty() {
            return Ok(());
        }

        let copy_statement = r"
            COPY pool_collect_event (
                chain_id, pool_address, block, transaction_hash, transaction_index,
                log_index, owner, amount0, amount1, tick_lower, tick_upper
            ) FROM STDIN WITH (FORMAT BINARY)";

        let mut copy_in = self
            .pool
            .copy_in_raw(copy_statement)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to start COPY operation: {e}"))?;

        // Write binary header
        self.write_copy_header(&mut copy_in).await?;

        // Write each collect event as binary data
        for collect in collects {
            self.write_pool_fee_collect_binary(&mut copy_in, chain_id, collect)
                .await?;
        }

        // Write binary trailer
        self.write_copy_trailer(&mut copy_in).await?;

        // Finish the COPY operation
        copy_in.finish().await.map_err(|e| {
            // Log detailed information about the failed batch
            tracing::error!("COPY operation failed for temp_pool_collect batch:");
            tracing::error!("  Chain ID: {}", chain_id);
            tracing::error!("  Batch size: {}", collects.len());

            if !collects.is_empty() {
                tracing::error!(
                    "  Block range: {} to {}",
                    collects.iter().map(|c| c.block).min().unwrap_or(0),
                    collects.iter().map(|c| c.block).max().unwrap_or(0)
                );
            }

            // Log first few collects with key details
            for (i, collect) in collects.iter().take(5).enumerate() {
                tracing::error!(
                    "  Collect[{}]: tx={} log_idx={} block={} pool={} owner={}",
                    i,
                    collect.transaction_hash,
                    collect.log_index,
                    collect.block,
                    collect.pool_address,
                    collect.owner
                );
            }

            if collects.len() > 5 {
                tracing::error!("  ... and {} more collects", collects.len() - 5);
            }

            anyhow::anyhow!("Failed to finish COPY operation: {e}")
        })?;

        Ok(())
    }

    /// Writes a single pool fee collect in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_pool_fee_collect_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        collect: &PoolFeeCollect,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (11)
        row_data.write_all(&11u16.to_be_bytes())?;

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // Field 2: pool_address (TEXT)
        let pool_address_bytes = collect.pool_address.to_string().as_bytes().to_vec();
        row_data.write_all(&(pool_address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&pool_address_bytes)?;

        // Field 3: block (INT8)
        let block_bytes = (collect.block as i64).to_be_bytes();
        row_data.write_all(&(block_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&block_bytes)?;

        // Field 4: transaction_hash (TEXT)
        let tx_hash_bytes = collect.transaction_hash.as_bytes();
        row_data.write_all(&(tx_hash_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(tx_hash_bytes)?;

        // Field 5: transaction_index (INT4)
        let tx_index_bytes = (collect.transaction_index as i32).to_be_bytes();
        row_data.write_all(&(tx_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tx_index_bytes)?;

        // Field 6: log_index (INT4)
        let log_index_bytes = (collect.log_index as i32).to_be_bytes();
        row_data.write_all(&(log_index_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&log_index_bytes)?;

        // Field 7: owner (TEXT)
        let owner_bytes = collect.owner.to_string().as_bytes().to_vec();
        row_data.write_all(&(owner_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&owner_bytes)?;

        // Field 8: amount0 (U256)
        let fee0_bytes = format_numeric(&collect.amount0).as_bytes().to_vec();
        row_data.write_all(&(fee0_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&fee0_bytes)?;

        // Field 9: amount1 (U256)
        let fee1_bytes = format_numeric(&collect.amount1).as_bytes().to_vec();
        row_data.write_all(&(fee1_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&fee1_bytes)?;

        // Field 10: tick_lower (INT4)
        let tick_lower_bytes = collect.tick_lower.to_be_bytes();
        row_data.write_all(&(tick_lower_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tick_lower_bytes)?;

        // Field 11: tick_upper (INT4)
        let tick_upper_bytes = collect.tick_upper.to_be_bytes();
        row_data.write_all(&(tick_upper_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&tick_upper_bytes)?;

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write pool fee collect data: {e}"))?;
        Ok(())
    }

    /// Writes a single token in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_token_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        token: &Token,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (5)
        row_data.write_all(&5u16.to_be_bytes())?;

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // Field 2: address (TEXT)
        let address_bytes = token.address.to_string().as_bytes().to_vec();
        row_data.write_all(&(address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&address_bytes)?;

        // Field 3: name (TEXT)
        let name_bytes = token.name.as_bytes();
        row_data.write_all(&(name_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(name_bytes)?;

        // Field 4: symbol (TEXT)
        let symbol_bytes = token.symbol.as_bytes();
        row_data.write_all(&(symbol_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(symbol_bytes)?;

        // Field 5: decimals (INT4)
        let decimals_bytes = (i32::from(token.decimals)).to_be_bytes();
        row_data.write_all(&(decimals_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&decimals_bytes)?;

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write token data: {e}"))?;
        Ok(())
    }

    /// Writes a single pool in PostgreSQL binary format.
    ///
    /// Each row in binary format consists of:
    /// - 2-byte field count
    /// - For each field: 4-byte length followed by data (or -1 for NULL)
    async fn write_pool_binary(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
        chain_id: u32,
        pool: &Pool,
    ) -> anyhow::Result<()> {
        use std::io::Write;
        let mut row_data = Vec::new();

        // Number of fields (12)
        row_data.write_all(&12u16.to_be_bytes())?;

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data.write_all(&(chain_id_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&chain_id_bytes)?;

        // Field 2: dex_name (TEXT)
        let dex_name_bytes = pool.dex.name.to_string().as_bytes().to_vec();
        row_data.write_all(&(dex_name_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&dex_name_bytes)?;

        // Field 3: address (TEXT)
        let address_bytes = pool.address.to_string().as_bytes().to_vec();
        row_data.write_all(&(address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&address_bytes)?;

        // Field 4: creation_block (INT8)
        let creation_block_bytes = (pool.creation_block as i64).to_be_bytes();
        row_data.write_all(&(creation_block_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&creation_block_bytes)?;

        // Field 5: token0_chain (INT4)
        let token0_chain_bytes = (pool.token0.chain.chain_id as i32).to_be_bytes();
        row_data.write_all(&(token0_chain_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&token0_chain_bytes)?;

        // Field 6: token0_address (TEXT)
        let token0_address_bytes = pool.token0.address.to_string().as_bytes().to_vec();
        row_data.write_all(&(token0_address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&token0_address_bytes)?;

        // Field 7: token1_chain (INT4)
        let token1_chain_bytes = (pool.token1.chain.chain_id as i32).to_be_bytes();
        row_data.write_all(&(token1_chain_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&token1_chain_bytes)?;

        // Field 8: token1_address (TEXT)
        let token1_address_bytes = pool.token1.address.to_string().as_bytes().to_vec();
        row_data.write_all(&(token1_address_bytes.len() as i32).to_be_bytes())?;
        row_data.write_all(&token1_address_bytes)?;

        // Field 9: fee (INT4, nullable)
        if let Some(fee) = pool.fee {
            let fee_bytes = (fee as i32).to_be_bytes();
            row_data.write_all(&(fee_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&fee_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // Field 10: tick_spacing (INT4, nullable)
        if let Some(tick_spacing) = pool.tick_spacing {
            let tick_spacing_bytes = (tick_spacing as i32).to_be_bytes();
            row_data.write_all(&(tick_spacing_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&tick_spacing_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // Field 11: initial_tick (INT4, nullable)
        if let Some(initial_tick) = pool.initial_tick {
            let initial_tick_bytes = initial_tick.to_be_bytes();
            row_data.write_all(&(initial_tick_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&initial_tick_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        // Field 12: initial_sqrt_price_x96 (TEXT, nullable)
        if let Some(ref initial_sqrt_price) = pool.initial_sqrt_price_x96 {
            let sqrt_price_bytes = format_numeric(initial_sqrt_price).as_bytes().to_vec();
            row_data.write_all(&(sqrt_price_bytes.len() as i32).to_be_bytes())?;
            row_data.write_all(&sqrt_price_bytes)?;
        } else {
            row_data.write_all(&(-1i32).to_be_bytes())?; // NULL
        }

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write pool data: {e}"))?;
        Ok(())
    }

    /// Writes the PostgreSQL COPY binary format trailer.
    ///
    /// The trailer is a 2-byte value of -1 to indicate end of data.
    async fn write_copy_trailer(
        &self,
        copy_in: &mut sqlx::postgres::PgCopyIn<sqlx::pool::PoolConnection<sqlx::Postgres>>,
    ) -> anyhow::Result<()> {
        // Binary trailer: -1 as i16 to indicate end of data
        let trailer = (-1i16).to_be_bytes();
        copy_in
            .send(trailer.to_vec())
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write COPY trailer: {e}"))?;
        Ok(())
    }
}
