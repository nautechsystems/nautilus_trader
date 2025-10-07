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

//! Pool profiler state comparison utilities.

use std::collections::HashMap;

use alloy_primitives::U160;

use super::{position::PoolPosition, profiler::PoolProfiler};
use crate::defi::tick_map::tick::Tick;

/// Compares a pool profiler's internal state with on-chain state to verify consistency.
///
/// This function validates that the profiler's tracked state matches the actual on-chain
/// pool state by comparing global pool parameters, tick data, and position data.
/// Any mismatches are logged as errors, while matches are logged as info.
///
/// # Arguments
///
/// * `profiler` - The pool profiler whose state should be compared
/// * `current_tick` - The current active tick from on-chain state
/// * `price_sqrt_ratio_x96` - The current sqrt price ratio (Q64.96 format) from on-chain state
/// * `fee_protocol` - The protocol fee setting from on-chain state
/// * `liquidity` - The current liquidity from on-chain state
/// * `ticks` - Map of tick indices to their on-chain tick data
/// * `positions` - Vector of on-chain position data
///
/// # Panics
///
/// Panics if the profiler has not been initialized
///
/// # Returns
///
/// Returns `true` if all compared values match, `false` if any mismatches are detected.
pub fn compare_pool_profiler(
    profiler: &PoolProfiler,
    current_tick: i32,
    price_sqrt_ratio_x96: U160,
    fee_protocol: u8,
    liquidity: u128,
    ticks: HashMap<i32, Tick>,
    positions: Vec<PoolPosition>,
) -> bool {
    if !profiler.is_initialized {
        panic!("Profiler is not initialized");
    }

    let mut all_match = true;
    let total_ticks = ticks.len();
    let total_positions = positions.len();

    if current_tick != profiler.state.current_tick {
        tracing::error!(
            "Tick mismatch: profiler={}, compared={}",
            profiler.state.current_tick,
            current_tick
        );
        all_match = false;
    } else {
        tracing::info!("✓ current_tick matches: {}", current_tick);
    }

    if price_sqrt_ratio_x96 != profiler.state.price_sqrt_ratio_x96 {
        tracing::error!(
            "Sqrt ratio mismatch: profiler={}, compared={}",
            profiler.state.price_sqrt_ratio_x96,
            price_sqrt_ratio_x96
        );
        all_match = false;
    } else {
        tracing::info!(
            "✓ sqrt_price_x96 matches: {}",
            profiler.state.price_sqrt_ratio_x96,
        );
    }

    if fee_protocol != profiler.state.fee_protocol {
        tracing::error!(
            "Fee protocol mismatch: profiler={}, compared={}",
            profiler.state.fee_protocol,
            fee_protocol
        );
        all_match = false;
    } else {
        tracing::info!("✓ fee_protocol matches: {}", fee_protocol);
    }

    if liquidity != profiler.tick_map.liquidity {
        tracing::error!(
            "Liquidity mismatch: profiler={}, compared={}",
            profiler.tick_map.liquidity,
            liquidity
        );
        all_match = false;
    } else {
        tracing::info!("✓ liquidity matches: {}", liquidity);
    }

    // TODO add growth fee checking

    // Check ticks
    let mut tick_mismatches = 0;
    for (tick, tick_data) in ticks {
        if let Some(profiler_tick) = profiler.get_tick(tick) {
            let mut all_tick_fields_matching = true;
            if profiler_tick.liquidity_net != tick_data.liquidity_net {
                tracing::error!(
                    "Tick {} mismatch on net liquidity: profiler={}, compared={}",
                    tick,
                    profiler_tick.liquidity_net,
                    tick_data.liquidity_net
                );
                all_tick_fields_matching = false;
            }
            if profiler_tick.liquidity_gross != tick_data.liquidity_gross {
                tracing::error!(
                    "Tick {} mismatch on gross liquidity: profiler={}, compared={}",
                    tick,
                    profiler_tick.liquidity_gross,
                    tick_data.liquidity_gross
                );
                all_tick_fields_matching = false;
            }
            // TODO add fees checking per tick

            if !all_tick_fields_matching {
                tick_mismatches += 1;
                all_match = false;
            }
        } else {
            tracing::error!(
                "Tick {} not found in the profiler but provided in the compare mapping",
                tick
            );
            all_match = false;
        }
    }

    if tick_mismatches == 0 {
        tracing::info!(
            "✓ Provided {} ticks with liquidity net and gross are matching",
            total_ticks
        );
    }

    // Check positions
    let mut position_mismatches = 0;
    for position in positions {
        if let Some(profiler_position) =
            profiler.get_position(&position.owner, position.tick_lower, position.tick_upper)
        {
            let position_key = PoolPosition::get_position_key(
                &position.owner,
                position.tick_lower,
                position.tick_upper,
            );
            if position.liquidity != profiler_position.liquidity {
                tracing::error!(
                    "Position '{}' mismatch on liquidity: profiler={}, compared={}",
                    position_key,
                    profiler_position.liquidity,
                    position.liquidity
                );
                position_mismatches += 1;
            }
            // TODO add fees and tokens owned checking
        } else {
            tracing::error!(
                "Position {} not found in the profiler but provided in the compare mapping",
                position.owner
            );
            all_match = false;
        }
    }

    if position_mismatches == 0 {
        tracing::info!(
            "✓ Provided {} active positions with liquidity are matching",
            total_positions
        );
    } else {
        all_match = false;
    }

    all_match
}
