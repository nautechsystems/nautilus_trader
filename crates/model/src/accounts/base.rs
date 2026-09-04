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

//! Base traits and common types shared by all account implementations.
//!
//! Concrete account types (`CashAccount`, `MarginAccount`, etc.) build on the abstractions defined
//! in this file.

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_core::{
    UnixNanos,
    correctness::{
        CorrectnessError, CorrectnessResult, FAILED, check_equal, check_predicate_false,
        check_predicate_true,
    },
    datetime::secs_to_nanos,
};
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity, money::MoneyRaw},
};

/// Represents the account state shared by every account type.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
pub struct BaseAccount {
    /// The account ID.
    pub id: AccountId,
    /// The type of the account (e.g., margin, spot, etc.).
    pub account_type: AccountType,
    /// The base currency for the account, if applicable.
    pub base_currency: Option<Currency>,
    /// Indicates if the account state is recalculated from order fills
    /// (as opposed to taken from venue reports).
    pub calculate_account_state: bool,
    /// The account state events, oldest first.
    pub events: Vec<AccountState>,
    /// The commissions charged so far, keyed by currency.
    pub commissions: AHashMap<Currency, Money>,
    /// The current balances in the account, keyed by currency.
    pub balances: IndexMap<Currency, AccountBalance>,
    /// The total balances the account started with, keyed by currency.
    pub balances_starting: IndexMap<Currency, Money>,
}

