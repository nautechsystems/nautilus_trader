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

use nautilus_model::defi::Block;
use sqlx::{PgPool, postgres::PgPoolCopyExt};

/// Handles PostgreSQL COPY BINARY operations for blockchain data.
#[derive(Debug)]
pub struct PostgresCopyHandler<'a> {
    pool: &'a PgPool,
}

impl<'a> PostgresCopyHandler<'a> {
    /// Creates a new COPY handler with a reference to the database pool.
    pub fn new(pool: &'a PgPool) -> Self {
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
        header.write_all(b"PGCOPY\n\xff\r\n\0").unwrap(); // Signature
        header.write_all(&[0, 0, 0, 0]).unwrap(); // Flags field
        header.write_all(&[0, 0, 0, 0]).unwrap(); // Header extension length

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
        row_data.write_all(&14u16.to_be_bytes()).unwrap();

        // Field 1: chain_id (INT4)
        let chain_id_bytes = (chain_id as i32).to_be_bytes();
        row_data
            .write_all(&(chain_id_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&chain_id_bytes).unwrap();

        // Field 2: number (INT8)
        let number_bytes = (block.number as i64).to_be_bytes();
        row_data
            .write_all(&(number_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&number_bytes).unwrap();

        // Field 3: hash (TEXT)
        let hash_bytes = block.hash.as_bytes();
        row_data
            .write_all(&(hash_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(hash_bytes).unwrap();

        // Field 4: parent_hash (TEXT)
        let parent_hash_bytes = block.parent_hash.as_bytes();
        row_data
            .write_all(&(parent_hash_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(parent_hash_bytes).unwrap();

        // Field 5: miner (TEXT)
        let miner_bytes = block.miner.to_string().as_bytes().to_vec();
        row_data
            .write_all(&(miner_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&miner_bytes).unwrap();

        // Field 6: gas_limit (INT8)
        let gas_limit_bytes = (block.gas_limit as i64).to_be_bytes();
        row_data
            .write_all(&(gas_limit_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&gas_limit_bytes).unwrap();

        // Field 7: gas_used (INT8)
        let gas_used_bytes = (block.gas_used as i64).to_be_bytes();
        row_data
            .write_all(&(gas_used_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&gas_used_bytes).unwrap();

        // Field 8: timestamp (TEXT)
        let timestamp_bytes = block.timestamp.to_string().as_bytes().to_vec();
        row_data
            .write_all(&(timestamp_bytes.len() as i32).to_be_bytes())
            .unwrap();
        row_data.write_all(&timestamp_bytes).unwrap();

        // Field 9: base_fee_per_gas (TEXT, nullable)
        if let Some(ref base_fee) = block.base_fee_per_gas {
            let base_fee_bytes = base_fee.to_string().as_bytes().to_vec();
            row_data
                .write_all(&(base_fee_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&base_fee_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        // Field 10: blob_gas_used (TEXT, nullable)
        if let Some(ref blob_gas) = block.blob_gas_used {
            let blob_gas_bytes = blob_gas.to_string().as_bytes().to_vec();
            row_data
                .write_all(&(blob_gas_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&blob_gas_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        // Field 11: excess_blob_gas (TEXT, nullable)
        if let Some(ref excess_blob) = block.excess_blob_gas {
            let excess_blob_bytes = excess_blob.to_string().as_bytes().to_vec();
            row_data
                .write_all(&(excess_blob_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&excess_blob_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        // Field 12: l1_gas_price (TEXT, nullable)
        if let Some(ref l1_gas_price) = block.l1_gas_price {
            let l1_gas_price_bytes = l1_gas_price.to_string().as_bytes().to_vec();
            row_data
                .write_all(&(l1_gas_price_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&l1_gas_price_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        // Field 13: l1_gas_used (INT8, nullable)
        if let Some(l1_gas_used) = block.l1_gas_used {
            let l1_gas_used_bytes = (l1_gas_used as i64).to_be_bytes();
            row_data
                .write_all(&(l1_gas_used_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&l1_gas_used_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        // Field 14: l1_fee_scalar (INT8, nullable)
        if let Some(l1_fee_scalar) = block.l1_fee_scalar {
            let l1_fee_scalar_bytes = (l1_fee_scalar as i64).to_be_bytes();
            row_data
                .write_all(&(l1_fee_scalar_bytes.len() as i32).to_be_bytes())
                .unwrap();
            row_data.write_all(&l1_fee_scalar_bytes).unwrap();
        } else {
            row_data.write_all(&(-1i32).to_be_bytes()).unwrap(); // NULL value
        }

        copy_in
            .send(row_data)
            .await
            .map_err(|e| anyhow::anyhow!("Failed to write block data: {e}"))?;
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
