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
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    UnixNanos,
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::Serializable,
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    pyclass::CompareOp,
    types::{PyDict, PyInt, PyString, PyTuple},
};

use crate::{
    data::{IndexPriceUpdate, MarkPriceUpdate},
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::price::{Price, PriceRaw},
};

impl MarkPriceUpdate {
    /// Create a new [`MarkPriceUpdate`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id_obj: Bound<'_, PyAny> = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str: String = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id =
            InstrumentId::from_str(instrument_id_str.as_str()).map_err(to_pyvalue_err)?;

        let value_py: Bound<'_, PyAny> = obj.getattr("value")?.extract()?;
        let value_raw: PriceRaw = value_py.getattr("raw")?.extract()?;
        let value_prec: u8 = value_py.getattr("precision")?.extract()?;
        let value = Price::from_raw(value_raw, value_prec);

        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        Ok(Self::new(
            instrument_id,
            value,
            ts_event.into(),
            ts_init.into(),
        ))
    }
}

#[pymethods]
impl MarkPriceUpdate {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        value: Price,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Ok(Self::new(
            instrument_id,
            value,
            ts_event.into(),
            ts_init.into(),
        ))
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.downcast::<PyTuple>()?;
        let binding = py_tuple.get_item(0)?;
        let instrument_id_str = binding.downcast::<PyString>()?.extract::<&str>()?;
        let value_raw = py_tuple
            .get_item(1)?
            .downcast::<PyInt>()?
            .extract::<PriceRaw>()?;
        let value_prec = py_tuple.get_item(2)?.downcast::<PyInt>()?.extract::<u8>()?;

        let ts_event = py_tuple
            .get_item(7)?
            .downcast::<PyInt>()?
            .extract::<u64>()?;
        let ts_init = py_tuple
            .get_item(8)?
            .downcast::<PyInt>()?
            .extract::<u64>()?;

        self.instrument_id = InstrumentId::from_str(instrument_id_str).map_err(to_pyvalue_err)?;
        self.value = Price::from_raw(value_raw, value_prec);
        self.ts_event = ts_event.into();
        self.ts_init = ts_init.into();

        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        (
            self.instrument_id.to_string(),
            self.value.raw,
            self.value.precision,
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .into_py_any(py)
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        (safe_constructor, PyTuple::empty(py), state).into_py_any(py)
    }

    #[staticmethod]
    fn _safe_constructor() -> Self {
        Self::new(
            InstrumentId::from("NULL.NULL"),
            Price::zero(0),
            UnixNanos::default(),
            UnixNanos::default(),
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
        format!("{}({})", stringify!(MarkPriceUpdate), self)
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
    #[pyo3(name = "value")]
    fn py_value(&self) -> Price {
        self.value
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(MarkPriceUpdate))
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

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrap safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrap safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py_any_unwrap(py)
    }
}

impl IndexPriceUpdate {
    /// Create a new [`IndexPriceUpdate`] extracted from the given [`PyAny`].
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let instrument_id_obj: Bound<'_, PyAny> = obj.getattr("instrument_id")?.extract()?;
        let instrument_id_str: String = instrument_id_obj.getattr("value")?.extract()?;
        let instrument_id =
            InstrumentId::from_str(instrument_id_str.as_str()).map_err(to_pyvalue_err)?;

        let value_py: Bound<'_, PyAny> = obj.getattr("value")?.extract()?;
        let value_raw: PriceRaw = value_py.getattr("raw")?.extract()?;
        let value_prec: u8 = value_py.getattr("precision")?.extract()?;
        let value = Price::from_raw(value_raw, value_prec);

        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        Ok(Self::new(
            instrument_id,
            value,
            ts_event.into(),
            ts_init.into(),
        ))
    }
}

