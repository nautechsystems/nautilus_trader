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

use std::collections::HashMap;

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

    #[must_use]
    pub fn base_balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        let currency = currency
            .or(self.base_currency)
            .expect("Currency must be specified");
        self.balances.get(&currency)
    }

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
        // Add expected commission
        let taker_fee = instrument.taker_fee().to_f64().unwrap();
        let locked: f64 = (notional * taker_fee).mul_add(2.0, notional);

        // Handle inverse
        if instrument.is_inverse() && !use_quote_for_inverse.unwrap_or(false) {
            Ok(Money::new(locked, base_currency))
        } else if side == OrderSide::Buy {
            Ok(Money::new(locked, quote_currency))
        } else if side == OrderSide::Sell {
            Ok(Money::new(locked, base_currency))
        } else {
            panic!("Invalid `OrderSide` in `base_calculate_balance_locked`")
        }
    }

    pub fn base_calculate_pnls(
        &self,
        instrument: InstrumentAny,
        fill: OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        let mut pnls: HashMap<Currency, Money> = HashMap::new();
        let quote_currency = instrument.quote_currency();
        let base_currency = instrument.base_currency();

        let fill_px = fill.last_px.as_f64();
        let fill_qty = position.map_or(fill.last_qty.as_f64(), |pos| {
            pos.quantity.as_f64().min(fill.last_qty.as_f64())
        });
        if fill.order_side == OrderSide::Buy {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    Money::new(fill_qty, base_currency_value),
                );
            }
            pnls.insert(
                quote_currency,
                Money::new(-(fill_qty * fill_px), quote_currency),
            );
        } else if fill.order_side == OrderSide::Sell {
            if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                pnls.insert(
                    base_currency_value,
                    Money::new(-fill_qty, base_currency_value),
                );
            }
            pnls.insert(
                quote_currency,
                Money::new(fill_qty * fill_px, quote_currency),
            );
        } else {
            panic!("Invalid `OrderSide` in base_calculate_pnls")
        }
        Ok(pnls.into_values().collect())
    }

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
