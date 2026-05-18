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

use nautilus_core::{UUID4, python::IntoPyObjectNautilusExt};
use pyo3::{basic::CompareOp, prelude::*};

use crate::{
    enums::AccountType,
    events::PortfolioSnapshot,
    identifiers::AccountId,
    types::{AccountBalance, Currency, MarginBalance, Money},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PortfolioSnapshot {
    /// Represents a point-in-time snapshot of portfolio state for a single account,
    /// emitted periodically while the account holds open positions.
    ///
    /// Unlike `AccountState`, which fires only on
    /// balance or margin changes, `PortfolioSnapshot` carries a continuous
    /// mark-to-market view by folding open-position valuations into the totals.
    /// Totals span every venue the account holds positions on, so multi-venue
    /// accounts (e.g., a prime broker routing across exchanges) produce a single
    /// account-wide snapshot rather than per-venue slices.
    #[expect(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (account_id, account_type, balances, margins, unrealized_pnls, realized_pnls, total_equity, event_id, ts_event, ts_init, base_currency=None))]
    fn py_new(
        account_id: AccountId,
        account_type: AccountType,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        unrealized_pnls: Vec<Money>,
        realized_pnls: Vec<Money>,
        total_equity: Vec<Money>,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        base_currency: Option<Currency>,
    ) -> Self {
        Self::new(
            account_id,
            account_type,
            base_currency,
            balances,
            margins,
            unrealized_pnls,
            realized_pnls,
            total_equity,
            event_id,
            ts_event.into(),
            ts_init.into(),
        )
    }

    #[getter]
    fn account_id(&self) -> AccountId {
        self.account_id
    }

    #[getter]
    fn account_type(&self) -> AccountType {
        self.account_type
    }

    #[getter]
    fn base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    fn balances(&self) -> Vec<AccountBalance> {
        self.balances.clone()
    }

    #[getter]
    fn margins(&self) -> Vec<MarginBalance> {
        self.margins.clone()
    }

    #[getter]
    fn unrealized_pnls(&self) -> Vec<Money> {
        self.unrealized_pnls.clone()
    }

    #[getter]
    fn realized_pnls(&self) -> Vec<Money> {
        self.realized_pnls.clone()
    }

    #[getter]
    fn total_equity(&self) -> Vec<Money> {
        self.total_equity.clone()
    }

    #[getter]
    fn event_id(&self) -> UUID4 {
        self.event_id
    }

    #[getter]
    fn ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    fn ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }
}
