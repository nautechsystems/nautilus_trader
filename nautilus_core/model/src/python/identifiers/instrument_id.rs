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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::python::to_pyvalue_err;
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyString, PyTuple},
};

use crate::identifiers::{instrument_id::InstrumentId, symbol::Symbol, venue::Venue};

#[pymethods]
impl InstrumentId {
    #[new]
    fn py_new(symbol: Symbol, venue: Venue) -> PyResult<Self> {
        Ok(Self::new(symbol, venue))
    }

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (&PyString, &PyString) = state.extract(py)?;
        self.symbol = Symbol::new(tuple.0.extract()?).map_err(to_pyvalue_err)?;
        self.venue = Venue::new(tuple.1.extract()?).map_err(to_pyvalue_err)?;
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        Ok((self.symbol.to_string(), self.venue.to_string()).to_object(py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Self::from_str("NULL.NULL").unwrap()) // Safe default
    }

    fn __richcmp__(&self, other: PyObject, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        if let Ok(other) = other.extract::<Self>(py) {
            match op {
                CompareOp::Eq => self.eq(&other).into_py(py),
                CompareOp::Ne => self.ne(&other).into_py(py),
                _ => py.NotImplemented(),
            }
        } else {
            py.NotImplemented()
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{}('{}')", stringify!(InstrumentId), self)
    }

    #[getter]
    #[pyo3(name = "symbol")]
    fn py_symbol(&self) -> Symbol {
        self.symbol
    }

    #[getter]
    #[pyo3(name = "venue")]
    fn py_venue(&self) -> Venue {
        self.venue
    }

    #[getter]
    fn value(&self) -> String {
        self.to_string()
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }

    #[pyo3(name = "is_synthetic")]
    fn py_is_synthetic(&self) -> bool {
        self.is_synthetic()
    }
}

impl ToPyObject for InstrumentId {
    fn to_object(&self, py: Python) -> PyObject {
        self.into_py(py)
    }
}
