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

use serde::{Deserialize, Serialize};

use crate::{
    accounts::{base::Account, cash::CashAccount, margin::MarginAccount},
    enums::AccountType,
    events::account::state::AccountState,
    identifiers::AccountId,
};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AccountAny {
    Margin(MarginAccount),
    Cash(CashAccount),
}

impl AccountAny {
    #[must_use]
    pub fn id(&self) -> AccountId {
        match self {
            AccountAny::Margin(margin) => margin.id,
            AccountAny::Cash(cash) => cash.id,
        }
    }

    pub fn last_event(&self) -> Option<AccountState> {
        match self {
            AccountAny::Margin(margin) => margin.last_event(),
            AccountAny::Cash(cash) => cash.last_event(),
        }
    }

    pub fn events(&self) -> Vec<AccountState> {
        match self {
            AccountAny::Margin(margin) => margin.events(),
            AccountAny::Cash(cash) => cash.events(),
        }
    }

    pub fn apply(&mut self, event: AccountState) {
        match self {
            AccountAny::Margin(margin) => margin.apply(event),
            AccountAny::Cash(cash) => cash.apply(event),
        }
    }

    pub fn from_events(events: Vec<AccountState>) -> anyhow::Result<Self> {
        if events.is_empty() {
            anyhow::bail!("No order events provided to create `AccountAny`");
        }

        let init_event = events.first().unwrap();
        let mut account = Self::from(init_event.clone());
        for event in events.iter().skip(1) {
            account.apply(event.clone());
        }
        Ok(account)
    }
}

impl From<AccountState> for AccountAny {
    fn from(event: AccountState) -> Self {
        match event.account_type {
            AccountType::Margin => AccountAny::Margin(MarginAccount::new(event, false)),
            AccountType::Cash => AccountAny::Cash(CashAccount::new(event, false)),
            AccountType::Betting => todo!("Betting account not implemented"),
        }
    }
}

impl Default for AccountAny {
    fn default() -> Self {
        AccountAny::Cash(CashAccount::default())
    }
}

impl PartialEq for AccountAny {
    fn eq(&self, other: &Self) -> bool {
        self.id() == other.id()
    }
}
