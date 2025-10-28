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

use std::cmp::Ord;

use alloy_primitives::U256;

use crate::defi::tick_map::liquidity_math::liquidity_math_add;

/// Represents a tick in a Uniswap V3-style AMM with liquidity tracking and fee accounting.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, serde::Serialize, serde::Deserialize,
)]
pub struct PoolTick {
    /// The referenced tick,
    pub value: i32,
    /// Total liquidity referencing this tick.
    pub liquidity_gross: u128,
    /// Net liquidity change when crossing this tick.
    pub liquidity_net: i128,
    /// Accumulated fees for token0 that have been collected outside this tick.
    pub fee_growth_outside_0: U256,
    /// Accumulated fees for token1 that have been collected outside this tick.
    pub fee_growth_outside_1: U256,
    /// Indicating whether this tick has been used.
    pub initialized: bool,
    /// Last block when this tick was used.
    pub last_updated_block: u64,
    /// Count of times this tick was updated.
    pub updates_count: usize,
}

impl PoolTick {
    /// Minimum valid tick value for Uniswap V3 pools.
    pub const MIN_TICK: i32 = -887272;
    /// Maximum valid tick value for Uniswap V3 pools.
    pub const MAX_TICK: i32 = -Self::MIN_TICK;

    /// Creates a new [`PoolTick`] with all specified parameters.
    #[must_use]
    pub fn new(
        value: i32,
        liquidity_gross: u128,
        liquidity_net: i128,
        fee_growth_outside_0: U256,
        fee_growth_outside_1: U256,
        initialized: bool,
        last_updated_block: u64,
    ) -> Self {
        Self {
            value,
            liquidity_gross,
            liquidity_net,
            fee_growth_outside_0,
            fee_growth_outside_1,
            initialized,
            last_updated_block,
            updates_count: 0,
        }
    }

    /// Creates a tick with default values for a given tick value.
    pub fn from_tick(tick: i32) -> Self {
        Self::new(tick, 0, 0, U256::ZERO, U256::ZERO, false, 0)
    }

    /// Updates liquidity amounts when positions are added/removed.
    pub fn update_liquidity(&mut self, liquidity_delta: i128, upper: bool) -> u128 {
        let liquidity_gross_before = self.liquidity_gross;
        self.liquidity_gross = liquidity_math_add(self.liquidity_gross, liquidity_delta);

        // liquidity_net tracks the net change when crossing this tick
        if upper {
            self.liquidity_net -= liquidity_delta;
        } else {
            self.liquidity_net += liquidity_delta;
        }
        self.updates_count += 1;

        liquidity_gross_before
    }

    /// Resets tick to the default state.
    pub fn clear(&mut self) {
        self.liquidity_gross = 0;
        self.liquidity_net = 0;
        self.fee_growth_outside_0 = U256::ZERO;
        self.fee_growth_outside_1 = U256::ZERO;
        self.initialized = false;
    }

    /// Checks if the tick is initialized and has liquidity.
    #[must_use]
    pub fn is_active(&self) -> bool {
        self.initialized && self.liquidity_gross > 0
    }

    /// Updates fee growth outside this tick.
    pub fn update_fee_growth(&mut self, fee_growth_global_0: U256, fee_growth_global_1: U256) {
        self.fee_growth_outside_0 = fee_growth_global_0 - self.fee_growth_outside_0;
        self.fee_growth_outside_1 = fee_growth_global_1 - self.fee_growth_outside_1;
    }

    /// Gets maximum valid tick for given spacing.
    pub fn get_max_tick(tick_spacing: i32) -> i32 {
        // Find the largest tick that is divisible by tick_spacing and <= MAX_TICK
        (Self::MAX_TICK / tick_spacing) * tick_spacing
    }

