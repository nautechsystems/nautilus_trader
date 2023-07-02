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
    str::FromStr,
};

use nautilus_core::{correctness, serialization::Serializable, time::UnixNanos};
use pyo3::{
    exceptions::{PyKeyError, PyValueError},
    prelude::*,
    pyclass::CompareOp,
    types::PyDict,
};
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
pub struct QuoteTick {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The top of book bid price.
    pub bid: Price,
    /// The top of book ask price.
    pub ask: Price,
    /// The top of book bid size.
    pub bid_size: Quantity,
    /// The top of book ask size.
    pub ask_size: Quantity,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    /// The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl QuoteTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        bid: Price,
        ask: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        correctness::u8_equal(
            bid.precision,
            ask.precision,
            "bid.precision",
            "ask.precision",
        );
        correctness::u8_equal(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        );
        Self {
            instrument_id,
            bid,
            ask,
            bid_size,
            ask_size,
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

    #[must_use]
    pub fn extract_price(&self, price_type: PriceType) -> Price {
        match price_type {
            PriceType::Bid => self.bid,
            PriceType::Ask => self.ask,
            PriceType::Mid => Price::from_raw(
                (self.bid.raw + self.ask.raw) / 2,
                cmp::min(self.bid.precision + 1, FIXED_PRECISION),
            ),
            _ => panic!("Cannot extract with price type {price_type}"),
        }
    }
}

impl Serializable for QuoteTick {}

impl Display for QuoteTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id, self.bid, self.ask, self.bid_size, self.ask_size, self.ts_event,
        )
    }
}

#[pymethods]
impl QuoteTick {
    #[new]
    fn new_py(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
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
    fn bid_price(&self) -> Price {
        self.bid
    }

    #[getter]
    fn ask_price(&self) -> Price {
        self.ask
    }

    #[getter]
    fn bid_size(&self) -> Quantity {
        self.bid_size
    }

    #[getter]
    fn ask_size(&self) -> Quantity {
        self.ask_size
    }

    #[getter]
    fn ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    fn ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    fn extract_price_py(&self, price_type: PriceType) -> PyResult<Price> {
        Ok(self.extract_price(price_type))
    }

    /// Return a dictionary representation of the object.
    pub fn as_dict(&self) -> Py<PyDict> {
        Python::with_gil(|py| {
            let dict = PyDict::new(py);

            dict.set_item("type", stringify!(QuoteTick)).unwrap();
            dict.set_item("instrument_id", self.instrument_id.to_string())
                .unwrap();
            dict.set_item("bid", self.bid.to_string()).unwrap();
            dict.set_item("ask", self.ask.to_string()).unwrap();
            dict.set_item("bid_size", self.bid_size.to_string())
                .unwrap();
            dict.set_item("ask_size", self.ask_size.to_string())
                .unwrap();
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
        let bid: String = values
            .get_item("bid")
            .ok_or(PyKeyError::new_err("'bid' not found in `values`"))?
            .extract()?;
        let ask: String = values
            .get_item("ask")
            .ok_or(PyKeyError::new_err("'ask' not found in `values`"))?
            .extract()?;
        let bid_size: String = values
            .get_item("bid_size")
            .ok_or(PyKeyError::new_err("'bid_size' not found in `values`"))?
            .extract()?;
        let ask_size: String = values
            .get_item("ask_size")
            .ok_or(PyKeyError::new_err("'ask_size' not found in `values`"))?
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
        let bid = Price::from_str(&bid).map_err(PyValueError::new_err)?;
        let ask = Price::from_str(&ask).map_err(PyValueError::new_err)?;
        let bid_size = Quantity::from_str(&bid_size).map_err(PyValueError::new_err)?;
        let ask_size = Quantity::from_str(&ask_size).map_err(PyValueError::new_err)?;

        Ok(Self::new(
            instrument_id,
            bid,
            ask,
            bid_size,
            ask_size,
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

    use nautilus_core::serialization::Serializable;
    use pyo3::Python;
    use rstest::rstest;

    use crate::{
        data::quote::QuoteTick,
        enums::PriceType,
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    fn create_stub_quote_tick() -> QuoteTick {
        QuoteTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            bid: Price::new(10000.0, 4),
            ask: Price::new(10001.0, 4),
            bid_size: Quantity::new(1.0, 8),
            ask_size: Quantity::new(1.0, 8),
            ts_event: 1,
            ts_init: 0,
        }
    }

    #[test]
    fn test_to_string() {
        let tick = create_stub_quote_tick();
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,1"
        );
    }

    #[rstest(
        input,
        expected,
        case(PriceType::Bid, 10_000_000_000_000),
        case(PriceType::Ask, 10_001_000_000_000),
        case(PriceType::Mid, 10_000_500_000_000)
    )]
    fn test_extract_price(input: PriceType, expected: i64) {
        let tick = create_stub_quote_tick();
        let result = tick.extract_price(input).raw;
        assert_eq!(result, expected);
    }

    #[test]
    fn test_to_dict_and_from_dict() {
        pyo3::prepare_freethreaded_python();

        let tick = create_stub_quote_tick();

        Python::with_gil(|py| {
            let dict = tick.as_dict();
            let parsed = QuoteTick::from_dict(dict.as_ref(py)).unwrap();
            assert_eq!(parsed, tick);
        });
    }

    #[test]
    fn test_json_serialization() {
        let tick = create_stub_quote_tick();
        let serialized = tick.as_json_bytes().unwrap();
        let deserialized = QuoteTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }

    #[test]
    fn test_msgpack_serialization() {
        let tick = create_stub_quote_tick();
        let serialized = tick.as_msgpack_bytes().unwrap();
        let deserialized = QuoteTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }
}
