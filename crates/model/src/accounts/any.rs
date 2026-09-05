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

//! Enum wrapper providing a type-erased view over the various concrete [`Account`] implementations.
//!
//! The `AccountAny` enum is primarily used when heterogeneous account types need to be stored in a
//! single collection (e.g. `Vec<AccountAny>`).  Each variant simply embeds one of the concrete
//! account structs defined in this module.

use enum_dispatch::enum_dispatch;
use indexmap::IndexMap;
use nautilus_core::correctness::{CorrectnessResult, CorrectnessResultExt, FAILED};
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, BettingAccount, CashAccount, MarginAccount, WalletAccount},
    enums::{AccountType, LiquiditySide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    instruments::InstrumentAny,
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

/// Represents any account type, so accounts can be held in one collection.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[enum_dispatch(Account)]
pub enum AccountAny {
    /// A margin account holding leveraged positions.
    Margin(MarginAccount),
    /// A cash account holding unleveraged positions.
    Cash(CashAccount),
    /// A betting account holding backed and laid stakes.
    Betting(BettingAccount),
    /// A blockchain wallet account holding native and token balances.
    Wallet(WalletAccount),
}

impl AccountAny {
    /// Returns a copy without stored account state events.
    #[must_use]
    pub fn clone_without_events(&self) -> Self {
        match self {
            Self::Margin(margin) => Self::Margin(margin.clone_without_events()),
            Self::Cash(cash) => Self::Cash(cash.clone_without_events()),
            Self::Betting(betting) => Self::Betting(betting.clone_without_events()),
            Self::Wallet(wallet) => Self::Wallet(wallet.clone_without_events()),
        }
    }

    #[must_use]
    pub fn id(&self) -> AccountId {
        Account::id(self)
    }

    #[must_use]
    pub fn last_event(&self) -> Option<AccountState> {
        Account::last_event(self)
    }

    #[must_use]
    pub fn events(&self) -> Vec<AccountState> {
        Account::events(self)
    }

    /// Applies an account state event to update the account.
    ///
    /// # Errors
    ///
    /// Returns an error if the event belongs to a different account or the account state cannot be
    /// applied (e.g., negative balance when borrowing is not allowed for a cash account).
    pub fn apply(&mut self, event: AccountState) -> anyhow::Result<()> {
        Account::apply(self, event)
    }

    /// Sets whether account state should be recalculated from order fills.
    pub fn set_calculate_account_state(&mut self, calculate_account_state: bool) {
        match self {
            Self::Margin(margin) => margin.base.calculate_account_state = calculate_account_state,
            Self::Cash(cash) => cash.base.calculate_account_state = calculate_account_state,
            Self::Betting(betting) => {
                betting.base.calculate_account_state = calculate_account_state;
            }
            Self::Wallet(wallet) => {
                wallet.base.calculate_account_state = calculate_account_state;
            }
        }
    }

    #[must_use]
    pub fn balances(&self) -> IndexMap<Currency, AccountBalance> {
        Account::balances(self)
    }

    #[must_use]
    pub fn balances_locked(&self) -> IndexMap<Currency, Money> {
        Account::balances_locked(self)
    }

    #[must_use]
    pub fn base_currency(&self) -> Option<Currency> {
        Account::base_currency(self)
    }

    /// # Errors
    ///
    /// Returns an error if `events` is empty or an account state cannot be created or applied.
    pub fn from_events(events: &[AccountState]) -> anyhow::Result<Self> {
        let Some((init_event, remaining_events)) = events.split_first() else {
            anyhow::bail!("No account events provided to create `AccountAny`");
        };

        let mut account = Self::from_state_checked(init_event.clone())?;

        for event in remaining_events {
            account.apply(event.clone())?;
        }

        Ok(account)
    }

    /// # Errors
    ///
    /// Returns an error if calculating P&Ls fails for the underlying account.
    pub fn calculate_pnls(
        &self,
        instrument: &InstrumentAny,
        fill: &OrderFilled,
        position: Option<Position>,
    ) -> anyhow::Result<Vec<Money>> {
        Account::calculate_pnls(self, instrument, fill, position)
    }

    /// # Errors
    ///
    /// Returns an error if calculating commission fails for the underlying account.
    pub fn calculate_commission(
        &self,
        instrument: &InstrumentAny,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
    ) -> anyhow::Result<Money> {
        Account::calculate_commission(
            self,
            instrument,
            last_qty,
            last_px,
            liquidity_side,
            use_quote_for_inverse,
        )
    }

    #[must_use]
    pub fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        Account::balance(self, currency)
    }
}

impl AccountAny {
    /// Creates an `AccountAny` from an `AccountState`.
    ///
    /// # Errors
    ///
    /// Returns an error if a wallet account state is invalid.
    pub fn try_from_state(event: AccountState) -> Result<Self, &'static str> {
        Self::from_state_checked(event).map_err(|_| "Invalid wallet account state")
    }

    fn from_state_checked(event: AccountState) -> CorrectnessResult<Self> {
        match event.account_type {
            AccountType::Margin => Ok(Self::Margin(MarginAccount::new(event, false))),
            AccountType::Cash => Ok(Self::Cash(CashAccount::new(event, false, false))),
            AccountType::Betting => Ok(Self::Betting(BettingAccount::new(event, false))),
            AccountType::Wallet => Ok(Self::Wallet(WalletAccount::new_checked(event, false)?)),
        }
    }
}

