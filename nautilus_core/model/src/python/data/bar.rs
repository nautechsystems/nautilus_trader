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
    python::{serialization::from_dict_pyo3, to_pyvalue_err},
    serialization::Serializable,
    time::UnixNanos,
};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};

use super::data_to_pycapsule;
use crate::{
    data::{
        bar::{Bar, BarSpecification, BarType},
        Data,
    },
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::instrument_id::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{price::Price, quantity::Quantity},
};

#[pymethods]
impl BarSpecification {
    #[new]
    fn py_new(step: usize, aggregation: BarAggregation, price_type: PriceType) -> Self {
        Self {
            step,
            aggregation,
            price_type,
        }
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

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(BarSpecification))
    }
}

#[pymethods]
impl BarType {
    #[new]
    #[pyo3(signature = (instrument_id, spec, aggregation_source = AggregationSource::External))]
    fn py_new(
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource,
    ) -> Self {
        Self {
            instrument_id,
            spec,
            aggregation_source,
        }
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

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(BarType))
    }

    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        Self::from_str(value).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[allow(clippy::too_many_arguments)]
impl Bar {
    #[new]
    fn py_new(
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> Self {
        Self::new(bar_type, open, high, low, close, volume, ts_event, ts_init)
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
    #[pyo3(name = "bar_type")]
    fn py_bar_type(&self) -> BarType {
        self.bar_type
    }

    #[getter]
    #[pyo3(name = "open")]
    fn py_open(&self) -> Price {
        self.open
    }

    #[getter]
    #[pyo3(name = "high")]
    fn py_high(&self) -> Price {
        self.high
    }

    #[getter]
    #[pyo3(name = "low")]
    fn py_low(&self) -> Price {
        self.low
    }

    #[getter]
    #[pyo3(name = "close")]
    fn py_close(&self) -> Price {
        self.close
    }

    #[getter]
    #[pyo3(name = "volume")]
    fn py_volume(&self) -> Quantity {
        self.volume
    }

    #[getter]
    #[pyo3(name = "ts_event")]
    fn py_ts_event(&self) -> UnixNanos {
        self.ts_event
    }

    #[getter]
    #[pyo3(name = "ts_init")]
    fn py_ts_init(&self) -> UnixNanos {
        self.ts_init
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(Bar))
    }

    /// Creates a `PyCapsule` containing a raw pointer to a `Data::Bar` object.
    ///
    /// This function takes the current object (assumed to be of a type that can be represented as
    /// `Data::Bar`), and encapsulates a raw pointer to it within a `PyCapsule`.
    ///
    /// # Safety
    ///
    /// This function is safe as long as the following conditions are met:
    /// - The `Data::Delta` object pointed to by the capsule must remain valid for the lifetime of the capsule.
    /// - The consumer of the capsule must ensure proper handling to avoid dereferencing a dangling pointer.
    ///
    /// # Panics
    ///
    /// The function will panic if the `PyCapsule` creation fails, which can occur if the
    /// `Data::Bar` object cannot be converted into a raw pointer.
    ///
    #[pyo3(name = "as_pycapsule")]
    fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
        data_to_pycapsule(py, Data::Bar(*self))
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
        bar_type: &BarType,
        price_precision: u8,
        size_precision: u8,
    ) -> PyResult<HashMap<String, String>> {
        Ok(Self::get_metadata(
            bar_type,
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

    use crate::data::bar::{stubs::stub_bar, Bar};

    #[rstest]
    fn test_as_dict(stub_bar: Bar) {
        pyo3::prepare_freethreaded_python();
        let bar = stub_bar;

        Python::with_gil(|py| {
            let dict_string = bar.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'Bar', 'bar_type': 'AUDUSD.SIM-1-MINUTE-BID-EXTERNAL', 'open': '1.00001', 'high': '1.00004', 'low': '1.00002', 'close': '1.00003', 'volume': '100000', 'ts_event': 0, 'ts_init': 1}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_as_from_dict(stub_bar: Bar) {
        pyo3::prepare_freethreaded_python();
        let bar = stub_bar;

        Python::with_gil(|py| {
            let dict = bar.py_as_dict(py).unwrap();
            let parsed = Bar::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, bar);
        });
    }

    #[rstest]
    fn test_from_pyobject(stub_bar: Bar) {
        pyo3::prepare_freethreaded_python();
        let bar = stub_bar;

        Python::with_gil(|py| {
            let bar_pyobject = bar.into_py(py);
            let parsed_bar = Bar::from_pyobject(bar_pyobject.as_ref(py)).unwrap();
            assert_eq!(parsed_bar, bar);
        });
    }
}
