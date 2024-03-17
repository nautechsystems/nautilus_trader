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

use serde::{Deserialize, Serialize};

use crate::{
    identifiers::instrument_id::InstrumentId,
    types::{currency::Currency, money::Money},
};

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct AccountBalance {
    pub currency: Currency,
    pub total: Money,
    pub locked: Money,
    pub free: Money,
}

impl AccountBalance {
    pub fn new(total: Money, locked: Money, free: Money) -> anyhow::Result<Self> {
        assert!(total == locked + free,
                "Total balance is not equal to the sum of locked and free balances: {total} != {locked} + {free}"
            );
        Ok(Self {
            currency: total.currency,
            total,
            locked,
            free,
        })
    }
}

impl Display for AccountBalance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "AccountBalance(total={}, locked={}, free={})",
            self.total, self.locked, self.free,
        )
    }
}

impl PartialEq for AccountBalance {
    fn eq(&self, other: &Self) -> bool {
        self.total == other.total && self.locked == other.locked && self.free == other.free
    }
}

#[derive(Debug, Copy, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model")
)]
pub struct MarginBalance {
    pub initial: Money,
    pub maintenance: Money,
    pub currency: Currency,
    pub instrument_id: InstrumentId,
}

impl MarginBalance {
    pub fn new(
        initial: Money,
        maintenance: Money,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            initial,
            maintenance,
            currency: initial.currency,
            instrument_id,
        })
    }
}

impl Display for MarginBalance {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "MarginBalance(initial={}, maintenance={}, instrument_id={})",
            self.initial, self.maintenance, self.instrument_id,
        )
    }
}

impl PartialEq for MarginBalance {
    fn eq(&self, other: &Self) -> bool {
        self.initial == other.initial
            && self.maintenance == other.maintenance
            && self.instrument_id == other.instrument_id
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use crate::types::{
        balance::{AccountBalance, MarginBalance},
        stubs::{account_balance_test, margin_balance_test},
    };

    #[rstest]
    fn test_account_balance_equality() {
        let account_balance_1 = account_balance_test();
        let account_balance_2 = account_balance_test();
        assert_eq!(account_balance_1, account_balance_2);
    }

    #[rstest]
    fn test_account_balance_display(account_balance_test: AccountBalance) {
        let display = format!("{account_balance_test}");
        assert_eq!(
            "AccountBalance(total=1525000.00 USD, locked=25000.00 USD, free=1500000.00 USD)",
            display
        );
    }

    #[rstest]
    fn test_margin_balance_equality() {
        let margin_balance_1 = margin_balance_test();
        let margin_balance_2 = margin_balance_test();
        assert_eq!(margin_balance_1, margin_balance_2);
    }

    #[rstest]
    fn test_margin_balance_display(margin_balance_test: MarginBalance) {
        let display = format!("{margin_balance_test}");
        assert_eq!(
            "MarginBalance(initial=5000.00 USD, maintenance=20000.00 USD, instrument_id=BTCUSDT.COINBASE)",
            display
        );
    }
}
