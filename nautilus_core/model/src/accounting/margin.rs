// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
};

use anyhow::Result;
use pyo3::prelude::*;
use rust_decimal::prelude::ToPrimitive;

use crate::{
    enums::AccountType,
    events::account::state::AccountState,
    identifiers::{account_id::AccountId, instrument_id::InstrumentId},
    instruments::Instrument,
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
        money::Money,
        price::Price,
        quantity::Quantity,
    },
};

#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarginAccount {
    pub id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Currency,
    calculate_account_state: bool,
    events: Vec<AccountState>,
    commissions: HashMap<Currency, f64>,
    balances: HashMap<Currency, AccountBalance>,
    balances_starting: HashMap<Currency, Money>,
    margins: HashMap<InstrumentId, MarginBalance>,
    pub leverages: HashMap<InstrumentId, f64>,
    pub default_leverage: f64,
}

impl MarginAccount {
    pub fn new(event: AccountState, calculate_account_state: bool) -> Result<Self> {
        let mut balances_starting: HashMap<Currency, Money> = HashMap::new();
        let mut balances: HashMap<Currency, AccountBalance> = HashMap::new();
        event.balances.iter().for_each(|balance| {
            balances_starting.insert(balance.currency, balance.total);
            balances.insert(balance.currency, *balance);
        });
        let mut margins: HashMap<InstrumentId, MarginBalance> = HashMap::new();
        event.margins.iter().for_each(|margin| {
            margins.insert(margin.instrument_id, *margin);
        });
        Ok(Self {
            id: event.account_id,
            account_type: event.account_type,
            base_currency: event.base_currency,
            calculate_account_state,
            events: vec![event],
            commissions: HashMap::new(),
            balances,
            balances_starting,
            margins: HashMap::new(),
            leverages: HashMap::new(),
            default_leverage: 1.0,
        })
    }

    pub fn update_balances(&mut self, balances: Vec<AccountBalance>) {
        balances.into_iter().for_each(|balance| {
            // clone real balance without reference
            if balance.total.raw < 0 {
                // TODO raise AccountBalanceNegative event
                panic!("Cannot update balances with total less than 0.0")
            } else {
                // clear asset balance
                self.balances.insert(balance.currency, balance);
            }
        });
    }

    pub fn set_default_leverage(&mut self, leverage: f64) {
        self.default_leverage = leverage;
    }

    pub fn set_leverage(&mut self, instrument_id: InstrumentId, leverage: f64) {
        self.leverages.insert(instrument_id, leverage);
    }

    pub fn get_leverage(&self, instrument_id: &InstrumentId) -> f64 {
        *self
            .leverages
            .get(instrument_id)
            .unwrap_or(&self.default_leverage)
    }

    pub fn is_unleveraged(&self, instrument_id: InstrumentId) -> bool {
        self.get_leverage(&instrument_id) == 1.0
    }

    pub fn is_cash_account(&self) -> bool {
        self.account_type == AccountType::Cash
    }
    pub fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    pub fn initial_margins(&self) -> HashMap<InstrumentId, Money> {
        let mut initial_margins: HashMap<InstrumentId, Money> = HashMap::new();
        self.margins.values().for_each(|margin_balance| {
            initial_margins.insert(margin_balance.instrument_id, margin_balance.initial);
        });
        initial_margins
    }

    pub fn maintenance_margins(&self) -> HashMap<InstrumentId, Money> {
        let mut maintenance_margins: HashMap<InstrumentId, Money> = HashMap::new();
        self.margins.values().for_each(|margin_balance| {
            maintenance_margins.insert(margin_balance.instrument_id, margin_balance.maintenance);
        });
        maintenance_margins
    }

    pub fn update_initial_margin(&mut self, instrument_id: InstrumentId, margin_init: Money) {
        let margin_balance = self.margins.get(&instrument_id);
        if margin_balance.is_none() {
            self.margins.insert(
                instrument_id,
                MarginBalance::new(
                    margin_init,
                    Money::new(0.0, margin_init.currency).unwrap(),
                    instrument_id,
                )
                .unwrap(),
            );
        } else {
            // update the margin_balance initial property with margin_init
            let mut new_margin_balance = *margin_balance.unwrap();
            new_margin_balance.initial = margin_init;
            self.margins.insert(instrument_id, new_margin_balance);
        }
        self.recalculate_balance(margin_init.currency)
    }

