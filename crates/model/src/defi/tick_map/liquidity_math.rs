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

use crate::defi::tick_map::tick::PoolTick;

/// Add a signed liquidity delta to liquidity and panic if it overflows or underflows.
///
/// # Returns
///
/// The resulting liquidity after applying the delta.
///
/// # Panics
///
/// This function panics if:
/// - Adding positive delta causes overflow.
/// - Subtracting causes underflow.
pub fn liquidity_math_add(x: u128, y: i128) -> u128 {
    if y < 0 {
        let delta = (-y) as u128;
        let z = x.wrapping_sub(delta);
        assert!(
            z < x,
            "Liquidity subtraction underflow: x={}, y={}, delta={}, result={}",
            x,
            y,
            delta,
            z
        );
        z
    } else {
        let delta = y as u128;
        let z = x.wrapping_add(delta);
        assert!(
            z >= x,
            "Liquidity addition overflow: x={}, y={}, delta={}, result={}",
            x,
            y,
            delta,
            z
        );
        z
    }
}

/// Derives max liquidity per tick from a given tick spacing
pub fn tick_spacing_to_max_liquidity_per_tick(tick_spacing: i32) -> u128 {
    // Calculate min and max tick aligned to tick spacing
    let min_tick = (PoolTick::MIN_TICK / tick_spacing) * tick_spacing;
    let max_tick = (PoolTick::MAX_TICK / tick_spacing) * tick_spacing;

    // Calculate total number of ticks, cast to i64 to avoid potential overflow in subtraction
    let num_ticks = ((max_tick as i64 - min_tick as i64) / tick_spacing as i64) + 1;

    u128::MAX / num_ticks as u128
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_add() {
        assert_eq!(liquidity_math_add(1, 0), 1);
        assert_eq!(liquidity_math_add(1, 1), 2);
    }

    #[rstest]
    fn test_subtract_one() {
        assert_eq!(liquidity_math_add(1, -1), 0);
        assert_eq!(liquidity_math_add(3, -2), 1);
    }

    #[rstest]
    #[should_panic(expected = "Liquidity addition overflow")]
    fn test_addition_overflow() {
        let x = u128::MAX - 14; // Close to max so adding 15 will overflow
        liquidity_math_add(x, 15);
    }

    #[rstest]
    #[should_panic(expected = "Liquidity subtraction underflow")]
    fn test_subtraction_underflow_zero() {
        liquidity_math_add(0, -1);
    }

    #[rstest]
    #[should_panic(expected = "Liquidity subtraction underflow")]
    fn test_subtraction_underflow() {
        liquidity_math_add(3, -4);
    }

    #[rstest]
    fn test_tick_spacing_to_max_liquidity() {
        // 0.01 tier ot 1 tick spacing
        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(1),
            191757530477355301479181766273477
        );
        // 0.05 % tier or 10 tick spacing
        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(10),
            1917569901783203986719870431555990
        );
        // 0.3 % tier or 60 tick spacing
        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(60),
            11505743598341114571880798222544994
        );
        // 1.00% tier or 200 tick spacing
        assert_eq!(
            tick_spacing_to_max_liquidity_per_tick(200),
            38350317471085141830651933667504588
        );
    }
}
