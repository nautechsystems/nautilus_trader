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

//! Mathematical utilities for blockchain value conversion.
//!
//! This module provides functions for converting large integer types (U256, I256)
//! used in blockchain applications to floating-point values, accounting for
//! token decimal places and precision requirements.
//!
//! The implementation prioritizes safety and precision by:
//! - Never converting to f64 before scaling
//! - Using pure integer math and string operations for exact decimal representation
//! - Providing guarded f64 conversion that refuses unsafe conversions
//! - Implementing explicit rounding with proper carry handling

use alloy::primitives::{I256, U256};
use anyhow::{Result, anyhow, bail};

/// Largest integer exactly representable in an IEEE-754 f64.
const MAX_SAFE_INT: u64 = 9_007_199_254_740_991; // 2^53 - 1

/// Conservative total significant digits we'll aim to keep in f64.
const MAX_SIG_DIGITS: usize = 15;

/// Maximum decimals s.t. 10^decimals fits in 256 bits.
/// floor(log10(2^256-1)) = 77
const MAX_DECIMALS_FIT: u32 = 77;

/// Compute 10^d as U256 (d <= 77).
fn pow10_u256(d: u32) -> Result<U256> {
    if d > MAX_DECIMALS_FIT {
        bail!("decimals={} exceeds 10^d capacity for U256", d);
    }
    let mut p = U256::from(1u8);
    if d == 0 {
        return Ok(p);
    }
    let ten = U256::from(10u8);
    for _ in 0..d {
        p *= ten;
    }
    Ok(p)
}

/// Split `amount / 10^decimals` into (integer_part, fractional_digits_string with length==decimals).
/// This is exact and uses only integer arithmetic + zero-padding.
fn u256_scaled_parts(amount: U256, decimals: u32) -> Result<(U256, String)> {
    if decimals == 0 {
        return Ok((amount, String::new()));
    }
    let denom = pow10_u256(decimals)?;
    let int_part = amount / denom;
    let frac = amount % denom;

    // Render remainder as decimal, left-pad with zeros to length `decimals`.
    let mut frac_str = frac.to_string();
    let d = decimals as usize;
    if frac_str.len() < d {
        let mut s = String::with_capacity(d);
        for _ in 0..(d - frac_str.len()) {
            s.push('0');
        }
        s.push_str(&frac_str);
        frac_str = s;
    }
    debug_assert_eq!(frac_str.len(), d);
    Ok((int_part, frac_str))
}

/// Exact, human-friendly decimal string without floating point.
///
/// # Examples
/// - amount=1_000_000_000_000_000_000, decimals=18 -> "1"
/// - amount=12345, decimals=6 -> "0.012345"
///
/// # Errors
///
/// Returns an error if the decimals parameter exceeds the maximum supported value.
pub fn u256_to_decimal_string(amount: U256, decimals: u32) -> Result<String> {
    let (int_part, mut frac_str) = u256_scaled_parts(amount, decimals)?;
    if decimals == 0 {
        return Ok(int_part.to_string());
    }
    // Trim trailing zeros in the fractional part; drop '.' if becomes empty.
    while frac_str.ends_with('0') {
        frac_str.pop();
    }
    if frac_str.is_empty() {
        Ok(int_part.to_string())
    } else {
        Ok(format!("{}.{}", int_part, frac_str))
    }
}