impl BaseAccount {
    /// Creates a new [`BaseAccount`] instance.
    #[must_use]
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        let mut balances_starting: IndexMap<Currency, Money> = IndexMap::new();
        let mut balances: IndexMap<Currency, AccountBalance> = IndexMap::new();
        event.balances.iter().for_each(|balance| {
            balances_starting.insert(balance.currency, balance.total);
            balances.insert(balance.currency, *balance);
        });
        Self {
            id: event.account_id,
            account_type: event.account_type,
            base_currency: event.base_currency,
            calculate_account_state,
            events: vec![event],
            commissions: AHashMap::new(),
            balances,
            balances_starting,
        }
    }

    #[must_use]
    pub(crate) fn clone_without_events(&self) -> Self {
        Self {
            id: self.id,
            account_type: self.account_type,
            base_currency: self.base_currency,
            calculate_account_state: self.calculate_account_state,
            events: Vec::new(),
            commissions: self.commissions.clone(),
            balances: self.balances.clone(),
            balances_starting: self.balances_starting.clone(),
        }
    }

    /// Returns a reference to the `AccountBalance` for the specified currency, or `None` if absent.
    ///
    /// # Panics
    ///
    /// Panics if `currency` is `None` and `self.base_currency` is `None`.
    #[must_use]
    pub fn base_balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        let currency = currency
            .or(self.base_currency)
            .expect("Currency must be specified");
        self.balances.get(&currency)
    }

    /// Returns the total `Money` balance for the specified currency, or `None` if absent.
    ///
    /// # Panics
    ///
    /// Panics if `currency` is `None` and `self.base_currency` is `None`.
    #[must_use]
    pub fn base_balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance(currency).map(|balance| balance.total)
    }

    #[must_use]
    pub fn base_balances_total(&self) -> IndexMap<Currency, Money> {
        self.balances
            .iter()
            .map(|(currency, balance)| (*currency, balance.total))
            .collect()
    }

    /// Returns the free `Money` balance for the specified currency, or `None` if absent.
    ///
    /// # Panics
    ///
    /// Panics if `currency` is `None` and `self.base_currency` is `None`.
    #[must_use]
    pub fn base_balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance(currency).map(|balance| balance.free)
    }

    #[must_use]
    pub fn base_balances_free(&self) -> IndexMap<Currency, Money> {
        self.balances
            .iter()
            .map(|(currency, balance)| (*currency, balance.free))
            .collect()
    }

    /// Returns the locked `Money` balance for the specified currency, or `None` if absent.
    ///
    /// # Panics
    ///
    /// Panics if `currency` is `None` and `self.base_currency` is `None`.
    #[must_use]
    pub fn base_balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance(currency).map(|balance| balance.locked)
    }

    #[must_use]
    pub fn base_balances_locked(&self) -> IndexMap<Currency, Money> {
        self.balances
            .iter()
            .map(|(currency, balance)| (*currency, balance.locked))
            .collect()
    }

    #[must_use]
    pub fn base_last_event(&self) -> Option<AccountState> {
        self.events.last().cloned()
    }

    /// Updates the account balances with the provided list of `AccountBalance` instances.
    ///
    /// Note: This method does NOT validate negative balances. Derived account types
    /// (`CashAccount`, `MarginAccount`) should perform their own validation in `apply()`:
    /// - `MarginAccount`: allows negative balances (normal for margin trading)
    /// - `CashAccount`: rejects negative unless `allow_borrowing` is true
    pub fn update_balances(&mut self, balances: &[AccountBalance]) {
        for balance in balances {
            self.balances.insert(balance.currency, *balance);
        }
    }

    /// Updates the account commissions with the provided amount.
    ///
    /// # Panics
    ///
    /// Panics if the accumulated commission exceeds [`Money`] bounds. Operational callers should
    /// use [`Self::try_update_commissions`] when the input is not already known to fit.
    pub fn update_commissions(&mut self, commission: Money) {
        self.try_update_commissions(commission)
            .expect("commission total exceeded Money bounds");
    }

    /// Updates the account commissions with the provided amount.
    ///
    /// # Errors
    ///
    /// Returns an error if the accumulated commission exceeds [`Money`] bounds.
    pub fn try_update_commissions(&mut self, commission: Money) -> anyhow::Result<()> {
        // TODO: Remove once from_raw enforces canonical precision alignment (v2)
        let commission = commission.normalized();
        if commission.is_zero() {
            return Ok(());
        }
        let currency = commission.currency;
        let total = self
            .commissions
            .get(&currency)
            .copied()
            .map_or(Some(commission), |total| total.checked_add(commission))
            .ok_or_else(|| anyhow::anyhow!("{currency} commission total exceeds Money bounds"))?;
        self.commissions.insert(currency, total);
        Ok(())
    }

    /// Returns the total commission for the specified currency.
    #[must_use]
    pub fn commission(&self, currency: &Currency) -> Option<Money> {
        self.commissions.get(currency).copied()
    }

    /// Returns a map of all commissions by currency.
    #[must_use]
    pub fn commissions(&self) -> AHashMap<Currency, Money> {
        self.commissions.clone()
    }

    /// Checks the event belongs to this account.
    ///
    /// Concrete accounts call this before mutating any state, so a foreign event is rejected
    /// rather than partially applied.
    ///
    /// # Errors
    ///
    /// Returns an error if `event.account_id` does not match this account's ID.
    pub(crate) fn check_event_account_id(&self, event: &AccountState) -> anyhow::Result<()> {
        anyhow::ensure!(
            event.account_id == self.id,
            "Account event had a different account ID: expected {}, received {}",
            self.id,
            event.account_id
        );
        Ok(())
    }

    /// Applies an [`AccountState`] event, updating balances.
    ///
    /// # Panics
    ///
    /// Panics if `event.account_id` does not match this account's ID. Every account rejects a
    /// foreign event before reaching here, so this remains an internal invariant.
    pub fn base_apply(&mut self, event: AccountState) {
        check_equal(&event.account_id, &self.id, "event.account_id", "self.id").expect(FAILED);
        self.update_balances(&event.balances);
        self.events.push(event);
    }

    /// Purges all account state events which are outside the lookback window.
    ///
    /// Guaranteed to retain at least the latest event.
    ///
    /// # Panics
    ///
    /// Panics if the purging implementation is changed and all events are purged.
    pub fn base_purge_account_events(&mut self, ts_now: UnixNanos, lookback_secs: u64) {
        let Ok(lookback_ns) = secs_to_nanos(lookback_secs as f64) else {
            log::warn!(
                "Cannot purge account events: lookback_secs {lookback_secs} is not representable in `u64` nanoseconds"
            );
            return;
        };
        let purge_cutoff = ts_now.checked_sub(lookback_ns);

        let mut retained_events = Vec::new();

        for event in &self.events {
            if purge_cutoff.is_none_or(|cutoff| event.ts_event > cutoff) {
                retained_events.push(event.clone());
            }
        }

        // Guarantee ≥ 1 event
        if retained_events.is_empty() && !self.events.is_empty() {
            retained_events.push(self.events.last().expect("events not empty").clone());
        }

        self.events = retained_events;
    }

    /// Calculates the amount of balance to lock for a new order based on the given side, quantity, and price.
    ///
    /// # Errors
    ///
    /// Returns an error if the locked amount cannot be represented in the target currency.
    pub fn base_calculate_balance_locked(
        &self,
        instrument: &InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        let base_currency = instrument
            .base_currency()
            .unwrap_or(instrument.quote_currency());
        let quote_currency = instrument.quote_currency();
        let amount = match side {
            // A buy at a negative price settles as a credit rather than a debit, so it
            // reserves nothing. Clamping per order rather than after aggregation keeps a
            // negative-price buy from financing a positive-price one before either fills.
            OrderSide::Buy => instrument
                .try_calculate_notional_value(quantity, price, use_quote_for_inverse)?
                .as_decimal()
                .max(Decimal::ZERO),
            OrderSide::Sell => quantity.as_decimal(),
        };

        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::from_decimal(amount, base_currency)?)
        } else {
            let currency = match side {
                OrderSide::Buy => quote_currency,
                OrderSide::Sell => base_currency,
            };
            Ok(Money::from_decimal(amount, currency)?)
        }
    }

    /// Calculates profit and loss amounts for a filled order.
    ///
    /// For cash accounts, this calculates the balance impact of a fill:
    /// - BUY: gain base currency quantity, lose quote currency notional.
    /// - SELL: lose base currency quantity, gain quote currency notional.
    ///
    /// Note: Unlike betting accounts, cash accounts do NOT cap to position quantity.
    /// The full fill quantity is used for PnL calculation.
    ///
    /// # Errors
    ///
    /// Returns an error if a PnL amount cannot be represented in the target currency.
    pub fn base_calculate_pnls(
        &self,
        instrument: &InstrumentAny,
        fill: &OrderFilled,
        _position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        let mut pnls: IndexMap<Currency, Money> = IndexMap::new();
        let base_currency = instrument.base_currency();

        // No quantity capping (betting accounts cap to position qty, cash accounts don't)
        let fill_qty = fill.last_qty;
        let notional = instrument.try_calculate_notional_value(fill_qty, fill.last_px, None)?;

        if fill.order_side == OrderSide::Buy {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    Money::from_decimal(fill_qty.as_decimal(), base_currency_value)?,
                );
            }
            pnls.insert(notional.currency, -notional);
        } else {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    -Money::from_decimal(fill_qty.as_decimal(), base_currency_value)?,
                );
            }
            pnls.insert(notional.currency, notional);
        }
        Ok(pnls.into_values().collect())
    }

    /// Calculates commission fees for a filled order.
    ///
    /// # Errors
    ///
    /// Returns an error if `liquidity_side` is invalid, the notional value cannot be calculated,
    /// or the commission cannot be represented in the target currency.
    pub fn base_calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        anyhow::ensure!(
            liquidity_side != LiquiditySide::NoLiquiditySide,
            "Invalid `LiquiditySide`: {liquidity_side}"
        );
        let notional =
            instrument.try_calculate_notional_value(last_qty, last_px, use_quote_for_inverse)?;
        let rate = match liquidity_side {
            LiquiditySide::Maker => instrument.maker_fee(),
            LiquiditySide::Taker => instrument.taker_fee(),
            LiquiditySide::NoLiquiditySide => {
                anyhow::bail!("Invalid `LiquiditySide`: {liquidity_side}")
            }
        };
        let commission = notional
            .as_decimal()
            .checked_mul(rate)
            .ok_or_else(|| anyhow::anyhow!("commission calculation overflow"))?;

        Ok(Money::from_decimal(commission, notional.currency)?)
    }
}

