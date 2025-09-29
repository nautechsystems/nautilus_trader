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

use alloy_primitives::{I256, U160, U256};

use crate::defi::tick_map::{
    full_math::FullMath,
    sqrt_price_math::{
        get_amount0_delta, get_amount1_delta, get_next_sqrt_price_from_input,
        get_next_sqrt_price_from_output,
    },
};

#[derive(Debug, Clone)]
pub struct SwapStepResult {
    pub sqrt_ratio_next_x96: U160,
    pub amount_in: U256,
    pub amount_out: U256,
    pub fee_amount: U256,
}

pub const MAX_FEE: U256 = U256::from_limbs([1000000, 0, 0, 0]);

/// Computes the result of swapping some amount in, or amount out, given the parameters of the swap.
///
/// # Arguments
///
/// - `sqrt_ratio_current_x96` - The current sqrt price of the pool
/// - `sqrt_ratio_target_x96` - The price that cannot be exceeded, from which the direction of the swap is inferred
/// - `liquidity` - The usable liquidity
/// - `amount_remaining` - How much input or output amount is remaining to be swapped in/out
/// - `fee_pips` - The fee taken from the input amount, expressed in hundredths of a bip
///
/// # Errors
///
/// This function returns an error if:
/// - Fee adjustment calculations overflow or encounter division by zero.
/// - Amount or price delta calculations overflow the supported numeric range.
pub fn compute_swap_step(
    sqrt_ratio_current_x96: U160,
    sqrt_ratio_target_x96: U160,
    liquidity: u128,
    amount_remaining: I256,
    fee_pips: u32,
) -> anyhow::Result<SwapStepResult> {
    let fee_pips = U256::from(fee_pips);
    let fee_complement = MAX_FEE - fee_pips;

    // Represent a direction of the swap, should we move price down (swap token0 for token1)
    // or price up (swap token1 for token0)
    let zero_for_one = sqrt_ratio_current_x96 >= sqrt_ratio_target_x96;

    // true = exact input swap (know input amount, calculate output)
    // false = exact output swap (know desired output, calculate required input)
    let exact_in = amount_remaining.is_positive() || amount_remaining.is_zero();

    let sqrt_ratio_next_x96: U160;
    let mut amount_in: U256 = U256::ZERO;
    let mut amount_out: U256 = U256::ZERO;

    if exact_in {
        // Calculate how much input is needed to reach target, considering fees
        let amount_remaining_less_fee =
            FullMath::mul_div(amount_remaining.into_raw(), fee_complement, MAX_FEE)?;

        amount_in = if zero_for_one {
            get_amount0_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                true,
            )
        } else {
            get_amount1_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                true,
            )
        };

        if amount_remaining_less_fee >= amount_in {
            sqrt_ratio_next_x96 = sqrt_ratio_target_x96;
        } else {
            sqrt_ratio_next_x96 = get_next_sqrt_price_from_input(
                sqrt_ratio_current_x96,
                liquidity,
                amount_remaining_less_fee,
                zero_for_one,
            );
        }
    } else {
        // Calculate how much output can be obtained to reach target
        amount_out = if zero_for_one {
            get_amount1_delta(
                sqrt_ratio_target_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )
        } else {
            get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_target_x96,
                liquidity,
                false,
            )
        };

        if U256::from(amount_remaining.unsigned_abs()) >= amount_out {
            sqrt_ratio_next_x96 = sqrt_ratio_target_x96;
        } else {
            sqrt_ratio_next_x96 = get_next_sqrt_price_from_output(
                sqrt_ratio_current_x96,
                liquidity,
                U256::from(amount_remaining.unsigned_abs()),
                zero_for_one,
            );
        }
    }

    let max = sqrt_ratio_target_x96 == sqrt_ratio_next_x96;

    // get the input/output amounts
    if zero_for_one {
        amount_in = if max && exact_in {
            amount_in
        } else {
            get_amount0_delta(sqrt_ratio_next_x96, sqrt_ratio_current_x96, liquidity, true)
        };
        amount_out = if max && !exact_in {
            amount_out
        } else {
            get_amount1_delta(
                sqrt_ratio_next_x96,
                sqrt_ratio_current_x96,
                liquidity,
                false,
            )
        };
    } else {
        amount_in = if max && exact_in {
            amount_in
        } else {
            get_amount1_delta(sqrt_ratio_current_x96, sqrt_ratio_next_x96, liquidity, true)
        };
        amount_out = if max && !exact_in {
            amount_out
        } else {
            get_amount0_delta(
                sqrt_ratio_current_x96,
                sqrt_ratio_next_x96,
                liquidity,
                false,
            )
        };
    }

    // cap the output amount to not exceed the remaining output amount
    if !exact_in && amount_out > U256::from(amount_remaining.unsigned_abs()) {
        amount_out = U256::from(amount_remaining.unsigned_abs());
    }

    let fee_amount: U256 = if exact_in && sqrt_ratio_next_x96 != sqrt_ratio_target_x96 {
        // we didn't reach the target, so take the remainder of the maximum input as fee
        U256::from(amount_remaining.unsigned_abs()) - amount_in
    } else {
        FullMath::mul_div_rounding_up(amount_in, fee_pips, fee_complement)?
    };

    Ok(SwapStepResult {
        sqrt_ratio_next_x96,
        amount_in,
        amount_out,
        fee_amount,
    })
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    // Most of the tests are from https://github.com/Uniswap/v3-core/blob/main/test/SwapMath.spec.ts
    use std::str::FromStr;

    use rstest::rstest;

    use super::*;
    use crate::defi::tick_map::sqrt_price_math::{encode_sqrt_ratio_x96, expand_to_18_decimals};

    #[rstest]
    fn test_exact_amount_in_that_gets_capped_at_price_target_in_one_for_zero() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let price_target = encode_sqrt_ratio_x96(101, 100);
        let liquidity = expand_to_18_decimals(2);
        let amount = expand_to_18_decimals(1);
        let fee = 600;

        let result = compute_swap_step(
            price,
            price_target,
            liquidity,
            I256::from_str(&amount.to_string()).unwrap(),
            fee,
        )
        .unwrap();

        assert_eq!(
            result.amount_in,
            U256::from_str("9975124224178055").unwrap()
        );
        assert_eq!(
            result.amount_out,
            U256::from_str("9925619580021728").unwrap()
        );
        assert_eq!(result.fee_amount, U256::from_str("5988667735148").unwrap());
        assert_eq!(result.sqrt_ratio_next_x96, price_target);

        // entire amount is not used
        assert!(result.amount_in + result.fee_amount < U256::from(amount));

        let price_after_whole_input_amount = get_next_sqrt_price_from_input(
            price,
            liquidity,
            U256::from(amount),
            false, // zero_for_one = false
        );

        // price is capped at price target
        assert_eq!(result.sqrt_ratio_next_x96, price_target);
        // price is less than price after whole input amount
        assert!(result.sqrt_ratio_next_x96 < price_after_whole_input_amount);
    }

    #[rstest]
    fn test_exact_amount_in_that_is_fully_spent_in_one_for_zero() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let price_target = encode_sqrt_ratio_x96(1000, 100);
        let liquidity = expand_to_18_decimals(2);
        let amount = expand_to_18_decimals(1);
        let fee = 600;

        let result = compute_swap_step(
            price,
            price_target,
            liquidity,
            I256::from_str(&amount.to_string()).unwrap(),
            fee,
        )
        .unwrap();

        assert_eq!(
            result.amount_in,
            U256::from_str("999400000000000000").unwrap()
        );
        assert_eq!(
            result.fee_amount,
            U256::from_str("600000000000000").unwrap()
        );
        assert_eq!(
            result.amount_out,
            U256::from_str("666399946655997866").unwrap()
        );

        // entire amount is used
        assert_eq!(result.amount_in + result.fee_amount, U256::from(amount));

        let price_after_whole_input_amount_less_fee = get_next_sqrt_price_from_input(
            price,
            liquidity,
            U256::from(amount) - result.fee_amount,
            false, // zero_for_one = false
        );

        // price does not reach price target
        assert!(result.sqrt_ratio_next_x96 < price_target);
        // price is equal to price after whole input amount
        assert_eq!(
            result.sqrt_ratio_next_x96,
            price_after_whole_input_amount_less_fee
        );
    }

    #[rstest]
    fn test_exact_amount_out_that_is_fully_received_in_one_for_zero() {
        let price = encode_sqrt_ratio_x96(1, 1);
        let price_target = encode_sqrt_ratio_x96(10000, 100);
        let liquidity = expand_to_18_decimals(2);
        let amount = expand_to_18_decimals(1);
        let fee = 600;

        // Negative amount for exact output
        let amount_negative = -I256::from_str(&amount.to_string()).unwrap();

        let result =
            compute_swap_step(price, price_target, liquidity, amount_negative, fee).unwrap();

        assert_eq!(
            result.amount_in,
            U256::from_str("2000000000000000000").unwrap()
        );
        assert_eq!(
            result.fee_amount,
            U256::from_str("1200720432259356").unwrap()
        );
        assert_eq!(result.amount_out, U256::from(amount));

        let price_after_whole_output_amount = get_next_sqrt_price_from_output(
            price,
            liquidity,
            U256::from(amount),
            false, // zero_for_one = false
        );

        // price does not reach price target
        assert!(result.sqrt_ratio_next_x96 < price_target);
        // price is equal to price after whole output amount
        assert_eq!(result.sqrt_ratio_next_x96, price_after_whole_output_amount);
    }

    #[rstest]
    fn test_amount_out_is_capped_at_the_desired_amount_out() {
        let result = compute_swap_step(
            U160::from_str("417332158212080721273783715441582").unwrap(),
            U160::from_str("1452870262520218020823638996").unwrap(),
            159344665391607089467575320103,
            I256::from_str("-1").unwrap(),
            1,
        )
        .unwrap();

        assert_eq!(result.amount_in, U256::from(1));
        assert_eq!(result.fee_amount, U256::from(1));
        assert_eq!(result.amount_out, U256::from(1)); // would be 2 if not capped
        assert_eq!(
            result.sqrt_ratio_next_x96,
            U160::from_str("417332158212080721273783715441581").unwrap()
        );
    }

    #[rstest]
    fn test_entire_input_amount_taken_as_fee() {
        let result = compute_swap_step(
            U160::from_str("2413").unwrap(),
            U160::from_str("79887613182836312").unwrap(),
            1985041575832132834610021537970,
            I256::from_str("10").unwrap(),
            1872,
        )
        .unwrap();

        assert_eq!(result.amount_in, U256::ZERO);
        assert_eq!(result.fee_amount, U256::from(10));
        assert_eq!(result.amount_out, U256::ZERO);
        assert_eq!(result.sqrt_ratio_next_x96, U160::from_str("2413").unwrap());
    }

    #[rstest]
    fn test_handles_intermediate_insufficient_liquidity_in_zero_for_one_exact_output_case() {
        let sqrt_p = U160::from_str("20282409603651670423947251286016").unwrap();
        // sqrtPTarget = sqrtP * 11 / 10
        let sqrt_p_target = U160::from(U256::from(sqrt_p) * U256::from(11) / U256::from(10));
        let liquidity = 1024;
        // virtual reserves of one are only 4
        let amount_remaining = I256::from_str("-4").unwrap();
        let fee_pips = 3000;

        let result =
            compute_swap_step(sqrt_p, sqrt_p_target, liquidity, amount_remaining, fee_pips)
                .unwrap();

        assert_eq!(result.amount_out, U256::ZERO);
        assert_eq!(result.sqrt_ratio_next_x96, sqrt_p_target);
        assert_eq!(result.amount_in, U256::from(26215));
        assert_eq!(result.fee_amount, U256::from(79));
    }

    #[rstest]
    fn test_handles_intermediate_insufficient_liquidity_in_one_for_zero_exact_output_case() {
        let sqrt_p = U160::from_str("20282409603651670423947251286016").unwrap();
        // sqrtPTarget = sqrtP * 9 / 10
        let sqrt_p_target = U160::from(U256::from(sqrt_p) * U256::from(9) / U256::from(10));
        let liquidity = 1024;
        // virtual reserves of zero are only 262144
        let amount_remaining = I256::from_str("-263000").unwrap();
        let fee_pips = 3000;

        let result =
            compute_swap_step(sqrt_p, sqrt_p_target, liquidity, amount_remaining, fee_pips)
                .unwrap();

        assert_eq!(result.amount_out, U256::from(26214));
        assert_eq!(result.sqrt_ratio_next_x96, sqrt_p_target);
        assert_eq!(result.amount_in, U256::from(1));
        assert_eq!(result.fee_amount, U256::from(1));
    }
}
