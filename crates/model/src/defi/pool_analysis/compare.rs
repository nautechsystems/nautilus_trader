// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

/// Result of comparing a replayed pool profiler against an on-chain snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PoolProfilerComparison {
    /// All compared fields match.
    Match,
    /// Only the sqrt price differs; structural pool state still matches.
    SqrtPriceMismatch,
    /// One or more structural fields differ.
    Mismatch,
}

impl PoolProfilerComparison {
    /// Returns `true` when every compared field matches.
    #[must_use]
    pub const fn is_exact_match(self) -> bool {
        matches!(self, Self::Match)
    }

    /// Returns `true` when the snapshot can seed the profiler cache.
    #[must_use]
    pub const fn is_valid_for_snapshot(self) -> bool {
        matches!(self, Self::Match | Self::SqrtPriceMismatch)
    }
}

/// Compares a pool profiler's internal state with on-chain state to verify consistency.
///
/// This function validates that the profiler's tracked state matches the actual on-chain
/// pool state by comparing global pool parameters, tick data, and position data.
/// Structural mismatches are logged as errors, sqrt price mismatches are logged as warnings,
/// and matches are logged as info.
///
/// # Arguments
///
/// * `profiler` - The pool profiler whose state should be compared
/// * `snapshot` - The on-chain snapshot to compare against
///
/// # Panics
///
/// Panics if the profiler has not been initialized.
///
/// # Returns
///
/// Returns `true` if all compared values match, `false` if any mismatches are detected.
#[must_use]
pub fn compare_pool_profiler(profiler: &PoolProfiler, snapshot: &PoolSnapshot) -> bool {
    compare_pool_profiler_detailed(profiler, snapshot).is_exact_match()
}

/// Compares a pool profiler's internal state with on-chain state and classifies mismatches.
///
/// Sqrt price can differ when replay is event-scoped but the RPC snapshot is block-scoped.
/// That mismatch is non-blocking when tick, liquidity, ticks, and positions all match.
///
/// # Panics
///
/// Panics if the profiler has not been initialized.
#[must_use]
pub fn compare_pool_profiler_detailed(
    profiler: &PoolProfiler,
    snapshot: &PoolSnapshot,
) -> PoolProfilerComparison {
    assert!(profiler.is_initialized, "Profiler is not initialized");

    let mut structural_match = true;
    let mut sqrt_price_matches = true;
    let total_ticks = snapshot.ticks.len();
    let total_positions = snapshot.positions.len();

    if snapshot.state.current_tick == profiler.state.current_tick {
        log::info!("✓ current_tick matches: {}", snapshot.state.current_tick);
    } else {
        log::error!(
            "Tick mismatch: profiler={}, compared={}",
            profiler.state.current_tick,
            snapshot.state.current_tick
        );
        structural_match = false;
    }

    if snapshot.state.price_sqrt_ratio_x96 == profiler.state.price_sqrt_ratio_x96 {
        log::info!(
            "✓ sqrt_price_x96 matches: {}",
            profiler.state.price_sqrt_ratio_x96,
        );
    } else {
        log::warn!(
            "Sqrt ratio mismatch: profiler={}, compared={}",
            profiler.state.price_sqrt_ratio_x96,
            snapshot.state.price_sqrt_ratio_x96
        );
        sqrt_price_matches = false;
    }

    if snapshot.state.fee_protocol == profiler.state.fee_protocol {
        log::info!("✓ fee_protocol matches: {}", snapshot.state.fee_protocol);
    } else {
        log::error!(
            "Fee protocol mismatch: profiler={}, compared={}",
            profiler.state.fee_protocol,
            snapshot.state.fee_protocol
        );
        structural_match = false;
    }

    if snapshot.state.liquidity == profiler.tick_map.liquidity {
        log::info!("✓ liquidity matches: {}", snapshot.state.liquidity);
    } else {
        log::error!(
            "Liquidity mismatch: profiler={}, compared={}",
            profiler.tick_map.liquidity,
            snapshot.state.liquidity
        );
        structural_match = false;
    }

    // TODO add growth fee checking

    // Check ticks
    let mut tick_mismatches = 0;

    for tick in &snapshot.ticks {
        if let Some(profiler_tick) = profiler.get_tick(tick.value) {
            let mut all_tick_fields_matching = true;

            if profiler_tick.liquidity_net != tick.liquidity_net {
                log::error!(
                    "Tick {} mismatch on net liquidity: profiler={}, compared={}",
                    tick.value,
                    profiler_tick.liquidity_net,
                    tick.liquidity_net
                );
                all_tick_fields_matching = false;
            }

            if profiler_tick.liquidity_gross != tick.liquidity_gross {
                log::error!(
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
                structural_match = false;
            }
        } else {
            log::error!(
                "Tick {} not found in the profiler but provided in the compare mapping",
                tick.value
            );
            structural_match = false;
        }
    }

    if tick_mismatches == 0 {
        log::info!("✓ Provided {total_ticks} ticks with liquidity net and gross are matching");
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
                log::error!(
                    "Position '{}' mismatch on liquidity: profiler={}, compared={}",
                    position_key,
                    profiler_position.liquidity,
                    position.liquidity
                );
                position_mismatches += 1;
            }
            // TODO add fees and tokens owned checking
        } else {
            log::error!(
                "Position {} not found in the profiler but provided in the compare mapping",
                position.owner
            );
            structural_match = false;
        }
    }

    if position_mismatches == 0 {
        log::info!("✓ Provided {total_positions} active positions with liquidity are matching");
    } else {
        structural_match = false;
    }

    if structural_match && sqrt_price_matches {
        PoolProfilerComparison::Match
    } else if structural_match {
        log::warn!("Pool profiler sqrt ratio differs, but all structural state matches");
        PoolProfilerComparison::SqrtPriceMismatch
    } else {
        PoolProfilerComparison::Mismatch
    }
}
