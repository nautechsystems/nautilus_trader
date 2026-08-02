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

use alloy_primitives::{U160, U256, U512};

use super::full_math::FullMath;
use crate::{
    defi::tick_map::tick_math::get_sqrt_ratio_at_tick,
    types::{PRICE_RAW_MAX, Price, fixed::FIXED_PRECISION},
};

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
#[must_use]
pub fn encode_sqrt_ratio_x96(amount0: u128, amount1: u128) -> U160 {
    let amount0_u256 = U256::from(amount0);
    let amount1_u256 = U256::from(amount1);

    assert!(!amount1_u256.is_zero(), "Division by zero");
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

        assert!(!sqrt_amount1.is_zero(), "Division by zero in sqrt");

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
        assert!(result <= U256::from(U160::MAX), "Result overflows U160");
        U160::from(result)
    } else {
        // require((product = amount * sqrtPX96) / amount == sqrtPX96 && numerator1 > product);
        assert!(
            (product / amount) == sqrt_price_x96 && numerator > product,
            "Invalid conditions for amount0 removal: overflow or underflow detected"
        );

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
        assert!(
            U256::from(sqrt_price_x96) > quotient,
            "sqrt_price_x96 must be greater than quotient"
        );

        // always fits 160 bits
        U160::from(U256::from(sqrt_price_x96) - quotient)
    }
}

/// Calculates the next sqrt price given an input amount.
///
/// # Panics
/// Panics if `sqrt_price_x96` is zero or if `liquidity` is zero.
#[must_use]
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
#[must_use]
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
#[must_use]
pub fn expand_to_18_decimals(amount: u64) -> u128 {
    u128::from(amount) * 10u128.pow(18)
}

/// Converts a sqrt price X96 to a raw Price (token1/token0 ratio without decimal adjustment).
///
/// To get fixed-point representation: `sqrt_price_x96^2 * 10^FIXED_PRECISION / 2^192`.
/// Scaling preserves the remainder from the full-width square so flooring occurs only once.
///
/// # Errors
///
/// Returns an error if:
/// - The price calculation overflows.
/// - The result exceeds `PRICE_RAW_MAX`.
pub fn decode_sqrt_price_x96_to_price(sqrt_price_x96: U160) -> anyhow::Result<Price> {
    let sqrt_price = U256::from(sqrt_price_x96);
    let fixed_scalar = FullMath::pow10(FIXED_PRECISION)?;
    let divisor = U256::from(1u128) << 192;
    let price_raw = FullMath::mul_div_scaled(sqrt_price, sqrt_price, divisor, &[fixed_scalar])?;

    price_from_u256(price_raw)
}

