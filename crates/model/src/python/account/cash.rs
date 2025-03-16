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

use std::collections::HashMap;

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    accounts::{Account, CashAccount},
    enums::{AccountType, LiquiditySide, OrderSide},
    events::{AccountState, OrderFilled},
    identifiers::AccountId,
    position::Position,
    python::instruments::pyobject_to_instrument_any,
    types::{Currency, Money, Price, Quantity},
};

#[pymethods]
impl CashAccount {
    #[new]
    pub fn py_new(event: AccountState, calculate_account_state: bool) -> Self {
        Self::new(event, calculate_account_state)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    #[getter]
    fn id(&self) -> AccountId {
        self.id
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(id={}, type={}, base={})",
            stringify!(CashAccount),
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }

    #[getter]
    #[pyo3(name = "id")]
    fn py_id(&self) -> AccountId {
        self.id
    }

    #[getter]
    #[pyo3(name = "account_type")]
    fn py_account_type(&self) -> AccountType {
        self.account_type
    }

    #[getter]
    #[pyo3(name = "base_currency")]
    fn py_base_currency(&self) -> Option<Currency> {
        self.base_currency
    }

    #[getter]
    #[pyo3(name = "last_event")]
    fn py_last_event(&self) -> Option<AccountState> {
        self.last_event()
    }

    #[getter]
    #[pyo3(name = "event_count")]
    fn py_event_count(&self) -> usize {
        self.event_count()
    }

    #[getter]
    #[pyo3(name = "events")]
    fn py_events(&self) -> Vec<AccountState> {
        self.events()
    }

    #[getter]
    #[pyo3(name = "calculate_account_state")]
    fn py_calculate_account_state(&self) -> bool {
        self.calculate_account_state
    }

    #[pyo3(name = "balance_total")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_total(&self, currency: Option<Currency>) -> Option<Money> {
        self.balance_total(currency)
    }

    #[pyo3(name = "balances_total")]
    fn py_balances_total(&self) -> HashMap<Currency, Money> {
        self.balances_total()
    }

    #[pyo3(name = "balance_free")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_free(&self, currency: Option<Currency>) -> Option<Money> {
        self.balance_free(currency)
    }

    #[pyo3(name = "balances_free")]
    fn py_balances_free(&self) -> HashMap<Currency, Money> {
        self.balances_free()
    }

    #[pyo3(name = "balance_locked")]
    #[pyo3(signature = (currency=None))]
    fn py_balance_locked(&self, currency: Option<Currency>) -> Option<Money> {
        self.balance_locked(currency)
    }
    #[pyo3(name = "balances_locked")]
    fn py_balances_locked(&self) -> HashMap<Currency, Money> {
        self.balances_locked()
    }

    #[pyo3(name = "apply")]
    fn py_apply(&mut self, event: AccountState) {
        self.apply(event);
    }

    #[pyo3(name = "calculate_balance_locked")]
    #[pyo3(signature = (instrument, side, quantity, price, use_quote_for_inverse=None))]
    fn py_calculate_balance_locked(
        &mut self,
        instrument: PyObject,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        self.calculate_balance_locked(instrument, side, quantity, price, use_quote_for_inverse)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate_commission")]
    #[pyo3(signature = (instrument, last_qty, last_px, liquidity_side, use_quote_for_inverse=None))]
    fn py_calculate_commission(
        &self,
        instrument: PyObject,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        if liquidity_side == LiquiditySide::NoLiquiditySide {
            return Err(to_pyvalue_err("Invalid liquidity side"));
        }
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        self.calculate_commission(
            instrument,
            last_qty,
            last_px,
            liquidity_side,
            use_quote_for_inverse,
        )
        .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "calculate_pnls")]
    #[pyo3(signature = (instrument, fill, position=None))]
    fn py_calculate_pnls(
        &self,
        instrument: PyObject,
        fill: OrderFilled,
        position: Option<Position>,
        py: Python,
    ) -> PyResult<Vec<Money>> {
        let instrument = pyobject_to_instrument_any(py, instrument)?;
        self.calculate_pnls(instrument, fill, position)
            .map_err(to_pyvalue_err)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("calculate_account_state", self.calculate_account_state)?;
        let events_list: PyResult<Vec<PyObject>> =
            self.events.iter().map(|item| item.py_to_dict(py)).collect();
        dict.set_item("events", events_list.unwrap())?;
        Ok(dict.into())
    }
}
