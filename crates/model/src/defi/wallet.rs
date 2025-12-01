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

use std::{collections::HashSet, fmt::Display};

use alloy_primitives::{Address, U256};

use crate::{
    defi::Token,
    types::{Money, Quantity},
};

/// Represents the balance of a specific ERC-20 token held in a wallet.
///
/// This struct tracks the raw token amount along with optional USD valuation
/// and the token metadata.
#[derive(Debug)]
pub struct TokenBalance {
    /// The raw token amount as a 256-bit unsigned integer.
    pub amount: U256,
    /// The optional USD equivalent value of the token balance.
    pub amount_usd: Option<Quantity>,
    /// The token metadata including chain, address, name, symbol, and decimals.
    pub token: Token,
}

impl TokenBalance {
    /// Creates a new [`TokenBalance`] instance.
    pub const fn new(amount: U256, token: Token) -> Self {
        Self {
            amount,
            token,
            amount_usd: None,
        }
    }

    /// Converts the raw token amount to a human-readable [`Quantity`].
    ///
    /// # Errors
    ///
    /// Returns an error if the U256 amount cannot be converted to a `Quantity`.
    pub fn as_quantity(&self) -> anyhow::Result<Quantity> {
        Quantity::from_u256(self.amount, self.token.decimals)
    }

    /// Sets the USD equivalent value for this token balance.
    pub fn set_amount_usd(&mut self, amount_usd: Quantity) {
        self.amount_usd = Some(amount_usd);
    }
}

impl Display for TokenBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let quantity = self.as_quantity().unwrap_or_default();
        match &self.amount_usd {
            Some(usd) => write!(
                f,
                "TokenBalance(token={}, amount={}, usd=${:.2})",
                self.token.symbol,
                quantity.as_decimal(),
                usd.as_f64()
            ),
            None => write!(
                f,
                "TokenBalance(token={}, amount={})",
                self.token.symbol,
                quantity.as_decimal()
            ),
        }
    }
}

/// Represents the complete balance state of a blockchain wallet.
///
/// Tracks both the native currency balance (e.g., ETH, ARB) and ERC-20 token
/// balances for a wallet address. The `token_universe` defines which tokens
/// should be tracked for balance fetching.
#[derive(Debug)]
pub struct WalletBalance {
    /// The balance of the chain's native currency
    pub native_currency: Option<Money>,
    /// Collection of ERC-20 token balances held in the wallet.
    pub token_balances: Vec<TokenBalance>,
    /// Set of token addresses to track for balance updates.
    pub token_universe: HashSet<Address>,
}

impl WalletBalance {
    /// Creates a new [`WalletBalance`] with the specified token universe.
    pub const fn new(token_universe: HashSet<Address>) -> Self {
        Self {
            native_currency: None,
            token_balances: vec![],
            token_universe,
        }
    }

    /// Returns `true` if the token universe has been initialized with token addresses.
    pub fn is_token_universe_initialized(&self) -> bool {
        !self.token_universe.is_empty()
    }

    /// Sets the native currency balance for the wallet.
    pub fn set_native_currency_balance(&mut self, balance: Money) {
        self.native_currency = Some(balance);
    }

