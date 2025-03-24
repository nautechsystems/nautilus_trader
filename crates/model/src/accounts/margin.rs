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

#![allow(dead_code)]

use std::{
    collections::HashMap,
    fmt::Display,
    hash::{Hash, Hasher},
    ops::{Deref, DerefMut},
};

use rust_decimal::prelude::ToPrimitive;
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, base::BaseAccount},
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarginAccount {
    pub base: BaseAccount,
    pub leverages: HashMap<InstrumentId, f64>,
    pub margins: HashMap<InstrumentId, MarginBalance>,
    pub default_leverage: f64,
}

impl MarginAccount {
    /// Creates a new [`MarginAccount`] instance.
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        Self {
            base: BaseAccount::new(event, calculate_account_state),
            leverages: HashMap::new(),
            margins: HashMap::new(),
            default_leverage: 1.0,
        }
    }

    pub fn set_default_leverage(&mut self, leverage: f64) {
        self.default_leverage = leverage;
    }

    pub fn set_leverage(&mut self, instrument_id: InstrumentId, leverage: f64) {
        self.leverages.insert(instrument_id, leverage);
    }

    #[must_use]
    pub fn get_leverage(&self, instrument_id: &InstrumentId) -> f64 {
        *self
            .leverages
            .get(instrument_id)
            .unwrap_or(&self.default_leverage)
    }

    #[must_use]
    pub fn is_unleveraged(&self, instrument_id: InstrumentId) -> bool {
        self.get_leverage(&instrument_id) == 1.0
    }

    #[must_use]
    pub fn is_cash_account(&self) -> bool {
        self.account_type == AccountType::Cash
    }
    #[must_use]
    pub fn is_margin_account(&self) -> bool {
        self.account_type == AccountType::Margin
    }

    #[must_use]
    pub fn initial_margins(&self) -> HashMap<InstrumentId, Money> {
        let mut initial_margins: HashMap<InstrumentId, Money> = HashMap::new();
        self.margins.values().for_each(|margin_balance| {
            initial_margins.insert(margin_balance.instrument_id, margin_balance.initial);
        });
        initial_margins
    }

    #[must_use]
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
                    Money::new(0.0, margin_init.currency),
                    instrument_id,
                ),
            );
        } else {
            // update the margin_balance initial property with margin_init
            let mut new_margin_balance = *margin_balance.unwrap();
            new_margin_balance.initial = margin_init;
            self.margins.insert(instrument_id, new_margin_balance);
        }
        self.recalculate_balance(margin_init.currency);
    }

    #[must_use]
    pub fn initial_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        assert!(
            margin_balance.is_some(),
            "Cannot get margin_init when no margin_balance"
        );
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
                    Money::new(0.0, margin_maintenance.currency),
                    margin_maintenance,
                    instrument_id,
                ),
            );
        } else {
            // update the margin_balance maintenance property with margin_maintenance
            let mut new_margin_balance = *margin_balance.unwrap();
            new_margin_balance.maintenance = margin_maintenance;
            self.margins.insert(instrument_id, new_margin_balance);
        }
        self.recalculate_balance(margin_maintenance.currency);
    }

    #[must_use]
    pub fn maintenance_margin(&self, instrument_id: InstrumentId) -> Money {
        let margin_balance = self.margins.get(&instrument_id);
        assert!(
            margin_balance.is_some(),
            "Cannot get maintenance_margin when no margin_balance"
        );
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
        let leverage = self.get_leverage(&instrument.id());
        if leverage == 0.0 {
            self.leverages
                .insert(instrument.id(), self.default_leverage);
        }
        let adjusted_notional = notional / leverage;
        let initial_margin_f64 = instrument.margin_init().to_f64().unwrap();
        let mut margin = adjusted_notional * initial_margin_f64;
        // add taker fee
        margin += adjusted_notional * instrument.taker_fee().to_f64().unwrap() * 2.0;
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        if instrument.is_inverse() && !use_quote_for_inverse {
            Money::new(margin, instrument.base_currency().unwrap())
        } else {
            Money::new(margin, instrument.quote_currency())
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
        let leverage = self.get_leverage(&instrument.id());
        if leverage == 0.0 {
            self.leverages
                .insert(instrument.id(), self.default_leverage);
        }
        let adjusted_notional = notional / leverage;
        let margin_maint_f64 = instrument.margin_maint().to_f64().unwrap();
        let mut margin = adjusted_notional * margin_maint_f64;
        // Add taker fee
        margin += adjusted_notional * instrument.taker_fee().to_f64().unwrap();
        let use_quote_for_inverse = use_quote_for_inverse.unwrap_or(false);
        if instrument.is_inverse() && !use_quote_for_inverse {
            Money::new(margin, instrument.base_currency().unwrap())
        } else {
            Money::new(margin, instrument.quote_currency())
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
        // TODO error handle this with AccountMarginExceeded
        assert!(
            total_free >= 0,
            "Cannot recalculate balance when total_free is less than 0.0"
        );
        let new_balance = AccountBalance::new(
            current_balance.total,
            Money::from_raw(total_margin, currency),
            Money::from_raw(total_free, currency),
        );
        self.balances.insert(currency, new_balance);
    }
}

impl Deref for MarginAccount {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for MarginAccount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl Account for MarginAccount {
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
        false // TODO (implement this logic)
    }

    fn balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_total(currency)
    }
    fn balances_total(&self) -> HashMap<Currency, Money> {
        self.base_balances_total()
    }

    fn balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_free(currency)
    }
    fn balances_free(&self) -> HashMap<Currency, Money> {
        self.base_balances_free()
    }

    fn balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        self.base_balance_locked(currency)
    }
    fn balances_locked(&self) -> HashMap<Currency, Money> {
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
    fn starting_balances(&self) -> HashMap<Currency, Money> {
        self.balances_starting.clone()
    }
    fn balances(&self) -> HashMap<Currency, AccountBalance> {
        self.balances.clone()
    }
    fn apply(&mut self, event: AccountState) {
        self.base_apply(event);
    }
    fn calculate_balance_locked(
        &mut self,
        instrument: InstrumentAny,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        self.base_calculate_balance_locked(instrument, side, quantity, price, use_quote_for_inverse)
    }
    fn calculate_pnls(
        &self,
        instrument: InstrumentAny,
        fill: OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        self.base_calculate_pnls(instrument, fill, position)
    }
    fn calculate_commission(
        &self,
        instrument: InstrumentAny,
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
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
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
    use std::collections::HashMap;

    use rstest::rstest;

    use crate::{
        accounts::{Account, MarginAccount, stubs::*},
        events::{AccountState, account::stubs::*},
        identifiers::{InstrumentId, stubs::*},
        instruments::{CryptoPerpetual, CurrencyPair, stubs::*},
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_display(margin_account: MarginAccount) {
        assert_eq!(
            margin_account.to_string(),
            "MarginAccount(id=SIM-001, type=MARGIN, base=USD)"
        );
    }

    #[rstest]
    fn test_base_account_properties(
        margin_account: MarginAccount,
        margin_account_state: AccountState,
    ) {
        assert_eq!(margin_account.base_currency, Some(Currency::from("USD")));
        assert_eq!(
            margin_account.last_event(),
            Some(margin_account_state.clone())
        );
        assert_eq!(margin_account.events(), vec![margin_account_state]);
        assert_eq!(margin_account.event_count(), 1);
        assert_eq!(
            margin_account.balance_total(None),
            Some(Money::from("1525000 USD"))
        );
        assert_eq!(
            margin_account.balance_free(None),
            Some(Money::from("1500000 USD"))
        );
        assert_eq!(
            margin_account.balance_locked(None),
            Some(Money::from("25000 USD"))
        );
        let mut balances_total_expected = HashMap::new();
        balances_total_expected.insert(Currency::from("USD"), Money::from("1525000 USD"));
        assert_eq!(margin_account.balances_total(), balances_total_expected);
        let mut balances_free_expected = HashMap::new();
        balances_free_expected.insert(Currency::from("USD"), Money::from("1500000 USD"));
        assert_eq!(margin_account.balances_free(), balances_free_expected);
        let mut balances_locked_expected = HashMap::new();
        balances_locked_expected.insert(Currency::from("USD"), Money::from("25000 USD"));
        assert_eq!(margin_account.balances_locked(), balances_locked_expected);
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
        margin_account.set_leverage(instrument_id_aud_usd_sim, 10.0);
        assert!(!margin_account.is_unleveraged(instrument_id_aud_usd_sim));
    }

    #[rstest]
    fn test_is_unleveraged_with_no_leverage_returns_true(
        mut margin_account: MarginAccount,
        instrument_id_aud_usd_sim: InstrumentId,
    ) {
        margin_account.set_leverage(instrument_id_aud_usd_sim, 1.0);
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
            xbtusd_bitmex,
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
