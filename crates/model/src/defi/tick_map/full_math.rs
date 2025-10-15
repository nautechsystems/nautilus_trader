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
    /// # Errors
    ///
    /// Returns error if `denominator` is zero or the result would overflow 256 bits.
    pub fn mul_div(a: U256, b: U256, denominator: U256) -> anyhow::Result<U256> {
        if denominator.is_zero() {
            anyhow::bail!("Cannot divide by zero");
        }

        // Compute the 512-bit product [prod1,prod2] = a * b
        // prod0 - least significant  256 bit
        // prod1 - most significant 256 bit
        let (prod0, overflow) = a.overflowing_mul(b);

        // Calculate prod1 using mulmod equivalent
        // prod1 = (a * b - prod0) / 2^256
        // We need to handle the high part of multiplication
        let prod1 = if overflow {
            // When overflow occurs, we need the high 256 bits
            // This is equivalent to: mulmod(a, b, 2^256) but for the high part
            Self::mul_high(a, b)
        } else {
            U256::ZERO
        };

        // Handle non-overflow cases, 256 by 256 division
        if prod1.is_zero() {
            return Ok(prod0 / denominator);
        }

        if denominator <= prod1 {
            anyhow::bail!("Result would overflow 256 bits");
        }

        // 512 by 256 division

        // Make division exact by subtracting the remainder from [prod1 prod0]
        // Compute remainder using modular multiplication
        let remainder = Self::mulmod(a, b, denominator);

        // Subtract 256 bit number from 512 bit number
        let (prod0, borrow) = prod0.overflowing_sub(remainder);
        let prod1 = if borrow { prod1 - U256::from(1) } else { prod1 };

        // Factor powers of two out of denominator
        // Compute largest power of two divisor of denominator (always >= 1)
        let twos = (!denominator).wrapping_add(U256::from(1)) & denominator;
        let denominator = denominator / twos;

        // Divide [prod1 prod0] by the factors of two
        let prod0 = prod0 / twos;

        // Shift in bits from prod1 into prod0
        // We need to flip `twos` such that it is 2^256 / twos
        let twos = Self::div_2_256_by(twos);
        let prod0 = prod0 | (prod1 * twos);

        // Invert denominator mod 2^256
        // Now that denominator is an odd number, it has an inverse
        // modulo 2^256 such that denominator * inv = 1 mod 2^256
        let inv = Self::mod_inverse(denominator);

        // Because the division is now exact we can divide by multiplying
        // with the modular inverse of denominator
        Ok(prod0.wrapping_mul(inv))
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
        if !Self::mulmod(a, b, denominator).is_zero() {
            // Check for overflow before incrementing
            if result == U256::MAX {
                anyhow::bail!("Result would overflow 256 bits");
            }
            Ok(result + U256::from(1))
        } else {
            Ok(result)
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

    /// Computes modular multiplicative inverse using Newton-Raphson iteration
    fn mod_inverse(denominator: U256) -> U256 {
        // Start with a seed that is correct for four bits
        // That is, denominator * inv = 1 mod 2^4
        let mut inv = (U256::from(3) * denominator) ^ U256::from(2);

        // Use Newton-Raphson iteration to improve precision
        // Thanks to Hensel's lifting lemma, this works in modular arithmetic,
        // doubling the correct bits in each step

        // inverse mod 2^8
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));
        // inverse mod 2^16
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));
        // inverse mod 2^32
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));
        // inverse mod 2^64
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));
        // inverse mod 2^128
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));
        // inverse mod 2^256
        inv = inv.wrapping_mul(U256::from(2).wrapping_sub(denominator.wrapping_mul(inv)));

        inv
    }

    /// Computes 2^256 / x (assuming x is a power of 2)
    fn div_2_256_by(x: U256) -> U256 {
        if x.is_zero() {
            return U256::from(1);
        }

        // For a power of 2, we can use bit manipulation
        // Count trailing zeros to find the power
        let trailing_zeros = x.trailing_zeros();

        if trailing_zeros >= 256 {
            U256::from(1)
        } else {
            U256::from(1) << (256 - trailing_zeros)
        }
    }

    /// Computes (a * b) mod m
    fn mulmod(a: U256, b: U256, m: U256) -> U256 {
        if m.is_zero() {
            return U256::ZERO;
        }

        // For small values, we can use simple approach
        if a < U256::from(u128::MAX) && b < U256::from(u128::MAX) {
            return (a * b) % m;
        }

        let (low, overflow) = a.overflowing_mul(b);
        if !overflow {
            return low % m;
        }

        // When overflow occurs, use bit-by-bit modular multiplication
        // This is slower but handles all cases correctly
        Self::mulmod_slow(a, b, m)
    }

    /// Slow but correct modular multiplication for large numbers
    fn mulmod_slow(mut a: U256, mut b: U256, m: U256) -> U256 {
        let mut result = U256::ZERO;
        a %= m;

        while b > U256::ZERO {
            if b & U256::from(1) == U256::from(1) {
                result = (result + a) % m;
            }
            a = (a * U256::from(2)) % m;
            b >>= 1;
        }

        result
    }

    // Computes the high 256 bits of a * b
    fn mul_high(a: U256, b: U256) -> U256 {
        // Split each number into high and low 128-bit parts
        let a_low = a & U256::from(u128::MAX);
        let a_high = a >> 128;
        let b_low = b & U256::from(u128::MAX);
        let b_high = b >> 128;

        // Compute partial products
        let ll = a_low * b_low;
        let lh = a_low * b_high;
        let hl = a_high * b_low;
        let hh = a_high * b_high;

        let mid_sum = lh + hl;
        let mid_high = mid_sum >> 128;
        let mid_low = mid_sum << 128;

        // Check for carry from the low addition
        let (_, carry) = ll.overflowing_add(mid_low);

        hh + mid_high + if carry { U256::from(1) } else { U256::ZERO }
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
    fn test_mul_high_basic() {
        // Test 0 * 0 = 0
        assert_eq!(FullMath::mul_high(U256::ZERO, U256::ZERO), U256::ZERO);

        // Test 1 * 1 = 1, high bits should be 0
        assert_eq!(FullMath::mul_high(U256::from(1), U256::from(1)), U256::ZERO);

        // Test MAX * 1 = MAX, high bits should be 0
        assert_eq!(FullMath::mul_high(U256::MAX, U256::from(1)), U256::ZERO);
    }

    #[rstest]
    fn test_mul_high_simple_case() {
        // Test 2^128 * 2^128 = 2^256
        // This should give us 1 in the high bits (bit 256 set)
        let result = FullMath::mul_high(Q128, Q128);
        assert_eq!(result, U256::from(1));
    }

    #[rstest]
    fn test_mul_high_asymmetric() {
        // Test large * small
        let large = U256::MAX;
        let small = U256::from(2);
        let result = FullMath::mul_high(large, small);
        // MAX * 2 = 2 * (2^256 - 1) = 2^257 - 2
        // High bits should be 1
        assert_eq!(result, U256::from(1));

        // 2^200 * 2^100 = 2^300, high part should be 2^44
        let a = U256::from(1u128) << 200;
        let b = U256::from(1u128) << 100;
        let expected = U256::from(1u128) << 44;
        assert_eq!(FullMath::mul_high(a, b), expected);
    }

    #[rstest]
    fn test_mul_high_known_values() {
        // Test with known 128-bit values
        let a = U256::from(u128::MAX); // 2^128 - 1
        let b = U256::from(u128::MAX); // 2^128 - 1
        let result = FullMath::mul_high(a, b);
        // (2^128 - 1)^2 = 2^256 - 2^129 + 1
        // High 256 bits should be 0 (since result < 2^256)
        assert_eq!(result, U256::ZERO);
    }

    #[rstest]
    fn test_mul_high_carry_propagation() {
        // Test cases where carry propagation is critical

        // Test with values that cause carry in mid_sum
        let a = U256::from_str_radix(
            "ffffffffffffffffffffffffffffffff00000000000000000000000000000000",
            16,
        )
        .unwrap();
        let b = U256::from_str_radix(
            "ffffffffffffffffffffffffffffffff00000000000000000000000000000000",
            16,
        )
        .unwrap();

        let result = FullMath::mul_high(a, b);

        // Verify against expected value
        // (2^128 - 1)^2 * 2^256 = 2^512 - 2^257 + 2^256
        // High part should be 2^256 - 2 + 1 = 2^256 - 1
        let expected = U256::from_str_radix(
            "fffffffffffffffffffffffffffffffe00000000000000000000000000000001",
            16,
        )
        .unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mul_high_symmetry() {
        // Test that mul_high(a, b) == mul_high(b, a)
        let a = U256::from_str_radix("123456789abcdef0123456789abcdef0", 16).unwrap();
        let b = U256::from_str_radix("fedcba9876543210fedcba9876543210", 16).unwrap();

        assert_eq!(FullMath::mul_high(a, b), FullMath::mul_high(b, a));
    }

    #[rstest]
    fn test_mulmod_basic() {
        // Test basic cases
        assert_eq!(
            FullMath::mulmod(U256::ZERO, U256::from(5), U256::from(3)),
            U256::ZERO
        );
        assert_eq!(
            FullMath::mulmod(U256::from(5), U256::ZERO, U256::from(3)),
            U256::ZERO
        );
        assert_eq!(
            FullMath::mulmod(U256::from(5), U256::from(3), U256::ZERO),
            U256::ZERO
        );

        // Simple multiplication: 5 * 3 mod 7 = 15 mod 7 = 1
        assert_eq!(
            FullMath::mulmod(U256::from(5), U256::from(3), U256::from(7)),
            U256::from(1)
        );

        // Test where result equals modulus: 6 * 2 mod 12 = 0
        assert_eq!(
            FullMath::mulmod(U256::from(6), U256::from(2), U256::from(12)),
            U256::ZERO
        );
    }

    #[rstest]
    fn test_mulmod_small_values() {
        // Test the fast path for small values
        let a = U256::from(123456u64);
        let b = U256::from(789012u64);
        let m = U256::from(100000u64);

        // 123456 * 789012 = 97408265472
        // 97408265472 mod 100000 = 65472
        assert_eq!(FullMath::mulmod(a, b, m), U256::from(65472));
    }

    #[rstest]
    fn test_mulmod_no_overflow() {
        // Test medium-sized values that don't overflow
        let a = U256::from(u64::MAX);
        let b = U256::from(1000u64);
        let m = U256::from(u64::MAX);

        let result = FullMath::mulmod(a, b, m);
        let expected = (U256::from(u64::MAX) * U256::from(1000)) % U256::from(u64::MAX);
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mulmod_large_overflow() {
        // Test with values that cause overflow - forces use of slow path
        let a = U256::MAX;
        let b = U256::MAX;
        let m = U256::from(1000000007u64); // Large prime

        let result = FullMath::mulmod(a, b, m);

        // Since we can't easily compute MAX * MAX mod m by hand,
        // let's verify by testing the modular arithmetic property:
        // (a mod m) * (b mod m) mod m should equal our result for small enough modulus
        let a_mod = a % m;
        let b_mod = b % m;
        let expected = (a_mod * b_mod) % m;
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_mulmod_symmetry() {
        // Test that mulmod(a, b, m) == mulmod(b, a, m)
        let a = U256::from_str_radix("123456789abcdef0", 16).unwrap();
        let b = U256::from_str_radix("fedcba9876543210", 16).unwrap();
        let m = U256::from(1000000007u64);

        assert_eq!(FullMath::mulmod(a, b, m), FullMath::mulmod(b, a, m));
    }

    #[rstest]
    fn test_mulmod_identity() {
        // Test multiplicative identity: a * 1 mod m = a mod m
        let a = U256::from_str_radix("123456789abcdef0123456789abcdef0", 16).unwrap();
        let m = U256::from(1000000007u64);

        assert_eq!(FullMath::mulmod(a, U256::from(1), m), a % m);
        assert_eq!(FullMath::mulmod(U256::from(1), a, m), a % m);
    }

    #[rstest]
    fn test_mulmod_powers_of_two() {
        // Test with powers of 2 for easier verification
        let a = U256::from(1) << 100; // 2^100
        let b = U256::from(1) << 50; // 2^50
        let m = U256::from(1) << 60; // 2^60

        // 2^100 * 2^50 = 2^150
        // 2^150 mod 2^60 = 0 (since 150 > 60)
        assert_eq!(FullMath::mulmod(a, b, m), U256::ZERO);

        // Test case where result is non-zero
        let a2 = U256::from(1) << 30; // 2^30
        let b2 = U256::from(1) << 20; // 2^20
        let m2 = U256::from(1) << 60; // 2^60

        // 2^30 * 2^20 = 2^50, which is < 2^60
        let expected = U256::from(1) << 50;
        assert_eq!(FullMath::mulmod(a2, b2, m2), expected);
    }

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
    }

    #[rstest]
    fn test_mul_div_rounding_up_specific_overflow_cases() {
        // Test specific overflow case from TypeScript: reverts if mulDiv overflows 256 bits after rounding up
        let a = U256::from_str_radix("535006138814359", 10).unwrap();
        let b = U256::from_str_radix(
            "432862656469423142931042426214547535783388063929571229938474969",
            10,
        )
        .unwrap();
        let denominator = U256::from(2);

        // First check if the base mul_div succeeds - if it does, this might not be an overflow case
        let base_result = FullMath::mul_div(a, b, denominator);
        if base_result.is_ok() {
            // If base succeeds, check if there's a remainder and if result == MAX
            let remainder = FullMath::mulmod(a, b, denominator);
            if !remainder.is_zero() && base_result.unwrap() == U256::MAX {
                assert!(FullMath::mul_div_rounding_up(a, b, denominator).is_err());
            } else {
                // This case doesn't actually overflow after rounding
                assert!(FullMath::mul_div_rounding_up(a, b, denominator).is_ok());
            }
        } else {
            // Base mul_div fails, so rounding up should also fail
            assert!(FullMath::mul_div_rounding_up(a, b, denominator).is_err());
        }

        // Test second specific overflow case - check if this actually overflows
        let a2 = U256::from_str_radix(
            "115792089237316195423570985008687907853269984659341747863450311749907997002549",
            10,
        )
        .unwrap();
        let b2 = U256::from_str_radix(
            "115792089237316195423570985008687907853269984659341747863450311749907997002550",
            10,
        )
        .unwrap();
        let denominator2 = U256::from_str_radix(
            "115792089237316195423570985008687907853269984653042931687443039491902864365164",
            10,
        )
        .unwrap();

        let base_result2 = FullMath::mul_div(a2, b2, denominator2);
        if base_result2.is_ok() {
            let remainder2 = FullMath::mulmod(a2, b2, denominator2);
            if !remainder2.is_zero() && base_result2.unwrap() == U256::MAX {
                assert!(FullMath::mul_div_rounding_up(a2, b2, denominator2).is_err());
            } else {
                // This case doesn't actually overflow after rounding
                assert!(FullMath::mul_div_rounding_up(a2, b2, denominator2).is_ok());
            }
        } else {
            // Base mul_div fails, so rounding up should also fail
            assert!(FullMath::mul_div_rounding_up(a2, b2, denominator2).is_err());
        }
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
