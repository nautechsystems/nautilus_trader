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

use std::fmt::{Display, Formatter};

use nautilus_core::{time::UnixNanos, uuid::UUID4};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AccountType,
    identifiers::account_id::AccountId,
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
    },
};

#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct AccountState {
    pub account_id: AccountId,
    pub account_type: AccountType,
    pub base_currency: Option<Currency>,
    pub balances: Vec<AccountBalance>,
    pub margins: Vec<MarginBalance>,
    pub is_reported: bool,
    pub event_id: UUID4,
    pub ts_event: UnixNanos,
    pub ts_init: UnixNanos,
}

impl AccountState {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        account_id: AccountId,
        account_type: AccountType,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        is_reported: bool,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        base_currency: Option<Currency>,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            account_id,
            account_type,
            base_currency,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event,
            ts_init,
        })
    }
}

impl Display for AccountState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AccountState(account_id={}, account_type={}, base_currency={}, is_reported={}, balances=[{}], margins=[{}], event_id={})",
            self.account_id,
            self.account_type,
            self.base_currency.map_or_else(|| "None".to_string(), |base_currency | format!("{}", base_currency.code)),
            self.is_reported,
            self.balances.iter().map(|b| format!("{b}")).collect::<Vec<String>>().join(","),
            self.margins.iter().map(|m| format!("{m}")).collect::<Vec<String>>().join(","),
            self.event_id
        )
    }
}

impl PartialEq for AccountState {
    fn eq(&self, other: &Self) -> bool {
        self.account_id == other.account_id
            && self.account_type == other.account_type
            && self.event_id == other.event_id
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::events::account::{
        state::AccountState,
        stubs::{cash_account_state, margin_account_state},
    };

    #[rstest]
    fn test_equality() {
        let cash_account_state_1 = cash_account_state();
        let cash_account_state_2 = cash_account_state();
        assert_eq!(cash_account_state_1, cash_account_state_2);
    }

    #[rstest]
    fn test_display_cash_account_state(cash_account_state: AccountState) {
        let display = format!("{cash_account_state}");
        assert_eq!(
            display,
            "AccountState(account_id=SIM-001, account_type=CASH, base_currency=USD, is_reported=true, \
            balances=[AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)], \
            margins=[], event_id=16578139-a945-4b65-b46c-bc131a15d8e7)"
        );
    }

    #[rstest]
    fn test_display_margin_account_state(margin_account_state: AccountState) {
        let display = format!("{margin_account_state}");
        assert_eq!(
            display,
            "AccountState(account_id=SIM-001, account_type=MARGIN, base_currency=USD, is_reported=true, \
            balances=[AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)], \
            margins=[MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)], \
            event_id=16578139-a945-4b65-b46c-bc131a15d8e7)"
        );
    }
}