#[pymethods]
impl IndexPriceUpdate {
    #[new]
    fn py_new(
        instrument_id: InstrumentId,
        value: Price,
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Ok(Self::new(
            instrument_id,
            value,
            ts_event.into(),
            ts_init.into(),
        ))
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.downcast::<PyTuple>()?;
        let binding = py_tuple.get_item(0)?;
        let instrument_id_str = binding.downcast::<PyString>()?.extract::<&str>()?;
        let value_raw = py_tuple
            .get_item(1)?
            .downcast::<PyInt>()?
            .extract::<PriceRaw>()?;
        let value_prec = py_tuple.get_item(2)?.downcast::<PyInt>()?.extract::<u8>()?;

        let ts_event = py_tuple
            .get_item(7)?
            .downcast::<PyInt>()?
            .extract::<u64>()?;
        let ts_init = py_tuple
            .get_item(8)?
            .downcast::<PyInt>()?
            .extract::<u64>()?;

        self.instrument_id = InstrumentId::from_str(instrument_id_str).map_err(to_pyvalue_err)?;
        self.value = Price::from_raw(value_raw, value_prec);
        self.ts_event = ts_event.into();
        self.ts_init = ts_init.into();

        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
        (
            self.instrument_id.to_string(),
            self.value.raw,
            self.value.precision,
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .into_py_any(py)
    }

    fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        (safe_constructor, PyTuple::empty(py), state).into_py_any(py)
    }

    #[staticmethod]
    fn _safe_constructor() -> Self {
        Self::new(
            InstrumentId::from("NULL.NULL"),
            Price::zero(0),
            UnixNanos::default(),
            UnixNanos::default(),
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
        format!("{}({})", stringify!(IndexPriceUpdate), self)
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
    #[pyo3(name = "value")]
    fn py_value(&self) -> Price {
        self.value
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(IndexPriceUpdate))
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

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrap safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // SAFETY: Unwrap safe when serializing a valid object
        self.as_msgpack_bytes().unwrap().into_py_any_unwrap(py)
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_core::python::IntoPyObjectNautilusExt;
    use pyo3::Python;
    use rstest::{fixture, rstest};

    use super::*;
    use crate::{identifiers::InstrumentId, types::Price};

    #[fixture]
    fn mark_price() -> MarkPriceUpdate {
        MarkPriceUpdate::new(
            InstrumentId::from("BTC-USDT.OKX"),
            Price::from("100_000.00"),
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
    }

    #[fixture]
    fn index_price() -> IndexPriceUpdate {
        IndexPriceUpdate::new(
            InstrumentId::from("BTC-USDT.OKX"),
            Price::from("100_000.00"),
            UnixNanos::from(1),
            UnixNanos::from(2),
        )
    }

    #[rstest]
    fn test_mark_price_as_dict(mark_price: MarkPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict_string = mark_price.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'MarkPriceUpdate', 'instrument_id': 'BTC-USDT.OKX', 'value': '100000.00', 'ts_event': 1, 'ts_init': 2}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_mark_price_from_dict(mark_price: MarkPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict = mark_price.py_as_dict(py).unwrap();
            let parsed = MarkPriceUpdate::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, mark_price);
        });
    }

    #[rstest]
    fn test_mark_price_from_pyobject(mark_price: MarkPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let tick_pyobject = mark_price.into_py_any_unwrap(py);
            let parsed_tick = MarkPriceUpdate::from_pyobject(tick_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_tick, mark_price);
        });
    }

    #[rstest]
    fn test_index_price_as_dict(index_price: IndexPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict_string = index_price.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'IndexPriceUpdate', 'instrument_id': 'BTC-USDT.OKX', 'value': '100000.00', 'ts_event': 1, 'ts_init': 2}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_index_price_from_dict(index_price: IndexPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let dict = index_price.py_as_dict(py).unwrap();
            let parsed = IndexPriceUpdate::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, index_price);
        });
    }

    #[rstest]
    fn test_index_price_from_pyobject(index_price: IndexPriceUpdate) {
        pyo3::prepare_freethreaded_python();

        Python::with_gil(|py| {
            let tick_pyobject = index_price.into_py_any_unwrap(py);
            let parsed_tick = IndexPriceUpdate::from_pyobject(tick_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_tick, index_price);
        });
    }
}
