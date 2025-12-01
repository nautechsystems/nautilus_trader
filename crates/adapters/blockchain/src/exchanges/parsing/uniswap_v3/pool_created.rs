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
use nautilus_model::defi::rpc::RpcLog;

use crate::{
    events::pool_created::PoolCreatedEvent,
    exchanges::parsing::core,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
};

const POOL_CREATED_EVENT_SIGNATURE_HASH: &str =
    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";

/// Parses a pool creation event from a HyperSync log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the block number is not set in the log.
pub fn parse_pool_created_event_hypersync(log: HypersyncLog) -> anyhow::Result<PoolCreatedEvent> {
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
            block_number,
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

/// Parses a pool creation event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_pool_created_event_rpc(log: &RpcLog) -> anyhow::Result<PoolCreatedEvent> {
    rpc_helpers::validate_event_signature(
        log,
        POOL_CREATED_EVENT_SIGNATURE_HASH,
        "PoolCreatedEvent",
    )?;

    let block_number = rpc_helpers::extract_block_number(log)?;
    let token0 = rpc_helpers::extract_address_from_topic(log, 1, "token0")?;
    let token1 = rpc_helpers::extract_address_from_topic(log, 2, "token1")?;

    // Extract fee from topic3
    let fee_bytes = rpc_helpers::extract_topic_bytes(log, 3)?;
    let fee = core::extract_u32_from_bytes(&fee_bytes)?;

    // Extract tick_spacing and pool from data
    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    anyhow::ensure!(
        data_bytes.len() >= 64,
        "Pool created event data too short: expected at least 64 bytes, got {}",
        data_bytes.len()
    );

    let tick_spacing = u32::from_be_bytes(data_bytes[28..32].try_into()?);
    let pool_address = Address::from_slice(&data_bytes[44..64]);

    Ok(PoolCreatedEvent::new(
        block_number,
        token0,
        token1,
        pool_address,
        Some(fee),
        Some(tick_spacing),
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

    // ========== Block 185 fixtures ==========
    // Pool: 0xB9Fc136980D98C034a529AadbD5651c087365D5f
    // token0: 0x2E5353426C89F4eCD52D1036DA822D47E73376C4
    // token1: 0x838930cFE7502dd36B0b1ebbef8001fbF94f3bFb
    // fee: 3000, tickSpacing: 60

    #[fixture]
    fn hypersync_log_block_185() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x0",
            "transaction_index": "0x0",
            "transaction_hash": "0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7",
            "block_hash": null,
            "block_number": "0xb9",
            "address": "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            "data": "0x000000000000000000000000000000000000000000000000000000000000003c000000000000000000000000b9fc136980d98c034a529aadbd5651c087365d5f",
            "topics": [
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                "0x0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4",
                "0x000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb",
                "0x0000000000000000000000000000000000000000000000000000000000000bb8"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log_block_185() -> RpcLog {
        RpcLog {
            removed: false,
            log_index: Some("0x0".to_string()),
            transaction_index: Some("0x0".to_string()),
            transaction_hash: Some(
                "0x24058dde7caf5b8b70041de8b27731f20f927365f210247c3e720e947b9098e7".to_string(),
            ),
            block_hash: Some(
                "0xd371b6c7b04ec33d6470f067a82e87d7b294b952bea7a46d7b939b4c7addc275".to_string(),
            ),
            block_number: Some("0xb9".to_string()),
            address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".to_string(),
            data: "0x000000000000000000000000000000000000000000000000000000000000003c000000000000000000000000b9fc136980d98c034a529aadbd5651c087365d5f".to_string(),
            topics: vec![
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118".to_string(),
                "0x0000000000000000000000002e5353426c89f4ecd52d1036da822d47e73376c4".to_string(),
                "0x000000000000000000000000838930cfe7502dd36b0b1ebbef8001fbf94f3bfb".to_string(),
                "0x0000000000000000000000000000000000000000000000000000000000000bb8".to_string(),
            ],
        }
    }

    // ========== Block 540 fixtures ==========
    // Pool: 0x7d25DE0bB3e4E4d5F7b399db5A0BCa9F60dD66e4
    // token0: 0x8dd7c686B11c115FfAbA245CBfc418B371087F68
    // token1: 0xBE5381d826375492E55E05039a541eb2CB978e76
    // fee: 500, tickSpacing: 10

    #[fixture]
    fn hypersync_log_block_540() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x0",
            "transaction_index": "0x0",
            "transaction_hash": "0x0810b3488eba9b0264d3544b4548b70d0c8667e05ac4a5d90686f4a9f70509df",
            "block_hash": null,
            "block_number": "0x21c",
            "address": "0x1f98431c8ad98523631ae4a59f267346ea31f984",
            "data": "0x000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000007d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4",
            "topics": [
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118",
                "0x0000000000000000000000008dd7c686b11c115ffaba245cbfc418b371087f68",
                "0x000000000000000000000000be5381d826375492e55e05039a541eb2cb978e76",
                "0x00000000000000000000000000000000000000000000000000000000000001f4"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log_block_540() -> RpcLog {
        RpcLog {
            removed: false,
            log_index: Some("0x0".to_string()),
            transaction_index: Some("0x0".to_string()),
            transaction_hash: Some(
                "0x0810b3488eba9b0264d3544b4548b70d0c8667e05ac4a5d90686f4a9f70509df".to_string(),
            ),
            block_hash: Some(
                "0x59bb10cdfd586affc6aa4a0b12f0662ec04599a1a459ac5b33129bc2c8705ccd".to_string(),
            ),
            block_number: Some("0x21c".to_string()),
            address: "0x1f98431c8ad98523631ae4a59f267346ea31f984".to_string(),
            data: "0x000000000000000000000000000000000000000000000000000000000000000a0000000000000000000000007d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4".to_string(),
            topics: vec![
                "0x783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118".to_string(),
                "0x0000000000000000000000008dd7c686b11c115ffaba245cbfc418b371087f68".to_string(),
                "0x000000000000000000000000be5381d826375492e55e05039a541eb2cb978e76".to_string(),
                "0x00000000000000000000000000000000000000000000000000000000000001f4".to_string(),
            ],
        }
    }

    // ========== HyperSync parser tests ==========

    #[rstest]
    fn test_parse_pool_created_hypersync_block_185(hypersync_log_block_185: HypersyncLog) {
        let event =
            parse_pool_created_event_hypersync(hypersync_log_block_185).expect("Failed to parse");

        assert_eq!(event.block_number, 185);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x2e5353426c89f4ecd52d1036da822d47e73376c4"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0x838930cfe7502dd36b0b1ebbef8001fbf94f3bfb"
        );
        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xb9fc136980d98c034a529aadbd5651c087365d5f"
        );
        assert_eq!(event.fee, Some(3000));
        assert_eq!(event.tick_spacing, Some(60));
    }

    #[rstest]
    fn test_parse_pool_created_hypersync_block_540(hypersync_log_block_540: HypersyncLog) {
        let event =
            parse_pool_created_event_hypersync(hypersync_log_block_540).expect("Failed to parse");

        assert_eq!(event.block_number, 540);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x8dd7c686b11c115ffaba245cbfc418b371087f68"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0xbe5381d826375492e55e05039a541eb2cb978e76"
        );
        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0x7d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4"
        );
        assert_eq!(event.fee, Some(500));
        assert_eq!(event.tick_spacing, Some(10));
    }

    // ========== RPC parser tests ==========

    #[rstest]
    fn test_parse_pool_created_rpc_block_185(rpc_log_block_185: RpcLog) {
        let event = parse_pool_created_event_rpc(&rpc_log_block_185).expect("Failed to parse");

        assert_eq!(event.block_number, 185);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x2e5353426c89f4ecd52d1036da822d47e73376c4"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0x838930cfe7502dd36b0b1ebbef8001fbf94f3bfb"
        );
        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xb9fc136980d98c034a529aadbd5651c087365d5f"
        );
        assert_eq!(event.fee, Some(3000));
        assert_eq!(event.tick_spacing, Some(60));
    }

    #[rstest]
    fn test_parse_pool_created_rpc_block_540(rpc_log_block_540: RpcLog) {
        let event = parse_pool_created_event_rpc(&rpc_log_block_540).expect("Failed to parse");

        assert_eq!(event.block_number, 540);
        assert_eq!(
            event.token0.to_string().to_lowercase(),
            "0x8dd7c686b11c115ffaba245cbfc418b371087f68"
        );
        assert_eq!(
            event.token1.to_string().to_lowercase(),
            "0xbe5381d826375492e55e05039a541eb2cb978e76"
        );
        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0x7d25de0bb3e4e4d5f7b399db5a0bca9f60dd66e4"
        );
        assert_eq!(event.fee, Some(500));
        assert_eq!(event.tick_spacing, Some(10));
    }

    // ========== Cross-validation tests ==========

    #[rstest]
    fn test_hypersync_rpc_match_block_185(
        hypersync_log_block_185: HypersyncLog,
        rpc_log_block_185: RpcLog,
    ) {
        let hypersync_event =
            parse_pool_created_event_hypersync(hypersync_log_block_185).expect("HyperSync parse");
        let rpc_event = parse_pool_created_event_rpc(&rpc_log_block_185).expect("RPC parse");

        assert_eq!(hypersync_event.block_number, rpc_event.block_number);
        assert_eq!(hypersync_event.token0, rpc_event.token0);
        assert_eq!(hypersync_event.token1, rpc_event.token1);
        assert_eq!(hypersync_event.pool_address, rpc_event.pool_address);
        assert_eq!(hypersync_event.fee, rpc_event.fee);
        assert_eq!(hypersync_event.tick_spacing, rpc_event.tick_spacing);
    }

    #[rstest]
    fn test_hypersync_rpc_match_block_540(
        hypersync_log_block_540: HypersyncLog,
        rpc_log_block_540: RpcLog,
    ) {
        let hypersync_event =
            parse_pool_created_event_hypersync(hypersync_log_block_540).expect("HyperSync parse");
        let rpc_event = parse_pool_created_event_rpc(&rpc_log_block_540).expect("RPC parse");

        assert_eq!(hypersync_event.block_number, rpc_event.block_number);
        assert_eq!(hypersync_event.token0, rpc_event.token0);
        assert_eq!(hypersync_event.token1, rpc_event.token1);
        assert_eq!(hypersync_event.pool_address, rpc_event.pool_address);
        assert_eq!(hypersync_event.fee, rpc_event.fee);
        assert_eq!(hypersync_event.tick_spacing, rpc_event.tick_spacing);
    }
}
