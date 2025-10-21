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

use alloy::primitives::U256;
use hypersync_client::format::Hex;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::defi::{Block, Blockchain, hex::from_str_hex_to_u64};
use ustr::Ustr;

/// Converts a HyperSync block format to our internal [`Block`] type.
///
/// # Errors
///
/// Returns an error if required block fields are missing or if hex parsing fails.
pub fn transform_hypersync_block(
    chain: Blockchain,
    received_block: hypersync_client::simple_types::Block,
) -> Result<Block, anyhow::Error> {
    let number = received_block
        .number
        .ok_or_else(|| anyhow::anyhow!("Missing block number"))?;
    let gas_limit = from_str_hex_to_u64(
        received_block
            .gas_limit
            .ok_or_else(|| anyhow::anyhow!("Missing gas limit"))?
            .encode_hex()
            .as_str(),
    )?;
    let gas_used = from_str_hex_to_u64(
        received_block
            .gas_used
            .ok_or_else(|| anyhow::anyhow!("Missing gas used"))?
            .encode_hex()
            .as_str(),
    )?;
    let timestamp = from_str_hex_to_u64(
        received_block
            .timestamp
            .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?
            .encode_hex()
            .as_str(),
    )?;

    let mut block = Block::new(
        received_block
            .hash
            .ok_or_else(|| anyhow::anyhow!("Missing hash"))?
            .to_string(),
        received_block
            .parent_hash
            .ok_or_else(|| anyhow::anyhow!("Missing parent hash"))?
            .to_string(),
        number,
        Ustr::from(
            received_block
                .miner
                .ok_or_else(|| anyhow::anyhow!("Missing miner"))?
                .to_string()
                .as_str(),
        ),
        gas_limit,
        gas_used,
        UnixNanos::new(timestamp * NANOSECONDS_IN_SECOND),
        Some(chain),
    );

    if let Some(base_fee_hex) = received_block.base_fee_per_gas {
        let s = base_fee_hex.encode_hex();
        let val = U256::from_str_radix(s.trim_start_matches("0x"), 16)?;
        block = block.with_base_fee(val);
    }

    if let (Some(used_hex), Some(excess_hex)) =
        (received_block.blob_gas_used, received_block.excess_blob_gas)
    {
        let used = U256::from_str_radix(used_hex.encode_hex().trim_start_matches("0x"), 16)?;
        let excess = U256::from_str_radix(excess_hex.encode_hex().trim_start_matches("0x"), 16)?;
        block = block.with_blob_gas(used, excess);
    }

    // TODO: HyperSync does not yet publish L1 gas metadata fields
    // if let (Some(price_hex), Some(l1_used_hex), Some(scalar_hex)) = (
    //     received_block.l1_gas_price,
    //     received_block.l1_gas_used,
    //     received_block.l1_fee_scalar,
    // ) {
    //     let price = U256::from_str_radix(price_hex.encode_hex().trim_start_matches("0x"), 16)?;
    //     let used = from_str_hex_to_u64(l1_used_hex.encode_hex().as_str())?;
    //     let scalar = from_str_hex_to_u64(scalar_hex.encode_hex().as_str())?;
    //     block = block.with_l1_fee_components(price, used, scalar);
    // }

    Ok(block)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {}
