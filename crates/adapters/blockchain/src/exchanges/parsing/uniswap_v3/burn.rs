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

use alloy::{dyn_abi::SolType, primitives::Address, sol};
use hypersync_client::simple_types::Log;
use nautilus_model::defi::SharedDex;

use crate::{
    events::burn::BurnEvent,
    hypersync::helpers::{
        extract_address_from_topic, extract_block_number, extract_log_index,
        extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
    },
};

const BURN_EVENT_SIGNATURE_HASH: &str =
    "0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c";

// Define sol macro for easier parsing of Burn event data
// It contains 3 parameters of 32 bytes each:
// amount (uint128), amount0 (uint256), amount1 (uint256)
sol! {
    struct BurnEventData {
        uint128 amount;
        uint256 amount0;
        uint256 amount1;
    }
}

/// Parses a burn event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_burn_event(dex: SharedDex, log: Log) -> anyhow::Result<BurnEvent> {
    validate_event_signature_hash("Burn", BURN_EVENT_SIGNATURE_HASH, &log)?;

    let owner = extract_address_from_topic(&log, 1, "owner")?;

    // Extract int24 tickLower from topic2 (stored as a 32-byte padded value)
    let tick_lower = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_lower_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_lower_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickLower in topic2 when parsing burn event"),
    };

    // Extract int24 tickUpper from topic3 (stored as a 32-byte padded value)
    let tick_upper = match log.topics.get(3).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_upper_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_upper_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickUpper in topic3 when parsing burn event"),
    };

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 3 parameters of 32 bytes each
        if data_bytes.len() < 3 * 32 {
            anyhow::bail!("Burn event data is too short");
        }

        // Decode the data using the BurnEventData struct
        let decoded = match <BurnEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode burn event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        Ok(BurnEvent::new(
            dex,
            pool_address,
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            owner,
            tick_lower,
            tick_upper,
            decoded.amount,
            decoded.amount0,
            decoded.amount1,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in burn event log"))
    }
}
