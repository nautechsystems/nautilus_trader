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

// PancakeSwap V3's Swap appends protocolFees fields, so its topic0 differs from Uniswap V3.
const SWAP_EVENT_SIGNATURE_HASH: &str =
    "19b47279256b2a23a1665c810c8d55a1758940ee09377d4f8d26497a3577dc83";

// Uniswap V3's Swap fields plus PancakeSwap's two appended protocolFees fields.
sol! {
    struct SwapEventData {
        int256 amount0;
        int256 amount1;
        uint160 sqrt_price_x96;
        uint128 liquidity;
        int24 tick;
        uint128 protocol_fees_token0;
        uint128 protocol_fees_token1;
    }
}

/// Parses a PancakeSwap V3 swap event from a HyperSync log.
///
/// The protocol-fee fields are decoded to validate the layout but dropped: the shared
/// [`SwapEvent`] mirrors the Uniswap V3 fields, which is all downstream pool profiling uses.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
///
/// # Panics
///
/// Panics if the contract address is not set in the log.
pub fn parse_swap_event_hypersync(dex: SharedDex, log: &HypersyncLog) -> anyhow::Result<SwapEvent> {
    validate_event_signature_hash("SwapEvent", SWAP_EVENT_SIGNATURE_HASH, log)?;

    let sender = extract_address_from_topic(log, 1, "sender")?;
    let recipient = extract_address_from_topic(log, 2, "recipient")?;

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        if data_bytes.len() < 7 * 32 {
            anyhow::bail!("Swap event data is too short");
        }

        let decoded = match <SwapEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode swap event data: {e}"),
        };
        let pool_address = Address::from_slice(
            log.address
                .clone()
                .expect("Contract address should be set in logs")
                .as_ref(),
        );
        let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
        Ok(SwapEvent::new(
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
            decoded.sqrt_price_x96,
            decoded.liquidity,
            decoded.tick.as_i32(),
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in swap event log"))
    }
}

