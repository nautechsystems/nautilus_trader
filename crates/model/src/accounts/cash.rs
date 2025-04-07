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

use std::{
    collections::HashMap,
    fmt::Display,
    ops::{Deref, DerefMut},
};

use rust_decimal::{Decimal, prelude::ToPrimitive};
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, base::BaseAccount},
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    instruments::InstrumentAny,
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct CashAccount {
    pub base: BaseAccount,
}

impl CashAccount {
    /// Creates a new [`CashAccount`] instance.
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        Self {
            base: BaseAccount::new(event, calculate_account_state),
        }
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
    pub const fn is_unleveraged(&self) -> bool {
        false
    }

    pub fn recalculate_balance(&mut self, currency: Currency) {
        let current_balance = match self.balances.get(&currency) {
            Some(balance) => *balance,
            None => {
                return;
            }
        };

        let total_locked = self
            .balances
            .values()
            .filter(|balance| balance.currency == currency)
            .fold(Decimal::ZERO, |acc, balance| {
                acc + balance.locked.as_decimal()
            });

        let new_balance = AccountBalance::new(
            current_balance.total,
            Money::new(total_locked.to_f64().unwrap(), currency),
            Money::new(
                (current_balance.total.as_decimal() - total_locked)
                    .to_f64()
                    .unwrap(),
                currency,
            ),
        );

        self.balances.insert(currency, new_balance);
    }
}

impl Account for CashAccount {
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
        instrument: InstrumentAny, // TODO: Make this a reference
        fill: OrderFilled,         // TODO: Make this a reference
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

impl Deref for CashAccount {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for CashAccount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl PartialEq for CashAccount {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for CashAccount {}

impl Display for CashAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CashAccount(id={}, type={}, base={})",
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::collections::{HashMap, HashSet};

    use rstest::rstest;

