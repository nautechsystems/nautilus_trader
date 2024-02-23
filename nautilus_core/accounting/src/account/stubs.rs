// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

use nautilus_model::{
    enums::LiquiditySide,
    events::account::{state::AccountState, stubs::*},
    instruments::Instrument,
    types::{currency::Currency, money::Money, price::Price, quantity::Quantity},
};
use rstest::fixture;

use crate::account::{cash::CashAccount, margin::MarginAccount, Account};

#[fixture]
pub fn margin_account(margin_account_state: AccountState) -> MarginAccount {
    MarginAccount::new(margin_account_state, true).unwrap()
}

#[fixture]
pub fn cash_account(cash_account_state: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state, true).unwrap()
}

#[fixture]
pub fn cash_account_million_usd(cash_account_state_million_usd: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state_million_usd, true).unwrap()
}

#[fixture]
pub fn cash_account_multi(cash_account_state_multi: AccountState) -> CashAccount {
    CashAccount::new(cash_account_state_multi, true).unwrap()
}

pub fn calculate_commission<T: Instrument>(
    instrument: T,
    quantity: Quantity,
    price: Price,
    currency: Option<Currency>,
) -> Money {
    let account_state = if Some(Currency::USDT()) == currency {
        cash_account_state_million_usdt()
    } else {
        cash_account_state_million_usd()
    };
    let account = cash_account_million_usd(account_state);
    account
        .calculate_commission(instrument, quantity, price, LiquiditySide::Taker, None)
        .unwrap()
}
