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

//! DeFi-specific extensions for the [`Money`] type.

use alloy_primitives::U256;

use crate::types::{Currency, Money};

impl Money {
    /// Creates a new [`Money`] instance from raw wei value with 18-decimal precision.
    ///
    /// This method is specifically designed for DeFi applications where values are
    /// represented in wei (the smallest unit of Ether, 1 ETH = 10^18 wei).
    ///
    /// # Panics
    ///
    /// Panics if `currency.precision` is not 18, or if the raw wei value exceeds the
    /// signed 128-bit range.
    pub fn from_wei<U>(raw_wei: U, currency: Currency) -> Self
    where
        U: Into<U256>,
    {
        assert!(
            currency.precision == 18,
            "`from_wei` requires a currency with precision 18, was {} for {}",
            currency.precision,
            currency.code,
        );

        let raw_u256: U256 = raw_wei.into();
        let raw_u128: u128 = raw_u256
            .try_into()
            .expect("raw wei value exceeds 128-bit range");

        assert!(
            raw_u128 <= i128::MAX as u128,
            "raw wei value exceeds signed 128-bit range"
        );

        let raw_i128: i128 = raw_u128 as i128;
        Self::from_raw(raw_i128, currency)
    }

    /// Converts this [`Money`] instance to raw wei value.
    ///
    /// # Panics
    ///
    /// Panics if `self.currency.precision` is not 18 or `self.raw` is negative.
    /// For other precisions convert to precision 18 first.
    #[must_use]
    pub fn to_wei(&self) -> U256 {
        assert!(
            self.currency.precision == 18,
            "Failed to convert money with precision {} to wei (requires precision 18)",
            self.currency.precision,
        );
        assert!(self.raw >= 0, "Failed to convert negative money to wei");
        U256::from(self.raw as u128)
    }
}

#[cfg(test)]
mod tests {
    use alloy_primitives::U256;
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::enums::CurrencyType;

    #[rstest]
    fn test_from_wei_one_eth() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let one_eth_wei = U256::from(1_000_000_000_000_000_000_u64);
        let money = Money::from_wei(one_eth_wei, eth);

