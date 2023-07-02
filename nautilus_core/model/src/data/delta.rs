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
    collections::HashMap,
    fmt::{Display, Formatter},
    hash::Hash,
    str::FromStr,
};

use nautilus_core::{serialization::Serializable, time::UnixNanos};
use pyo3::{
    exceptions::{PyKeyError, PyValueError},
    prelude::*,
    pyclass::CompareOp,
    types::PyDict,
};
use serde::{Deserialize, Serialize};

use super::order::BookOrder;
use crate::{
    enums::{BookAction, OrderSide},
    identifiers::instrument_id::InstrumentId,
    types::{price::Price, quantity::Quantity},
};

/// Represents a single change/delta in an order book.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[pyclass]
pub struct OrderBookDelta {
    /// The instrument ID for the book.
    pub instrument_id: InstrumentId,
    /// The order book delta action.
    pub action: BookAction,
    /// The order to apply.
    pub order: BookOrder,
    /// A combination of packet end with matching engine status.
    pub flags: u8,
    /// The message sequence number assigned at the venue.
    pub sequence: u64,
    /// The UNIX timestamp (nanoseconds) when the data event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl OrderBookDelta {
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        }
    }

    pub fn get_metadata(
        instrument_id: &InstrumentId,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        let mut metadata = HashMap::new();
        metadata.insert("instrument_id".to_string(), instrument_id.to_string());
        metadata.insert("price_precision".to_string(), price_precision.to_string());
        metadata.insert("size_precision".to_string(), size_precision.to_string());
        metadata
    }
}

impl Serializable for OrderBookDelta {}

impl Display for OrderBookDelta {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{},{}",
            self.instrument_id,
            self.action,
            self.order,
            self.flags,
            self.sequence,
            self.ts_event,
            self.ts_init
        )
    }
}

#[pymethods]
impl OrderBookDelta {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder,
        flags: u8,
        sequence: u64,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
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
    fn action(&self) -> BookAction {
        self.action
    }

    #[getter]
    fn order(&self) -> BookOrder {
        self.order
    }

    #[getter]
    fn flags(&self) -> u8 {
        self.flags
    }

    #[getter]
    fn sequence(&self) -> u64 {
        self.sequence
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

            dict.set_item("type", stringify!(OrderBookDelta)).unwrap();
            dict.set_item("instrument_id", self.instrument_id.to_string())
                .unwrap();
            dict.set_item("action", self.action.to_string()).unwrap();
            dict.set_item("side", self.order.side.to_string()).unwrap();
            dict.set_item("price", self.order.price.to_string())
                .unwrap();
            dict.set_item("size", self.order.size.to_string()).unwrap();
            dict.set_item("order_id", self.order.order_id).unwrap();
            dict.set_item("flags", self.flags).unwrap();
            dict.set_item("sequence", self.sequence).unwrap();
            dict.set_item("ts_event", self.ts_event).unwrap();
            dict.set_item("ts_init", self.ts_init).unwrap();

            dict.into_py(py)
        })
    }

    /// Return a new object from the given dictionary representation.
    #[staticmethod]
    pub fn from_dict(values: &PyDict) -> PyResult<Self> {
        let instrument_id: String = values
            .get_item("instrument_id")
            .ok_or(PyKeyError::new_err("'instrument_id' not found in `values`"))?
            .extract()?;
        let action: String = values
            .get_item("action")
            .ok_or(PyKeyError::new_err("'action' not found in `values`"))?
            .extract()?;
        let side: String = values
            .get_item("side")
            .ok_or(PyKeyError::new_err("'side' not found in `values`"))?
            .extract()?;
        let price: String = values
            .get_item("price")
            .ok_or(PyKeyError::new_err("'price' not found in `values`"))?
            .extract()?;
        let size: String = values
            .get_item("size")
            .ok_or(PyKeyError::new_err("'size' not found in `values`"))?
            .extract()?;
        let order_id: u64 = values
            .get_item("order_id")
            .ok_or(PyKeyError::new_err("'order_id' not found in `values`"))?
            .extract()?;
        let flags: u8 = values
            .get_item("flags")
            .ok_or(PyKeyError::new_err("'flags' not found in `values`"))?
            .extract()?;
        let sequence: u64 = values
            .get_item("sequence")
            .ok_or(PyKeyError::new_err("'sequence' not found in `values`"))?
            .extract()?;
        let ts_event: UnixNanos = values
            .get_item("ts_event")
            .ok_or(PyKeyError::new_err("'ts_event' not found in `values`"))?
            .extract()?;
        let ts_init: UnixNanos = values
            .get_item("ts_init")
            .ok_or(PyKeyError::new_err("'ts_init' not found in `values`"))?
            .extract()?;

        let instrument_id = InstrumentId::from_str(&instrument_id)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        let action =
            BookAction::from_str(&action).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let side = OrderSide::from_str(&side).map_err(|e| PyValueError::new_err(e.to_string()))?;
        let price = Price::from_str(&price).map_err(PyValueError::new_err)?;
        let size = Quantity::from_str(&size).map_err(PyValueError::new_err)?;
        let order = BookOrder::new(side, price, size, order_id);

        Ok(Self::new(
            instrument_id,
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        ))
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;
    use crate::{
        enums::OrderSide,
        types::{price::Price, quantity::Quantity},
    };

    fn create_stub_delta() -> OrderBookDelta {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);
        OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order,
            flags,
            sequence,
            ts_event,
            ts_init,
        )
    }

    #[test]
    fn test_new() {
        let instrument_id = InstrumentId::from_str("AAPL.NASDAQ").unwrap();
        let action = BookAction::Add;
        let price = Price::from("100.00");
        let size = Quantity::from("10");
        let side = OrderSide::Buy;
        let order_id = 123456;
        let flags = 0;
        let sequence = 1;
        let ts_event = 1;
        let ts_init = 2;

        let order = BookOrder::new(side, price.clone(), size.clone(), order_id);

        let delta = OrderBookDelta::new(
            instrument_id.clone(),
            action,
            order.clone(),
            flags,
            sequence,
            ts_event,
            ts_init,
        );

        assert_eq!(delta.instrument_id, instrument_id);
        assert_eq!(delta.action, action);
        assert_eq!(delta.order.price, price);
        assert_eq!(delta.order.size, size);
        assert_eq!(delta.order.side, side);
        assert_eq!(delta.order.order_id, order_id);
        assert_eq!(delta.flags, flags);
        assert_eq!(delta.sequence, sequence);
        assert_eq!(delta.ts_event, ts_event);
        assert_eq!(delta.ts_init, ts_init);
    }

    #[test]
    fn test_display() {
        let delta = create_stub_delta();
        assert_eq!(
            format!("{}", delta),
            "AAPL.NASDAQ,ADD,100.00,10,BUY,123456,0,1,1,2".to_string()
        );
    }

    #[test]
    fn test_to_dict_and_from_dict() {
        pyo3::prepare_freethreaded_python();

        let delta = create_stub_delta();

        Python::with_gil(|py| {
            let dict = delta.as_dict();
            let parsed = OrderBookDelta::from_dict(dict.as_ref(py)).unwrap();
            assert_eq!(parsed, delta);
        });
    }

    #[test]
    fn test_json_serialization() {
        let delta = create_stub_delta();
        let serialized = delta.as_json_bytes().unwrap();
        let deserialized = OrderBookDelta::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, delta);
    }

    #[test]
    fn test_msgpack_serialization() {
        let delta = create_stub_delta();
        let serialized = delta.as_msgpack_bytes().unwrap();
        let deserialized = OrderBookDelta::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, delta);
    }
}