    pub fn initial_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        if margin_balance.is_none() {
            panic!("Cannot get margin_init when no margin_balance")
        }
        margin_balance.unwrap().initial
    }

    pub fn update_maintenance_margin(
        &mut self,
        instrument_id: InstrumentId,
        margin_maintenance: Money,
    ) {
        let margin_balance = self.margins.get(&instrument_id);
        if margin_balance.is_none() {
            self.margins.insert(
                instrument_id,
                MarginBalance::new(
                    Money::new(0.0, margin_maintenance.currency).unwrap(),
                    margin_maintenance,
                    instrument_id,
                )
                .unwrap(),
            );
        } else {
            // update the margin_balance maintenance property with margin_maintenance
            let mut new_margin_balance = *margin_balance.unwrap();
            new_margin_balance.maintenance = margin_maintenance;
            self.margins.insert(instrument_id, new_margin_balance);
        }
        self.recalculate_balance(margin_maintenance.currency)
    }

    pub fn maintenance_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        if margin_balance.is_none() {
            panic!("Cannot get maintenance_margin when no margin_balance")
        }
        margin_balance.unwrap().maintenance
    }

    pub fn calculate_initial_margin<T: Instrument>(
        &mut self,
        instrument: T,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        let notional = instrument.calculate_notional_value(quantity, price, use_quote_for_inverse);
        let leverage = self.get_leverage(instrument.id());
        if leverage == 0.0 {
            self.leverages
                .insert(*instrument.id(), self.default_leverage);
        }
        let adjusted_notional = notional / leverage;
        let initial_margin_f64 = instrument.margin_init().to_f64().unwrap();
        let mut margin = adjusted_notional * initial_margin_f64;
        // add taker fee
        margin += adjusted_notional * instrument.taker_fee().to_f64().unwrap() * 2.0;
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        if instrument.is_inverse() && !use_quote_for_inverse {
            Money::new(margin, *instrument.base_currency().unwrap()).unwrap()
        } else {
            Money::new(margin, *instrument.quote_currency()).unwrap()
        }
    }

    pub fn calculate_maintenance_margin<T: Instrument>(
        &mut self,
        instrument: T,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> Money {
        let notional = instrument.calculate_notional_value(quantity, price, use_quote_for_inverse);
        let leverage = self.get_leverage(instrument.id());
        if leverage == 0.0 {
            self.leverages
                .insert(*instrument.id(), self.default_leverage);
        }
        let adjusted_notional = notional / leverage;
        let margin_maint_f64 = instrument.margin_maint().to_f64().unwrap();
        let mut margin = adjusted_notional * margin_maint_f64;
        // Add taker fee
        margin += adjusted_notional * instrument.taker_fee().to_f64().unwrap();
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        if instrument.is_inverse() && !use_quote_for_inverse {
            Money::new(margin, *instrument.base_currency().unwrap()).unwrap()
        } else {
            Money::new(margin, *instrument.quote_currency()).unwrap()
        }
    }

    pub fn recalculate_balance(&mut self, currency: Currency) {
        let current_balance = match self.balances.get(&currency) {
            Some(balance) => balance,
            None => panic!("Cannot recalculate balance when no starting balance"),
        };

        let mut total_margin = 0;
        // iterate over margins
        self.margins.values().for_each(|margin| {
            if margin.currency == currency {
                total_margin += margin.initial.raw;
                total_margin += margin.maintenance.raw;
            }
        });
        let total_free = current_balance.total.raw - total_margin;
        if total_free < 0 {
            // TODO error handle this with AccountMarginExceeded
            panic!("Cannot recalculate balance when total_free is less than 0.0")
        }
        let new_balance = AccountBalance::new(
            current_balance.total,
            Money::from_raw(total_margin, currency),
            Money::from_raw(total_free, currency),
        )
        .unwrap();
        self.balances.insert(currency, new_balance);
    }
}

impl PartialEq for MarginAccount {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for MarginAccount {}

impl Display for MarginAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MarginAccount(id={}, type={}, base={})",
            self.id, self.account_type, self.base_currency.code
        )
    }
}

impl Hash for MarginAccount {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.id.hash(state);
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::{
        accounting::{margin::MarginAccount, stubs::*},
        identifiers::{instrument_id::InstrumentId, stubs::*},
        instruments::{
            crypto_perpetual::CryptoPerpetual,
            currency_pair::CurrencyPair,
            stubs::{audusd_sim, xbtusd_bitmex},
        },
        types::{money::Money, price::Price, quantity::Quantity},
    };

    #[rstest]
    fn test_display(margin_account: MarginAccount) {
        assert_eq!(
            margin_account.to_string(),
            "MarginAccount(id=SIM-001, type=MARGIN, base=USD)"
        );
    }

