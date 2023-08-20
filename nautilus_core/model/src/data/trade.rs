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

use nautilus_core::{serialization::Serializable, time::UnixNanos};
use pyo3::{exceptions::PyValueError, prelude::*, pyclass::CompareOp, types::PyDict};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    enums::{AggressorSide, FromU8},
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    python::value_to_pydict,
    types::{price::Price, quantity::Quantity},
};

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type")]
#[pyclass]
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

    /// Create a new [`TradeTick`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &PyAny) -> PyResult<Self> {
        let instrument_id_obj: &PyAny = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id = InstrumentId::from_str(instrument_id_str)
            .map_err(|e| PyValueError::new_err(format!("{}", e)))
            .unwrap();

        let price_py: &PyAny = obj.getattr("price")?;
        let price_raw: i64 = price_py.getattr("raw")?.extract()?;
        let price_prec: u8 = price_py.getattr("precision")?.extract()?;
        let price = Price::from_raw(price_raw, price_prec);

        let size_py: &PyAny = obj.getattr("size")?;
        let size_raw: u64 = size_py.getattr("raw")?.extract()?;
        let size_prec: u8 = size_py.getattr("precision")?.extract()?;
        let size = Quantity::from_raw(size_raw, size_prec);

        let aggressor_side_obj: &PyAny = obj.getattr("aggressor_side")?.extract()?;
        let aggressor_side_u8 = aggressor_side_obj.getattr("value")?.extract()?;
        let aggressor_side = AggressorSide::from_u8(aggressor_side_u8).unwrap();

        let trade_id_obj: &PyAny = obj.getattr("trade_id")?.extract()?;
        let trade_id_str = trade_id_obj.getattr("value")?.extract()?;
        let trade_id = TradeId::from_str(trade_id_str)
            .map_err(|e| PyValueError::new_err(e.to_string()))
            .unwrap();

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
        // Serialize object to JSON values
        let json_value =
            serde_json::to_value(self).map_err(|e| PyValueError::new_err(e.to_string()))?;

        // Parse JSON into a Python dictionary
        if let Value::Object(_) = &json_value {
            value_to_pydict(py, &json_value)
        } else {
            Err(PyValueError::new_err("Expected JSON object"))
        }
    }

    // /// Return a new object from the given dictionary representation.
    // #[staticmethod]
    // pub fn from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
    //     // Serialize to JSON bytes
    //     let json_bytes: Vec<u8> = PyModule::import(py, "msgspec")?
    //         .getattr("json")?
    //         .call_method("encode", (values,), None)?
    //         .extract()?;
    //     // Deserialize to object
    //     let instance = serde_json::from_slice(&json_bytes)
    //         .map_err(|e| PyValueError::new_err(e.to_string()))?;
    //     Ok(instance)
    // }

    #[staticmethod]
    fn from_json(data: Vec<u8>) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    #[staticmethod]
    fn from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(|e| PyValueError::new_err(e.to_string()))
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
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use nautilus_core::serialization::Serializable;
    use pyo3::{IntoPy, Python};

    use crate::{
        data::trade::TradeTick,
        enums::AggressorSide,
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    fn create_stub_trade_tick() -> TradeTick {
        TradeTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            price: Price::new(10000.0, 4),
            size: Quantity::new(1.0, 8),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789"),
            ts_event: 1,
            ts_init: 0,
        }
    }

    #[test]
    fn test_to_string() {
        let tick = create_stub_trade_tick();
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,1"
        );
    }

    #[test]
    fn test_deserialize_raw_string() {
        let raw_string = r#"{
            "type": "TradeTick",
            "instrument_id": "ETHUSDT-PERP.BINANCE",
            "price": "10000.0000",
            "size": "1.00000000",
            "aggressor_side": "BUYER",
            "trade_id": "123456789",
            "ts_event": 1,
            "ts_init": 0
        }"#;

        let tick: TradeTick = serde_json::from_str(raw_string).unwrap();

        assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
    }

    // #[test]
    // fn test_as_dict() {
    //     pyo3::prepare_freethreaded_python();
    //
    //     let tick = create_stub_trade_tick();
    //
    //     Python::with_gil(|py| {
    //         let dict_string = tick.as_dict(py).unwrap().to_string();
    //         let expected_string = r#"{'type': 'TradeTick', 'instrument_id': 'ETHUSDT-PERP.BINANCE', 'price': '10000.0000', 'size': '1.00000000', 'aggressor_side': 'BUYER', 'trade_id': '123456789', 'ts_event': 1, 'ts_init': 0}"#;
    //         assert_eq!(dict_string, expected_string);
    //     });
    // }
    //
    // #[test]
    // fn test_from_dict() {
    //     pyo3::prepare_freethreaded_python();
    //
    //     let tick = create_stub_trade_tick();
    //
    //     Python::with_gil(|py| {
    //         let dict = tick.as_dict(py).unwrap();
    //         let parsed = TradeTick::from_dict(py, dict).unwrap();
    //         assert_eq!(parsed, tick);
    //     });
    // }

    #[test]
    fn test_from_pyobject() {
        pyo3::prepare_freethreaded_python();
        let tick = create_stub_trade_tick();

        Python::with_gil(|py| {
            let tick_pyobject = tick.into_py(py);
            let parsed_tick = TradeTick::from_pyobject(tick_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_tick, tick);
        });
    }

    #[test]
    fn test_json_serialization() {
        let tick = create_stub_trade_tick();
        let serialized = tick.as_json_bytes().unwrap();
        let deserialized = TradeTick::from_json_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }

    #[test]
    fn test_msgpack_serialization() {
        let tick = create_stub_trade_tick();
        let serialized = tick.as_msgpack_bytes().unwrap();
        let deserialized = TradeTick::from_msgpack_bytes(serialized).unwrap();
        assert_eq!(deserialized, tick);
    }
}
