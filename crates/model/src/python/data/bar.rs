// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
    serialization::{
        Serializable,
        msgpack::{FromMsgPack, ToMsgPack},
    },
};
use pyo3::{
    IntoPyObjectExt,
    prelude::*,
    pyclass::CompareOp,
    types::{PyDict, PyTuple},
};

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
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BarSpecification {
    /// Represents a bar aggregation specification including a step, aggregation
    /// method/rule and price type.
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

    #[getter]
    #[pyo3(name = "step")]
    fn py_step(&self) -> usize {
        self.step.get()
    }

    #[getter]
    #[pyo3(name = "aggregation")]
    fn py_aggregation(&self) -> BarAggregation {
        self.aggregation
    }

    #[getter]
    #[pyo3(name = "price_type")]
    fn py_price_type(&self) -> PriceType {
        self.price_type
    }

    #[staticmethod]
    #[pyo3(name = "fully_qualified_name")]
    fn py_fully_qualified_name() -> String {
        format!("{}:{}", PY_MODULE_MODEL, stringify!(BarSpecification))
    }

    /// Returns the `TimeDelta` interval for this bar specification.
    ///
    /// # Notes
    ///
    /// For `BarAggregation.Month` and `BarAggregation.Year`, proxy values are used
    /// (30 days for months, 365 days for years) to estimate their respective durations,
    /// since months and years have variable lengths.
    #[getter]
    #[pyo3(name = "timedelta")]
    fn py_timedelta(&self) -> PyResult<chrono::TimeDelta> {
        if !self.is_time_aggregated() {
            return Err(to_pyvalue_err(format!(
                "Timedelta not supported for aggregation type: {:?}",
                self.aggregation
            )));
        }
        Ok(self.timedelta())
    }

    /// Return a value indicating whether the aggregation method is time-driven:
    ///  - `BarAggregation.Millisecond`
    ///  - `BarAggregation.Second`
    ///  - `BarAggregation.Minute`
    ///  - `BarAggregation.Hour`
    ///  - `BarAggregation.Day`
    ///  - `BarAggregation.Week`
    ///  - `BarAggregation.Month`
    ///  - `BarAggregation.Year`
    #[pyo3(name = "is_time_aggregated")]
    fn py_is_time_aggregated(&self) -> bool {
        self.is_time_aggregated()
    }

    /// Return a value indicating whether the aggregation method is threshold-driven:
    ///  - `BarAggregation.Tick`
    ///  - `BarAggregation.TickImbalance`
    ///  - `BarAggregation.Volume`
    ///  - `BarAggregation.VolumeImbalance`
    ///  - `BarAggregation.Value`
    ///  - `BarAggregation.ValueImbalance`
    #[pyo3(name = "is_threshold_aggregated")]
    fn py_is_threshold_aggregated(&self) -> bool {
        self.is_threshold_aggregated()
    }

    /// Return a value indicating whether the aggregation method is information-driven:
    ///  - `BarAggregation.TickRuns`
    ///  - `BarAggregation.VolumeRuns`
    ///  - `BarAggregation.ValueRuns`
    #[pyo3(name = "is_information_aggregated")]
    fn py_is_information_aggregated(&self) -> bool {
        self.is_information_aggregated()
    }

    /// Returns the interval length in nanoseconds for time-based bar specifications.
    #[pyo3(name = "get_interval_ns")]
    fn py_get_interval_ns(&self) -> PyResult<u64> {
        if !self.is_time_aggregated() {
            return Err(to_pyvalue_err(format!(
                "Aggregation not time based, was {:?}",
                self.aggregation
            )));
        }
        let td = self.timedelta();
        Ok(td.num_nanoseconds().unwrap() as u64)
    }

    /// Creates a `BarSpecification` from a Python `timedelta` and price type.
    #[staticmethod]
    #[pyo3(name = "from_timedelta")]
    fn py_from_timedelta(duration: chrono::TimeDelta, price_type: PriceType) -> PyResult<Self> {
        if duration.num_milliseconds() <= 0 {
            return Err(to_pyvalue_err(format!(
                "Duration must be positive, was {duration:?}"
            )));
        }
        let total_secs_f64 = duration.num_milliseconds() as f64 / 1000.0;
        let days = duration.num_days();

        let (step, aggregation) = if days >= 7 {
            (days / 7, BarAggregation::Week)
        } else if days >= 1 {
            (days, BarAggregation::Day)
        } else if total_secs_f64 >= 3600.0 {
            ((total_secs_f64 / 3600.0) as i64, BarAggregation::Hour)
        } else if total_secs_f64 >= 60.0 {
            ((total_secs_f64 / 60.0) as i64, BarAggregation::Minute)
        } else if total_secs_f64 >= 1.0 {
            (total_secs_f64 as i64, BarAggregation::Second)
        } else {
            (
                (total_secs_f64 * 1000.0) as i64,
                BarAggregation::Millisecond,
            )
        };

        let spec =
            Self::new_checked(step as usize, aggregation, price_type).map_err(to_pyvalue_err)?;

        // Validate roundtrip
        let roundtrip = spec.timedelta();
        if roundtrip != duration {
            return Err(to_pyvalue_err(format!(
                "Duration {duration:?} is ambiguous"
            )));
        }

        Ok(spec)
    }

    /// Returns whether the given aggregation is time-based.
    #[staticmethod]
    #[pyo3(name = "check_time_aggregated")]
    fn py_check_time_aggregated(aggregation: BarAggregation) -> bool {
        matches!(
            aggregation,
            BarAggregation::Millisecond
                | BarAggregation::Second
                | BarAggregation::Minute
                | BarAggregation::Hour
                | BarAggregation::Day
                | BarAggregation::Week
                | BarAggregation::Month
                | BarAggregation::Year
        )
    }

    /// Returns whether the given aggregation is threshold-based.
    #[staticmethod]
    #[pyo3(name = "check_threshold_aggregated")]
    fn py_check_threshold_aggregated(aggregation: BarAggregation) -> bool {
        matches!(
            aggregation,
            BarAggregation::Tick
                | BarAggregation::TickImbalance
                | BarAggregation::Volume
                | BarAggregation::VolumeImbalance
                | BarAggregation::Value
                | BarAggregation::ValueImbalance
        )
    }

    /// Returns whether the given aggregation is information-based.
    #[staticmethod]
    #[pyo3(name = "check_information_aggregated")]
    fn py_check_information_aggregated(aggregation: BarAggregation) -> bool {
        matches!(
            aggregation,
            BarAggregation::TickRuns | BarAggregation::VolumeRuns | BarAggregation::ValueRuns
        )
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let from_str = py.get_type::<Self>().getattr("from_str")?;
        (from_str, (self.to_string(),)).into_py_any(py)
    }

    /// Creates a `BarSpecification` from a string representation.
    #[staticmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(value: &str) -> PyResult<Self> {
        let pieces: Vec<&str> = value.rsplitn(3, '-').collect();
        if pieces.len() != 3 {
            return Err(to_pyvalue_err(format!(
                "The `BarSpecification` string value was malformed, was {value}"
            )));
        }
        let step: usize = pieces[2].parse().map_err(to_pyvalue_err)?;
        let aggregation = BarAggregation::from_str(pieces[1]).map_err(to_pyvalue_err)?;
        let price_type = PriceType::from_str(pieces[0]).map_err(to_pyvalue_err)?;
        Self::new_checked(step, aggregation, price_type).map_err(to_pyvalue_err)
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BarType {
    /// Represents a bar type including the instrument ID, bar specification and
    /// aggregation source.
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

    /// Creates a new composite `BarType` instance.
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

    /// Returns whether this instance is a standard bar type.
    #[pyo3(name = "is_standard")]
    fn py_is_standard(&self) -> bool {
        self.is_standard()
    }

    /// Returns whether this instance is a composite bar type.
    #[pyo3(name = "is_composite")]
    fn py_is_composite(&self) -> bool {
        self.is_composite()
    }

    /// Returns the standard bar type component.
    #[pyo3(name = "standard")]
    fn py_standard(&self) -> Self {
        self.standard()
    }

    /// Returns any composite bar type component.
    #[pyo3(name = "composite")]
    fn py_composite(&self) -> Self {
        self.composite()
    }

    /// Returns the instrument ID and bar specification as a tuple key.
    ///
    /// Useful as a hashmap key when aggregation source should be ignored,
    /// such as for indicator registration where INTERNAL and EXTERNAL bars
    /// should trigger the same indicators.
    #[pyo3(name = "id_spec_key")]
    fn py_id_spec_key(&self) -> (InstrumentId, BarSpecification) {
        self.id_spec_key()
    }

    /// Returns whether this bar type is externally aggregated.
    #[pyo3(name = "is_externally_aggregated")]
    fn py_is_externally_aggregated(&self) -> bool {
        self.aggregation_source() == AggregationSource::External
    }

    /// Returns whether this bar type is internally aggregated.
    #[pyo3(name = "is_internally_aggregated")]
    fn py_is_internally_aggregated(&self) -> bool {
        self.aggregation_source() == AggregationSource::Internal
    }

    /// Returns the `InstrumentId` for this bar type.
    #[getter]
    #[pyo3(name = "instrument_id")]
    fn py_instrument_id(&self) -> InstrumentId {
        self.instrument_id()
    }

    /// Returns the `BarSpecification` for this bar type.
    #[getter]
    #[pyo3(name = "spec")]
    fn py_spec(&self) -> BarSpecification {
        self.spec()
    }

    /// Returns the `AggregationSource` for this bar type.
    #[getter]
    #[pyo3(name = "aggregation_source")]
    fn py_aggregation_source(&self) -> AggregationSource {
        self.aggregation_source()
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let from_str = py.get_type::<Self>().getattr("from_str")?;
        (from_str, (self.to_string(),)).into_py_any(py)
    }
}

impl Bar {
    /// Creates a Rust `Bar` instance from a Python object.
    ///
    /// # Errors
    ///
    /// Returns a `PyErr` if retrieving any attribute or converting types fails.
    pub fn from_pyobject(obj: &Bound<'_, PyAny>) -> PyResult<Self> {
        let bar_type_obj: Bound<'_, PyAny> = obj.getattr("bar_type")?.extract()?;
        let bar_type_str: String = bar_type_obj.call_method0("__str__")?.extract()?;
        let bar_type = BarType::from(bar_type_str);

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
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[expect(clippy::too_many_arguments)]
impl Bar {
    /// Represents an aggregated bar.
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

    /// Returns the metadata for the type, for use with serialization formats.
    #[staticmethod]
    #[pyo3(name = "get_metadata")]
    fn py_get_metadata(
        bar_type: &BarType,
        price_precision: u8,
        size_precision: u8,
    ) -> HashMap<String, String> {
        Self::get_metadata(bar_type, price_precision, size_precision)
    }

    /// Returns the field map for the type, for use with Arrow schemas.
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
    fn py_as_pycapsule(&self, py: Python<'_>) -> Py<PyAny> {
        data_to_pycapsule(py, Data::Bar(*self))
    }

    /// Return a dictionary representation of the object.
    #[pyo3(name = "to_dict")]
    fn py_to_dict(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        to_dict_pyo3(py, self)
    }

    /// Return JSON encoded bytes representation of the object.
    #[pyo3(name = "to_json_bytes")]
    fn py_to_json_bytes(&self, py: Python<'_>) -> Py<PyAny> {
        self.to_json_bytes().unwrap().into_py_any_unwrap(py)
    }

    /// Return `MsgPack` encoded bytes representation of the object.
    #[pyo3(name = "to_msgpack_bytes")]
    fn py_to_msgpack_bytes(&self, py: Python<'_>) -> Py<PyAny> {
        self.to_msgpack_bytes().unwrap().into_py_any_unwrap(py)
    }

    fn __setstate__(&mut self, state: &Bound<'_, PyAny>) -> PyResult<()> {
        let py_tuple: &Bound<'_, PyTuple> = state.cast::<PyTuple>()?;
        let bar_type_str: String = py_tuple.get_item(0)?.extract()?;
        let open_raw: PriceRaw = py_tuple.get_item(1)?.extract()?;
        let open_prec: u8 = py_tuple.get_item(2)?.extract()?;
        let high_raw: PriceRaw = py_tuple.get_item(3)?.extract()?;
        let low_raw: PriceRaw = py_tuple.get_item(4)?.extract()?;
        let close_raw: PriceRaw = py_tuple.get_item(5)?.extract()?;
        let volume_raw: QuantityRaw = py_tuple.get_item(6)?.extract()?;
        let volume_prec: u8 = py_tuple.get_item(7)?.extract()?;
        let ts_event: u64 = py_tuple.get_item(8)?.extract()?;
        let ts_init: u64 = py_tuple.get_item(9)?.extract()?;

        self.bar_type = BarType::from_str(&bar_type_str).map_err(to_pyvalue_err)?;
        self.open = Price::from_raw(open_raw, open_prec);
        self.high = Price::from_raw(high_raw, open_prec);
        self.low = Price::from_raw(low_raw, open_prec);
        self.close = Price::from_raw(close_raw, open_prec);
        self.volume = Quantity::from_raw(volume_raw, volume_prec);
        self.ts_event = ts_event.into();
        self.ts_init = ts_init.into();
        Ok(())
    }

    fn __getstate__(&self, py: Python) -> PyResult<Py<PyAny>> {
        (
            self.bar_type.to_string(),
            self.open.raw,
            self.open.precision,
            self.high.raw,
            self.low.raw,
            self.close.raw,
            self.volume.raw,
            self.volume.precision,
            self.ts_event.as_u64(),
            self.ts_init.as_u64(),
        )
            .into_py_any(py)
    }

    fn __reduce__(&self, py: Python) -> PyResult<Py<PyAny>> {
        let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
        let state = self.__getstate__(py)?;
        (safe_constructor, PyTuple::empty(py), state).into_py_any(py)
    }

    #[staticmethod]
    fn _safe_constructor() -> Self {
        Self::new(
            BarType::from("NULL.NULL-1-TICK-LAST-EXTERNAL"),
            Price::zero(0),
            Price::zero(0),
            Price::zero(0),
            Price::zero(0),
            Quantity::from(1),
            0.into(),
            0.into(),
        )
    }
}

#[pymethods]
impl Bar {
    #[staticmethod]
    #[pyo3(name = "from_json")]
    fn py_from_json(data: &[u8]) -> PyResult<Self> {
        Self::from_json_bytes(data).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_msgpack")]
    fn py_from_msgpack(data: &[u8]) -> PyResult<Self> {
        Self::from_msgpack_bytes(data).map_err(to_pyvalue_err)
    }
}

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
    fn test_to_dict() {
        let bar = Bar::default();

        Python::initialize();
        Python::attach(|py| {
            let dict_string = bar.py_to_dict(py).unwrap().to_string();
            let expected_string = "{'type': 'Bar', 'bar_type': 'AUDUSD.SIM-1-MINUTE-LAST-INTERNAL', 'open': '1.00010', 'high': '1.00020', 'low': '1.00000', 'close': '1.00010', 'volume': '100000', 'ts_event': 0, 'ts_init': 0}";
            assert_eq!(dict_string, expected_string);
        });
    }

    #[rstest]
    fn test_as_from_dict() {
        let bar = Bar::default();

        Python::initialize();
        Python::attach(|py| {
            let dict = bar.py_to_dict(py).unwrap();
            let parsed = Bar::py_from_dict(py, dict).unwrap();
            assert_eq!(parsed, bar);
        });
    }

    #[rstest]
    fn test_from_pyobject() {
        let bar = Bar::default();

        Python::initialize();
        Python::attach(|py| {
            let bar_pyobject = bar.into_py_any_unwrap(py);
            let parsed_bar = Bar::from_pyobject(bar_pyobject.bind(py)).unwrap();
            assert_eq!(parsed_bar, bar);
        });
    }
}
