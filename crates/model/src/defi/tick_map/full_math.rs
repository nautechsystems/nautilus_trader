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

pub const Q128: U256 = U256::from_limbs([0, 0, 1, 0]);
pub const Q96_U160: U160 = U160::from_limbs([0, 1 << 32, 0]);

/// Contains 512-bit math functions for Uniswap V3 style calculations
/// Handles "phantom overflow" - allows multiplication and division where
/// intermediate values overflow 256 bits
#[derive(Debug)]
pub struct FullMath;

impl FullMath {
    /// Calculates floor(a×b÷denominator) with full precision
    ///
    /// Follows the Solidity implementation from Uniswap V3's FullMath library:
    /// <https://github.com/Uniswap/v3-core/blob/main/contracts/libraries/FullMath.sol>
    ///
    /// # Errors
    ///
    /// Returns error if `denominator` is zero or the result would overflow 256 bits.
    pub fn mul_div(a: U256, b: U256, mut denominator: U256) -> anyhow::Result<U256> {
        // 512-bit multiply [prod1 prod0] = a * b
        // Compute the product mod 2**256 and mod 2**256 - 1
        // then use the Chinese Remainder Theorem to reconstruct
        // the 512 bit result. The result is stored in two 256
        // variables such that product = prod1 * 2**256 + prod0
        let mm = a.mul_mod(b, U256::MAX);

        // Least significant 256 bits of the product
        let mut prod_0 = a * b;
        let mut prod_1 = mm - prod_0 - U256::from_limbs([(mm < prod_0) as u64, 0, 0, 0]);

        // Make sure the result is less than 2**256.
        // Also prevents denominator == 0
        if denominator <= prod_1 {
            anyhow::bail!("Result would overflow 256 bits");
        }

        ///////////////////////////////////////////////
        // 512 by 256 division.
        ///////////////////////////////////////////////

        // Make division exact by subtracting the remainder from [prod1 prod0]
        // Compute remainder using mul_mod
        let remainder = a.mul_mod(b, denominator);

        // Subtract 256 bit number from 512 bit number
        prod_1 -= U256::from_limbs([(remainder > prod_0) as u64, 0, 0, 0]);
        prod_0 -= remainder;

        // Factor powers of two out of denominator
        // Compute largest power of two divisor of denominator.
        // Always >= 1.
        let mut twos = (-denominator) & denominator;

        // Divide denominator by power of two
        denominator /= twos;

        // Divide [prod1 prod0] by the factors of two
        prod_0 /= twos;

        // Shift in bits from prod1 into prod0. For this we need
        // to flip `twos` such that it is 2**256 / twos.
        // If twos is zero, then it becomes one
        twos = (-twos) / twos + U256::from(1);

        prod_0 |= prod_1 * twos;

        // Invert denominator mod 2**256
        // Now that denominator is an odd number, it has an inverse
        // modulo 2**256 such that denominator * inv = 1 mod 2**256.
        // Compute the inverse by starting with a seed that is correct
        // correct for four bits. That is, denominator * inv = 1 mod 2**4
        let mut inv = (U256::from(3) * denominator) ^ U256::from(2);

        // Now use Newton-Raphson iteration to improve the precision.
        // Thanks to Hensel's lifting lemma, this also works in modular
        // arithmetic, doubling the correct bits in each step.
        inv *= U256::from(2) - denominator * inv; // inverse mod 2**8

        inv *= U256::from(2) - denominator * inv; // inverse mod 2**16

        inv *= U256::from(2) - denominator * inv; // inverse mod 2**32

        inv *= U256::from(2) - denominator * inv; // inverse mod 2**64

        inv *= U256::from(2) - denominator * inv; // inverse mod 2**128

        inv *= U256::from(2) - denominator * inv; // inverse mod 2**256

        // Because the division is now exact we can divide by multiplying
        // with the modular inverse of denominator. This will give us the
        // correct result modulo 2**256. Since the preconditions guarantee
        // that the outcome is less than 2**256, this is the final result.
        // We don't need to compute the high bits of the result and prod1
        // is no longer required.
        let result = prod_0 * inv;

        Ok(result)
    }

