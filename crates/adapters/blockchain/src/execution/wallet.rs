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

use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    time::{Duration, Instant},
};

use alloy::primitives::{Address, U256};
use nautilus_model::{
    defi::{
        SharedChain, Token,
        wallet::{TokenBalance, WalletBalance},
    },
    enums::CurrencyType,
    types::{
        AccountBalance, Currency, Money,
        money::{MONEY_RAW_MAX, MONEY_RAW_MIN, MoneyRaw},
    },
};

use crate::{
    contracts::erc20::Erc20Contract,
    rpc::{error::BlockchainRpcClientError, http::BlockchainHttpRpcClient},
};

const DEFAULT_WALLET_SNAPSHOT_TTL_SECS: u64 = 30;
const DEFAULT_WALLET_MAX_TOKENS_PER_REFRESH: usize = 256;
const DEFAULT_MULTICALL_MAX_BATCH_SIZE: usize = 64;
const DEFAULT_MULTICALL_MIN_BATCH_SIZE: usize = 4;

const SPLIT_ERROR_HINTS: [&str; 14] = [
    "out of gas",
    "query returned more than",
    "too many results",
    "response size exceeded",
    "log response size exceeded",
    "please reduce",
    "result window is too large",
    "max block range",
    "block range is too wide",
    "request exceeds",
    "please narrow your query",
    "request entity too large",
    "payload too large",
    "timeout",
];

/// Configures wallet refresh behavior for execution preflight and account snapshots.
#[derive(Debug, Clone)]
pub struct WalletTrackerConfig {
    /// Optional list of spender addresses used for allowance refresh.
    pub allowance_spenders: Vec<Address>,
    /// Maximum staleness of a wallet snapshot before refresh is required.
    pub snapshot_ttl: Duration,
    /// Hard cap on tracked tokens per refresh cycle.
    pub max_tokens_per_refresh: usize,
    /// Maximum tokens per multicall batch.
    pub multicall_max_batch_size: usize,
    /// Minimum token batch size when adaptively splitting provider-limit errors.
    pub multicall_min_batch_size: usize,
}

impl Default for WalletTrackerConfig {
    fn default() -> Self {
        Self {
            allowance_spenders: Vec::new(),
            snapshot_ttl: Duration::from_secs(DEFAULT_WALLET_SNAPSHOT_TTL_SECS),
            max_tokens_per_refresh: DEFAULT_WALLET_MAX_TOKENS_PER_REFRESH,
            multicall_max_batch_size: DEFAULT_MULTICALL_MAX_BATCH_SIZE,
            multicall_min_batch_size: DEFAULT_MULTICALL_MIN_BATCH_SIZE,
        }
    }
}

/// Summary emitted after each deterministic wallet refresh cycle.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WalletRefreshSummary {
    /// Number of tokens refreshed.
    pub token_count: usize,
    /// Number of spenders refreshed for allowance snapshots.
    pub spender_count: usize,
}

/// Tracks wallet balances and allowances with deterministic refresh semantics.
#[derive(Debug)]
pub struct WalletTracker {
    chain: SharedChain,
    wallet_address: Address,
    token_universe: BTreeSet<Address>,
    config: WalletTrackerConfig,
    known_tokens: HashMap<Address, Token>,
    wallet_balance: WalletBalance,
    allowances: BTreeMap<(Address, Address), U256>,
    last_refresh_at: Option<Instant>,
}

impl WalletTracker {
    /// Creates a new wallet tracker.
    #[must_use]
    pub fn new(
        chain: SharedChain,
        wallet_address: Address,
        token_universe: HashSet<Address>,
        config: WalletTrackerConfig,
    ) -> Self {
        let mut ordered_universe = BTreeSet::new();
        ordered_universe.extend(token_universe.iter().copied());

        Self {
            chain,
            wallet_address,
            token_universe: ordered_universe,
            config,
            known_tokens: HashMap::new(),
            wallet_balance: WalletBalance::new(token_universe),
            allowances: BTreeMap::new(),
            last_refresh_at: None,
        }
    }

    /// Returns `true` if the snapshot is stale or missing and should be refreshed.
    #[must_use]
    pub fn needs_refresh(&self) -> bool {
        match self.last_refresh_at {
            Some(last) => last.elapsed() >= self.config.snapshot_ttl,
            None => true,
        }
    }

    /// Returns the latest tracked wallet balance snapshot.
    #[must_use]
    pub const fn wallet_balance(&self) -> &WalletBalance {
        &self.wallet_balance
    }

