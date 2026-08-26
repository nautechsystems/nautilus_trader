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

//! A blockchain wallet account holding unleveraged native and ERC-20 token balances.
//!
//! The account is multi-currency with no base currency, no margin entries, and no borrowing:
//! every reported `total` is the observed on-chain balance and negative totals are rejected.
//! ERC-20 allowances are spender authorizations and are not represented as balances or locked
//! funds.
//!
//! # Balance locking
//!
//! Locked balances track local pending-order reservations per `(InstrumentId, Currency)`,
//! without changing the reported on-chain totals: `free = total - locked`. Account state events
//! contribute totals only; locked and free balances are always derived from local reservations.
//! Rebuilding an instrument reservation clears its prior currency locks before applying the new
//! exact set. Computed BUY notionals for non-inverse, non-quanto instruments round up to the
//! observed currency's smallest unit so the reservation never understates the possible spend.
//!
//! # Graceful degradation
//!
//! When total locked exceeds total balance (e.g., due to on-chain state latency), the account
//! clamps locked to total rather than raising an error. This yields zero free balance,
//! preventing new orders while avoiding crashes in live trading.

use std::{
    cmp::Ordering,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use ahash::{AHashMap, AHashSet};
use indexmap::IndexMap;
use nautilus_core::correctness::{
    CorrectnessError, CorrectnessResult, CorrectnessResultExt, FAILED, check_predicate_false,
    check_predicate_true,
};
use ruint::aliases::U512;
use serde::{Deserialize, Deserializer, Serialize, de};

use crate::{
    accounts::{
        Account,
        base::{self, BaseAccount},
    },
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{
        AccountBalance, Currency, Money, Price, Quantity,
        fixed::{FIXED_PRECISION, check_fixed_raw_i128, check_fixed_raw_u128, raw_scale},
        money::MoneyRaw,
    },
};

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct WalletAccount {
    pub base: BaseAccount,
    /// Per-(instrument, currency) locked balances (transient, not persisted).
    #[serde(skip, default)]
    pub balances_locked: AHashMap<(InstrumentId, Currency), Money>,
}

impl WalletAccount {
    /// Creates a new [`WalletAccount`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the initial event is not a valid wallet account state.
    pub fn new_checked(
        mut event: AccountState,
        calculate_account_state: bool,
    ) -> CorrectnessResult<Self> {
        Self::validate_event(&event)?;
        event.balances = Self::normalize_balances(&event.balances)?;
        Ok(Self {
            base: BaseAccount::new(event, calculate_account_state),
            balances_locked: AHashMap::new(),
        })
    }

    /// Creates a new [`WalletAccount`] instance.
    ///
    /// # Panics
    ///
    /// Panics if the initial event is not a valid wallet account state.
    #[must_use]
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        Self::new_checked(event, calculate_account_state).expect_display(FAILED)
    }

    /// Updates the locked balance for the given instrument and currency.
    ///
    /// # Errors
    ///
    /// Returns an error if `locked` is negative, the wallet has no observed balance for its
    /// currency, or the local reservations cannot produce a valid balance.
    pub fn update_balance_locked(
        &mut self,
        instrument_id: InstrumentId,
        locked: Money,
    ) -> anyhow::Result<()> {
        let current_balance = self
            .base
            .balances
            .get(&locked.currency)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!("wallet has no observed balance for {}", locked.currency)
            })?;
        Self::validate_observed_balance(current_balance)?;
        let locked = Self::normalize_reservation(locked, current_balance.currency)?;
        let key = (instrument_id, current_balance.currency);
        let previous = self.balances_locked.remove_entry(&key);
        self.balances_locked.insert(key, locked);
        let balance = match Self::balance_from_locks_checked(current_balance, &self.balances_locked)
        {
            Ok(balance) => balance,
            Err(e) => {
                self.balances_locked.remove(&key);
                if let Some((previous_key, previous)) = previous {
                    self.balances_locked.insert(previous_key, previous);
                }

                return Err(e.into());
            }
        };
        self.base.balances.insert(current_balance.currency, balance);

        Ok(())
    }

    /// Clears all locked balances for the given instrument ID.
    pub fn clear_balance_locked(&mut self, instrument_id: InstrumentId) {
        let currencies = self
            .balances_locked
            .iter()
            .filter(|((id, _), _)| *id == instrument_id)
            .flat_map(|((_, key_currency), locked)| [*key_currency, locked.currency])
            .collect::<AHashSet<_>>();
        let mut balances_locked = self.balances_locked.clone();
        balances_locked.retain(|(id, _), _| *id != instrument_id);
        let mut balances = self.base.balances.clone();

        for currency in currencies {
            let Some(current_balance) = balances.get(&currency).copied() else {
                log::error!("Cannot clear wallet reservations: no observed balance for {currency}");
                return;
            };
            let balance = match Self::balance_from_locks_checked(current_balance, &balances_locked)
            {
                Ok(balance) => balance,
                Err(e) => {
                    log::error!("Cannot clear wallet reservations for {currency}: {e}");
                    return;
                }
            };
            balances.insert(current_balance.currency, balance);
        }

        self.base.balances = balances;
        self.balances_locked = balances_locked;
    }

    /// Updates the account balances, rejecting negative totals.
    ///
    /// A wallet balance is an observed on-chain amount and cannot become negative.
    ///
    /// # Errors
    ///
    /// Returns an error if any balance has a negative total, currencies are duplicated, or the
    /// local reservations cannot produce valid balances.
    pub fn update_balances(&mut self, balances: &[AccountBalance]) -> anyhow::Result<()> {
        let balances = Self::normalize_balances(balances)?
            .into_iter()
            .map(|balance| Self::balance_from_locks_checked(balance, &self.balances_locked))
            .collect::<CorrectnessResult<Vec<_>>>()?;
        self.base.update_balances(&balances);

        Ok(())
    }

    #[must_use]
    pub const fn is_unleveraged(&self) -> bool {
        true
    }

    /// Recalculates the account balance for the specified currency based on per-instrument locks.
    ///
    /// Sums all per-instrument locked amounts for the currency and updates the balance.
    /// If the total locked exceeds the total balance, clamps to total (free = 0).
    pub fn recalculate_balance(&mut self, currency: Currency) {
        let Some(current_balance) = self.base.balances.get(&currency).copied() else {
            log::debug!("Cannot recalculate balance when no current balance for {currency}");
            return;
        };

        match Self::balance_from_locks_checked(current_balance, &self.balances_locked) {
            Ok(balance) => {
                self.base.balances.insert(current_balance.currency, balance);
            }
            Err(e) => {
                log::error!("Cannot recalculate {currency} balance from reservations: {e}");
            }
        }
    }

    fn validate_event(event: &AccountState) -> CorrectnessResult<()> {
        check_predicate_true(
            event.account_type == AccountType::Wallet,
            "Wallet account event had a non-wallet account type",
        )?;
        check_predicate_true(
            event.base_currency.is_none(),
            "Wallet account event had a base currency",
        )?;
        check_predicate_true(
            event.margins.is_empty(),
            "Wallet account event had margin balances",
        )?;
        Ok(())
    }

    fn normalize_balances(balances: &[AccountBalance]) -> CorrectnessResult<Vec<AccountBalance>> {
        let mut currencies = AHashSet::new();

        balances
            .iter()
            .map(|balance| {
                check_predicate_true(
                    currencies.insert(balance.currency),
                    &format!(
                        "Wallet account balances had duplicate currency {}",
                        balance.currency
                    ),
                )?;
                check_predicate_false(
                    balance.total.raw < 0,
                    "Wallet account balance total was negative",
                )?;
                Self::validate_observed_balance(*balance)?;
                AccountBalance::new_checked(
                    balance.total,
                    Money::zero(balance.currency),
                    balance.total,
                )
            })
            .collect()
    }

    fn validate_observed_balance(balance: AccountBalance) -> CorrectnessResult<()> {
        check_predicate_true(
            balance.currency == balance.total.currency
                && balance.currency.precision == balance.total.currency.precision,
            &format!(
                "Wallet account balance currency {} precision {} differed from total currency {} precision {}",
                balance.currency,
                balance.currency.precision,
                balance.total.currency,
                balance.total.currency.precision,
            ),
        )?;
        Self::validate_money(balance.total)
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn validate_money(money: Money) -> CorrectnessResult<()> {
        Money::from_raw_checked(money.raw, money.currency)?;
        Self::validate_raw(i128::from(money.raw), money.currency.precision)
    }

    fn validate_raw(raw: i128, precision: u8) -> CorrectnessResult<()> {
        check_fixed_raw_i128(raw, precision).map_err(|e| CorrectnessError::PredicateViolation {
            message: e.to_string(),
        })
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn validate_quantity(quantity: Quantity) -> CorrectnessResult<()> {
        check_predicate_false(quantity.is_undefined(), "quantity was undefined")?;
        Quantity::from_raw_checked(quantity.raw, quantity.precision)?;
        check_fixed_raw_u128(u128::from(quantity.raw), quantity.precision).map_err(|e| {
            CorrectnessError::PredicateViolation {
                message: e.to_string(),
            }
        })
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn validate_price(price: Price) -> CorrectnessResult<()> {
        check_predicate_true(price.is_positive(), "price was not positive")?;
        Price::from_raw_checked(price.raw, price.precision)?;
        check_fixed_raw_i128(i128::from(price.raw), price.precision).map_err(|e| {
            CorrectnessError::PredicateViolation {
                message: e.to_string(),
            }
        })
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn normalize_reservation(locked: Money, currency: Currency) -> CorrectnessResult<Money> {
        check_predicate_false(
            locked.raw < 0,
            &format!("locked balance was negative: {locked}"),
        )?;
        Self::validate_money(locked)?;

        let source_precision = locked.currency.precision.max(FIXED_PRECISION);
        let target_precision = currency.precision.max(FIXED_PRECISION);
        let raw = i128::from(locked.raw);
        let raw = match source_precision.cmp(&target_precision) {
            Ordering::Less => {
                let scale = 10_i128.pow(u32::from(target_precision - source_precision));
                raw.checked_mul(scale)
                    .ok_or_else(|| CorrectnessError::PredicateViolation {
                        message: format!(
                            "wallet reservation for {currency} overflowed while increasing raw scale"
                        ),
                    })?
            }
            Ordering::Greater => {
                let scale = 10_i128.pow(u32::from(source_precision - target_precision));
                check_predicate_true(
                    raw % scale == 0,
                    &format!(
                        "wallet reservation for {currency} loses precision when decreasing raw scale"
                    ),
                )?;
                raw / scale
            }
            Ordering::Equal => raw,
        };
        Self::validate_raw(raw, currency.precision)?;
        let raw: MoneyRaw = raw
            .try_into()
            .map_err(|_| CorrectnessError::PredicateViolation {
                message: format!("wallet reservation for {currency} exceeds Money raw bounds"),
            })?;

        Money::from_raw_checked(raw, currency)
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn money_from_quantity(quantity: Quantity, currency: Currency) -> CorrectnessResult<Money> {
        Self::validate_quantity(quantity)?;
        let source_precision = quantity.precision.max(FIXED_PRECISION);
        let target_precision = currency.precision.max(FIXED_PRECISION);
        let raw = i128::try_from(u128::from(quantity.raw)).map_err(|_| {
            CorrectnessError::PredicateViolation {
                message: format!("quantity for {currency} exceeds signed raw bounds"),
            }
        })?;
        let raw = match source_precision.cmp(&target_precision) {
            Ordering::Less => {
                let scale = 10_i128.pow(u32::from(target_precision - source_precision));
                raw.checked_mul(scale)
                    .ok_or_else(|| CorrectnessError::PredicateViolation {
                        message: format!(
                            "quantity for {currency} overflowed while increasing raw scale"
                        ),
                    })?
            }
            Ordering::Greater => {
                let scale = 10_i128.pow(u32::from(source_precision - target_precision));
                check_predicate_true(
                    raw % scale == 0,
                    &format!("quantity for {currency} loses precision when decreasing raw scale"),
                )?;
                raw / scale
            }
            Ordering::Equal => raw,
        };
        Self::validate_raw(raw, currency.precision)?;
        let raw: MoneyRaw = raw
            .try_into()
            .map_err(|_| CorrectnessError::PredicateViolation {
                message: format!("quantity for {currency} exceeds Money raw bounds"),
            })?;

        Money::from_raw_checked(raw, currency)
    }

    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn calculate_notional_exact(
        instrument: &InstrumentAny,
        quantity: Quantity,
        price: Price,
        currency: Currency,
    ) -> CorrectnessResult<Money> {
        let multiplier = instrument.multiplier();
        Self::validate_quantity(quantity)?;
        Self::validate_quantity(multiplier)?;
        Self::validate_price(price)?;

        let quantity_raw = U512::from(quantity.raw);
        let multiplier_raw = U512::from(multiplier.raw);
        let price_raw = U512::from(u128::try_from(price.raw).map_err(|_| {
            CorrectnessError::PredicateViolation {
                message: "price raw value was negative".to_string(),
            }
        })?);
        let target_scale = U512::from(raw_scale(currency.precision));
        let numerator = quantity_raw
            .checked_mul(multiplier_raw)
            .and_then(|value| value.checked_mul(price_raw))
            .and_then(|value| value.checked_mul(target_scale))
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: "wallet notional numerator overflowed".to_string(),
            })?;
        let denominator = U512::from(raw_scale(quantity.precision))
            .checked_mul(U512::from(raw_scale(multiplier.precision)))
            .and_then(|value| value.checked_mul(U512::from(raw_scale(price.precision))))
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: "wallet notional denominator overflowed".to_string(),
            })?;
        let grid = raw_scale(currency.precision) / 10_u128.pow(u32::from(currency.precision));
        let denominator = denominator.checked_mul(U512::from(grid)).ok_or_else(|| {
            CorrectnessError::PredicateViolation {
                message: "wallet notional grid denominator overflowed".to_string(),
            }
        })?;
        let units = numerator / denominator;
        let units = if (numerator % denominator).is_zero() {
            units
        } else {
            units.checked_add(U512::from(1_u8)).ok_or_else(|| {
                CorrectnessError::PredicateViolation {
                    message: "wallet notional ceiling overflowed".to_string(),
                }
            })?
        };
        let raw = units.checked_mul(U512::from(grid)).ok_or_else(|| {
            CorrectnessError::PredicateViolation {
                message: "wallet notional raw value overflowed".to_string(),
            }
        })?;
        let raw = u128::try_from(raw).map_err(|_| CorrectnessError::PredicateViolation {
            message: format!("wallet notional for {currency} exceeds raw bounds"),
        })?;
        let raw: MoneyRaw = raw
            .try_into()
            .map_err(|_| CorrectnessError::PredicateViolation {
                message: format!("wallet notional for {currency} exceeds Money raw bounds"),
            })?;
        Self::validate_raw(i128::from(raw), currency.precision)?;

        Money::from_raw_checked(raw, currency)
    }

    fn balance_from_locks_checked(
        current_balance: AccountBalance,
        balances_locked: &AHashMap<(InstrumentId, Currency), Money>,
    ) -> CorrectnessResult<AccountBalance> {
        Self::validate_observed_balance(current_balance)?;
        let currency = current_balance.currency;
        let mut total_locked = Money::zero(currency);

        for ((_, key_currency), locked) in balances_locked
            .iter()
            .filter(|((_, key), locked)| *key == currency || locked.currency == currency)
        {
            check_predicate_true(
                *key_currency == locked.currency
                    && key_currency.precision == locked.currency.precision,
                &format!(
                    "wallet reservation key currency {} precision {} differed from value currency {} precision {}",
                    key_currency,
                    key_currency.precision,
                    locked.currency,
                    locked.currency.precision,
                ),
            )?;
            check_predicate_true(
                locked.currency.precision == currency.precision,
                &format!(
                    "locked balance precision {} differed from balance precision {} for {currency}",
                    locked.currency.precision, currency.precision
                ),
            )?;
            check_predicate_false(
                locked.raw < 0,
                &format!("locked balance was negative: {locked}"),
            )?;
            Self::validate_money(*locked)?;
            total_locked = total_locked.checked_add(*locked).ok_or_else(|| {
                CorrectnessError::PredicateViolation {
                    message: format!("{currency} wallet reservation total exceeds Money bounds"),
                }
            })?;
        }

        base::balance_from_locks(current_balance, balances_locked)
    }

    fn from_base_checked(mut base: BaseAccount) -> CorrectnessResult<Self> {
        check_predicate_true(
            base.account_type == AccountType::Wallet,
            "Wallet account had a non-wallet account type",
        )?;
        check_predicate_true(
            base.base_currency.is_none(),
            "Wallet account had a base currency",
        )?;
        check_predicate_false(base.events.is_empty(), "Wallet account had no events")?;

        for event in &base.events {
            Self::validate_event(event)?;
            Self::normalize_balances(&event.balances)?;
            check_predicate_true(
                event.account_id == base.id,
                "Wallet account event had a different account ID",
            )?;
        }

        for starting in base.balances_starting.values() {
            check_predicate_false(
                starting.raw < 0,
                "Wallet account starting balance was negative",
            )?;
        }

        let balances = base.balances.values().copied().collect::<Vec<_>>();
        base.balances = Self::normalize_balances(&balances)?
            .into_iter()
            .map(|balance| (balance.currency, balance))
            .collect();

        Ok(Self {
            base,
            balances_locked: AHashMap::new(),
        })
    }
}