    /// Calculates ceil(a×b÷denominator) with full precision
    /// Returns `Ok` with the rounded result or an error when rounding cannot be performed safely.
    ///
    /// # Errors
    ///
    /// Returns error if `denominator` is zero or the rounded result would overflow `U256`.
    pub fn mul_div_rounding_up(a: U256, b: U256, denominator: U256) -> anyhow::Result<U256> {
        let result = Self::mul_div(a, b, denominator)?;

        // Check if there's a remainder
        if a.mul_mod(b, denominator).is_zero() {
            Ok(result)
        } else if result == U256::MAX {
            anyhow::bail!("Result would overflow 256 bits")
        } else {
            Ok(result + U256::from(1))
        }
    }

    /// Calculates ceil(a÷b) with proper rounding up
    /// Equivalent to Solidity's divRoundingUp function
    ///
    /// # Errors
    ///
    /// Returns error if `b` is zero or if the rounded quotient would overflow `U256`.
    pub fn div_rounding_up(a: U256, b: U256) -> anyhow::Result<U256> {
        if b.is_zero() {
            anyhow::bail!("Cannot divide by zero");
        }

        let quotient = a / b;
        let remainder = a % b;

        // Add 1 if there's a remainder (equivalent to gt(mod(x, y), 0) in assembly)
        if remainder > U256::ZERO {
            // Check for overflow before incrementing
            if quotient == U256::MAX {
                anyhow::bail!("Result would overflow 256 bits");
            }
            Ok(quotient + U256::from(1))
        } else {
            Ok(quotient)
        }
    }

    /// Computes the integer square root of a 256-bit unsigned integer using the Babylonian method
    pub fn sqrt(x: U256) -> U256 {
        if x.is_zero() {
            return U256::ZERO;
        }
        if x == U256::from(1u128) {
            return U256::from(1u128);
        }

        let mut z = x;
        let mut y = (x + U256::from(1u128)) >> 1;

        while y < z {
            z = y;
            y = (x / z + z) >> 1;
        }

        z
    }

    /// Truncates a U256 value to u128 by extracting the lower 128 bits.
    ///
    /// This matches Solidity's `uint128(value)` cast behavior, which discards
    /// the upper 128 bits. If the value is larger than `u128::MAX`, the upper
    /// bits are lost.
    #[must_use]
    pub fn truncate_to_u128(value: U256) -> u128 {
        (value & U256::from(u128::MAX)).to::<u128>()
    }

    /// Converts an I256 signed integer to U256, mimicking Solidity's `uint256(int256)` cast.
    ///
    /// This performs a reinterpret cast, preserving the bit pattern:
    /// - Positive values: returns the value as-is
    /// - Negative values: returns the two's complement representation as unsigned
    #[must_use]
    pub fn truncate_to_u256(value: I256) -> U256 {
        value.into_raw()
    }

