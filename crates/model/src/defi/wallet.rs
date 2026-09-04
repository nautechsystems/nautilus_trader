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

use std::{collections::HashSet, fmt::Display};

use alloy_primitives::{Address, U256};

use crate::{
    defi::Token,
    enums::CurrencyType,
    types::{AccountBalance, Currency, Money, Quantity},
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
    #[must_use]
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
        Quantity::from_u256(self.amount, self.token.decimals).map_err(Into::into)
    }

    fn as_money(&self) -> anyhow::Result<Money> {
        let currency = Currency::new_checked(
            &self.token.symbol,
            self.token.decimals,
            0,
            &self.token.name,
            CurrencyType::Crypto,
        )?;
        Money::from_u256(self.amount, currency).map_err(Into::into)
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
    #[must_use]
    pub const fn new(token_universe: HashSet<Address>) -> Self {
        Self {
            native_currency: None,
            token_balances: vec![],
            token_universe,
        }
    }

    /// Returns `true` if the token universe has been initialized with token addresses.
    #[must_use]
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

    /// Replaces the complete native and token balance snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if the token balances do not exactly match the configured universe,
    /// contain duplicate currencies, or cannot be represented as account balances. The existing
    /// snapshot remains unchanged on error.
    pub fn replace_balances(
        &mut self,
        native_currency: Money,
        mut token_balances: Vec<TokenBalance>,
    ) -> anyhow::Result<Vec<AccountBalance>> {
        token_balances.sort_unstable_by_key(|balance| balance.token.address);
        let replacement = Self {
            native_currency: Some(native_currency),
            token_balances,
            token_universe: self.token_universe.clone(),
        };
        let balances = replacement.account_balances(replacement.token_balances.iter())?;
        *self = replacement;
        Ok(balances)
    }

    /// Returns the complete wallet snapshot as account balances.
    ///
    /// # Errors
    ///
    /// Returns an error if the snapshot is incomplete, contains duplicate currencies, or a token
    /// amount cannot be represented as money.
    pub fn as_account_balances(&self) -> anyhow::Result<Vec<AccountBalance>> {
        let mut token_balances = self.token_balances.iter().collect::<Vec<_>>();
        token_balances.sort_unstable_by_key(|balance| balance.token.address);
        self.account_balances(token_balances)
    }

    fn account_balances<'a>(
        &'a self,
        token_balances: impl IntoIterator<Item = &'a TokenBalance>,
    ) -> anyhow::Result<Vec<AccountBalance>> {
        self.validate_token_addresses()?;

        let native_currency = self
            .native_currency
            .ok_or_else(|| anyhow::anyhow!("Wallet balance snapshot has no native currency"))?;
        let mut currencies = HashSet::new();
        currencies.insert(native_currency.currency);

        let mut balances = Vec::with_capacity(self.token_balances.len() + 1);
        balances.push(AccountBalance::new_checked(
            native_currency,
            Money::zero(native_currency.currency),
            native_currency,
        )?);

        for token_balance in token_balances {
            let total = token_balance.as_money()?;
            if !currencies.insert(total.currency) {
                anyhow::bail!(
                    "Wallet balance snapshot contains duplicate currency {}",
                    total.currency
                );
            }
            balances.push(AccountBalance::new_checked(
                total,
                Money::zero(total.currency),
                total,
            )?);
        }

        Ok(balances)
    }

    fn validate_token_addresses(&self) -> anyhow::Result<()> {
        let mut token_addresses = HashSet::with_capacity(self.token_balances.len());
        let mut duplicates = Vec::new();

        for balance in &self.token_balances {
            if !token_addresses.insert(balance.token.address) {
                duplicates.push(balance.token.address);
            }
        }

        if !duplicates.is_empty() {
            duplicates.sort_unstable();
            duplicates.dedup();
            anyhow::bail!(
                "Wallet balance snapshot contains duplicate token addresses: {}",
                format_addresses(&duplicates)
            );
        }

        let mut missing = self
            .token_universe
            .difference(&token_addresses)
            .copied()
            .collect::<Vec<_>>();
        let mut unexpected = token_addresses
            .difference(&self.token_universe)
            .copied()
            .collect::<Vec<_>>();
        missing.sort_unstable();
        unexpected.sort_unstable();

        match (missing.is_empty(), unexpected.is_empty()) {
            (false, false) => anyhow::bail!(
                "Wallet balance snapshot is missing configured token addresses: {}; contains unexpected token addresses: {}",
                format_addresses(&missing),
                format_addresses(&unexpected)
            ),
            (false, true) => anyhow::bail!(
                "Wallet balance snapshot is missing configured token addresses: {}",
                format_addresses(&missing)
            ),
            (true, false) => anyhow::bail!(
                "Wallet balance snapshot contains unexpected token addresses: {}",
                format_addresses(&unexpected)
            ),
            (true, true) => Ok(()),
        }
    }
}

