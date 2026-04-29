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

//! DeFi-specific extensions for the [`Price`] type.

use alloy_primitives::U256;

use crate::types::price::Price;

impl Price {
    /// Constructs a [`Price`] from a raw amount expressed in wei (18-decimal fixed-point).
    ///
    /// The resulting [`Price`] will always have `precision` equal to `18`.
    ///
    /// # Panics
    ///
    /// Panics if the supplied `raw_wei` cannot fit into a signed 128-bit integer (this would
    /// exceed the numeric range of the internal `PriceRaw` representation).
    #[must_use]
    pub fn from_wei<U>(raw_wei: U) -> Self
    where
        U: Into<U256>,
    {
        let raw_u256: U256 = raw_wei.into();
        let raw_u128: u128 = raw_u256
            .try_into()
            .expect("raw wei value exceeds 128-bit range");

        assert!(
            raw_u128 <= i128::MAX as u128,
            "raw wei value exceeds signed 128-bit range"
        );

        let raw_i128: i128 = raw_u128 as i128;
        Self::from_raw(raw_i128, 18)
    }

    /// Converts this [`Price`] to a wei amount (U256).
    ///
    /// Only valid for prices with precision 18. For other precisions convert to precision 18 first.
    ///
    /// # Panics
    ///
    /// Panics if the price has precision other than 18 or if the raw value is negative.
    #[must_use]
    pub fn as_wei(&self) -> U256 {
        assert!(
            self.precision == 18,
            "Failed to convert price with precision {} to wei (requires precision 18)",
            self.precision
        );

        assert!(self.raw >= 0, "Failed to convert negative price to wei");

        // We've checked that raw is non-negative, so casting to u128 is safe
        U256::from(self.raw as u128)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_from_wei_basic() {
        let price = Price::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1 ETH in wei
        assert_eq!(price.precision, 18);
        assert_eq!(price.as_decimal(), dec!(1.0));
    }

    #[rstest]
    fn test_precision_18_requires_from_wei() {
        // Verify that precision 18 cannot be used with float-based constructor
        let result = Price::new_checked(1.0, 18);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("use `Price::from_wei()` for wei values")
        );

        // But from_wei works fine with precision 18
        let price = Price::from_wei(U256::from(1_000_000_000_000_000_000_u128));
        assert_eq!(price.precision, 18);
        assert_eq!(price.as_decimal(), dec!(1.0));
    }

    #[rstest]
    fn test_as_wei_basic() {
        let price = Price::from_raw(1_000_000_000_000_000_000_i128, 18);
        let wei = price.as_wei();
        assert_eq!(wei, U256::from(1_000_000_000_000_000_000_u128));
    }

