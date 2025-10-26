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

use super::full_math::FullMath;
use crate::defi::tick_map::tick_math::get_sqrt_ratio_at_tick;

/// Encodes the sqrt ratio of two token amounts as a Q64.96 fixed point number.
///
/// Calculates sqrt(amount0 / amount1) * 2^96 to encode the price ratio between
/// two tokens as a fixed-point number suitable for AMM calculations.
///
/// # Panics
///
/// This function panics if:
/// - `amount1` is zero (division by zero)
/// - `sqrt(amount1)` is zero during overflow handling
/// - Mathematical operations result in overflow during `mul_div`
pub fn encode_sqrt_ratio_x96(amount0: u128, amount1: u128) -> U160 {
    let amount0_u256 = U256::from(amount0);
    let amount1_u256 = U256::from(amount1);

    if amount1_u256.is_zero() {
        panic!("Division by zero");
    }
    if amount0_u256.is_zero() {
        return U160::ZERO;
    }

    // We need to calculate: sqrt(amount0 / amount1) * 2^96
    // To maintain precision, we'll calculate: sqrt(amount0 * 2^192 / amount1)
    // This is because: sqrt(amount0/amount1) * 2^96 = sqrt(amount0 * 2^192 / amount1)

    // First, scale amount0 by 2^192
    let q192 = U256::from(1u128) << 192;

    // Check if amount0 * 2^192 would overflow
    if amount0_u256 > U256::MAX / q192 {
        // If it would overflow, we need to handle it differently
        // We'll use: sqrt(amount0) * 2^96 / sqrt(amount1)
        let sqrt_amount0 = FullMath::sqrt(amount0_u256);
        let sqrt_amount1 = FullMath::sqrt(amount1_u256);

        if sqrt_amount1.is_zero() {
            panic!("Division by zero in sqrt");
        }

        let q96 = U256::from(1u128) << 96;

        // Use FullMath for precise division
        let result = FullMath::mul_div(sqrt_amount0, q96, sqrt_amount1).expect("mul_div overflow");

        // Convert to U160, truncating if necessary
        return if result > U256::from(U160::MAX) {
            U160::MAX
        } else {
            U160::from(result)
        };
    }

    // Standard path: calculate (amount0 * 2^192) / amount1, then sqrt
    let ratio_q192 = FullMath::mul_div(amount0_u256, q192, amount1_u256).expect("mul_div overflow");

    // Take the square root of the ratio
    let sqrt_result = FullMath::sqrt(ratio_q192);

    // Convert to U160, truncating if necessary
    if sqrt_result > U256::from(U160::MAX) {
        U160::MAX
    } else {
        U160::from(sqrt_result)
    }
}

