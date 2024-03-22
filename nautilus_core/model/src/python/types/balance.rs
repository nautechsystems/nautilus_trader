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

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    identifiers::instrument_id::InstrumentId,
    types::{
        balance::{AccountBalance, MarginBalance},
        currency::Currency,
        money::Money,
    },
};

#[pymethods]
impl AccountBalance {
    #[new]
    fn py_new(total: Money, locked: Money, free: Money) -> PyResult<Self> {
        Self::new(total, locked, free).map_err(to_pyvalue_err)
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
            "{}(total={},locked={},free={})",
            stringify!(AccountBalance),
            self.total,
            self.locked,
            self.free
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}(total={},locked={},free={})",
            stringify!(AccountBalance),
            self.total,
            self.locked,
            self.free
        )
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        let dict = values.as_ref(py);
        let currency: &str = dict.get_item("currency")?.unwrap().extract()?;
        let total_str: &str = dict.get_item("total")?.unwrap().extract()?;
        let total: f64 = total_str.parse::<f64>().unwrap();
        let free_str: &str = dict.get_item("free")?.unwrap().extract()?;
        let free: f64 = free_str.parse::<f64>().unwrap();
        let locked_str: &str = dict.get_item("locked")?.unwrap().extract()?;
        let locked: f64 = locked_str.parse::<f64>().unwrap();
        let currency = Currency::from_str(currency).map_err(to_pyvalue_err)?;
        let account_balance = Self::new(
            Money::new(total, currency).map_err(to_pyvalue_err)?,
            Money::new(locked, currency).map_err(to_pyvalue_err)?,
            Money::new(free, currency).map_err(to_pyvalue_err)?,
        )
        .unwrap();
        Ok(account_balance)
    }

    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
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
    fn py_new(initial: Money, maintenance: Money, instrument: InstrumentId) -> PyResult<Self> {
        Self::new(initial, maintenance, instrument).map_err(to_pyvalue_err)
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
            "{}(initial={},maintenance={},instrument_id={})",
            stringify!(MarginBalance),
            self.initial,
            self.maintenance,
            self.instrument_id,
        )
    }

    fn __str__(&self) -> String {
        format!(
            "{}(initial={},maintenance={},instrument_id={})",
            stringify!(MarginBalance),
            self.initial,
            self.maintenance,
            self.instrument_id,
        )
    }

    #[staticmethod]
    #[pyo3(name = "from_dict")]
    pub fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        let dict = values.as_ref(py);
        let currency: &str = dict.get_item("currency")?.unwrap().extract()?;
        let initial_str: &str = dict.get_item("initial")?.unwrap().extract()?;
        let initial: f64 = initial_str.parse::<f64>().unwrap();
        let maintenance_str: &str = dict.get_item("maintenance")?.unwrap().extract()?;
        let maintenance: f64 = maintenance_str.parse::<f64>().unwrap();
        let instrument_id_str: &str = dict.get_item("instrument_id")?.unwrap().extract()?;
        let currency = Currency::from_str(currency).map_err(to_pyvalue_err)?;
        let account_balance = Self::new(
            Money::new(initial, currency).map_err(to_pyvalue_err)?,
            Money::new(maintenance, currency).map_err(to_pyvalue_err)?,
            InstrumentId::from_str(instrument_id_str).unwrap(),
        )
        .unwrap();
        Ok(account_balance)
    }

    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
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
