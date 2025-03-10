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

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use pyo3::{IntoPyObjectExt, basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    accounts::MarginAccount,
    events::AccountState,
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
    python::instruments::pyobject_to_instrument_any,
    types::{Money, Price, Quantity},
};

#[pymethods]
impl MarginAccount {
    #[new]
    fn py_new(event: AccountState, calculate_account_state: bool) -> Self {
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

    #[getter]
    fn default_leverage(&self) -> f64 {
        self.default_leverage
    }

    #[getter]
    #[pyo3(name = "calculate_account_state")]
    fn py_calculate_account_state(&self) -> bool {
        self.calculate_account_state
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(id={}, type={}, base={})",
            stringify!(MarginAccount),
            self.id,
            self.account_type,
            self.base_currency.map_or_else(
                || "None".to_string(),
                |base_currency| format!("{}", base_currency.code)
            ),
        )
    }

    #[pyo3(name = "set_default_leverage")]
    fn py_set_default_leverage(&mut self, default_leverage: f64) -> PyResult<()> {
        self.set_default_leverage(default_leverage);
        Ok(())
    }

    #[pyo3(name = "leverages")]
    fn py_leverages(&self, py: Python) -> PyResult<PyObject> {
        let leverages = PyDict::new(py);
        for (key, &value) in &self.leverages {
            leverages
                .set_item(key.into_py_any_unwrap(py), value)
                .unwrap();
        }
        leverages.into_py_any(py)
    }

    #[pyo3(name = "leverage")]
    fn py_leverage(&self, instrument_id: &InstrumentId) -> PyResult<f64> {
        Ok(self.get_leverage(instrument_id))
    }

    #[pyo3(name = "set_leverage")]
    fn py_set_leverage(&mut self, instrument_id: InstrumentId, leverage: f64) -> PyResult<()> {
        self.set_leverage(instrument_id, leverage);
        Ok(())
    }

    #[pyo3(name = "is_unleveraged")]
    fn py_is_unleveraged(&self, instrument_id: InstrumentId) -> PyResult<bool> {
        Ok(self.is_unleveraged(instrument_id))
    }

    #[pyo3(name = "initial_margins")]
    fn py_initial_margins(&self, py: Python) -> PyResult<PyObject> {
        let initial_margins = PyDict::new(py);
        for (key, &value) in &self.initial_margins() {
            initial_margins
                .set_item(key.into_py_any_unwrap(py), value.into_py_any_unwrap(py))
                .unwrap();
        }
        initial_margins.into_py_any(py)
    }

    #[pyo3(name = "maintenance_margins")]
    fn py_maintenance_margins(&self, py: Python) -> PyResult<PyObject> {
        let maintenance_margins = PyDict::new(py);
        for (key, &value) in &self.maintenance_margins() {
            maintenance_margins
                .set_item(key.into_py_any_unwrap(py), value.into_py_any_unwrap(py))
                .unwrap();
        }
        maintenance_margins.into_py_any(py)
    }

    #[pyo3(name = "update_initial_margin")]
    fn py_update_initial_margin(
        &mut self,
        instrument_id: InstrumentId,
        initial_margin: Money,
    ) -> PyResult<()> {
        self.update_initial_margin(instrument_id, initial_margin);
        Ok(())
    }

    #[pyo3(name = "initial_margin")]
    fn py_initial_margin(&self, instrument_id: InstrumentId) -> PyResult<Money> {
        Ok(self.initial_margin(instrument_id))
    }

    #[pyo3(name = "update_maintenance_margin")]
    fn py_update_maintenance_margin(
        &mut self,
        instrument_id: InstrumentId,
        maintenance_margin: Money,
    ) -> PyResult<()> {
        self.update_maintenance_margin(instrument_id, maintenance_margin);
        Ok(())
    }

    #[pyo3(name = "maintenance_margin")]
    fn py_maintenance_margin(&self, instrument_id: InstrumentId) -> PyResult<Money> {
        Ok(self.maintenance_margin(instrument_id))
    }

    #[pyo3(name = "calculate_initial_margin")]
    #[pyo3(signature = (instrument, quantity, price, use_quote_for_inverse=None))]
    pub fn py_calculate_initial_margin(
        &mut self,
        instrument: PyObject,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument_type = pyobject_to_instrument_any(py, instrument)?;
        match instrument_type {
            InstrumentAny::CryptoFuture(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::CryptoPerpetual(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::CurrencyPair(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::Equity(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::FuturesContract(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::OptionContract(inst) => {
                Ok(self.calculate_initial_margin(inst, quantity, price, use_quote_for_inverse))
            }
            _ => Err(to_pyvalue_err("Unsupported instrument type")),
        }
    }

    #[pyo3(name = "calculate_maintenance_margin")]
    #[pyo3(signature = (instrument, quantity, price, use_quote_for_inverse=None))]
    pub fn py_calculate_maintenance_margin(
        &mut self,
        instrument: PyObject,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: Option<bool>,
        py: Python,
    ) -> PyResult<Money> {
        let instrument_type = pyobject_to_instrument_any(py, instrument)?;
        match instrument_type {
            InstrumentAny::CryptoFuture(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::CryptoPerpetual(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::CurrencyPair(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::Equity(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::FuturesContract(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            InstrumentAny::OptionContract(inst) => {
                Ok(self.calculate_maintenance_margin(inst, quantity, price, use_quote_for_inverse))
            }
            _ => Err(to_pyvalue_err("Unsupported instrument type")),
        }
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
