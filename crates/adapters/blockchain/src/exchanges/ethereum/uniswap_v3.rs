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

use std::sync::LazyLock;

use alloy::{
    primitives::{Address, Signed, U160, U256},
    sol,
    sol_types::SolType,
};
use hypersync_client::simple_types::Log;
use nautilus_model::{
    defi::{
        chain::chains,
        dex::{AmmType, Dex},
        token::Token,
    },
    enums::OrderSide,
    types::{Price, Quantity, fixed::FIXED_PRECISION},
};

use crate::{
    events::{burn::BurnEvent, mint::MintEvent, pool_created::PoolCreatedEvent, swap::SwapEvent},
    exchanges::extended::DexExtended,
    hypersync::helpers::{
        extract_block_number, extract_log_index, extract_transaction_hash,
        extract_transaction_index, validate_event_signature_hash,
    },
    math::convert_i256_to_f64,
};

const POOL_CREATED_EVENT_SIGNATURE_HASH: &str =
    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";
const SWAP_EVENT_SIGNATURE_HASH: &str =
    "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";
const MINT_EVENT_SIGNATURE_HASH: &str =
    "7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde";
const BURN_EVENT_SIGNATURE_HASH: &str =
    "0c396cd989a39f4459b5fa1aed6a9a8dcdbc45908acfd67e028cd568da98982c";

/// Uniswap V3 DEX on Ethereum.
pub static UNISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = DexExtended::new(Dex::new(
        chains::ETHEREUM.clone(),
        "Uniswap V3",
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
        "Mint(address,address,int24,int24,uint128,uint256,uint256)",
        "Burn(address,int24,int24,uint128,uint256,uint256)",
    ));
    dex.set_pool_created_event_parsing(parse_pool_created_event);
    dex.set_swap_event_parsing(parse_swap_event);
    dex.set_convert_trade_data(convert_to_trade_data);
    dex.set_mint_event_parsing(parse_mint_event);
    dex.set_burn_event_parsing(parse_burn_event);
    dex
});

fn parse_pool_created_event(log: Log) -> anyhow::Result<PoolCreatedEvent> {
    validate_event_signature_hash("PoolCreatedEvent", POOL_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = log
        .block_number
        .expect("Block number should be set in logs");

    let token = if let Some(topic) = log.topics.get(1).and_then(|t| t.as_ref()) {
        // Address is stored in the last 20 bytes of the 32-byte topic
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token0 address in topic1 when parsing pool created event");
    };

    let token1 = if let Some(topic) = log.topics.get(2).and_then(|t| t.as_ref()) {
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token1 address in topic2 when parsing pool created event");
    };

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
            block_number.into(),
            token,
            token1,
            fee,
            tick_spacing,
            pool_address,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in pool created event log"))
    }
}

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

fn parse_swap_event(log: Log) -> anyhow::Result<SwapEvent> {
    validate_event_signature_hash("SwapEvent", SWAP_EVENT_SIGNATURE_HASH, &log)?;

    let sender = match log.topics.get(1).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => anyhow::bail!("Missing sender address in topic1 when parsing swap event"),
    };

    let recipient = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => anyhow::bail!("Missing recipient address in topic2 when parsing swap event"),
    };

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
        decoded.amount0;

        Ok(SwapEvent::new(
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            sender,
            recipient,
            decoded.amount0,
            decoded.amount1,
            decoded.sqrt_price_x96,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in swap event log"))
    }
}

/// <https://blog.uniswap.org/uniswap-v3-math-primer>
fn calculate_price_from_sqrt_price(
    sqrt_price_x96: U160,
    token0_decimals: u8,
    token1_decimals: u8,
) -> f64 {
    let sqrt_price = sqrt_price_x96 >> 96;
    let price = sqrt_price * sqrt_price;
    let price: f64 = U256::from(price)
        .to_string()
        .parse()
        .expect("Failed to parse U256 to f64");
    let token0_multiplier = 10u128.pow(u32::from(token0_decimals));
    let token1_multiplier = 10u128.pow(u32::from(token1_decimals));
    let factor = token1_multiplier as f64 / token0_multiplier as f64;
    factor / price
}

fn convert_to_trade_data(
    token0: &Token,
    token1: &Token,
    swap_event: &SwapEvent,
) -> anyhow::Result<(OrderSide, Quantity, Price)> {
    let price_f64 = calculate_price_from_sqrt_price(
        swap_event.sqrt_price_x96,
        token0.decimals,
        token1.decimals,
    );
    let price = Price::from(format!(
        "{:.precision$}",
        price_f64,
        precision = FIXED_PRECISION as usize
    ));
    let quantity_f64 = convert_i256_to_f64(swap_event.amount1, token1.decimals)?.abs();
    let quantity = Quantity::from(format!(
        "{:.precision$}",
        quantity_f64,
        precision = FIXED_PRECISION as usize
    ));
    let zero = Signed::<256, 4>::ZERO;
    let side = if swap_event.amount1 > zero {
        OrderSide::Sell
    } else {
        OrderSide::Buy
    };
    Ok((side, quantity, price))
}

// Define sol macro for easier parsing of Mint event data
// It contains 4 parameters of 32 bytes each:
// sender (address), amount (uint128), amount0 (uint256), amount1 (uint256)
sol! {
    struct MintEventData {
        address sender;
        uint128 amount;
        uint256 amount0;
        uint256 amount1;
    }
}