    /// Adds an ERC-20 token balance to the wallet.
    pub fn add_token_balance(&mut self, token_balance: TokenBalance) {
        self.token_balances.push(token_balance);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use alloy_primitives::{U256, address};
    use rstest::rstest;

    use super::*;
    use crate::defi::{
        SharedChain, Token,
        chain::chains,
        stubs::{arbitrum, usdc, weth},
    };

    // Helper to create a token with specific decimals
    fn create_token(symbol: &str, decimals: u8) -> Token {
        Token::new(
            Arc::new(chains::ETHEREUM.clone()),
            address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48"),
            format!("{symbol} Token"),
            symbol.to_string(),
            decimals,
        )
    }

    #[rstest]
    fn test_token_balance_as_quantity_18_decimals(#[from(arbitrum)] chain: SharedChain) {
        // Test case: NU token with 18 decimals
        // Raw amount: 10342000000000000000000 (10342 * 10^18)
        // Expected: 10342.000000000000000000
        let token = Token::new(
            chain,
            address!("0x4fE83213D56308330EC302a8BD641f1d0113A4Cc"),
            "NuCypher".to_string(),
            "NU".to_string(),
            18,
        );
        let amount = U256::from(10342u64) * U256::from(10u64).pow(U256::from(18u64));
        let balance = TokenBalance::new(amount, token);

        let quantity = balance.as_quantity().unwrap();
        assert_eq!(
            quantity.as_decimal().to_string(),
            "10342.000000000000000000"
        );
    }

    #[rstest]
    fn test_token_balance_as_quantity_6_decimals() {
        // Test case: USDC with 6 decimals
        // Raw amount: 92220728254 (92220.728254 * 10^6)
        // Expected: 92220.728254
        let token = create_token("USDC", 6);
        let amount = U256::from(92220728254u64);
        let balance = TokenBalance::new(amount, token);

        let quantity = balance.as_quantity().unwrap();
        assert_eq!(quantity.as_decimal().to_string(), "92220.728254");
    }

    #[rstest]
    fn test_token_balance_as_quantity_fractional_18_decimals(#[from(arbitrum)] chain: SharedChain) {
        // Test case: mETH with 18 decimals and fractional amount
        // Raw amount: 758325512078001391
        // Expected: 0.758325512078001391
        let token = Token::new(
            chain,
            address!("0xd5F7838F5C461fefF7FE49ea5ebaF7728bB0ADfa"),
            "mETH".to_string(),
            "mETH".to_string(),
            18,
        );
        let amount = U256::from(758325512078001391u64);
        let balance = TokenBalance::new(amount, token);

        let quantity = balance.as_quantity().unwrap();
        assert_eq!(quantity.as_decimal().to_string(), "0.758325512078001391");
    }

    #[rstest]
    fn test_token_balance_display_18_decimals(#[from(arbitrum)] chain: SharedChain) {
        // Test Display implementation with 18 decimal token
        let token = Token::new(
            chain,
            address!("0x912CE59144191C1204E64559FE8253a0e49E6548"),
            "Arbitrum".to_string(),
            "ARB".to_string(),
            18,
        );
        // 7922.013795343949480329 ARB
        let amount = U256::from_str_radix("7922013795343949480329", 10).unwrap();
        let balance = TokenBalance::new(amount, token);

        let display = balance.to_string();
        assert!(display.contains("ARB"));
        assert!(display.contains("7922.013795343949480329"));
    }

    #[rstest]
    fn test_token_balance_display_6_decimals() {
        // Test Display implementation with 6 decimal token (USDC)
        let token = create_token("USDC", 6);
        let amount = U256::from(92220728254u64); // 92220.728254 USDC
        let balance = TokenBalance::new(amount, token);

        let display = balance.to_string();
        assert!(display.contains("USDC"));
        assert!(display.contains("92220.728254"));
    }

    #[rstest]
    fn test_token_balance_set_amount_usd(weth: Token) {
        let amount = U256::from(1u64) * U256::from(10u64).pow(U256::from(18u64));
        let mut balance = TokenBalance::new(amount, weth);

        assert!(balance.amount_usd.is_none());

        let usd_value = Quantity::from("3500.00");
        balance.set_amount_usd(usd_value);

        assert!(balance.amount_usd.is_some());
        assert_eq!(
            balance.amount_usd.unwrap().as_decimal().to_string(),
            "3500.00"
        );
    }

    #[rstest]
    fn test_wallet_balance_new_empty() {
        let wallet = WalletBalance::new(HashSet::new());

        assert!(wallet.native_currency.is_none());
        assert!(wallet.token_balances.is_empty());
        assert!(!wallet.is_token_universe_initialized());
    }

    #[rstest]
    fn test_wallet_balance_with_token_universe() {
        let mut tokens = HashSet::new();
        tokens.insert(address!("0xA0b86991c6218b36c1d19D4a2e9Eb0cE3606eB48")); // USDC
        tokens.insert(address!("0x912CE59144191C1204E64559FE8253a0e49E6548")); // ARB

        let wallet = WalletBalance::new(tokens);

        assert!(wallet.is_token_universe_initialized());
        assert_eq!(wallet.token_universe.len(), 2);
    }

    #[rstest]
    fn test_wallet_balance_set_native_currency() {
        let mut wallet = WalletBalance::new(HashSet::new());

        assert!(wallet.native_currency.is_none());

        let eth_balance = Money::new(50.936054, crate::types::Currency::ETH());
        wallet.set_native_currency_balance(eth_balance);

        assert!(wallet.native_currency.is_some());
    }

    #[rstest]
    fn test_wallet_balance_add_token_balance(usdc: Token, weth: Token) {
        let mut wallet = WalletBalance::new(HashSet::new());

        let usdc_balance = TokenBalance::new(U256::from(100_000_000u64), usdc); // 100 USDC
        let weth_balance = TokenBalance::new(U256::from(10u64).pow(U256::from(18u64)), weth); // 1 WETH

        wallet.add_token_balance(usdc_balance);
        wallet.add_token_balance(weth_balance);

        assert_eq!(wallet.token_balances.len(), 2);
        assert_eq!(wallet.token_balances[0].token.symbol, "USDC");
        assert_eq!(wallet.token_balances[1].token.symbol, "WETH");
    }
}
