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

use std::{
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use nautilus_core::{
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::Serializable,
};
use pyo3::{basic::CompareOp, exceptions::PyValueError, prelude::*, types::PyDict};

use super::ERROR_MONOTONICITY;
use crate::{
    data::close::InstrumentClose, enums::InstrumentCloseType, identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL, types::Price,
};

////////////////////////////////////////
// Type methods
////////////////////////////////////////
#[pymethods]
impl InstrumentClose {
    #[new]
    #[pyo3(signature = (instrument_id, close_price, close_type, ts_event, ts_init))]
    fn py_new(
        instrument_id: InstrumentId,
        close_price: Price,
        close_type: InstrumentCloseType,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self {
            instrument_id,
            close_price,
            close_type,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        }
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
        format!("{}({})", stringify!(InstrumentStatus), self)
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
    #[pyo3(name = "close_price")]
    pub fn py_close_price(&self) -> Price {
        self.close_price
    }

    #[getter]
    #[pyo3(name = "close_type")]
    pub fn py_close_type(&self) -> InstrumentCloseType {
        self.close_type
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    pub fn py_ts_event(&self) -> u64 {
        self.ts_event.as_u64()
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    pub fn py_ts_init(&self) -> u64 {
        self.ts_init.as_u64()
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(InstrumentClose))
    }

    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
    ) -> PyResult<HashMap<String, String>> {
        Ok(Self::get_metadata(instrument_id, price_precision))
    }

    #[staticmethod]
    #[pyo3(name = "get_fields")]
    fn py_get_fields(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let py_dict = PyDict::new(py);
        for (k, v) in Self::get_fields() {
            py_dict.set_item(k, v)?;
        }

        Ok(py_dict)
    }

    /// Returns a new object from the given dictionary representation.
    #[staticmethod]
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
    }

    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: Vec<u8>) -> PyResult<Self> {
        Self::from_json_bytes(&data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_msgpack")]
    fn py_from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        Self::from_msgpack_bytes(&data).map_err(to_pyvalue_err)
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py_any_unwrap(py)
    }
}

impl InstrumentClose {
    /// Create a [`InstrumentClose`] from a PyObject reference.
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id = obj.getattr("instrument_id")?.extract::<InstrumentId>()?;
        let close_price = obj.getattr("close_price")?.extract::<Price>()?;
        let close_type = obj
            .getattr("close_type")?
            .extract::<InstrumentCloseType>()?;
        let ts_event = obj.getattr("ts_event")?.extract::<u64>()?;
        let ts_init = obj.getattr("ts_init")?.extract::<u64>()?;

        Ok(Self {
            instrument_id,
            close_price,
            close_type,
            ts_event: ts_event.into(),
            ts_init: ts_init.into(),
        })
    }
}

/// Transforms the given `data` Python objects into a vector of [`InstrumentClose`] objects.
pub fn pyobjects_to_instrument_closes(
    data: Vec<Bound<'_, PyAny>>,
) -> PyResult<Vec<InstrumentClose>> {
    let closes = data
        .into_iter()
        .map(|obj| InstrumentClose::from_pyobject(&obj))
        .collect::<PyResult<Vec<InstrumentClose>>>()?;

    // Validate monotonically increasing by timestamp initialization
    if !crate::data::is_monotonically_increasing_by_init(&closes) {
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(closes)
}
