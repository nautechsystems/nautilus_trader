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
    events::flash::FlashEvent,
    hypersync::helpers::{
        extract_address_from_topic, extract_block_number, extract_log_index,
        extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
    },
};

// Placeholder hash - will be calculated properly later
const FLASH_EVENT_SIGNATURE_HASH: &str =
    "bdbdb71d7860376ba52b25a5028beea23581364a40522f6bcfb86bb1f2dca633";

// Define sol macro for easier parsing of Flash event data
// event Flash(address indexed sender, address indexed recipient, uint256 amount0, uint256 amount1, uint256 paid0, uint256 paid1)
sol! {
    struct FlashEventData {
        uint256 amount0;
        uint256 amount1;
        uint256 paid0;
        uint256 paid1;
    }
}

/// Parses a flash event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_flash_event(dex: SharedDex, log: Log) -> anyhow::Result<FlashEvent> {
    validate_event_signature_hash("FlashEvent", FLASH_EVENT_SIGNATURE_HASH, &log)?;

    let sender = extract_address_from_topic(&log, 1, "sender")?;
    let recipient = extract_address_from_topic(&log, 2, "recipient")?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 4 parameters of 32 bytes each
        if data_bytes.len() < 4 * 32 {
            anyhow::bail!("Flash event data is too short");
        }

        // Decode the data using the FlashEventData struct
        let decoded = match <FlashEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode flash event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );

        Ok(FlashEvent::new(
            dex,
            pool_address,
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            sender,
            recipient,
            decoded.amount0,
            decoded.amount1,
            decoded.paid0,
            decoded.paid1,
        ))
    } else {
        anyhow::bail!("Missing data in flash event log");
    }
}