/// Updates the locked balance for the given instrument and currency, then recalculates the
/// account balance for that currency from all per-(instrument, currency) locks.
///
/// The reservation is recorded without a balance when the currency has no observed balance yet,
/// so a later balance report derives from it.
///
/// # Errors
///
/// Returns an error if `locked` is negative, its precision differs from the balance precision,
/// or the reservations cannot produce a valid balance. Balances and reservations are left
/// unchanged when an error is returned.
pub(crate) fn update_balance_locked(
    balances: &mut IndexMap<Currency, AccountBalance>,
    balances_locked: &mut AHashMap<(InstrumentId, Currency), Money>,
    instrument_id: InstrumentId,
    locked: Money,
) -> anyhow::Result<()> {
    anyhow::ensure!(locked.raw >= 0, "locked balance was negative: {locked}");

    let currency = locked.currency;
    let key = (instrument_id, currency);

    let Some(current_balance) = balances.get(&currency).copied() else {
        balances_locked.insert(key, locked);
        return Ok(());
    };

    anyhow::ensure!(
        current_balance.currency.precision == currency.precision,
        "Cannot update {currency} reservation: precision {} differed from balance precision {}",
        currency.precision,
        current_balance.currency.precision
    );

    let previous = balances_locked.insert(key, locked);

    match balance_from_locks(current_balance, balances_locked) {
        Ok(balance) => {
            balances.insert(currency, balance);
            Ok(())
        }
        Err(e) => {
            // Restore the prior reservation so a rejected update leaves nothing behind
            match previous {
                Some(previous) => balances_locked.insert(key, previous),
                None => balances_locked.remove(&key),
            };
            Err(e.into())
        }
    }
}