/// Lossy but *guarded* conversion to f64:
/// - Never converts to f64 before scaling
/// - Refuses when integer part > 2^53-1
/// - Caps total significant digits (~15), rounding fractional digits (half-up) to fit
///
///   Use this only at the edge (e.g., UI or an external API that demands f64).
///
/// # Errors
///
/// Returns an error if:
/// - The integer part after scaling exceeds 2^53-1 (max safe integer for f64)
/// - Integer overflow occurs during rounding calculations
/// - The decimal value exceeds the maximum supported precision
pub fn convert_u256_to_f64_checked(amount: U256, decimals: u32) -> Result<f64> {
    // 1) Split scaled value exactly.
    let (int_part_u256, mut frac_str) = u256_scaled_parts(amount, decimals)?;

    // 2) Bound the integer part for exact f64 representation.
    let int_part_u64 = if int_part_u256 > U256::from(MAX_SAFE_INT) {
        bail!(
            "integer part {} exceeds f64 exact range (2^53-1). Refuse lossy conversion.",
            int_part_u256
        );
    } else {
        // Safe: int_part_u256 <= MAX_SAFE_INT
        int_part_u256.to::<u64>()
    };

    // 3) If there is no fractional part, we're done.
    if frac_str.is_empty() {
        return Ok(int_part_u64 as f64);
    }

    // Remove trailing zeros (no information content).
    while frac_str.ends_with('0') {
        frac_str.pop();
    }

    // 4) Decide how many fractional digits we can keep given MAX_SIG_DIGITS.
    let int_digits = {
        // Using decimal length of the integer part.
        // int_part_u64 is small enough to format cheaply.
        if int_part_u64 == 0 {
            1
        } else {
            (int_part_u64 as f64).log10().floor() as usize + 1
        }
    };
    let keep_frac = if MAX_SIG_DIGITS > int_digits {
        let budget = MAX_SIG_DIGITS - int_digits;
        core::cmp::min(budget, frac_str.len())
    } else {
        0
    };

    if keep_frac == 0 {
        // No fractional precision left; integer is exact in f64.
        return Ok(int_part_u64 as f64);
    }

    // 5) Round the fractional digits to `keep_frac` (decimal half-up).
    if frac_str.len() > keep_frac {
        // Look at the next digit for rounding.
        let next_digit = frac_str.as_bytes()[keep_frac];
        // Truncate to keep_frac first.
        frac_str.truncate(keep_frac);

        if next_digit >= b'5' {
            // Carry into the truncated fraction using safe string operations.
            let mut chars: Vec<char> = frac_str.chars().collect();
            let mut i = chars.len();
            let mut carry = true;
            while carry && i > 0 {
                i -= 1;
                if chars[i] == '9' {
                    chars[i] = '0';
                } else {
                    chars[i] = char::from(chars[i] as u8 + 1); // increment digit
                    carry = false;
                }
            }
            frac_str = chars.into_iter().collect();

            if carry {
                // Fraction overflowed, so increment the integer part.
                let new_int = int_part_u64
                    .checked_add(1)
                    .ok_or_else(|| anyhow!("integer carry overflow during rounding"))?;
                if new_int > MAX_SAFE_INT {
                    bail!("rounded integer {} exceeds f64 exact range.", new_int);
                }
                // Update integer part and reset fractional part to all zeros.
                // Since all zeros add nothing, we can drop them entirely.
                return Ok(new_int as f64);
            }
        }
    }

    // 6) Build f64 as: int + (frac_int / 10^keep_frac), both as f64.
    //    keep_frac ≤ 15 → frac_int fits in u64.
    let frac_int = if frac_str.is_empty() {
        0u64
    } else {
        frac_str.parse::<u64>()?
    };
    let frac = (frac_int as f64) / 10f64.powi(keep_frac as i32);

    Ok((int_part_u64 as f64) + frac)
}

/// Convert an alloy's U256 value to f64, accounting for token decimals.
///
/// # Errors
///
/// Returns an error if the U256 value cannot be safely converted to f64.
/// This function is deprecated - prefer `convert_u256_to_f64_checked` for safety.
pub fn convert_u256_to_f64(amount: U256, decimals: u8) -> anyhow::Result<f64> {
    convert_u256_to_f64_checked(amount, decimals as u32)
}

/// Convert an alloy's I256 value to f64, accounting for token decimals.
///
/// # Errors
///
/// Returns an error if the I256 value cannot be safely converted to f64.
pub fn convert_i256_to_f64(amount: I256, decimals: u8) -> anyhow::Result<f64> {
    // Handle the sign separately
    let is_negative = amount.is_negative();
    let abs_amount = if is_negative { -amount } else { amount };

    // Convert to U256 for processing
    let abs_u256 = U256::from_be_bytes(abs_amount.to_be_bytes::<32>());

    // Use the safe U256 conversion
    let abs_result = convert_u256_to_f64_checked(abs_u256, decimals as u32)?;

    // Apply sign
    Ok(if is_negative { -abs_result } else { abs_result })
}

#[cfg(test)]
mod tests {
    use super::*;
    use core::str::FromStr;

    fn u256_dec(s: &str) -> U256 {
        U256::from_str(s).unwrap()
    }

    #[test]
    fn decimal_string_one_token_18() {
        // 1e18 -> "1"
        let s = u256_to_decimal_string(u256_dec("1000000000000000000"), 18).unwrap();
        assert_eq!(s, "1");
    }

    #[test]
    fn decimal_string_small_fraction() {
        // 12345 with 6 decimals -> "0.012345"
        let s = u256_to_decimal_string(U256::from(12345u32), 6).unwrap();
        assert_eq!(s, "0.012345");
    }

    #[test]
    fn decimal_string_trim_trailing_zeros() {
        // 120000 with 5 decimals -> "1.2"
        let s = u256_to_decimal_string(U256::from(120000u32), 5).unwrap();
        assert_eq!(s, "1.2");
    }

    #[test]
    fn f64_guard_blocks_huge_integer_part() {
        // integer part = 2^53 -> must refuse
        let amt = u256_dec("9007199254740992"); // 2^53
        let err = convert_u256_to_f64_checked(amt, 0).unwrap_err();
        assert!(err.to_string().contains("exceeds f64 exact range"));
    }

    #[test]
    fn f64_guard_allows_max_safe_integer() {
        let amt = u256_dec("9007199254740991"); // 2^53 - 1
        let v = convert_u256_to_f64_checked(amt, 0).unwrap();
        assert_eq!(v, 9007199254740991.0);
    }

    #[test]
    fn rounding_fraction_no_carry() {
        // int=1234567890123, frac=456 -> keep 2 decimals after budgeting
        let amt = u256_dec("1234567890123456"); // decimals=3 -> 1_234_567_890_123.456
        let v = convert_u256_to_f64_checked(amt, 3).unwrap();
        // The integer part has 13 digits, so we can keep 2 fractional digits (15-13=2)
        // This should round to 1_234_567_890_123.46 (456 rounded to 2 digits = 46)
        let expected = 1_234_567_890_123.46;
        assert!(
            (v - expected).abs() < 1e-10,
            "Expected {}, got {}",
            expected,
            v
        );
    }

