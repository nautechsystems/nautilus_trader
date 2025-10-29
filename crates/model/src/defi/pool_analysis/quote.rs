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

use std::cmp::Ordering;

use alloy_primitives::{I256, U160, U256};

use crate::defi::tick_map::tick::CrossedTick;

/// Comprehensive swap quote containing profiling metrics for a hypothetical swap.
///
/// This structure provides detailed analysis of what would happen if a swap were executed,
/// including price impact, fees, slippage, and execution details, without actually
/// modifying the pool state.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct SwapQuote {
    /// Amount of token0 that would be swapped (positive = in, negative = out).
    pub amount0: I256,
    /// Amount of token1 that would be swapped (positive = in, negative = out).
    pub amount1: I256,
    /// Square root price before the swap (Q96 format).
    pub sqrt_price_before_x96: U160,
    /// Square root price after the swap (Q96 format).
    pub sqrt_price_after_x96: U160,
    /// Tick position before the swap.
    pub tick_before: i32,
    /// Tick position after the swap.
    pub tick_after: i32,
    /// Fee growth global for target token after the swap (Q128.128 format).
    pub fee_growth_global_after: U256,
    /// Total fees paid to liquidity providers.
    pub lp_fee: U256,
    /// Total fees paid to the protocol.
    pub protocol_fee: U256,
    /// List of tick boundaries crossed during the swap, in order of crossing.
    pub crossed_ticks: Vec<CrossedTick>,
}

impl SwapQuote {
    #[allow(clippy::too_many_arguments)]
    /// Creates a [`SwapQuote`] instance with comprehensive swap simulation results.
    pub fn new(
        amount0: I256,
        amount1: I256,
        sqrt_price_before_x96: U160,
        sqrt_price_after_x96: U160,
        tick_before: i32,
        tick_after: i32,
        fee_growth_global_after: U256,
        lp_fee: U256,
        protocol_fee: U256,
        crossed_ticks: Vec<CrossedTick>,
    ) -> Self {
        Self {
            amount0,
            amount1,
            sqrt_price_before_x96,
            sqrt_price_after_x96,
            tick_before,
            tick_after,
            fee_growth_global_after,
            lp_fee,
            protocol_fee,
            crossed_ticks,
        }
    }

    /// Determines swap direction from tick movement or amount sign.
    ///
    /// Returns `true` if swapping token0 for token1 (zero_for_one),
    /// `false` if swapping token1 for token0.
    ///
    /// The direction is inferred from:
    /// 1. Tick movement (if ticks changed): downward = token0→token1
    /// 2. Amount sign (if tick unchanged): positive amount0 = token0→token1
    pub fn zero_for_one(&self) -> bool {
        match self.tick_after.cmp(&self.tick_before) {
            Ordering::Less => true,     // Tick went down, swap was token0 -> token1
            Ordering::Greater => false, // Tick went up, swap was token1 -> token0
            Ordering::Equal => {
                // Tick unchanged, very small swap, we fall back to the amount sign
                self.amount0.is_positive()
            }
        }
    }

    /// Returns the total fees paid (LP fees + protocol fees).
    pub fn total_fee(&self) -> U256 {
        self.lp_fee + self.protocol_fee
    }

    /// Returns the number of tick boundaries crossed during this swap.
    ///
    /// This equals the length of the `crossed_ticks` vector and indicates
    /// how much liquidity the swap traversed.
    pub fn total_crossed_ticks(&self) -> u32 {
        self.crossed_ticks.len() as u32
    }
}
