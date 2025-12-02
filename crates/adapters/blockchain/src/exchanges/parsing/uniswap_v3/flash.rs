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
use nautilus_model::defi::{PoolIdentifier, SharedDex, rpc::RpcLog};
use ustr::Ustr;

use crate::{
    events::flash::FlashEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, extract_log_index,
            extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
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
pub fn parse_flash_event_hypersync(
    dex: SharedDex,
    log: HypersyncLog,
) -> anyhow::Result<FlashEvent> {
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
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));

        Ok(FlashEvent::new(
            dex,
            pool_identifier,
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

/// Parses a flash event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_flash_event_rpc(dex: SharedDex, log: &RpcLog) -> anyhow::Result<FlashEvent> {
    rpc_helpers::validate_event_signature(log, FLASH_EVENT_SIGNATURE_HASH, "Flash")?;

    let sender = rpc_helpers::extract_address_from_topic(log, 1, "sender")?;
    let recipient = rpc_helpers::extract_address_from_topic(log, 2, "recipient")?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    // Validate if data contains 4 parameters of 32 bytes each
    if data_bytes.len() < 4 * 32 {
        anyhow::bail!("Flash event data is too short");
    }

    // Decode the data using the FlashEventData struct
    let decoded = match <FlashEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode flash event data: {e}"),
    };

    let pool_address = rpc_helpers::extract_address(log)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
    Ok(FlashEvent::new(
        dex,
        pool_identifier,
        rpc_helpers::extract_block_number(log)?,
        rpc_helpers::extract_transaction_hash(log)?,
        rpc_helpers::extract_transaction_index(log)?,
        rpc_helpers::extract_log_index(log)?,
        sender,
        recipient,
        decoded.amount0,
        decoded.amount1,
        decoded.paid0,
        decoded.paid1,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use alloy::primitives::U256;
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::exchanges::arbitrum;

    /// Real HyperSync log from Arbitrum Flash event at block 0xfe9d5ce (266982862)
    /// Pool: 0x4cef551255ec96d89fec975446301b5c4e164c59
    /// sender: 0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b
    /// recipient: 0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b
    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x3b",
            "transaction_index": "0x4",
            "transaction_hash": "0x4d345a8cae1e39654904bb7ca04e552b0fc8728ed68a28563ea4b151b96262aa",
            "block_hash": null,
            "block_number": "0xfe9d5ce",
            "address": "0x4CEf551255EC96d89feC975446301b5C4e164C59",
            "data": "0x00000000000000000000000000000000000000000000002c55804c34816b99060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000220c6ab9806365120000000000000000000000000000000000000000000000000000000000000000",
            "topics": [
                "0xbdbdb71d7860376ba52b25a5028beea23581364a40522f6bcfb86bb1f2dca633",
                "0x000000000000000000000000f3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b",
                "0x000000000000000000000000f3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    /// Real RPC log from Arbitrum Flash event at block 0xfe9d5ce (266982862)
    #[fixture]
    fn rpc_log() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x3b",
            "transactionIndex": "0x4",
            "transactionHash": "0x4d345a8cae1e39654904bb7ca04e552b0fc8728ed68a28563ea4b151b96262aa",
            "blockHash": "0xf10a01cbc75fccad0384a7447f37f06bfb01fbd08d7541a6e5f558ff9bc31ea4",
            "blockNumber": "0xfe9d5ce",
            "address": "0x4CEf551255EC96d89feC975446301b5C4e164C59",
            "data": "0x00000000000000000000000000000000000000000000002c55804c34816b99060000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000220c6ab9806365120000000000000000000000000000000000000000000000000000000000000000",
            "topics": [
                "0xbdbdb71d7860376ba52b25a5028beea23581364a40522f6bcfb86bb1f2dca633",
                "0x000000000000000000000000f3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b",
                "0x000000000000000000000000f3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_flash_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_flash_event_hypersync(dex, hypersync_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x4CEf551255EC96d89feC975446301b5C4e164C59"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
        );
        let expected_amount0 = U256::from_str_radix("2c55804c34816b9906", 16).unwrap();
        assert_eq!(event.amount0, expected_amount0);
        assert_eq!(event.amount1, U256::ZERO);
        let expected_paid0 = U256::from_str_radix("220c6ab980636512", 16).unwrap();
        assert_eq!(event.paid0, expected_paid0);
        assert_eq!(event.paid1, U256::ZERO);
        assert_eq!(event.block_number, 266982862);
    }

    #[rstest]
    fn test_parse_flash_event_rpc(rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_flash_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x4CEf551255EC96d89feC975446301b5C4e164C59"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xf3f521ee74debaa28fd0ea1e8ca2fd8d6c110d8b"
        );
        let expected_amount0 = U256::from_str_radix("2c55804c34816b9906", 16).unwrap();
        assert_eq!(event.amount0, expected_amount0);
        assert_eq!(event.amount1, U256::ZERO);
        let expected_paid0 = U256::from_str_radix("220c6ab980636512", 16).unwrap();
        assert_eq!(event.paid0, expected_paid0);
        assert_eq!(event.paid1, U256::ZERO);
        assert_eq!(event.block_number, 266982862);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event_hypersync = parse_flash_event_hypersync(dex.clone(), hypersync_log).unwrap();
        let event_rpc = parse_flash_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_identifier, event_rpc.pool_identifier);
        assert_eq!(event_hypersync.sender, event_rpc.sender);
        assert_eq!(event_hypersync.recipient, event_rpc.recipient);
        assert_eq!(event_hypersync.amount0, event_rpc.amount0);
        assert_eq!(event_hypersync.amount1, event_rpc.amount1);
        assert_eq!(event_hypersync.paid0, event_rpc.paid0);
        assert_eq!(event_hypersync.paid1, event_rpc.paid1);
        assert_eq!(event_hypersync.block_number, event_rpc.block_number);
        assert_eq!(event_hypersync.transaction_hash, event_rpc.transaction_hash);
    }
}
