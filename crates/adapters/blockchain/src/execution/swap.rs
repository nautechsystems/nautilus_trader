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

//! Uniswap V4 swap execution via Universal Router.
//!
//! This module provides swap execution functionality using the V4 PoolManager
//! through the Universal Router contract.

use alloy::primitives::{Address, Bytes, U256};
use thiserror::Error;

use crate::contracts::universal_router::{
    PoolKey, RoutePlanner, SwapExactInSingleParams, V4Planner, deployments::get_router_address,
};

/// Errors that can occur during V4 swap execution.
#[derive(Debug, Error)]
pub enum V4SwapError {
    #[error("Unsupported chain ID: {0}")]
    UnsupportedChain(u64),

    #[error("No private key configured for signing")]
    NoPrivateKey,

    #[error("Invalid token address: {0}")]
    InvalidTokenAddress(String),

    #[error("Insufficient balance for swap")]
    InsufficientBalance,

    #[error("Transaction failed: {0}")]
    TransactionFailed(String),

    #[error("RPC error: {0}")]
    RpcError(String),
}

/// Parameters for executing a V4 swap.
#[derive(Debug, Clone)]
pub struct V4SwapParams {
    /// Input token address
    pub token_in: Address,
    /// Output token address
    pub token_out: Address,
    /// Pool fee in hundredths of a bip (e.g., 500 = 0.05%)
    pub fee: u32,
    /// Pool tick spacing
    pub tick_spacing: i32,
    /// Hooks contract address (Address::ZERO for no hooks)
    pub hooks: Address,
    /// Amount of input token
    pub amount_in: u128,
    /// Minimum amount of output token (slippage protection)
    pub amount_out_minimum: u128,
    /// Recipient address for output tokens
    pub recipient: Address,
    /// Unix timestamp deadline
    pub deadline: u64,
}

impl V4SwapParams {
    /// Create swap params with default settings (no hooks, 0.3% fee).
    #[must_use]
    pub fn new(
        token_in: Address,
        token_out: Address,
        amount_in: u128,
        amount_out_minimum: u128,
        recipient: Address,
        deadline: u64,
    ) -> Self {
        Self {
            token_in,
            token_out,
            fee: 3000, // 0.30%
            tick_spacing: 60,
            hooks: Address::ZERO,
            amount_in,
            amount_out_minimum,
            recipient,
            deadline,
        }
    }

    /// Set the pool fee tier.
    #[must_use]
    pub fn with_fee(mut self, fee: u32, tick_spacing: i32) -> Self {
        self.fee = fee;
        self.tick_spacing = tick_spacing;
        self
    }

    /// Set the hooks contract.
    #[must_use]
    pub fn with_hooks(mut self, hooks: Address) -> Self {
        self.hooks = hooks;
        self
    }
}

/// Builds the calldata for a V4 swap via Universal Router.
///
/// Uses the Uniswap V4 SDK recommended pattern:
/// 1. `SWAP_EXACT_IN_SINGLE` - Execute the swap
/// 2. `SETTLE_ALL` - Pay all input tokens
/// 3. `TAKE_ALL` - Collect all output tokens
///
/// # Arguments
/// * `params` - Swap parameters
/// * `chain_id` - Target chain ID
///
/// # Returns
/// Tuple of (router_address, calldata) for the swap transaction
///
/// # Errors
///
/// Returns `V4SwapError::UnsupportedChain` if the chain ID is not supported.
pub fn build_v4_swap_calldata(
    params: &V4SwapParams,
    chain_id: u64,
) -> Result<(Address, Bytes), V4SwapError> {
    let router_address =
        get_router_address(chain_id).ok_or(V4SwapError::UnsupportedChain(chain_id))?;

    let pool_key = PoolKey::new(
        params.token_in,
        params.token_out,
        params.fee,
        params.tick_spacing,
        params.hooks,
    );

    // Determine swap direction based on token sorting
    // zeroForOne = true means swapping currency0 -> currency1
    let zero_for_one = params.token_in == pool_key.currency0;

    // Build V4Planner actions following SDK pattern
    let mut v4_planner = V4Planner::new();

    // 1. SWAP_EXACT_IN_SINGLE - Perform the swap
    v4_planner.add_swap_exact_in_single(SwapExactInSingleParams {
        pool_key: pool_key.clone(),
        zero_for_one,
        amount_in: params.amount_in,
        amount_out_minimum: params.amount_out_minimum,
        hook_data: Bytes::new(),
    });

    // 2. SETTLE_ALL - Pay all input tokens (currency0 if zeroForOne, else currency1)
    let settle_currency = if zero_for_one {
        pool_key.currency0
    } else {
        pool_key.currency1
    };
    v4_planner.add_settle_all(settle_currency, U256::from(params.amount_in));

    // 3. TAKE_ALL - Collect all output tokens (currency1 if zeroForOne, else currency0)
    let take_currency = if zero_for_one {
        pool_key.currency1
    } else {
        pool_key.currency0
    };
    v4_planner.add_take_all(take_currency, U256::from(params.amount_out_minimum));

    // Build route planner with V4_SWAP command
    let mut route_planner = RoutePlanner::new();
    route_planner.add_v4_swap(&v4_planner);

    // Encode the execute call
    let calldata = route_planner.encode_execute(U256::from(params.deadline));

    Ok((router_address, calldata))
}

/// Calculate the minimum output amount with slippage tolerance.
///
/// # Arguments
/// * `expected_output` - Expected output amount from quote
/// * `slippage_bps` - Slippage tolerance in basis points (e.g., 50 = 0.5%)
#[must_use]
pub fn calculate_min_output(expected_output: u128, slippage_bps: u32) -> u128 {
    let slippage_factor = 10_000 - slippage_bps as u128;
    expected_output * slippage_factor / 10_000
}

/// Common fee tiers for V4 pools.
pub mod fee_tiers {
    /// 0.01% fee, 1 tick spacing (stable pairs)
    pub const FEE_LOWEST: (u32, i32) = (100, 1);
    /// 0.05% fee, 10 tick spacing
    pub const FEE_LOW: (u32, i32) = (500, 10);
    /// 0.30% fee, 60 tick spacing (default)
    pub const FEE_MEDIUM: (u32, i32) = (3000, 60);
    /// 1.00% fee, 200 tick spacing (exotic pairs)
    pub const FEE_HIGH: (u32, i32) = (10000, 200);
}

#[cfg(test)]
mod tests {
    use alloy::primitives::address;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_build_swap_calldata() {
        let usdc = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
        let weth = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
        let recipient = address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        let params = V4SwapParams::new(
            usdc,
            weth,
            1_000_000, // 1 USDC
            0,
            recipient,
            u64::MAX,
        );

        let result = build_v4_swap_calldata(&params, 1);
        assert!(result.is_ok());

        let (router, calldata) = result.unwrap();
        assert!(!calldata.is_empty());
        assert_eq!(router, address!("66a9893cc07d91d95644aedd05d03f95e1dba8af"));
    }

    #[rstest]
    fn test_unsupported_chain() {
        let params = V4SwapParams::new(Address::ZERO, Address::ZERO, 0, 0, Address::ZERO, 0);

        let result = build_v4_swap_calldata(&params, 999999);
        assert!(matches!(result, Err(V4SwapError::UnsupportedChain(999999))));
    }

    #[rstest]
    fn test_calculate_min_output() {
        // 1000 tokens with 0.5% slippage
        let min = calculate_min_output(1000, 50);
        assert_eq!(min, 995);

        // 1000 tokens with 1% slippage
        let min = calculate_min_output(1000, 100);
        assert_eq!(min, 990);
    }
}
