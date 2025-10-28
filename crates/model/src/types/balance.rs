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

//! Represents an account balance denominated in a particular currency.

use std::fmt::{Debug, Display};

use nautilus_core::correctness::{FAILED, check_predicate_true};
use serde::{Deserialize, Serialize};

use crate::{
    identifiers::InstrumentId,
    types::{Currency, Money},
};

/// Represents an account balance denominated in a particular currency.
#[derive(Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", frozen, eq)
)]
pub struct AccountBalance {
    /// The account balance currency.
    pub currency: Currency,
    /// The total account balance.
    pub total: Money,
    /// The account balance locked (assigned to pending orders).
    pub locked: Money,
    /// The account balance free for trading.
    pub free: Money,
}

impl AccountBalance {
    /// Creates a new [`AccountBalance`] instance with correctness checking.
    ///
    /// # Errors
    ///
    /// Returns an error if `total` is not the result of `locked` + `free`.
    ///
    /// # Notes
    ///
    /// PyO3 requires a `Result` type that stacktrace can be printed for errors.
    pub fn new_checked(total: Money, locked: Money, free: Money) -> anyhow::Result<Self> {
        check_predicate_true(
            total == locked + free,
            &format!("`total` ({total}) - `locked` ({locked}) != `free` ({free})"),
        )?;
        Ok(Self {
            currency: total.currency,
            total,
            locked,
            free,
        })
    }

    /// Creates a new [`AccountBalance`] instance.
    ///
    /// # Panics
    ///
    /// Panics if a correctness check fails. See [`AccountBalance::new_checked`] for more details.
    pub fn new(total: Money, locked: Money, free: Money) -> Self {
        Self::new_checked(total, locked, free).expect(FAILED)
    }
}

impl PartialEq for AccountBalance {
    fn eq(&self, other: &Self) -> bool {
        self.total == other.total && self.locked == other.locked && self.free == other.free
    }
}

impl Debug for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(total={}, locked={}, free={})",
            stringify!(AccountBalance),
            self.total,
            self.locked,
            self.free,
        )
    }
}

impl Display for AccountBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

#[derive(Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", frozen, eq)
)]
pub struct MarginBalance {
    pub initial: Money,
    pub maintenance: Money,
    pub currency: Currency,
    pub instrument_id: InstrumentId,
}

impl MarginBalance {
    /// Creates a new [`MarginBalance`] instance.
    pub fn new(initial: Money, maintenance: Money, instrument_id: InstrumentId) -> Self {
        Self {
            initial,
            maintenance,
            currency: initial.currency,
            instrument_id,
        }
    }
}

impl PartialEq for MarginBalance {
    fn eq(&self, other: &Self) -> bool {
        self.initial == other.initial
            && self.maintenance == other.maintenance
            && self.instrument_id == other.instrument_id
    }
}

impl Debug for MarginBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(initial={}, maintenance={}, instrument_id={})",
            stringify!(MarginBalance),
            self.initial,
            self.maintenance,
            self.instrument_id,
        )
    }
}

impl Display for MarginBalance {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::types::{
        AccountBalance, MarginBalance,
        stubs::{stub_account_balance, stub_margin_balance},
    };

    #[rstest]
    fn test_account_balance_equality() {
        let account_balance_1 = stub_account_balance();
        let account_balance_2 = stub_account_balance();
        assert_eq!(account_balance_1, account_balance_2);
    }

    #[rstest]
    fn test_account_balance_debug(stub_account_balance: AccountBalance) {
        let result = format!("{stub_account_balance:?}");
        let expected =
            "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_account_balance_display(stub_account_balance: AccountBalance) {
        let result = format!("{stub_account_balance}");
        let expected =
            "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)";
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_margin_balance_equality() {
        let margin_balance_1 = stub_margin_balance();
        let margin_balance_2 = stub_margin_balance();
        assert_eq!(margin_balance_1, margin_balance_2);
    }

    #[rstest]
    fn test_margin_balance_debug(stub_margin_balance: MarginBalance) {
        let display = format!("{stub_margin_balance:?}");
        assert_eq!(
            "MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)",
            display
        );
    }

    #[rstest]
    fn test_margin_balance_display(stub_margin_balance: MarginBalance) {
        let display = format!("{stub_margin_balance}");
        assert_eq!(
            "MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)",
            display
        );
    }
}
