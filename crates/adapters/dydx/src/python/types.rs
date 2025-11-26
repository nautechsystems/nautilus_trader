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

//! Python bindings for dYdX custom data types.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::IntoPyObjectNautilusExt;
use nautilus_model::{identifiers::InstrumentId, types::Price};
use pyo3::{prelude::*, types::PyDict};

use crate::types::DydxOraclePrice;

#[pymethods]
impl DydxOraclePrice {
    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            pyo3::pyclass::CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.hash(&mut hasher);
        hasher.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!(
            "{}(instrument_id={}, oracle_price={}, ts_event={}, ts_init={})",
            stringify!(DydxOraclePrice),
            self.instrument_id,
            self.oracle_price,
            self.ts_event,
            self.ts_init,
        )
    }

    fn __str__(&self) -> String {
        self.__repr__()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    const fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "oracle_price")]
    const fn py_oracle_price(&self) -> Price {
        self.oracle_price
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    const fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    const fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    /// # Errors
    ///
    /// Returns a `PyErr` if generating the Python dictionary fails.
    #[pyo3(name = "to_dict")]
    pub fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        let dict = PyDict::new(py);
        dict.set_item("type", stringify!(DydxOraclePrice))?;
        dict.set_item("instrument_id", self.instrument_id.to_string())?;
        dict.set_item("oracle_price", self.oracle_price.to_string())?;
        dict.set_item("ts_event", self.ts_event.as_u64())?;
        dict.set_item("ts_init", self.ts_init.as_u64())?;
        Ok(dict.into())
    }
}
