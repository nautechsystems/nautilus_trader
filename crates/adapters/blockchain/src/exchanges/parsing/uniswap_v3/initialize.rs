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
    events::initialize::InitializeEvent,
    hypersync::{HypersyncLog, helpers::validate_event_signature_hash},
    rpc::helpers as rpc_helpers,
};

const INITIALIZE_EVENT_SIGNATURE_HASH: &str =
    "98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95";

// Define sol macro for easier parsing of Initialize event data
// It contains 2 parameters:
// sqrtPriceX96 (uint160), tick (int24)
sol! {
    struct InitializeEventData {
        uint160 sqrt_price_x96;
        int24 tick;
    }
}

/// Parses an initialize event from a Uniswap V3 log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_initialize_event_hypersync(
    dex: SharedDex,
    log: HypersyncLog,
) -> anyhow::Result<InitializeEvent> {
    validate_event_signature_hash("InitializeEvent", INITIALIZE_EVENT_SIGNATURE_HASH, &log)?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 2 parameters of 32 bytes each (sqrtPriceX96 and tick)
        if data_bytes.len() < 2 * 32 {
            anyhow::bail!("Initialize event data is too short");
        }

        // Decode the data using the InitializeEventData struct
        let decoded = match <InitializeEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode initialize event data: {e}"),
        };

        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );

        Ok(InitializeEvent::new(
            dex,
            pool_address,
            decoded.sqrt_price_x96,
            i32::try_from(decoded.tick)?,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in initialize event log"))
    }
}

/// Parses an initialize event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_initialize_event_rpc(dex: SharedDex, log: &RpcLog) -> anyhow::Result<InitializeEvent> {
    rpc_helpers::validate_event_signature(log, INITIALIZE_EVENT_SIGNATURE_HASH, "Initialize")?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    // Validate if data contains 2 parameters of 32 bytes each (sqrtPriceX96 and tick)
    if data_bytes.len() < 2 * 32 {
        anyhow::bail!("Initialize event data is too short");
    }

    // Decode the data using the InitializeEventData struct
    let decoded = match <InitializeEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode initialize event data: {e}"),
    };

    Ok(InitializeEvent::new(
        dex,
        rpc_helpers::extract_address(log)?,
        decoded.sqrt_price_x96,
        i32::try_from(decoded.tick)?,
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use alloy::primitives::U160;
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::exchanges::arbitrum;

    /// Real HyperSync log from Arbitrum Initialize event at block 391053023
    /// Pool: 0xd13040d4fe917ee704158cfcb3338dcd2838b245
    /// sqrtPriceX96: 0x3d409fc4ca983d2e3df335 (large number)
    /// tick: -139514 (0xfffddf06 as signed int24)
    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x4",
            "transaction_index": "0x3",
            "transaction_hash": "0x8f91d60156ea7a34a6bf1d411852f3ef2ad255ec84e493c9e902e4a1ff4a46af",
            "block_hash": null,
            "block_number": "0x175122df",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0x0000000000000000000000000000000000000000003d409fc4ca983d2e3df335fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffddf06",
            "topics": [
                "0x98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95",
                null,
                null,
                null
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    /// Real RPC log from Arbitrum Initialize event at block 391053023
    #[fixture]
    fn rpc_log() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x4",
            "transactionIndex": "0x3",
            "transactionHash": "0x8f91d60156ea7a34a6bf1d411852f3ef2ad255ec84e493c9e902e4a1ff4a46af",
            "blockHash": "0xfc49f94161e2cdef8339c0b430868d64ee1f5d0bd8b8b6e45a25487958d68b25",
            "blockNumber": "0x175122df",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0x0000000000000000000000000000000000000000003d409fc4ca983d2e3df335fffffffffffffffffffffffffffffffffffffffffffffffffffffffffffddf06",
            "topics": [
                "0x98636036cb66a9c19a37435efc1e90142190214e8abeb821bdba3f2990dd4c95"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_initialize_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_initialize_event_hypersync(dex, hypersync_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        let expected_sqrt_price = U160::from_str_radix("3d409fc4ca983d2e3df335", 16).unwrap();
        assert_eq!(event.sqrt_price_x96, expected_sqrt_price);
        assert_eq!(event.tick, -139514);
    }

    #[rstest]
    fn test_parse_initialize_event_rpc(rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_initialize_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        let expected_sqrt_price = U160::from_str_radix("3d409fc4ca983d2e3df335", 16).unwrap();
        assert_eq!(event.sqrt_price_x96, expected_sqrt_price);
        assert_eq!(event.tick, -139514);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event_hypersync = parse_initialize_event_hypersync(dex.clone(), hypersync_log).unwrap();
        let event_rpc = parse_initialize_event_rpc(dex, &rpc_log).unwrap();

        // Both parsers should produce identical results
        assert_eq!(event_hypersync.pool_address, event_rpc.pool_address);
        assert_eq!(event_hypersync.sqrt_price_x96, event_rpc.sqrt_price_x96);
        assert_eq!(event_hypersync.tick, event_rpc.tick);
    }
}
