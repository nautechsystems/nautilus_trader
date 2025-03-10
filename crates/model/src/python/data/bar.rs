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
    collections::{HashMap, hash_map::DefaultHasher},
    hash::{Hash, Hasher},
    str::FromStr,
};

use nautilus_core::{
    python::{
        IntoPyObjectNautilusExt,
        serialization::{from_dict_pyo3, to_dict_pyo3},
        to_pyvalue_err,
    },
    serialization::Serializable,
};
use pyo3::{prelude::*, pyclass::CompareOp, types::PyDict};

use super::data_to_pycapsule;
use crate::{
    data::{
        Data,
        bar::{Bar, BarSpecification, BarType},
    },
    enums::{AggregationSource, BarAggregation, PriceType},
    identifiers::InstrumentId,
    python::common::PY_MODULE_MODEL,
    types::{
        price::{Price, PriceRaw},
        quantity::{Quantity, QuantityRaw},
    },
};

#[pymethods]
impl BarSpecification {
    #[new]
    fn py_new(step: usize, aggregation: BarAggregation, price_type: PriceType) -> PyResult<Self> {
        Self::new_checked(step, aggregation, price_type).map_err(to_pyvalue_err)
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
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
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
    #[pyo3(signature = (instrument_id, spec, aggregation_source = AggregationSource::External)
    )]
    fn py_new(
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource,
    ) -> Self {
        Self::new(instrument_id, spec, aggregation_source)
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
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
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

    #[staticmethod]
    #[pyo3(name = "new_composite")]
    fn py_new_composite(
        instrument_id: InstrumentId,
        spec: BarSpecification,
        aggregation_source: AggregationSource,
        composite_step: usize,
        composite_aggregation: BarAggregation,
        composite_aggregation_source: AggregationSource,
    ) -> Self {
        Self::new_composite(
            instrument_id,
            spec,
            aggregation_source,
            composite_step,
            composite_aggregation,
            composite_aggregation_source,
        )
    }

    #[pyo3(name = "is_standard")]
    fn py_is_standard(&self) -> bool {
        self.is_standard()
    }

    #[pyo3(name = "is_composite")]
    fn py_is_composite(&self) -> bool {
        self.is_composite()
    }

    #[pyo3(name = "standard")]
    fn py_standard(&self) -> Self {
        self.standard()
    }

    #[pyo3(name = "composite")]
    fn py_composite(&self) -> Self {
        self.composite()
    }
}

impl Bar {
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let bar_type_obj: Bound<'_, PyAny> = obj.getattr("bar_type")?.extract()?;
        let bar_type_str: String = bar_type_obj.call_method0("__str__")?.extract()?;
        let bar_type = BarType::from(bar_type_str.as_str());

        let open_py: Bound<'_, PyAny> = obj.getattr("open")?;
        let price_prec: u8 = open_py.getattr("precision")?.extract()?;
        let open_raw: PriceRaw = open_py.getattr("raw")?.extract()?;
        let open = Price::from_raw(open_raw, price_prec);

        let high_py: Bound<'_, PyAny> = obj.getattr("high")?;
        let high_raw: PriceRaw = high_py.getattr("raw")?.extract()?;
        let high = Price::from_raw(high_raw, price_prec);

        let low_py: Bound<'_, PyAny> = obj.getattr("low")?;
        let low_raw: PriceRaw = low_py.getattr("raw")?.extract()?;
        let low = Price::from_raw(low_raw, price_prec);

        let close_py: Bound<'_, PyAny> = obj.getattr("close")?;
        let close_raw: PriceRaw = close_py.getattr("raw")?.extract()?;
        let close = Price::from_raw(close_raw, price_prec);

        let volume_py: Bound<'_, PyAny> = obj.getattr("volume")?;
        let volume_raw: QuantityRaw = volume_py.getattr("raw")?.extract()?;
        let volume_prec: u8 = volume_py.getattr("precision")?.extract()?;
        let volume = Quantity::from_raw(volume_raw, volume_prec);

        let ts_event: u64 = obj.getattr("ts_event")?.extract()?;
        let ts_init: u64 = obj.getattr("ts_init")?.extract()?;

