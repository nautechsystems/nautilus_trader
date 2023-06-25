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
};

use nautilus_core::{serialization::Serializable, time::UnixNanos};
use pyo3::{exceptions::PyValueError, prelude::*, pyclass::CompareOp, types::PyDict};
use serde::{Deserialize, Serialize};

use crate::{
    enums::AggressorSide,
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    types::{price::Price, quantity::Quantity},
};

/// Represents a single trade tick in a financial market.
#[repr(C)]
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[pyclass]
pub struct TradeTick {
    pub instrument_id: InstrumentId,
    pub price: Price,
    pub size: Quantity,
    pub aggressor_side: AggressorSide,
    pub trade_id: TradeId,
    pub ts_event: UnixNanos,
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
        self.trade_id.clone()
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
    fn to_dict(&self) -> Py<PyDict> {
        Python::with_gil(|py| {
            let dict = PyDict::new(py);

            dict.set_item("type", stringify!(TradeTick)).unwrap();
            dict.set_item("instrument_id", self.instrument_id.to_string())
                .unwrap();
            dict.set_item("price", self.price.to_string()).unwrap();
            dict.set_item("size", self.size.to_string()).unwrap();
            dict.set_item("aggressor_side", self.aggressor_side.to_string())
                .unwrap();
            dict.set_item("trade_id", self.trade_id.to_string())
                .unwrap();
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

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use crate::{
        data::trade::TradeTick,
        enums::AggressorSide,
        identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
        types::{price::Price, quantity::Quantity},
    };

    #[test]
    fn test_to_string() {
        let tick = TradeTick {
            instrument_id: InstrumentId::from_str("ETHUSDT-PERP.BINANCE").unwrap(),
            price: Price::new(10000.0, 4),
            size: Quantity::new(1.0, 8),
            aggressor_side: AggressorSide::Buyer,
            trade_id: TradeId::new("123456789"),
            ts_event: 0,
            ts_init: 0,
        };
        assert_eq!(
            tick.to_string(),
            "ETHUSDT-PERP.BINANCE,10000.0000,1.00000000,BUYER,123456789,0"
        );
    }
}
