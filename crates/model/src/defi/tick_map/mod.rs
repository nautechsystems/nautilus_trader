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

use std::collections::HashMap;

use alloy_primitives::U256;

use crate::defi::tick_map::{
    liquidity_math::tick_spacing_to_max_liquidity_per_tick, tick::PoolTick, tick_bitmap::TickBitmap,
};

pub mod bit_math;
pub mod full_math;
pub mod liquidity_math;
pub mod sqrt_price_math;
pub mod tick;
pub mod tick_bitmap;
pub mod tick_math;

/// A tick map implementation for managing liquidity distribution in an AMM (Automated Market Maker).
///
/// This structure maintains a mapping of price ticks to liquidity data, allowing efficient
/// navigation and manipulation of concentrated liquidity positions. It tracks active liquidity,
/// global fee growth, and uses a bitmap for efficient tick traversal during swaps.
#[derive(Debug, Clone)]
pub struct TickMap {
    /// Mapping of tick indices to tick data
    ticks: HashMap<i32, PoolTick>,
    /// Tick bitmap for efficient tick navigation
    tick_bitmap: TickBitmap,
    /// Current active liquidity
    pub liquidity: u128,
    /// Maximum liquidity that can be concentrated in a single tick based on tick spacing.
    pub max_liquidity_per_tick: u128,
}

impl Default for TickMap {
    fn default() -> Self {
        Self::new(0)
    }
}

impl TickMap {
    /// Creates a new [`TickMap`] with the specified tick spacing.
    pub fn new(tick_spacing: u32) -> Self {
        Self {
            ticks: HashMap::new(),
            tick_bitmap: TickBitmap::new(tick_spacing),
            liquidity: 0,
            max_liquidity_per_tick: tick_spacing_to_max_liquidity_per_tick(tick_spacing as i32),
        }
    }

    /// Retrieves a reference to the tick data at the specified tick index.
    pub fn get_tick(&self, tick: i32) -> Option<&PoolTick> {
        self.ticks.get(&tick)
    }

    /// Gets a mutable reference to the tick data, initializing it if it doesn't exist.
    pub fn get_tick_or_init(&mut self, tick: i32) -> &mut PoolTick {
        self.ticks
            .entry(tick)
            .or_insert_with(|| PoolTick::from_tick(tick))
    }

    /// Calculates the fee growth inside a price range defined by lower and upper ticks.
    pub fn get_fee_growth_inside(
        &mut self,
        lower_tick: i32,
        upper_tick: i32,
        current_tick: i32,
        fee_growth_global_0: U256,
        fee_growth_global_1: U256,
    ) -> (U256, U256) {
        // Ensure both ticks exist by initializing them first
        self.ticks
            .entry(lower_tick)
            .or_insert_with(|| PoolTick::from_tick(lower_tick));
        self.ticks
            .entry(upper_tick)
            .or_insert_with(|| PoolTick::from_tick(upper_tick));

        // Now safely access both ticks (they're guaranteed to exist)
        let lower_tick = &self.ticks[&lower_tick];
        let upper_tick = &self.ticks[&upper_tick];

        // Calculate the fee growth below
        let fee_growth_below_0 = if current_tick >= lower_tick.value {
            lower_tick.fee_growth_outside_0
        } else {
            fee_growth_global_0 - lower_tick.fee_growth_outside_0
        };
        let fee_growth_below_1 = if current_tick >= lower_tick.value {
            lower_tick.fee_growth_outside_1
        } else {
            fee_growth_global_1 - lower_tick.fee_growth_outside_1
        };

        // Calculate the fee growth above
        let fee_growth_above_0 = if current_tick < upper_tick.value {
            upper_tick.fee_growth_outside_0
        } else {
            fee_growth_global_0 - upper_tick.fee_growth_outside_0
        };
        let fee_growth_above_1 = if current_tick < upper_tick.value {
            upper_tick.fee_growth_outside_1
        } else {
            fee_growth_global_1 - upper_tick.fee_growth_outside_1
        };

        // Calculate the fee growth inside
        let fee_growth_inside_0 = fee_growth_global_0 - fee_growth_below_0 - fee_growth_above_0;
        let fee_growth_inside_1 = fee_growth_global_1 - fee_growth_below_1 - fee_growth_above_1;

        (fee_growth_inside_0, fee_growth_inside_1)
    }

