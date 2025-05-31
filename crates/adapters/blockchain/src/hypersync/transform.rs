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

use hypersync_client::format::Hex;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::defi::{block::Block, hex::from_str_hex_to_u64};
use ustr::Ustr;

/// Converts a HyperSync block format to our internal Block type.
pub fn transform_hypersync_block(
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

    Ok(Block::new(
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
    ))
}
