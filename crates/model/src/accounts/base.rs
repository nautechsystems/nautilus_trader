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
use rust_decimal::prelude::ToPrimitive;
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

    pub fn update_commissions(&mut self, commission: Money) {
        // TODO: Remove once from_raw enforces canonical precision alignment (v2)
        let commission = commission.normalized();
        if commission.is_zero() {
            return;
        }
        let currency = commission.currency;
        self.commissions
            .entry(currency)
            .and_modify(|total| *total = *total + commission)
            .or_insert(commission);
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
    /// This function never returns an error (TBD).
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
        let notional: f64 = match side {
            OrderSide::Buy => instrument
                .calculate_notional_value(quantity, price, use_quote_for_inverse)
                .as_f64(),
            OrderSide::Sell => quantity.as_f64(),
            OrderSide::NoOrderSide => {
                anyhow::bail!("Invalid `OrderSide` in `base_calculate_balance_locked`: {side}")
            }
        };

        // Handle inverse
        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::new(notional, base_currency))
        } else if side == OrderSide::Buy {
            Ok(Money::new(notional, quote_currency))
        } else if side == OrderSide::Sell {
            Ok(Money::new(notional, base_currency))
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
    /// This function never returns an error (TBD).
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
        let fill_qty_value = fill_qty.as_f64();

        let notional = instrument.calculate_notional_value(fill_qty, fill.last_px, None);

        if fill.order_side == OrderSide::Buy {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    Money::new(fill_qty_value, base_currency_value),
                );
            }
            pnls.insert(
                notional.currency,
                Money::new(-notional.as_f64(), notional.currency),
            );
        } else if fill.order_side == OrderSide::Sell {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    Money::new(-fill_qty_value, base_currency_value),
                );
            }
            pnls.insert(
                notional.currency,
                Money::new(notional.as_f64(), notional.currency),
            );
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
    /// # Panics
    ///
    /// Panics if instrument fees cannot be converted to f64, or if base currency is unavailable for inverse instruments.
    #[expect(
        clippy::missing_errors_doc,
        reason = "Error conditions documented inline"
    )]
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
        let notional = instrument
            .calculate_notional_value(last_qty, last_px, use_quote_for_inverse)
            .as_f64();
        let commission = if liquidity_side == LiquiditySide::Maker {
            notional * instrument.maker_fee().to_f64().unwrap()
        } else if liquidity_side == LiquiditySide::Taker {
            notional * instrument.taker_fee().to_f64().unwrap()
        } else {
            anyhow::bail!("Invalid `LiquiditySide`: {liquidity_side}");
        };

        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::new(commission, instrument.base_currency().unwrap()))
        } else {
            Ok(Money::new(commission, instrument.quote_currency()))
        }
    }
}

#[cfg(all(test, feature = "stubs"))]
mod tests {
    use rstest::rstest;

    use super::*;

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
}
