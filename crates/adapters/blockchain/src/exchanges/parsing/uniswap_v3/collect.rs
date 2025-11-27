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
use nautilus_model::defi::{SharedDex, rpc::RpcLog};

use crate::{
    events::collect::CollectEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, extract_log_index,
            extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
};

const COLLECT_EVENT_SIGNATURE_HASH: &str =
    "70935338e69775456a85ddef226c395fb668b63fa0115f5f20610b388e6ca9c0";

// Define sol macro for easier parsing of Collect event data
// It contains 3 parameters of 32 bytes each:
// recipient (address), amount0 (uint128), amount1 (uint128)
sol! {
    struct CollectEventData {
        address recipient;
        uint128 amount0;
        uint128 amount1;
    }
}

/// Parses a collect event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_collect_event_hypersync(
    dex: SharedDex,
    log: HypersyncLog,
) -> anyhow::Result<CollectEvent> {
    validate_event_signature_hash("Collect", COLLECT_EVENT_SIGNATURE_HASH, &log)?;

    let owner = extract_address_from_topic(&log, 1, "owner")?;

    // Extract int24 tickLower from topic2 (stored as a 32-byte padded value)
    let tick_lower = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_lower_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_lower_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickLower in topic2 when parsing collect event"),
    };

    // Extract int24 tickUpper from topic3 (stored as a 32-byte padded value)
    let tick_upper = match log.topics.get(3).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_upper_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_upper_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickUpper in topic3 when parsing collect event"),
    };

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 3 parameters of 32 bytes each
        if data_bytes.len() < 3 * 32 {
            anyhow::bail!("Collect event data is too short");
        }

        // Decode the data using the CollectEventData struct
        let decoded = match <CollectEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode collect event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        Ok(CollectEvent::new(
            dex,
            pool_address,
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            owner,
            decoded.recipient,
            tick_lower,
            tick_upper,
            decoded.amount0,
            decoded.amount1,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in collect event log"))
    }
}

