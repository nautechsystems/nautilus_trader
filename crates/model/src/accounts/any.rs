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
use serde::{Deserialize, Serialize};

use crate::{
    accounts::{Account, BettingAccount, CashAccount, MarginAccount},
    enums::{AccountType, LiquiditySide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    instruments::InstrumentAny,
    position::Position,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[enum_dispatch(Account)]
pub enum AccountAny {
    Margin(MarginAccount),
    Cash(CashAccount),
    Betting(BettingAccount),
}

impl AccountAny {
    #[must_use]
    pub fn id(&self) -> AccountId {
        match self {
            Self::Margin(margin) => margin.id,
            Self::Cash(cash) => cash.id,
            Self::Betting(betting) => betting.id,
        }
    }

    #[must_use]
    pub fn last_event(&self) -> Option<AccountState> {
        match self {
            Self::Margin(margin) => margin.last_event(),
            Self::Cash(cash) => cash.last_event(),
            Self::Betting(betting) => betting.last_event(),
        }
    }

    #[must_use]
    pub fn events(&self) -> Vec<AccountState> {
        match self {
            Self::Margin(margin) => margin.events(),
            Self::Cash(cash) => cash.events(),
            Self::Betting(betting) => betting.events(),
        }
    }

    /// Applies an account state event to update the account.
    ///
    /// # Errors
    ///
    /// Returns an error if the account state cannot be applied (e.g., negative balance
    /// when borrowing is not allowed for a cash account).
    pub fn apply(&mut self, event: AccountState) -> anyhow::Result<()> {
        match self {
            Self::Margin(margin) => margin.apply(event),
            Self::Cash(cash) => cash.apply(event),
            Self::Betting(betting) => betting.apply(event),
        }
    }

    #[must_use]
    pub fn balances(&self) -> IndexMap<Currency, AccountBalance> {
        match self {
            Self::Margin(margin) => margin.balances(),
            Self::Cash(cash) => cash.balances(),
            Self::Betting(betting) => betting.balances(),
        }
    }

    #[must_use]
    pub fn balances_locked(&self) -> IndexMap<Currency, Money> {
        match self {
            Self::Margin(margin) => margin.balances_locked(),
            Self::Cash(cash) => cash.balances_locked(),
            Self::Betting(betting) => betting.balances_locked(),
        }
    }

    #[must_use]
    pub fn base_currency(&self) -> Option<Currency> {
        match self {
            Self::Margin(margin) => margin.base_currency(),
            Self::Cash(cash) => cash.base_currency(),
            Self::Betting(betting) => betting.base_currency(),
        }
    }

    /// # Errors
    ///
    /// Returns an error if `events` is empty.
    #[expect(clippy::missing_panics_doc)] // Guarded by empty check above
    pub fn from_events(events: &[AccountState]) -> anyhow::Result<Self> {
        if events.is_empty() {
            anyhow::bail!("No order events provided to create `AccountAny`");
        }

        let init_event = events.first().unwrap();
        let mut account = Self::from(init_event.clone());
        for event in events.iter().skip(1) {
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
        match self {
            Self::Margin(margin) => margin.calculate_pnls(instrument, fill, position),
            Self::Cash(cash) => cash.calculate_pnls(instrument, fill, position),
            Self::Betting(betting) => betting.calculate_pnls(instrument, fill, position),
        }
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
        match self {
            Self::Margin(margin) => margin.calculate_commission(
                instrument,
                last_qty,
                last_px,
                liquidity_side,
                use_quote_for_inverse,
            ),
            Self::Cash(cash) => cash.calculate_commission(
                instrument,
                last_qty,
                last_px,
                liquidity_side,
                use_quote_for_inverse,
            ),
            Self::Betting(betting) => betting.calculate_commission(
                instrument,
                last_qty,
                last_px,
                liquidity_side,
                use_quote_for_inverse,
            ),
        }
    }

    #[must_use]
    pub fn balance(&self, currency: Option<Currency>) -> Option<&AccountBalance> {
        match self {
            Self::Margin(margin) => margin.balance(currency),
            Self::Cash(cash) => cash.balance(currency),
            Self::Betting(betting) => betting.balance(currency),
        }
    }
}

impl AccountAny {
    /// Creates an `AccountAny` from an `AccountState`, returning an error for unsupported types.
    ///
    /// # Errors
    ///
    /// Returns an error if the account type is `Wallet` (unsupported in Rust).
    pub fn try_from_state(event: AccountState) -> Result<Self, &'static str> {
        match event.account_type {
            AccountType::Margin => Ok(Self::Margin(MarginAccount::new(event, false))),
            AccountType::Cash => Ok(Self::Cash(CashAccount::new(event, false, false))),
            AccountType::Betting => Ok(Self::Betting(BettingAccount::new(event, false))),
            AccountType::Wallet => Err("Wallet accounts are not yet implemented in Rust"),
        }
    }
}

impl From<AccountState> for AccountAny {
    /// Creates an `AccountAny` from an `AccountState`.
    ///
    /// # Panics
    ///
    /// Panics if the account type is `Wallet` (unsupported in Rust).
    /// Use [`AccountAny::try_from_state`] for fallible conversion.
    fn from(event: AccountState) -> Self {
        Self::try_from_state(event).expect("Unsupported account type")
    }
}

impl PartialEq for AccountAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::UUID4;
    use rstest::rstest;

    use crate::{
        accounts::AccountAny,
        enums::AccountType,
        events::{AccountState, account::stubs::*},
        identifiers::AccountId,
    };

    #[rstest]
    fn test_from_events_empty_returns_error() {
        let events: Vec<AccountState> = vec![];
        let result = AccountAny::from_events(&events);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_from_events_single_cash_event(cash_account_state: AccountState) {
        let result = AccountAny::from_events(&[cash_account_state]);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Cash(_)));
    }

    #[rstest]
    fn test_from_events_single_margin_event(margin_account_state: AccountState) {
        let result = AccountAny::from_events(&[margin_account_state]);
        assert!(result.is_ok());
        assert!(matches!(result.unwrap(), AccountAny::Margin(_)));
    }

    #[rstest]
    fn test_try_from_state_cash(cash_account_state: AccountState) {
        let result = AccountAny::try_from_state(cash_account_state);
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
    fn test_try_from_state_wallet_returns_error() {
        let state = AccountState::new(
            AccountId::from("WALLET-001"),
            AccountType::Wallet,
            vec![],
            vec![],
            true,
            UUID4::default(),
            0.into(),
            0.into(),
            None,
        );
        let result = AccountAny::try_from_state(state);
        assert!(result.is_err());
    }
}
