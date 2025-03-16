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

use std::str::FromStr;

use nautilus_core::{
    UUID4,
    python::{IntoPyObjectNautilusExt, to_pyvalue_err},
};
use pyo3::{
    basic::CompareOp,
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{
    enums::AccountType,
    events::AccountState,
    identifiers::AccountId,
    types::{AccountBalance, Currency, MarginBalance},
};

#[pymethods]
impl AccountState {
    #[allow(clippy::too_many_arguments)]
    #[new]
    #[pyo3(signature = (account_id, account_type, balances, margins, is_reported, event_id, ts_event, ts_init, base_currency=None))]
    fn py_new(
        account_id: AccountId,
        account_type: AccountType,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        is_reported: bool,
        event_id: UUID4,
        ts_event: u64,
        ts_init: u64,
        base_currency: Option<Currency>,
    ) -> Self {
        Self::new(
            account_id,
            account_type,
            balances,
            margins,
            is_reported,
            event_id,
            ts_event.into(),
            ts_init.into(),
            base_currency,
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

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let dict = values.as_ref();
        let account_id: String = dict.get_item("account_id")?.extract()?;
        let account_type: String = dict.get_item("account_type")?.extract()?;
        let base_currency: String = dict.get_item("base_currency")?.extract()?;
        let balances_list: Bound<'_, PyList> = dict.get_item("balances")?.extract()?;
        let balances: Vec<AccountBalance> = balances_list
            .iter()
            .map(|b| {
                let balance_dict = b.extract::<Bound<'_, PyDict>>()?;
                AccountBalance::py_from_dict(&balance_dict)
            })
            .collect::<PyResult<Vec<AccountBalance>>>()?;
        let margins_list: Bound<'_, PyList> = dict.get_item("margins")?.extract()?;
        let margins: Vec<MarginBalance> = margins_list
            .iter()
            .map(|m| {
                let margin_dict = m.extract::<Bound<'_, PyDict>>()?;
                MarginBalance::py_from_dict(&margin_dict)
            })
            .collect::<PyResult<Vec<MarginBalance>>>()?;
        let reported: bool = dict.get_item("reported")?.extract()?;
        let event_id: String = dict.get_item("event_id")?.extract()?;
        let ts_event: u64 = dict.get_item("ts_event")?.extract()?;
        let ts_init: u64 = dict.get_item("ts_init")?.extract()?;
        let account = Self::new(
            AccountId::from(account_id.as_str()),
            AccountType::from_str(account_type.as_str()).unwrap(),
            balances,
            margins,
            reported,
            UUID4::from_str(event_id.as_str()).unwrap(),
            ts_event.into(),
            ts_init.into(),
            Some(Currency::from_str(base_currency.as_str()).map_err(to_pyvalue_err)?),
        );
        Ok(account)
    }

    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
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
        dict.set_item("reported", self.is_reported)?;
        dict.set_item("event_id", self.event_id.to_string())?;
        dict.set_item("info", PyDict::new(py))?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        match self.base_currency {
            Some(base_currency) => {
                dict.set_item("base_currency", base_currency.code.to_string())?;
            }
            None => dict.set_item("base_currency", "None")?,
        }
        Ok(dict.into())
    }
}