impl From<AccountState> for AccountAny {
    /// Creates an `AccountAny` from an `AccountState`.
    ///
    /// # Panics
    ///
    /// Panics if a wallet account state is invalid.
    /// Use [`AccountAny::try_from_state`] for fallible conversion.
    fn from(event: AccountState) -> Self {
        Self::from_state_checked(event).expect_display(FAILED)
    }
}

impl PartialEq for AccountAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use rust_decimal::Decimal;

    use crate::{
        accounts::{
            Account, AccountAny,
            margin_model::{MarginModel, MarginModelAny, StandardMarginModel},
        },
        events::{AccountState, account::stubs::*},
        identifiers::{AccountId, InstrumentId},
        types::Money,
    };

    #[rstest]
    fn test_from_events_empty_returns_error() {
        let events: Vec<AccountState> = vec![];
        let result = AccountAny::from_events(&events);

        assert_eq!(
            result.unwrap_err().to_string(),
            "No account events provided to create `AccountAny`"
        );
    }

    #[rstest]
    fn test_from_events_single_cash_event(cash_account_state: AccountState) {
        let result = AccountAny::from_events(&[cash_account_state]);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Cash(_)));
    }

    #[rstest]
    fn test_from_events_rejects_different_account(cash_account_state: AccountState) {
        let mut different_account = cash_account_state.clone();
        different_account.account_id = AccountId::from("OTHER-001");

        let result = AccountAny::from_events(&[cash_account_state, different_account]);

        assert_eq!(
            result.unwrap_err().to_string(),
            "Account event had a different account ID: expected SIM-001, received OTHER-001"
        );
    }

    #[rstest]
    #[case::cash(cash_account_state())]
    #[case::margin(margin_account_state())]
    #[case::betting(betting_account_state())]
    #[case::wallet(wallet_account_state())]
    fn test_apply_rejects_different_account_without_mutation(#[case] state: AccountState) {
        let mut account = AccountAny::try_from_state(state.clone()).unwrap();
        let balances_before = account.balances();
        let mut foreign = state;
        foreign.account_id = AccountId::from("OTHER-001");

        let error = account.apply(foreign).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Account event had a different account ID: expected SIM-001, received OTHER-001"
        );
        assert_eq!(account.event_count(), 1);
        assert_eq!(account.balances(), balances_before);
    }

    #[rstest]
    fn test_from_events_single_margin_event(margin_account_state: AccountState) {
        let result = AccountAny::from_events(&[margin_account_state]);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Margin(_)));
    }

    #[rstest]
    #[case::cash(cash_account_state())]
    #[case::margin(margin_account_state())]
    #[case::betting(betting_account_state())]
    #[case::wallet(wallet_account_state())]
    fn test_clone_without_events_preserves_current_state(#[case] state: AccountState) {
        let currency = state.balances[0].currency;
        let instrument_id = InstrumentId::from("CLONE-TEST.SIM");
        let locked = Money::from_decimal(Decimal::new(725, 2), currency).unwrap();
        let commission = Money::from_decimal(Decimal::new(135, 2), currency).unwrap();
        let mut account = AccountAny::try_from_state(state.clone()).unwrap();
        account.apply(state).unwrap();

        let base = match &mut account {
            AccountAny::Margin(account) => &mut account.base,
            AccountAny::Cash(account) => &mut account.base,
            AccountAny::Betting(account) => &mut account.base,
            AccountAny::Wallet(account) => &mut account.base,
        };
        base.calculate_account_state = true;
        base.commissions.insert(currency, commission);

        match &mut account {
            AccountAny::Margin(account) => {
                account.set_default_leverage(Decimal::new(7, 0));
                account.set_leverage(instrument_id, Decimal::new(3, 0));
                account.set_margin_model(MarginModelAny::Standard(StandardMarginModel).into());
            }
            AccountAny::Cash(account) => {
                account.allow_borrowing = true;
                account
                    .balances_locked
                    .insert((instrument_id, currency), locked);
            }
            AccountAny::Betting(account) => {
                account
                    .balances_locked
                    .insert((instrument_id, currency), locked);
            }
            AccountAny::Wallet(account) => {
                account
                    .balances_locked
                    .insert((instrument_id, currency), locked);
            }
        }

        let cloned = account.clone_without_events();
        let mut expected = account.clone();

        match &mut expected {
            AccountAny::Margin(account) => account.base.events.clear(),
            AccountAny::Cash(account) => account.base.events.clear(),
            AccountAny::Betting(account) => account.base.events.clear(),
            AccountAny::Wallet(account) => account.base.events.clear(),
        }

        assert_eq!(account.event_count(), 2);
        assert_eq!(cloned.event_count(), 0);
        match (&account, &cloned) {
            (AccountAny::Margin(source), AccountAny::Margin(cloned)) => {
                assert_eq!(cloned.margin_model().name(), source.margin_model().name());
                assert_eq!(cloned.margin_model().name(), "standard");
            }
            (AccountAny::Cash(source), AccountAny::Cash(cloned)) => {
                assert_eq!(cloned.balances_locked, source.balances_locked);
                assert!(cloned.allow_borrowing);
            }
            (AccountAny::Betting(source), AccountAny::Betting(cloned)) => {
                assert_eq!(cloned.balances_locked, source.balances_locked);
            }
            (AccountAny::Wallet(source), AccountAny::Wallet(cloned)) => {
                assert_eq!(cloned.balances_locked, source.balances_locked);
            }
            _ => panic!("cloned account variant changed"),
        }
        assert_eq!(
            serde_json::to_value(&cloned).unwrap(),
            serde_json::to_value(&expected).unwrap()
        );
    }

    #[rstest]
    fn test_try_from_state_cash(cash_account_state: AccountState) {
        let result: Result<AccountAny, &'static str> =
            AccountAny::try_from_state(cash_account_state);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Cash(_)));
    }

    #[rstest]
    fn test_try_from_state_margin(margin_account_state: AccountState) {
        let result = AccountAny::try_from_state(margin_account_state);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Margin(_)));
    }

    #[rstest]
    fn test_try_from_state_betting(betting_account_state: AccountState) {
        let result = AccountAny::try_from_state(betting_account_state);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Betting(_)));
    }

    #[rstest]
    fn test_try_from_state_wallet(wallet_account_state: AccountState) {
        let result = AccountAny::try_from_state(wallet_account_state);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Wallet(_)));
    }

    #[rstest]
    fn test_try_from_state_invalid_wallet_returns_static_error() {
        let result: Result<AccountAny, &'static str> =
            AccountAny::try_from_state(invalid_wallet_state());

        assert_eq!(result.unwrap_err(), "Invalid wallet account state");
    }

    #[rstest]
    fn test_from_events_wallet_applies_sequence(
        wallet_account_state: AccountState,
        wallet_account_state_changed: AccountState,
    ) {
        let result = AccountAny::from_events(&[wallet_account_state, wallet_account_state_changed]);
        assert!(result.is_ok());
        let account = result.unwrap();
        assert!(matches!(account, AccountAny::Wallet(_)));
        assert_eq!(account.event_count(), 2);
    }

    #[rstest]
    fn test_from_events_wallet_rejects_negative_initial_balance() {
        let result = AccountAny::from_events(&[invalid_wallet_state()]);

        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "Wallet account balance total was negative"
        );
    }

    #[rstest]
    #[case::cash(cash_account_state(), "Cash")]
    #[case::margin(margin_account_state(), "Margin")]
    #[case::betting(betting_account_state(), "Betting")]
    #[case::wallet(wallet_account_state(), "Wallet")]
    fn test_serde_round_trip_preserves_variant_payload(
        #[case] state: AccountState,
        #[case] expected_variant: &str,
    ) {
        let account = AccountAny::try_from_state(state).unwrap();

        let value = serde_json::to_value(&account).unwrap();
        let object = value.as_object().unwrap();
        assert_eq!(object.len(), 1);
        assert!(object.contains_key(expected_variant));

        let deserialized: AccountAny = serde_json::from_value(value).unwrap();
        assert_eq!(deserialized.id(), account.id());
        assert_eq!(deserialized.events(), account.events());
        assert_eq!(deserialized.balances(), account.balances());
    }

    #[rstest]
    #[case::cash(include_str!("../../test_data/account_legacy_cash.json"), "Cash")]
    #[case::margin(include_str!("../../test_data/account_legacy_margin.json"), "Margin")]
    #[case::betting(include_str!("../../test_data/account_legacy_betting.json"), "Betting")]
    fn test_deserializes_legacy_payload(#[case] json: &str, #[case] expected_variant: &str) {
        let account: AccountAny = serde_json::from_str(json).unwrap();
        let variant = match &account {
            AccountAny::Cash(_) => "Cash",
            AccountAny::Margin(_) => "Margin",
            AccountAny::Betting(_) => "Betting",
            AccountAny::Wallet(_) => "Wallet",
        };
        assert_eq!(variant, expected_variant);
        assert_eq!(account.event_count(), 1);
    }

    fn invalid_wallet_state() -> AccountState {
        AccountState::new(
            AccountId::from("WALLET-001"),
            crate::enums::AccountType::Wallet,
            vec![crate::types::AccountBalance::new(
                crate::types::Money::from("-1 ETH"),
                crate::types::Money::from("0 ETH"),
                crate::types::Money::from("-1 ETH"),
            )],
            vec![],
            true,
            crate::identifiers::stubs::uuid4(),
            0.into(),
            0.into(),
            None,
        )
    }
}