        Ok(Self::new(
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            ts_event.into(),
            ts_init.into(),
        ))
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
        ts_event: u64,
        ts_init: u64,
    ) -> PyResult<Self> {
        Self::new_checked(
            bar_type,
            open,
            high,
            low,
            close,
            volume,
            ts_event.into(),
            ts_init.into(),
        )
        .map_err(to_pyvalue_err)
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
        format!("{self:?}")
    }

    fn __str__(&self) -> String {
        self.to_string()
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
        format!("{}:{}", PY_MODULE_MODEL, stringify!(Bar))
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
    fn py_get_fields(py: Python<'_>) -> PyResult<Bound<'_, PyDict>> {
        let py_dict = PyDict::new(py);
        for (k, v) in Self::get_fields() {
            py_dict.set_item(k, v)?;
        }

        Ok(py_dict)
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
    #[pyo3(name = "as_pycapsule")]
    fn py_as_pycapsule(&self, py: Python<'_>) -> PyObject {
        data_to_pycapsule(py, Data::Bar(*self))
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "as_dict")]
    fn py_as_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "as_json")]
    fn py_as_json(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
        self.as_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return MsgPack encoded bytes representation of the object.
    #[pyo3(name = "as_msgpack")]
    fn py_as_msgpack(&self, py: Python<'_>) -> Py<PyAny> {
        // Unwrapping is safe when serializing a valid object
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
    use rstest::rstest;

    use crate::{
        data::{Bar, BarType},
        types::{Price, Quantity},
    };

    #[rstest]
    #[case("10.0000", "10.0010", "10.0020", "10.0005")] // low > high
    #[case("10.0000", "10.0010", "10.0005", "10.0030")] // close > high
    #[case("10.0000", "9.9990", "9.9980", "9.9995")] // high < open
    #[case("10.0000", "10.0010", "10.0015", "10.0020")] // low > close
    #[case("10.0000", "10.0000", "10.0001", "10.0002")] // low > high (equal high/open edge case)
    fn test_bar_py_new_invalid(
        #[case] open: &str,
        #[case] high: &str,
        #[case] low: &str,
        #[case] close: &str,
    ) {
        pyo3::prepare_freethreaded_python();

        let bar_type = BarType::from("AUDUSD.SIM-1-MINUTE-LAST-INTERNAL");
        let open = Price::from(open);
        let high = Price::from(high);
        let low = Price::from(low);
        let close = Price::from(close);
        let volume = Quantity::from(100_000);
        let ts_event = 0;
        let ts_init = 1;

        let result = Bar::py_new(bar_type, open, high, low, close, volume, ts_event, ts_init);
        assert!(result.is_err());
    }

    #[rstest]
    fn test_bar_py_new() {
        pyo3::prepare_freethreaded_python();

        let bar_type = BarType::from("AUDUSD.SIM-1-MINUTE-LAST-INTERNAL");
        let open = Price::from("1.00005");
        let high = Price::from("1.00010");
        let low = Price::from("1.00000");
        let close = Price::from("1.00007");
        let volume = Quantity::from(100_000);
        let ts_event = 0;
        let ts_init = 1;

        let result = Bar::py_new(bar_type, open, high, low, close, volume, ts_event, ts_init);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_as_dict() {
        pyo3::prepare_freethreaded_python();
        let bar = Bar::default();

        Python::with_gil(|py| {
            let dict_string = bar.py_as_dict(py).unwrap().to_string();
            let expected_string = r"{'type': 'Bar', 'bar_type': 'AUDUSD.SIM-1-MINUTE-LAST-INTERNAL', 'open': '1.00010', 'high': '1.00020', 'low': '1.00000', 'close': '1.00010', 'volume': '100000', 'ts_event': 0, 'ts_init': 0}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_as_from_dict() {
        pyo3::prepare_freethreaded_python();
        let bar = Bar::default();

        Python::with_gil(|py| {
            let dict = bar.py_as_dict(py).unwrap();
            let parsed = Bar::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, bar);
        });
    }

    #[rstest]
    fn test_from_pyobject() {
        pyo3::prepare_freethreaded_python();
        let bar = Bar::default();

        Python::with_gil(|py| {
            let bar_pyobject = bar.into_py_any_unwrap(py);
            let parsed_bar = Bar::from_pyobject(bar_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_bar, bar);
        });
    }
}