/// Calculates the next sqrt price when trading token0 for token1, rounding up.
fn get_next_sqrt_price_from_amount0_rounding_up(
    sqrt_price_x96: U160,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> U160 {
    if amount.is_zero() {
        return sqrt_price_x96;
    }
    let numerator = U256::from(liquidity) << 96;
    let sqrt_price_x96 = U256::from(sqrt_price_x96);
    let product = amount * sqrt_price_x96;

    if add {
        if product / amount == sqrt_price_x96 {
            let denominator = numerator + product;
            if denominator >= numerator {
                // always fit to 160bits
                let result = FullMath::mul_div_rounding_up(numerator, sqrt_price_x96, denominator)
                    .expect("mul_div_rounding_up failed");
                return U160::from(result);
            }
        }

        // Fallback: divRoundingUp(numerator1, (numerator1 / sqrtPX96).add(amount))
        let fallback_denominator = (numerator / sqrt_price_x96) + amount;
        let result = FullMath::div_rounding_up(numerator, fallback_denominator)
            .expect("div_rounding_up failed");

        // Check if result fits in U160
        if result > U256::from(U160::MAX) {
            panic!("Result overflows U160");
        }
        U160::from(result)
    } else {
        // require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product);
        if !((product / amount) == sqrt_price_x96 && numerator > product) {
            panic!("Invalid conditions for amount0 removal: overflow or underflow detected")
        }

        let denominator = numerator - product;
        let result = FullMath::mul_div_rounding_up(numerator, sqrt_price_x96, denominator)
            .expect("mul_div_rounding_up failed");
        U160::from(result)
    }
}

/// Calculates the next sqrt price when trading token1 for token0, rounding down.
fn get_next_sqrt_price_from_amount1_rounding_down(
    sqrt_price_x96: U160,
    liquidity: u128,
    amount: U256,
    add: bool,
) -> U160 {
    // if we're adding (subtracting), rounding down requires rounding the quotient down (up)
    // in both cases, avoid a mulDiv for most inputs
    if add {
        let quotient = if amount <= U256::from(U160::MAX) {
            // We have a small amount and use only bit shifting for efficiency
            (amount << 96) / U256::from(liquidity)
        } else {
            // Use mul_div to prevent overflow
            FullMath::mul_div(amount, U256::from(1u128) << 96, U256::from(liquidity))
                .unwrap_or(U256::ZERO)
        };

        // sqrtPX96.add(quotient).toUint160()
        U160::from(U256::from(sqrt_price_x96) + quotient)
    } else {
        let quotient = if amount <= U256::from(U160::MAX) {
            // UnsafeMath.divRoundingUp(amount << FixedPoint96.RESOLUTION, liquidity)
            FullMath::div_rounding_up(amount << 96, U256::from(liquidity)).unwrap_or(U256::ZERO)
        } else {
            // FullMath.mulDivRoundingUp(amount, FixedPoint96.Q96, liquidity)
            FullMath::mul_div_rounding_up(amount, U256::from(1u128) << 96, U256::from(liquidity))
                .unwrap_or(U256::ZERO)
        };

        // require(sqrtPX96 > quotient);
        if U256::from(sqrt_price_x96) <= quotient {
            panic!("sqrt_price_x96 must be greater than quotient");
        }

        // always fits 160 bits
        U160::from(U256::from(sqrt_price_x96) - quotient)
    }
}

/// Calculates the next sqrt price given an input amount.
///
/// # Panics
/// Panics if `sqrt_price_x96` is zero or if `liquidity` is zero.
pub fn get_next_sqrt_price_from_input(
    sqrt_price_x96: U160,
    liquidity: u128,
    amount_in: U256,
    zero_for_one: bool,
) -> U160 {
    assert!(
        sqrt_price_x96 > U160::ZERO,
        "sqrt_price_x96 must be greater than zero"
    );
    assert!(liquidity > 0, "Liquidity must be greater than zero");

    if zero_for_one {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_price_x96, liquidity, amount_in, true)
    } else {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_price_x96, liquidity, amount_in, true)
    }
}

/// Calculates the next sqrt price given an output amount.
///
/// # Panics
/// Panics if `sqrt_price_x96` is zero or if `liquidity` is zero.
pub fn get_next_sqrt_price_from_output(
    sqrt_price_x96: U160,
    liquidity: u128,
    amount_out: U256,
    zero_for_one: bool,
) -> U160 {
    assert!(
        sqrt_price_x96 > U160::ZERO,
        "sqrt_price_x96 must be greater than zero"
    );
    assert!(liquidity > 0, "Liquidity must be greater than zero");

    if zero_for_one {
        get_next_sqrt_price_from_amount1_rounding_down(sqrt_price_x96, liquidity, amount_out, false)
    } else {
        get_next_sqrt_price_from_amount0_rounding_up(sqrt_price_x96, liquidity, amount_out, false)
    }
}