    #[test]
    fn rounding_carry_into_integer() {
        // Value ~ 0.999999... should round to 1.0 when budgeted precision is small.
        // Choose many 9s; keep_frac will be limited by MAX_SIG_DIGITS.
        let amt = u256_dec("999999999999999"); // 15 nines
        let v = convert_u256_to_f64_checked(amt, 15).unwrap();
        assert!((v - 1.0).abs() <= f64::EPSILON);
    }

    #[test]
    fn scale_invariance_decimal_string_exact() {
        let d = 24u32;
        let x = u256_dec("123456789012345678901234567890");
        let s1 = u256_to_decimal_string(x, d).unwrap();
        let s2 = u256_to_decimal_string(x * U256::from(10u8), d + 1).unwrap();
        assert_eq!(s1, s2);
    }

    #[test]
    fn scale_invariance_f64_checked() {
        let d = 24u32;
        // Choose an amount whose integer part after scaling is small enough.
        let x = u256_dec("855134645380964426167598426305908"); // arbitrary large remainder
        // Both computations represent the same real number.
        let a = convert_u256_to_f64_checked(x, d).unwrap();
        let b = convert_u256_to_f64_checked(x * U256::from(10u8), d + 1).unwrap();
        // With identical rounding budget, they should produce the same f64.
        assert_eq!(a.to_bits(), b.to_bits());
    }

    #[test]
    fn large_decimals_zero_leading_fraction() {
        // Make sure we can render long leading zeros correctly.
        let s = u256_to_decimal_string(U256::from(42u8), 10).unwrap();
        assert_eq!(s, "0.0000000042");
    }

    #[test]
    fn distinct_integers_map_to_distinct_f64() {
        let a = U256::from(1_000_000u64);
        let b = U256::from(1_000_001u64);
        let fa = convert_u256_to_f64_checked(a, 0).unwrap();
        let fb = convert_u256_to_f64_checked(b, 0).unwrap();
        assert!(fb > fa);
        assert_ne!(fa.to_bits(), fb.to_bits());
    }

    #[test]
    fn test_convert_positive_u256_to_f64() {
        // Test with 6 decimals (USDC-like)
        let amount = U256::from_str("1000000").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 1.0);

        // Test with 18 decimals (ETH-like)
        let amount = U256::from_str("1000000000000000000").unwrap();
        let result = convert_u256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1.0);
    }

    #[test]
    fn test_convert_zero_u256_to_f64() {
        let amount = U256::ZERO;
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0);

        let result = convert_u256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_convert_fractional_u256_amounts() {
        // Test 0.5 with 6 decimals
        let amount = U256::from_str("500000").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.5);

        // Test 0.123456 with 6 decimals
        let amount = U256::from_str("123456").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.123456);
    }

    #[test]
    fn test_convert_positive_i256_to_f64() {
        // Test with 6 decimals (USDC-like)
        let amount = I256::from_str("1000000").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 1.0);

        // Test with 18 decimals (ETH-like)
        let amount = I256::from_str("1000000000000000000").unwrap();
        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1.0);
    }

    #[test]
    fn test_convert_negative_i256_to_f64() {
        // Test negative value with 6 decimals
        let amount = I256::from_str("-1000000").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, -1.0);

        // Test negative value with 18 decimals
        let amount = I256::from_str("-2500000000000000000").unwrap();
        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, -2.5);
    }

    #[test]
    fn test_convert_zero_i256_to_f64() {
        let amount = I256::ZERO;
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0);

        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 0.0);
    }

    #[test]
    fn test_convert_fractional_amounts() {
        // Test 0.5 with 6 decimals
        let amount = I256::from_str("500000").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.5);

        // Test 0.123456 with 6 decimals
        let amount = I256::from_str("123456").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.123456);

        // Test negative fractional
        let amount = I256::from_str("-123456").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, -0.123456);
    }

    #[test]
    fn test_u256_vs_i256_consistency() {
        // Test that positive values give same results for U256 and I256
        let u256_amount = U256::from_str("1000000000000000000").unwrap();
        let i256_amount = I256::from_str("1000000000000000000").unwrap();

        let u256_result = convert_u256_to_f64(u256_amount, 18).unwrap();
        let i256_result = convert_i256_to_f64(i256_amount, 18).unwrap();

        assert_eq!(u256_result, i256_result);
        assert_eq!(u256_result, 1.0);
    }

    #[test]
    fn test_convert_real_world_examples() {
        // Example: 1234.567890 USDC (6 decimals)
        let amount = U256::from_str("1234567890").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert!((result - 1234.567890).abs() < f64::EPSILON);

        // Example: Large liquidity amount - 100,000 tokens with 8 decimals
        let amount = U256::from_str("10000000000000").unwrap();
        let result = convert_u256_to_f64(amount, 8).unwrap();
        assert_eq!(result, 100_000.0);
    }
}
