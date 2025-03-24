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

use std::fmt::{Display, Formatter};

use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AccountType,
    identifiers::AccountId,
    types::{AccountBalance, Currency, MarginBalance},
};

/// Represents an event which includes information on the state of the account.
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct AccountState {
    /// The account ID associated with the event.
    pub account_id: AccountId,
    /// The type of the account (e.g., margin, spot, etc.).
    pub account_type: AccountType,
    /// The base currency for the account, if applicable.
    pub base_currency: Option<Currency>,
    /// The balances in the account.
    pub balances: Vec<AccountBalance>,
    /// The margin balances in the account.
    pub margins: Vec<MarginBalance>,
    /// Indicates if the account state is reported by the exchange
    /// (as opposed to system-calculated).
    pub is_reported: bool,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl AccountState {
    /// Creates a new [`AccountState`] instance.
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
    ) -> Self {
        Self {
            account_id,
            account_type,
            base_currency,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for AccountState {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(account_id={}, account_type={}, base_currency={}, is_reported={}, balances=[{}], margins=[{}], event_id={})",
            stringify!(AccountState),
            self.account_id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
            self.is_reported,
            self.balances
                .iter()
                .map(|b| format!("{b}"))
                .collect::<Vec<String>>()
                .join(","),
            self.margins
                .iter()
                .map(|m| format!("{m}"))
                .collect::<Vec<String>>()
                .join(","),
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

    use crate::events::{
        AccountState,
        account::stubs::{cash_account_state, margin_account_state},
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
