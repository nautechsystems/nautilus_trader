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
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};

use super::data_to_pycapsule;
use crate::{
    data::{
        Data,
        depth::{DEPTH10_LEN, OrderBookDepth10},
        order::BookOrder,
    },
    enums::OrderSide,
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{Price, Quantity},
};

#[pymethods]
impl OrderBookDepth10 {
    #[allow(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        bids: [BookOrder; DEPTH10_LEN],
        asks: [BookOrder; DEPTH10_LEN],
        bid_counts: [u32; DEPTH10_LEN],
        ask_counts: [u32; DEPTH10_LEN],
        flags: u8,
        sequence: u64,
        ts_event: u64,
        ts_init: u64,
    ) -> Self {
        Self::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event.into(),
            ts_init.into(),
        )
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
    #[pyo3(name = "bids")]
    fn py_bids(&self) -> [BookOrder; DEPTH10_LEN] {
        self.bids
    }

    #[getter]
    #[pyo3(name = "asks")]
    fn py_asks(&self) -> [BookOrder; DEPTH10_LEN] {
        self.asks
    }

    #[getter]
    #[pyo3(name = "bid_counts")]
    fn py_bid_counts(&self) -> [u32; DEPTH10_LEN] {
        self.bid_counts
    }

    #[getter]
    #[pyo3(name = "ask_counts")]
    fn py_ask_counts(&self) -> [u32; DEPTH10_LEN] {
        self.ask_counts
    }

    #[getter]
    #[pyo3(name = "flags")]
    fn py_flags(&self) -> u8 {
        self.flags
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(OrderBookDepth10))
    }

    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> PyResult<HashMap<String, String>> {
        Ok(Self::get_metadata(
            instrument_id,
            price_precision,
            size_precision,
        ))
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

    // TODO: Expose this properly from a test stub provider
    #[staticmethod]
    #[pyo3(name = "get_stub")]
    fn py_get_stub() -> Self {
        let instrument_id = InstrumentId::from("AAPL.XNAS");
        let flags = 0;
        let sequence = 0;
        let ts_event = 1;
        let ts_init = 2;

        let mut bids: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];
        let mut asks: [BookOrder; DEPTH10_LEN] = [BookOrder::default(); DEPTH10_LEN];

        // Create bids
        let mut price = 99.00;
        let mut quantity = 100.0;
        let mut order_id = 1;

        for order in bids.iter_mut().take(DEPTH10_LEN) {
            *order = BookOrder::new(
                OrderSide::Buy,
                Price::new(price, 2),
                Quantity::new(quantity, 0),
                order_id,
            );

            price -= 1.0;
            quantity += 100.0;
            order_id += 1;
        }

        // Create asks
        let mut price = 100.00;
        let mut quantity = 100.0;
        let mut order_id = 11;

        for order in asks.iter_mut().take(DEPTH10_LEN) {
            *order = BookOrder::new(
                OrderSide::Sell,
                Price::new(price, 2),
                Quantity::new(quantity, 0),
                order_id,
            );

            price += 1.0;
            quantity += 100.0;
            order_id += 1;
        }

        let bid_counts: [u32; 10] = [1; 10];
        let ask_counts: [u32; 10] = [1; 10];

        Self::new(
            instrument_id,
            bids,
            asks,
            bid_counts,
            ask_counts,
            flags,
            sequence,
            ts_event.into(),
            ts_init.into(),
        )
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

    /// Creates a `PyCapsule` containing a raw pointer to a `Data::Depth10` object.
    ///
    /// This function takes the current object (assumed to be of a type that can be represented as
    /// `Data::Depth10`), and encapsulates a raw pointer to it within a `PyCapsule`.
    ///
    /// # Safety
    ///
    /// This function is safe as long as the following conditions are met:
    /// - The `Data::Depth10` object pointed to by the capsule must remain valid for the lifetime of the capsule.
    /// - The consumer of the capsule must ensure proper handling to avoid dereferencing a dangling pointer.
    ///
    /// # Panics
    ///
    /// The function will panic if the `PyCapsule` creation fails, which can occur if the
    /// `Data::Depth10` object cannot be converted into a raw pointer.
    #[pyo3(name = "as_pycapsule")]
    fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
        data_to_pycapsule(py, Data::from(*self))
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
