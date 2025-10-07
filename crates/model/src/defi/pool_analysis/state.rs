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
    /// Total amount of token0 deposited through mints.
    pub total_amount0_deposited: U256,
    /// Total amount of token1 deposited through mints.
    pub total_amount1_deposited: U256,
    /// Total amount of token0 withdrawn through burns.
    pub total_amount0_withdrawn: U256,
    /// Total amount of token1 withdrawn through burns.
    pub total_amount1_withdrawn: U256,
    /// Accumulated protocol fees in token0 units.
    pub protocol_fees_token0: U256,
    /// Accumulated protocol fees in token1 units.
    pub protocol_fees_token1: U256,
    /// Protocol fee packed: lower 4 bits for token0, upper 4 bits for token1.
    pub fee_protocol: u8,
}

impl PoolState {
    /// Creates a new `PoolState` with the specified parameters.
    pub fn new(
        total_amount0_deposited: U256,
        total_amount1_deposited: U256,
        total_amount0_withdrawn: U256,
        total_amount1_withdrawn: U256,
        protocol_fees_token0: U256,
        protocol_fees_token1: U256,
        fee_protocol: u8,
    ) -> Self {
        Self {
            current_tick: 0,
            price_sqrt_ratio_x96: U160::ZERO,
            total_amount0_deposited,
            total_amount1_deposited,
            total_amount0_withdrawn,
            total_amount1_withdrawn,
            protocol_fees_token0,
            protocol_fees_token1,
            fee_protocol,
        }
    }
}

impl Default for PoolState {
    fn default() -> Self {
        Self {
            current_tick: 0,
            price_sqrt_ratio_x96: U160::ZERO,
            total_amount0_deposited: U256::ZERO,
            total_amount1_deposited: U256::ZERO,
            total_amount0_withdrawn: U256::ZERO,
            total_amount1_withdrawn: U256::ZERO,
            protocol_fees_token0: U256::ZERO,
            protocol_fees_token1: U256::ZERO,
            fee_protocol: 0,
        }
    }
}