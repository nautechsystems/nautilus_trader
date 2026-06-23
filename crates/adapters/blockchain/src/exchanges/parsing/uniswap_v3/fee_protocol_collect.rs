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

use alloy::{dyn_abi::SolType, primitives::Address, sol};
use nautilus_model::defi::{PoolIdentifier, SharedDex, rpc::RpcLog};
use ustr::Ustr;

use crate::{
    events::fee_protocol_collect::FeeProtocolCollectEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, extract_log_index,
            extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
};

const FEE_PROTOCOL_COLLECT_EVENT_SIGNATURE_HASH: &str =
    "596b573906218d3411850b26a6b437d6c4522fdb43d2d2386263f86d50b8b151";

// Define sol macro for parsing CollectProtocol event data.
// sender and recipient are indexed (in topics); amount0 and amount1 are non-indexed (in data):
// amount0 (uint128), amount1 (uint128)
sol! {
    struct FeeProtocolCollectEventData {
        uint128 amount0;
        uint128 amount1;
    }
}

/// Parses a `CollectProtocol` event from a Uniswap V3 HyperSync log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_fee_protocol_collect_event_hypersync(
    dex: SharedDex,
    log: &HypersyncLog,
) -> anyhow::Result<FeeProtocolCollectEvent> {
    validate_event_signature_hash(
        "CollectProtocol",
        FEE_PROTOCOL_COLLECT_EVENT_SIGNATURE_HASH,
        log,
    )?;

    let sender = extract_address_from_topic(log, 1, "sender")?;
    let recipient = extract_address_from_topic(log, 2, "recipient")?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate the data contains 2 parameters of 32 bytes each
        if data_bytes.len() < 2 * 32 {
            anyhow::bail!("CollectProtocol event data is too short");
        }

        let decoded = match <FeeProtocolCollectEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode CollectProtocol event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));

        Ok(FeeProtocolCollectEvent::new(
            dex,
            pool_identifier,
            extract_block_number(log)?,
            extract_transaction_hash(log)?,
            extract_transaction_index(log)?,
            extract_log_index(log)?,
            sender,
            recipient,
            decoded.amount0,
            decoded.amount1,
        ))
    } else {
        anyhow::bail!("Missing data in CollectProtocol event log");
    }
}

/// Parses a `CollectProtocol` event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_fee_protocol_collect_event_rpc(
    dex: SharedDex,
    log: &RpcLog,
) -> anyhow::Result<FeeProtocolCollectEvent> {
    rpc_helpers::validate_event_signature(
        log,
        FEE_PROTOCOL_COLLECT_EVENT_SIGNATURE_HASH,
        "CollectProtocol",
    )?;

    let sender = rpc_helpers::extract_address_from_topic(log, 1, "sender")?;
    let recipient = rpc_helpers::extract_address_from_topic(log, 2, "recipient")?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    // Validate the data contains 2 parameters of 32 bytes each
    if data_bytes.len() < 2 * 32 {
        anyhow::bail!("CollectProtocol event data is too short");
    }

    let decoded = match <FeeProtocolCollectEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode CollectProtocol event data: {e}"),
    };

    let pool_address = rpc_helpers::extract_address(log)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
    Ok(FeeProtocolCollectEvent::new(
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
    ))
}

#[cfg(test)]
mod tests {
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::exchanges::arbitrum;

    // CollectProtocol log at block 0x175a6484 (391799940), pool
    // 0xd13040d4fe917EE704158CfCB3338dCd2838B245.
    // sender:    0xc36442b4a4522e871399cd717abdd847ab11fe88
    // recipient: 0xa61da382c18d9d5beb905ea192bae25e4c15d512
    // Asymmetric amounts (amount0 != amount1) catch a token0/token1 column swap.
    const EXPECTED_AMOUNT0: u128 = 0x1234_5678_90ab_cdef;
    const EXPECTED_AMOUNT1: u128 = 0x0fed_cba9_8765_4321;
    const DATA: &str = "0x0000000000000000000000000000000000000000000000001234567890abcdef0000000000000000000000000000000000000000000000000fedcba987654321";

    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x11",
            "transaction_index": "0x5",
            "transaction_hash": "0x0c70f6d6bcf8508ba620b9d1250c95ad67108e35707c5d7456349ea207051bae",
            "block_hash": null,
            "block_number": "0x175a6484",
            "address": "0xd13040d4fe917EE704158CfCB3338dCd2838B245",
            "data": DATA,
            "topics": [
                "0x596b573906218d3411850b26a6b437d6c4522fdb43d2d2386263f86d50b8b151",
                "0x000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88",
                "0x000000000000000000000000a61da382c18d9d5beb905ea192bae25e4c15d512"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x11",
            "transactionIndex": "0x5",
            "transactionHash": "0x0c70f6d6bcf8508ba620b9d1250c95ad67108e35707c5d7456349ea207051bae",
            "blockHash": "0xe925eaa1f5178ceedfa24043a974edb928ddab7195600b6b99ff5403fbf13c8b",
            "blockNumber": "0x175a6484",
            "address": "0xd13040d4fe917EE704158CfCB3338dCd2838B245",
            "data": DATA,
            "topics": [
                "0x596b573906218d3411850b26a6b437d6c4522fdb43d2d2386263f86d50b8b151",
                "0x000000000000000000000000c36442b4a4522e871399cd717abdd847ab11fe88",
                "0x000000000000000000000000a61da382c18d9d5beb905ea192bae25e4c15d512"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_fee_protocol_collect_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_fee_protocol_collect_event_hypersync(dex, &hypersync_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0xd13040d4fe917EE704158CfCB3338dCd2838B245"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0xc36442b4a4522e871399cd717abdd847ab11fe88"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xa61da382c18d9d5beb905ea192bae25e4c15d512"
        );
        assert_eq!(event.amount0, EXPECTED_AMOUNT0);
        assert_eq!(event.amount1, EXPECTED_AMOUNT1);
        assert_eq!(event.block_number, 391_799_940);
        assert_eq!(event.transaction_index, 5);
        assert_eq!(event.log_index, 17);
    }

    #[rstest]
    fn test_parse_fee_protocol_collect_event_rpc(rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_fee_protocol_collect_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0xd13040d4fe917EE704158CfCB3338dCd2838B245"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0xc36442b4a4522e871399cd717abdd847ab11fe88"
        );
        assert_eq!(
            event.recipient.to_string().to_lowercase(),
            "0xa61da382c18d9d5beb905ea192bae25e4c15d512"
        );
        assert_eq!(event.amount0, EXPECTED_AMOUNT0);
        assert_eq!(event.amount1, EXPECTED_AMOUNT1);
        assert_eq!(event.block_number, 391_799_940);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event_hypersync =
            parse_fee_protocol_collect_event_hypersync(dex.clone(), &hypersync_log).unwrap();
        let event_rpc = parse_fee_protocol_collect_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_identifier, event_rpc.pool_identifier);
        assert_eq!(event_hypersync.sender, event_rpc.sender);
        assert_eq!(event_hypersync.recipient, event_rpc.recipient);
        assert_eq!(event_hypersync.amount0, event_rpc.amount0);
        assert_eq!(event_hypersync.amount1, event_rpc.amount1);
        assert_eq!(event_hypersync.block_number, event_rpc.block_number);
        assert_eq!(event_hypersync.transaction_hash, event_rpc.transaction_hash);
        assert_eq!(event_hypersync.log_index, event_rpc.log_index);
    }
}
