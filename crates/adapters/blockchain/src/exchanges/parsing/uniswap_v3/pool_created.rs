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
use hypersync_client::simple_types::Log;

use crate::{
    events::pool_created::PoolCreatedEvent,
    hypersync::helpers::{
        extract_address_from_topic, extract_block_number, validate_event_signature_hash,
    },
};

const POOL_CREATED_EVENT_SIGNATURE_HASH: &str =
    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";

/// Parses a pool creation event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the block number is not set in the log.
pub fn parse_pool_created_event(log: Log) -> anyhow::Result<PoolCreatedEvent> {
    validate_event_signature_hash("PoolCreatedEvent", POOL_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = extract_block_number(&log)?;

    let token = extract_address_from_topic(&log, 1, "token0")?;
    let token1 = extract_address_from_topic(&log, 2, "token1")?;

    let fee = if let Some(topic) = log.topics.get(3).and_then(|t| t.as_ref()) {
        U256::from_be_slice(topic.as_ref()).as_limbs()[0] as u32
    } else {
        anyhow::bail!("Missing fee in topic3 when parsing pool created event");
    };

    if let Some(data) = log.data {
        // Data contains: [tick_spacing (32 bytes), pool_address (32 bytes)]
        let data_bytes = data.as_ref();

        // Extract tick_spacing (first 32 bytes)
        let tick_spacing_bytes: [u8; 32] = data_bytes[0..32].try_into()?;
        let tick_spacing = u32::from_be_bytes(tick_spacing_bytes[28..32].try_into()?);

        // Extract pool_address (next 32 bytes)
        let pool_address_bytes: [u8; 32] = data_bytes[32..64].try_into()?;
        let pool_address = Address::from_slice(&pool_address_bytes[12..32]);

        Ok(PoolCreatedEvent::new(
            block_number.into(),
            token,
            token1,
            pool_address,
            Some(fee),
            Some(tick_spacing),
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in pool created event log"))
    }
}
