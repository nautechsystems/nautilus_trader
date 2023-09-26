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
    collections::{hash_map::DefaultHasher, HashMap},
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use anyhow::Result;
use indexmap::IndexMap;
use nautilus_core::{
    correctness::check_u8_equal, python::to_pyvalue_err, serialization::Serializable,
    time::UnixNanos,
};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};
use serde::{Deserialize, Serialize};

use crate::{
    enums::PriceType,
    identifiers::instrument_id::InstrumentId,
    types::{fixed::FIXED_PRECISION, price::Price, quantity::Quantity},
};

/// Represents a single quote tick in a financial market.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[pyclass]
pub struct QuoteTick {
    /// The quotes instrument ID.
    pub instrument_id: InstrumentId,
    /// The top of book bid price.
    pub bid_price: Price,
    /// The top of book ask price.
    pub ask_price: Price,
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
    pub fn new(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Result<Self> {
        check_u8_equal(
            bid_price.precision,
            ask_price.precision,
            "bid_price.precision",
            "ask_price.precision",
        )?;
        check_u8_equal(
            bid_size.precision,
            ask_size.precision,
            "bid_size.precision",
            "ask_size.precision",
        )?;
        Ok(Self {
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        })
    }

    /// Returns the metadata for the type, for use with serialization formats.
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

    /// Returns the field map for the type, for use with arrow schemas.
    pub fn get_fields() -> IndexMap<String, String> {
        let mut metadata = IndexMap::new();
        metadata.insert("bid_price".to_string(), "Int64".to_string());
        metadata.insert("ask_price".to_string(), "Int64".to_string());
        metadata.insert("bid_size".to_string(), "UInt64".to_string());
        metadata.insert("ask_size".to_string(), "UInt64".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }

    /// Create a new [`QuoteTick`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &PyAny) -> PyResult<Self> {
        let instrument_id_obj: &PyAny = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id = InstrumentId::from_str(instrument_id_str)
            .map_err(to_pyvalue_err)
            .unwrap();

        let bid_price_py: &PyAny = obj.getattr("bid_price")?;
        let bid_price_raw: i64 = bid_price_py.getattr("raw")?.extract()?;
        let bid_price_prec: u8 = bid_price_py.getattr("precision")?.extract()?;
        let bid_price = Price::from_raw(bid_price_raw, bid_price_prec);

        let ask_price_py: &PyAny = obj.getattr("ask_price")?;
        let ask_price_raw: i64 = ask_price_py.getattr("raw")?.extract()?;
        let ask_price_prec: u8 = ask_price_py.getattr("precision")?.extract()?;
        let ask_price = Price::from_raw(ask_price_raw, ask_price_prec);

        let bid_size_py: &PyAny = obj.getattr("bid_size")?;
        let bid_size_raw: u64 = bid_size_py.getattr("raw")?.extract()?;
        let bid_size_prec: u8 = bid_size_py.getattr("precision")?.extract()?;
        let bid_size = Quantity::from_raw(bid_size_raw, bid_size_prec);

        let ask_size_py: &PyAny = obj.getattr("ask_size")?;
        let ask_size_raw: u64 = ask_size_py.getattr("raw")?.extract()?;
        let ask_size_prec: u8 = ask_size_py.getattr("precision")?.extract()?;
        let ask_size = Quantity::from_raw(ask_size_raw, ask_size_prec);

        let ts_event: UnixNanos = obj.getattr("ts_event")?.extract()?;
        let ts_init: UnixNanos = obj.getattr("ts_init")?.extract()?;

        Self::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        )
        .map_err(to_pyvalue_err)
    }

