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

use super::{position::PoolPosition, profiler::PoolProfiler};
use crate::defi::pool_analysis::snapshot::PoolSnapshot;

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
pub fn compare_pool_profiler(profiler: &PoolProfiler, snapshot: &PoolSnapshot) -> bool {
    if !profiler.is_initialized {
        panic!("Profiler is not initialized");
    }

    let mut all_match = true;
    let total_ticks = snapshot.ticks.len();
    let total_positions = snapshot.positions.len();

    if snapshot.state.current_tick != profiler.state.current_tick {
        tracing::error!(
            "Tick mismatch: profiler={}, compared={}",
            profiler.state.current_tick,
            snapshot.state.current_tick
        );
        all_match = false;
    } else {
        tracing::info!("✓ current_tick matches: {}", snapshot.state.current_tick);
    }

    if snapshot.state.price_sqrt_ratio_x96 != profiler.state.price_sqrt_ratio_x96 {
        tracing::error!(
            "Sqrt ratio mismatch: profiler={}, compared={}",
            profiler.state.price_sqrt_ratio_x96,
            snapshot.state.price_sqrt_ratio_x96
        );
        all_match = false;
    } else {
        tracing::info!(
            "✓ sqrt_price_x96 matches: {}",
            profiler.state.price_sqrt_ratio_x96,
        );
    }

    if snapshot.state.fee_protocol != profiler.state.fee_protocol {
        tracing::error!(
            "Fee protocol mismatch: profiler={}, compared={}",
            profiler.state.fee_protocol,
            snapshot.state.fee_protocol
        );
        all_match = false;
    } else {
        tracing::info!("✓ fee_protocol matches: {}", snapshot.state.fee_protocol);
    }

    if snapshot.state.liquidity != profiler.tick_map.liquidity {
        tracing::error!(
            "Liquidity mismatch: profiler={}, compared={}",
            profiler.tick_map.liquidity,
            snapshot.state.liquidity
        );
        all_match = false;
    } else {
        tracing::info!("✓ liquidity matches: {}", snapshot.state.liquidity);
    }

    // TODO add growth fee checking

    // Check ticks
    let mut tick_mismatches = 0;
    for tick in &snapshot.ticks {
        if let Some(profiler_tick) = profiler.get_tick(tick.value) {
            let mut all_tick_fields_matching = true;
            if profiler_tick.liquidity_net != tick.liquidity_net {
                tracing::error!(
                    "Tick {} mismatch on net liquidity: profiler={}, compared={}",
                    tick.value,
                    profiler_tick.liquidity_net,
                    tick.liquidity_net
                );
                all_tick_fields_matching = false;
            }
            if profiler_tick.liquidity_gross != tick.liquidity_gross {
                tracing::error!(
                    "Tick {} mismatch on gross liquidity: profiler={}, compared={}",
                    tick.value,
                    profiler_tick.liquidity_gross,
                    tick.liquidity_gross
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
                tick.value
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
    for position in &snapshot.positions {
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