fn format_addresses(addresses: &[Address]) -> String {
    addresses
        .iter()
        .map(ToString::to_string)
        .collect::<Vec<_>>()
        .join(", ")
}

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
        let amount = U256::from(92_220_728_254_u64);
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
        let amount = U256::from(758_325_512_078_001_391_u64);
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
        let amount = U256::from(92_220_728_254_u64); // 92220.728254 USDC
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

        let eth_balance = Money::new(50.936_054, crate::types::Currency::ETH());
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

    #[rstest]
    fn test_replace_balances_retains_snapshot_on_conversion_failure(weth: Token) {
        let mut wallet = WalletBalance::new(HashSet::from([weth.address]));
        let native_currency = weth.chain.native_currency();
        let native = Money::from_wei(U256::from(1_000_000_000_000_000_000_u64), native_currency);
        let token = TokenBalance::new(U256::from(2_000_000_000_000_000_000_u64), weth.clone());
        wallet.replace_balances(native, vec![token]).unwrap();

        let error = wallet
            .replace_balances(
                Money::from_wei(U256::from(3_000_000_000_000_000_000_u64), native_currency),
                vec![TokenBalance::new(U256::MAX, weth)],
            )
            .unwrap_err();

        assert!(
            error.to_string().contains("exceeds QuantityRaw range"),
            "was: {error}"
        );
        assert_eq!(wallet.native_currency.unwrap(), native);
        assert_eq!(wallet.token_balances.len(), 1);
        assert_eq!(
            wallet.token_balances[0].amount,
            U256::from(2_000_000_000_000_000_000_u64)
        );
    }

    #[rstest]
    fn test_account_balances_reports_missing_token_address(usdc: Token, weth: Token) {
        let missing = weth.address;
        let wallet = WalletBalance {
            native_currency: None,
            token_balances: vec![TokenBalance::new(U256::from(1_u64), usdc.clone())],
            token_universe: HashSet::from([usdc.address, missing]),
        };

        let error = wallet.as_account_balances().unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Wallet balance snapshot is missing configured token addresses: {missing}")
        );
    }

    #[rstest]
    fn test_account_balances_reports_unexpected_token_address(usdc: Token, weth: Token) {
        let unexpected = weth.address;
        let wallet = WalletBalance {
            native_currency: None,
            token_balances: vec![
                TokenBalance::new(U256::from(1_u64), usdc.clone()),
                TokenBalance::new(U256::from(2_u64), weth),
            ],
            token_universe: HashSet::from([usdc.address]),
        };

        let error = wallet.as_account_balances().unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Wallet balance snapshot contains unexpected token addresses: {unexpected}")
        );
    }

    #[rstest]
    fn test_account_balances_reports_missing_and_unexpected_token_addresses(
        usdc: Token,
        weth: Token,
    ) {
        let missing = usdc.address;
        let unexpected = weth.address;
        let wallet = WalletBalance {
            native_currency: None,
            token_balances: vec![TokenBalance::new(U256::from(1_u64), weth)],
            token_universe: HashSet::from([missing]),
        };

        let error = wallet.as_account_balances().unwrap_err();

        assert_eq!(
            error.to_string(),
            format!(
                "Wallet balance snapshot is missing configured token addresses: {missing}; contains unexpected token addresses: {unexpected}"
            )
        );
    }

    #[rstest]
    fn test_account_balances_reports_duplicate_before_set_differences(usdc: Token, weth: Token) {
        let duplicate = usdc.address;
        let wallet = WalletBalance {
            native_currency: None,
            token_balances: vec![
                TokenBalance::new(U256::from(1_u64), usdc.clone()),
                TokenBalance::new(U256::from(2_u64), usdc),
            ],
            token_universe: HashSet::from([weth.address]),
        };

        let error = wallet.as_account_balances().unwrap_err();

        assert_eq!(
            error.to_string(),
            format!("Wallet balance snapshot contains duplicate token addresses: {duplicate}")
        );
    }

    #[rstest]
    fn test_replace_balances_rejects_duplicate_currency_symbols(
        usdc: Token,
        #[from(arbitrum)] chain: SharedChain,
    ) {
        let duplicate = Token::new(
            chain,
            address!("0x0000000000000000000000000000000000000001"),
            "Other USD Coin".to_string(),
            "USDC".to_string(),
            18,
        );
        let mut wallet = WalletBalance::new(HashSet::from([usdc.address, duplicate.address]));

        let error = wallet
            .replace_balances(
                Money::from_wei(
                    U256::from(1_000_000_000_000_000_000_u64),
                    usdc.chain.native_currency(),
                ),
                vec![
                    TokenBalance::new(U256::from(1_u64), usdc),
                    TokenBalance::new(U256::from(2_u64), duplicate),
                ],
            )
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Wallet balance snapshot contains duplicate currency USDC"
        );
        assert!(wallet.native_currency.is_none());
        assert!(wallet.token_balances.is_empty());
    }
}