#[derive(Deserialize)]
struct WalletAccountSerde {
    base: BaseAccount,
}

impl<'de> Deserialize<'de> for WalletAccount {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let account = WalletAccountSerde::deserialize(deserializer)?;
        Self::from_base_checked(account.base).map_err(de::Error::custom)
    }
}

impl Account for WalletAccount {
    fn id(&self) -> AccountId {
        self.id
    }

    fn account_type(&self) -> AccountType {
        self.account_type
    }

    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    fn is_cash_account(&self) -> bool {
        self.account_type == AccountType::Cash
    }

    fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    fn calculated_account_state(&self) -> bool {
        self.calculate_account_state
    }

    fn balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_total(currency)
    }

    fn balances_total(&self) -> IndexMap<Currency, Money> {
        self.base_balances_total()
    }

    fn balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_free(currency)
    }

    fn balances_free(&self) -> IndexMap<Currency, Money> {
        self.base_balances_free()
    }

    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_locked(currency)
    }

    fn balances_locked(&self) -> IndexMap<Currency, Money> {
        self.base_balances_locked()
    }

    fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        self.base_balance(currency)
    }

    fn last_event(&self) -> Option<AccountState> {
        self.base_last_event()
    }

    fn events(&self) -> Vec<AccountState> {
        self.events.clone()
    }

    fn event_count(&self) -> usize {
        self.events.len()
    }

    fn currencies(&self) -> Vec<Currency> {
        self.balances.keys().copied().collect()
    }

    fn starting_balances(&self) -> IndexMap<Currency, Money> {
        self.balances_starting.clone()
    }

    fn balances(&self) -> IndexMap<Currency, AccountBalance> {
        self.balances.clone()
    }

    fn apply(&mut self, event: AccountState) -> anyhow::Result<()> {
        check_predicate_true(
            event.account_id == self.id,
            "Wallet account event had a different account ID",
        )?;
        Self::validate_event(&event)?;
        let mut event = event;
        event.balances = Self::normalize_balances(&event.balances)?
            .into_iter()
            .map(|balance| Self::balance_from_locks_checked(balance, &self.balances_locked))
            .collect::<CorrectnessResult<Vec<_>>>()?;
        self.base_apply(event);

        Ok(())
    }

    fn purge_account_events(&mut self, ts_now: nautilus_core::UnixNanos, lookback_secs: u64) {
        self.base.base_purge_account_events(ts_now, lookback_secs);
    }

    fn calculate_balance_locked(
        &mut self,
        instrument: &InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let base_currency = instrument
            .base_currency()
            .unwrap_or(instrument.quote_currency());
        let source_currency = if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false)
        {
            base_currency
        } else if side == OrderSide::Buy {
            instrument.quote_currency()
        } else if side == OrderSide::Sell {
            base_currency
        } else {
            anyhow::bail!("Invalid `OrderSide` in `calculate_balance_locked`: {side}")
        };
        let current_balance = self
            .base
            .balances
            .get(&source_currency)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!("wallet has no observed balance for {source_currency}")
            })?;
        Self::validate_observed_balance(current_balance)?;

        if side == OrderSide::Sell {
            return Self::money_from_quantity(quantity, current_balance.currency)
                .map_err(Into::into);
        }

        Self::validate_quantity(quantity)?;
        Self::validate_price(price)?;

        if !instrument.is_inverse() && !instrument.is_quanto() {
            return Self::calculate_notional_exact(
                instrument,
                quantity,
                price,
                current_balance.currency,
            )
            .map_err(Into::into);
        }

        let locked = self.base_calculate_balance_locked(
            instrument,
            side,
            quantity,
            price,
            use_quote_for_inverse,
        )?;
        Self::normalize_reservation(locked, current_balance.currency).map_err(Into::into)
    }

    fn calculate_pnls(
        &self,
        instrument: &InstrumentAny,
        fill: &OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        self.base_calculate_pnls(instrument, fill, position)
    }

    fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.base_calculate_commission(
            instrument,
            last_qty,
            last_px,
            liquidity_side,
            use_quote_for_inverse,
        )
    }
}