        // Use decimal comparison for high precision values
        assert_eq!(money.as_decimal(), dec!(1));
        assert_eq!(money.currency.precision, 18);
    }

    #[rstest]
    fn test_from_wei_small_amount() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let small_wei = U256::from(1_000_000_000_000_u64); // 0.000001 ETH
        let money = Money::from_wei(small_wei, eth);

        // Use decimal comparison for high precision values
        assert_eq!(money.as_decimal(), dec!(0.000001)); // 0.000001
    }

    #[rstest]
    fn test_to_wei_one_eth() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let money = Money::from_wei(U256::from(1_000_000_000_000_000_000_u64), eth);
        let wei_value = money.to_wei();

        assert_eq!(wei_value, U256::from(1_000_000_000_000_000_000_u64));
    }

    #[rstest]
    fn test_to_wei_small_amount() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let money = Money::from_wei(U256::from(1_000_000_000_000_u64), eth);
        let wei_value = money.to_wei();

        assert_eq!(wei_value, U256::from(1_000_000_000_000_u64));
    }

    #[rstest]
    fn test_wei_roundtrip() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let original_wei = U256::from(1_234_567_890_123_456_789_u64);
        let money = Money::from_wei(original_wei, eth);
        let roundtrip_wei = money.to_wei();

        assert_eq!(original_wei, roundtrip_wei);
    }

    #[rstest]
    fn test_checked_arith_rejects_mixed_scale_same_code() {
        // Currency equality compares code only, so an 18-decimal ETH and the standard
        // 8-decimal ETH compare equal. Their raw values are at different scales, so
        // checked_add / checked_sub must detect the mismatch and return None.
        let eth_standard = Currency::ETH(); // precision 8
        let eth_wei = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);

        let standard = Money::new(1.0, eth_standard);
        let wei = Money::from_wei(U256::from(1_000_000_000_000_000_000_u128), eth_wei);

        assert_eq!(wei.checked_add(standard), None);
        assert_eq!(standard.checked_add(wei), None);
        assert_eq!(wei.checked_sub(standard), None);
        assert_eq!(standard.checked_sub(wei), None);
    }

    #[rstest]
    #[should_panic(expected = "`from_wei` requires a currency with precision 18")]
    fn test_from_wei_rejects_non_18_precision() {
        let eth_8 = Currency::ETH(); // precision 8
        let _ = Money::from_wei(U256::from(1_000_000_000_000_000_000_u128), eth_8);
    }

    #[rstest]
    #[should_panic(expected = "requires precision 18")]
    fn test_to_wei_rejects_non_18_precision() {
        let usd = Currency::new("USD", 2, 840, "United States dollar", CurrencyType::Fiat);
        let m = Money::new(1.0, usd);
        let _ = m.to_wei();
    }

    #[rstest]
    fn test_from_wei_zero() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let money = Money::from_wei(U256::ZERO, eth);

        assert!(money.is_zero());
        assert_eq!(money.as_decimal(), Decimal::ZERO);
        assert_eq!(money.to_wei(), U256::ZERO);
    }

    // The largest `u128` value does not fit into an *signed* 128-bit integer and therefore must
    // trigger a safety panic.
    #[rstest]
    #[should_panic(expected = "raw wei value exceeds signed 128-bit range")]
    fn test_from_wei_maximum_u128() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let max_wei = U256::from(u128::MAX);
        let _ = Money::from_wei(max_wei, eth);
    }

    #[rstest]
    #[should_panic(expected = "raw wei value exceeds 128-bit range")]
    fn test_from_wei_overflow() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let overflow_wei = U256::from(u128::MAX) + U256::from(1u64);
        Money::from_wei(overflow_wei, eth);
    }

    #[rstest]
    fn test_from_wei_different_tokens() {
        let usdc = Currency::new("USDC", 18, 0, "USD Coin", CurrencyType::Crypto);
        let dai = Currency::new("DAI", 18, 0, "Dai Stablecoin", CurrencyType::Crypto);

        let wei_amount = U256::from(500_000_000_000_000_000_u64); // 0.5 tokens
        let usdc_money = Money::from_wei(wei_amount, usdc);
        let dai_money = Money::from_wei(wei_amount, dai);

        assert_eq!(usdc_money.as_decimal(), dai_money.as_decimal());
        assert_eq!(usdc_money.to_wei(), dai_money.to_wei());
        assert_ne!(usdc_money.currency, dai_money.currency);
    }

    #[rstest]
    fn test_arithmetic_with_wei_values() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let money1 = Money::from_wei(U256::from(1_000_000_000_000_000_000_u64), eth); // 1 ETH
        let money2 = Money::from_wei(U256::from(500_000_000_000_000_000_u64), eth); // 0.5 ETH

        let sum = money1 + money2;
        assert_eq!(sum.as_decimal(), dec!(1.5)); // 1.5
        assert_eq!(sum.to_wei(), U256::from(1_500_000_000_000_000_000_u64));

        let diff = money1 - money2;
        assert_eq!(diff.as_decimal(), dec!(0.5)); // 0.5
        assert_eq!(diff.to_wei(), U256::from(500_000_000_000_000_000_u64));
    }

    #[rstest]
    fn test_comparison_with_wei_values() {
        let eth = Currency::new("ETH", 18, 0, "Ethereum", CurrencyType::Crypto);
        let money1 = Money::from_wei(U256::from(1_000_000_000_000_000_000_u64), eth); // 1 ETH
        let money2 = Money::from_wei(U256::from(2_000_000_000_000_000_000_u64), eth); // 2 ETH
        let money3 = Money::from_wei(U256::from(1_000_000_000_000_000_000_u64), eth); // 1 ETH

        assert!(money1 < money2);
        assert!(money2 > money1);
        assert_eq!(money1, money3);
        assert!(money1 <= money3);
        assert!(money1 >= money3);
    }
}
