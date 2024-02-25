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

use nautilus_core::{
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    time::UnixNanos,
    uuid::UUID4,
};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};
use rust_decimal::prelude::ToPrimitive;

use crate::{
    enums::AccountType,
    events::account::state::AccountState,
    identifiers::account_id::AccountId,
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
    },
};

#[pymethods]
impl AccountState {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        account_id: AccountId,
        account_type: AccountType,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        is_reported: bool,
        event_id: UUID4,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        base_currency: Option<Currency>,
    ) -> PyResult<Self> {
        Self::new(
            account_id,
            account_type,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event,
            ts_init,
            base_currency,
        )
        .map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(account_id={},account_type={},base_currency={},balances={},margins={},is_reported={},event_id={})",
            stringify!(AccountState),
            self.account_id,
            self.account_type,
            self.base_currency.map_or_else(|| "None".to_string(), |base_currency | format!("{}", base_currency.code)),            self.balances.iter().map(|b| format!("{b}")).collect::<Vec<String>>().join(","),
            self.margins.iter().map(|m| format!("{m}")).collect::<Vec<String>>().join(","),
            self.is_reported,
            self.event_id,
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}(account_id={},account_type={},base_currency={},balances={},margins={},is_reported={},event_id={})",
            stringify!(AccountState),
            self.account_id,
            self.account_type,
            self.base_currency.map_or_else(|| "None".to_string(), |base_currency | format!("{}", base_currency.code)),            self.balances.iter().map(|b| format!("{b}")).collect::<Vec<String>>().join(","),
            self.margins.iter().map(|m| format!("{m}")).collect::<Vec<String>>().join(","),
            self.is_reported,
            self.event_id,
        )
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(AccountState))?;
        dict.set_item("account_id", self.account_id.to_string())?;
        dict.set_item("account_type", self.account_type.to_string())?;
        // iterate over balances and margins and run to_dict on each item and collect them
        let balances_dict: PyResult<Vec<_>> =
            self.balances.iter().map(|b| b.py_to_dict(py)).collect();
        let margins_dict: PyResult<Vec<_>> =
            self.margins.iter().map(|m| m.py_to_dict(py)).collect();
        dict.set_item("balances", balances_dict?)?;
        dict.set_item("margins", margins_dict?)?;
        dict.set_item("is_reported", self.is_reported)?;
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("ts_event", self.ts_event.to_u64())?;
        dict.set_item("ts_init", self.ts_init.to_u64())?;
        match self.base_currency {
            Some(base_currency) => {
                dict.set_item("base_currency", base_currency.code.to_string())?;
            }
            None => dict.set_item("base_currency", "None")?,
        }
        Ok(dict.into())
    }
}
