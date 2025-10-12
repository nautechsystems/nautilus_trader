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

use crate::defi::tick_map::{bit_math::most_significant_bit, tick::PoolTick};

/// The minimum value that can be returned from get_sqrt_ratio_at_tick
pub const MIN_SQRT_RATIO: U160 = U160::from_limbs([4295128739u64, 0, 0]);

/// The maximum value that can be returned from get_sqrt_ratio_at_tick
pub const MAX_SQRT_RATIO: U160 = U160::from_limbs([
    0x5d951d5263988d26u64, // Lower 64 bits
    0xefd1fc6a50648849u64, // Middle 64 bits
    0xfffd8963u64,         // Upper 32 bits
]);

/// Returns the sqrt ratio as a Q64.96 for the given tick. The sqrt ratio is computed as
/// sqrt(1.0001)^tick.
///
/// ## Arguments
///
/// * `tick`: the tick for which to compute the sqrt ratio
///
/// ## Returns
///
/// The sqrt ratio as a Q64.96
///
/// # Panics
///
/// Panics if the absolute tick exceeds [`PoolTick::MAX_TICK`].
#[inline]
pub fn get_sqrt_ratio_at_tick(tick: i32) -> U160 {
    let abs_tick = tick.abs();

    assert!(
        abs_tick <= PoolTick::MAX_TICK,
        "Tick {} out of bounds",
        tick
    );

    // Equivalent: ratio = 2**128 / sqrt(1.0001) if abs_tick & 0x1 else 1 << 128
    let mut ratio = if abs_tick & 0x1 != 0 {
        U256::from_str_radix("fffcb933bd6fad37aa2d162d1a594001", 16).unwrap()
    } else {
        U256::from_str_radix("100000000000000000000000000000000", 16).unwrap()
    };

    // Iterate through 1th to 19th bit of abs_tick because MAX_TICK < 2**20
    if abs_tick & 0x2 != 0 {
        ratio =
            (ratio * U256::from_str_radix("fff97272373d413259a46990580e213a", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x4 != 0 {
        ratio =
            (ratio * U256::from_str_radix("fff2e50f5f656932ef12357cf3c7fdcc", 16).unwrap()) >> 128;
    };
    if abs_tick & 0x8 != 0 {
        ratio =
            (ratio * U256::from_str_radix("ffe5caca7e10e4e61c3624eaa0941cd0", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x10 != 0 {
        ratio =
            (ratio * U256::from_str_radix("ffcb9843d60f6159c9db58835c926644", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x20 != 0 {
        ratio =
            (ratio * U256::from_str_radix("ff973b41fa98c081472e6896dfb254c0", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x40 != 0 {
        ratio =
            (ratio * U256::from_str_radix("ff2ea16466c96a3843ec78b326b52861", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x80 != 0 {
        ratio =
            (ratio * U256::from_str_radix("fe5dee046a99a2a811c461f1969c3053", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x100 != 0 {
        ratio =
            (ratio * U256::from_str_radix("fcbe86c7900a88aedcffc83b479aa3a4", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x200 != 0 {
        ratio =
            (ratio * U256::from_str_radix("f987a7253ac413176f2b074cf7815e54", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x400 != 0 {
        ratio =
            (ratio * U256::from_str_radix("f3392b0822b70005940c7a398e4b70f3", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x800 != 0 {
        ratio =
            (ratio * U256::from_str_radix("e7159475a2c29b7443b29c7fa6e889d9", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x1000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("d097f3bdfd2022b8845ad8f792aa5825", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x2000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("a9f746462d870fdf8a65dc1f90e061e5", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x4000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("70d869a156d2a1b890bb3df62baf32f7", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x8000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("31be135f97d08fd981231505542fcfa6", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x10000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("9aa508b5b7a84e1c677de54f3e99bc9", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x20000 != 0 {
        ratio =
            (ratio * U256::from_str_radix("5d6af8dedb81196699c329225ee604", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x40000 != 0 {
        ratio = (ratio * U256::from_str_radix("2216e584f5fa1ea926041bedfe98", 16).unwrap()) >> 128;
    }
    if abs_tick & 0x80000 != 0 {
        ratio = (ratio * U256::from_str_radix("48a170391f7dc42444e8fa2", 16).unwrap()) >> 128;
    }

    if tick.is_positive() {
        ratio = U256::MAX / ratio;
    }

    ratio = (ratio + U256::from(0xffffffffu32)) >> 32;
    U160::from(ratio)
}

/// Returns the tick corresponding to the given sqrt ratio.
///
/// Converts a sqrt price ratio (as Q64.96 fixed point) back to its corresponding
/// tick value using logarithmic calculations. This is the inverse operation of
/// `get_sqrt_ratio_at_tick`.
///
/// # Panics
/// Panics if the sqrt price is outside the valid range:
/// - `sqrt_price_x96 < MIN_SQRT_RATIO` (too small)
/// - `sqrt_price_x96 >= MAX_SQRT_RATIO` (too large)
///
/// Valid range is approximately from tick -887272 to +887272.
pub fn get_tick_at_sqrt_ratio(sqrt_price_x96: U160) -> i32 {
    assert!(
        sqrt_price_x96 >= MIN_SQRT_RATIO && sqrt_price_x96 < MAX_SQRT_RATIO,
        "Sqrt price out of bounds"
    );

    let ratio = U256::from(sqrt_price_x96) << 32;
    let msb = most_significant_bit(ratio);

    // Build log_2_x64 using U256 throughout
    // When msb < 128, we simulate negative by subtracting from 2^256
    let mut log_2_x64 = if msb >= 128 {
        U256::from((msb - 128) as u64) << 64
    } else {
        // For negative values, use two's complement representation
        U256::MAX - (U256::from((128 - msb) as u64) << 64) + U256::from(1)
    };

    // Calculate r for iterations
    let mut r = if msb >= 128 {
        ratio >> (msb - 127)
    } else {
        ratio << (127 - msb)
    };

    // 14 iterations to compute the fractional part
    let mut decimals = U256::ZERO;
    for i in (50..=63).rev() {
        r = (r * r) >> 127;
        let f = r >> 128;
        if f > U256::ZERO {
            decimals |= U256::ONE << i;
            r >>= 1;
        }
    }

    // Add fractional bits to log_2_x64
    log_2_x64 |= decimals;

    // sqrt_ratio = sqrt(1.0001^tick)
    // tick = log_{sqrt(1.0001)}(sqrt_ratio) = log_2(sqrt_ratio) / log_2(sqrt(1.0001))
    // 2**64 / log_2(sqrt(1.0001)) = 255738958999603826347141
    let log_sqrt10001 = log_2_x64 * U256::from(255738958999603826347141u128);

    // Calculate tick bounds using wrapping arithmetic
    let tick_low_offset =
        U256::from_str_radix("3402992956809132418596140100660247210", 10).unwrap();
    let tick_hi_offset =
        U256::from_str_radix("291339464771989622907027621153398088495", 10).unwrap();

    let tick_low_u256: U256 = (log_sqrt10001 - tick_low_offset) >> 128;
    let tick_hi_u256: U256 = (log_sqrt10001 + tick_hi_offset) >> 128;

    // Convert to i32 by directly casting
    // The values after >> 128 should fit in i32 range
    // For negative values, the wraparound in U256 will be preserved in the cast
    let tick_low = tick_low_u256.as_le_bytes()[0] as i32
        | ((tick_low_u256.as_le_bytes()[1] as i32) << 8)
        | ((tick_low_u256.as_le_bytes()[2] as i32) << 16)
        | ((tick_low_u256.as_le_bytes()[3] as i32) << 24);
    let tick_hi = tick_hi_u256.as_le_bytes()[0] as i32
        | ((tick_hi_u256.as_le_bytes()[1] as i32) << 8)
        | ((tick_hi_u256.as_le_bytes()[2] as i32) << 16)
        | ((tick_hi_u256.as_le_bytes()[3] as i32) << 24);

    // Final selection
    if tick_low == tick_hi {
        tick_low
    } else if get_sqrt_ratio_at_tick(tick_hi) <= sqrt_price_x96 {
        tick_hi
    } else {
        tick_low
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;
    use crate::defi::tick_map::sqrt_price_math::encode_sqrt_ratio_x96;

    #[rstest]
    fn test_get_sqrt_ratio_at_tick_zero() {
        let sqrt_ratio = get_sqrt_ratio_at_tick(0);
        // At tick 0, price is 1, sqrt_price is 1, sqrt_price_x96 is 1 * 2^96
        let expected = U160::from(1u128) << 96;
        assert_eq!(sqrt_ratio, expected);
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio() {
        let sqrt_ratio_u160 = U160::from(1u128 << 96); // sqrt price = 1, price = 1
        let tick = get_tick_at_sqrt_ratio(sqrt_ratio_u160);
        assert_eq!(tick, 0);
    }

    #[rstest]
    #[should_panic(expected = "Tick 887273 out of bounds")]
    fn test_get_sqrt_ratio_at_tick_panics_above_max() {
        let _ = get_sqrt_ratio_at_tick(PoolTick::MAX_TICK + 1);
    }

    #[rstest]
    #[should_panic(expected = "Tick -887273 out of bounds")]
    fn test_get_sqrt_ratio_at_tick_panics_below_min() {
        let _ = get_sqrt_ratio_at_tick(PoolTick::MIN_TICK - 1);
    }

    // Tests for get_tick_at_sqrt_ratio matching the JavaScript tests
    #[rstest]
    #[should_panic(expected = "Sqrt price out of bounds")]
    fn test_get_tick_at_sqrt_ratio_throws_for_too_low() {
        let _ = get_tick_at_sqrt_ratio(MIN_SQRT_RATIO - U160::from(1));
    }

    #[rstest]
    #[should_panic(expected = "Sqrt price out of bounds")]
    fn test_get_tick_at_sqrt_ratio_throws_for_too_high() {
        let _ = get_tick_at_sqrt_ratio(MAX_SQRT_RATIO);
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio_min_tick() {
        let result = get_tick_at_sqrt_ratio(MIN_SQRT_RATIO);
        assert_eq!(result, PoolTick::MIN_TICK);
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ration_various_values() {
        assert_eq!(
            get_tick_at_sqrt_ratio(U160::from_str("511495728837967332084595714").unwrap()),
            -100860
        );
        assert_eq!(
            get_tick_at_sqrt_ratio(U160::from_str("14464772219441977173490711849216").unwrap()),
            104148
        );
        assert_eq!(
            get_tick_at_sqrt_ratio(U160::from_str("17148448136625419841777674413284").unwrap()),
            107552
        );
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio_min_tick_plus_one() {
        let result = get_tick_at_sqrt_ratio(U160::from(4295343490u64));
        assert_eq!(result, PoolTick::MIN_TICK + 1);
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio_max_tick_minus_one() {
        // Test with the exact value from Uniswap tests for MAX_TICK - 1
        // This value is: 1461373636630004318706518188784493106690254656249
        let sqrt_ratio =
            U160::from_str_radix("fffa429fbf7baeed2496f0a9f5ccf2bb4abf52f9", 16).unwrap();

        // This value should work now that MAX_SQRT_RATIO has been updated
        let result = get_tick_at_sqrt_ratio(sqrt_ratio);

        // This should give us MAX_TICK - 1 (887271)
        assert_eq!(
            result,
            PoolTick::MAX_TICK - 1,
            "Uniswap test value should map to MAX_TICK - 1"
        );
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio_closest_to_max_tick() {
        // Test the actual maximum valid sqrt_ratio
        let sqrt_ratio = MAX_SQRT_RATIO - U160::from(1);
        let result = get_tick_at_sqrt_ratio(sqrt_ratio);

        // Verify it's a valid positive tick less than MAX_TICK
        assert!(result > 0 && result < PoolTick::MAX_TICK);

        // Verify that MAX_SQRT_RATIO itself would panic (it's exclusive upper bound)
        // This is tested in test_get_tick_at_sqrt_ratio_throws_for_too_high
    }

    #[rstest]
    #[case::min_sqrt_ratio(MIN_SQRT_RATIO)]
    #[case::price_10_12_to_1(encode_sqrt_ratio_x96(1, 1000000000000))] // 10^12 / 1
    #[case::price_10_6_to_1(encode_sqrt_ratio_x96(1, 1000000))] // 10^6 / 1
    #[case::price_1_to_64(encode_sqrt_ratio_x96(64, 1))] // 1 / 64
    #[case::price_1_to_8(encode_sqrt_ratio_x96(8, 1))] // 1 / 8
    #[case::price_1_to_2(encode_sqrt_ratio_x96(2, 1))] // 1 / 2
    #[case::price_1_to_1(encode_sqrt_ratio_x96(1, 1))] // 1 / 1
    #[case::price_2_to_1(encode_sqrt_ratio_x96(1, 2))] // 2 / 1
    #[case::price_8_to_1(encode_sqrt_ratio_x96(1, 8))] // 8 / 1
    #[case::price_64_to_1(encode_sqrt_ratio_x96(1, 64))] // 64 / 1
    #[case::price_1_to_10_6(encode_sqrt_ratio_x96(1000000, 1))] // 1 / 10^6
    #[case::price_1_to_10_12(encode_sqrt_ratio_x96(1000000000000, 1))] // 1 / 10^12
    #[case::max_sqrt_ratio_minus_one(MAX_SQRT_RATIO - U160::from(1))]
    fn test_get_tick_at_sqrt_ratio_accuracy(#[case] ratio: U160) {
        let tick = get_tick_at_sqrt_ratio(ratio);

        // Test 1: Check that result is at most off by 1 from theoretical value
        let ratio_f64 = ratio.to_string().parse::<f64>().unwrap();
        let price = (ratio_f64 / (1u128 << 96) as f64).powi(2);
        let theoretical_tick = (price.ln() / 1.0001_f64.ln()).floor() as i32;
        let diff = (tick - theoretical_tick).abs();
        assert!(
            diff <= 1,
            "Tick {} differs from theoretical {} by more than 1",
            tick,
            theoretical_tick
        );

        // Test 2: Check that ratio is between tick and tick+1
        let ratio_of_tick = U256::from(get_sqrt_ratio_at_tick(tick));
        let ratio_of_tick_plus_one = U256::from(get_sqrt_ratio_at_tick(tick + 1));
        let ratio_u256 = U256::from(ratio);

        assert!(
            ratio_u256 >= ratio_of_tick,
            "Ratio {} should be >= ratio of tick {}",
            ratio_u256,
            ratio_of_tick
        );
        assert!(
            ratio_u256 < ratio_of_tick_plus_one,
            "Ratio {} should be < ratio of tick+1 {}",
            ratio_u256,
            ratio_of_tick_plus_one
        );
    }

    #[rstest]
    fn test_get_tick_at_sqrt_ratio_specific_values() {
        // Test some specific known values
        let test_cases = vec![
            (MIN_SQRT_RATIO, PoolTick::MIN_TICK),
            (U160::from(1u128 << 96), 0), // sqrt price = 1, price = 1, tick = 0
        ];

        for (sqrt_ratio, expected_tick) in test_cases {
            let result = get_tick_at_sqrt_ratio(sqrt_ratio);
            assert_eq!(
                result, expected_tick,
                "Failed for sqrt_ratio {}",
                sqrt_ratio
            );
        }
    }

    #[rstest]
    fn test_round_trip_tick_sqrt_ratio() {
        // Test round trip: tick -> sqrt_ratio -> tick
        // Note: Very high ticks (above ~790227) produce sqrt_ratios >= MAX_SQRT_RATIO,
        // so we limit our test to ticks that produce valid sqrt_ratios
        let test_ticks = vec![
            -887272, -100000, -1000, -100, -1, 0, 1, 100, 1000, 100000, 700000,
        ];

        for original_tick in test_ticks {
            let sqrt_ratio = get_sqrt_ratio_at_tick(original_tick);

            // Check if the sqrt_ratio is within bounds for get_tick_at_sqrt_ratio
            if sqrt_ratio < MAX_SQRT_RATIO {
                let recovered_tick = get_tick_at_sqrt_ratio(sqrt_ratio);

                // Should be exact for round trip
                assert_eq!(
                    recovered_tick, original_tick,
                    "Round trip failed: {} -> {} -> {}",
                    original_tick, sqrt_ratio, recovered_tick
                );
            } else {
                // For very high ticks, the sqrt_ratio exceeds MAX_SQRT_RATIO
                // This is expected behavior - not all ticks can round-trip
                println!(
                    "Tick {} produces sqrt_ratio {} which exceeds MAX_SQRT_RATIO",
                    original_tick, sqrt_ratio
                );
            }
        }
    }

    #[rstest]
    fn test_extreme_ticks_behavior() {
        let min_sqrt = get_sqrt_ratio_at_tick(PoolTick::MIN_TICK);
        assert_eq!(
            min_sqrt, MIN_SQRT_RATIO,
            "MIN_TICK should produce MIN_SQRT_RATIO"
        );
        let recovered_min = get_tick_at_sqrt_ratio(min_sqrt);
        assert_eq!(
            recovered_min,
            PoolTick::MIN_TICK,
            "MIN_TICK should round-trip correctly"
        );

        // MAX_TICK produces a value equal to MAX_SQRT_RATIO
        let max_sqrt = get_sqrt_ratio_at_tick(PoolTick::MAX_TICK);

        // Now that MAX_SQRT_RATIO has been updated to the actual max value,
        // get_sqrt_ratio_at_tick(MAX_TICK) should equal MAX_SQRT_RATIO
        assert_eq!(
            max_sqrt, MAX_SQRT_RATIO,
            "MAX_TICK should produce exactly MAX_SQRT_RATIO"
        );

        // The highest tick that can be passed to get_tick_at_sqrt_ratio is MAX_TICK - 1
        // because get_tick_at_sqrt_ratio requires sqrt_price_x96 < MAX_SQRT_RATIO (exclusive)
        let max_valid_sqrt = MAX_SQRT_RATIO - U160::from(1);
        let max_valid_tick = get_tick_at_sqrt_ratio(max_valid_sqrt);

        // This should give us MAX_TICK - 1 (887271)
        assert_eq!(
            max_valid_tick,
            PoolTick::MAX_TICK - 1,
            "MAX_SQRT_RATIO - 1 should map to MAX_TICK - 1"
        );
    }
}
