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
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    serialization::Serializable,
    time::UnixNanos,
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyDict, PyLong, PyString, PyTuple},
};

use crate::{
    data::trade::TradeTick,
    enums::{AggressorSide, FromU8},
    identifiers::{instrument_id::InstrumentId, trade_id::TradeId},
    python::PY_MODULE_MODEL,
    types::{price::Price, quantity::Quantity},
};

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

    fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
        let tuple: (
            &PyString,
            &PyLong,
            &PyLong,
            &PyLong,
            &PyLong,
            &PyLong,
            &PyString,
            &PyLong,
            &PyLong,
        ) = state.extract(py)?;
        let instrument_id_str: &str = tuple.0.extract()?;
        let price_raw = tuple.1.extract()?;
        let price_prec = tuple.2.extract()?;
        let size_raw = tuple.3.extract()?;
        let size_prec = tuple.4.extract()?;
        let aggressor_side_u8 = tuple.5.extract()?;
        let trade_id_str = tuple.6.extract()?;

        self.instrument_id = InstrumentId::from_str(instrument_id_str).map_err(to_pyvalue_err)?;
        self.price = Price::from_raw(price_raw, price_prec).map_err(to_pyvalue_err)?;
        self.size = Quantity::from_raw(size_raw, size_prec).map_err(to_pyvalue_err)?;
        self.aggressor_side = AggressorSide::from_u8(aggressor_side_u8).unwrap();
        self.trade_id = TradeId::from_str(trade_id_str).map_err(to_pyvalue_err)?;
        self.ts_event = tuple.7.extract()?;
        self.ts_init = tuple.8.extract()?;

        Ok(())
    }

    fn __getstate__(&self, _py: Python) -> PyResult<PyObject> {
        Ok((
            self.instrument_id.to_string(),
            self.price.raw,
            self.price.precision,
            self.size.raw,
            self.size.precision,
            self.aggressor_side as u8,
            self.trade_id.to_string(),
            self.ts_event,
            self.ts_init,
        )
            .to_object(_py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Ok(Self::new(
            InstrumentId::from("NULL.NULL"),
            Price::zero(0),
            Quantity::zero(0),
            AggressorSide::NoAggressor,
            TradeId::from("NULL"),
            0,
            0,
        ))
        // Safe default
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
        format!("{}({})", stringify!(TradeTick), self)
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

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(TradeTick))
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
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
    #[pyo3(name = "from_dict")]
    fn py_from_dict(py: Python<'_>, values: Py<PyDict>) -> PyResult<Self> {
        from_dict_pyo3(py, values)
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
    #[pyo3(name = "from_json")]
    fn py_from_json(data: Vec<u8>) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_msgpack")]
    fn py_from_msgpack(data: Vec<u8>) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(to_pyvalue_err)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrapping is safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py(py)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use pyo3::{IntoPy, Python};
    use rstest::rstest;

    use crate::data::trade::{stubs::*, TradeTick};

    #[rstest]
    fn test_as_dict(trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();
        let tick = trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let dict_string = tick.py_as_dict(py).unwrap().to_string();
            let expected_string = r#"{'type': 'TradeTick', 'instrument_id': 'ETHUSDT-PERP.BINANCE', 'price': '10000.0000', 'size': '1.00000000', 'aggressor_side': 'BUYER', 'trade_id': '123456789', 'ts_event': 0, 'ts_init': 1}"#;
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(trade_tick_ethusdt_buyer: TradeTick) {
        pyo3::prepare_freethreaded_python();
        let tick = trade_tick_ethusdt_buyer;

        Python::with_gil(|py| {
            let dict = tick.py_as_dict(py).unwrap();
            let parsed = TradeTick::py_from_dict(py, dict).unwrap();
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
}