    /// Internal helper to update tick data and return flip status.
    fn update_tick_data(
        &mut self,
        tick: i32,
        tick_current: i32,
        liquidity_delta: i128,
        upper: bool,
        fee_growth_global_0: U256,
        fee_growth_global_1: U256,
    ) -> bool {
        let max_liquidity_per_tick = self.max_liquidity_per_tick;
        let tick = self.get_tick_or_init(tick);

        let liquidity_gross_before = tick.update_liquidity(liquidity_delta, upper);
        let liquidity_gross_after = tick.liquidity_gross;
        assert!(
            liquidity_gross_after <= max_liquidity_per_tick,
            "Liquidity exceeds maximum per tick"
        );

        if liquidity_gross_before == 0 {
            // By convention, we assume that all growth before a tick was initialized happened _below_ the tick
            if tick.value <= tick_current {
                tick.fee_growth_outside_0 = fee_growth_global_0;
                tick.fee_growth_outside_1 = fee_growth_global_1;
            }
            tick.initialized = true;
        }

        // Check if tick was flipped from inactive to active or vice versa
        (liquidity_gross_after == 0) != (liquidity_gross_before == 0)
    }

    /// Updates liquidity at a specific tick and manages the tick bitmap.
    pub fn update(
        &mut self,
        tick: i32,
        tick_current: i32,
        liquidity_delta: i128,
        upper: bool,
        fee_growth_global_0: U256,
        fee_growth_global_1: U256,
    ) -> bool {
        let flipped = self.update_tick_data(
            tick,
            tick_current,
            liquidity_delta,
            upper,
            fee_growth_global_0,
            fee_growth_global_1,
        );

        // Only flip the bitmap if the tick actually flipped state
        if flipped {
            self.tick_bitmap.flip_tick(tick);
        }

        flipped
    }

    /// Crosses a tick during a swap, updating fee growth tracking.
    pub fn cross_tick(
        &mut self,
        tick: i32,
        fee_growth_global_0: U256,
        fee_growth_global_1: U256,
    ) -> i128 {
        let tick = self.get_tick_or_init(tick);
        tick.update_fee_growth(fee_growth_global_0, fee_growth_global_1);

        tick.liquidity_net
    }

    /// Returns the number of currently active (initialized) ticks.
    #[must_use]
    pub fn active_tick_count(&self) -> usize {
        self.ticks
            .iter()
            .filter(|(_, tick)| self.is_tick_initialized(tick.value))
            .count()
    }

    /// Returns the total number of ticks stored in the map.
    #[must_use]
    pub fn total_tick_count(&self) -> usize {
        self.ticks.len()
    }

    /// Returns a reference to all ticks in the map for debugging/analysis purposes.
    #[must_use]
    pub fn get_all_ticks(&self) -> &HashMap<i32, PoolTick> {
        &self.ticks
    }

    /// Sets the tick data for a specific tick index.
    pub fn set_tick(&mut self, tick_data: PoolTick) {
        let tick = tick_data.value;
        self.ticks.insert(tick, tick_data);
    }

    /// Restores a tick from a snapshot, updating both tick data and bitmap.
    ///
    /// This method is used when restoring pool state from a saved snapshot.
    /// It sets the tick data and updates the bitmap if the tick is initialized.
    pub fn restore_tick(&mut self, tick_data: PoolTick) {
        let is_initialized = tick_data.initialized;
        let tick_value = tick_data.value;

        self.set_tick(tick_data);

        // Update bitmap if the tick is initialized
        if is_initialized {
            self.tick_bitmap.flip_tick(tick_value);
        }
    }

    /// Clears all data in a tick by removing it from the tick map.
    pub fn clear(&mut self, tick: i32) {
        self.ticks.remove(&tick);
    }

    /// Finds the next initialized tick after the given tick.
    pub fn next_initialized_tick(&self, tick: i32, lte: bool) -> (i32, bool) {
        self.tick_bitmap
            .next_initialized_tick_within_one_word(tick, lte)
    }

