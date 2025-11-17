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

//! Size estimation utilities for DeFi pool profiler.
//!
//! This module provides functions for estimating optimal trade sizes based on
//! target price impact/slippage levels using binary search and liquidity analysis.

use alloy_primitives::U256;

use super::PoolProfiler;

/// Configuration for size estimation algorithms.
///
/// Controls the behavior of the binary search algorithm including convergence
/// criteria and adaptive bound expansion.
#[derive(Debug, Clone)]
pub struct EstimationConfig {
    /// Enable adaptive upper bound expansion during binary search (default: true).
    pub enable_adaptive_bounds: bool,
    /// Maximum number of times to expand upper bound (default: 10).
    pub max_bound_expansions: u32,
    /// Binary search tolerance in basis points (default: 1).
    pub tolerance_bps: u32,
    /// Maximum iterations for binary search (default: 50).
    pub max_iterations: u32,
}

impl Default for EstimationConfig {
    fn default() -> Self {
        Self {
            enable_adaptive_bounds: true,
            max_bound_expansions: 10,
            tolerance_bps: 1,
            max_iterations: 50,
        }
    }
}

/// Detailed result of a size-for-impact search.
///
/// Contains comprehensive diagnostics about the binary search process including
/// convergence information, iterations taken, bounds used, and final accuracy.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct SizeForImpactResult {
    /// Target slippage requested in basis points.
    pub target_impact_bps: u32,
    /// Optimal trade size found.
    pub size: U256,
    /// Actual slippage at the found size in basis points.
    pub actual_impact_bps: u32,
    /// Swap direction (true = token0 for token1).
    pub zero_for_one: bool,
    /// Number of binary search iterations performed.
    pub iterations: u32,
    /// Whether the search converged successfully.
    pub converged: bool,
    /// Number of times the upper bound was expanded.
    pub expansion_count: u32,
    /// Initial upper bound estimate used.
    pub initial_high: U256,
    /// Final lower bound when search terminated.
    pub final_low: U256,
    /// Final upper bound when search terminated.
    pub final_high: U256,
}

impl SizeForImpactResult {
    /// Check if the result is within the specified tolerance.
    pub fn within_tolerance(&self, tolerance_bps: u32) -> bool {
        let diff = self.actual_impact_bps.abs_diff(self.target_impact_bps);
        diff <= tolerance_bps
    }

    /// Get the convergence quality as a percentage.
    ///
    /// # Returns
    /// Accuracy percentage (100.0 = perfect match, lower = less accurate)
    pub fn accuracy_percent(&self) -> f64 {
        if self.target_impact_bps == 0 {
            return 100.0;
        }
        let diff = self.actual_impact_bps.abs_diff(self.target_impact_bps) as f64;
        let target = self.target_impact_bps as f64;
        100.0 - (diff / target * 100.0).min(100.0)
    }
}

/// Internal state from binary search algorithm.
///
/// Used by the private helper function to return all tracking information
/// without timing overhead.
#[derive(Debug, Clone)]
struct BinarySearchState {
    /// Final lower bound when search terminated.
    low: U256,
    /// Final upper bound when search terminated.
    high: U256,
    /// Initial upper bound estimate used.
    initial_high: U256,
    /// Number of binary search iterations performed.
    iterations: u32,
    /// Number of times the upper bound was expanded.
    expansions: u32,
    /// Whether the search converged successfully.
    converged: bool,
    /// Final slippage in bps (if calculated during search).
    final_slippage_bps: Option<u32>,
}