    #[rstest]
    fn test_set_default_leverage(mut margin_account: MarginAccount) {
        assert_eq!(margin_account.default_leverage, 1.0);
        margin_account.set_default_leverage(10.0);
        assert_eq!(margin_account.default_leverage, 10.0);
    }

    #[rstest]
    fn test_get_leverage_default_leverage(
        margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(margin_account.get_leverage(&instrument_id_aud_usd_sim), 1.0);
    }

    #[rstest]
    fn test_set_leverage(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(margin_account.leverages.len(), 0);
        margin_account.set_leverage(instrument_id_aud_usd_sim, 10.0);
        assert_eq!(margin_account.leverages.len(), 1);
        assert_eq!(
            margin_account.get_leverage(&instrument_id_aud_usd_sim),
            10.0
        );
    }

    #[rstest]
    fn test_is_unleveraged_with_leverage_returns_false(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        margin_account.set_leverage(instrument_id_aud_usd_sim.clone(), 10.0);
        assert!(!margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_is_unleveraged_with_no_leverage_returns_true(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        margin_account.set_leverage(instrument_id_aud_usd_sim.clone(), 1.0);
        assert!(margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_is_unleveraged_with_default_leverage_of_1_returns_true(
        margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert!(margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_update_margin_init(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        assert_eq!(margin_account.margins.len(), 0);
        let margin = Money::from("10000 USD");
        margin_account.update_initial_margin(instrument_id_aud_usd_sim, margin);
        assert_eq!(
            margin_account.initial_margin(instrument_id_aud_usd_sim),
            margin
        );
        let margins: Vec<Money> = margin_account
            .margins
            .values()
            .map(|margin_balance| margin_balance.initial)
            .collect();
        assert_eq!(margins, vec![margin]);
    }

    #[rstest]
    fn test_update_margin_maintenance(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        let margin = Money::from("10000 USD");
        margin_account.update_maintenance_margin(instrument_id_aud_usd_sim, margin);
        assert_eq!(
            margin_account.maintenance_margin(instrument_id_aud_usd_sim),
            margin
        );
        let margins: Vec<Money> = margin_account
            .margins
            .values()
            .map(|margin_balance| margin_balance.maintenance)
            .collect();
        assert_eq!(margins, vec![margin]);
    }

    #[rstest]
    fn test_calculate_margin_init_with_leverage(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_leverage(audusd_sim.id, 50.0);
        let result = margin_account.calculate_initial_margin(
            audusd_sim,
            Quantity::from(100_000),
            Price::from("0.8000"),
            None,
        );
        assert_eq!(result, Money::from("48.06 USD"));
    }

    #[rstest]
    fn test_calculate_margin_init_with_default_leverage(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_default_leverage(10.0);
        let result = margin_account.calculate_initial_margin(
            audusd_sim,
            Quantity::from(100_000),
            Price::from("0.8"),
            None,
        );
        assert_eq!(result, Money::from("240.32 USD"));
    }

    #[rstest]
    fn test_calculate_margin_init_with_no_leverage_for_inverse(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result_use_quote_inverse_true = margin_account.calculate_initial_margin(
            xbtusd_bitmex.clone(),
            Quantity::from(100_000),
            Price::from("11493.60"),
            Some(false),
        );
        assert_eq!(result_use_quote_inverse_true, Money::from("0.10005568 BTC"));
        let result_use_quote_inverse_false = margin_account.calculate_initial_margin(
            xbtusd_bitmex,
            Quantity::from(100_000),
            Price::from("11493.60"),
            Some(true),
        );
        assert_eq!(result_use_quote_inverse_false, Money::from("1150 USD"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_no_leverage(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result = margin_account.calculate_maintenance_margin(
            xbtusd_bitmex,
            Quantity::from(100_000),
            Price::from("11493.60"),
            None,
        );
        assert_eq!(result, Money::from("0.03697710 BTC"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_leverage_fx_instrument(
        mut margin_account: MarginAccount,
        audusd_sim: CurrencyPair,
    ) {
        margin_account.set_default_leverage(50.0);
        let result = margin_account.calculate_maintenance_margin(
            audusd_sim,
            Quantity::from(1_000_000),
            Price::from("1"),
            None,
        );
        assert_eq!(result, Money::from("600.40 USD"));
    }

    #[rstest]
    fn test_calculate_margin_maintenance_with_leverage_inverse_instrument(
        mut margin_account: MarginAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        margin_account.set_default_leverage(10.0);
        let result = margin_account.calculate_maintenance_margin(
            xbtusd_bitmex,
            Quantity::from(100_000),
            Price::from("100000.00"),
            None,
        );
        assert_eq!(result, Money::from("0.00042500 BTC"));
    }
}