    /// Checks if a tick is initialized in the bitmap.
    pub fn is_tick_initialized(&self, tick: i32) -> bool {
        self.tick_bitmap.is_initialized(tick)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::{fixture, rstest};

    use super::*;

    #[fixture]
    fn tick_map() -> TickMap {
        TickMap::new(1)
    }

    #[rstest]
    fn test_new_tick_maps(tick_map: TickMap) {
        assert_eq!(tick_map.active_tick_count(), 0);
        assert_eq!(tick_map.liquidity, 0);
    }

    #[rstest]
    fn test_get_fee_growth_inside_uninitialized_ticks(mut tick_map: TickMap) {
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(15);

        // If tick is inside: Tick 0 is inside -2 and 2
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 0, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::from_str("15").unwrap());
        assert_eq!(fee_growth_inside_1, U256::from_str("15").unwrap());

        // If tick is above: Tick 4 is not in [-2,2] so above 2
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 4, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::ZERO);
        assert_eq!(fee_growth_inside_1, U256::ZERO);

        // If tick is below: Tick -4 is not in [-2,2] so below -2
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, -4, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::ZERO);
        assert_eq!(fee_growth_inside_1, U256::ZERO);
    }

    #[rstest]
    fn test_get_fee_growth_inside_if_upper_tick_is_below(mut tick_map: TickMap) {
        tick_map.set_tick(PoolTick::new(
            2, // Set 2 at the upper range boundary
            0,
            0,
            U256::from(2),
            U256::from(3),
            true,
            0,
        ));
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(15);
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 0, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::from(13));
        assert_eq!(fee_growth_inside_1, U256::from(12));
    }

    #[rstest]
    fn test_get_fee_growth_inside_if_lower_tick_is_above(mut tick_map: TickMap) {
        tick_map.set_tick(PoolTick::new(
            -2, // Set -2 at the lower range boundary
            0,
            0,
            U256::from(2),
            U256::from(3),
            true,
            0,
        ));
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(15);
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 0, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::from(13));
        assert_eq!(fee_growth_inside_1, U256::from(12));
    }

    #[rstest]
    fn test_get_fee_growth_inside_if_upper_and_lower_tick_are_initialized(mut tick_map: TickMap) {
        tick_map.set_tick(PoolTick::new(
            -2, // Set -2 at the lower range boundary
            0,
            0,
            U256::from(2),
            U256::from(3),
            true,
            0,
        ));
        tick_map.set_tick(PoolTick::new(
            2, // Set -2 at the lower range boundary
            0,
            0,
            U256::from(4),
            U256::from(1),
            true,
            0,
        ));
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(15);
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 0, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::from(9));
        assert_eq!(fee_growth_inside_1, U256::from(11));
    }

    #[rstest]
    fn test_get_fee_growth_inside_with_overflow(mut tick_map: TickMap) {
        tick_map.set_tick(PoolTick::new(
            -2,
            0,
            0,
            U256::MAX - U256::from(3u32), // MaxUint256 - 3
            U256::MAX - U256::from(2u32), // MaxUint256 - 2
            true,
            0,
        ));
        tick_map.set_tick(PoolTick::new(
            2,
            0,
            0,
            U256::from(3u32),
            U256::from(5u32),
            true,
            0,
        ));
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(15);
        let (fee_growth_inside_0, fee_growth_inside_1) =
            tick_map.get_fee_growth_inside(-2, 2, 0, fee_growth_global_0, fee_growth_global_1);
        assert_eq!(fee_growth_inside_0, U256::from(16u32));
        assert_eq!(fee_growth_inside_1, U256::from(13u32));
    }

    #[rstest]
    fn test_update_flips_from_zero_to_nonzero(mut tick_map: TickMap) {
        // Initially tick should not be initialized in bitmap
        assert!(!tick_map.is_tick_initialized(0));

        let flipped = tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);
        assert!(flipped);

        // After flipping from zero to nonzero, tick should be initialized in bitmap
        assert!(tick_map.is_tick_initialized(0));
    }

    #[rstest]
    fn test_update_does_not_flip_from_nonzero_to_greater_nonzero(mut tick_map: TickMap) {
        // First update: flip from 0 to 1
        tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);
        assert!(tick_map.is_tick_initialized(0));

        // Second update: should not flip from 1 to 2
        let flipped = tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);
        assert!(!flipped);

        // Bitmap should remain unchanged (still initialized)
        assert!(tick_map.is_tick_initialized(0));
    }

    #[rstest]
    fn test_update_flips_from_nonzero_to_zero(mut tick_map: TickMap) {
        // First update: flip from 0 to 1
        let flipped_first = tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);
        assert!(flipped_first);
        assert!(tick_map.is_tick_initialized(0));

        // Second update: flip from 1 to 0 (remove all liquidity)
        let flipped_second = tick_map.update(0, 0, -1, false, U256::ZERO, U256::ZERO);
        assert!(flipped_second);

        // After flipping back to zero, tick should not be initialized in bitmap
        assert!(!tick_map.is_tick_initialized(0));
    }

    #[rstest]
    fn test_update_does_not_flip_from_nonzero_to_lesser_nonzero(mut tick_map: TickMap) {
        // First update: flip from 0 to 2
        tick_map.update(0, 0, 2, false, U256::ZERO, U256::ZERO);
        assert!(tick_map.is_tick_initialized(0));

        // Second update: should not flip from 2 to 1 (remove some but not all liquidity)
        let flipped = tick_map.update(0, 0, -1, false, U256::ZERO, U256::ZERO);
        assert!(!flipped);

        // Bitmap should remain unchanged (still initialized)
        assert!(tick_map.is_tick_initialized(0));
    }

    #[rstest]
    #[should_panic(expected = "Liquidity exceeds maximum per tick")]
    fn test_update_reverts_if_total_liquidity_gross_exceeds_max() {
        let mut tick_map = TickMap::new(200); // Higher tick spacing = lower max liquidity per tick

        // Add liquidity close to max
        let max_liquidity = tick_map.max_liquidity_per_tick;
        tick_map.update(
            0,
            0,
            (max_liquidity / 2) as i128,
            false,
            U256::ZERO,
            U256::ZERO,
        );
        tick_map.update(
            0,
            0,
            (max_liquidity / 2) as i128,
            true,
            U256::ZERO,
            U256::ZERO,
        );

        // This should panic as it exceeds max liquidity per tick
        tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);
    }

    #[rstest]
    fn test_update_nets_liquidity_based_on_upper_flag(mut tick_map: TickMap) {
        // Update with upper=false: liquidity_net += delta
        tick_map.update(0, 0, 2, false, U256::ZERO, U256::ZERO);
        // Update with upper=true: liquidity_net -= delta
        tick_map.update(0, 0, 1, true, U256::ZERO, U256::ZERO);
        // Update with upper=true: liquidity_net -= delta
        tick_map.update(0, 0, 3, true, U256::ZERO, U256::ZERO);
        // Update with upper=false: liquidity_net += delta
        tick_map.update(0, 0, 1, false, U256::ZERO, U256::ZERO);

        let tick = tick_map.get_tick(0).unwrap();

        // liquidity_gross should be the sum of all absolute deltas: 2 + 1 + 3 + 1 = 7
        assert_eq!(tick.liquidity_gross, 7);

        // liquidity_net should be: 2 - 1 - 3 + 1 = -1
        assert_eq!(tick.liquidity_net, -1);
    }

    #[rstest]
    fn test_update_assumes_all_growth_happens_below_ticks_lte_current_tick() {
        let mut tick_map = TickMap::new(1);
        let fee_growth_global_0 = U256::from(15);
        let fee_growth_global_1 = U256::from(2);
        // Update tick 1 when current tick is 1 (tick <= current_tick)
        tick_map.update(1, 1, 1, false, fee_growth_global_0, fee_growth_global_1);

        let tick = tick_map.get_tick(1).unwrap();

        // Since tick <= current_tick, fee growth outside should be set to global values
        assert_eq!(tick.fee_growth_outside_0, U256::from(15u32));
        assert_eq!(tick.fee_growth_outside_1, U256::from(2u32));
        assert!(tick.initialized);
        assert_eq!(tick.liquidity_gross, 1);
        assert_eq!(tick.liquidity_net, 1);
    }

    #[rstest]
    fn test_update_does_not_set_growth_fields_if_tick_already_initialized() {
        let mut tick_map = TickMap::new(1);
        let fee_growth_0_initial = U256::from(1);
        let fee_growth_1_initial = U256::from(2);
        // First update: Initialize the tick
        tick_map.update(1, 1, 1, false, fee_growth_0_initial, fee_growth_1_initial);

        // Second update: Different fee growth values, but tick is already initialized
        let fee_growth_0_second = U256::from(6);
        let fee_growth_1_second = U256::from(7);
        tick_map.update(1, 1, 1, false, fee_growth_0_second, fee_growth_1_second);

        let tick = tick_map.get_tick(1).unwrap();

        // Fee growth outside should still be the original values from first initialization
        assert_eq!(tick.fee_growth_outside_0, U256::from(1u32)); // Still 1, not 6
        assert_eq!(tick.fee_growth_outside_1, U256::from(2u32)); // Still 2, not 7
        assert!(tick.initialized);
        assert_eq!(tick.liquidity_gross, 2); // Should be 1 + 1 = 2
        assert_eq!(tick.liquidity_net, 2); // Should be 1 + 1 = 2
    }

    #[rstest]
    fn test_update_does_not_set_growth_fields_for_ticks_gt_current_tick() {
        let mut tick_map = TickMap::new(1);
        let fee_growth_global_0 = U256::from(1);
        let fee_growth_global_1 = U256::from(2u32);
        // Update tick 2 when current tick is 1 (tick > current_tick)
        tick_map.update(2, 1, 1, false, fee_growth_global_0, fee_growth_global_1);

        let tick = tick_map.get_tick(2).unwrap();

        // Since tick > current_tick, fee growth outside should remain 0 (not set to global values)
        assert_eq!(tick.fee_growth_outside_0, U256::ZERO); // Should be 0, not 1
        assert_eq!(tick.fee_growth_outside_1, U256::ZERO); // Should be 0, not 2
        assert!(tick.initialized);
        assert_eq!(tick.liquidity_gross, 1);
        assert_eq!(tick.liquidity_net, 1);
    }

    #[rstest]
    fn test_clear_deletes_all_data_in_tick(mut tick_map: TickMap) {
        // Set a tick with various data
        tick_map.set_tick(PoolTick::new(
            2,
            3,
            4,
            U256::from(1u32),
            U256::from(2u32),
            true,
            0,
        ));

        // Verify the tick exists with the set data
        let tick_before = tick_map.get_tick(2).unwrap();
        assert_eq!(tick_before.fee_growth_outside_0, U256::from(1u32));
        assert_eq!(tick_before.fee_growth_outside_1, U256::from(2u32));
        assert_eq!(tick_before.liquidity_gross, 3);
        assert_eq!(tick_before.liquidity_net, 4);
        assert!(tick_before.initialized);

        // Clear the tick
        tick_map.clear(2);

        // Verify the tick no longer exists
        assert_eq!(tick_map.get_tick(2), None);
    }

    #[rstest]
    fn test_cross_tick_flips_growth_variables(mut tick_map: TickMap) {
        // Set a tick with initial values
        tick_map.set_tick(PoolTick::new(
            2,
            3,
            4,
            U256::from(1u32),
            U256::from(2u32),
            true,
            7,
        ));

        // Cross the tick with global values: (7, 9)
        // Expected results: fee_growth_outside = global - current
        // fee_growth_outside_0: 7 - 1 = 6
        // fee_growth_outside_1: 9 - 2 = 7
        let liquidity_net = tick_map.cross_tick(
            2,
            U256::from(7u32), // fee_growth_global_0
            U256::from(9u32), // fee_growth_global_1
        );

        let tick = tick_map.get_tick(2).unwrap();

        // Verify fee growth variables were flipped (global - current)
        assert_eq!(tick.fee_growth_outside_0, U256::from(6u32)); // 7 - 1 = 6
        assert_eq!(tick.fee_growth_outside_1, U256::from(7u32)); // 9 - 2 = 7

        // Verify liquidity_net is returned
        assert_eq!(liquidity_net, 4);

        // Other fields should remain unchanged
        assert_eq!(tick.liquidity_gross, 3);
        assert_eq!(tick.liquidity_net, 4);
        assert!(tick.initialized);
    }
}
