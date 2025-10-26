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

use nautilus_core::python::{parsing::get_required_string, to_pyvalue_err};
use pyo3::{prelude::*, types::PyDict};

use crate::{
    identifiers::InstrumentId,
    types::{AccountBalance, Currency, MarginBalance, Money},
};

#[pymethods]
impl AccountBalance {
    #[new]
    fn py_new(total: Money, locked: Money, free: Money) -> PyResult<Self> {
        Self::new_checked(total, locked, free).map_err(to_pyvalue_err)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    /// Constructs an [`AccountBalance`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if parsing or conversion fails.
    ///
    /// # Panics
    ///
    /// Panics if parsing numeric values (`unwrap()`) fails due to invalid format.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let total_str = get_required_string(values, "total")?;
        let total: f64 = total_str.parse::<f64>().unwrap();
        let free_str = get_required_string(values, "free")?;
        let free: f64 = free_str.parse::<f64>().unwrap();
        let locked_str = get_required_string(values, "locked")?;
        let locked: f64 = locked_str.parse::<f64>().unwrap();
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        Self::new_checked(
            Money::new(total, currency),
            Money::new(locked, currency),
            Money::new(free, currency),
        )
        .map_err(to_pyvalue_err)
    }

    /// Converts this [`AccountBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(AccountBalance))?;
        dict.set_item(
            "total",
            format!(
                "{:.*}",
                self.total.currency.precision as usize,
                self.total.as_f64()
            ),
        )?;
        dict.set_item(
            "locked",
            format!(
                "{:.*}",
                self.locked.currency.precision as usize,
                self.locked.as_f64()
            ),
        )?;
        dict.set_item(
            "free",
            format!(
                "{:.*}",
                self.free.currency.precision as usize,
                self.free.as_f64()
            ),
        )?;
        dict.set_item("currency", self.currency.code.to_string())?;
        Ok(dict.into())
    }
}

#[pymethods]
impl MarginBalance {
    #[new]
    fn py_new(initial: Money, maintenance: Money, instrument: InstrumentId) -> Self {
        Self::new(initial, maintenance, instrument)
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    /// Constructs a [`MarginBalance`] from a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if parsing or conversion fails.
    ///
    /// # Panics
    ///
    /// Panics if parsing numeric values (`unwrap()`) fails due to invalid format.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(values: &Bound<'_, PyDict>) -> PyResult<Self> {
        let currency_str = get_required_string(values, "currency")?;
        let initial_str = get_required_string(values, "initial")?;
        let initial: f64 = initial_str.parse::<f64>().unwrap();
        let maintenance_str = get_required_string(values, "maintenance")?;
        let maintenance: f64 = maintenance_str.parse::<f64>().unwrap();
        let instrument_id_str = get_required_string(values, "instrument_id")?;
        let currency = Currency::from_str(currency_str.as_str()).map_err(to_pyvalue_err)?;
        let account_balance = Self::new(
            Money::new(initial, currency),
            Money::new(maintenance, currency),
            InstrumentId::from(instrument_id_str.as_str()),
        );
        Ok(account_balance)
    }

    /// Converts this [`MarginBalance`] into a Python dict.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if serialization fails.
    ///
    /// # Panics
    ///
    /// Panics if parsing numeric values (`unwrap()`) fails due to invalid format.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(MarginBalance))?;
        dict.set_item(
            "initial",
            format!(
                "{:.*}",
                self.initial.currency.precision as usize,
                self.initial.as_f64()
            ),
        )?;
        dict.set_item(
            "maintenance",
            format!(
                "{:.*}",
                self.maintenance.currency.precision as usize,
                self.maintenance.as_f64()
            ),
        )?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        Ok(dict.into())
    }
}
