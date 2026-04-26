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

//! A betting account with sports-betting specific balance locking and PnL rules.

use std::{
    fmt::Display,
    ops::{Deref, DerefMut},
};

use ahash::AHashMap;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, base::BaseAccount},
    enums::{AccountType, InstrumentClass, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity, money::MoneyRaw},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BettingAccount {
    pub base: BaseAccount,
    /// Per-(instrument, currency) locked balances (transient, not persisted).
    #[serde(skip, default)]
    pub balances_locked: AHashMap<(InstrumentId, Currency), Money>,
}

impl BettingAccount {
    /// Creates a new [`BettingAccount`] instance.
    #[must_use]
    pub fn new(event: AccountState, calculate_account_state: bool) -> Self {
        Self {
            base: BaseAccount::new(event, calculate_account_state),
            balances_locked: AHashMap::new(),
        }
    }

    /// Updates the locked balance for the given instrument and currency.
    ///
    /// # Panics
    ///
    /// Panics if `locked` is negative.
    pub fn update_balance_locked(&mut self, instrument_id: InstrumentId, locked: Money) {
        assert!(locked.raw >= 0, "locked balance was negative: {locked}");
        let currency = locked.currency;
        self.balances_locked
            .insert((instrument_id, currency), locked);
        self.recalculate_balance(currency);
    }

    /// Clears all locked balances for the given instrument ID.
    pub fn clear_balance_locked(&mut self, instrument_id: InstrumentId) {
        let currencies_to_recalc: Vec<Currency> = self
            .balances_locked
            .keys()
            .filter(|(id, _)| *id == instrument_id)
            .map(|(_, currency)| *currency)
            .collect();

        for currency in &currencies_to_recalc {
            self.balances_locked.remove(&(instrument_id, *currency));
        }

        for currency in currencies_to_recalc {
            self.recalculate_balance(currency);
        }
    }

    /// Updates the account balances, rejecting negative totals.
    ///
    /// # Errors
    ///
    /// Returns an error if any balance has a negative total.
    pub fn update_balances(&mut self, balances: &[AccountBalance]) -> anyhow::Result<()> {
        for balance in balances {
            if balance.total.raw < 0 {
                anyhow::bail!(
                    "Betting account balance would become negative: {} {} ({})",
                    balance.total.as_decimal(),
                    balance.currency.code,
                    self.id
                );
            }
        }
        self.base.update_balances(balances);
        Ok(())
    }

    #[must_use]
    pub const fn is_unleveraged(&self) -> bool {
        true
    }

    /// Returns the balance impact for a betting order.
    ///
    /// For `Sell` (back) the impact is the negative stake (quantity).
    /// For `Buy` (lay) the impact is the negative liability (quantity * (price - 1)).
    ///
    /// # Panics
    ///
    /// Panics if `order_side` is `NoOrderSide`.
    #[must_use]
    pub fn balance_impact(
        &self,
        instrument: &InstrumentAny,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money {
        let currency = instrument.quote_currency();
        let quantity_f64 = quantity.as_f64();
        let price_f64 = price.as_f64();
        let impact = match order_side {
            OrderSide::Sell => -quantity_f64,
            OrderSide::Buy => -(quantity_f64 * (price_f64 - 1.0)),
            OrderSide::NoOrderSide => panic!("invalid `OrderSide`, was {order_side}"),
        };
        Money::new(impact, currency)
    }

    /// Recalculates the account balance for the specified currency based on per-instrument locks.
    pub fn recalculate_balance(&mut self, currency: Currency) {
        let current_balance = if let Some(balance) = self.balances.get(&currency) {
            *balance
        } else {
            log::debug!("Cannot recalculate balance when no current balance for {currency}");
            return;
        };

        let total_locked_raw: MoneyRaw = self
            .balances_locked
            .values()
            .filter(|locked| locked.currency == currency)
            .map(|locked| locked.raw)
            .fold(0, |acc, raw| acc.saturating_add(raw));

        let total_raw = current_balance.total.raw;
        let (locked_raw, free_raw) = if total_locked_raw > total_raw && total_raw >= 0 {
            (total_raw, 0)
        } else {
            (total_locked_raw, total_raw - total_locked_raw)
        };

        let new_balance = AccountBalance::new(
            current_balance.total,
            Money::from_raw(locked_raw, currency),
            Money::from_raw(free_raw, currency),
        );

        self.balances.insert(currency, new_balance);
    }
}

impl Account for BettingAccount {
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
        true
    }

