// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
    cmp,
    collections::HashMap,
    fmt::{Display, Formatter},
};

use nautilus_core::{correctness, serialization::Serializable, time::UnixNanos};
use pyo3::{exceptions::PyValueError, prelude::*, pyclass::CompareOp, types::PyDict};
use serde::{Deserialize, Serialize};

use crate::{
    enums::PriceType,
    identifiers::instrument_id::InstrumentId,
    types::{fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[pyclass]
pub struct Ticker {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl Ticker {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            ts_event,
            ts_init,
        }
    }
}

impl Serializable for Ticker {}

#[pymethods]
impl Ticker {
    #[new]
    fn new_py(
        instrument_id: InstrumentId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(
            instrument_id,
            ts_event,
            ts_init,
        )
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
        }
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    fn __repr__(&self) -> String {
        format!("{self:?}")
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id.clone()
    }

    #[getter]
    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    /// Return a dictionary representation of the object.
    pub fn as_dict(&self) -> Py<PyDict> {
        Python::with_gil(|py| {
            let dict = PyDict::new(py);

            dict.set_item("type", stringify!(Ticker)).unwrap();
            dict.set_item("instrument_id", self.instrument_id.to_string());
            dict.set_item("ts_event", self.ts_event).unwrap();
            dict.set_item("ts_init", self.ts_init).unwrap();

            dict.into_py(py)
        })
    }

    #[staticmethod]
    fn from_json(data: Vec<u8>) -> PyResult<Self> {
        match Self::from_json_bytes(data) {
            Ok(quote) => Ok(quote),
            Err(err) => Err(PyValueError::new_err(format!(
                "Failed to deserialize JSON: {}",
                err
            ))),
        }
    }

    #[staticmethod]
    fn from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        match Self::from_msgpack_bytes(data) {
            Ok(quote) => Ok(quote),
            Err(err) => Err(PyValueError::new_err(format!(
                "Failed to deserialize MsgPack: {}",
                err
            ))),
        }
    }

    /// Return JSON encoded bytes representation of the object.
    fn as_json(&self) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        Python::with_gil(|py| self.as_json_bytes().unwrap().into_py(py))
    }

    /// Return MsgPack encoded bytes representation of the object.
    fn as_msgpack(&self) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        Python::with_gil(|py| self.as_msgpack_bytes().unwrap().into_py(py))
    }
}
