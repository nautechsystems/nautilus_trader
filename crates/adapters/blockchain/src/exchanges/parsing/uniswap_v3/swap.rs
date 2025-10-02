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
    events::swap::SwapEvent,
    hypersync::helpers::{
        extract_address_from_topic, extract_block_number, extract_log_index,
        extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
    },
};

const SWAP_EVENT_SIGNATURE_HASH: &str =
    "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

// Define sol macro for easier parsing of Swap event data
// It contains 5 parameters of 32 bytes each:
// amount0 (int256), amount1 (int256), sqrtPriceX96 (uint160), liquidity (uint128), tick (int24)
sol! {
    struct SwapEventData {
        int256 amount0;
        int256 amount1;
        uint160 sqrt_price_x96;
        uint128 liquidity;
        int24 tick;
    }
}

/// Parses a swap event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_swap_event(dex: SharedDex, log: Log) -> anyhow::Result<SwapEvent> {
    validate_event_signature_hash("SwapEvent", SWAP_EVENT_SIGNATURE_HASH, &log)?;

    let sender = extract_address_from_topic(&log, 1, "sender")?;
    let recipient = extract_address_from_topic(&log, 2, "recipient")?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 5 parameters of 32 bytes each
        if data_bytes.len() < 5 * 32 {
            anyhow::bail!("Swap event data is too short");
        }

        // Decode the data using the SwapEventData struct
        let decoded = match <SwapEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode swap event data: {e}"),
        };
        let _ = decoded.amount0;
        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        Ok(SwapEvent::new(
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
            decoded.sqrt_price_x96,
            decoded.liquidity,
            decoded.tick.as_i32(),
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in swap event log"))
    }
}