/// Parses a PancakeSwap V3 swap event from an RPC log.
///
/// # Errors
///
/// Returns an error if the log parsing fails or if the event data is invalid.
pub fn parse_swap_event_rpc(dex: SharedDex, log: &RpcLog) -> anyhow::Result<SwapEvent> {
    rpc_helpers::validate_event_signature(log, SWAP_EVENT_SIGNATURE_HASH, "Swap")?;

    let sender = rpc_helpers::extract_address_from_topic(log, 1, "sender")?;
    let recipient = rpc_helpers::extract_address_from_topic(log, 2, "recipient")?;

    let data_bytes = rpc_helpers::extract_data_bytes(log)?;

    if data_bytes.len() < 7 * 32 {
        anyhow::bail!("Swap event data is too short");
    }

    let decoded = match <SwapEventData as SolType>::abi_decode(&data_bytes) {
        Ok(decoded) => decoded,
        Err(e) => anyhow::bail!("Failed to decode swap event data: {e}"),
    };

    let pool_address = rpc_helpers::extract_address(log)?;
    let pool_identifier = PoolIdentifier::Address(Ustr::from(&pool_address.to_string()));
    Ok(SwapEvent::new(
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
        decoded.sqrt_price_x96,
        decoded.liquidity,
        decoded.tick.as_i32(),
    ))
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{I256, U160, U256};
    use rstest::*;

    use super::*;
    use crate::exchanges::bsc;

    // Real PancakeSwap V3 Swap from BSC block 105495649 (WBNB/USDT 0.01% pool
    // 0x172fcd41e0913e95784454622d1c3724f546f849)
    const HYPERSYNC_SWAP_LOG: &str =
        include_str!("../../../../test_data/pancakeswap_v3_swap_hypersync.json");
    const RPC_SWAP_LOG: &str = include_str!("../../../../test_data/pancakeswap_v3_swap_rpc.json");

    #[fixture]
    fn hypersync_log() -> HypersyncLog {
        serde_json::from_str(HYPERSYNC_SWAP_LOG).expect("Failed to deserialize HyperSync log")
    }

    #[fixture]
    fn rpc_log() -> RpcLog {
        serde_json::from_str(RPC_SWAP_LOG).expect("Failed to deserialize RPC log")
    }

    #[rstest]
    fn test_parse_swap_event_hypersync(hypersync_log: HypersyncLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event = parse_swap_event_hypersync(dex, &hypersync_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x172fcD41E0913e95784454622d1c3724f546f849"
        );
        assert_eq!(
            event.sender.to_string().to_lowercase(),
            "0x7eded5ce04fd9bb6d125a0a470cc3ffcd972e182"
        );
        assert_eq!(
            event.receiver.to_string().to_lowercase(),
            "0x7eded5ce04fd9bb6d125a0a470cc3ffcd972e182"
        );
        assert_eq!(
            event.amount0,
            I256::try_from(2291588381489685660_i128).unwrap()
        );
        let expected_amount1 = I256::from_raw(
            U256::from_str_radix(
                "fffffffffffffffffffffffffffffffffffffffffffffffffff22743d8dee163",
                16,
            )
            .unwrap(),
        );
        assert_eq!(event.amount1, expected_amount1);
        let expected_sqrt_price = U160::from_str_radix("a8edeae49c411da42257f71", 16).unwrap();
        assert_eq!(event.sqrt_price_x96, expected_sqrt_price);
        let expected_liquidity = u128::from_str_radix("310fdcabce7b0096dfc84", 16).unwrap();
        assert_eq!(event.liquidity, expected_liquidity);
        assert_eq!(event.tick, -63769);
        assert_eq!(event.block_number, 105495649);
    }

    #[rstest]
    fn test_parse_swap_event_rpc(rpc_log: RpcLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event = parse_swap_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(
            event.pool_identifier.to_string(),
            "0x172fcD41E0913e95784454622d1c3724f546f849"
        );
        assert_eq!(
            event.amount0,
            I256::try_from(2291588381489685660_i128).unwrap()
        );
        let expected_amount1 = I256::from_raw(
            U256::from_str_radix(
                "fffffffffffffffffffffffffffffffffffffffffffffffffff22743d8dee163",
                16,
            )
            .unwrap(),
        );
        assert_eq!(event.amount1, expected_amount1);
        assert_eq!(event.tick, -63769);
        assert_eq!(event.block_number, 105495649);
    }

    #[rstest]
    fn test_hypersync_rpc_match(hypersync_log: HypersyncLog, rpc_log: RpcLog) {
        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let event_hypersync = parse_swap_event_hypersync(dex.clone(), &hypersync_log).unwrap();
        let event_rpc = parse_swap_event_rpc(dex, &rpc_log).unwrap();

        assert_eq!(event_hypersync.pool_identifier, event_rpc.pool_identifier);
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

    #[rstest]
    fn test_rejects_uniswap_v3_length_data() {
        // Truncate the data to the Uniswap V3 5-word layout: PancakeSwap V3 requires the two
        // appended protocolFees words, so the shorter payload must be rejected.
        let mut value: serde_json::Value = serde_json::from_str(HYPERSYNC_SWAP_LOG).unwrap();
        let data = value["data"].as_str().unwrap();
        // 0x + first 5 of the 7 32-byte words.
        let truncated = data[..2 + 5 * 64].to_string();
        value["data"] = serde_json::Value::String(truncated);
        let log: HypersyncLog = serde_json::from_value(value).unwrap();

        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let err = parse_swap_event_hypersync(dex, &log).unwrap_err();
        assert!(err.to_string().contains("too short"));
    }

    #[rstest]
    fn test_rejects_uniswap_v3_swap_topic() {
        // The Uniswap V3 Swap topic0 must be rejected: PancakeSwap V3 hashes to its own
        // topic even though the leading data fields overlap.
        let uniswap_v3_topic = "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";
        let log_json = HYPERSYNC_SWAP_LOG.replace(SWAP_EVENT_SIGNATURE_HASH, uniswap_v3_topic);
        let log: HypersyncLog = serde_json::from_str(&log_json).unwrap();

        let dex = bsc::PANCAKESWAP_V3.dex.clone();
        let result = parse_swap_event_hypersync(dex, &log);
        assert!(result.is_err());
    }
}
