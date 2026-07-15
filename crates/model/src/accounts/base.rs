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
    correctness::{FAILED, check_equal},
    datetime::secs_to_nanos_unchecked,
};
use serde::{Deserialize, Serialize};

use crate::{
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
pub struct BaseAccount {
    pub id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub calculate_account_state: bool,
    pub events: Vec<AccountState>,
    pub commissions: AHashMap<Currency, Money>,
    pub balances: IndexMap<Currency, AccountBalance>,
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
        let currency = currency
            .or(self.base_currency)
            .expect("Currency must be specified");
        let account_balance = self.balances.get(&currency);
        account_balance.map(|balance| balance.total)
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
        let currency = currency
            .or(self.base_currency)
            .expect("Currency must be specified");
        let account_balance = self.balances.get(&currency);
        account_balance.map(|balance| balance.free)
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
        let currency = currency
            .or(self.base_currency)
            .expect("Currency must be specified");
        let account_balance = self.balances.get(&currency);
        account_balance.map(|balance| balance.locked)
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

    /// Applies an [`AccountState`] event, updating balances.
    ///
    /// # Panics
    ///
    /// Panics if `event.account_id` does not match this account's ID.
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
        let lookback_ns = UnixNanos::from(secs_to_nanos_unchecked(lookback_secs as f64));

        let mut retained_events = Vec::new();

        for event in &self.events {
            if event.ts_event + lookback_ns > ts_now {
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
    ///
    pub fn base_calculate_balance_locked(
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
        let quote_currency = instrument.quote_currency();
        let amount = match side {
            OrderSide::Buy => instrument
                .try_calculate_notional_value(quantity, price, use_quote_for_inverse)?
                .as_decimal(),
            OrderSide::Sell => quantity.as_decimal(),
            OrderSide::NoOrderSide => {
                anyhow::bail!("Invalid `OrderSide` in `base_calculate_balance_locked`: {side}")
            }
        };

        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::from_decimal(amount, base_currency)?)
        } else if side == OrderSide::Buy {
            Ok(Money::from_decimal(amount, quote_currency)?)
        } else if side == OrderSide::Sell {
            Ok(Money::from_decimal(amount, base_currency)?)
        } else {
            anyhow::bail!("Invalid `OrderSide` in `base_calculate_balance_locked`: {side}")
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
    ///
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
        } else if fill.order_side == OrderSide::Sell {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    -Money::from_decimal(fill_qty.as_decimal(), base_currency_value)?,
                );
            }
            pnls.insert(notional.currency, notional);
        } else {
            anyhow::bail!(
                "Invalid `OrderSide` in base_calculate_pnls: {}",
                fill.order_side
            );
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

#[cfg(all(test, feature = "stubs"))]
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