/// Clears all locked balances for the given instrument ID, recalculating each affected currency.
pub(crate) fn clear_balance_locked(
    balances: &mut IndexMap<Currency, AccountBalance>,
    balances_locked: &mut AHashMap<(InstrumentId, Currency), Money>,
    instrument_id: InstrumentId,
) {
    let currencies_to_recalc: Vec<Currency> = balances_locked
        .keys()
        .filter(|(id, _)| *id == instrument_id)
        .map(|(_, currency)| *currency)
        .collect();

    for currency in &currencies_to_recalc {
        balances_locked.remove(&(instrument_id, *currency));
    }

    for currency in currencies_to_recalc {
        recalculate_balance(balances, balances_locked, currency);
    }
}

/// Recalculates the account balance for the specified currency based on per-instrument locks.
///
/// Sums all per-instrument locked amounts for the currency and updates the balance.
/// If the total locked exceeds the total balance, clamps to total (free = 0).
pub(crate) fn recalculate_balance(
    balances: &mut IndexMap<Currency, AccountBalance>,
    balances_locked: &AHashMap<(InstrumentId, Currency), Money>,
    currency: Currency,
) {
    let current_balance = if let Some(balance) = balances.get(&currency) {
        *balance
    } else {
        log::debug!("Cannot recalculate balance when no current balance for {currency}");
        return;
    };

    let new_balance = match balance_from_locks(current_balance, balances_locked) {
        Ok(balance) => balance,
        Err(e) => {
            log::error!(
                "Cannot recalculate {currency} balance from reservations: {e}; using a non-spendable balance"
            );
            non_spendable_balance(current_balance)
        }
    };

    balances.insert(currency, new_balance);
}

/// Derives an account balance from its total and all local reservations for its currency.
///
/// # Errors
///
/// Returns an error if a reservation is negative, uses a different fixed precision, or the
/// derived locked or free balance exceeds [`Money`] bounds.
pub(crate) fn balance_from_locks(
    current_balance: AccountBalance,
    balances_locked: &AHashMap<(InstrumentId, Currency), Money>,
) -> CorrectnessResult<AccountBalance> {
    let currency = current_balance.currency;
    let mut total_locked_raw: MoneyRaw = 0;

    for locked in balances_locked
        .values()
        .filter(|locked| locked.currency == currency)
    {
        check_predicate_false(
            locked.raw < 0,
            &format!("locked balance was negative: {locked}"),
        )?;
        check_predicate_true(
            locked.currency.precision == currency.precision,
            &format!(
                "locked balance precision {} differed from balance precision {} for {currency}",
                locked.currency.precision, currency.precision
            ),
        )?;
        total_locked_raw = total_locked_raw.saturating_add(locked.raw);
    }

    let total_raw = current_balance.total.raw;
    let locked_raw = if total_raw >= 0 {
        total_locked_raw.min(total_raw)
    } else {
        total_locked_raw
    };
    let free_raw =
        total_raw
            .checked_sub(locked_raw)
            .ok_or_else(|| CorrectnessError::PredicateViolation {
                message: format!(
                    "derived free balance overflowed for total {} and locked raw {locked_raw}",
                    current_balance.total
                ),
            })?;
    let locked = Money::from_raw_checked(locked_raw, currency)?;
    let free = Money::from_raw_checked(free_raw, currency)?;

    AccountBalance::new_checked(current_balance.total, locked, free)
}

