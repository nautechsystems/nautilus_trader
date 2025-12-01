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

//! Uniswap V4 swap calldata generation.
//!
//! Builds V4 swap calldata via Universal Router using the V4Planner pattern.
//!
//! This binary is READ-ONLY: it computes and prints calldata only.
//! It never sends a transaction or mutates on-chain state.
//!
//! Run with:
//! ```bash
//! cargo run -p nautilus-blockchain --bin uniswap_v4_swap --features hypersync
//! ```

use alloy::primitives::{Address, Bytes, U256, address};
use nautilus_blockchain::{
    contracts::{
        uniswap_v4_pool::{UniswapV4PoolManagerContract, deployments},
        universal_router::{PoolKey, RoutePlanner, SwapExactInSingleParams, V4Planner, permit2},
    },
    execution::swap::{V4SwapParams, build_v4_swap_calldata, fee_tiers},
};

// Ethereum Mainnet token addresses
const WETH: Address = address!("C02aaA39b223FE8D0A0e5C4F27eAD9083C756Cc2");
const USDC: Address = address!("A0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48");
const USDT: Address = address!("dAC17F958D2ee523a2206206994597C13D831ec7");
const WBTC: Address = address!("2260FAC5E5542a773Aa44fBCfeDf7C193bc2C599");

// Fee tiers (hundredths of a bip)
const FEE_LOWEST: u32 = 100; // 0.01%
const FEE_MEDIUM: u32 = 3000; // 0.30%

fn main() -> anyhow::Result<()> {
    println!("=== Uniswap V4 Swap Calldata Generator ===\n");
    println!("NOTE: This binary is READ-ONLY.");
    println!("  • Computes calldata and prints it");
    println!("  • Does NOT send any transaction\n");

    let chain_id: u64 = 1;
    let pool_manager_address = deployments::pool_manager_address(chain_id)
        .ok_or_else(|| anyhow::anyhow!("PoolManager not deployed on chain {chain_id}"))?;

    println!("Network: Ethereum Mainnet (Chain ID: {chain_id})");
    println!("PoolManager: {pool_manager_address:?}\n");

    build_swap_calldata(chain_id)?;
    show_v4_planner_example();
    show_other_pool_ids();

    println!("=== Done ===");

    Ok(())
}

/// Builds V4 swap calldata for USDC -> WETH swap.
fn build_swap_calldata(chain_id: u64) -> anyhow::Result<()> {
    println!("=== V4 Swap Calldata Generation ===\n");

    let swap_amount: u128 = 1_000_000_000; // 1000 USDC (6 decimals)
    let min_output: u128 = 0; // WARNING: 0 = unlimited slippage
    let recipient = address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
    let deadline = u64::MAX; // WARNING: no expiry

    let (currency0, currency1) = sort_currencies(USDC, WETH);
    let (label0, label1) = sorted_symbol_labels(USDC, WETH, "USDC", "WETH");
    let fee = FEE_MEDIUM;
    let tick_spacing = tick_spacing_for_fee(fee);
    let hooks = Address::ZERO;

    println!("Pool Configuration (USDC/WETH 0.30%):");
    println!("  Currency0 ({label0}): {currency0:?}");
    println!("  Currency1 ({label1}): {currency1:?}");
    println!("  Fee: {:.2}%", f64::from(fee) / 10_000.0);
    println!("  Tick Spacing: {tick_spacing}");
    println!("  Hooks: {hooks:?}");

    let pool_id = UniswapV4PoolManagerContract::compute_pool_id(
        currency0,
        currency1,
        fee,
        tick_spacing,
        hooks,
    );
    println!("  Pool ID: 0x{}\n", hex::encode(pool_id));

    println!("Swap Parameters:");
    println!("  Token In:   USDC ({USDC:?})");
    println!("  Token Out:  WETH ({WETH:?})");
    println!("  Amount In:  {} USDC", swap_amount as f64 / 1_000_000.0);
    println!("  Fee Tier:   0.30%");
    println!("  Min Out:    {min_output} (WARNING: 0 = unlimited slippage)");
    println!("  Deadline:   MAX (WARNING: no expiry)\n");

    let swap_params = V4SwapParams::new(USDC, WETH, swap_amount, min_output, recipient, deadline)
        .with_fee(fee_tiers::FEE_MEDIUM.0, fee_tiers::FEE_MEDIUM.1);

    let (router_address, calldata) = build_v4_swap_calldata(&swap_params, chain_id)?;

    println!("Calldata Built:");
    println!("  Universal Router: {router_address:?}");
    println!("  Calldata length:  {} bytes", calldata.len());
    println!("\nCalldata (hex):");
    println!("0x{}\n", hex::encode(&calldata));

    println!("V4 Actions:");
    println!("  1. SWAP_EXACT_IN_SINGLE (0x06) - Execute swap");
    println!("  2. SETTLE_ALL (0x0c)           - Pay input tokens (USDC)");
    println!("  3. TAKE_ALL (0x0f)             - Collect output tokens (WETH)\n");

    println!("To execute on-chain:");
    println!("  1. Approve USDC to Permit2: {:?}", permit2::PERMIT2);
    println!("  2. Set Permit2 allowance for Universal Router");
    println!("  3. Send transaction to Universal Router with calldata above\n");

    Ok(())
}

