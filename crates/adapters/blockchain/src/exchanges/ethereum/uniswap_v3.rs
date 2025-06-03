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
    primitives::{Address, I256, Signed, U160, U256},
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
    events::{pool_created::PoolCreated, swap::SwapEvent},
    exchanges::extended::DexExtended,
    hypersync::helpers::validate_event_signature_hash,
};

const POOL_CREATED_EVENT_SIGNATURE_HASH: &str =
    "783cca1c0412dd0d695e784568c96da2e9c22ff989357a2e8b1d9b2b4e6b7118";
const SWAP_EVENT_SIGNATURE_HASH: &str =
    "c42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67";

/// Uniswap V3 DEX on Ethereum.
pub static UNISWAP_V3: LazyLock<DexExtended> = LazyLock::new(|| {
    let mut dex = DexExtended::new(Dex::new(
        chains::ETHEREUM.clone(),
        "Uniswap V3",
        "0x1F98431c8aD98523631AE4a59f267346ea31F984",
        AmmType::CLAMM,
        "PoolCreated(address,address,uint24,int24,address)",
        "Swap(address,address,int256,int256,uint160,uint128,int24)",
    ));
    dex.set_pool_created_event_parsing(parse_pool_created_event);
    dex.set_swap_event_parsing(parse_swap_event);
    dex.set_convert_trade_data(convert_to_trade_data);
    dex
});

fn parse_pool_created_event(log: Log) -> anyhow::Result<PoolCreated> {
    validate_event_signature_hash("PoolCreatedEvent", POOL_CREATED_EVENT_SIGNATURE_HASH, &log)?;

    let block_number = log
        .block_number
        .expect("Block number should be set in logs");

    let token = if let Some(topic) = log.topics.get(1).and_then(|t| t.as_ref()) {
        // Address is stored in the last 20 bytes of the 32-byte topic
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token0 address in topic1");
    };

    let token1 = if let Some(topic) = log.topics.get(2).and_then(|t| t.as_ref()) {
        Address::from_slice(&topic.as_ref()[12..32])
    } else {
        anyhow::bail!("Missing token1 address in topic2");
    };

    let fee = if let Some(topic) = log.topics.get(3).and_then(|t| t.as_ref()) {
        U256::from_be_slice(topic.as_ref()).as_limbs()[0] as u32
    } else {
        anyhow::bail!("Missing fee in topic3");
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

        Ok(PoolCreated::new(
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

// Define sol macro for easier parsing of data log object
// Data contains 5 parameters of 32 bytes each:
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

    let block_number = log
        .block_number
        .expect("Block number should be set in logs");

    let sender = match log.topics.get(1).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => return Err(anyhow::anyhow!("Missing sender address in topic1")),
    };

    let recipient = match log.topics.get(2).and_then(|t| t.as_ref()) {
        Some(topic) => Address::from_slice(&topic.as_ref()[12..32]),
        None => return Err(anyhow::anyhow!("Missing recipient address in topic2")),
    };

    if let Some(data) = log.data {
        let data_bytes = data.as_ref();

        // Validate if data contains 5 parameters of 32 bytes each
        if data_bytes.len() < 5 * 32 {
            return Err(anyhow::anyhow!("Swap event data is too short"));
        }

        // Decode the data using the SwapEventData struct
        let decoded = match <SwapEventData as SolType>::abi_decode(&data_bytes) {
            Ok(decoded) => decoded,
            Err(e) => return Err(anyhow::anyhow!("Failed to decode swap event data: {}", e)),
        };
        decoded.amount0;

        Ok(SwapEvent::new(
            block_number.into(),
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

fn convert_amount_to_f64(amount: I256, decimals: u8) -> f64 {
    let amount_str = amount.to_string();
    let amount_f64: f64 = amount_str.parse().expect("Failed to parse I256 to f64");
    let factor = 10f64.powi(decimals as i32);
    amount_f64 / factor
}

/// https://blog.uniswap.org/uniswap-v3-math-primer
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
    let token0_multiplier = 10u128.pow(token0_decimals as u32);
    let token1_multiplier = 10u128.pow(token1_decimals as u32);
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
    let quantity_f64 = convert_amount_to_f64(swap_event.amount1, token1.decimals).abs();
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
