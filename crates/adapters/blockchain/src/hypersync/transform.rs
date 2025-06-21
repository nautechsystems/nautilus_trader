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

use std::sync::Arc;

use alloy::primitives::{Address, I256, U256};
use hypersync_client::format::Hex;
use nautilus_core::{UnixNanos, datetime::NANOSECONDS_IN_SECOND};
use nautilus_model::{
    defi::{Block, Blockchain, Chain, Dex, Pool, PoolSwap, hex::from_str_hex_to_u64},
    enums::OrderSide,
};
use ustr::Ustr;

use crate::hypersync::helpers::*;

/// Converts a HyperSync block format to our internal [`Block`] type.
pub fn transform_hypersync_block(
    chain: Blockchain,
    received_block: hypersync_client::simple_types::Block,
) -> Result<Block, anyhow::Error> {
    let number = received_block
        .number
        .ok_or_else(|| anyhow::anyhow!("Missing block number"))?;
    let gas_limit = from_str_hex_to_u64(
        received_block
            .gas_limit
            .ok_or_else(|| anyhow::anyhow!("Missing gas limit"))?
            .encode_hex()
            .as_str(),
    )?;
    let gas_used = from_str_hex_to_u64(
        received_block
            .gas_used
            .ok_or_else(|| anyhow::anyhow!("Missing gas used"))?
            .encode_hex()
            .as_str(),
    )?;
    let timestamp = from_str_hex_to_u64(
        received_block
            .timestamp
            .ok_or_else(|| anyhow::anyhow!("Missing timestamp"))?
            .encode_hex()
            .as_str(),
    )?;

    let mut block = Block::new(
        received_block
            .hash
            .ok_or_else(|| anyhow::anyhow!("Missing hash"))?
            .to_string(),
        received_block
            .parent_hash
            .ok_or_else(|| anyhow::anyhow!("Missing parent hash"))?
            .to_string(),
        number,
        Ustr::from(
            received_block
                .miner
                .ok_or_else(|| anyhow::anyhow!("Missing miner"))?
                .to_string()
                .as_str(),
        ),
        gas_limit,
        gas_used,
        UnixNanos::new(timestamp * NANOSECONDS_IN_SECOND),
        Some(chain),
    );

    if let Some(base_fee_hex) = received_block.base_fee_per_gas {
        let s = base_fee_hex.encode_hex();
        let val = U256::from_str_radix(s.trim_start_matches("0x"), 16)?;
        block = block.with_base_fee(val);
    }

    if let (Some(used_hex), Some(excess_hex)) =
        (received_block.blob_gas_used, received_block.excess_blob_gas)
    {
        let used = U256::from_str_radix(used_hex.encode_hex().trim_start_matches("0x"), 16)?;
        let excess = U256::from_str_radix(excess_hex.encode_hex().trim_start_matches("0x"), 16)?;
        block = block.with_blob_gas(used, excess);
    }

    // TODO: HyperSync does not yet publush L1 gas metadata fields
    // if let (Some(price_hex), Some(l1_used_hex), Some(scalar_hex)) = (
    //     received_block.l1_gas_price,
    //     received_block.l1_gas_used,
    //     received_block.l1_fee_scalar,
    // ) {
    //     let price = U256::from_str_radix(price_hex.encode_hex().trim_start_matches("0x"), 16)?;
    //     let used = from_str_hex_to_u64(l1_used_hex.encode_hex().as_str())?;
    //     let scalar = from_str_hex_to_u64(scalar_hex.encode_hex().as_str())?;
    //     block = block.with_l1_fee_components(price, used, scalar);
    // }

    Ok(block)
}