/// Shows manual V4Planner construction.
fn show_v4_planner_example() {
    println!("=== Manual V4Planner Construction ===\n");

    let swap_amount: u128 = 1_000_000_000;
    let min_output: u128 = 0;

    let (pool_currency0, pool_currency1) = sort_currencies(USDC, WETH);
    let fee = FEE_MEDIUM;
    let tick_spacing = tick_spacing_for_fee(fee);

    let pool_key = PoolKey::new(
        pool_currency0,
        pool_currency1,
        fee,
        tick_spacing,
        Address::ZERO,
    );

    let zero_for_one = USDC == pool_currency0;
    let (token_in, token_out) = if zero_for_one {
        (pool_key.currency0, pool_key.currency1)
    } else {
        (pool_key.currency1, pool_key.currency0)
    };

    let mut v4_planner = V4Planner::new();

    v4_planner.add_swap_exact_in_single(SwapExactInSingleParams {
        pool_key,
        zero_for_one,
        amount_in: swap_amount,
        amount_out_minimum: min_output,
        hook_data: Bytes::new(),
    });

    v4_planner.add_settle_all(token_in, U256::from(swap_amount));
    v4_planner.add_take_all(token_out, U256::from(min_output));

    let mut route_planner = RoutePlanner::new();
    route_planner.add_v4_swap(&v4_planner);

    println!("  V4Planner actions: {:?}", v4_planner.actions);
    println!("  RoutePlanner commands: {:?}\n", route_planner.commands());
}

/// Computes pool IDs for other common pairs.
fn show_other_pool_ids() {
    println!("=== Other V4 Pool IDs ===\n");

    let pools = [
        (WETH, USDT, FEE_MEDIUM, "WETH/USDT 0.30%"),
        (USDC, USDT, FEE_LOWEST, "USDC/USDT 0.01%"),
        (WBTC, WETH, FEE_MEDIUM, "WBTC/WETH 0.30%"),
    ];

    for (token_a, token_b, pool_fee, name) in pools {
        let (c0, c1) = sort_currencies(token_a, token_b);
        let ts = tick_spacing_for_fee(pool_fee);
        let pid =
            UniswapV4PoolManagerContract::compute_pool_id(c0, c1, pool_fee, ts, Address::ZERO);
        println!("{name}: 0x{}", hex::encode(pid));
    }
    println!();
}

fn tick_spacing_for_fee(fee: u32) -> i32 {
    match fee {
        100 => 1,
        500 => 10,
        3000 => 60,
        10_000 => 200,
        _ => 60,
    }
}

fn sort_currencies(token_a: Address, token_b: Address) -> (Address, Address) {
    if token_a < token_b {
        (token_a, token_b)
    } else {
        (token_b, token_a)
    }
}

fn sorted_symbol_labels(
    token_a: Address,
    token_b: Address,
    label_a: &'static str,
    label_b: &'static str,
) -> (&'static str, &'static str) {
    if token_a < token_b {
        (label_a, label_b)
    } else {
        (label_b, label_a)
    }
}