fn parse_mint_event(log: Log) -> anyhow::Result<MintEvent> {
    validate_event_signature_hash("Mint", MINT_EVENT_SIGNATURE_HASH, &log)?;

    let owner = match log.topics.get(1).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => anyhow::bail!("Missing owner address in topic1 when parsing mint event"),
    };

    // Extract int24 tickLower from topic2 (stored as a 32-byte padded value)
    let tick_lower = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_lower_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_lower_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickLower in topic2 when parsing mint event"),
    };

    // Extract int24 tickUpper from topic3 (stored as a 32-byte padded value)
    let tick_upper = match log.topics.get(3).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_upper_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_upper_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickUpper in topic3 when parsing mint event"),
    };

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 4 parameters of 32 bytes each
        if data_bytes.len() < 4 * 32 {
            anyhow::bail!("Mint event data is too short");
        }

        // Decode the data using the MintEventData struct
        let decoded = match <MintEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode mint event data: {e}"),
        };

        Ok(MintEvent::new(
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            decoded.sender,
            owner,
            tick_lower,
            tick_upper,
            decoded.amount,
            decoded.amount0,
            decoded.amount1,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in mint event log"))
    }
}

// Define sol macro for easier parsing of Burn event data
// It contains 3 parameters of 32 bytes each:
// amount (uint128), amount0 (uint256), amount1 (uint256)
sol! {
    struct BurnEventData {
        uint128 amount;
        uint256 amount0;
        uint256 amount1;
    }
}

fn parse_burn_event(log: Log) -> anyhow::Result<BurnEvent> {
    validate_event_signature_hash("Burn", BURN_EVENT_SIGNATURE_HASH, &log)?;

    let owner = match log.topics.get(1).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => anyhow::bail!("Missing owner address in topic1 when parsing burn event"),
    };

    // Extract int24 tickLower from topic2 (stored as a 32-byte padded value)
    let tick_lower = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_lower_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_lower_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickLower in topic2 when parsing burn event"),
    };

    // Extract int24 tickUpper from topic3 (stored as a 32-byte padded value)
    let tick_upper = match log.topics.get(3).and_then(|t| t.as_ref()) {
        Some(topic) => {
            let tick_upper_bytes: [u8; 32] = topic.as_ref().try_into()?;
            i32::from_be_bytes(tick_upper_bytes[28..32].try_into()?)
        }
        None => anyhow::bail!("Missing tickUpper in topic3 when parsing burn event"),
    };

    if let Some(data) = &log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 3 parameters of 32 bytes each
        if data_bytes.len() < 3 * 32 {
            anyhow::bail!("Burn event data is too short");
        }

        // Decode the data using the BurnEventData struct
        let decoded = match <BurnEventData as SolType>::abi_decode(data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => anyhow::bail!("Failed to decode burn event data: {e}"),
        };

        Ok(BurnEvent::new(
            extract_block_number(&log)?,
            extract_transaction_hash(&log)?,
            extract_transaction_index(&log)?,
            extract_log_index(&log)?,
            owner,
            tick_lower,
            tick_upper,
            decoded.amount,
            decoded.amount0,
            decoded.amount1,
        ))
    } else {
        Err(anyhow::anyhow!("Missing data in burn event log"))
    }
}

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[fixture]
    fn mint_event_log() -> Log {
        serde_json::from_str(r#"{
            "removed": null,
            "log_index": null,
            "transaction_index": null,
            "transaction_hash": null,
            "block_hash": null,
            "block_number": "0x1581756",
            "address": null,
            "data": "0x000000000000000000000000f5a96d43e4b9a2c47f302b54d006d7e20f038658000000000000000000000000000000000000000000000028c8b4995ae1ad0e9e000000000000000000000000000000000000000000000000000009423c32486c0000000000000000000000000000000000000000000000bb5bc19aa32e5d05b4",
            "topics": [
                "0x7a53080ba414158be7ec69b987b5fb7d07dee101fe85488f0853ae16239d0bde",
                "0x000000000000000000000000a69babef1ca67a37ffaf7a485dfff3382056e78c",
                "0x00000000000000000000000000000000000000000000000000000000000304e4",
                "0x00000000000000000000000000000000000000000000000000000000000304ee"
            ]
        }"#).unwrap()
    }

    #[rstest]
    fn test_parse_mint_event(mint_event_log: Log) {
        let result = parse_mint_event(mint_event_log);
        assert!(result.is_ok());
        let mint_event = result.unwrap();

        assert_eq!(mint_event.block_number, 0x1581756);
        assert_eq!(
            mint_event.owner.to_string().to_lowercase(),
            "0xa69babef1ca67a37ffaf7a485dfff3382056e78c"
        );
        assert_eq!(mint_event.tick_lower, 197860); // 0x304e4
        assert_eq!(mint_event.tick_upper, 197870); // 0x304ee
        assert_eq!(
            mint_event.sender.to_string().to_lowercase(),
            "0xf5a96d43e4b9a2c47f302b54d006d7e20f038658"
        );
        assert_eq!(mint_event.amount, 0x28c8b4995ae1ad0e9e);
        assert_eq!(mint_event.amount0.to_string(), "10180082419820");
        assert_eq!(mint_event.amount1.to_string(), "3456152877537290945972");
    }

    #[rstest]
    fn test_parse_mint_event_missing_data() {
        let mut log = mint_event_log();
        log.data = None;

        let result = parse_mint_event(log);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing data"));
    }

    #[rstest]
    fn test_parse_mint_event_missing_topics() {
        let mut log = mint_event_log();

        // Test missing owner
        log.topics.truncate(1);
        let result = parse_mint_event(log.clone());
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Missing owner"));

        // Test missing tickLower
        log = mint_event_log();
        log.topics.truncate(2);
        let result = parse_mint_event(log.clone());
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing tickLower")
        );

        // Test missing tickUpper
        log = mint_event_log();
        log.topics.truncate(3);
        let result = parse_mint_event(log);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Missing tickUpper")
        );
    }
}