fn non_spendable_balance(current_balance: AccountBalance) -> AccountBalance {
    let zero = Money::zero(current_balance.currency);
    let (locked, free) = if current_balance.total.raw >= 0 {
        (current_balance.total, zero)
    } else {
        (zero, current_balance.total)
    };

    AccountBalance {
        currency: current_balance.currency,
        total: current_balance.total,
        locked,
        free,
    }
}

#[cfg(all(test, feature = "test-support"))]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::{events::account::stubs::cash_account_state, types::money::MONEY_RAW_MAX};

    #[rstest]
    fn test_base_purge_account_events_retains_latest_when_all_purged() {
        use crate::{
            enums::AccountType,
            events::account::stubs::cash_account_state,
            identifiers::stubs::{account_id, uuid4},
            types::{Currency, stubs::stub_account_balance},
        };

        let mut account = BaseAccount::new(cash_account_state(), true);

        // Create events with different timestamps manually
        let event1 = AccountState::new(
            account_id(),
            AccountType::Cash,
            vec![stub_account_balance()],
            vec![],
            true,
            uuid4(),
            UnixNanos::from(100_000_000),
            UnixNanos::from(100_000_000),
            Some(Currency::USD()),
        );
        let event2 = AccountState::new(
            account_id(),
            AccountType::Cash,
            vec![stub_account_balance()],
            vec![],
            true,
            uuid4(),
            UnixNanos::from(200_000_000),
            UnixNanos::from(200_000_000),
            Some(Currency::USD()),
        );
        let event3 = AccountState::new(
            account_id(),
            AccountType::Cash,
            vec![stub_account_balance()],
            vec![],
            true,
            uuid4(),
            UnixNanos::from(300_000_000),
            UnixNanos::from(300_000_000),
            Some(Currency::USD()),
        );

        account.base_apply(event1);
        account.base_apply(event2);
        account.base_apply(event3.clone());

        assert_eq!(account.events.len(), 4);

        account.base_purge_account_events(UnixNanos::from(1_000_000_000), 0);

        assert_eq!(account.events.len(), 1);
        assert_eq!(account.events[0].ts_event, event3.ts_event);
        assert_eq!(account.base_last_event().unwrap().ts_event, event3.ts_event);
    }

    #[rstest]
    fn test_base_purge_account_events_retains_all_for_overflowing_lookback() {
        let mut account = BaseAccount::new(cash_account_state(), true);
        let mut event = cash_account_state();
        event.ts_event = UnixNanos::from(1);
        account.base_apply(event);

        account.base_purge_account_events(UnixNanos::from(u64::MAX), u64::MAX);

        assert_eq!(account.events.len(), 2);
    }

    #[rstest]
    fn test_base_purge_account_events_retains_future_event_without_overflow() {
        let mut event = cash_account_state();
        event.ts_event = UnixNanos::from(u64::MAX - 1);
        let mut account = BaseAccount::new(event, true);

        account.base_purge_account_events(UnixNanos::from(u64::MAX), 60);

        assert_eq!(account.events.len(), 1);
    }

    #[rstest]
    #[should_panic(
        expected = r#"lhs_param: "event.account_id", rhs_param: "self.id", lhs: "OTHER-001", rhs: "SIM-001""#
    )]
    fn test_base_apply_panics_on_different_account_id() {
        let mut account = BaseAccount::new(cash_account_state(), true);
        let mut event = cash_account_state();
        event.account_id = AccountId::from("OTHER-001");

        account.base_apply(event);
    }

    fn usd_balances(total: &str) -> IndexMap<Currency, AccountBalance> {
        let total = Money::from(total);
        let mut balances = IndexMap::new();
        balances.insert(
            Currency::USD(),
            AccountBalance::new(total, Money::zero(Currency::USD()), total),
        );
        balances
    }

    fn mismatched_usd() -> Currency {
        Currency::new(
            "USD",
            Currency::USD().precision + 1,
            840,
            "United States dollar",
            crate::enums::CurrencyType::Fiat,
        )
    }

    // The observed case also exercises `balance_from_locks`; the unobserved case reaches the
    // early return, so only the guard in `update_balance_locked` can reject it.
    #[rstest]
    #[case::observed_currency(true)]
    #[case::unobserved_currency(false)]
    fn test_update_balance_locked_rejects_negative_without_mutation(#[case] observed: bool) {
        let mut balances = if observed {
            usd_balances("1000 USD")
        } else {
            IndexMap::new()
        };
        let balances_before = balances.clone();
        let mut balances_locked = AHashMap::new();
        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        let error = update_balance_locked(
            &mut balances,
            &mut balances_locked,
            instrument_id,
            Money::from("-1 USD"),
        )
        .unwrap_err();

        assert_eq!(error.to_string(), "locked balance was negative: -1.00 USD");
        assert!(balances_locked.is_empty());
        assert_eq!(balances, balances_before);
    }

    #[rstest]
    fn test_update_balance_locked_restores_prior_reservation_on_failure() {
        let usd = Currency::USD();
        let mut balances = usd_balances("1000 USD");
        let stale_key = (InstrumentId::from("EUR/USD.SIM"), mismatched_usd());
        let stale = Money::from_decimal(Decimal::from(10), mismatched_usd()).unwrap();
        let mut balances_locked = AHashMap::from([(stale_key, stale)]);
        let instrument_id = InstrumentId::from("AUD/USD.SIM");

        // A reservation that is valid on its own, but cannot derive a balance alongside the
        // stale entry already recorded for this currency
        let result = update_balance_locked(
            &mut balances,
            &mut balances_locked,
            instrument_id,
            Money::from("100 USD"),
        );

        assert!(result.is_err());
        assert_eq!(balances_locked, AHashMap::from([(stale_key, stale)]));
        assert_eq!(balances, usd_balances("1000 USD"));
        assert_eq!(balances[&usd].free, Money::from("1000 USD"));
    }

    #[rstest]
    #[case::positive_total("1000 USD", "1000 USD", "0 USD")]
    #[case::negative_total("-1000 USD", "0 USD", "-1000 USD")]
    fn test_recalculate_balance_degrades_to_non_spendable_for_invalid_reservation(
        #[case] total: &str,
        #[case] expected_locked: &str,
        #[case] expected_free: &str,
    ) {
        use crate::{enums::CurrencyType, types::Currency};

        let usd = Currency::USD();
        let total = Money::from(total);
        let mut balances = IndexMap::new();
        balances.insert(usd, AccountBalance::new(total, Money::zero(usd), total));
        // A reservation at a differing precision cannot derive a valid balance
        let mismatched_usd = Currency::new(
            "USD",
            usd.precision + 1,
            840,
            "United States dollar",
            CurrencyType::Fiat,
        );
        let mut balances_locked = AHashMap::new();
        balances_locked.insert(
            (InstrumentId::from("AUD/USD.SIM"), mismatched_usd),
            Money::from_decimal(Decimal::from(100), mismatched_usd).unwrap(),
        );

        recalculate_balance(&mut balances, &balances_locked, usd);

        let balance = balances.get(&usd).expect("balance should be retained");
        assert_eq!(balance.total, total);
        assert_eq!(balance.locked, Money::from(expected_locked));
        assert_eq!(balance.free, Money::from(expected_free));
    }

    #[rstest]
    fn test_update_commissions_sub_canonical_raw_skipped() {
        use crate::{
            events::account::stubs::cash_account_state,
            types::{Currency, Money},
        };

        let mut account = BaseAccount::new(cash_account_state(), true);
        let usd = Currency::USD();

        // Sub-canonical raw (1 < tick size for USD precision 2) normalizes to zero
        account.update_commissions(Money::from_raw(1, usd));

        assert!(account.commission(&usd).is_none());
    }

    #[rstest]
    fn test_try_update_commissions_overflow_preserves_total() {
        let mut account = BaseAccount::new(cash_account_state(), true);
        let usd = Currency::USD();
        let maximum = Money::from_raw(MONEY_RAW_MAX, usd);

        account.try_update_commissions(maximum).unwrap();
        let result = account.try_update_commissions(Money::from("0.01 USD"));

        assert!(result.is_err());
        assert_eq!(account.commission(&usd), Some(maximum));
    }
}
