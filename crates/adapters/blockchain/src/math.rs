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

use alloy::primitives::{I256, U256};

/// Convert an alloy's I256 value to f64, accounting for token decimals.
///
/// # Errors
///
/// Returns an error if the I256 value cannot be parsed to f64.
pub fn convert_i256_to_f64(amount: I256, decimals: u8) -> anyhow::Result<f64> {
    // Handle the sign separately
    let is_negative = amount.is_negative();
    let abs_amount = if is_negative { -amount } else { amount };

    // Convert to string to avoid precision loss for large numbers
    let amount_str = abs_amount.to_string();
    let mut amount_f64: f64 = amount_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse I256 to f64: {}", e))?;

    // Apply sign
    if is_negative {
        amount_f64 = -amount_f64;
    }

    // Apply decimal scaling
    let factor = 10f64.powi(i32::from(decimals));
    Ok(amount_f64 / factor)
}

/// Convert an alloy's U256 value to f64, accounting for token decimals.
///
/// # Errors
///
/// Returns an error if the U256 value cannot be parsed to f64.
pub fn convert_u256_to_f64(amount: U256, decimals: u8) -> anyhow::Result<f64> {
    // Convert to string to avoid precision loss for large numbers
    let amount_str = amount.to_string();
    let amount_f64: f64 = amount_str
        .parse()
        .map_err(|e| anyhow::anyhow!("Failed to parse U256 to f64: {}", e))?;

    // Apply decimal scaling
    let factor = 10f64.powi(i32::from(decimals));
    Ok(amount_f64 / factor)
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use alloy::primitives::{I256, U256};
    use rstest::rstest;

    use super::*;

    #[rstest]
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

    #[rstest]
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

    #[rstest]
    fn test_convert_zero_i256_to_f64() {
        let amount = I256::ZERO;
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0);

        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 0.0);
    }

    #[rstest]
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

    #[rstest]
    fn test_convert_large_i256_values() {
        // Test very large positive value
        let large_value = U256::from(10).pow(U256::from(30)); // 10^30
        let amount = I256::try_from(large_value).unwrap();
        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1e12); // 10^30 / 10^18 = 10^12

        // Test maximum safe integer range
        let amount = I256::from_str("9007199254740991").unwrap(); // MAX_SAFE_INTEGER
        let result = convert_i256_to_f64(amount, 0).unwrap();
        assert_eq!(result, 9_007_199_254_740_991.0);
    }

    #[rstest]
    fn test_convert_with_different_decimals() {
        let amount = I256::from_str("1000000000").unwrap();

        // 0 decimals
        let result = convert_i256_to_f64(amount, 0).unwrap();
        assert_eq!(result, 1_000_000_000.0);

        // 9 decimals
        let result = convert_i256_to_f64(amount, 9).unwrap();
        assert_eq!(result, 1.0);

        // 12 decimals
        let result = convert_i256_to_f64(amount, 12).unwrap();
        assert_eq!(result, 0.001);
    }

    #[rstest]
    fn test_convert_edge_cases() {
        // Test very small positive amount with high decimals
        let amount = I256::from_str("1").unwrap();
        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1e-18);

        // Test amount smaller than decimal places
        let amount = I256::from_str("100").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0001);
    }

    #[rstest]
    fn test_convert_real_world_examples() {
        // Example: 1234.567890 USDC (6 decimals)
        let amount = I256::from_str("1234567890").unwrap();
        let result = convert_i256_to_f64(amount, 6).unwrap();
        assert!((result - 1234.567890).abs() < f64::EPSILON);

        // Example: -0.005 ETH (18 decimals)
        let amount = I256::from_str("-5000000000000000").unwrap();
        let result = convert_i256_to_f64(amount, 18).unwrap();
        assert_eq!(result, -0.005);

        // Example: Large swap amount - 100,000 tokens with 8 decimals
        let amount = I256::from_str("10000000000000").unwrap();
        let result = convert_i256_to_f64(amount, 8).unwrap();
        assert_eq!(result, 100_000.0);
    }

    #[rstest]
    fn test_precision_boundaries() {
        // Test precision near f64 boundaries
        // f64 can accurately represent integers up to 2^53
        let max_safe = I256::from_str("9007199254740992").unwrap(); // 2^53
        let result = convert_i256_to_f64(max_safe, 0).unwrap();
        assert_eq!(result, 9_007_199_254_740_992.0);

        // Test with scientific notation result
        let amount = I256::from_str("1234567890123456789").unwrap();
        let result = convert_i256_to_f64(amount, 9).unwrap();
        assert!((result - 1_234_567_890.123_456_7).abs() < 1.0); // Some precision loss expected
    }

    // U256 Tests
    #[rstest]
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

    #[rstest]
    fn test_convert_zero_u256_to_f64() {
        let amount = U256::ZERO;
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0);

        let result = convert_u256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 0.0);
    }

    #[rstest]
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

    #[rstest]
    fn test_convert_large_u256_values() {
        // Test very large positive value
        let large_value = U256::from(10).pow(U256::from(30)); // 10^30
        let result = convert_u256_to_f64(large_value, 18).unwrap();
        assert_eq!(result, 1e12); // 10^30 / 10^18 = 10^12

        // Test maximum safe integer range
        let amount = U256::from_str("9007199254740991").unwrap(); // MAX_SAFE_INTEGER
        let result = convert_u256_to_f64(amount, 0).unwrap();
        assert_eq!(result, 9_007_199_254_740_991.0);
    }

    #[rstest]
    fn test_convert_u256_with_different_decimals() {
        let amount = U256::from_str("1000000000").unwrap();

        // 0 decimals
        let result = convert_u256_to_f64(amount, 0).unwrap();
        assert_eq!(result, 1_000_000_000.0);

        // 9 decimals
        let result = convert_u256_to_f64(amount, 9).unwrap();
        assert_eq!(result, 1.0);

        // 12 decimals
        let result = convert_u256_to_f64(amount, 12).unwrap();
        assert_eq!(result, 0.001);
    }

    #[rstest]
    fn test_convert_u256_edge_cases() {
        // Test very small positive amount with high decimals
        let amount = U256::from_str("1").unwrap();
        let result = convert_u256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1e-18);

        // Test amount smaller than decimal places
        let amount = U256::from_str("100").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert_eq!(result, 0.0001);
    }

    #[rstest]
    fn test_convert_u256_real_world_examples() {
        // Example: 1234.567890 USDC (6 decimals)
        let amount = U256::from_str("1234567890").unwrap();
        let result = convert_u256_to_f64(amount, 6).unwrap();
        assert!((result - 1234.567890).abs() < f64::EPSILON);

        // Example: Large liquidity amount - 100,000 tokens with 8 decimals
        let amount = U256::from_str("10000000000000").unwrap();
        let result = convert_u256_to_f64(amount, 8).unwrap();
        assert_eq!(result, 100_000.0);

        // Example: Very large supply - 1 trillion tokens with 18 decimals
        let amount = U256::from_str("1000000000000000000000000000000").unwrap(); // 10^30
        let result = convert_u256_to_f64(amount, 18).unwrap();
        assert_eq!(result, 1e12);
    }

    #[rstest]
    fn test_convert_u256_precision_boundaries() {
        // Test precision near f64 boundaries
        // f64 can accurately represent integers up to 2^53
        let max_safe = U256::from_str("9007199254740992").unwrap(); // 2^53
        let result = convert_u256_to_f64(max_safe, 0).unwrap();
        assert_eq!(result, 9_007_199_254_740_992.0);

        // Test with scientific notation result
        let amount = U256::from_str("1234567890123456789").unwrap();
        let result = convert_u256_to_f64(amount, 9).unwrap();
        assert!((result - 1_234_567_890.123_456_7).abs() < 1.0); // Some precision loss expected
    }

    #[rstest]
    fn test_convert_u256_vs_i256_consistency() {
        // Test that positive values give same results for U256 and I256
        let u256_amount = U256::from_str("1000000000000000000").unwrap();
        let i256_amount = I256::from_str("1000000000000000000").unwrap();

        let u256_result = convert_u256_to_f64(u256_amount, 18).unwrap();
        let i256_result = convert_i256_to_f64(i256_amount, 18).unwrap();

        assert_eq!(u256_result, i256_result);
        assert_eq!(u256_result, 1.0);
    }

    #[rstest]
    fn test_convert_u256_max_values() {
        // Test very large U256 values that wouldn't fit in I256
        let large_u256 = U256::from(2).pow(U256::from(255)); // Close to U256::MAX
        let result = convert_u256_to_f64(large_u256, 0).unwrap();
        // Should be a very large number but not infinite
        assert!(result.is_finite());
        assert!(result > 0.0);

        // Test with decimals to bring it down to reasonable range
        let large_u256_with_decimals = U256::from(2).pow(U256::from(60)); // 2^60
        let result = convert_u256_to_f64(large_u256_with_decimals, 18).unwrap();
        // 2^60 ≈ 1.15e18, so 2^60 / 10^18 ≈ 1.15
        assert!(result.is_finite());
        assert!(result > 1.0);
        assert!(result < 2.0); // Should be around 1.15
    }
}