    fn is_margin_account(&self) -> bool {
        false
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
        for balance in &event.balances {
            if balance.total.raw < 0 {
                anyhow::bail!(
                    "Cannot apply betting account state: balance would be negative {} {} ({})",
                    balance.total.as_decimal(),
                    balance.currency.code,
                    self.id
                );
            }
        }

        if event.is_reported {
            self.balances_locked.clear();
        }

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
        anyhow::ensure!(
            instrument.instrument_class() == InstrumentClass::SportsBetting,
            "BettingAccount requires a sports betting instrument"
        );
        anyhow::ensure!(
            use_quote_for_inverse != Some(true),
            "`use_quote_for_inverse` is not applicable for betting accounts"
        );

        let locked = match side {
            OrderSide::Sell => quantity.as_f64(),
            OrderSide::Buy => quantity.as_f64() * (price.as_f64() - 1.0),
            OrderSide::NoOrderSide => {
                anyhow::bail!("Invalid `OrderSide` in `calculate_balance_locked`: {side}")
            }
        };

        Ok(Money::new(locked, instrument.quote_currency()))
    }

    fn calculate_pnls(
        &self,
        instrument: &InstrumentAny,
        fill: &OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        anyhow::ensure!(
            instrument.instrument_class() == InstrumentClass::SportsBetting,
            "BettingAccount requires a sports betting instrument"
        );

        let mut pnls: IndexMap<Currency, Money> = IndexMap::new();
        let quote_currency = instrument.quote_currency();
        let base_currency = instrument.base_currency();

        let mut fill_qty = fill.last_qty;

        if let Some(position) = position.as_ref()
            && position.quantity.raw != 0
            && position.entry != fill.order_side
        {
            fill_qty = Quantity::from_raw(
                fill.last_qty.raw.min(position.quantity.raw),
                fill.last_qty.precision,
            );
        }

        let quote_pnl = Money::new(fill.last_px.as_f64() * fill_qty.as_f64(), quote_currency);

        match fill.order_side {
            OrderSide::Buy => {
                if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                    pnls.insert(
                        base_currency_value,
                        Money::new(fill_qty.as_f64(), base_currency_value),
                    );
                }
                pnls.insert(
                    quote_currency,
                    Money::new(-quote_pnl.as_f64(), quote_currency),
                );
            }
            OrderSide::Sell => {
                if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                    pnls.insert(
                        base_currency_value,
                        Money::new(-fill_qty.as_f64(), base_currency_value),
                    );
                }
                pnls.insert(quote_currency, quote_pnl);
            }
            OrderSide::NoOrderSide => {
                anyhow::bail!("Invalid `OrderSide` in calculate_pnls: {}", fill.order_side)
            }
        }

        Ok(pnls.into_values().collect())
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

impl Deref for BettingAccount {
    type Target = BaseAccount;

    fn deref(&self) -> &Self::Target {
        &self.base
    }
}

impl DerefMut for BettingAccount {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.base
    }
}

impl PartialEq for BettingAccount {
    fn eq(&self, other: &Self) -> bool {
        self.id == other.id
    }
}

impl Eq for BettingAccount {}