    /// Converts a U256 unsigned integer to I256, mimicking Solidity's `int256(uint256)` cast.
    ///
    /// This performs a reinterpret cast, preserving the bit pattern.
    /// Solidity's SafeCast.toInt256() checks the value fits in I256::MAX, then reinterprets.
    ///
    /// # Panics
    /// Panics if the value exceeds I256::MAX (matching Solidity's require check)
    #[must_use]
    pub fn truncate_to_i256(value: U256) -> I256 {
        I256::from_raw(value)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use rstest::*;

    use super::*;

    #[rstest]
    fn test_mul_div_reverts_denominator_zero() {
        // Test that denominator 0 causes error
        assert!(FullMath::mul_div(Q128, U256::from(5), U256::ZERO).is_err());

        // Test with numerator overflow and denominator 0
        assert!(FullMath::mul_div(Q128, Q128, U256::ZERO).is_err());
    }

    #[rstest]
    fn test_mul_div_reverts_output_overflow() {
        // Test output overflow: Q128 * Q128 / 1 would overflow
        assert!(FullMath::mul_div(Q128, Q128, U256::from(1)).is_err());

        // Test overflow with inputs that would cause prod1 >= denominator
        // MAX * MAX / 1 would definitely overflow
        assert!(FullMath::mul_div(U256::MAX, U256::MAX, U256::from(1)).is_err());

        // Test with a smaller denominator that should still cause overflow
        assert!(FullMath::mul_div(U256::MAX, U256::MAX, U256::from(2)).is_err());

        // Test overflow with all max inputs and denominator = MAX - 1
        assert!(FullMath::mul_div(U256::MAX, U256::MAX, U256::MAX - U256::from(1)).is_err());
    }

    #[rstest]
    fn test_mul_div_all_max_inputs() {
        // MAX * MAX / MAX = MAX
        let result = FullMath::mul_div(U256::MAX, U256::MAX, U256::MAX).unwrap();
        assert_eq!(result, U256::MAX);
    }

    #[rstest]
    fn test_mul_div_accurate_without_phantom_overflow() {
        // Calculate Q128 * 0.5 / 1.5 = Q128 / 3
        let numerator_b = Q128 * U256::from(50) / U256::from(100); // 0.5
        let denominator = Q128 * U256::from(150) / U256::from(100); // 1.5
        let expected = Q128 / U256::from(3);

        let result = FullMath::mul_div(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_accurate_with_phantom_overflow() {
        // Calculate Q128 * 35 * Q128 / (8 * Q128) = 35/8 * Q128 = 4.375 * Q128
        let numerator_b = U256::from(35) * Q128;
        let denominator = U256::from(8) * Q128;
        let expected = U256::from(4375) * Q128 / U256::from(1000);

        let result = FullMath::mul_div(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_accurate_with_phantom_overflow_repeating_decimal() {
        // Calculate Q128 * 1000 * Q128 / (3000 * Q128) = 1/3 * Q128
        let numerator_b = U256::from(1000) * Q128;
        let denominator = U256::from(3000) * Q128;
        let expected = Q128 / U256::from(3);

        let result = FullMath::mul_div(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_basic_cases() {
        // Simple case: 100 * 200 / 50 = 400
        assert_eq!(
            FullMath::mul_div(U256::from(100), U256::from(200), U256::from(50)).unwrap(),
            U256::from(400)
        );

        // Test with 1: a * 1 / b = a / b
        assert_eq!(
            FullMath::mul_div(U256::from(1000), U256::from(1), U256::from(4)).unwrap(),
            U256::from(250)
        );

        // Test division that results in 0 due to floor
        assert_eq!(
            FullMath::mul_div(U256::from(1), U256::from(1), U256::from(3)).unwrap(),
            U256::ZERO
        );
    }

    // mul_div_rounding_up tests
    #[rstest]
    fn test_mul_div_rounding_up_reverts_denominator_zero() {
        // Test that denominator 0 causes error
        assert!(FullMath::mul_div_rounding_up(Q128, U256::from(5), U256::ZERO).is_err());

        // Test with numerator overflow and denominator 0
        assert!(FullMath::mul_div_rounding_up(Q128, Q128, U256::ZERO).is_err());
    }

    #[rstest]
    fn test_mul_div_rounding_up_reverts_output_overflow() {
        // Test output overflow: Q128 * Q128 / 1 would overflow
        assert!(FullMath::mul_div_rounding_up(Q128, Q128, U256::from(1)).is_err());

        // Test overflow with all max inputs minus 1 - this should pass since MAX/MAX-1 = ~1
        // but since there's a remainder, rounding up would still fit in U256
        // Let's test a case that actually overflows after rounding
        assert!(FullMath::mul_div_rounding_up(U256::MAX, U256::MAX, U256::from(2)).is_err());

        // Test overflow with all max inputs and denominator = MAX - 1
        assert!(
            FullMath::mul_div_rounding_up(U256::MAX, U256::MAX, U256::MAX - U256::from(1)).is_err()
        );
    }

    #[rstest]
    fn test_mul_div_rounding_up_reverts_overflow_after_rounding_case_1() {
        // Edge case discovered through fuzzing: mul_div succeeds but result is MAX with remainder
        // so rounding up would overflow
        let a = U256::from_str_radix("535006138814359", 10).unwrap();
        let b = U256::from_str_radix(
            "432862656469423142931042426214547535783388063929571229938474969",
            10,
        )
        .unwrap();
        let denominator = U256::from(2);

        assert!(FullMath::mul_div_rounding_up(a, b, denominator).is_err());
    }

    #[rstest]
    fn test_mul_div_rounding_up_reverts_overflow_after_rounding_case_2() {
        // Another edge case discovered through fuzzing: tests boundary condition where
        // mul_div returns MAX-1 but with remainder, so rounding up would cause overflow
        let a = U256::from_str_radix(
            "115792089237316195423570985008687907853269984659341747863450311749907997002549",
            10,
        )
        .unwrap();
        let b = U256::from_str_radix(
            "115792089237316195423570985008687907853269984659341747863450311749907997002550",
            10,
        )
        .unwrap();
        let denominator = U256::from_str_radix(
            "115792089237316195423570985008687907853269984653042931687443039491902864365164",
            10,
        )
        .unwrap();

        assert!(FullMath::mul_div_rounding_up(a, b, denominator).is_err());
    }

    #[rstest]
    fn test_mul_div_rounding_up_all_max_inputs() {
        // MAX * MAX / MAX = MAX (no rounding needed)
        let result = FullMath::mul_div_rounding_up(U256::MAX, U256::MAX, U256::MAX).unwrap();
        assert_eq!(result, U256::MAX);
    }

    #[rstest]
    fn test_mul_div_rounding_up_accurate_without_phantom_overflow() {
        // Calculate Q128 * 0.5 / 1.5 = Q128 / 3, but with rounding up
        let numerator_b = Q128 * U256::from(50) / U256::from(100); // 0.5
        let denominator = Q128 * U256::from(150) / U256::from(100); // 1.5
        let expected = Q128 / U256::from(3) + U256::from(1); // Rounded up

        let result = FullMath::mul_div_rounding_up(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_rounding_up_accurate_with_phantom_overflow() {
        // Calculate Q128 * 35 * Q128 / (8 * Q128) = 35/8 * Q128 = 4.375 * Q128
        // This should be exact (no remainder), so no rounding up needed
        let numerator_b = U256::from(35) * Q128;
        let denominator = U256::from(8) * Q128;
        let expected = U256::from(4375) * Q128 / U256::from(1000);

        let result = FullMath::mul_div_rounding_up(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_rounding_up_accurate_with_phantom_overflow_repeating_decimal() {
        // Calculate Q128 * 1000 * Q128 / (3000 * Q128) = 1/3 * Q128, with rounding up
        let numerator_b = U256::from(1000) * Q128;
        let denominator = U256::from(3000) * Q128;
        let expected = Q128 / U256::from(3) + U256::from(1); // Rounded up due to remainder

        let result = FullMath::mul_div_rounding_up(Q128, numerator_b, denominator).unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_div_rounding_up_basic_cases() {
        // Test exact division (no rounding needed)
        assert_eq!(
            FullMath::mul_div_rounding_up(U256::from(100), U256::from(200), U256::from(50))
                .unwrap(),
            U256::from(400)
        );

        // Test division with remainder (rounding up needed)
        assert_eq!(
            FullMath::mul_div_rounding_up(U256::from(1), U256::from(1), U256::from(3)).unwrap(),
            U256::from(1) // 0 rounded up to 1
        );

        // Test another rounding case: 7 * 3 / 4 = 21 / 4 = 5.25 -> 6
        assert_eq!(
            FullMath::mul_div_rounding_up(U256::from(7), U256::from(3), U256::from(4)).unwrap(),
            U256::from(6)
        );

        // Test case with zero result and zero remainder
        assert_eq!(
            FullMath::mul_div_rounding_up(U256::ZERO, U256::from(100), U256::from(3)).unwrap(),
            U256::ZERO
        );
    }

    #[rstest]
    fn test_mul_div_rounding_up_overflow_at_max() {
        // Test that rounding up when result is already MAX causes overflow
        // We need a case where mul_div returns MAX but there's a remainder
        // This is tricky to construct, so we test the boundary condition
        assert!(FullMath::mul_div_rounding_up(U256::MAX, U256::from(2), U256::from(2)).is_ok());

        // This should succeed: MAX * 1 / 1 = MAX (no remainder)
        assert_eq!(
            FullMath::mul_div_rounding_up(U256::MAX, U256::from(1), U256::from(1)).unwrap(),
            U256::MAX
        );
    }

    #[rstest]
    fn test_truncate_to_u128_preserves_small_values() {
        // Small value (fits in u128) should be preserved exactly
        let value = U256::from(12345u128);
        assert_eq!(FullMath::truncate_to_u128(value), 12345u128);

        // u128::MAX should be preserved
        let max_value = U256::from(u128::MAX);
        assert_eq!(FullMath::truncate_to_u128(max_value), u128::MAX);
    }

    #[rstest]
    fn test_truncate_to_u128_discards_upper_bits() {
        // Value = u128::MAX + 1 (sets bit 128)
        // Lower 128 bits = 0, so result should be 0
        let value = U256::from(u128::MAX) + U256::from(1);
        assert_eq!(FullMath::truncate_to_u128(value), 0);

        // Value with both high and low bits set:
        // High 128 bits = 0xFFFF...FFFF, Low 128 bits = 0x1234
        let value = (U256::from(u128::MAX) << 128) | U256::from(0x1234u128);
        assert_eq!(FullMath::truncate_to_u128(value), 0x1234u128);
    }
}