    use crate::{
        accounts::{Account, CashAccount, stubs::*},
        enums::{AccountType, LiquiditySide, OrderSide, OrderType},
        events::{AccountState, account::stubs::*},
        identifiers::{AccountId, position_id::PositionId},
        instruments::{CryptoPerpetual, CurrencyPair, Equity, Instrument, InstrumentAny, stubs::*},
        orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        position::Position,
        types::{Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_display(cash_account: CashAccount) {
        assert_eq!(
            format!("{cash_account}"),
            "CashAccount(id=SIM-001, type=CASH, base=USD)"
        );
    }

    #[rstest]
    fn test_instantiate_single_asset_cash_account(
        cash_account: CashAccount,
        cash_account_state: AccountState,
    ) {
        assert_eq!(cash_account.id, AccountId::from("SIM-001"));
        assert_eq!(cash_account.account_type, AccountType::Cash);
        assert_eq!(cash_account.base_currency, Some(Currency::from("USD")));
        assert_eq!(cash_account.last_event(), Some(cash_account_state.clone()));
        assert_eq!(cash_account.events(), vec![cash_account_state]);
        assert_eq!(cash_account.event_count(), 1);
        assert_eq!(
            cash_account.balance_total(None),
            Some(Money::from("1525000 USD"))
        );
        assert_eq!(
            cash_account.balance_free(None),
            Some(Money::from("1500000 USD"))
        );
        assert_eq!(
            cash_account.balance_locked(None),
            Some(Money::from("25000 USD"))
        );
        let mut balances_total_expected = HashMap::new();
        balances_total_expected.insert(Currency::from("USD"), Money::from("1525000 USD"));
        assert_eq!(cash_account.balances_total(), balances_total_expected);
        let mut balances_free_expected = HashMap::new();
        balances_free_expected.insert(Currency::from("USD"), Money::from("1500000 USD"));
        assert_eq!(cash_account.balances_free(), balances_free_expected);
        let mut balances_locked_expected = HashMap::new();
        balances_locked_expected.insert(Currency::from("USD"), Money::from("25000 USD"));
        assert_eq!(cash_account.balances_locked(), balances_locked_expected);
    }

    #[rstest]
    fn test_instantiate_multi_asset_cash_account(
        cash_account_multi: CashAccount,
        cash_account_state_multi: AccountState,
    ) {
        assert_eq!(cash_account_multi.id, AccountId::from("SIM-001"));
        assert_eq!(cash_account_multi.account_type, AccountType::Cash);
        assert_eq!(
            cash_account_multi.last_event(),
            Some(cash_account_state_multi.clone())
        );
        assert_eq!(cash_account_state_multi.base_currency, None);
        assert_eq!(cash_account_multi.events(), vec![cash_account_state_multi]);
        assert_eq!(cash_account_multi.event_count(), 1);
        assert_eq!(
            cash_account_multi.balance_total(Some(Currency::BTC())),
            Some(Money::from("10 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_total(Some(Currency::ETH())),
            Some(Money::from("20 ETH"))
        );
        assert_eq!(
            cash_account_multi.balance_free(Some(Currency::BTC())),
            Some(Money::from("10 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_free(Some(Currency::ETH())),
            Some(Money::from("20 ETH"))
        );
        assert_eq!(
            cash_account_multi.balance_locked(Some(Currency::BTC())),
            Some(Money::from("0 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_locked(Some(Currency::ETH())),
            Some(Money::from("0 ETH"))
        );
        let mut balances_total_expected = HashMap::new();
        balances_total_expected.insert(Currency::from("BTC"), Money::from("10 BTC"));
        balances_total_expected.insert(Currency::from("ETH"), Money::from("20 ETH"));
        assert_eq!(cash_account_multi.balances_total(), balances_total_expected);
        let mut balances_free_expected = HashMap::new();
        balances_free_expected.insert(Currency::from("BTC"), Money::from("10 BTC"));
        balances_free_expected.insert(Currency::from("ETH"), Money::from("20 ETH"));
        assert_eq!(cash_account_multi.balances_free(), balances_free_expected);
        let mut balances_locked_expected = HashMap::new();
        balances_locked_expected.insert(Currency::from("BTC"), Money::from("0 BTC"));
        balances_locked_expected.insert(Currency::from("ETH"), Money::from("0 ETH"));
        assert_eq!(
            cash_account_multi.balances_locked(),
            balances_locked_expected
        );
    }

    #[rstest]
    fn test_apply_given_new_state_event_updates_correctly(
        mut cash_account_multi: CashAccount,
        cash_account_state_multi: AccountState,
        cash_account_state_multi_changed_btc: AccountState,
    ) {
        // apply second account event
        cash_account_multi.apply(cash_account_state_multi_changed_btc.clone());
        assert_eq!(
            cash_account_multi.last_event(),
            Some(cash_account_state_multi_changed_btc.clone())
        );
        assert_eq!(
            cash_account_multi.events,
            vec![
                cash_account_state_multi,
                cash_account_state_multi_changed_btc
            ]
        );
        assert_eq!(cash_account_multi.event_count(), 2);
        assert_eq!(
            cash_account_multi.balance_total(Some(Currency::BTC())),
            Some(Money::from("9 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_free(Some(Currency::BTC())),
            Some(Money::from("8.5 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_locked(Some(Currency::BTC())),
            Some(Money::from("0.5 BTC"))
        );
        assert_eq!(
            cash_account_multi.balance_total(Some(Currency::ETH())),
            Some(Money::from("20 ETH"))
        );
        assert_eq!(
            cash_account_multi.balance_free(Some(Currency::ETH())),
            Some(Money::from("20 ETH"))
        );
        assert_eq!(
            cash_account_multi.balance_locked(Some(Currency::ETH())),
            Some(Money::from("0 ETH"))
        );
    }

    #[rstest]
    fn test_calculate_balance_locked_buy(
        mut cash_account_million_usd: CashAccount,
        audusd_sim: CurrencyPair,
    ) {
        let balance_locked = cash_account_million_usd
            .calculate_balance_locked(
                audusd_sim.into_any(),
                OrderSide::Buy,
                Quantity::from("1000000"),
                Price::from("0.8"),
                None,
            )
            .unwrap();
        assert_eq!(balance_locked, Money::from("800032 USD"));
    }

    #[rstest]
    fn test_calculate_balance_locked_sell(
        mut cash_account_million_usd: CashAccount,
        audusd_sim: CurrencyPair,
    ) {
        let balance_locked = cash_account_million_usd
            .calculate_balance_locked(
                audusd_sim.into_any(),
                OrderSide::Sell,
                Quantity::from("1000000"),
                Price::from("0.8"),
                None,
            )
            .unwrap();
        assert_eq!(balance_locked, Money::from("1000040 AUD"));
    }

    #[rstest]
    fn test_calculate_balance_locked_sell_no_base_currency(
        mut cash_account_million_usd: CashAccount,
        equity_aapl: Equity,
    ) {
        let balance_locked = cash_account_million_usd
            .calculate_balance_locked(
                equity_aapl.into_any(),
                OrderSide::Sell,
                Quantity::from("100"),
                Price::from("1500.0"),
                None,
            )
            .unwrap();
        assert_eq!(balance_locked, Money::from("100 USD"));
    }

    #[rstest]
    fn test_calculate_pnls_for_single_currency_cash_account(
        cash_account_million_usd: CashAccount,
        audusd_sim: CurrencyPair,
    ) {
        let audusd_sim = InstrumentAny::CurrencyPair(audusd_sim);
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(audusd_sim.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1000000"))
            .build();
        let fill = TestOrderEventStubs::filled(
            &order,
            &audusd_sim,
            None,
            Some(PositionId::new("P-123456")),
            Some(Price::from("0.8")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let position = Position::new(&audusd_sim, fill.clone().into());
        let pnls = cash_account_million_usd
            .calculate_pnls(audusd_sim, fill.into(), Some(position)) // TODO: Remove clone
            .unwrap();
        assert_eq!(pnls, vec![Money::from("-800000 USD")]);
    }

    #[rstest]
    fn test_calculate_pnls_for_multi_currency_cash_account_btcusdt(
        cash_account_multi: CashAccount,
        currency_pair_btcusdt: CurrencyPair,
    ) {
        let btcusdt = InstrumentAny::CurrencyPair(currency_pair_btcusdt);
        let order1 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(currency_pair_btcusdt.id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("0.5"))
            .build();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &btcusdt,
            None,
            Some(PositionId::new("P-123456")),
            Some(Price::from("45500.00")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let position = Position::new(&btcusdt, fill1.clone().into());
        let result1 = cash_account_multi
            .calculate_pnls(
                currency_pair_btcusdt.into_any(),
                fill1.into(), // TODO: This doesn't need to be owned
                Some(position.clone()),
            )
            .unwrap();
        let order2 = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(currency_pair_btcusdt.id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("0.5"))
            .build();
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &btcusdt,
            None,
            Some(PositionId::new("P-123456")),
            Some(Price::from("45500.00")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let result2 = cash_account_multi
            .calculate_pnls(
                currency_pair_btcusdt.into_any(),
                fill2.into(),
                Some(position),
            )
            .unwrap();
        // use hash set to ignore order of results
        let result1_set: HashSet<Money> = result1.into_iter().collect();
        let result1_expected: HashSet<Money> =
            vec![Money::from("22750 USDT"), Money::from("-0.5 BTC")]
                .into_iter()
                .collect();
        let result2_set: HashSet<Money> = result2.into_iter().collect();
        let result2_expected: HashSet<Money> =
            vec![Money::from("-22750 USDT"), Money::from("0.5 BTC")]
                .into_iter()
                .collect();
        assert_eq!(result1_set, result1_expected);
        assert_eq!(result2_set, result2_expected);
    }

    #[rstest]
    #[case(false, Money::from("-0.00218331 BTC"))]
    #[case(true, Money::from("-25.0 USD"))]
    fn test_calculate_commission_for_inverse_maker_crypto(
        #[case] use_quote_for_inverse: bool,
        #[case] expected: Money,
        cash_account_million_usd: CashAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result = cash_account_million_usd
            .calculate_commission(
                xbtusd_bitmex.into_any(),
                Quantity::from("100000"),
                Price::from("11450.50"),
                LiquiditySide::Maker,
                Some(use_quote_for_inverse),
            )
            .unwrap();
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_calculate_commission_for_taker_fx(
        cash_account_million_usd: CashAccount,
        audusd_sim: CurrencyPair,
    ) {
        let result = cash_account_million_usd
            .calculate_commission(
                audusd_sim.into_any(),
                Quantity::from("1500000"),
                Price::from("0.8005"),
                LiquiditySide::Taker,
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("24.02 USD"));
    }

    #[rstest]
    fn test_calculate_commission_crypto_taker(
        cash_account_million_usd: CashAccount,
        xbtusd_bitmex: CryptoPerpetual,
    ) {
        let result = cash_account_million_usd
            .calculate_commission(
                xbtusd_bitmex.into_any(),
                Quantity::from("100000"),
                Price::from("11450.50"),
                LiquiditySide::Taker,
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("0.00654993 BTC"));
    }

    #[rstest]
    fn test_calculate_commission_fx_taker(cash_account_million_usd: CashAccount) {
        let instrument = usdjpy_idealpro();
        let result = cash_account_million_usd
            .calculate_commission(
                instrument.into_any(),
                Quantity::from("2200000"),
                Price::from("120.310"),
                LiquiditySide::Taker,
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from("5294 JPY"));
    }
}
