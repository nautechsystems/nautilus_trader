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
    defi::{
        Block, Blockchain, Chain, Dex, PoolLiquidityUpdate, PoolLiquidityUpdateType, PoolSwap,
        Token, hex::from_str_hex_to_u64,
    },
    enums::OrderSide,
    identifiers::InstrumentId,
    types::Quantity,
};
use ustr::Ustr;

use crate::{
    decode::{u256_to_price, u256_to_quantity},
    hypersync::helpers::{
        extract_block_number, extract_log_index, extract_transaction_hash,
        extract_transaction_index,
    },
};

/// Converts a HyperSync block format to our internal [`Block`] type.
///
/// # Errors
///
/// Returns an error if required block fields are missing or if hex parsing fails.
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

/// Converts a HyperSync log entry to a [`PoolSwap`].
///
/// # Errors
///
/// Returns an error if log parsing, data extraction, or conversion fails.
///
/// # Panics
///
/// Panics if byte array conversion fails during amount parsing.
#[allow(clippy::too_many_arguments)]
pub fn transform_hypersync_swap_log(
    chain_ref: Arc<Chain>,
    dex: Arc<Dex>,
    instrument_id: InstrumentId,
    pool_address: Address,
    token0: Arc<Token>,
    token1: Arc<Token>,
    block_timestamp: UnixNanos,
    log: &hypersync_client::simple_types::Log,
) -> Result<PoolSwap, anyhow::Error> {
    let block_number = extract_block_number(log)?;
    let transaction_hash = extract_transaction_hash(log)?;
    let transaction_index = extract_transaction_index(log)?;
    let log_index = extract_log_index(log)?;

    let sender = log
        .topics
        .get(1)
        .and_then(|t| t.as_ref())
        .map(|t| Address::from_slice(&t[12..32]))
        .ok_or_else(|| anyhow::anyhow!("Missing sender address in swap log"))?;

    let data = log
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing data field in swap log"))?;

    if data.len() < 160 {
        // 5 * 32 bytes = 160 bytes minimum
        anyhow::bail!("Insufficient data length for Uniswap V3 swap event");
    }

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

    tracing::debug!(
        "Raw amounts: amount0_signed={}, amount1_signed={}, amount0={}, amount1={}",
        amount0_signed,
        amount1_signed,
        amount0,
        amount1
    );

    let side = if amount0_signed.is_positive() {
        OrderSide::Sell // Selling token0 (pool received token0)
    } else {
        OrderSide::Buy // Buying token0 (pool gave token0)
    };

    let quantity = if token0.decimals == 18 {
        Quantity::from_wei(amount0)
    } else {
        u256_to_quantity(amount0, token0.decimals)?
    };

    let amount1_quantity = if token1.decimals == 18 {
        Quantity::from_wei(amount1)
    } else {
        u256_to_quantity(amount1, token1.decimals)?
    };

    tracing::debug!(
        "Converted amounts: amount0={} -> {} {}, amount1={} -> {} {}",
        amount0,
        quantity,
        token0.symbol,
        amount1,
        amount1_quantity,
        token1.symbol
    );

    let price = if !amount0.is_zero() && !amount1.is_zero() {
        // Calculate price as amount1/amount0, adjusting for decimal differences
        // Price precision should account for token decimal differences
        let price_precision = token1.decimals.min(9);

        // Scale amount1 to have same precision as token0, then divide
        let amount0_scaled = if token0.decimals > token1.decimals {
            amount0 / U256::from(10_u128.pow(u32::from(token0.decimals - token1.decimals)))
        } else if token1.decimals > token0.decimals {
            amount0 * U256::from(10_u128.pow(u32::from(token1.decimals - token0.decimals)))
        } else {
            amount0
        };

        // Calculate price with appropriate scaling
        let price_raw =
            amount1 * U256::from(10_u128.pow(u32::from(price_precision))) / amount0_scaled;

        u256_to_price(price_raw, price_precision)?
    } else {
        anyhow::bail!("Invalid swap: amount0 or amount1 is zero, cannot calculate price");
    };

    let swap = PoolSwap::new(
        chain_ref,
        dex,
        instrument_id,
        pool_address,
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

/// Converts a HyperSync log entry to a [`PoolLiquidityUpdate`] for mint events.
///
/// # Errors
///
/// Returns an error if log parsing, data extraction, or conversion fails.
///
/// # Panics
///
/// Panics if the byte array conversion fails during amount parsing.
#[allow(clippy::too_many_arguments)]
pub fn transform_hypersync_mint_log(
    chain_ref: Arc<Chain>,
    dex: Arc<Dex>,
    instrument_id: InstrumentId,
    pool_address: Address,
    token0: Arc<Token>,
    token1: Arc<Token>,
    block_timestamp: UnixNanos,
    log: &hypersync_client::simple_types::Log,
) -> Result<PoolLiquidityUpdate, anyhow::Error> {
    let block_number = extract_block_number(log)?;
    let transaction_hash = extract_transaction_hash(log)?;
    let transaction_index = extract_transaction_index(log)?;
    let log_index = extract_log_index(log)?;

    let sender = log
        .topics
        .get(1)
        .and_then(|t| t.as_ref())
        .map(|t| Address::from_slice(&t[12..32]))
        .ok_or_else(|| anyhow::anyhow!("Missing sender address in mint log"))?;

    let owner = log
        .topics
        .get(2)
        .and_then(|t| t.as_ref())
        .map(|t| Address::from_slice(&t[12..32]))
        .ok_or_else(|| anyhow::anyhow!("Missing owner address in mint log"))?;

    let data = log
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing data field in mint log"))?;

    if data.len() < 160 {
        // 5 * 32 bytes = 160 bytes minimum (amount, amount0, amount1, tick_lower, tick_upper)
        anyhow::bail!("Insufficient data length for Uniswap V3 mint event");
    }

    let amount_bytes = &data[0..32];
    let amount0_bytes = &data[32..64];
    let amount1_bytes = &data[64..96];

    let amount = U256::from_be_bytes::<32>(amount_bytes.try_into().unwrap());
    let amount0 = U256::from_be_bytes::<32>(amount0_bytes.try_into().unwrap());
    let amount1 = U256::from_be_bytes::<32>(amount1_bytes.try_into().unwrap());

    let liquidity = u256_to_quantity(amount, 18)?; // Liquidity is typically 18 decimals
    let amount0_quantity = u256_to_quantity(amount0, token0.decimals)?;
    let amount1_quantity = u256_to_quantity(amount1, token1.decimals)?;

    // Extract tick information from topics (indexed parameters)
    let tick_lower = if let Some(topic3) = log.topics.get(3).and_then(|t| t.as_ref()) {
        i32::from_be_bytes([topic3[28], topic3[29], topic3[30], topic3[31]])
    } else {
        0 // Default value if not available
    };

    let tick_upper = if let Some(topic4) = log.topics.get(4).and_then(|t| t.as_ref()) {
        i32::from_be_bytes([topic4[28], topic4[29], topic4[30], topic4[31]])
    } else {
        0 // Default value if not available
    };

    let liquidity_update = PoolLiquidityUpdate::new(
        chain_ref,
        dex,
        instrument_id,
        pool_address,
        PoolLiquidityUpdateType::Mint,
        block_number,
        transaction_hash,
        transaction_index,
        log_index,
        Some(sender),
        owner,
        liquidity,
        amount0_quantity,
        amount1_quantity,
        tick_lower,
        tick_upper,
        block_timestamp,
    );

    Ok(liquidity_update)
}

/// Converts a HyperSync log entry to a [`PoolLiquidityUpdate`] for burn events.
///
/// # Errors
///
/// Returns an error if log parsing, data extraction, or conversion fails.
///
/// # Panics
///
/// Panics if the byte array conversion fails during amount parsing.
#[allow(clippy::too_many_arguments)]
pub fn transform_hypersync_burn_log(
    chain_ref: Arc<Chain>,
    dex: Arc<Dex>,
    instrument_id: InstrumentId,
    pool_address: Address,
    token0: Arc<Token>,
    token1: Arc<Token>,
    block_timestamp: UnixNanos,
    log: &hypersync_client::simple_types::Log,
) -> Result<PoolLiquidityUpdate, anyhow::Error> {
    let block_number = extract_block_number(log)?;
    let transaction_hash = extract_transaction_hash(log)?;
    let transaction_index = extract_transaction_index(log)?;
    let log_index = extract_log_index(log)?;

    let owner = log
        .topics
        .get(1)
        .and_then(|t| t.as_ref())
        .map(|t| Address::from_slice(&t[12..32]))
        .ok_or_else(|| anyhow::anyhow!("Missing owner address in burn log"))?;

    let data = log
        .data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Missing data field in burn log"))?;

    if data.len() < 96 {
        // 3 * 32 bytes = 96 bytes minimum (amount, amount0, amount1)
        anyhow::bail!("Insufficient data length for Uniswap V3 burn event");
    }

    let amount_bytes = &data[0..32];
    let amount0_bytes = &data[32..64];
    let amount1_bytes = &data[64..96];

    let amount = U256::from_be_bytes::<32>(amount_bytes.try_into().unwrap());
    let amount0 = U256::from_be_bytes::<32>(amount0_bytes.try_into().unwrap());
    let amount1 = U256::from_be_bytes::<32>(amount1_bytes.try_into().unwrap());

    let liquidity = u256_to_quantity(amount, 18)?; // Liquidity is typically 18 decimals
    let amount0_quantity = u256_to_quantity(amount0, token0.decimals)?;
    let amount1_quantity = u256_to_quantity(amount1, token1.decimals)?;

    // Extract tick information from topics (indexed parameters)
    let tick_lower = if let Some(topic2) = log.topics.get(2).and_then(|t| t.as_ref()) {
        i32::from_be_bytes([topic2[28], topic2[29], topic2[30], topic2[31]])
    } else {
        0 // Default value if not available
    };

    let tick_upper = if let Some(topic3) = log.topics.get(3).and_then(|t| t.as_ref()) {
        i32::from_be_bytes([topic3[28], topic3[29], topic3[30], topic3[31]])
    } else {
        0 // Default value if not available
    };

    let liquidity_update = PoolLiquidityUpdate::new(
        chain_ref,
        dex,
        instrument_id,
        pool_address,
        PoolLiquidityUpdateType::Burn,
        block_number,
        transaction_hash,
        transaction_index,
        log_index,
        None, // Burn events don't have a sender
        owner,
        liquidity,
        amount0_quantity,
        amount1_quantity,
        tick_lower,
        tick_upper,
        block_timestamp,
    );

    Ok(liquidity_update)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::defi::{AmmType, Chain, Dex, Pool, Token};
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
            0,
            AmmType::CLAMM,
            "PoolCreated(address,address,uint24,int24,address)",
            "Swap(address,address,int256,int256,uint160,uint128,int24)",
            "Mint(address,address,int24,int24,uint128,uint256,uint256)",
            "Burn(address,int24,int24,uint128,uint256,uint256)",
        ));

        let token0 = Arc::new(Token::new(
            chain.clone(),
            "0xA0b86a33E6441b936662bb6B5d1F8Fb0E2b57A5D"
                .parse()
                .unwrap(),
            "Wrapped Ether".to_string(),
            "WETH".to_string(),
            18,
        ));

        let token1 = Arc::new(Token::new(
            chain.clone(),
            "0xdAC17F958D2ee523a2206206994597C13D831ec7"
                .parse()
                .unwrap(),
            "Tether USD".to_string(),
            "USDT".to_string(),
            6,
        ));

        let pool = Arc::new(Pool::new(
            chain.clone(),
            (*dex).clone(),
            "0x11b815efB8f581194ae79006d24E0d814B7697F6"
                .parse()
                .unwrap(),
            12345678,
            (*token0).clone(),
            (*token1).clone(),
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
            chain,
            dex,
            pool.instrument_id,
            pool.address,
            token0,
            token1,
            UnixNanos::default(),
            &log,
        );

        assert!(
            result.is_ok(),
            "Transform should succeed with valid log data"
        );
        let swap = result.unwrap();

        // Assert all fields are correctly transformed
        assert_eq!(swap.block, 0x1581b7e);
        assert_eq!(
            swap.transaction_hash,
            "0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef"
        );
        assert_eq!(swap.transaction_index, 5);
        assert_eq!(swap.log_index, 10);
        assert_eq!(swap.timestamp, UnixNanos::default());
        assert_eq!(
            swap.sender,
            "0x3fc91a3afd70395cd496c647d5a6cc9d4b2b7fad"
                .parse::<Address>()
                .unwrap()
        );
        assert_eq!(swap.side, OrderSide::Sell); // amount0 is positive (1 ETH), so selling token0

        // Test data has amount0 = 1 ETH (0x0de0b6b3a7640000) and amount1 = 500 USDT (0x1dcd6500)
        // amount0 = 1000000000000000000 wei = 1.0 ETH
        assert_eq!(swap.size.precision, 18);
        assert_eq!(swap.size.raw, 1_000_000_000_000_000_000_u128); // 1 ETH in wei

        // Price should be amount1/amount0 = 500 USDT / 1 ETH = 500.0
        assert_eq!(swap.price.as_f64(), 500.0);
        assert_eq!(swap.price.precision, 6); // USDT has 6 decimals, so price uses 6 precision for the quote token
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
        assert_eq!(
            transformed_block.miner.as_str(),
            "0x0000000000000000000000000000000000000000"
        );
        assert_eq!(transformed_block.gas_limit, 0x1c9c380);
        assert_eq!(transformed_block.gas_used, 0x5208);

        // timestamp 0x61bc3f2d = 1639726893 seconds = 1639726893000000000 nanoseconds
        let expected_timestamp = UnixNanos::new(1639726893 * NANOSECONDS_IN_SECOND);
        assert_eq!(transformed_block.timestamp, expected_timestamp);

        assert_eq!(transformed_block.chain, Some(Blockchain::Ethereum));

        // Optional fields should be None when not provided in test data
        assert!(transformed_block.base_fee_per_gas.is_none());
        assert!(transformed_block.blob_gas_used.is_none());
        assert!(transformed_block.excess_blob_gas.is_none());
    }
}