    /// Returns the latest tracked allowance snapshot by (`token`, `spender`).
    #[must_use]
    pub const fn allowances(&self) -> &BTreeMap<(Address, Address), U256> {
        &self.allowances
    }

    /// Seeds token metadata directly (useful for tests or pre-discovered pools).
    pub fn seed_token_metadata(&mut self, token: Token) {
        self.known_tokens.insert(token.address, token);
    }

    /// Produces deterministic `AccountBalance` rows for account-state snapshots.
    ///
    /// # Errors
    ///
    /// Returns an error if a token amount cannot be represented as `Money`.
    pub fn account_balances(&self) -> anyhow::Result<Vec<AccountBalance>> {
        let mut balances = Vec::with_capacity(self.wallet_balance.token_balances.len() + 1);

        if let Some(native) = self.wallet_balance.native_currency {
            balances.push(AccountBalance::new(
                native,
                Money::from_raw(0, native.currency),
                native,
            ));
        }

        for token_balance in &self.wallet_balance.token_balances {
            let currency = token_currency(token_balance);
            let total = token_amount_to_money(
                token_balance.amount,
                token_balance.token.decimals,
                currency,
            )?;
            balances.push(AccountBalance::new(
                total,
                Money::from_raw(0, currency),
                total,
            ));
        }

        balances.sort_by_key(|balance| balance.currency.code.to_string());
        Ok(balances)
    }

    /// Refreshes native balance, token balances, and allowances in one deterministic pass.
    ///
    /// # Errors
    ///
    /// Returns an error if RPC requests fail or if configured limits are exceeded.
    pub async fn refresh(
        &mut self,
        rpc_client: &BlockchainHttpRpcClient,
        erc20_contract: &Erc20Contract,
    ) -> anyhow::Result<WalletRefreshSummary> {
        if self.token_universe.len() > self.config.max_tokens_per_refresh {
            anyhow::bail!(
                "Token universe size {} exceeds wallet_max_tokens_per_refresh={}",
                self.token_universe.len(),
                self.config.max_tokens_per_refresh
            );
        }

        let native_wei = rpc_client.get_balance(&self.wallet_address, None).await?;
        let native_balance = Money::from_wei(native_wei, self.chain.native_currency());

        let token_addresses: Vec<Address> = self.token_universe.iter().copied().collect();
        self.ensure_token_metadata(erc20_contract, &token_addresses)
            .await?;

        let balance_snapshot = self
            .fetch_balances_with_adaptive_split(erc20_contract, &token_addresses)
            .await?;

        let mut allowance_snapshot = BTreeMap::new();
        for spender in &self.config.allowance_spenders {
            let allowances = self
                .fetch_allowances_with_adaptive_split(erc20_contract, &token_addresses, spender)
                .await?;
            for (token_address, allowance) in allowances {
                allowance_snapshot.insert((token_address, *spender), allowance);
            }
        }

        let mut token_balances = Vec::with_capacity(token_addresses.len());
        for token_address in token_addresses {
            let token = self
                .known_tokens
                .get(&token_address)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Missing token metadata for {token_address}"))?;
            let amount = balance_snapshot
                .get(&token_address)
                .copied()
                .unwrap_or(U256::ZERO);
            token_balances.push(TokenBalance::new(amount, token));
        }
        token_balances.sort_by_key(|balance| balance.token.address);

        // Deterministic replacement: clear+replace token rows every cycle.
        self.wallet_balance
            .set_native_currency_balance(native_balance);
        self.wallet_balance.token_balances = token_balances;
        self.allowances = allowance_snapshot;
        self.last_refresh_at = Some(Instant::now());

        Ok(WalletRefreshSummary {
            token_count: self.wallet_balance.token_balances.len(),
            spender_count: self.config.allowance_spenders.len(),
        })
    }

    async fn ensure_token_metadata(
        &mut self,
        erc20_contract: &Erc20Contract,
        token_addresses: &[Address],
    ) -> anyhow::Result<()> {
        let missing_tokens: Vec<Address> = token_addresses
            .iter()
            .copied()
            .filter(|token| !self.known_tokens.contains_key(token))
            .collect();

        if missing_tokens.is_empty() {
            return Ok(());
        }

        let infos = erc20_contract
            .batch_fetch_token_info(&missing_tokens)
            .await?;
        for token_address in missing_tokens {
            let token_info = infos
                .get(&token_address)
                .ok_or_else(|| anyhow::anyhow!("Missing token info for {token_address}"))?
                .as_ref()
                .map_err(|e| anyhow::anyhow!("Failed token metadata for {token_address}: {e}"))?;

            let token = Token::new(
                self.chain.clone(),
                token_address,
                token_info.name.clone(),
                token_info.symbol.clone(),
                token_info.decimals,
            );
            self.known_tokens.insert(token.address, token);
        }

        Ok(())
    }

