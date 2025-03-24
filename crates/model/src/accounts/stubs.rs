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

use rstest::fixture;

use crate::{
    accounts::{Account, AccountAny, CashAccount, MarginAccount},
    enums::{AccountType, LiquiditySide},
    events::account::{state::AccountState, stubs::*},
    identifiers::stubs::{account_id, uuid4},
    instruments::InstrumentAny,
    types::{AccountBalance, Currency, Money, Price, Quantity},
};

impl Default for CashAccount {
    /// Creates a new default [`CashAccount`] instance.
    fn default() -> Self {
        // million dollar account
        let init_event = AccountState::new(
            account_id(),
            AccountType::Cash,
            vec![AccountBalance::new(
                Money::from("1000000 USD"),
                Money::from("0 USD"),
                Money::from("1000000 USD"),
            )],
            vec![],
            true,
            uuid4(),
            0.into(),
            0.into(),
            Some(Currency::USD()),
        );
        Self::new(init_event, false)
    }
}

impl Default for AccountAny {
    /// Creates a new default [`AccountAny`] instance.
    fn default() -> Self {
        AccountAny::Cash(CashAccount::default())
    }
}

#[fixture]
pub fn margin_account(margin_account_state: AccountState) -> MarginAccount {
    MarginAccount::new(margin_account_state, true)
}

#[fixture]
pub fn cash_account(cash_account_state: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state, true)
}

#[fixture]
pub fn cash_account_million_usd(cash_account_state_million_usd: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state_million_usd, true)
}

#[fixture]
pub fn cash_account_multi(cash_account_state_multi: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state_multi, true)
}

#[must_use]
pub fn calculate_commission(
    instrument: InstrumentAny,
    quantity: Quantity,
    price: Price,
    currency: Option<Currency>,
) -> Money {
    let account_state = if Some(Currency::USDT()) == currency {
        cash_account_state_million_usdt()
    } else {
        cash_account_state_million_usd("1000000 USD", "0 USD", "1000000 USD")
    };
    let account = cash_account_million_usd(account_state);
    account
        .calculate_commission(instrument, quantity, price, LiquiditySide::Taker, None)
        .unwrap()
}