    /// Gets minimum valid tick for given spacing.
    pub fn get_min_tick(tick_spacing: i32) -> i32 {
        // Find the smallest tick that is divisible by tick_spacing and >= MIN_TICK
        (Self::MIN_TICK / tick_spacing) * tick_spacing
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_update_liquidity_add_remove() {
        let mut tick = PoolTick::from_tick(100);
        tick.initialized = true;

        // Add liquidity
        tick.update_liquidity(1000, false); // lower tick
        assert_eq!(tick.liquidity_gross, 1000);
        assert_eq!(tick.liquidity_net, 1000); // lower tick: net = +delta
        assert!(tick.is_active());

        // Add more liquidity
        tick.update_liquidity(500, false);
        assert_eq!(tick.liquidity_gross, 1500);
        assert_eq!(tick.liquidity_net, 1500);
        assert!(tick.is_active());

        // Remove some liquidity
        tick.update_liquidity(-300, false);
        assert_eq!(tick.liquidity_gross, 1200);
        assert_eq!(tick.liquidity_net, 1200);
        assert!(tick.is_active());

        // Remove all remaining liquidity
        tick.update_liquidity(-1200, false);
        assert_eq!(tick.liquidity_gross, 0);
        assert_eq!(tick.liquidity_net, 0);
        assert!(!tick.is_active()); // Should not be active when liquidity_gross == 0
    }

    #[rstest]
    fn test_update_liquidity_upper_tick() {
        let mut tick = PoolTick::from_tick(200);
        tick.initialized = true;

        // Add liquidity (upper tick)
        tick.update_liquidity(1000, true);
        assert_eq!(tick.liquidity_gross, 1000);
        assert_eq!(tick.liquidity_net, -1000); // upper tick: net = -delta
        assert!(tick.is_active());

        // Remove liquidity (upper tick)
        tick.update_liquidity(-500, true);
        assert_eq!(tick.liquidity_gross, 500);
        assert_eq!(tick.liquidity_net, -500); // upper tick: net = -delta
        assert!(tick.is_active());
    }

    #[rstest]
    fn test_get_max_tick() {
        // Test with common Uniswap V3 tick spacings

        // Tick spacing 1 (0.01% fee tier)
        let max_tick_1 = PoolTick::get_max_tick(1);
        assert_eq!(max_tick_1, 887272); // Should be exactly MAX_TICK since it's divisible by 1

        // Tick spacing 10 (0.05% fee tier)
        let max_tick_10 = PoolTick::get_max_tick(10);
        assert_eq!(max_tick_10, 887270); // 887272 / 10 * 10 = 887270
        assert_eq!(max_tick_10 % 10, 0);
        assert!(max_tick_10 <= PoolTick::MAX_TICK);

        // Tick spacing 60 (0.3% fee tier)
        let max_tick_60 = PoolTick::get_max_tick(60);
        assert_eq!(max_tick_60, 887220); // 887272 / 60 * 60 = 887220
        assert_eq!(max_tick_60 % 60, 0);
        assert!(max_tick_60 <= PoolTick::MAX_TICK);

        // Tick spacing 200 (1% fee tier)
        let max_tick_200 = PoolTick::get_max_tick(200);
        assert_eq!(max_tick_200, 887200); // 887272 / 200 * 200 = 887200
        assert_eq!(max_tick_200 % 200, 0);
        assert!(max_tick_200 <= PoolTick::MAX_TICK);
    }

    #[rstest]
    fn test_get_min_tick() {
        // Test with common Uniswap V3 tick spacings

        // Tick spacing 1 (0.01% fee tier)
        let min_tick_1 = PoolTick::get_min_tick(1);
        assert_eq!(min_tick_1, -887272); // Should be exactly MIN_TICK since it's divisible by 1

        // Tick spacing 10 (0.05% fee tier)
        let min_tick_10 = PoolTick::get_min_tick(10);
        assert_eq!(min_tick_10, -887270); // -887272 / 10 * 10 = -887270
        assert_eq!(min_tick_10 % 10, 0);
        assert!(min_tick_10 >= PoolTick::MIN_TICK);

        // Tick spacing 60 (0.3% fee tier)
        let min_tick_60 = PoolTick::get_min_tick(60);
        assert_eq!(min_tick_60, -887220); // -887272 / 60 * 60 = -887220
        assert_eq!(min_tick_60 % 60, 0);
        assert!(min_tick_60 >= PoolTick::MIN_TICK);

        // Tick spacing 200 (1% fee tier)
        let min_tick_200 = PoolTick::get_min_tick(200);
        assert_eq!(min_tick_200, -887200); // -887272 / 200 * 200 = -887200
        assert_eq!(min_tick_200 % 200, 0);
        assert!(min_tick_200 >= PoolTick::MIN_TICK);
    }

    #[rstest]
    fn test_tick_spacing_symmetry() {
        // Test that max and min ticks are symmetric for all common spacings
        let spacings = [1, 10, 60, 200];

        for spacing in spacings {
            let max_tick = PoolTick::get_max_tick(spacing);
            let min_tick = PoolTick::get_min_tick(spacing);

            // Should be symmetric (max = -min)
            assert_eq!(max_tick, -min_tick, "Asymmetry for spacing {}", spacing);

            // Both should be divisible by spacing
            assert_eq!(max_tick % spacing, 0);
            assert_eq!(min_tick % spacing, 0);

            // Should be within bounds
            assert!(max_tick <= PoolTick::MAX_TICK);
            assert!(min_tick >= PoolTick::MIN_TICK);
        }
    }
}