/// Estimates the maximum trade size for a given impact target.
///
/// Uses a simple heuristic: size ≈ liquidity × price_factor × impact_ratio × safety_multiplier
/// The binary search will refine this estimate, so perfect accuracy isn't needed.
///
/// # Arguments
/// * `profiler` - Reference to the pool profiler
/// * `impact_bps` - Target impact in basis points
/// * `zero_for_one` - Swap direction
/// * `config` - Estimation configuration (only uses safety_multiplier)
///
/// # Returns
/// Estimated maximum size as U256
pub fn estimate_max_size_for_impact(
    profiler: &PoolProfiler,
    impact_bps: u32,
    zero_for_one: bool,
) -> U256 {
    let liquidity = profiler.get_active_liquidity();
    if liquidity == 0 {
        return U256::from(1_000_000);
    }

    let sqrt_price = U256::from(profiler.state.price_sqrt_ratio_x96);
    let q96 = U256::from(1u128) << 96;
    let liquidity_u256 = U256::from(liquidity);
    let impact_ratio = U256::from(impact_bps);

    let base = if zero_for_one {
        (liquidity_u256 * q96 * impact_ratio) / (sqrt_price * U256::from(10000))
    } else {
        (liquidity_u256 * sqrt_price * impact_ratio) / (q96 * U256::from(10000))
    };

    // 2x safety factor, clamp to reasonable range
    let doubled = base * U256::from(2);
    let min_val = U256::from(1_000_000);
    let max_val = U256::from(1_000_000_000_000_000_000_000_000_000_000u128);

    if doubled < min_val {
        min_val
    } else if doubled > max_val {
        max_val
    } else {
        doubled
    }
}

/// Calculates the slippage in basis points for a given trade size.
///
/// This function simulates a swap with the specified size and returns the slippage
/// (total execution cost including fees) in basis points. Slippage is calculated
/// as the difference between the execution price and the spot price before the swap.
///
/// # Returns
/// Slippage in basis points (10000 = 100%)
///
/// # Errors
/// Returns error if:
/// - Pool is not initialized
/// - Swap simulation fails
/// - Trade info calculation fails
pub fn slippage_for_size_bps(
    profiler: &PoolProfiler,
    size: U256,
    zero_for_one: bool,
) -> anyhow::Result<u32> {
    profiler.check_if_initialized();

    if size.is_zero() {
        return Ok(0);
    }

    let mut quote = profiler.swap_exact_in(size, zero_for_one, None)?;
    quote.calculate_trade_info(&profiler.pool.token0, &profiler.pool.token1)?;
    let trade_info = quote
        .trade_info
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("Trade info not initialized"))?;

    trade_info.get_slippage_bps()
}

/// Core binary search algorithm for finding optimal trade size.
///
/// # Returns
/// State containing final bounds, iterations, convergence status, and optionally
/// the final slippage if it was calculated during convergence.
fn binary_search_for_size(
    profiler: &PoolProfiler,
    impact_bps: u32,
    zero_for_one: bool,
    config: &EstimationConfig,
) -> anyhow::Result<BinarySearchState> {
    // Validate inputs
    if impact_bps == 0 {
        anyhow::bail!("Impact must be greater than zero");
    }
    if impact_bps > 10000 {
        anyhow::bail!("Impact cannot exceed 100% (10000 bps)");
    }
    profiler.check_if_initialized();

    // Estimate initial bounds
    let mut low = U256::ZERO;
    let mut high = estimate_max_size_for_impact(profiler, impact_bps, zero_for_one);
    let initial_high = high;

    let mut iterations = 0;
    let mut expansions = 0;
    let mut converged = false;
    let mut final_slippage_bps = None;

    // Binary search with optional adaptive expansion
    while iterations < config.max_iterations {
        iterations += 1;

        // Calculate midpoint
        let mid = (low + high) / U256::from(2);

        if mid.is_zero() {
            break;
        }

        // Calculate slippage at midpoint
        let slippage_mid = match slippage_for_size_bps(profiler, mid, zero_for_one) {
            Ok(s) => s,
            Err(_) => {
                // Swap failed, mid too large
                high = mid;
                continue;
            }
        };

        // Check convergence by slippage
        let diff_bps = slippage_mid.abs_diff(impact_bps);
        if diff_bps <= config.tolerance_bps {
            low = mid;
            final_slippage_bps = Some(slippage_mid);
            converged = true;
            break;
        }

        // Adjust bounds
        if slippage_mid < impact_bps {
            low = mid;

            // Adaptive expansion: only expand when midpoint is in the top 20% of the range
            // This indicates we're approaching the upper bound
            let range = high - low;
            let threshold = range / U256::from(5); // 20% of range
            if config.enable_adaptive_bounds
                && high - mid <= threshold
                && expansions < config.max_bound_expansions
            {
                high *= U256::from(2);
                expansions += 1;
                tracing::debug!(
                    "Expanding upper bound (expansion {}/{}): new high={}",
                    expansions,
                    config.max_bound_expansions,
                    high
                );
            }
        } else {
            high = mid;
        }
    }

    if iterations >= config.max_iterations {
        tracing::warn!(
            "Binary search did not converge after {} iterations, returning conservative estimate",
            iterations
        );
    }

    Ok(BinarySearchState {
        low,
        high,
        initial_high,
        iterations,
        expansions,
        converged,
        final_slippage_bps,
    })
}