    #[must_use]
    pub fn extract_price(&self, price_type: PriceType) -> Price {
        match price_type {
            PriceType::Bid => self.bid_price,
            PriceType::Ask => self.ask_price,
            PriceType::Mid => Price::from_raw(
                (self.bid_price.raw + self.ask_price.raw) / 2,
                cmp::min(self.bid_price.precision + 1, FIXED_PRECISION),
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
            self.instrument_id,
            self.bid_price,
            self.ask_price,
            self.bid_size,
            self.ask_size,
            self.ts_event,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl QuoteTick {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> PyResult<Self> {
        Self::new(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event,
            ts_init,
        )
        .map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            CompareOp::Eq => self.eq(other).into_py(py),
            CompareOp::Ne => self.ne(other).into_py(py),
            _ => py.NotImplemented(),
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
        format!("{self:?}")
    }

    #[getter]
    fn instrument_id(&self) -> InstrumentId {
        self.instrument_id
    }

    #[getter]
    fn bid_price(&self) -> Price {
        self.bid_price
    }

    #[getter]
    fn ask_price(&self) -> Price {
        self.ask_price
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
    pub fn as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        // Serialize object to JSON bytes
        let json_str = serde_json::to_string(self).map_err(to_pyvalue_err)?;
        // Parse JSON into a Python dictionary
        let py_dict: Py<PyDict> = PyModule::import(py, "json")?
            .call_method("loads", (json_str,), None)?
            .extract()?;
        Ok(py_dict)
    }

    /// Return a new object from the given dictionary representation.
    #[staticmethod]
    pub fn from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        // Extract to JSON string
        let json_str: String = PyModule::import(py, "json")?
            .call_method("dumps", (values,), None)?
            .extract()?;

        // Deserialize to object
        let instance = serde_json::from_slice(&json_str.into_bytes()).map_err(to_pyvalue_err)?;
        Ok(instance)
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
    fn py_get_fields(py: Python<'_>) -> PyResult<&PyDict> {
        let py_dict = PyDict::new(py);
        for (k, v) in Self::get_fields() {
            py_dict.set_item(k, v)?;
        }

        Ok(py_dict)
    }

    #[staticmethod]
    fn from_json(data: Vec<u8>) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    fn from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(to_pyvalue_err)
    }

    /// Return JSON encoded bytes representation of the object.
    fn as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    fn as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py(py)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Stubs
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
pub mod stubs {
    use rstest::fixture;

    use crate::{
        data::quote::QuoteTick,
        identifiers::instrument_id::InstrumentId,
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn quote_tick_ethusdt_binance() -> QuoteTick {
        QuoteTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            bid_price: Price::from("10000.0000"),
            ask_price: Price::from("10001.0000"),
            bid_size: Quantity::from("1.00000000"),
            ask_size: Quantity::from("1.00000000"),
            ts_event: 0,
            ts_init: 1,
        }
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::serialization::Serializable;
    use pyo3::{IntoPy, Python};
    use rstest::rstest;

    use super::stubs::*;
    use crate::{data::quote::QuoteTick, enums::PriceType};

    #[rstest]
    fn test_to_string(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,10001.0000,1.00000000,1.00000000,0"
        );
    }

    #[rstest]
    #[case(PriceType::Bid, 10_000_000_000_000)]
    #[case(PriceType::Ask, 10_001_000_000_000)]
    #[case(PriceType::Mid, 10_000_500_000_000)]
    fn test_extract_price(
        #[case] input: PriceType,
        #[case] expected: i64,
        quote_tick_ethusdt_binance: QuoteTick,
    ) {
        let tick = quote_tick_ethusdt_binance;
        let result = tick.extract_price(input).raw;
        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_as_dict(quote_tick_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();

        let tick = quote_tick_ethusdt_binance;

        Python::with_gil(|py| {
            let dict_string = tick.as_dict(py).unwrap().to_string();
            let expected_string = r#"{'instrument_id': 'ETHUSDT-PERP.BINANCE', 'bid_price': '10000.0000', 'ask_price': '10001.0000', 'bid_size': '1.00000000', 'ask_size': '1.00000000', 'ts_event': 0, 'ts_init': 1}"#;
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(quote_tick_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();

        let tick = quote_tick_ethusdt_binance;

        Python::with_gil(|py| {
            let dict = tick.as_dict(py).unwrap();
            let parsed = QuoteTick::from_dict(py, dict).unwrap();
            assert_eq!(parsed, tick);
        });
    }

    #[rstest]
    fn test_from_pyobject(quote_tick_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();
        let tick = quote_tick_ethusdt_binance;

        Python::with_gil(|py| {
            let tick_pyobject = tick.into_py(py);
            let parsed_tick = QuoteTick::from_pyobject(tick_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_tick, tick);
        });
    }

    #[rstest]
    fn test_json_serialization(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        let serialized = tick.as_json_bytes().unwrap();
        let deserialized = QuoteTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }

    #[rstest]
    fn test_msgpack_serialization(quote_tick_ethusdt_binance: QuoteTick) {
        let tick = quote_tick_ethusdt_binance;
        let serialized = tick.as_msgpack_bytes().unwrap();
        let deserialized = QuoteTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }
}