/// Calculates the amount of token0 delta between two sqrt price ratios.
#[must_use]
pub fn get_amount0_delta(
    sqrt_ratio_ax96: U160,
    sqrt_ratio_bx96: U160,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sqrt_ratio_a, sqrt_ratio_b) = if sqrt_ratio_ax96 > sqrt_ratio_bx96 {
        (sqrt_ratio_bx96, sqrt_ratio_ax96)
    } else {
        (sqrt_ratio_ax96, sqrt_ratio_bx96)
    };

    let numerator1 = U256::from(liquidity) << 96;
    let numerator2 = U256::from(sqrt_ratio_b - sqrt_ratio_a);

    if round_up {
        // Use mul_div_rounding_up for the first operation
        let result =
            FullMath::mul_div_rounding_up(numerator1, numerator2, U256::from(sqrt_ratio_b))
                .unwrap_or(U256::ZERO);

        // Use proper div_rounding_up for the second operation to match Solidity UnsafeMath.divRoundingUp
        FullMath::div_rounding_up(result, U256::from(sqrt_ratio_a)).unwrap_or(U256::ZERO)
    } else {
        let result = FullMath::mul_div(numerator1, numerator2, U256::from(sqrt_ratio_b))
            .unwrap_or(U256::ZERO);
        result / U256::from(sqrt_ratio_a)
    }
}
/// Calculates the amount of token1 delta between two sqrt price ratios.
#[must_use]
pub fn get_amount1_delta(
    sqrt_ratio_ax96: U160,
    sqrt_ratio_bx96: U160,
    liquidity: u128,
    round_up: bool,
) -> U256 {
    let (sqrt_ratio_a, sqrt_ratio_b) = if sqrt_ratio_ax96 > sqrt_ratio_bx96 {
        (sqrt_ratio_bx96, sqrt_ratio_ax96)
    } else {
        (sqrt_ratio_ax96, sqrt_ratio_bx96)
    };

    let liquidity_u256 = U256::from(liquidity);
    let sqrt_ratio_diff = U256::from(sqrt_ratio_b - sqrt_ratio_a);
    let q96 = U256::from(1u128) << 96;

    if round_up {
        FullMath::mul_div_rounding_up(liquidity_u256, sqrt_ratio_diff, q96).unwrap_or(U256::ZERO)
    } else {
        FullMath::mul_div(liquidity_u256, sqrt_ratio_diff, q96).unwrap_or(U256::ZERO)
    }
}

/// Calculates the token amounts required for a given liquidity position.
#[must_use]
pub fn get_amounts_for_liquidity(
    sqrt_ratio_x96: U160,
    tick_lower: i32,
    tick_upper: i32,
    liquidity: u128,
    round_up: bool,
) -> (U256, U256) {
    let sqrt_ratio_lower_x96 = get_sqrt_ratio_at_tick(tick_lower);
    let sqrt_ratio_upper_x96 = get_sqrt_ratio_at_tick(tick_upper);

    // Ensure lower <= upper
    let (sqrt_ratio_a, sqrt_ratio_b) = if sqrt_ratio_lower_x96 > sqrt_ratio_upper_x96 {
        (sqrt_ratio_upper_x96, sqrt_ratio_lower_x96)
    } else {
        (sqrt_ratio_lower_x96, sqrt_ratio_upper_x96)
    };

    let amount0 = if sqrt_ratio_x96 <= sqrt_ratio_a {
        // Current price is below the range, all liquidity is in token0
        get_amount0_delta(sqrt_ratio_a, sqrt_ratio_b, liquidity, round_up)
    } else if sqrt_ratio_x96 < sqrt_ratio_b {
        // Current price is within the range
        get_amount0_delta(sqrt_ratio_x96, sqrt_ratio_b, liquidity, round_up)
    } else {
        // Current price is above the range, no token0 needed
        U256::ZERO
    };

    let amount1 = if sqrt_ratio_x96 < sqrt_ratio_a {
        // Current price is below the range, no token1 needed
        U256::ZERO
    } else if sqrt_ratio_x96 < sqrt_ratio_b {
        // Current price is within the range
        get_amount1_delta(sqrt_ratio_a, sqrt_ratio_x96, liquidity, round_up)
    } else {
        // Current price is above the range, all liquidity is in token1
        get_amount1_delta(sqrt_ratio_a, sqrt_ratio_b, liquidity, round_up)
    };

    (amount0, amount1)
}