impl Deref for WalletAccount {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for WalletAccount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl PartialEq for WalletAccount {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for WalletAccount {}

impl Display for WalletAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "WalletAccount(id={}, type={}, base={})",
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexMap;
    use rstest::rstest;

    use crate::{
        accounts::{Account, WalletAccount, stubs::*},
        enums::{AccountType, LiquiditySide, OrderSide},
        events::{AccountState, account::stubs::*},
        identifiers::{AccountId, InstrumentId, stubs::uuid4},
        instruments::{CurrencyPair, Instrument, stubs::*},
        orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{
            AccountBalance, Currency, Money, Price, Quantity,
            money::{MONEY_RAW_MAX, MoneyRaw},
        },
    };
    #[cfg(feature = "defi")]
    use crate::{enums::CurrencyType, identifiers::Symbol, types::fixed::FIXED_PRECISION};

    #[rstest]
    fn test_display(wallet_account: WalletAccount) {
        assert_eq!(
            format!("{wallet_account}"),
            "WalletAccount(id=SIM-001, type=WALLET, base=None)"
        );
    }

    #[rstest]
    fn test_instantiate_multi_currency_wallet_account(
        wallet_account: WalletAccount,
        wallet_account_state: AccountState,
    ) {
        assert_eq!(wallet_account.id, AccountId::from("SIM-001"));
        assert_eq!(wallet_account.account_type, AccountType::Wallet);
        assert_eq!(wallet_account.base_currency, None);
        assert!(wallet_account.is_unleveraged());
        assert!(!wallet_account.is_cash_account());
        assert!(!wallet_account.is_margin_account());
        assert_eq!(
            wallet_account.last_event(),
            Some(wallet_account_state.clone())
        );
        assert_eq!(wallet_account.events(), vec![wallet_account_state]);
        assert_eq!(wallet_account.event_count(), 1);
        assert_eq!(
            wallet_account.balance_total(Some(Currency::ETH())),
            Some(Money::from("10 ETH"))
        );
        assert_eq!(
            wallet_account.balance_total(Some(Currency::USDC())),
            Some(Money::from("25000 USDC"))
        );
        assert_eq!(
            wallet_account.balance_free(Some(Currency::ETH())),
            Some(Money::from("10 ETH"))
        );
        assert_eq!(
            wallet_account.balance_locked(Some(Currency::USDC())),
            Some(Money::from("0 USDC"))
        );

        let mut balances_total_expected = IndexMap::new();
        balances_total_expected.insert(Currency::ETH(), Money::from("10 ETH"));
        balances_total_expected.insert(Currency::USDC(), Money::from("25000 USDC"));
        assert_eq!(wallet_account.balances_total(), balances_total_expected);

        let mut starting_balances_expected = IndexMap::new();
        starting_balances_expected.insert(Currency::ETH(), Money::from("10 ETH"));
        starting_balances_expected.insert(Currency::USDC(), Money::from("25000 USDC"));
        assert_eq!(
            wallet_account.starting_balances(),
            starting_balances_expected
        );
    }