    async fn fetch_balances_with_adaptive_split(
        &self,
        erc20_contract: &Erc20Contract,
        token_addresses: &[Address],
    ) -> anyhow::Result<HashMap<Address, U256>> {
        self.run_chunked(token_addresses, |chunk| async move {
            erc20_contract
                .batch_balance_of(&chunk, &self.wallet_address)
                .await
        })
        .await
    }

    async fn fetch_allowances_with_adaptive_split(
        &self,
        erc20_contract: &Erc20Contract,
        token_addresses: &[Address],
        spender: &Address,
    ) -> anyhow::Result<HashMap<Address, U256>> {
        self.run_chunked(token_addresses, |chunk| async move {
            erc20_contract
                .batch_allowance(&chunk, &self.wallet_address, spender)
                .await
        })
        .await
    }

    async fn run_chunked<F, Fut>(
        &self,
        token_addresses: &[Address],
        mut call: F,
    ) -> anyhow::Result<HashMap<Address, U256>>
    where
        F: FnMut(Vec<Address>) -> Fut,
        Fut: std::future::Future<Output = Result<HashMap<Address, U256>, BlockchainRpcClientError>>,
    {
        if token_addresses.is_empty() {
            return Ok(HashMap::new());
        }

        let mut pending_chunks: Vec<Vec<Address>> = token_addresses
            .chunks(self.config.multicall_max_batch_size.max(1))
            .map(|chunk| chunk.to_vec())
            .collect();

        let mut merged = HashMap::with_capacity(token_addresses.len());

        while let Some(chunk) = pending_chunks.pop() {
            match call(chunk.clone()).await {
                Ok(values) => merged.extend(values),
                Err(e) => {
                    let min_batch = self.config.multicall_min_batch_size.max(1);
                    if chunk.len() > min_batch && should_split_on_error(e.to_string().as_str()) {
                        let midpoint = chunk.len() / 2;
                        let right = chunk[midpoint..].to_vec();
                        let left = chunk[..midpoint].to_vec();
                        if !right.is_empty() {
                            pending_chunks.push(right);
                        }
                        if !left.is_empty() {
                            pending_chunks.push(left);
                        }
                    } else {
                        anyhow::bail!("Wallet multicall chunk failed (size={}): {e}", chunk.len());
                    }
                }
            }
        }

        Ok(merged)
    }
}

fn token_currency(token_balance: &TokenBalance) -> Currency {
    let token = &token_balance.token;
    let precision = token.decimals.min(16);
    let address = token.address.to_string();

    let mut base_code = token.symbol.trim().to_ascii_uppercase();
    if base_code.is_empty() {
        base_code = format!("TKN{}", &address[2..8]).to_ascii_uppercase();
    }

    for suffix_index in 0_u32.. {
        let code = match suffix_index {
            0 => base_code.clone(),
            1 => format!("{base_code}{}", &address[2..8]).to_ascii_uppercase(),
            2 => format!("TKN{}", &address[2..10]).to_ascii_uppercase(),
            _ => format!("TKN{}{suffix_index}", &address[2..10]).to_ascii_uppercase(),
        };

        if let Some(existing) = Currency::try_from_str(code.as_str()) {
            if existing.precision == precision {
                return existing;
            }
            continue;
        }

        let name = if token.name.trim().is_empty() {
            code.clone()
        } else {
            token.name.clone()
        };

        let currency = Currency::new(
            code.as_str(),
            precision,
            0,
            name.as_str(),
            CurrencyType::Crypto,
        );
        if let Err(e) = Currency::register(currency, false) {
            log::warn!("Failed to register token currency {code}: {e}");
        }

        match Currency::try_from_str(code.as_str()) {
            Some(existing) if existing.precision == precision => return existing,
            Some(_) => continue,
            None => return currency,
        }
    }

    unreachable!("token currency code generation should always terminate")
}

