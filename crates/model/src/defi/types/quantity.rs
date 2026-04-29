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

//! DeFi-specific extensions for the [`Quantity`] type.

use alloy_primitives::U256;

use crate::types::quantity::Quantity;

impl Quantity {
    /// Constructs a [`Quantity`] from a raw amount expressed in wei (18-decimal fixed-point).
    ///
    /// The resulting [`Quantity`] will always have `precision` equal to `18`.
    ///
    /// # Panics
    ///
    /// Panics if the supplied `raw_wei` cannot fit into an **unsigned** 128-bit integer (this
    /// would exceed the numeric range of the internal `QuantityRaw` representation).
    #[must_use]
    pub fn from_wei<U>(raw_wei: U) -> Self
    where
        U: Into<U256>,
    {
        let raw_u256: U256 = raw_wei.into();
        let raw_u128: u128 = raw_u256
            .try_into()
            .expect("raw wei value exceeds unsigned 128-bit range");

        Self::from_raw(raw_u128, 18)
    }

    /// Converts this [`Quantity`] to a wei amount (U256).
    ///
    /// Only valid for prices with precision 18. For other precisions convert to precision 18 first.
    ///
    /// # Panics
    ///
    /// Panics if the quantity has precision other than 18.
    #[must_use]
    pub fn as_wei(&self) -> U256 {
        assert!(
            self.precision == 18,
            "Failed to convert quantity with precision {} to wei (requires precision 18)",
            self.precision
        );

        U256::from(self.raw)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    #[rstest]
    fn test_from_wei_basic() {
        let quantity = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1 token in wei
        assert_eq!(quantity.precision, 18);
        assert_eq!(quantity.as_decimal(), dec!(1.0));
    }

    #[rstest]
    fn test_as_wei_basic() {
        let quantity = Quantity::from_raw(1_000_000_000_000_000_000_u128, 18);
        let wei = quantity.as_wei();
        assert_eq!(wei, U256::from(1_000_000_000_000_000_000_u128));
    }

    #[rstest]
    #[should_panic(
        expected = "Failed to convert quantity with precision 2 to wei (requires precision 18)"
    )]
    fn test_as_wei_wrong_precision() {
        let quantity = Quantity::new(1.23, 2);
        let _ = quantity.as_wei();
    }

    #[rstest]
    fn test_wei_round_trip() {
        let original_wei = U256::from(1_500_000_000_000_000_000_u128); // 1.5 tokens
        let quantity = Quantity::from_wei(original_wei);
        let converted_wei = quantity.as_wei();
        assert_eq!(original_wei, converted_wei);
        assert_eq!(quantity.as_decimal(), dec!(1.5));
    }

    #[rstest]
    fn test_checked_arith_accepts_wei_precision() {
        let a = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128));
        let b = Quantity::from_wei(U256::from(2_000_000_000_000_000_000_u128));
        let sum = a
            .checked_add(b)
            .expect("checked_add must accept wei quantities");
        assert_eq!(sum.as_decimal(), dec!(3));
        let diff = b
            .checked_sub(a)
            .expect("checked_sub must accept wei quantities");
        assert_eq!(diff.as_decimal(), dec!(1));
    }

    #[rstest]
    fn test_checked_arith_rejects_mixed_scale() {
        // Wei (precision 18, raw at 10^18 scale) and standard (precision 0, raw at
        // FIXED_SCALAR scale) cannot be added with raw arithmetic without rescaling.
        let wei = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128));
        let standard = Quantity::new(1.0, 0);
        assert_eq!(wei.checked_add(standard), None);
        assert_eq!(standard.checked_add(wei), None);
        assert_eq!(wei.checked_sub(standard), None);
        assert_eq!(standard.checked_sub(wei), None);
    }

    #[rstest]
    fn test_checked_arith_rejects_mixed_defi_scales() {
        // Both above FIXED_PRECISION but at different native scales:
        // precision 17 stores raw at 10^17, precision 18 stores raw at 10^18.
        let q17 = Quantity::from_raw(100_000_000_000_000_000_u128, 17);
        let q18 = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128));
        assert_eq!(q17.checked_add(q18), None);
        assert_eq!(q18.checked_add(q17), None);
        assert_eq!(q17.checked_sub(q18), None);
        assert_eq!(q18.checked_sub(q17), None);
    }

    #[rstest]
    fn test_from_wei_large_value() {
        // Test with a large but valid wei amount
        let large_wei = U256::from(1_000_000_000_000_000_000_000_u128); // 1000 tokens
        let quantity = Quantity::from_wei(large_wei);
        assert_eq!(quantity.precision, 18);
        assert_eq!(quantity.as_decimal(), dec!(1000.0));
    }

    #[rstest]
    fn test_from_wei_small_value() {
        // Test with a small but representable wei amount (1 million wei = 1e-12)
        // Very small values like 1 wei (1e-18) are at the edge of f64 precision
        let small_wei = U256::from(1_000_000_u128);
        let quantity = Quantity::from_wei(small_wei);
        assert_eq!(quantity.precision, 18);
        assert_eq!(quantity.as_decimal(), dec!(0.000000000001));
    }

    #[rstest]
    fn test_from_wei_zero() {
        let quantity = Quantity::from_wei(U256::ZERO);
        assert_eq!(quantity.precision, 18);
        assert_eq!(quantity.as_decimal(), dec!(0));
        assert_eq!(quantity.as_wei(), U256::ZERO);
    }

    #[rstest]
    fn test_from_wei_very_large_value() {
        // Test with a very large but valid wei amount (1 billion tokens)
        let large_wei = U256::from(1_000_000_000_000_000_000_000_000_000_u128);
        let quantity = Quantity::from_wei(large_wei);
        assert_eq!(quantity.precision, 18);
        assert_eq!(quantity.as_wei(), large_wei);
        assert_eq!(quantity.as_decimal(), dec!(1000000000));
    }

    #[rstest]
    #[should_panic(expected = "raw wei value exceeds unsigned 128-bit range")]
    fn test_from_wei_overflow() {
        let overflow_wei = U256::from(u128::MAX) + U256::from(1_u64);
        let _ = Quantity::from_wei(overflow_wei);
    }

    #[rstest]
    fn test_from_wei_various_amounts() {
        // Test various wei amounts and their decimal equivalents
        let test_cases = vec![
            (1_u128, dec!(0.000000000000000001)),        // 1 wei
            (1000_u128, dec!(0.000000000000001)),        // 1 thousand wei
            (1_000_000_u128, dec!(0.000000000001)),      // 1 million wei
            (1_000_000_000_u128, dec!(0.000000001)),     // 1 gwei
            (1_000_000_000_000_u128, dec!(0.000001)),    // 1 microtoken
            (1_000_000_000_000_000_u128, dec!(0.001)),   // 1 millitoken
            (1_000_000_000_000_000_000_u128, dec!(1)),   // 1 token
            (10_000_000_000_000_000_000_u128, dec!(10)), // 10 tokens
        ];

        for (wei_amount, expected_decimal) in test_cases {
            let quantity = Quantity::from_wei(U256::from(wei_amount));
            assert_eq!(quantity.precision, 18);
            assert_eq!(quantity.as_decimal(), expected_decimal);
            assert_eq!(quantity.as_wei(), U256::from(wei_amount));
        }
    }

    #[rstest]
    fn test_as_wei_precision_validation() {
        // Test that as_wei() requires exactly precision 18
        for precision in [2, 6, 8, 16] {
            let quantity = Quantity::new(123.45, precision);
            let result = std::panic::catch_unwind(|| quantity.as_wei());
            assert!(
                result.is_err(),
                "as_wei() should panic for precision {precision}"
            );
        }
    }

    #[rstest]
    fn test_arithmetic_operations_with_wei() {
        let quantity1 = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0
        let quantity2 = Quantity::from_wei(U256::from(500_000_000_000_000_000_u128)); // 0.5

        // Test addition
        let sum = quantity1 + quantity2;
        assert_eq!(sum.precision, 18);
        assert_eq!(sum.as_decimal(), dec!(1.5));
        assert_eq!(sum.as_wei(), U256::from(1_500_000_000_000_000_000_u128));

        // Test subtraction
        let diff = quantity1 - quantity2;
        assert_eq!(diff.precision, 18);
        assert_eq!(diff.as_decimal(), dec!(0.5));
        assert_eq!(diff.as_wei(), U256::from(500_000_000_000_000_000_u128));
    }

    #[rstest]
    fn test_comparison_operations_with_wei() {
        let quantity1 = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0
        let quantity2 = Quantity::from_wei(U256::from(2_000_000_000_000_000_000_u128)); // 2.0
        let quantity3 = Quantity::from_wei(U256::from(1_000_000_000_000_000_000_u128)); // 1.0

        assert!(quantity1 < quantity2);
        assert!(quantity2 > quantity1);
        assert_eq!(quantity1, quantity3);
        assert!(quantity1 <= quantity3);
        assert!(quantity1 >= quantity3);
    }
}
