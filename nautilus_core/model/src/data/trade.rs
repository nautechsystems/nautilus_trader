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
    collections::{hash_map::DefaultHasher, HashMap},
    fmt::{Display, Formatter},
    hash::{Hash, Hasher},
    str::FromStr,
};

use indexmap::IndexMap;
use nautilus_core::{python::to_pyvalue_err, serialization::Serializable, time::UnixNanos};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};
use serde::{Deserialize, Serialize};

use crate::{
    enums::{AggressorSide, FromU8},
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.model.data")]
pub struct TradeTick {
    /// The trade instrument ID.
    pub instrument_id: InstrumentId,
    /// The traded price.
    pub price: Price,
    /// The traded size.
    pub size: Quantity,
    /// The trade aggressor side.
    pub aggressor_side: AggressorSide,
    /// The trade match ID (assigned by the venue).
    pub trade_id: TradeId,
    /// The UNIX timestamp (nanoseconds) when the tick event occurred.
    pub ts_event: UnixNanos,
    ///  The UNIX timestamp (nanoseconds) when the data object was initialized.
    pub ts_init: UnixNanos,
}

impl TradeTick {
    #[must_use]
    pub fn new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self {
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        }
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
        metadata.insert("price".to_string(), "Int64".to_string());
        metadata.insert("size".to_string(), "UInt64".to_string());
        metadata.insert("aggressor_side".to_string(), "UInt8".to_string());
        metadata.insert("trade_id".to_string(), "Utf8".to_string());
        metadata.insert("ts_event".to_string(), "UInt64".to_string());
        metadata.insert("ts_init".to_string(), "UInt64".to_string());
        metadata
    }

    /// Create a new [`TradeTick`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &PyAny) -> PyResult<Self> {
        let instrument_id_obj: &PyAny = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id = InstrumentId::from_str(instrument_id_str).map_err(to_pyvalue_err)?;

        let price_py: &PyAny = obj.getattr("price")?;
        let price_raw: i64 = price_py.getattr("raw")?.extract()?;
        let price_prec: u8 = price_py.getattr("precision")?.extract()?;
        let price = Price::from_raw(price_raw, price_prec).map_err(to_pyvalue_err)?;

        let size_py: &PyAny = obj.getattr("size")?;
        let size_raw: u64 = size_py.getattr("raw")?.extract()?;
        let size_prec: u8 = size_py.getattr("precision")?.extract()?;
        let size = Quantity::from_raw(size_raw, size_prec).map_err(to_pyvalue_err)?;

        let aggressor_side_obj: &PyAny = obj.getattr("aggressor_side")?.extract()?;
        let aggressor_side_u8 = aggressor_side_obj.getattr("value")?.extract()?;
        let aggressor_side = AggressorSide::from_u8(aggressor_side_u8).unwrap();

        let trade_id_obj: &PyAny = obj.getattr("trade_id")?.extract()?;
        let trade_id_str = trade_id_obj.getattr("value")?.extract()?;
        let trade_id = TradeId::from_str(trade_id_str).map_err(to_pyvalue_err)?;

        let ts_event: UnixNanos = obj.getattr("ts_event")?.extract()?;
        let ts_init: UnixNanos = obj.getattr("ts_init")?.extract()?;

        Ok(Self::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
            ts_event,
            ts_init,
        ))
    }
}

impl Serializable for TradeTick {}

impl Display for TradeTick {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{},{},{},{},{},{}",
            self.instrument_id,
            self.price,
            self.size,
            self.aggressor_side,
            self.trade_id,
            self.ts_event,
        )
    }
}

////////////////////////////////////////////////////////////////////////////////
// Python API
////////////////////////////////////////////////////////////////////////////////
#[cfg(feature = "python")]
#[pymethods]
impl TradeTick {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(
            instrument_id,
            price,
            size,
            aggressor_side,
            trade_id,
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
    fn price(&self) -> Price {
        self.price
    }

    #[getter]
    fn size(&self) -> Quantity {
        self.size
    }

    #[getter]
    fn aggressor_side(&self) -> AggressorSide {
        self.aggressor_side
    }

    #[getter]
    fn trade_id(&self) -> TradeId {
        self.trade_id
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
        data::trade::TradeTick,
        enums::AggressorSide,
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    #[fixture]
    pub fn trade_tick_ethusdt_buyer() -> TradeTick {
        TradeTick {
            instrument_id: InstrumentId::from("ETHUSDT-PERP.BINANCE"),
            price: Price::from("10000.0000"),
            size: Quantity::from("1.00000000"),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789").unwrap(),
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
    use crate::{data::trade::TradeTick, enums::AggressorSide};

    #[rstest]
    fn test_to_string(trade_tick_ethusdt_buyer: TradeTick) {
        let tick = trade_tick_ethusdt_buyer;
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,0"
        );
    }

    #[rstest]
    fn test_deserialize_raw_string() {
        let raw_string = r#"{
            "type": "TradeTick",
            "instrument_id": "ETHUSDT-PERP.BINANCE",
            "price": "10000.0000",
            "size": "1.00000000",
            "aggressor_side": "BUYER",
            "trade_id": "123456789",
            "ts_event": 0,
            "ts_init": 1
        }"#;

        let tick: TradeTick = serde_json::from_str(raw_string).unwrap();

        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
    }

    #[rstest]
    fn test_as_dict(trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();

        let tick = trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let dict_string = tick.as_dict(py).unwrap().to_string();
            let expected_string = r#"{'type': 'TradeTick', 'instrument_id': 'ETHUSDT-PERP.BINANCE', 'price': '10000.0000', 'size': '1.00000000', 'aggressor_side': 'BUYER', 'trade_id': '123456789', 'ts_event': 0, 'ts_init': 1}"#;
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();

        let tick = trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let dict = tick.as_dict(py).unwrap();
            let parsed = TradeTick::from_dict(py, dict).unwrap();
            assert_eq!(parsed, tick);
        });
    }

    #[rstest]
    fn test_from_pyobject(trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();
        let tick = trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let tick_pyobject = tick.into_py(py);
            let parsed_tick = TradeTick::from_pyobject(tick_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_tick, tick);
        });
    }

    #[rstest]
    fn test_json_serialization(trade_tick_ethusdt_buyer: TradeTick) {
        let tick = trade_tick_ethusdt_buyer;
        let serialized = tick.as_json_bytes().unwrap();
        let deserialized = TradeTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }

    #[rstest]
    fn test_msgpack_serialization(trade_tick_ethusdt_buyer: TradeTick) {
        let tick = trade_tick_ethusdt_buyer;
        let serialized = tick.as_msgpack_bytes().unwrap();
        let deserialized = TradeTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }
}