fn token_amount_to_money(
    amount: U256,
    token_decimals: u8,
    currency: Currency,
) -> anyhow::Result<Money> {
    let target_precision = currency.precision;
    let scaled = if token_decimals > target_precision {
        let divisor = pow10(token_decimals - target_precision);
        amount / divisor
    } else if token_decimals < target_precision {
        let multiplier = pow10(target_precision - token_decimals);
        amount.checked_mul(multiplier).ok_or_else(|| {
            anyhow::anyhow!("Token amount overflow while scaling to Money precision")
        })?
    } else {
        amount
    };

    let raw_i128 = i128::try_from(scaled).map_err(|_| {
        anyhow::anyhow!(
            "Token amount {scaled} exceeds Money raw range for currency {}",
            currency
        )
    })?;
    let raw: MoneyRaw = raw_i128
        .try_into()
        .map_err(|_| anyhow::anyhow!("Token amount exceeds MoneyRaw range"))?;
    if !(MONEY_RAW_MIN..=MONEY_RAW_MAX).contains(&raw) {
        anyhow::bail!("Token amount exceeds Money bounds [{MONEY_RAW_MIN}, {MONEY_RAW_MAX}]");
    }

    Ok(Money::from_raw(raw, currency))
}

fn pow10(exp: u8) -> U256 {
    U256::from(10u8).pow(U256::from(exp))
}

fn should_split_on_error(message: &str) -> bool {
    let lowered = message.to_ascii_lowercase();
    SPLIT_ERROR_HINTS
        .iter()
        .any(|needle| lowered.contains(needle))
}

#[cfg(test)]
mod tests {
    use alloy::primitives::{Address, U256};
    use nautilus_model::{
        currencies::CURRENCY_MAP,
        defi::{chain::chains, wallet::TokenBalance},
        enums::CurrencyType,
        types::{Currency, Money, fixed::FIXED_PRECISION},
    };

    use super::{token_amount_to_money, token_currency};

    fn make_token(address: Address, symbol: &str, decimals: u8) -> nautilus_model::defi::Token {
        nautilus_model::defi::Token::new(
            std::sync::Arc::new(chains::BSC.clone()),
            address,
            symbol.to_string(),
            symbol.to_string(),
            decimals,
        )
    }

    #[test]
    fn test_token_currency_avoids_symbol_collisions() {
        let token_a = make_token(
            "0x1111111111111111111111111111111111111111"
                .parse()
                .expect("valid token address"),
            "USDT",
            18,
        );
        let token_b = make_token(
            "0x2222222222222222222222222222222222222222"
                .parse()
                .expect("valid token address"),
            "USDT",
            18,
        );

        let currency_a = token_currency(&TokenBalance::new(U256::from(1u8), token_a));
        let currency_b = token_currency(&TokenBalance::new(U256::from(1u8), token_b));

        assert_ne!(currency_a.code, currency_b.code);
    }

    #[test]
    fn test_token_currency_skips_registered_code_with_mismatched_precision() {
        let address: Address = "0xaabbccddeeff00112233445566778899aabbccdd"
            .parse()
            .expect("valid token address");
        let token = make_token(address, "PCSFIX", 6);

        let base_code = "PCSFIX";
        let second_code = format!("{base_code}{}", &address.to_string()[2..8]).to_ascii_uppercase();
        let final_code = format!("TKN{}", &address.to_string()[2..10]).to_ascii_uppercase();

        let base_currency = Currency::new(
            base_code.to_string(),
            18,
            0,
            format!("{base_code} existing"),
            CurrencyType::Crypto,
        );
        Currency::register(base_currency, true).expect("should register base currency");

        let second_currency = Currency::new(
            second_code.clone(),
            18,
            0,
            format!("{second_code} existing"),
            CurrencyType::Crypto,
        );
        Currency::register(second_currency, true).expect("should register second currency");

        let final_currency = Currency::new(
            final_code.clone(),
            18,
            0,
            format!("{final_code} existing"),
            CurrencyType::Crypto,
        );
        Currency::register(final_currency, true).expect("should register final currency");

        let currency = token_currency(&TokenBalance::new(U256::from(1u8), token));

        assert_eq!(currency.precision, 6);
        assert_ne!(currency.code.as_str(), final_code);

        let map = CURRENCY_MAP
            .lock()
            .expect("currency map lock should succeed");
        assert_eq!(
            map.get(currency.code.as_str())
                .expect("generated currency should be registered")
                .precision,
            6
        );
    }

    #[test]
    fn test_token_amount_to_money_large_raw_returns_error() {
        let currency = Currency::new(
            "TKNTEST",
            FIXED_PRECISION,
            0,
            "Token Test",
            nautilus_model::enums::CurrencyType::Crypto,
        );
        let too_large_raw = nautilus_model::types::money::MONEY_RAW_MAX
            .saturating_add(1)
            .unsigned_abs();
        let amount = U256::from(too_large_raw);

        let result = token_amount_to_money(amount, FIXED_PRECISION, currency);

        assert!(result.is_err());
        let _ = Money::from_raw(0, currency);
    }
}
