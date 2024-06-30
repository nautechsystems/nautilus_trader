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
    accounts::{cash::CashAccount, margin::MarginAccount},
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
}
