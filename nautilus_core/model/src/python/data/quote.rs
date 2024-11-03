// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
    nanos::UnixNanos,
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    serialization::Serializable,
};
use pyo3::{
    prelude::*,
    pyclass::CompareOp,
    types::{PyDict, PyLong, PyString, PyTuple},
};

use super::data_to_pycapsule;
use crate::{
    data::{quote::QuoteTick, Data},
    enums::PriceType,
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{price::Price, quantity::Quantity},
};

impl QuoteTick {
    /// Create a new [`QuoteTick`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id_obj: Bound<'_, PyAny> = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str: String = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id =
            InstrumentId::from_str(instrument_id_str.as_str()).map_err(to_pyvalue_err)?;

        let bid_price_py: Bound<'_, PyAny> = obj.getattr("bid_price")?.extract()?;
        let bid_price_raw: i64 = bid_price_py.getattr("raw")?.extract()?;
        let bid_price_prec: u8 = bid_price_py.getattr("precision")?.extract()?;
        let bid_price = Price::from_raw(bid_price_raw, bid_price_prec);

        let ask_price_py: Bound<'_, PyAny> = obj.getattr("ask_price")?.extract()?;
        let ask_price_raw: i64 = ask_price_py.getattr("raw")?.extract()?;
        let ask_price_prec: u8 = ask_price_py.getattr("precision")?.extract()?;
        let ask_price = Price::from_raw(ask_price_raw, ask_price_prec);

        let bid_size_py: Bound<'_, PyAny> = obj.getattr("bid_size")?.extract()?;
        let bid_size_raw: u64 = bid_size_py.getattr("raw")?.extract()?;
        let bid_size_prec: u8 = bid_size_py.getattr("precision")?.extract()?;
        let bid_size = Quantity::from_raw(bid_size_raw, bid_size_prec);

        let ask_size_py: Bound<'_, PyAny> = obj.getattr("ask_size")?.extract()?;
        let ask_size_raw: u64 = ask_size_py.getattr("raw")?.extract()?;
        let ask_size_prec: u8 = ask_size_py.getattr("precision")?.extract()?;
        let ask_size = Quantity::from_raw(ask_size_raw, ask_size_prec);

        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        Self::new_checked(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
    }
}

#[pymethods]
impl QuoteTick {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Self::new_checked(
            instrument_id,
            bid_price,
            ask_price,
            bid_size,
            ask_size,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
    }

    fn __setstate__(&mut self, _py: Python, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.downcast::<PyTuple>()?;
        let binding = py_tuple.get_item(0)?;
        let instrument_id_str: &str = binding.downcast::<PyString>()?.extract()?;
        let bid_price_raw: i64 = py_tuple.get_item(1)?.downcast::<PyLong>()?.extract()?;
        let ask_price_raw: i64 = py_tuple.get_item(2)?.downcast::<PyLong>()?.extract()?;
        let bid_price_prec: u8 = py_tuple.get_item(3)?.downcast::<PyLong>()?.extract()?;
        let ask_price_prec: u8 = py_tuple.get_item(4)?.downcast::<PyLong>()?.extract()?;

        let bid_size_raw: u64 = py_tuple.get_item(5)?.downcast::<PyLong>()?.extract()?;
        let ask_size_raw: u64 = py_tuple.get_item(6)?.downcast::<PyLong>()?.extract()?;
        let bid_size_prec: u8 = py_tuple.get_item(7)?.downcast::<PyLong>()?.extract()?;
        let ask_size_prec: u8 = py_tuple.get_item(8)?.downcast::<PyLong>()?.extract()?;
        let ts_event: u64 = py_tuple.get_item(9)?.downcast::<PyLong>()?.extract()?;
        let ts_init: u64 = py_tuple.get_item(10)?.downcast::<PyLong>()?.extract()?;

        self.instrument_id = InstrumentId::from_str(instrument_id_str).map_err(to_pyvalue_err)?;
        self.bid_price = Price::from_raw(bid_price_raw, bid_price_prec);
        self.ask_price = Price::from_raw(ask_price_raw, ask_price_prec);
        self.bid_size = Quantity::from_raw(bid_size_raw, bid_size_prec);
        self.ask_size = Quantity::from_raw(ask_size_raw, ask_size_prec);
        self.ts_event = ts_event.into();
        self.ts_init = ts_init.into();

        Ok(())
    }

