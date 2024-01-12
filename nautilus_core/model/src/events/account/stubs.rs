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

use rstest::fixture;

use crate::{
    enums::AccountType,
    events::account::state::AccountState,
    identifiers::stubs::{account_id, uuid4},
    types::{
        currency::Currency,
        stubs::{account_balance_test, margin_balance_test},
    },
};

#[fixture]
pub fn cash_account_state() -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Cash,
        Currency::USD(),
        vec![account_balance_test()],
        vec![],
        true,
        uuid4(),
        0,
        0,
    )
    .unwrap()
}

#[fixture]
pub fn margin_account_state() -> AccountState {
    AccountState::new(
        account_id(),
        AccountType::Margin,
        Currency::USD(),
        vec![account_balance_test()],
        vec![margin_balance_test()],
        true,
        uuid4(),
        0,
        0,
    )
    .unwrap()
}
