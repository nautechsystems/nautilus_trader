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

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use nautilus_core::python::{IntoPyObjectNautilusExt, serialization::to_dict_pyo3, to_pyvalue_err};
use pyo3::{IntoPyObjectExt, prelude::*, pyclass::CompareOp, types::PyList};

use crate::{
    data::{OrderBookDelta, OrderBookDeltas},
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OrderBookDeltas {
    /// Represents a grouped batch of `OrderBookDelta` updates for an `OrderBook`.
    ///
    /// This type cannot be `repr(C)` due to the `deltas` vec.
    #[new]
    fn py_new(instrument_id: InstrumentId, deltas: Vec<OrderBookDelta>) -> PyResult<Self> {
        Self::new_checked(instrument_id, deltas).map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py_any_unwrap(py),
            CompareOp::Ne => self.ne(other).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut h = DefaultHasher::new();
        self.hash(&mut h);
        h.finish() as isize
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    #[pyo3(name = "deltas")]
    fn py_deltas(&self) -> Vec<OrderBookDelta> {
        // `OrderBookDelta` is `Copy`
        self.deltas.clone()
    }

    #[getter]
    #[pyo3(name = "flags")]
    fn py_flags(&self) -> u8 {
        self.flags
    }

    /// Returns whether the batch is a snapshot.
    #[getter]
    #[pyo3(name = "is_snapshot")]
    fn py_is_snapshot(&self) -> bool {
        self.is_snapshot()
    }

    #[getter]
    #[pyo3(name = "sequence")]
    fn py_sequence(&self) -> u64 {
        self.sequence
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(OrderBookDeltas))
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let reconstruct = py.get_type::<Self>().getattr("_from_dicts")?;
        let delta_dicts: Vec<_> = self
            .deltas
            .iter()
            .map(|d| to_dict_pyo3(py, d))
            .collect::<PyResult<_>>()?;
        let py_list = PyList::new(py, delta_dicts)?;
        (reconstruct, (self.instrument_id, py_list)).into_py_any(py)
    }

    #[staticmethod]
    fn _from_dicts(
        instrument_id: InstrumentId,
        delta_dicts: Vec<pyo3::Py<pyo3::types::PyDict>>,
    ) -> PyResult<Self> {
        use nautilus_core::python::serialization::from_dict_pyo3;
        let deltas: Vec<OrderBookDelta> = pyo3::Python::attach(|py| {
            delta_dicts
                .into_iter()
                .map(|d| from_dict_pyo3(py, d))
                .collect::<PyResult<_>>()
        })?;
        Self::new_checked(instrument_id, deltas).map_err(to_pyvalue_err)
    }
}
