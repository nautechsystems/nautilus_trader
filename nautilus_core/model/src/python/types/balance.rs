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

use nautilus_core::python::{serialization::from_dict_pyo3, to_pyvalue_err};
use pyo3::{basic::CompareOp, prelude::*, types::PyDict};

use crate::{
    identifiers::instrument_id::InstrumentId,
    types::{
        balance::{AccountBalance, MarginBalance},
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
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(AccountBalance))?;
        dict.set_item("total", self.total.to_string())?;
        dict.set_item("locked", self.locked.to_string())?;
        dict.set_item("free", self.free.to_string())?;
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
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<PyObject> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(MarginBalance))?;
        dict.set_item("initial", self.initial.to_string())?;
        dict.set_item("maintenance", self.maintenance.to_string())?;
        dict.set_item("currency", self.currency.code.to_string())?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        Ok(dict.into())
    }
}
