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
use rust_decimal::Decimal;
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{
        Account,
        base::{self, BaseAccount},
    },
    enums::{InstrumentClass, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::InstrumentId,
    instruments::{Instrument, InstrumentAny},
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

/// Represents a betting account that stakes on sports betting markets.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct BettingAccount {
    /// The account state shared by every account type.
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

    #[must_use]
    pub(crate) fn clone_without_events(&self) -> Self {
        Self {
            base: self.base.clone_without_events(),
            balances_locked: self.balances_locked.clone(),
        }
    }

    /// Updates the locked balance for the given instrument and currency.
    ///
    /// # Errors
    ///
    /// Returns an error if `locked` is negative, its precision differs from the balance
    /// precision, or the reservations cannot produce a valid balance. The balance and
    /// reservations are left unchanged when an error is returned.
    pub fn update_balance_locked(
        &mut self,
        instrument_id: InstrumentId,
        locked: Money,
    ) -> anyhow::Result<()> {
        base::update_balance_locked(
            &mut self.base.balances,
            &mut self.balances_locked,
            instrument_id,
            locked,
        )
    }

    /// Clears all locked balances for the given instrument ID.
    pub fn clear_balance_locked(&mut self, instrument_id: InstrumentId) {
        base::clear_balance_locked(
            &mut self.base.balances,
            &mut self.balances_locked,
            instrument_id,
        );
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
    /// Panics if the impact cannot be represented in the quote currency.
    #[must_use]
    pub fn balance_impact(
        &self,
        instrument: &InstrumentAny,
        quantity: Quantity,
        price: Price,
        order_side: OrderSide,
    ) -> Money {
        let currency = instrument.quote_currency();
        let impact = match order_side {
            OrderSide::Sell => -quantity.as_decimal(),
            OrderSide::Buy => -(quantity.as_decimal() * (price.as_decimal() - Decimal::ONE)),
        };
        Money::from_decimal(impact, currency).expect("invalid betting balance impact")
    }

    /// Recalculates the account balance for the specified currency based on per-instrument locks.
    pub fn recalculate_balance(&mut self, currency: Currency) {
        base::recalculate_balance(&mut self.base.balances, &self.balances_locked, currency);
    }
}

impl Account for BettingAccount {
    impl_account_base_members!();

    fn is_cash_account(&self) -> bool {
        true
    }

    fn is_margin_account(&self) -> bool {
        false
    }

    fn apply(&mut self, event: AccountState) -> anyhow::Result<()> {
        self.check_event_account_id(&event)?;

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

    fn calculate_balance_locked(
        &self,
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
            OrderSide::Sell => quantity.as_decimal(),
            OrderSide::Buy => quantity.as_decimal() * (price.as_decimal() - Decimal::ONE),
        };

        Ok(Money::from_decimal(locked, instrument.quote_currency())?)
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

        let quote_pnl = Money::from_decimal(
            fill.last_px.as_decimal() * fill_qty.as_decimal(),
            quote_currency,
        )?;

        match fill.order_side {
            OrderSide::Buy => {
                if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                    pnls.insert(
                        base_currency_value,
                        Money::from_decimal(fill_qty.as_decimal(), base_currency_value)?,
                    );
                }
                pnls.insert(quote_currency, -quote_pnl);
            }
            OrderSide::Sell => {
                if let (Some(base_currency_value), None) = (base_currency, self.base_currency) {
                    pnls.insert(
                        base_currency_value,
                        -Money::from_decimal(fill_qty.as_decimal(), base_currency_value)?,
                    );
                }
                pnls.insert(quote_currency, quote_pnl);
            }
        }

        Ok(pnls.into_values().collect())
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
    use rust_decimal::Decimal;

    use crate::{
        accounts::{Account, BettingAccount, stubs::*},
        enums::{AccountType, CurrencyType, LiquiditySide, OrderSide},
        events::{AccountState, account::stubs::*},
        identifiers::{AccountId, InstrumentId},
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
        betting_account: BettingAccount,
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
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("Invalid `LiquiditySide`: NO_LIQUIDITY_SIDE")
        );
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

        betting_account
            .update_balance_locked(instrument_id, Money::from("1500 GBP"))
            .unwrap();

        let balance = betting_account.balance(Some(Currency::GBP())).unwrap();
        assert_eq!(balance.locked, Money::from("1000 GBP"));
        assert_eq!(balance.free, Money::from("0 GBP"));
        assert_eq!(balance.total, Money::from("1000 GBP"));
    }

    #[rstest]
    fn test_update_balance_locked_precision_mismatch_preserves_state(
        mut betting_account: BettingAccount,
    ) {
        let instrument_id = InstrumentId::from("BETFAIR-1.2345678-12345678-0.0.NONE");
        let gbp = Currency::GBP();
        betting_account
            .update_balance_locked(instrument_id, Money::from("100 GBP"))
            .unwrap();
        let balance_before = *betting_account.balance(Some(gbp)).unwrap();
        let locks_before = betting_account.balances_locked.clone();
        let mismatched_gbp = Currency::new(
            "GBP",
            gbp.precision + 1,
            826,
            "Pound Sterling",
            CurrencyType::Fiat,
        );
        let locked = Money::from_decimal(Decimal::from(50), mismatched_gbp).unwrap();

        let error = betting_account
            .update_balance_locked(instrument_id, locked)
            .unwrap_err();

        assert_eq!(
            error.to_string(),
            "Cannot update GBP reservation: precision 3 differed from balance precision 2"
        );
        assert_eq!(betting_account.balance(Some(gbp)), Some(&balance_before));
        assert_eq!(betting_account.balances_locked, locks_before);
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
        betting_account: BettingAccount,
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

    #[rstest]
    fn test_calculate_balance_locked_rejects_use_quote_for_inverse(
        betting_account: BettingAccount,
        betting: crate::instruments::BettingInstrument,
    ) {
        let result = betting_account.calculate_balance_locked(
            &betting.into_any(),
            OrderSide::Buy,
            Quantity::from("100"),
            Price::from("1.5"),
            Some(true),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "`use_quote_for_inverse` is not applicable for betting accounts"
        );
    }

    #[rstest]
    fn test_calculate_pnls_rejects_non_betting_instrument(betting_account: BettingAccount) {
        let audusd = crate::instruments::stubs::audusd_sim();
        let audusd_any = audusd.into_any();
        let order = crate::orders::builder::OrderTestBuilder::new(crate::enums::OrderType::Market)
            .instrument_id(audusd_any.id())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100000"))
            .build();
        let fill: crate::events::OrderFilled = TestOrderEventStubs::filled(
            &order,
            &audusd_any,
            None,
            None,
            Some(Price::from("0.8")),
            None,
            None,
            None,
            None,
            Some(AccountId::from("SIM-001")),
        )
        .into();

        let result = betting_account.calculate_pnls(&audusd_any, &fill, None);

        assert_eq!(
            result.unwrap_err().to_string(),
            "BettingAccount requires a sports betting instrument"
        );
    }
}