/// Finds the maximum trade size that produces a target slippage (including fees).
///
/// Uses binary search with optional adaptive upper bound expansion to find the
/// largest trade size that results in slippage at or below the target. The method
/// iteratively simulates swaps at different sizes until it converges to the optimal
/// size within the specified tolerance.
///
/// # Algorithm
/// 1. Estimate initial upper bound using hybrid strategy (heuristic + liquidity scan)
/// 2. Binary search between 0 and upper bound
/// 3. For each midpoint, calculate actual slippage via simulation
/// 4. Adjust bounds based on whether slippage is above or below target
/// 5. If adaptive bounds enabled and upper bound reached, expand and continue
/// 6. Converge when slippage is within tolerance or size delta is minimal
///
/// # Returns
/// The maximum trade size (U256) that produces the target slippage
///
/// # Errors
/// Returns error if:
/// - Impact is zero or exceeds 100% (10000 bps)
/// - Pool is not initialized
/// - Swap simulations fail
pub fn size_for_impact_bps(
    profiler: &PoolProfiler,
    impact_bps: u32,
    zero_for_one: bool,
    config: &EstimationConfig,
) -> anyhow::Result<U256> {
    let state = binary_search_for_size(profiler, impact_bps, zero_for_one, config)?;
    Ok(state.low)
}

/// Finds the maximum trade size with detailed search diagnostics.
///
/// This is the detailed version of [`size_for_impact_bps`] that returns comprehensive
/// information about the search process including convergence metrics, iterations,
/// bounds used, and timing information.
///
/// # Arguments
/// * `profiler` - Reference to the pool profiler
/// * `impact_bps` - Target slippage in basis points (including fees)
/// * `zero_for_one` - Swap direction (true = token0 for token1)
/// * `config` - Estimation configuration
///
/// # Returns
/// Detailed result containing size, search metrics, and convergence information
///
/// # Errors
/// Returns error if:
/// - Impact is zero or exceeds 100% (10000 bps)
/// - Pool is not initialized
/// - Swap simulations fail
pub fn size_for_impact_bps_detailed(
    profiler: &PoolProfiler,
    impact_bps: u32,
    zero_for_one: bool,
    config: &EstimationConfig,
) -> anyhow::Result<SizeForImpactResult> {
    let state = binary_search_for_size(profiler, impact_bps, zero_for_one, config)?;

    // Get actual slippage - reuse from state if available to avoid redundant calculation
    let actual_impact = if let Some(slippage) = state.final_slippage_bps {
        slippage
    } else if state.low.is_zero() {
        0
    } else {
        slippage_for_size_bps(profiler, state.low, zero_for_one)?
    };

    Ok(SizeForImpactResult {
        target_impact_bps: impact_bps,
        size: state.low,
        actual_impact_bps: actual_impact,
        zero_for_one,
        iterations: state.iterations,
        converged: state.converged,
        expansion_count: state.expansions,
        initial_high: state.initial_high,
        final_low: state.low,
        final_high: state.high,
    })
}
