// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

use std::fmt::Display;

use nautilus_core::{UUID4, UnixNanos};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AccountType,
    identifiers::AccountId,
    types::{AccountBalance, Currency, MarginBalance, Money},
};

/// Represents a point-in-time snapshot of portfolio state for a single account,
/// emitted periodically while the account holds open positions.
///
/// Unlike [`AccountState`](crate::events::AccountState), which fires only on
/// balance or margin changes, `PortfolioSnapshot` carries a continuous
/// mark-to-market view by folding open-position valuations into the totals.
/// Totals span every venue the account holds positions on, so multi-venue
/// accounts (e.g., a prime broker routing across exchanges) produce a single
/// account-wide snapshot rather than per-venue slices.
#[repr(C)]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.model", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")
)]
pub struct PortfolioSnapshot {
    /// The account ID this snapshot belongs to.
    pub account_id: AccountId,
    /// The type of the account (e.g., cash, margin).
    pub account_type: AccountType,
    /// The base currency for the account, if applicable.
    pub base_currency: Option<Currency>,
    /// The per-currency account balances at snapshot time.
    pub balances: Vec<AccountBalance>,
    /// The per-instrument margin balances at snapshot time (margin accounts only).
    pub margins: Vec<MarginBalance>,
    /// The per-currency unrealized PnL across all open positions at snapshot time.
    pub unrealized_pnls: Vec<Money>,
    /// The per-currency realized PnL accumulated for positions opened in this session.
    pub realized_pnls: Vec<Money>,
    /// The per-currency total equity (mark-to-market).
    ///
    /// For cash accounts: `balance.total + Σ mark_value(open positions)` in the same currency.
    /// For margin accounts: `balance.total + Σ unrealized_pnl(open positions)` in the same currency.
    pub total_equity: Vec<Money>,
    /// The unique identifier for the event.
    pub event_id: UUID4,
    /// UNIX timestamp (nanoseconds) when the event occurred.
    pub ts_event: UnixNanos,
    /// UNIX timestamp (nanoseconds) when the event was initialized.
    pub ts_init: UnixNanos,
}

impl PortfolioSnapshot {
    /// Creates a new [`PortfolioSnapshot`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Option<Currency>,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        unrealized_pnls: Vec<Money>,
        realized_pnls: Vec<Money>,
        total_equity: Vec<Money>,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            account_id,
            account_type,
            base_currency,
            balances,
            margins,
            unrealized_pnls,
            realized_pnls,
            total_equity,
            event_id,
            ts_event,
            ts_init,
        }
    }
}

impl Display for PortfolioSnapshot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}(account_id={}, account_type={}, total_equity=[{}], unrealized_pnls=[{}], realized_pnls=[{}], event_id={})",
            stringify!(PortfolioSnapshot),
            self.account_id,
            self.account_type,
            self.total_equity
                .iter()
                .map(|m| format!("{m}"))
                .collect::<Vec<_>>()
                .join(", "),
            self.unrealized_pnls
                .iter()
                .map(|m| format!("{m}"))
                .collect::<Vec<_>>()
                .join(", "),
            self.realized_pnls
                .iter()
                .map(|m| format!("{m}"))
                .collect::<Vec<_>>()
                .join(", "),
            self.event_id,
        )
    }
}

impl PartialEq for PortfolioSnapshot {
    fn eq(&self, other: &Self) -> bool {
        self.account_id == other.account_id && self.event_id == other.event_id
    }
}