    fn __getstate__(&self, _py: Python) -> PyResult<PyObject> {
        Ok((
            self.instrument_id.to_string(),
            self.bid_price.raw,
            self.ask_price.raw,
            self.bid_price.precision,
            self.ask_price.precision,
            self.bid_size.raw,
            self.ask_size.raw,
            self.bid_size.precision,
            self.ask_size.precision,
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .to_object(_py))
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type_bound::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        Ok((safe_constructor, PyTuple::empty_bound(py), state).to_object(py))
    }

    #[staticmethod]
    fn _safe_constructor() -> PyResult<Self> {
        Self::new_checked(
            InstrumentId::from("NULL.NULL"),
            Price::zero(0),
            Price::zero(0),
            Quantity::zero(0),
            Quantity::zero(0),
            UnixNanos::default(),
            UnixNanos::default(),
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

    fn __repr__(&self) -> String {
        format!("{}({})", stringify!(QuoteTick), self)
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
    #[pyo3(name = "bid_price")]
    fn py_bid_price(&self) -> Price {
        self.bid_price
    }

    #[getter]
    #[pyo3(name = "ask_price")]
    fn py_ask_price(&self) -> Price {
        self.ask_price
    }

    #[getter]
    #[pyo3(name = "bid_size")]
    fn py_bid_size(&self) -> Quantity {
        self.bid_size
    }

    #[getter]
    #[pyo3(name = "ask_size")]
    fn py_ask_size(&self) -> Quantity {
        self.ask_size
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(QuoteTick))
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
        let py_dict = PyDict::new_bound(py);
        for (k, v) in Self::get_fields() {
            py_dict.set_item(k, v)?;
        }

        Ok(py_dict)
    }

    #[staticmethod]
    #[pyo3(name = "from_raw")]
    #[allow(clippy::too_many_arguments)]
    fn py_from_raw(
        _py: Python<'_>,
        instrument_id: InstrumentId,
        bid_price_raw: i64,
        ask_price_raw: i64,
        bid_price_prec: u8,
        ask_price_prec: u8,
        bid_size_raw: u64,
        ask_size_raw: u64,
        bid_size_prec: u8,
        ask_size_prec: u8,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Self::new_checked(
            instrument_id,
            Price::from_raw(bid_price_raw, bid_price_prec),
            Price::from_raw(ask_price_raw, ask_price_prec),
            Quantity::from_raw(bid_size_raw, bid_size_prec),
            Quantity::from_raw(ask_size_raw, ask_size_prec),
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
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

    #[pyo3(name = "extract_price")]
    fn py_extract_price(&self, price_type: PriceType) -> PyResult<Price> {
        Ok(self.extract_price(price_type))
    }

    #[pyo3(name = "extract_size")]
    fn py_extract_size(&self, price_type: PriceType) -> PyResult<Quantity> {
        Ok(self.extract_size(price_type))
    }

    /// Creates a `PyCapsule` containing a raw pointer to a `Data::Quote` object.
    ///
    /// This function takes the current object (assumed to be of a type that can be represented as
    /// `Data::Quote`), and encapsulates a raw pointer to it within a `PyCapsule`.
    ///
    /// # Safety
    ///
    /// This function is safe as long as the following conditions are met:
    /// - The `Data::Quote` object pointed to by the capsule must remain valid for the lifetime of the capsule.
    /// - The consumer of the capsule must ensure proper handling to avoid dereferencing a dangling pointer.
    ///
    /// # Panics
    ///
    /// The function will panic if the `PyCapsule` creation fails, which can occur if the
    /// `Data::Quote` object cannot be converted into a raw pointer.
    #[pyo3(name = "as_pycapsule")]
    fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
        data_to_pycapsule(py, Data::Quote(*self))
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        // Serialize object to JSON bytes
        let json_str = serde_json::to_string(self).map_err(to_pyvalue_err)?;
        // Parse JSON into a Python dictionary
        let py_dict: Py<PyDict> = PyModule::import_bound(py, "json")?
            .call_method("loads", (json_str,), None)?
            .extract()?;
        Ok(py_dict)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
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

    use crate::data::{quote::QuoteTick, stubs::quote_ethusdt_binance};

    #[rstest]
    fn test_as_dict(quote_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();
        let quote = quote_ethusdt_binance;

        Python::with_gil(|py| {
            let dict_string = quote.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'QuoteTick', 'instrument_id': 'ETHUSDT-PERP.BINANCE', 'bid_price': '10000.0000', 'ask_price': '10001.0000', 'bid_size': '1.00000000', 'ask_size': '1.00000000', 'ts_event': 0, 'ts_init': 1}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_from_dict(quote_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();
        let quote = quote_ethusdt_binance;

        Python::with_gil(|py| {
            let dict = quote.py_as_dict(py).unwrap();
            let parsed = QuoteTick::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, quote);
        });
    }

    #[rstest]
    fn test_from_pyobject(quote_ethusdt_binance: QuoteTick) {
        pyo3::prepare_freethreaded_python();
        let quote = quote_ethusdt_binance;

        Python::with_gil(|py| {
            let tick_pyobject = quote.into_py(py);
            let parsed_tick = QuoteTick::from_pyobject(tick_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_tick, quote);
        });
    }
}
