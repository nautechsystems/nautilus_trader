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

use alloy::primitives::Address;
use nautilus_model::defi::{PoolIdentifier, rpc::RpcLog};
use ustr::Ustr;

use crate::{
    events::pool_created::PoolCreatedEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
};

const PAIR_CREATED_EVENT_SIGNATURE_HASH: &str =
    "0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9";

/// Parses a UniswapV2 PairCreated event from a HyperSync log.
///
/// UniswapV2 emits PairCreated with:
/// - topic0: event signature
/// - topic1: token0 (indexed)
/// - topic2: token1 (indexed)
/// - data: pair address (32 bytes) + pair count (32 bytes)
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the block number is not set in the log.
pub fn parse_pool_created_event_hypersync(log: HypersyncLog) -> anyhow::Result<PoolCreatedEvent> {
    validate_event_signature_hash("PairCreatedEvent", PAIR_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = extract_block_number(&log)?;
    let token0 = extract_address_from_topic(&log, 1, "token0")?;
    let token1 = extract_address_from_topic(&log, 2, "token1")?;

    if let Some(data) = log.data {
        // Data contains: [pair_address (32 bytes), pair_count (32 bytes)]
        let data_bytes = data.as_ref();

        anyhow::ensure!(
            data_bytes.len() >= 32,
            "PairCreated event data too short: expected at least 32 bytes, got {}",
            data_bytes.len()
        );

        // Extract pair address (first 32 bytes, address is right-aligned)
        let pair_address = Address::from_slice(&data_bytes[12..32]);
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pair_address.to_string()));

        Ok(PoolCreatedEvent::new(
            block_number,
            token0,
            token1,
            pair_address,
            pool_identifier, // For V2/V3, pool_identifier = pool_address
            None,            // V2 has no fee tiers (fixed 0.3%)
            None,            // V2 has no tick spacing (CPAMM)
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in pair created event log"))
    }
}

/// Parses a UniswapV2 PairCreated event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_pool_created_event_rpc(log: &RpcLog) -> anyhow::Result<PoolCreatedEvent> {
    rpc_helpers::validate_event_signature(
        log,
        PAIR_CREATED_EVENT_SIGNATURE_HASH,
        "PairCreatedEvent",
    )?;

    let block_number = rpc_helpers::extract_block_number(log)?;
    let token0 = rpc_helpers::extract_address_from_topic(log, 1, "token0")?;
    let token1 = rpc_helpers::extract_address_from_topic(log, 2, "token1")?;

    // Extract pair address from data
    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    anyhow::ensure!(
        data_bytes.len() >= 32,
        "PairCreated event data too short: expected at least 32 bytes, got {}",
        data_bytes.len()
    );

    // Pair address is in the first 32 bytes (right-aligned)
    let pair_address = Address::from_slice(&data_bytes[12..32]);
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pair_address.to_string()));

    Ok(PoolCreatedEvent::new(
        block_number,
        token0,
        token1,
        pair_address,
        pool_identifier, // For V2/V3, pool_identifier = pool_address
        None,            // V2 has no fee tiers
        None,            // V2 has no tick spacing
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::{fixture, rstest};
    use serde_json::json;

    use super::*;

    // Real UniswapV2 PairCreated event from Arbitrum
    // Pair: WETH-USDC
    // Block: 0x8fcb296 (150582934)
    // Tx: 0xe7b5c25477c6dd2425c4bc07547ffb2777e018a12eed1d348d7bf553913d97b7

    #[fixture]
    fn hypersync_log_weth_usdt() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x0",
            "transaction_index": "0x1",
            "transaction_hash": "0xe7b5c25477c6dd2425c4bc07547ffb2777e018a12eed1d348d7bf553913d97b7",
            "block_hash": null,
            "block_number": "0x8fcb296",
            "address": "0xf1d7cc64fb4452f05c498126312ebe29f30fbcf9",
            "data": "0x000000000000000000000000f64dfe17c8b87f012fcf50fbda1d62bfa148366a0000000000000000000000000000000000000000000000000000000000000001",
            "topics": [
                "0x0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9",
                "0x00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1",
                "0x000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log_weth_usdt() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x0",
            "transactionIndex": "0x1",
            "transactionHash": "0xe7b5c25477c6dd2425c4bc07547ffb2777e018a12eed1d348d7bf553913d97b7",
            "blockHash": "0x5053fe02da5bb0c2fc690a467c1cc36e791047fc48c3ea4fe8bbeed069f3f7ba",
            "blockNumber": "0x8fcb296",
            "address": "0xf1d7cc64fb4452f05c498126312ebe29f30fbcf9",
            "data": "0x000000000000000000000000f64dfe17c8b87f012fcf50fbda1d62bfa148366a0000000000000000000000000000000000000000000000000000000000000001",
            "topics": [
                "0x0d3648bd0f6ba80134a33ba9275ac585d9d315f0ad8355cddefde31afa28d0e9",
                "0x00000000000000000000000082af49447d8a07e3bd95bd0d56f35241523fbab1",
                "0x000000000000000000000000af88d065e77c8cc2239327c5edb3a432268e5831"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    // ========== HyperSync parser tests ==========

    #[rstest]
    fn test_parse_pair_created_hypersync(hypersync_log_weth_usdt: HypersyncLog) {
        let event =
            parse_pool_created_event_hypersync(hypersync_log_weth_usdt).expect("Failed to parse");

        assert_eq!(event.block_number, 150778518);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831"
        );
        assert_eq!(
            event.pool_identifier.to_string(),
            "0xF64Dfe17C8b87F012FCf50FbDA1D62bfA148366a",
        );
        assert_eq!(event.fee, None);
        assert_eq!(event.tick_spacing, None);
    }

    // ========== RPC parser tests ==========

    #[rstest]
    fn test_parse_pair_created_rpc(rpc_log_weth_usdt: RpcLog) {
        let event = parse_pool_created_event_rpc(&rpc_log_weth_usdt).expect("Failed to parse");

        assert_eq!(event.block_number, 150778518);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x82af49447d8a07e3bd95bd0d56f35241523fbab1"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0xaf88d065e77c8cc2239327c5edb3a432268e5831"
        );
        assert_eq!(
            event.pool_identifier.to_string(),
            "0xF64Dfe17C8b87F012FCf50FbDA1D62bfA148366a"
        );
        assert_eq!(event.fee, None);
        assert_eq!(event.tick_spacing, None);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log_weth_usdt: HypersyncLog, rpc_log_weth_usdt: RpcLog) {
        let hypersync_event =
            parse_pool_created_event_hypersync(hypersync_log_weth_usdt).expect("HyperSync parse");
        let rpc_event = parse_pool_created_event_rpc(&rpc_log_weth_usdt).expect("RPC parse");

        assert_eq!(hypersync_event.block_number, rpc_event.block_number);
        assert_eq!(hypersync_event.token0, rpc_event.token0);
        assert_eq!(hypersync_event.token1, rpc_event.token1);
        assert_eq!(hypersync_event.pool_identifier, rpc_event.pool_identifier);
        assert_eq!(hypersync_event.fee, rpc_event.fee);
        assert_eq!(hypersync_event.tick_spacing, rpc_event.tick_spacing);
    }
}
