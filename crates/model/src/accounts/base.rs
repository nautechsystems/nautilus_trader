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

//! Base traits and common types shared by all account implementations.
//!
//! Concrete account types (`CashAccount`, `MarginAccount`, etc.) build on the abstractions defined
//! in this file.

use std::collections::HashMap;

use nautilus_core::{UnixNanos, datetime::secs_to_nanos};
use rust_decimal::{Decimal, prelude::ToPrimitive};
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
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct BaseAccount {
    pub id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub calculate_account_state: bool,
    pub events: Vec<AccountState>,
    pub commissions: HashMap<Currency, f64>,
    pub balances: HashMap<Currency, AccountBalance>,
    pub balances_starting: HashMap<Currency, Money>,
}

impl BaseAccount {
    /// Creates a new [`BaseAccount`] instance.
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        let mut balances_starting: HashMap<Currency, Money> = HashMap::new();
        let mut balances: HashMap<Currency, AccountBalance> = HashMap::new();
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
            commissions: HashMap::new(),
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
    pub fn base_balances_total(&self) -> HashMap<Currency, Money> {
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
    pub fn base_balances_free(&self) -> HashMap<Currency, Money> {
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
    pub fn base_balances_locked(&self) -> HashMap<Currency, Money> {
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
    /// # Panics
    ///
    /// Panics if any updated `AccountBalance` has a total less than zero.
    pub fn update_balances(&mut self, balances: Vec<AccountBalance>) {
        for balance in balances {
            // clone real balance without reference
            if balance.total.raw < 0 {
                // TODO raise AccountBalanceNegative event
                panic!("Cannot update balances with total less than 0.0")
            } else {
                // clear asset balance
                self.balances.insert(balance.currency, balance);
            }
        }
    }

    pub fn update_commissions(&mut self, commission: Money) {
        if commission.as_decimal() == Decimal::ZERO {
            return;
        }

        let currency = commission.currency;
        let total_commissions = self.commissions.get(&currency).unwrap_or(&0.0);

        self.commissions
            .insert(currency, total_commissions + commission.as_f64());
    }

    pub fn base_apply(&mut self, event: AccountState) {
        self.update_balances(event.balances.clone());
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
        let lookback_ns = UnixNanos::from(secs_to_nanos(lookback_secs as f64));

        let mut retained_events = Vec::new();

        for event in &self.events {
            if event.ts_event + lookback_ns > ts_now {
                retained_events.push(event.clone());
            }
        }

        // Guarantee ≥ 1 event
        if retained_events.is_empty() && !self.events.is_empty() {
            // SAFETY: events was already checked not empty
            retained_events.push(self.events.last().unwrap().clone());
        }

        self.events = retained_events;
    }

    /// Calculates the amount of balance to lock for a new order based on the given side, quantity, and price.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD).
    ///
    /// # Panics
    ///
    /// Panics if `side` is not [`OrderSide::Buy`] or [`OrderSide::Sell`].
    pub fn base_calculate_balance_locked(
        &mut self,
        instrument: InstrumentAny,
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
            _ => panic!("Invalid `OrderSide` in `base_calculate_balance_locked`"),
        };

        // Handle inverse
        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::new(notional, base_currency))
        } else if side == OrderSide::Buy {
            Ok(Money::new(notional, quote_currency))
        } else if side == OrderSide::Sell {
            Ok(Money::new(notional, base_currency))
        } else {
            panic!("Invalid `OrderSide` in `base_calculate_balance_locked`")
        }
    }

    /// Calculates profit and loss amounts for a filled order.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD).
    ///
    /// # Panics
    ///
    /// Panics if `fill.order_side` is neither [`OrderSide::Buy`] nor [`OrderSide::Sell`].
    pub fn base_calculate_pnls(
        &self,
        instrument: InstrumentAny,
        fill: OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        let mut pnls: HashMap<Currency, Money> = HashMap::new();
        let base_currency = instrument.base_currency();

        let fill_qty_value = position.map_or(fill.last_qty.as_f64(), |pos| {
            pos.quantity.as_f64().min(fill.last_qty.as_f64())
        });
        let fill_qty = Quantity::new(fill_qty_value, fill.last_qty.precision);

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
            panic!("Invalid `OrderSide` in base_calculate_pnls")
        }
        Ok(pnls.into_values().collect())
    }

    /// Calculates commission fees for a filled order.
    ///
    /// # Errors
    ///
    /// This function never returns an error (TBD).
    ///
    /// # Panics
    ///
    /// Panics if `liquidity_side` is `LiquiditySide::NoLiquiditySide` or otherwise invalid.
    pub fn base_calculate_commission(
        &self,
        instrument: InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        assert!(
            liquidity_side != LiquiditySide::NoLiquiditySide,
            "Invalid `LiquiditySide`"
        );
        let notional = instrument
            .calculate_notional_value(last_qty, last_px, use_quote_for_inverse)
            .as_f64();
        let commission = if liquidity_side == LiquiditySide::Maker {
            notional * instrument.maker_fee().to_f64().unwrap()
        } else if liquidity_side == LiquiditySide::Taker {
            notional * instrument.taker_fee().to_f64().unwrap()
        } else {
            panic!("Invalid `LiquiditySide` {liquidity_side}")
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
}