/// Expands an amount to 18 decimal places (multiplies by 10^18).
pub fn expand_to_18_decimals(amount: u64) -> u128 {
    amount as u128 * 10u128.pow(18)
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    // Most of the tests are based on https://github.com/Uniswap/v3-core/blob/main/test/SqrtPriceMath.spec.ts
    use rstest::*;

    use super::*;
    use crate::defi::tick_map::full_math::Q96_U160;

    #[rstest]
    #[should_panic(expected = "sqrt_price_x96 must be greater than zero")]
    fn test_if_get_next_sqrt_price_from_input_panic_if_price_zero() {
        let _ = get_next_sqrt_price_from_input(U160::ZERO, 1, U256::ZERO, true);
    }

    #[rstest]
    #[should_panic(expected = "Liquidity must be greater than zero")]
    fn test_if_get_next_sqrt_price_from_input_panic_if_liquidity_zero() {
        let _ = get_next_sqrt_price_from_input(U160::from(1), 0, U256::ZERO, true);
    }

    #[rstest]
    #[should_panic(expected = "Uint conversion error: Value is too large for Uint<160>")]
    fn test_if_get_next_sqrt_price_from_input_panics_from_big_price() {
        let price = U160::MAX - U160::from(1);
        let _ = get_next_sqrt_price_from_input(price, 1024, U256::from(1024), false);
    }

    #[rstest]
    fn test_any_input_amount_cannot_underflow_the_price() {
        // Testing that when we have minimal price(1) and an enormous input amount (2^255)
        // the price calculation doesn't "underflow" to zero or wrap around to invalid value
        let price = U160::from(1);
        let liquidity = 1;
        let amount_in = U256::from(2).pow(U256::from(255));
        let result = get_next_sqrt_price_from_input(price, liquidity, amount_in, true);
        assert_eq!(result, U160::from(1));
    }

    #[rstest]
    fn test_returns_input_price_if_amount_in_is_zero_and_zero_for_one_true() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let liquidity = expand_to_18_decimals(1) / 10;
        let result = get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, true);
        assert_eq!(result, price);
    }

    #[rstest]
    fn test_returns_input_price_if_amount_in_is_zero_and_zero_for_one_false() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let liquidity = expand_to_18_decimals(1) / 10;
        let result = get_next_sqrt_price_from_input(price, liquidity, U256::ZERO, false);
        assert_eq!(result, price);
    }

    #[rstest]
    fn test_returns_the_minimum_price_for_max_inputs() {
        let sqrt_p = U160::MAX;
        let liquidity = u128::MAX;
        let max_amount_no_overflow = U256::MAX - (U256::from(liquidity) << 96) / U256::from(sqrt_p);
        let result =
            get_next_sqrt_price_from_input(sqrt_p, liquidity, max_amount_no_overflow, true);
        assert_eq!(result, U160::from(1));
    }

    #[rstest]
    fn test_input_amount_of_0_1_token1() {
        let sqrt_q = get_next_sqrt_price_from_input(
            encode_sqrt_ratio_x96(1, 1),
            expand_to_18_decimals(1),
            U256::from(expand_to_18_decimals(1)) / U256::from(10),
            false,
        );
        assert_eq!(
            sqrt_q,
            U160::from_str_radix("87150978765690771352898345369", 10).unwrap()
        );
    }

    #[rstest]
    fn test_input_amount_of_0_1_token0() {
        let sqrt_q = get_next_sqrt_price_from_input(
            encode_sqrt_ratio_x96(1, 1),
            expand_to_18_decimals(1),
            U256::from(expand_to_18_decimals(1)) / U256::from(10),
            true,
        );
        assert_eq!(
            sqrt_q,
            U160::from_str_radix("72025602285694852357767227579", 10).unwrap()
        );
    }

    #[rstest]
    fn test_amount_in_greater_than_uint96_max_and_zero_for_one_true() {
        let result = get_next_sqrt_price_from_input(
            encode_sqrt_ratio_x96(1, 1),
            expand_to_18_decimals(10),
            U256::from(2).pow(U256::from(100)),
            true,
        );
        assert_eq!(
            result,
            U160::from_str_radix("624999999995069620", 10).unwrap()
        );
    }

    #[rstest]
    fn test_can_return_1_with_enough_amount_in_and_zero_for_one_true() {
        let result = get_next_sqrt_price_from_input(
            encode_sqrt_ratio_x96(1, 1),
            1,
            U256::MAX / U256::from(2),
            true,
        );
        assert_eq!(result, U160::from(1));
    }

    #[rstest]
    #[should_panic(
        expected = "Invalid conditions for amount0 removal: overflow or underflow detected"
    )]
    fn test_fails_if_output_amount_is_exactly_virtual_reserves_of_token0() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(4);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, false);
    }

    #[rstest]
    #[should_panic(
        expected = "Invalid conditions for amount0 removal: overflow or underflow detected"
    )]
    fn test_fails_if_output_amount_is_greater_than_virtual_reserves_of_token0() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(5);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, false);
    }

    #[rstest]
    #[should_panic(expected = "sqrt_price_x96 must be greater than quotient")]
    fn test_fails_if_output_amount_is_greater_than_virtual_reserves_of_token1() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(262145);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, true);
    }

    #[rstest]
    #[should_panic(expected = "sqrt_price_x96 must be greater than quotient")]
    fn test_fails_if_output_amount_is_exactly_virtual_reserves_of_token1() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(262144);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, true);
    }

    #[rstest]
    fn test_succeeds_if_output_amount_is_just_less_than_virtual_reserves_of_token1() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(262143);
        let result = get_next_sqrt_price_from_output(price, liquidity, amount_out, true);
        assert_eq!(
            result,
            U160::from_str_radix("77371252455336267181195264", 10).unwrap()
        );
    }

    #[rstest]
    fn test_returns_input_price_if_amount_out_is_zero_and_zero_for_one_true() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let liquidity = expand_to_18_decimals(1) / 10;
        let result = get_next_sqrt_price_from_output(price, liquidity, U256::ZERO, true);
        assert_eq!(result, price);
    }

    #[rstest]
    fn test_returns_input_price_if_amount_out_is_zero_and_zero_for_one_false() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let liquidity = expand_to_18_decimals(1) / 10;
        let result = get_next_sqrt_price_from_output(price, liquidity, U256::ZERO, false);
        assert_eq!(result, price);
    }

    #[rstest]
    fn test_output_amount_of_0_1_token1_zero_for_one_false() {
        let sqrt_q = get_next_sqrt_price_from_output(
            encode_sqrt_ratio_x96(1, 1),
            expand_to_18_decimals(1),
            U256::from(expand_to_18_decimals(1)) / U256::from(10),
            false,
        );
        assert_eq!(
            sqrt_q,
            U160::from_str_radix("88031291682515930659493278152", 10).unwrap()
        );
    }

    #[rstest]
    fn test_output_amount_of_0_1_token1_zero_for_one_true() {
        let sqrt_q = get_next_sqrt_price_from_output(
            encode_sqrt_ratio_x96(1, 1),
            expand_to_18_decimals(1),
            U256::from(expand_to_18_decimals(1)) / U256::from(10),
            true,
        );
        assert_eq!(
            sqrt_q,
            U160::from_str_radix("71305346262837903834189555302", 10).unwrap()
        );
    }

    #[rstest]
    #[should_panic(expected = "sqrt_price_x96 must be greater than zero")]
    fn test_if_get_next_sqrt_price_from_output_panic_if_price_zero() {
        let _ = get_next_sqrt_price_from_output(U160::ZERO, 1, U256::ZERO, true);
    }

    #[rstest]
    #[should_panic(expected = "Liquidity must be greater than zero")]
    fn test_if_get_next_sqrt_price_from_output_panic_if_liquidity_zero() {
        let _ = get_next_sqrt_price_from_output(U160::from(1), 0, U256::ZERO, true);
    }

    #[rstest]
    fn test_encode_sqrt_ratio_x98_some_values() {
        assert_eq!(encode_sqrt_ratio_x96(1, 1), Q96_U160);
        assert_eq!(
            encode_sqrt_ratio_x96(100, 1),
            U160::from(792281625142643375935439503360_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(1, 100),
            U160::from(7922816251426433759354395033_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(111, 333),
            U160::from(45742400955009932534161870629_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(333, 111),
            U160::from(137227202865029797602485611888_u128)
        );
    }

    #[rstest]
    fn test_get_amount0_delta_returns_0_if_liquidity_is_0() {
        let amount0 = get_amount0_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(2, 1),
            0,
            true,
        );
        assert_eq!(amount0, U256::ZERO);
    }

    #[rstest]
    fn test_get_amount0_delta_returns_0_if_prices_are_equal() {
        let amount0 = get_amount0_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(1, 1),
            0,
            true,
        );
        assert_eq!(amount0, U256::ZERO);
    }

    #[rstest]
    fn test_get_amount0_delta_returns_0_1_amount1_for_price_of_1_to_1_21() {
        let amount0 = get_amount0_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(121, 100),
            expand_to_18_decimals(1),
            true,
        );
        assert_eq!(
            amount0,
            U256::from_str_radix("90909090909090910", 10).unwrap()
        );

        let amount0_rounded_down = get_amount0_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(121, 100),
            expand_to_18_decimals(1),
            false,
        );

        assert_eq!(amount0_rounded_down, amount0 - U256::from(1));
    }

    #[rstest]
    fn test_get_amount0_delta_works_for_prices_that_overflow() {
        // Create large prices: 2^90 and 2^96
        let price_low =
            encode_sqrt_ratio_x96(U256::from(2).pow(U256::from(90)).try_into().unwrap(), 1);
        let price_high =
            encode_sqrt_ratio_x96(U256::from(2).pow(U256::from(96)).try_into().unwrap(), 1);

        let amount0_up = get_amount0_delta(price_low, price_high, expand_to_18_decimals(1), true);

        let amount0_down =
            get_amount0_delta(price_low, price_high, expand_to_18_decimals(1), false);

        assert_eq!(amount0_up, amount0_down + U256::from(1));
    }

    #[rstest]
    fn test_get_amount1_delta_returns_0_if_liquidity_is_0() {
        let amount1 = get_amount1_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(2, 1),
            0,
            true,
        );
        assert_eq!(amount1, U256::ZERO);
    }

    #[rstest]
    fn test_get_amount1_delta_returns_0_if_prices_are_equal() {
        let amount1 = get_amount1_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(1, 1),
            0,
            true,
        );
        assert_eq!(amount1, U256::ZERO);
    }

    #[rstest]
    fn test_get_amount1_delta_returns_0_1_amount1_for_price_of_1_to_1_21() {
        let amount1 = get_amount1_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(121, 100),
            expand_to_18_decimals(1),
            true,
        );
        assert_eq!(
            amount1,
            U256::from_str_radix("100000000000000000", 10).unwrap()
        );

        let amount1_rounded_down = get_amount1_delta(
            encode_sqrt_ratio_x96(1, 1),
            encode_sqrt_ratio_x96(121, 100),
            expand_to_18_decimals(1),
            false,
        );

        assert_eq!(amount1_rounded_down, amount1 - U256::from(1));
    }
}