/// Converts a sqrt price X96 to a human-readable spot price adjusted for token decimals.
///
/// # Arguments
/// - `sqrt_price_x96` - The sqrt price in X96 format from the pool
/// - `token0_decimals` - Number of decimals for token0
/// - `token1_decimals` - Number of decimals for token1
/// - `invert` - If true, returns token0/token1; if false, returns token1/token0
///
/// # Pool Price Format
/// Uniswap V3 pools always store price as **token1/token0** where tokens are sorted by address.
///
/// # Errors
///
/// Returns an error if:
/// - `sqrt_price_x96` is zero and `invert` is true.
/// - A token decimal count exceeds `DECIMAL_EXPONENT_MAX` (77).
/// - The price calculation exceeds its supported wide-integer range.
/// - The result exceeds `PRICE_RAW_MAX`.
///
/// # Notes
///
/// Prices smaller than the fixed-point resolution are floored to
/// `Price::zero(FIXED_PRECISION)`.
pub fn decode_sqrt_price_x96_to_price_tokens_adjusted(
    sqrt_price_x96: U160,
    token0_decimals: u8,
    token1_decimals: u8,
    invert: bool,
) -> anyhow::Result<Price> {
    let sqrt_price = U256::from(sqrt_price_x96);
    let decimal_diff = i32::from(token0_decimals) - i32::from(token1_decimals);
    let token0_scalar = FullMath::pow10(token0_decimals)?;
    let token1_scalar = FullMath::pow10(token1_decimals)?;
    let decimal_adjustment = if decimal_diff >= 0 {
        token0_scalar / token1_scalar
    } else {
        token1_scalar / token0_scalar
    };
    let fixed_scalar = FullMath::pow10(FIXED_PRECISION)?;
    let divisor_base: U256 = U256::from(1u128) << 192;

    let price_raw = if invert {
        if decimal_diff >= 0 {
            let numerator = divisor_base
                .checked_mul(fixed_scalar)
                .ok_or_else(|| anyhow::anyhow!("Inverted price numerator exceeds U256 range"))?;
            let price_square: U512 = sqrt_price.widening_mul(sqrt_price);
            let max_square = U512::from(numerator / decimal_adjustment);

            if price_square > max_square {
                U256::ZERO
            } else {
                let price_square = U256::checked_from_limbs_slice(price_square.as_limbs())
                    .ok_or_else(|| {
                        anyhow::anyhow!("Inverted price denominator exceeds U256 range")
                    })?;
                let denominator =
                    price_square
                        .checked_mul(decimal_adjustment)
                        .ok_or_else(|| {
                            anyhow::anyhow!("Inverted price denominator exceeds U256 range")
                        })?;
                FullMath::mul_div(numerator, U256::from(1), denominator)?
            }
        } else {
            let price_square: U512 = sqrt_price.widening_mul(sqrt_price);
            anyhow::ensure!(
                !price_square.is_zero(),
                "Cannot decode inverted price from zero sqrt_price_x96"
            );
            let numerator = U512::from(divisor_base)
                .checked_mul(U512::from(decimal_adjustment))
                .and_then(|value| value.checked_mul(U512::from(fixed_scalar)))
                .ok_or_else(|| anyhow::anyhow!("Inverted price numerator exceeds U512 range"))?;
            let quotient = numerator / price_square;
            U256::checked_from_limbs_slice(quotient.as_limbs())
                .ok_or_else(|| anyhow::anyhow!("Inverted price exceeds U256 range"))?
        }
    } else if decimal_diff >= 0 {
        FullMath::mul_div_scaled(
            sqrt_price,
            sqrt_price,
            divisor_base,
            &[fixed_scalar, decimal_adjustment],
        )?
    } else {
        FullMath::mul_div_scaled(sqrt_price, sqrt_price, divisor_base, &[fixed_scalar])?
            / decimal_adjustment
    };

    price_from_u256(price_raw)
}