    #[rstest]
    #[should_panic(
        expected = "Failed to convert price with precision 2 to wei (requires precision 18)"
    )]
    fn test_as_wei_wrong_precision() {
        let price = Price::new(1.23, 2);
        let _ = price.as_wei();
    }

    #[rstest]
    #[should_panic(expected = "Failed to convert negative price")]
    fn test_as_wei_negative_price() {
        let price = Price::from_raw(-1_000_000_000_000_000_000_i128, 18);
        let _ = price.as_wei();
    }

    #[rstest]
    fn test_wei_round_trip() {
        let original_wei = U256::from(1_500_000_000_000_000_000_u128); // 1.5 ETH
        let price = Price::from_wei(original_wei);
        let converted_wei = price.as_wei();
        assert_eq!(original_wei, converted_wei);
        assert_eq!(price.as_decimal(), dec!(1.5));
    }

    #[rstest]
    fn test_from_wei_zero() {
        let price = Price::from_wei(U256::ZERO);
        assert_eq!(price.precision, 18);
        assert_eq!(price.as_decimal(), dec!(0));
        assert_eq!(price.as_wei(), U256::ZERO);
    }

    #[rstest]
    fn test_from_wei_very_large_value() {
        // Test with a very large but valid wei amount (1 billion ETH)
        let large_wei = U256::from(1_000_000_000_000_000_000_000_000_000_u128);
        let price = Price::from_wei(large_wei);
        assert_eq!(price.precision, 18);
        assert_eq!(price.as_wei(), large_wei);
        assert_eq!(price.as_decimal(), dec!(1000000000));
    }

    #[rstest]
    #[should_panic(expected = "raw wei value exceeds 128-bit range")]
    fn test_from_wei_overflow() {
        let overflow_wei = U256::from(u128::MAX) + U256::from(1u64);
        let _ = Price::from_wei(overflow_wei);
    }

    #[rstest]
    fn test_checked_arith_accepts_wei_precision() {
        // Wei prices use precision 18 (> FIXED_PRECISION = 16) but are valid, not sentinels.
        let a = Price::from_wei(U256::from(1_000_000_000_000_000_000u128));
        let b = Price::from_wei(U256::from(2_000_000_000_000_000_000u128));
        let sum = a
            .checked_add(b)
            .expect("checked_add must accept wei prices");
        assert_eq!(sum.as_decimal(), dec!(3));
        let diff = b
            .checked_sub(a)
            .expect("checked_sub must accept wei prices");
        assert_eq!(diff.as_decimal(), dec!(1));
    }

    #[rstest]
    fn test_checked_arith_rejects_mixed_scale() {
        // Wei (precision 18, raw at 10^18 scale) and standard (precision 0, raw at
        // FIXED_SCALAR scale) cannot be added with raw arithmetic without rescaling.
        // checked_add / checked_sub must return None rather than produce a wrong result.
        let wei = Price::from_wei(U256::from(1_000_000_000_000_000_000u128));
        let standard = Price::new(1.0, 0);
        assert_eq!(wei.checked_add(standard), None);
        assert_eq!(standard.checked_add(wei), None);
        assert_eq!(wei.checked_sub(standard), None);
        assert_eq!(standard.checked_sub(wei), None);
    }

    #[rstest]
    fn test_checked_arith_rejects_mixed_defi_scales() {
        // Both above FIXED_PRECISION but at different native scales:
        // precision 17 stores raw at 10^17, precision 18 stores raw at 10^18.
        let p17 = Price::from_raw(100_000_000_000_000_000_i128, 17);
        let p18 = Price::from_wei(U256::from(1_000_000_000_000_000_000u128));
        assert_eq!(p17.checked_add(p18), None);
        assert_eq!(p18.checked_add(p17), None);
        assert_eq!(p17.checked_sub(p18), None);
        assert_eq!(p18.checked_sub(p17), None);
    }

    #[rstest]
    fn test_from_wei_small_amounts() {
        // Test various small wei amounts
        let test_cases = vec![
            (1_u128, dec!(0.000000000000000001)),    // 1 wei
            (1000_u128, dec!(0.000000000000001)),    // 1 picoether
            (1_000_000_u128, dec!(0.000000000001)),  // 1 nanoether
            (1_000_000_000_u128, dec!(0.000000001)), // 1 gwei
        ];

        for (wei_amount, expected_decimal) in test_cases {
            let price = Price::from_wei(U256::from(wei_amount));
            assert_eq!(price.precision, 18);
            assert_eq!(price.as_decimal(), expected_decimal);
            assert_eq!(price.as_wei(), U256::from(wei_amount));
        }
    }

    #[rstest]
    fn test_from_wei_large_amounts() {
        // Test various large wei amounts
        let test_cases = vec![
            (1_000_000_000_000_000_000_u128, dec!(1)),        // 1 ETH
            (10_000_000_000_000_000_000_u128, dec!(10)),      // 10 ETH
            (100_000_000_000_000_000_000_u128, dec!(100)),    // 100 ETH
            (1_000_000_000_000_000_000_000_u128, dec!(1000)), // 1000 ETH
        ];

        for (wei_amount, expected_decimal) in test_cases {
            let price = Price::from_wei(U256::from(wei_amount));
            assert_eq!(price.precision, 18);
            assert_eq!(price.as_decimal(), expected_decimal);
            assert_eq!(price.as_wei(), U256::from(wei_amount));
        }
    }

    #[rstest]
    fn test_as_wei_precision_validation() {
        // Test that as_wei() requires exactly precision 18
        for precision in [2, 6, 8, 16] {
            let price = Price::new(123.45, precision);
            let result = std::panic::catch_unwind(|| price.as_wei());
            assert!(
                result.is_err(),
                "as_wei() should panic for precision {precision}"
            );
        }
    }

    #[rstest]
    fn test_arithmetic_operations_with_wei() {
        let price1 = Price::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0
        let price2 = Price::from_wei(U256::from(500_000_000_000_000_000_u128)); // 0.5

        // Test addition
        let sum = price1 + price2;
        assert_eq!(sum.precision, 18);
        assert_eq!(sum.as_decimal(), dec!(1.5));

        // Test subtraction
        let diff = price1 - price2;
        assert_eq!(diff.precision, 18);
        assert_eq!(diff.as_decimal(), dec!(0.5));
    }

    #[rstest]
    fn test_comparison_operations_with_wei() {
        let price1 = Price::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0
        let price2 = Price::from_wei(U256::from(2_000_000_000_000_000_000_u128)); // 2.0
        let price3 = Price::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0

        assert!(price1 < price2);
        assert!(price2 > price1);
        assert_eq!(price1, price3);
        assert!(price1 <= price3);
        assert!(price1 >= price3);
    }
}
