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

use std::{
    collections::HashMap,
    fmt::{Display, Formatter},
};

use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AccountType,
    identifiers::{AccountId, InstrumentId},
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

    /// Returns `true` if this account state has the same balances and margins as another.
    ///
    /// This compares all balances and margins for equality, returning `true` only if
    /// all balances and margins are equal. If any balance or margin is different or
    /// missing, returns `false`.
    ///
    /// # Note
    ///
    /// This method does not compare event IDs, timestamps, or other metadata - only
    /// the actual balance and margin values.
    pub fn has_same_balances_and_margins(&self, other: &Self) -> bool {
        // Quick check - if lengths differ, they can't be equal
        if self.balances.len() != other.balances.len() || self.margins.len() != other.margins.len()
        {
            return false;
        }

        // Compare balances by currency
        let self_balances: HashMap<Currency, &AccountBalance> = self
            .balances
            .iter()
            .map(|balance| (balance.currency, balance))
            .collect();

        let other_balances: HashMap<Currency, &AccountBalance> = other
            .balances
            .iter()
            .map(|balance| (balance.currency, balance))
            .collect();

        // Check if all balances are equal
        for (currency, self_balance) in &self_balances {
            match other_balances.get(currency) {
                Some(other_balance) => {
                    if self_balance != other_balance {
                        return false;
                    }
                }
                None => return false, // Currency missing in other
            }
        }

        // Compare margins by instrument_id
        let self_margins: HashMap<InstrumentId, &MarginBalance> = self
            .margins
            .iter()
            .map(|margin| (margin.instrument_id, margin))
            .collect();

        let other_margins: HashMap<InstrumentId, &MarginBalance> = other
            .margins
            .iter()
            .map(|margin| (margin.instrument_id, margin))
            .collect();

        // Check if all margins are equal
        for (instrument_id, self_margin) in &self_margins {
            match other_margins.get(instrument_id) {
                Some(other_margin) => {
                    if self_margin != other_margin {
                        return false;
                    }
                }
                None => return false, // Instrument missing in other
            }
        }

        true
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
    use nautilus_core::{UUID4, UnixNanos};
    use rstest::rstest;

    use crate::{
        enums::AccountType,
        events::{
            AccountState,
            account::stubs::{cash_account_state, margin_account_state},
        },
        identifiers::{AccountId, InstrumentId},
        types::{AccountBalance, Currency, MarginBalance, Money},
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

    #[rstest]
    fn test_has_same_balances_and_margins_when_identical() {
        let state1 = cash_account_state();
        let state2 = cash_account_state();
        assert!(state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_balance_amounts() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Create a different balance with same currency
        let usd = Currency::USD();
        let different_balance = AccountBalance::new(
            Money::new(2000000.0, usd),
            Money::new(50000.0, usd),
            Money::new(1950000.0, usd),
        );
        state2.balances = vec![different_balance];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_balance_currencies() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Create a balance with different currency
        let eur = Currency::EUR();
        let different_balance = AccountBalance::new(
            Money::new(1525000.0, eur),
            Money::new(25000.0, eur),
            Money::new(1500000.0, eur),
        );
        state2.balances = vec![different_balance];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_missing_balance() {
        let state1 = cash_account_state();
        let mut state2 = cash_account_state();
        // Add an additional balance to state2
        let eur = Currency::EUR();
        let additional_balance = AccountBalance::new(
            Money::new(1000000.0, eur),
            Money::new(0.0, eur),
            Money::new(1000000.0, eur),
        );
        state2.balances.push(additional_balance);
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_margin_amounts() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Create a different margin with same instrument_id
        let usd = Currency::USD();
        let instrument_id = InstrumentId::from("BTCUSDT.COINBASE");
        let different_margin = MarginBalance::new(
            Money::new(10000.0, usd),
            Money::new(40000.0, usd),
            instrument_id,
        );
        state2.margins = vec![different_margin];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_different_margin_instruments() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Create a margin with different instrument_id
        let usd = Currency::USD();
        let different_instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let different_margin = MarginBalance::new(
            Money::new(5000.0, usd),
            Money::new(20000.0, usd),
            different_instrument_id,
        );
        state2.margins = vec![different_margin];
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_when_missing_margin() {
        let state1 = margin_account_state();
        let mut state2 = margin_account_state();
        // Add an additional margin to state2
        let usd = Currency::USD();
        let additional_instrument_id = InstrumentId::from("ETHUSDT.BINANCE");
        let additional_margin = MarginBalance::new(
            Money::new(3000.0, usd),
            Money::new(15000.0, usd),
            additional_instrument_id,
        );
        state2.margins.push(additional_margin);
        assert!(!state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_with_empty_collections() {
        let account_id = AccountId::new("TEST-001");
        let event_id = UUID4::new();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let state1 = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![], // Empty balances
            vec![], // Empty margins
            true,
            event_id,
            ts_event,
            ts_init,
            Some(Currency::USD()),
        );

        let state2 = AccountState::new(
            account_id,
            AccountType::Cash,
            vec![], // Empty balances
            vec![], // Empty margins
            true,
            UUID4::new(),       // Different event_id
            UnixNanos::from(3), // Different timestamps
            UnixNanos::from(4),
            Some(Currency::USD()),
        );

        assert!(state1.has_same_balances_and_margins(&state2));
    }

    #[rstest]
    fn test_has_same_balances_and_margins_with_multiple_balances_and_margins() {
        let account_id = AccountId::new("TEST-001");
        let event_id = UUID4::new();
        let ts_event = UnixNanos::from(1);
        let ts_init = UnixNanos::from(2);

        let usd = Currency::USD();
        let eur = Currency::EUR();
        let btc_instrument = InstrumentId::from("BTCUSDT.COINBASE");
        let eth_instrument = InstrumentId::from("ETHUSDT.BINANCE");

        let balances = vec![
            AccountBalance::new(
                Money::new(1000000.0, usd),
                Money::new(0.0, usd),
                Money::new(1000000.0, usd),
            ),
            AccountBalance::new(
                Money::new(500000.0, eur),
                Money::new(10000.0, eur),
                Money::new(490000.0, eur),
            ),
        ];

        let margins = vec![
            MarginBalance::new(
                Money::new(5000.0, usd),
                Money::new(20000.0, usd),
                btc_instrument,
            ),
            MarginBalance::new(
                Money::new(3000.0, usd),
                Money::new(15000.0, usd),
                eth_instrument,
            ),
        ];

        let state1 = AccountState::new(
            account_id,
            AccountType::Margin,
            balances.clone(),
            margins.clone(),
            true,
            event_id,
            ts_event,
            ts_init,
            Some(usd),
        );

        let state2 = AccountState::new(
            account_id,
            AccountType::Margin,
            balances,
            margins,
            true,
            UUID4::new(),       // Different event_id
            UnixNanos::from(3), // Different timestamps
            UnixNanos::from(4),
            Some(usd),
        );

        assert!(state1.has_same_balances_and_margins(&state2));
    }
}