pub(crate) fn price_from_u256(price_raw: U256) -> anyhow::Result<Price> {
    anyhow::ensure!(
        price_raw <= U256::from(PRICE_RAW_MAX as u128),
        "Price overflow: {price_raw} exceeds maximum valid raw price {PRICE_RAW_MAX}"
    );
    let price_raw: i128 = price_raw
        .try_into()
        .map_err(|_| anyhow::anyhow!("Price overflow: {price_raw} exceeds PriceRaw range"))?;

    Price::from_raw_checked(price_raw, FIXED_PRECISION).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    // Most of the tests are based on https://github.com/Uniswap/v3-core/blob/main/test/SqrtPriceMath.spec.ts
    use rstest::*;

    use super::*;
    use crate::defi::tick_map::{
        full_math::{DECIMAL_EXPONENT_MAX, Q96_U160},
        tick_math::MAX_SQRT_RATIO,
    };

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
        let amount_out = U256::from(262_145);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, true);
    }

    #[rstest]
    #[should_panic(expected = "sqrt_price_x96 must be greater than quotient")]
    fn test_fails_if_output_amount_is_exactly_virtual_reserves_of_token1() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(262_144);
        let _ = get_next_sqrt_price_from_output(price, liquidity, amount_out, true);
    }

    #[rstest]
    fn test_succeeds_if_output_amount_is_just_less_than_virtual_reserves_of_token1() {
        let price = U160::from_str_radix("20282409603651670423947251286016", 10).unwrap();
        let liquidity = 1024;
        let amount_out = U256::from(262_143);
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
            U160::from(792_281_625_142_643_375_935_439_503_360_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(1, 100),
            U160::from(7_922_816_251_426_433_759_354_395_033_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(111, 333),
            U160::from(45_742_400_955_009_932_534_161_870_629_u128)
        );
        assert_eq!(
            encode_sqrt_ratio_x96(333, 111),
            U160::from(137_227_202_865_029_797_602_485_611_888_u128)
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

    #[rstest]
    fn test_decode_sqrt_price_x96_to_price_and_decimal_adjustments() {
        // Use values from https://blog.uniswap.org/uniswap-v3-math-primer
        let sqrt_price_x96 =
            U160::from_str_radix("2018382873588440326581633304624437", 10).unwrap();

        let raw_price = decode_sqrt_price_x96_to_price(sqrt_price_x96).unwrap();
        assert_eq!(raw_price.as_f64(), 649_004_842.701_37);

        // We want the adjusted price inverted as USDC is token0 and WETH is token1
        let adjusted_price =
            decode_sqrt_price_x96_to_price_tokens_adjusted(sqrt_price_x96, 6, 18, true).unwrap();
        assert_eq!(adjusted_price.as_f64(), 1_540.820_552_028_045_8);
    }

    #[rstest]
    #[case::normal_positive_difference(2, 0, false, 1_000_000_000_000_000_000)]
    #[case::normal_negative_difference(0, 2, false, 100_000_000_000_000)]
    #[case::inverted_positive_difference(2, 0, true, 100_000_000_000_000)]
    #[case::inverted_negative_difference(0, 2, true, 1_000_000_000_000_000_000)]
    fn test_decode_sqrt_price_x96_to_price_adjusts_direction_and_decimals_exactly(
        #[case] token0_decimals: u8,
        #[case] token1_decimals: u8,
        #[case] invert: bool,
        #[case] expected_raw: i128,
    ) {
        let result = decode_sqrt_price_x96_to_price_tokens_adjusted(
            Q96_U160,
            token0_decimals,
            token1_decimals,
            invert,
        )
        .unwrap();

        assert_eq!(result, Price::from_raw(expected_raw, FIXED_PRECISION));
    }

    #[rstest]
    fn test_decode_sqrt_price_x96_to_price_handles_max_ratio_without_wrapping() {
        let sqrt_price_x96 = MAX_SQRT_RATIO - U160::from(1);

        let raw_error = decode_sqrt_price_x96_to_price(sqrt_price_x96).unwrap_err();
        let normal_error =
            decode_sqrt_price_x96_to_price_tokens_adjusted(sqrt_price_x96, 0, 0, false)
                .unwrap_err();
        let inverted =
            decode_sqrt_price_x96_to_price_tokens_adjusted(sqrt_price_x96, 0, 0, true).unwrap();

        assert!(
            raw_error
                .to_string()
                .contains("exceeds maximum valid raw price")
        );
        assert!(
            normal_error
                .to_string()
                .contains("exceeds maximum valid raw price")
        );
        assert_eq!(inverted, Price::zero(FIXED_PRECISION));
    }

    #[rstest]
    fn test_decode_sqrt_price_x96_to_price_handles_inverted_denominator_boundary() {
        let sqrt_price_x96 = Q96_U160 * U160::from(100_000_000);

        let result =
            decode_sqrt_price_x96_to_price_tokens_adjusted(sqrt_price_x96, 0, 0, true).unwrap();

        assert_eq!(result, Price::from_raw(1, FIXED_PRECISION));
    }

    #[rstest]
    fn test_decode_sqrt_price_x96_to_price_handles_largest_decimal_exponent() {
        let valid = decode_sqrt_price_x96_to_price_tokens_adjusted(
            Q96_U160,
            0,
            DECIMAL_EXPONENT_MAX,
            false,
        )
        .unwrap();
        let normal_overflow = decode_sqrt_price_x96_to_price_tokens_adjusted(
            Q96_U160,
            DECIMAL_EXPONENT_MAX,
            0,
            false,
        )
        .unwrap_err();
        let inverted_overflow =
            decode_sqrt_price_x96_to_price_tokens_adjusted(Q96_U160, 0, DECIMAL_EXPONENT_MAX, true)
                .unwrap_err();

        assert_eq!(valid, Price::zero(FIXED_PRECISION));
        assert_eq!(
            normal_overflow.to_string(),
            "Scaled result exceeds 256-bit range"
        );
        assert_eq!(
            inverted_overflow.to_string(),
            "Inverted price exceeds U256 range"
        );
    }

    #[rstest]
    fn test_decode_sqrt_price_x96_to_price_rejects_first_unsupported_decimal_exponent() {
        let error = decode_sqrt_price_x96_to_price_tokens_adjusted(
            Q96_U160,
            0,
            DECIMAL_EXPONENT_MAX + 1,
            false,
        )
        .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Decimal exponent 78 exceeds supported maximum 77"
        );
    }
}