    #[rstest]
    fn test_apply_given_new_state_event_updates_correctly(
        mut wallet_account: WalletAccount,
        wallet_account_state: AccountState,
        wallet_account_state_changed: AccountState,
    ) {
        wallet_account
            .apply(wallet_account_state_changed.clone())
            .unwrap();

        assert_eq!(
            wallet_account.last_event(),
            Some(wallet_account_state_changed.clone())
        );
        assert_eq!(
            wallet_account.events,
            vec![wallet_account_state, wallet_account_state_changed]
        );
        assert_eq!(wallet_account.event_count(), 2);
        assert_eq!(
            wallet_account.balance_total(Some(Currency::ETH())),
            Some(Money::from("9.5 ETH"))
        );
        assert_eq!(
            wallet_account.balance_locked(Some(Currency::ETH())),
            Some(Money::from("0 ETH"))
        );
        assert_eq!(
            wallet_account.balance_free(Some(Currency::ETH())),
            Some(Money::from("9.5 ETH"))
        );
        assert_eq!(
            wallet_account.balance_total(Some(Currency::USDC())),
            Some(Money::from("30000 USDC"))
        );
    }

    #[rstest]
    fn test_apply_rejects_negative_balance(mut wallet_account: WalletAccount) {
        let negative_state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::from("-1 ETH"),
                Money::from("0 ETH"),
                Money::from("-1 ETH"),
            )],
            vec![],
            false,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        let result = wallet_account.apply(negative_state);
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Wallet account balance total was negative"
        );
    }

    #[rstest]
    fn test_apply_rejects_different_account_without_mutation(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
        mut wallet_account_state_changed: AccountState,
    ) {
        wallet_account
            .update_balance_locked(currency_pair_btcusdt.id, Money::from("2 ETH"))
            .unwrap();
        let events_before = wallet_account.events.clone();
        let balances_before = wallet_account.balances.clone();
        let locks_before = wallet_account.balances_locked.clone();
        wallet_account_state_changed.account_id = AccountId::from("OTHER-001");

        let result = wallet_account.apply(wallet_account_state_changed);

        assert_eq!(
            result.unwrap_err().to_string(),
            "Wallet account event had a different account ID"
        );
        assert_eq!(wallet_account.events, events_before);
        assert_eq!(wallet_account.balances, balances_before);
        assert_eq!(wallet_account.balances_locked, locks_before);
    }

    #[rstest]
    fn test_apply_rejects_duplicate_currency_without_mutation(
        mut wallet_account: WalletAccount,
        mut wallet_account_state_changed: AccountState,
    ) {
        let events_before = wallet_account.events.clone();
        let balances_before = wallet_account.balances.clone();
        let duplicate = wallet_account_state_changed.balances[0];
        wallet_account_state_changed.balances.push(duplicate);

        let result = wallet_account.apply(wallet_account_state_changed);

        assert_eq!(
            result.unwrap_err().to_string(),
            "Wallet account balances had duplicate currency ETH"
        );
        assert_eq!(wallet_account.events, events_before);
        assert_eq!(wallet_account.balances, balances_before);
    }

    #[rstest]
    fn test_apply_rejects_negative_local_lock_without_mutation(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
        wallet_account_state_changed: AccountState,
    ) {
        wallet_account.balances_locked.insert(
            (currency_pair_btcusdt.id, Currency::ETH()),
            Money::from("-1 ETH"),
        );
        let events_before = wallet_account.events.clone();
        let balances_before = wallet_account.balances.clone();
        let locks_before = wallet_account.balances_locked.clone();

        let result = wallet_account.apply(wallet_account_state_changed);

        assert_eq!(
            result.unwrap_err().to_string(),
            "locked balance was negative: -1.00000000 ETH"
        );
        assert_eq!(wallet_account.events, events_before);
        assert_eq!(wallet_account.balances, balances_before);
        assert_eq!(wallet_account.balances_locked, locks_before);
    }

    #[rstest]
    fn test_update_balances_rejects_negative_total(mut wallet_account: WalletAccount) {
        let result = wallet_account.update_balances(&[AccountBalance::new(
            Money::from("-10 USDC"),
            Money::from("0 USDC"),
            Money::from("-10 USDC"),
        )]);

        assert!(result.is_err());
    }

    #[rstest]
    fn test_new_checked_rejects_negative_initial_balance() {
        let negative_state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::from("-1 ETH"),
                Money::from("0 ETH"),
                Money::from("-1 ETH"),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        let result = WalletAccount::new_checked(negative_state, true);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Wallet account balance total was negative"
        );
    }

    #[rstest]
    fn test_update_balance_locked_reserves_without_changing_total(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;

        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();

        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("10 ETH"));
        assert_eq!(balance.locked, Money::from("2 ETH"));
        assert_eq!(balance.free, Money::from("8 ETH"));
        assert_eq!(wallet_account.balances_locked.len(), 1);
    }

    #[rstest]
    fn test_update_balance_locked_rejects_missing_observed_currency(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let balances_before = wallet_account.base.balances.clone();
        let locks_before = wallet_account.balances_locked.clone();
        let events_before = wallet_account.events.clone();
        let result =
            wallet_account.update_balance_locked(currency_pair_btcusdt.id, Money::from("1 BTC"));

        assert_eq!(
            result.unwrap_err().to_string(),
            "wallet has no observed balance for BTC"
        );
        assert_eq!(wallet_account.base.balances, balances_before);
        assert_eq!(wallet_account.balances_locked, locks_before);
        assert_eq!(wallet_account.events, events_before);
    }

    #[rstest]
    fn test_update_balance_locked_multiple_currencies(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;

        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();
        wallet_account
            .update_balance_locked(instrument_id, Money::from("5000 USDC"))
            .unwrap();

        assert_eq!(wallet_account.balances_locked.len(), 2);
        let eth_balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(eth_balance.locked, Money::from("2 ETH"));
        assert_eq!(eth_balance.free, Money::from("8 ETH"));
        let usdc_balance = wallet_account.balance(Some(Currency::USDC())).unwrap();
        assert_eq!(usdc_balance.total, Money::from("25000 USDC"));
        assert_eq!(usdc_balance.locked, Money::from("5000 USDC"));
        assert_eq!(usdc_balance.free, Money::from("20000 USDC"));
    }

    #[rstest]
    fn test_clear_balance_locked_only_removes_target_instrument(mut wallet_account: WalletAccount) {
        let weth_usdc_id = InstrumentId::from("WETHUSDC.BLOCKCHAIN");
        let weth_dai_id = InstrumentId::from("WETHDAI.BLOCKCHAIN");

        wallet_account
            .update_balance_locked(weth_usdc_id, Money::from("2 ETH"))
            .unwrap();
        wallet_account
            .update_balance_locked(weth_dai_id, Money::from("1 ETH"))
            .unwrap();
        assert_eq!(wallet_account.balances_locked.len(), 2);

        wallet_account.clear_balance_locked(weth_usdc_id);

        assert_eq!(wallet_account.balances_locked.len(), 1);
        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("10 ETH"));
        assert_eq!(balance.locked, Money::from("1 ETH"));
        assert_eq!(balance.free, Money::from("9 ETH"));
    }

    #[rstest]
    fn test_recalculate_balance_clamps_when_locked_exceeds_total(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;

        wallet_account
            .update_balance_locked(instrument_id, Money::from("15 ETH"))
            .unwrap();

        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("10 ETH"));
        assert_eq!(balance.locked, Money::from("10 ETH"));
        assert_eq!(balance.free, Money::from("0 ETH"));
    }

    #[rstest]
    fn test_update_balance_locked_rejects_aggregate_overflow_without_mutation(
        mut wallet_account: WalletAccount,
    ) {
        let maximum = Money::from_raw(MONEY_RAW_MAX, Currency::ETH());

        wallet_account
            .update_balance_locked(InstrumentId::from("WETHUSDC.BLOCKCHAIN"), maximum)
            .unwrap();
        let balances_before = wallet_account.base.balances.clone();
        let locks_before = wallet_account.balances_locked.clone();
        let events_before = wallet_account.events.clone();

        let result =
            wallet_account.update_balance_locked(InstrumentId::from("WETHDAI.BLOCKCHAIN"), maximum);

        assert_eq!(
            result.unwrap_err().to_string(),
            "ETH wallet reservation total exceeds Money bounds"
        );
        assert_eq!(wallet_account.base.balances, balances_before);
        assert_eq!(wallet_account.balances_locked, locks_before);
        assert_eq!(wallet_account.events, events_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_normalizes_to_observed_precision() {
        let observed = test_currency("TST", 18);
        let source = test_currency("TST", 16);
        let mut wallet = wallet_with_total(observed, 1_000_000_000_000_000_000);
        let instrument_id = InstrumentId::from("TSTUSDC.BLOCKCHAIN");
        let reservation = Money::from_raw(1_234_567_890_123_456, source);

        wallet
            .update_balance_locked(instrument_id, reservation)
            .unwrap();

        let stored = wallet
            .balances_locked
            .get(&(instrument_id, observed))
            .unwrap();
        let balance = wallet.balance(Some(observed)).unwrap();
        assert_eq!(stored.currency, observed);
        assert_eq!(stored.currency.precision, 18);
        assert_eq!(stored.raw, 123_456_789_012_345_600);
        assert_eq!(balance.total.raw, 1_000_000_000_000_000_000);
        assert_eq!(balance.locked.raw, 123_456_789_012_345_600);
        assert_eq!(balance.free.raw, 876_543_210_987_654_400);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_normalizes_to_observed_currency_grid() {
        let observed = test_currency("GRID", 6);
        let source = test_currency("GRID", FIXED_PRECISION);
        let scale = money_raw(10_i128.pow(u32::from(FIXED_PRECISION)));
        let grid = money_raw(10_i128.pow(u32::from(FIXED_PRECISION - observed.precision)));
        let reservation_raw = 123_456 * grid;
        let mut wallet = wallet_with_total(observed, scale);
        let instrument_id = InstrumentId::from("GRIDUSDC.BLOCKCHAIN");
        let reservation = Money::from_raw(reservation_raw, source);

        wallet
            .update_balance_locked(instrument_id, reservation)
            .unwrap();

        let stored = wallet
            .balances_locked
            .get(&(instrument_id, observed))
            .unwrap();
        let balance = wallet.balance(Some(observed)).unwrap();
        assert_eq!(stored.currency, observed);
        assert_eq!(stored.currency.precision, 6);
        assert_eq!(stored.raw, reservation_raw);
        assert_eq!(balance.total.raw, scale);
        assert_eq!(balance.locked.raw, reservation_raw);
        assert_eq!(balance.free.raw, scale - reservation_raw);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_rejects_observed_currency_grid_loss_without_mutation() {
        let observed = test_currency("GRID", 6);
        let source = test_currency("GRID", FIXED_PRECISION);
        let scale = money_raw(10_i128.pow(u32::from(FIXED_PRECISION)));
        let grid = money_raw(10_i128.pow(u32::from(FIXED_PRECISION - observed.precision)));
        let mut wallet = wallet_with_total(observed, scale);
        let balances_before = wallet.base.balances.clone();
        let locks_before = wallet.balances_locked.clone();
        let events_before = wallet.events.clone();

        let result = wallet.update_balance_locked(
            InstrumentId::from("GRIDUSDC.BLOCKCHAIN"),
            Money::from_raw(123_456 * grid + 1, source),
        );

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid fixed-point raw value")
        );
        assert_eq!(wallet.base.balances, balances_before);
        assert_eq!(wallet.balances_locked, locks_before);
        assert_eq!(wallet.events, events_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_rejects_lossy_downscale_without_mutation() {
        let observed = test_currency("LOSS", 16);
        let source = test_currency("LOSS", 18);
        let mut wallet = wallet_with_total(observed, 10_000_000_000_000_000);
        let balances_before = wallet.base.balances.clone();
        let locks_before = wallet.balances_locked.clone();
        let events_before = wallet.events.clone();

        let result = wallet.update_balance_locked(
            InstrumentId::from("LOSSUSDC.BLOCKCHAIN"),
            Money::from_raw(1, source),
        );

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("loses precision when decreasing raw scale")
        );
        assert_eq!(wallet.base.balances, balances_before);
        assert_eq!(wallet.balances_locked, locks_before);
        assert_eq!(wallet.events, events_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_rejects_non_canonical_raw_without_mutation() {
        let observed = test_currency("RAW", 18);
        let source = test_currency("RAW", 15);
        let mut wallet = wallet_with_total(observed, 10_000_000_000_000_000);
        let balances_before = wallet.base.balances.clone();
        let locks_before = wallet.balances_locked.clone();
        let events_before = wallet.events.clone();

        let result = wallet.update_balance_locked(
            InstrumentId::from("RAWUSDC.BLOCKCHAIN"),
            Money::from_raw(1, source),
        );

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid fixed-point raw value")
        );
        assert_eq!(wallet.base.balances, balances_before);
        assert_eq!(wallet.balances_locked, locks_before);
        assert_eq!(wallet.events, events_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_update_balance_locked_rejects_scale_overflow_without_mutation() {
        let observed = test_currency("OVR", 18);
        let source = test_currency("OVR", 16);
        let mut wallet = wallet_with_total(observed, 10_000_000_000_000_000);
        let balances_before = wallet.base.balances.clone();
        let locks_before = wallet.balances_locked.clone();
        let events_before = wallet.events.clone();

        let result = wallet.update_balance_locked(
            InstrumentId::from("OVRUSDC.BLOCKCHAIN"),
            Money::from_raw(MONEY_RAW_MAX, source),
        );

        assert!(result.unwrap_err().to_string().contains("exceeded bounds"));
        assert_eq!(wallet.base.balances, balances_before);
        assert_eq!(wallet.balances_locked, locks_before);
        assert_eq!(wallet.events, events_before);
    }

    #[rstest]
    fn test_update_balance_locked_rejects_negative_without_mutation() {
        let mut wallet = wallet_with_total(Currency::ETH(), 10_000_000_000_000_000);
        let balances_before = wallet.base.balances.clone();
        let locks_before = wallet.balances_locked.clone();
        let events_before = wallet.events.clone();

        let result = wallet.update_balance_locked(
            InstrumentId::from("WETHUSDC.BLOCKCHAIN"),
            Money::from("-1 ETH"),
        );

        assert!(result.unwrap_err().to_string().contains("was negative"));
        assert_eq!(wallet.base.balances, balances_before);
        assert_eq!(wallet.balances_locked, locks_before);
        assert_eq!(wallet.events, events_before);
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_new_checked_rejects_non_canonical_observed_total() {
        let currency = test_currency("OBS", 15);
        let total = Money::from_raw(1, currency);
        let state = AccountState::new(
            AccountId::from("WALLET-OBS"),
            AccountType::Wallet,
            vec![AccountBalance::new(total, Money::zero(currency), total)],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        let result = WalletAccount::new_checked(state, true);

        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid fixed-point raw value")
        );
    }

    #[rstest]
    fn test_apply_reported_snapshot_preserves_locks(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
        mut wallet_account_state_changed: AccountState,
    ) {
        let instrument_id = currency_pair_btcusdt.id;
        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();
        wallet_account_state_changed.balances[0] = AccountBalance::new(
            Money::from("9.5 ETH"),
            Money::from("1 ETH"),
            Money::from("8.5 ETH"),
        );

        wallet_account.apply(wallet_account_state_changed).unwrap();

        assert_eq!(
            wallet_account
                .balances_locked
                .get(&(instrument_id, Currency::ETH(),)),
            Some(&Money::from("2 ETH"))
        );
        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("9.5 ETH"));
        assert_eq!(balance.locked, Money::from("2 ETH"));
        assert_eq!(balance.free, Money::from("7.5 ETH"));
    }

    #[rstest]
    fn test_apply_reported_empty_balances_preserves_locks(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;
        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();

        let empty_snapshot = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );
        wallet_account.apply(empty_snapshot).unwrap();

        assert_eq!(wallet_account.balances_locked.len(), 1);
        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.locked, Money::from("2 ETH"));
        assert_eq!(balance.free, Money::from("8 ETH"));
    }

    #[rstest]
    fn test_apply_partial_snapshot_preserves_omitted_currency_lock(
        mut wallet_account: WalletAccount,
    ) {
        let instrument_id = InstrumentId::from("WETHUSDC.BLOCKCHAIN");
        wallet_account
            .update_balance_locked(instrument_id, Money::from("5000 USDC"))
            .unwrap();
        let snapshot = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::from("9.5 ETH"),
                Money::from("0 ETH"),
                Money::from("9.5 ETH"),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );

        wallet_account.apply(snapshot).unwrap();
        wallet_account.clear_balance_locked(instrument_id);

        let balance = wallet_account.balance(Some(Currency::USDC())).unwrap();
        assert_eq!(balance.total, Money::from("25000 USDC"));
        assert_eq!(balance.locked, Money::from("0 USDC"));
        assert_eq!(balance.free, Money::from("25000 USDC"));
    }

    #[rstest]
    fn test_update_balances_rederives_existing_lock(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;
        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();

        wallet_account
            .update_balances(&[AccountBalance::new(
                Money::from("9 ETH"),
                Money::from("0 ETH"),
                Money::from("9 ETH"),
            )])
            .unwrap();

        let balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("9 ETH"));
        assert_eq!(balance.locked, Money::from("2 ETH"));
        assert_eq!(balance.free, Money::from("7 ETH"));
    }

    #[rstest]
    fn test_apply_retains_requested_lock_across_total_recovery(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;
        wallet_account
            .update_balance_locked(instrument_id, Money::from("8 ETH"))
            .unwrap();
        let reduced = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::from("5 ETH"),
                Money::from("0 ETH"),
                Money::from("5 ETH"),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );
        wallet_account.apply(reduced).unwrap();
        let reduced_balance = *wallet_account.balance(Some(Currency::ETH())).unwrap();

        let recovered = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Wallet,
            vec![AccountBalance::new(
                Money::from("10 ETH"),
                Money::from("0 ETH"),
                Money::from("10 ETH"),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            None,
        );
        wallet_account.apply(recovered).unwrap();

        let recovered_balance = wallet_account.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(reduced_balance.locked, Money::from("5 ETH"));
        assert_eq!(reduced_balance.free, Money::from("0 ETH"));
        assert_eq!(recovered_balance.locked, Money::from("8 ETH"));
        assert_eq!(recovered_balance.free, Money::from("2 ETH"));
    }

    #[rstest]
    fn test_serde_round_trip_rederives_balances_without_transient_locks(
        mut wallet_account: WalletAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let instrument_id = currency_pair_btcusdt.id;
        wallet_account
            .update_balance_locked(instrument_id, Money::from("2 ETH"))
            .unwrap();

        let json = serde_json::to_string(&wallet_account).unwrap();
        let deserialized: WalletAccount = serde_json::from_str(&json).unwrap();

        assert_eq!(deserialized.id, wallet_account.id);
        assert_eq!(deserialized.account_type, AccountType::Wallet);
        assert_eq!(deserialized.events(), wallet_account.events());
        assert!(deserialized.balances_locked.is_empty());
        let balance = deserialized.balance(Some(Currency::ETH())).unwrap();
        assert_eq!(balance.total, Money::from("10 ETH"));
        assert_eq!(balance.locked, Money::from("0 ETH"));
        assert_eq!(balance.free, Money::from("10 ETH"));
    }

    #[rstest]
    fn test_calculate_balance_locked_buy(audusd_sim: CurrencyPair) {
        let mut wallet_account = wallet_with_total(Currency::USD(), 1_000_000_000_000_000_000);
        let balance_locked = wallet_account
            .calculate_balance_locked(
                &audusd_sim.into_any(),
                OrderSide::Buy,
                Quantity::from("25000"),
                Price::from("0.8"),
                None,
            )
            .unwrap();

        assert_eq!(balance_locked, Money::from("20000 USD"));
    }

    #[rstest]
    fn test_calculate_balance_locked_buy_ceil_to_currency_grid(audusd_sim: CurrencyPair) {
        let mut wallet_account = wallet_with_total(Currency::USD(), Money::from("1 USD").raw);
        let balance_locked = wallet_account
            .calculate_balance_locked(
                &audusd_sim.into_any(),
                OrderSide::Buy,
                Quantity::from("1"),
                Price::from("0.001"),
                None,
            )
            .unwrap();

        assert_eq!(balance_locked, Money::from("0.01 USD"));
    }

    #[rstest]
    fn test_calculate_balance_locked_sell(audusd_sim: CurrencyPair) {
        let mut wallet_account = wallet_with_total(Currency::AUD(), 1_000_000_000_000_000_000);
        let balance_locked = wallet_account
            .calculate_balance_locked(
                &audusd_sim.into_any(),
                OrderSide::Sell,
                Quantity::from("2"),
                Price::from("0.8"),
                None,
            )
            .unwrap();

        assert_eq!(balance_locked, Money::from("2 AUD"));
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn test_calculate_balance_locked_buy_ceil_to_observed_currency_grid() {
        let base = test_currency("WBASE", 16);
        let quote = test_currency("WQUOTE", 16);
        let observed = test_currency("WQUOTE", 6);
        let instrument = test_currency_pair(base, quote);
        let scale = money_raw(10_i128.pow(u32::from(FIXED_PRECISION)));
        let grid = money_raw(10_i128.pow(u32::from(FIXED_PRECISION - observed.precision)));
        let mut wallet = wallet_with_total(observed, 10 * scale);

        let locked = wallet
            .calculate_balance_locked(
                &instrument.into_any(),
                OrderSide::Buy,
                Quantity::from("1.55"),
                Price::from("3.123456"),
                None,
            )
            .unwrap();

        assert_eq!(locked.currency, observed);
        assert_eq!(locked.currency.precision, 6);
        assert_eq!(locked.raw, 4_841_357 * grid);
    }

    #[rstest]
    fn test_calculate_pnls_buy(wallet_account: WalletAccount, currency_pair_btcusdt: CurrencyPair) {
        let order = OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(currency_pair_btcusdt.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .build();
        let instrument_any = currency_pair_btcusdt.into_any();
        let fill = TestOrderEventStubs::filled(
            &order,
            &instrument_any,
            None,
            None,
            Some(Price::from("50000")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let fill_owned: crate::events::OrderFilled = fill.into();

        let result = wallet_account
            .calculate_pnls(&instrument_any, &fill_owned, None)
            .unwrap();

        assert_eq!(
            result,
            vec![Money::from("1 BTC"), Money::from("-50000 USDT")]
        );
    }

    #[rstest]
    fn test_calculate_commission(wallet_account: WalletAccount, audusd_sim: CurrencyPair) {
        let commission = wallet_account
            .calculate_commission(
                &audusd_sim.into_any(),
                Quantity::from("100000"),
                Price::from("0.8"),
                LiquiditySide::Taker,
                None,
            )
            .unwrap();

        assert_eq!(commission, Money::from("1.60 USD"));
    }

    #[rstest]
    fn test_calculate_commission_invalid_liquidity_side_returns_error(
        wallet_account: WalletAccount,
        audusd_sim: CurrencyPair,
    ) {
        let result = wallet_account.calculate_commission(
            &audusd_sim.into_any(),
            Quantity::from("1"),
            Price::from("1"),
            LiquiditySide::NoLiquiditySide,
            None,
        );

        assert!(result.is_err());
    }

    #[cfg(feature = "defi")]
    fn test_currency(code: &str, precision: u8) -> Currency {
        Currency::new(code, precision, 0, code, CurrencyType::Crypto)
    }

    #[cfg(feature = "defi")]
    #[allow(
        clippy::useless_conversion,
        reason = "the raw width differs when high-precision is disabled"
    )]
    fn money_raw(raw: i128) -> MoneyRaw {
        raw.try_into().unwrap()
    }

    #[cfg(feature = "defi")]
    fn test_currency_pair(base: Currency, quote: Currency) -> CurrencyPair {
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from("WBASEWQUOTE.BLOCKCHAIN"))
            .raw_symbol(Symbol::from("WBASEWQUOTE"))
            .base_currency(base)
            .quote_currency(quote)
            .price_precision(16)
            .size_precision(16)
            .price_increment(Price::from_raw(1, 16))
            .size_increment(Quantity::from_raw(1, 16))
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap()
    }

    fn wallet_with_total(currency: Currency, raw: MoneyRaw) -> WalletAccount {
        let total = Money::from_raw(raw, currency);
        WalletAccount::new(
            AccountState::new(
                AccountId::from("WALLET-TEST"),
                AccountType::Wallet,
                vec![AccountBalance::new(total, Money::zero(currency), total)],
                vec![],
                true,
                uuid4(),
                0.into(),
                0.into(),
                None,
            ),
            true,
        )
    }
}
