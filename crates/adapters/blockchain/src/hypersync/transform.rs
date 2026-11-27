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

use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::defi::{Block, Blockchain};
use ustr::Ustr;

use super::HypersyncBlock;

/// Converts a HyperSync block to the NautilusTrader domain type.
///
/// # Errors
///
/// Returns an error if a required block field is absent or the timestamp overflows `u64` nanoseconds.
pub fn transform_hypersync_block(
    chain: Blockchain,
    received_block: HypersyncBlock,
) -> anyhow::Result<Block> {
    let number = received_block
        .number
        .ok_or_else(|| anyhow::anyhow!("Missing block number"))?;
    let gas_limit = received_block
        .gas_limit
        .ok_or_else(|| anyhow::anyhow!("Missing gas limit"))?;
    let gas_used = received_block
        .gas_used
        .ok_or_else(|| anyhow::anyhow!("Missing gas used"))?;
    let timestamp = received_block
        .timestamp
        .and_then(|value| value.checked_mul(NANOSECONDS_IN_SECOND))
        .ok_or_else(|| anyhow::anyhow!("Missing or out-of-range block timestamp"))?;
    let hash = received_block
        .hash
        .ok_or_else(|| anyhow::anyhow!("Missing hash"))?;
    let parent_hash = received_block
        .parent_hash
        .ok_or_else(|| anyhow::anyhow!("Missing parent hash"))?;
    let miner = received_block
        .miner
        .ok_or_else(|| anyhow::anyhow!("Missing miner"))?;
    let hash = hash.to_string();
    let parent_hash = parent_hash.to_string();
    let miner = Ustr::from(format!("{miner:#x}").as_str());
    let ts_event = UnixNanos::new(timestamp);

    let mut block = Block::new(
        hash,
        parent_hash,
        number,
        miner,
        gas_limit,
        gas_used,
        ts_event,
        Some(chain),
    );

    if let Some(base_fee) = received_block.base_fee_per_gas {
        block = block.with_base_fee(base_fee);
    }

    if let (Some(used), Some(excess)) =
        (received_block.blob_gas_used, received_block.excess_blob_gas)
    {
        block = block.with_blob_gas(used, excess);
    }

    Ok(block)
}