impl Display for BettingAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "BettingAccount(id={}, type={}, base={})",
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
        accounts::{Account, BettingAccount, stubs::*},
        enums::{AccountType, LiquiditySide, OrderSide},
        events::{AccountState, account::stubs::*},
        identifiers::AccountId,
        instruments::{Instrument, stubs::betting},
        orders::stubs::TestOrderEventStubs,
        position::Position,
        types::{AccountBalance, Currency, Money, Price, Quantity},
    };

    #[rstest]
    fn test_display(betting_account: BettingAccount) {
        assert_eq!(
            format!("{betting_account}"),
            "BettingAccount(id=SIM-001, type=BETTING, base=GBP)"
        );
    }

    #[rstest]
    fn test_instantiate_single_asset_betting_account(
        betting_account: BettingAccount,
        betting_account_state: AccountState,
    ) {
        assert_eq!(betting_account.id, AccountId::from("SIM-001"));
        assert_eq!(betting_account.account_type, AccountType::Betting);
        assert_eq!(betting_account.base_currency, Some(Currency::GBP()));
        assert_eq!(
            betting_account.last_event(),
            Some(betting_account_state.clone())
        );
        assert_eq!(betting_account.events(), vec![betting_account_state]);
        assert_eq!(betting_account.event_count(), 1);
        assert_eq!(
            betting_account.balance_total(None),
            Some(Money::from("1000 GBP"))
        );
        assert_eq!(
            betting_account.balance_free(None),
            Some(Money::from("1000 GBP"))
        );
        assert_eq!(
            betting_account.balance_locked(None),
            Some(Money::from("0 GBP"))
        );

        let mut balances_total_expected = IndexMap::new();
        balances_total_expected.insert(Currency::GBP(), Money::from("1000 GBP"));
        assert_eq!(betting_account.balances_total(), balances_total_expected);
    }

    #[rstest]
    fn test_apply_given_new_state_event_updates_correctly(
        mut betting_account: BettingAccount,
        betting_account_state: AccountState,
        betting_account_state_changed: AccountState,
    ) {
        betting_account
            .apply(betting_account_state_changed.clone())
            .unwrap();

        assert_eq!(
            betting_account.last_event(),
            Some(betting_account_state_changed.clone())
        );
        assert_eq!(
            betting_account.events,
            vec![betting_account_state, betting_account_state_changed]
        );
        assert_eq!(betting_account.event_count(), 2);
        assert_eq!(
            betting_account.balance_total(None),
            Some(Money::from("900 GBP"))
        );
        assert_eq!(
            betting_account.balance_free(None),
            Some(Money::from("850 GBP"))
        );
        assert_eq!(
            betting_account.balance_locked(None),
            Some(Money::from("50 GBP"))
        );
    }

    #[rstest]
    #[case(OrderSide::Sell, "1.60", "10", "10 GBP")]
    #[case(OrderSide::Sell, "2.00", "10", "10 GBP")]
    #[case(OrderSide::Sell, "10.00", "20", "20 GBP")]
    #[case(OrderSide::Buy, "1.25", "10", "2.5 GBP")]
    #[case(OrderSide::Buy, "2.00", "10", "10 GBP")]
    #[case(OrderSide::Buy, "10.00", "10", "90 GBP")]
    fn test_calculate_balance_locked(
        mut betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
        #[case] side: OrderSide,
        #[case] price: &str,
        #[case] quantity: &str,
        #[case] expected: &str,
    ) {
        let result = betting_account
            .calculate_balance_locked(
                &betting.into_any(),
                side,
                Quantity::from(quantity),
                Price::from(price),
                None,
            )
            .unwrap();
        assert_eq!(result, Money::from(expected));
    }

    #[rstest]
    fn test_calculate_pnls_single_currency_account(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
    ) {
        let order = crate::orders::builder::OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(betting.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();
        let betting_any = betting.into_any();
        let fill = TestOrderEventStubs::filled(
            &order,
            &betting_any,
            None,
            None,
            Some(Price::from("0.8")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let position = Position::new(&betting_any, fill.clone().into());
        let fill_owned: crate::events::OrderFilled = fill.into();

        let result = betting_account
            .calculate_pnls(&betting_any, &fill_owned, Some(position))
            .unwrap();

        assert_eq!(result, vec![Money::from("-80 GBP")]);
    }

    #[rstest]
    fn test_calculate_pnls_partially_closed(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
    ) {
        let order1 = crate::orders::builder::OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(betting.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .build();
        let betting_any = betting.clone().into_any();
        let fill1 = TestOrderEventStubs::filled(
            &order1,
            &betting_any,
            None,
            None,
            Some(Price::from("0.5")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );

        let order2 = crate::orders::builder::OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(betting.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("50"))
            .build();
        let fill2 = TestOrderEventStubs::filled(
            &order2,
            &betting_any,
            None,
            None,
            Some(Price::from("0.8")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );

        let position = Position::new(&betting_any, fill1.into());
        let fill2_owned: crate::events::OrderFilled = fill2.into();
        let result = betting_account
            .calculate_pnls(&betting_any, &fill2_owned, Some(position))
            .unwrap();

        assert_eq!(result, vec![Money::from("40 GBP")]);
    }

    #[rstest]
    fn test_calculate_commission_invalid_liquidity_side_raises(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
    ) {
        let result = betting_account.calculate_commission(
            &betting.into_any(),
            Quantity::from("1"),
            Price::from("1"),
            LiquiditySide::NoLiquiditySide,
            None,
        );
        assert!(result.is_err());
    }

    #[rstest]
    #[case(OrderSide::Buy, "5.0", "100", "-400 GBP")]
    #[case(OrderSide::Buy, "1.5", "100", "-50 GBP")]
    #[case(OrderSide::Sell, "5.0", "100", "-100 GBP")]
    #[case(OrderSide::Sell, "10.0", "100", "-100 GBP")]
    fn test_balance_impact(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
        #[case] side: OrderSide,
        #[case] price: &str,
        #[case] quantity: &str,
        #[case] expected: &str,
    ) {
        let impact = betting_account.balance_impact(
            &betting.into_any(),
            Quantity::from(quantity),
            Price::from(price),
            side,
        );

        assert_eq!(impact, Money::from(expected));
    }

    #[rstest]
    fn test_apply_rejects_negative_balance(mut betting_account: BettingAccount) {
        let negative_state = AccountState::new(
            AccountId::from("SIM-001"),
            AccountType::Betting,
            vec![AccountBalance::new(
                Money::from("-50 GBP"),
                Money::from("0 GBP"),
                Money::from("-50 GBP"),
            )],
            vec![],
            false,
            crate::identifiers::stubs::uuid4(),
            0.into(),
            0.into(),
            Some(Currency::GBP()),
        );

        let result = betting_account.apply(negative_state);
        assert!(result.is_err());
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("balance would be negative")
        );
    }

    #[rstest]
    fn test_update_balances_rejects_negative_total(mut betting_account: BettingAccount) {
        let result = betting_account.update_balances(&[AccountBalance::new(
            Money::from("-10 GBP"),
            Money::from("0 GBP"),
            Money::from("-10 GBP"),
        )]);

        assert!(result.is_err());
    }

    #[rstest]
    fn test_recalculate_balance_clamps_locked_to_total(mut betting_account: BettingAccount) {
        let instrument_id =
            crate::identifiers::InstrumentId::from("BETFAIR-1.2345678-12345678-0.0.NONE");

        betting_account.update_balance_locked(instrument_id, Money::from("1500 GBP"));

        let balance = betting_account.balance(Some(Currency::GBP())).unwrap();
        assert_eq!(balance.locked, Money::from("1000 GBP"));
        assert_eq!(balance.free, Money::from("0 GBP"));
        assert_eq!(balance.total, Money::from("1000 GBP"));
    }

    #[rstest]
    fn test_calculate_pnls_sell_fill(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
    ) {
        let order = crate::orders::builder::OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(betting.id())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("100"))
            .build();
        let betting_any = betting.into_any();
        let fill = TestOrderEventStubs::filled(
            &order,
            &betting_any,
            None,
            None,
            Some(Price::from("0.8")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        );
        let position = Position::new(&betting_any, fill.clone().into());
        let fill_owned: crate::events::OrderFilled = fill.into();

        let result = betting_account
            .calculate_pnls(&betting_any, &fill_owned, Some(position))
            .unwrap();

        assert_eq!(result, vec![Money::from("80 GBP")]);
    }

    #[rstest]
    fn test_calculate_balance_locked_rejects_non_betting_instrument(
        mut betting_account: BettingAccount,
    ) {
        let audusd = crate::instruments::stubs::audusd_sim();
        let result = betting_account.calculate_balance_locked(
            &audusd.into(),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("1.5"),
            None,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("sports betting"));
    }
}
