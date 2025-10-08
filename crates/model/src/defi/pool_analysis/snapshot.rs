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

use alloy_primitives::{U160, U256};

use crate::defi::{
    data::block::BlockPosition, pool_analysis::position::PoolPosition, tick_map::tick::Tick,
};

/// Complete snapshot of a liquidity pool's state at a specific point in time.
///
/// `PoolSnapshot` provides a comprehensive, self-contained representation of a pool's
/// entire state, bundling together the global state variables, all liquidity positions,
/// and the complete tick distribution.
#[derive(Debug, Clone)]
pub struct PoolSnapshot {
    /// Global pool state including price, tick, fees, and cumulative flows.
    pub state: PoolState,
    /// All liquidity positions in the pool.
    pub positions: Vec<PoolPosition>,
    /// Complete tick distribution across the pool's price range.
    pub ticks: Vec<Tick>,
    /// Analytics counters for the pool.
    pub analytics: PoolAnalytics,
    /// Block position where this snapshot was taken.
    pub block_position: BlockPosition,
}

impl PoolSnapshot {
    /// Creates a new `PoolSnapshot` with the specified state, positions, ticks, analytics, and block position.
    pub fn new(
        state: PoolState,
        positions: Vec<PoolPosition>,
        ticks: Vec<Tick>,
        analytics: PoolAnalytics,
        block_position: BlockPosition,
    ) -> Self {
        Self {
            state,
            positions,
            ticks,
            analytics,
            block_position,
        }
    }
}

/// Global state snapshot of a liquidity pool at a specific point in time.
///
/// `PoolState` encapsulates the core global variables that define a UniswapV3-style
/// AMM pool's current state. This includes the current price position, cumulative
/// deposit/withdrawal flows, and protocol fee configuration.
#[derive(Debug, Clone)]
pub struct PoolState {
    /// Current tick position of the pool price.
    pub current_tick: i32,
    /// Current sqrt price ratio as Q64.96 fixed point number.
    pub price_sqrt_ratio_x96: U160,
    /// Current active liquidity in the pool.
    pub liquidity: u128,
    /// Accumulated protocol fees in token0 units.
    pub protocol_fees_token0: U256,
    /// Accumulated protocol fees in token1 units.
    pub protocol_fees_token1: U256,
    /// Protocol fee packed: lower 4 bits for token0, upper 4 bits for token1.
    pub fee_protocol: u8,
    /// Global fee growth for token0 as Q128.128 fixed-point number.
    pub fee_growth_global_0: U256,
    /// Global fee growth for token1 as Q128.128 fixed-point number.
    pub fee_growth_global_1: U256,
}

impl PoolState {
    /// Creates a new `PoolState` with the specified parameters.
    pub fn new(protocol_fees_token0: U256, protocol_fees_token1: U256, fee_protocol: u8) -> Self {
        Self {
            current_tick: 0,
            price_sqrt_ratio_x96: U160::ZERO,
            liquidity: 0,
            protocol_fees_token0,
            protocol_fees_token1,
            fee_protocol,
            fee_growth_global_0: U256::ZERO,
            fee_growth_global_1: U256::ZERO,
        }
    }
}

impl Default for PoolState {
    fn default() -> Self {
        Self {
            current_tick: 0,
            price_sqrt_ratio_x96: U160::ZERO,
            liquidity: 0,
            protocol_fees_token0: U256::ZERO,
            protocol_fees_token1: U256::ZERO,
            fee_protocol: 0,
            fee_growth_global_0: U256::ZERO,
            fee_growth_global_1: U256::ZERO,
        }
    }
}

/// Analytics counters and metrics for pool operations.
///
/// It tracks cumulative statistics about pool activity, including
/// deposit and collection flows, event counts, and performance metrics for debugging.
#[derive(Debug, Clone)]
pub struct PoolAnalytics {
    /// Total amount of token0 deposited through mints.
    pub total_amount0_deposited: U256,
    /// Total amount of token1 deposited through mints.
    pub total_amount1_deposited: U256,
    /// Total amount of token0 collected
    pub total_amount0_collected: U256,
    /// Total amount of token1 collected.
    pub total_amount1_collected: U256,
    /// Total number of swap events processed.
    pub total_swaps: u64,
    /// Total number of mint events processed.
    pub total_mints: u64,
    /// Total number of burn events processed.
    pub total_burns: u64,
    /// Total number of fee collection events processed.
    pub total_fee_collects: u64,
    /// Time spent processing swap events (debug builds only).
    #[cfg(debug_assertions)]
    pub swap_processing_time: std::time::Duration,
    /// Time spent processing mint events (debug builds only).
    #[cfg(debug_assertions)]
    pub mint_processing_time: std::time::Duration,
    /// Time spent processing burn events (debug builds only).
    #[cfg(debug_assertions)]
    pub burn_processing_time: std::time::Duration,
    /// Time spent processing collect events (debug builds only).
    #[cfg(debug_assertions)]
    pub collect_processing_time: std::time::Duration,
}

impl Default for PoolAnalytics {
    fn default() -> Self {
        Self {
            total_amount0_deposited: U256::ZERO,
            total_amount1_deposited: U256::ZERO,
            total_amount0_collected: U256::ZERO,
            total_amount1_collected: U256::ZERO,
            total_swaps: 0,
            total_mints: 0,
            total_burns: 0,
            total_fee_collects: 0,
            #[cfg(debug_assertions)]
            swap_processing_time: std::time::Duration::ZERO,
            #[cfg(debug_assertions)]
            mint_processing_time: std::time::Duration::ZERO,
            #[cfg(debug_assertions)]
            burn_processing_time: std::time::Duration::ZERO,
            #[cfg(debug_assertions)]
            collect_processing_time: std::time::Duration::ZERO,
        }
    }
}
