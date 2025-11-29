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
    events::swap::SwapEvent,
    hypersync::{
        HypersyncLog,
        helpers::{
            extract_address_from_topic, extract_block_number, extract_log_index,
            extract_transaction_hash, extract_transaction_index, validate_event_signature_hash,
        },
    },
    rpc::helpers as rpc_helpers,
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

/// Parses a swap event from a HyperSync log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_swap_event_hypersync(dex: SharedDex, log: HypersyncLog) -> anyhow::Result<SwapEvent> {
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

/// Parses a swap event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_swap_event_rpc(dex: SharedDex, log: &RpcLog) -> anyhow::Result<SwapEvent> {
    rpc_helpers::validate_event_signature(log, SWAP_EVENT_SIGNATURE_HASH, "Swap")?;

    let sender = rpc_helpers::extract_address_from_topic(log, 1, "sender")?;
    let recipient = rpc_helpers::extract_address_from_topic(log, 2, "recipient")?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    // Validate if data contains 5 parameters of 32 bytes each
    if data_bytes.len() < 5 * 32 {
        anyhow::bail!("Swap event data is too short");
    }

    // Decode the data using the SwapEventData struct
    let decoded = match <SwapEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode swap event data: {e}"),
    };

    Ok(SwapEvent::new(
        dex,
        rpc_helpers::extract_address(log)?,
        rpc_helpers::extract_block_number(log)?,
        rpc_helpers::extract_transaction_hash(log)?,
        rpc_helpers::extract_transaction_index(log)?,
        rpc_helpers::extract_log_index(log)?,
        sender,
        recipient,
        decoded.amount0,
        decoded.amount1,
        decoded.sqrt_price_x96,
        decoded.liquidity,
        decoded.tick.as_i32(),
    ))
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use alloy::primitives::{I256, U160, U256};
    use rstest::*;
    use serde_json::json;

    use super::*;
    use crate::exchanges::arbitrum;

    /// Real HyperSync log from Arbitrum Swap event at block 0x17513444 (391197764)
    /// Pool: 0xd13040d4fe917ee704158cfcb3338dcd2838b245
    /// sender: 0x9da4a7d3cf502337797ea37724f7afc426377119
    /// recipient: 0xd491076c7316bc28fd4d35e3da9ab5286d079250
    /// amount0: negative (token out)
    /// amount1: positive (token in)
    /// tick: -139475 (0xfffddf2d)
    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        let log_json = json!({
            "removed": null,
            "log_index": "0x6",
            "transaction_index": "0x3",
            "transaction_hash": "0x381ae1c1b65bba31abdfc68ef6b3e3e49913161a15398ccff3b242b05473e720",
            "block_hash": null,
            "block_number": "0x17513444",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0xffffffffffffffffffffffffffffffffffffffffffffff0918233055494456fe000000000000000000000000000000000000000000000000000e2a274937d6380000000000000000000000000000000000000000003d5fe159ea44896552c1cd000000000000000000000000000000000000000000000074009aac72ba0a9b1cfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffddf2d",
            "topics": [
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                "0x0000000000000000000000009da4a7d3cf502337797ea37724f7afc426377119",
                "0x000000000000000000000000d491076c7316bc28fd4d35e3da9ab5286d079250"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize HyperSync log")
    }

    /// Real RPC log from Arbitrum Swap event at block 0x17513444 (391197764)
    #[fixture]
    fn rpc_log() -> RpcLog {
        let log_json = json!({
            "removed": false,
            "logIndex": "0x6",
            "transactionIndex": "0x3",
            "transactionHash": "0x381ae1c1b65bba31abdfc68ef6b3e3e49913161a15398ccff3b242b05473e720",
            "blockHash": "0x43082eabb648a3b87bd22abf7ec645a97e6e7f099dcc18894830c70d85675fae",
            "blockNumber": "0x17513444",
            "address": "0xd13040d4fe917ee704158cfcb3338dcd2838b245",
            "data": "0xffffffffffffffffffffffffffffffffffffffffffffff0918233055494456fe000000000000000000000000000000000000000000000000000e2a274937d6380000000000000000000000000000000000000000003d5fe159ea44896552c1cd000000000000000000000000000000000000000000000074009aac72ba0a9b1cfffffffffffffffffffffffffffffffffffffffffffffffffffffffffffddf2d",
            "topics": [
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                "0x0000000000000000000000009da4a7d3cf502337797ea37724f7afc426377119",
                "0x000000000000000000000000d491076c7316bc28fd4d35e3da9ab5286d079250"
            ]
        });
        serde_json::from_value(log_json).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_swap_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_swap_event_hypersync(dex, hypersync_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0x9da4a7d3cf502337797ea37724f7afc426377119"
        );
        assert_eq!(
            event.receiver.to_string().to_lowercase(),
            "0xd491076c7316bc28fd4d35e3da9ab5286d079250"
        );
        let expected_amount0 = I256::from_raw(
            U256::from_str_radix(
                "ffffffffffffffffffffffffffffffffffffffffffffff0918233055494456fe",
                16,
            )
            .unwrap(),
        );
        assert_eq!(event.amount0, expected_amount0);
        let expected_amount1 = I256::from_raw(U256::from_str_radix("0e2a274937d638", 16).unwrap());
        assert_eq!(event.amount1, expected_amount1);
        let expected_sqrt_price = U160::from_str_radix("3d5fe159ea44896552c1cd", 16).unwrap();
        assert_eq!(event.sqrt_price_x96, expected_sqrt_price);
        let expected_liquidity = u128::from_str_radix("74009aac72ba0a9b1c", 16).unwrap();
        assert_eq!(event.liquidity, expected_liquidity);
        assert_eq!(event.tick, -139475);
        assert_eq!(event.block_number, 391197764);
    }

    #[rstest]
    fn test_parse_swap_event_rpc(rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event = parse_swap_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_address.to_string().to_lowercase(),
            "0xd13040d4fe917ee704158cfcb3338dcd2838b245"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0x9da4a7d3cf502337797ea37724f7afc426377119"
        );
        assert_eq!(
            event.receiver.to_string().to_lowercase(),
            "0xd491076c7316bc28fd4d35e3da9ab5286d079250"
        );
        let expected_amount0 = I256::from_raw(
            U256::from_str_radix(
                "ffffffffffffffffffffffffffffffffffffffffffffff0918233055494456fe",
                16,
            )
            .unwrap(),
        );
        assert_eq!(event.amount0, expected_amount0);
        let expected_amount1 = I256::from_raw(U256::from_str_radix("0e2a274937d638", 16).unwrap());
        assert_eq!(event.amount1, expected_amount1);
        let expected_sqrt_price = U160::from_str_radix("3d5fe159ea44896552c1cd", 16).unwrap();
        assert_eq!(event.sqrt_price_x96, expected_sqrt_price);
        let expected_liquidity = u128::from_str_radix("74009aac72ba0a9b1c", 16).unwrap();
        assert_eq!(event.liquidity, expected_liquidity);
        assert_eq!(event.tick, -139475);
        assert_eq!(event.block_number, 391197764);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = arbitrum::UNISWAP_V3.dex.clone();
        let event_hypersync = parse_swap_event_hypersync(dex.clone(), hypersync_log).unwrap();
        let event_rpc = parse_swap_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_address, event_rpc.pool_address);
        assert_eq!(event_hypersync.sender, event_rpc.sender);
        assert_eq!(event_hypersync.receiver, event_rpc.receiver);
        assert_eq!(event_hypersync.amount0, event_rpc.amount0);
        assert_eq!(event_hypersync.amount1, event_rpc.amount1);
        assert_eq!(event_hypersync.sqrt_price_x96, event_rpc.sqrt_price_x96);
        assert_eq!(event_hypersync.liquidity, event_rpc.liquidity);
        assert_eq!(event_hypersync.tick, event_rpc.tick);
        assert_eq!(event_hypersync.block_number, event_rpc.block_number);
        assert_eq!(event_hypersync.transaction_hash, event_rpc.transaction_hash);
    }
}