/// Converts a HyperSync log entry to a [`PoolSwap`] using provided context.
pub fn transform_hypersync_swap_log(
    chain_ref: Arc<Chain>,
    dex: Arc<Dex>,
    pool: Arc<Pool>,
    block_timestamp: UnixNanos,
    log: &hypersync_client::simple_types::Log,
) -> Result<PoolSwap, anyhow::Error> {
    // Extract log information using helper functions
    let block_number = extract_block_number(log)?;
    let transaction_hash = extract_transaction_hash(log)?;
    let transaction_index = extract_transaction_index(log)?;
    let log_index = extract_log_index(log)?;

    // Parse swap event data from log
    // For Uniswap V3: Swap(address indexed sender, address indexed recipient, int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
    let sender = log
        .topics
        .get(1)
        .and_then(|t| t.as_ref())
        .map(|t| Address::from_slice(&t[12..32]))
        .ok_or_else(|| anyhow::anyhow!("Missing sender address in swap log"))?;

    // Parse swap event data from log
    // Uniswap V3 Swap event data structure: (int256 amount0, int256 amount1, uint160 sqrtPriceX96, uint128 liquidity, int24 tick)
    let data = log
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing data field in swap log"))?;

    if data.len() < 160 {
        // 5 * 32 bytes = 160 bytes minimum
        anyhow::bail!("Insufficient data length for Uniswap V3 swap event");
    }

    // Decode the data field (each field is 32 bytes)
    let amount0_bytes = &data[0..32];
    let amount1_bytes = &data[32..64];

    // Convert signed integers (int256) - handle negative amounts
    let amount0_signed = I256::from_be_bytes::<32>(amount0_bytes.try_into().unwrap());
    let amount1_signed = I256::from_be_bytes::<32>(amount1_bytes.try_into().unwrap());

    // Get absolute values for quantity calculations
    let amount0 = if amount0_signed.is_negative() {
        U256::from(-amount0_signed)
    } else {
        U256::from(amount0_signed)
    };
    let amount1 = if amount1_signed.is_negative() {
        U256::from(-amount1_signed)
    } else {
        U256::from(amount1_signed)
    };

    // Determine trade side based on amount signs
    // In Uniswap V3: negative amount0 means token0 is being sold (going out), positive means being bought (coming in)
    let side = if amount0_signed.is_positive() {
        OrderSide::Buy // Buying token0
    } else {
        OrderSide::Sell // Selling token0
    };

    // Convert amounts to quantity and price using proper decimal handling
    let quantity = crate::decode::u256_to_quantity(amount0, pool.token0.decimals as u8)?;

    let price = if !amount0.is_zero() {
        // Calculate price as amount1/amount0, adjusting for decimal differences
        // If tokens have different decimals, we need to normalize
        let decimals_diff = pool.token1.decimals as i32 - pool.token0.decimals as i32;
        let price_amount = if decimals_diff >= 0 {
            // token1 has more or equal decimals
            (amount1 * U256::from(10_u128.pow(decimals_diff as u32))) / amount0
        } else {
            // token0 has more decimals
            amount1 / (amount0 * U256::from(10_u128.pow((-decimals_diff) as u32)))
        };
        crate::decode::u256_to_price(price_amount, pool.token1.decimals as u8)?
    } else {
        anyhow::bail!("Invalid swap: amount0 is zero, cannot calculate price");
    };

    let swap = PoolSwap::new(
        chain_ref,
        dex,
        pool,
        block_number,
        transaction_hash,
        transaction_index,
        log_index,
        block_timestamp,
        sender,
        side,
        quantity,
        price,
    );

    Ok(swap)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::defi::{AmmType, Chain, Dex, Token};
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn test_transform_hypersync_swap_log() {
        let chain = Arc::new(Chain::new(Blockchain::Ethereum, 1));

        let dex = Arc::new(Dex::new(
            (*chain).clone(),
            "Uniswap V3",
            "0x1F98431c8aD98523631AE4a59f267346ea31F984",
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
        ));

        let token0 = Token::new(
            chain.clone(),
            "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
                .parse()
                .unwrap(),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        );

        let token1 = Token::new(
            chain.clone(),
            "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            "Tether USD".to_string(),
            "USDT".to_string(),
            6,
        );

        let pool = Arc::new(Pool::new(
            chain.clone(),
            (*dex).clone(),
            "0x11b815efB8f581194ae79006d24E0d814B7697F6"
                .parse()
                .unwrap(),
            12345678,
            token0,
            token1,
            3000,
            60,
            UnixNanos::default(),
        ));

        let log_json = json!({
            "block_number": "0x1581b7e",
            "transaction_hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "transaction_index": "0x5",
            "log_index": "0xa",
            "data": "0x0000000000000000000000000000000000000000000000000de0b6b3a7640000000000000000000000000000000000000000000000000000000000001dcd6500000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000",
            "topics": [
                "0xc42079f94a6350d7e6235f29174924f928cc2ac818eb64fed8004e115fbcca67",
                "0x0000000000000000000000003fc91a3afd70395cd496c647d5a6cc9d4b2b7fad",
                "0x0000000000000000000000003fc91a3afd70395cd496c647d5a6cc9d4b2b7fad"
            ]
        });

        let log: hypersync_client::simple_types::Log =
            serde_json::from_value(log_json).expect("Failed to deserialize log");

        let result = transform_hypersync_swap_log(
            chain.clone(),
            dex.clone(),
            pool.clone(),
            UnixNanos::default(),
            &log,
        );

        assert!(
            result.is_ok(),
            "Transform should succeed with valid log data"
        );
        let swap = result.unwrap();

        assert_eq!(swap.block, 0x1581b7e);
        assert_eq!(
            swap.transaction_hash,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        );
        assert_eq!(swap.transaction_index, 5);
        assert_eq!(swap.log_index, 10);
    }

    #[rstest]
    fn test_transform_hypersync_block() {
        let block_json = json!({
            "number": 0x1581b7e_u64,
            "hash": "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef",
            "parent_hash": "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890",
            "miner": "0x0000000000000000000000000000000000000000",
            "gas_limit": "0x1c9c380",
            "gas_used": "0x5208",
            "timestamp": "0x61bc3f2d"
        });

        let block: hypersync_client::simple_types::Block =
            serde_json::from_value(block_json).expect("Failed to deserialize block");

        let result = transform_hypersync_block(Blockchain::Ethereum, block);

        assert!(
            result.is_ok(),
            "Transform should succeed with valid block data"
        );
        let transformed_block = result.unwrap();

        assert_eq!(transformed_block.number, 0x1581b7e);
        assert_eq!(
            transformed_block.hash,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        );
        assert_eq!(
            transformed_block.parent_hash,
            "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890"
        );
        assert_eq!(transformed_block.gas_limit, 0x1c9c380);
        assert_eq!(transformed_block.gas_used, 0x5208);
    }
}