/// Parses a collect event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_collect_event_rpc(dex: SharedDex, log: &RpcLog) -> anyhow::Result<CollectEvent> {
    rpc_helpers::validate_event_signature(log, COLLECT_EVENT_SIGNATURE_HASH, "Collect")?;

    let owner = rpc_helpers::extract_address_from_topic(log, 1, "owner")?;

    // Extract int24 tickLower from topic2 (stored as a 32-byte padded value)
    let tick_lower_bytes = rpc_helpers::extract_topic_bytes(log, 2)?;
    let tick_lower = i32::from_be_bytes(tick_lower_bytes[28..32].try_into()?);

    // Extract int24 tickUpper from topic3 (stored as a 32-byte padded value)
    let tick_upper_bytes = rpc_helpers::extract_topic_bytes(log, 3)?;
    let tick_upper = i32::from_be_bytes(tick_upper_bytes[28..32].try_into()?);

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    // Validate if data contains 3 parameters of 32 bytes each
    if data_bytes.len() < 3 * 32 {
        anyhow::bail!("Collect event data is too short");
    }

    // Decode the data using the CollectEventData struct
    let decoded = match <CollectEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode collect event data: {e}"),
    };

    Ok(CollectEvent::new(
        dex,
        rpc_helpers::extract_address(log)?,
        rpc_helpers::extract_block_number(log)?,
        rpc_helpers::extract_transaction_hash(log)?,
        rpc_helpers::extract_transaction_index(log)?,
        rpc_helpers::extract_log_index(log)?,
        owner,
        decoded.recipient,
        tick_lower,
        tick_upper,
        decoded.amount0,
        decoded.amount1,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::exchanges::arbitrum;

    /// Real HyperSync log from Arbitrum Collect event at block 0x175a6484 (391799940)
    /// Pool: 0xd13040d4fe917ee704158cfcb3338dcd2838b245
    /// owner: 0xc36442b4a4522e871399cd717abdd847ab11fe88 (NonfungiblePositionManager)
    /// recipient: 0xa61da382c18d9d5beb905ea192bae25e4c15d512
    /// tickLower: -139767 (0xfffdde09)
    /// tickUpper: -139764 (0xfffdde0c)
    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x11",
            "transaction_index": "0x5",
            "transaction_hash": "0x0c70f6d6bcf8508ba620b9d1250c95ad67108e35707c5d7456349ea207051bae",
            "block_hash": null,
            "block_number": "0x175a6484",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0x000000000000000000000000a61da382c18d9d5beb905ea192bae25e4c15d5120000000000000000000000000000000000000000000000bf28af828dd3ae56340000000000000000000000000000000000000000000000000665eae21b1cffc8",
            "topics": [
                "0x70935338e69775456a85ddef226c395fb668b63fa0115f5f20610b388e6ca9c0",
                "0x000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88",
                "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffdde09",
                "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffdde0c"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    /// Real RPC log from Arbitrum Collect event at block 0x175a6484 (391799940)
    #[fixture]
    fn rpc_log() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x11",
            "transactionIndex": "0x5",
            "transactionHash": "0x0c70f6d6bcf8508ba620b9d1250c95ad67108e35707c5d7456349ea207051bae",
            "blockHash": "0xe925eaa1f5178ceedfa24043a974edb928ddab7195600b6b99ff5403fbf13c8b",
            "blockNumber": "0x175a6484",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0x000000000000000000000000a61da382c18d9d5beb905ea192bae25e4c15d5120000000000000000000000000000000000000000000000bf28af828dd3ae56340000000000000000000000000000000000000000000000000665eae21b1cffc8",
            "topics": [
                "0x70935338e69775456a85ddef226c395fb668b63fa0115f5f20610b388e6ca9c0",
                "0x000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88",
                "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffdde09",
                "0xfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffdde0c"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_collect_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_collect_event_hypersync(dex, hypersync_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        assert_eq!(
            event.owner.to_string().to_lowercase(),
            "0xc36442b4a4522e871399cd717abdd847ab11fe88"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xa61da382c18d9d5beb905ea192bae25e4c15d512"
        );
        assert_eq!(event.tick_lower, -139767);
        assert_eq!(event.tick_upper, -139764);
        let expected_amount0 = u128::from_str_radix("bf28af828dd3ae5634", 16).unwrap();
        assert_eq!(event.amount0, expected_amount0);
        let expected_amount1 = u128::from_str_radix("665eae21b1cffc8", 16).unwrap();
        assert_eq!(event.amount1, expected_amount1);
        assert_eq!(event.block_number, 391799940);
    }

    #[rstest]
    fn test_parse_collect_event_rpc(rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_collect_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        assert_eq!(
            event.owner.to_string().to_lowercase(),
            "0xc36442b4a4522e871399cd717abdd847ab11fe88"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xa61da382c18d9d5beb905ea192bae25e4c15d512"
        );
        assert_eq!(event.tick_lower, -139767);
        assert_eq!(event.tick_upper, -139764);
        let expected_amount0 = u128::from_str_radix("bf28af828dd3ae5634", 16).unwrap();
        assert_eq!(event.amount0, expected_amount0);
        let expected_amount1 = u128::from_str_radix("665eae21b1cffc8", 16).unwrap();
        assert_eq!(event.amount1, expected_amount1);
        assert_eq!(event.block_number, 391799940);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event_hypersync = parse_collect_event_hypersync(dex.clone(), hypersync_log).unwrap();
        let event_rpc = parse_collect_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_address, event_rpc.pool_address);
        assert_eq!(event_hypersync.owner, event_rpc.owner);
        assert_eq!(event_hypersync.recipient, event_rpc.recipient);
        assert_eq!(event_hypersync.tick_lower, event_rpc.tick_lower);
        assert_eq!(event_hypersync.tick_upper, event_rpc.tick_upper);
        assert_eq!(event_hypersync.amount0, event_rpc.amount0);
        assert_eq!(event_hypersync.amount1, event_rpc.amount1);
        assert_eq!(event_hypersync.block_number, event_rpc.block_number);
        assert_eq!(event_hypersync.transaction_hash, event_rpc.transaction_hash);
    }
}
