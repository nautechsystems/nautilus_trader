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
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
};

use nautilus_core::{
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::{
        Serializable,
        msgpack::{FromMsgPack, ToMsgPack},
    },
};
use pyo3::{IntoPyObjectExt, prelude::*, pyclass::CompareOp, types::PyDict};

use crate::{
    data::{depth::OrderBookDepth, order::BookOrder},
    enums::OrderSide,
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{Price, Quantity},
};

const DEPTH10_LEN: usize = 10;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OrderBookDepth {
    /// Represents one aggregated order book snapshot with any number of levels per side.
    ///
    /// The plural name denotes the many levels in one snapshot. In contrast, `super.OrderBookDeltas`
    /// is a container of multiple update events. Up to ten levels per side remain inline; deeper venue
    /// snapshots spill transparently without changing the data type.
    ///
    /// Per-level `BookOrder.order_id` values are retained when supplied by the venue.
    #[expect(clippy::too_many_arguments)]
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        bids: Vec<BookOrder>,
        asks: Vec<BookOrder>,
        bid_counts: Vec<u32>,
        ask_counts: Vec<u32>,
        flags: u8,
        sequence: u64,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        if bids.len() != bid_counts.len() {
            return Err(to_pyvalue_err(format!(
                "bid order and count lengths must match: {} orders and {} counts",
                bids.len(),
                bid_counts.len(),
            )));
        }

        if asks.len() != ask_counts.len() {
            return Err(to_pyvalue_err(format!(
                "ask order and count lengths must match: {} orders and {} counts",
                asks.len(),
                ask_counts.len(),
            )));
        }

        Self::new_checked(
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
        .map_err(to_pyvalue_err)
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
    fn py_bids(&self) -> Vec<BookOrder> {
        self.bids.to_vec()
    }

    #[getter]
    #[pyo3(name = "asks")]
    fn py_asks(&self) -> Vec<BookOrder> {
        self.asks.to_vec()
    }

    #[getter]
    #[pyo3(name = "bid_counts")]
    fn py_bid_counts(&self) -> Vec<u32> {
        self.bid_counts.to_vec()
    }

    #[getter]
    #[pyo3(name = "ask_counts")]
    fn py_ask_counts(&self) -> Vec<u32> {
        self.ask_counts.to_vec()
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(OrderBookDepth))
    }

    /// Returns the metadata for the type, for use with serialization formats.
    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        Self::get_metadata(instrument_id, price_precision, size_precision)
    }

    /// Returns the field map for the type, for use with Arrow schemas.
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

        for (i, order) in bids.iter_mut().take(DEPTH10_LEN).enumerate() {
            *order = BookOrder::new(
                OrderSide::Buy,
                Price::new(price, 2),
                Quantity::new(quantity, 0),
                (i + 1) as u64,
            );

            price -= 1.0;
            quantity += 100.0;
        }

        // Create asks
        let mut price = 100.00;
        let mut quantity = 100.0;

        for (i, order) in asks.iter_mut().take(DEPTH10_LEN).enumerate() {
            *order = BookOrder::new(
                OrderSide::Sell,
                Price::new(price, 2),
                Quantity::new(quantity, 0),
                (i + 11) as u64,
            );

            price += 1.0;
            quantity += 100.0;
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

    /// Return a dictionary representation of the object.
    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "to_json_bytes")]
    fn py_to_json_bytes(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.to_json_bytes()
            .map_err(to_pyvalue_err)?
            .into_py_any(py)
    }

    /// Return `MsgPack` encoded bytes representation of the object.
    #[pyo3(name = "to_msgpack_bytes")]
    fn py_to_msgpack_bytes(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        self.to_msgpack_bytes()
            .map_err(to_pyvalue_err)?
            .into_py_any(py)
    }
}

#[pymethods]
impl OrderBookDepth {
    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: &[u8]) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_msgpack")]
    fn py_from_msgpack(data: &[u8]) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(to_pyvalue_err)
    }
}
