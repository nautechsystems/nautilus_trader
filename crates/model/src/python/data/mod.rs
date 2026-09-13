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

//! Data types for the trading domain model.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

pub mod bar;
pub mod bet;
pub mod close;
pub mod data_type;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod funding;
pub mod greeks;
pub mod option_chain;
pub mod order;
pub mod prices;
pub mod quote;
pub mod status;
pub mod trade;

#[cfg(feature = "python")]
pub mod custom;

#[cfg(feature = "python")]
use nautilus_core::python::{to_pyruntime_err, to_pytype_err, to_pyvalue_err};
use pyo3::prelude::*;

use crate::{
    data::{
        Bar, CustomData, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
        MarkPriceUpdate, NautilusDataType, NautilusRecordType, OptionGreeks, OrderBookDelta,
        QuoteTick, TradeTick, close::InstrumentClose, is_monotonically_increasing_by_init,
        register_python_data_class,
    },
    python::instruments::instrument_any_to_pyobject,
};

const ERROR_MONOTONICITY: &str = "`data` was not monotonically increasing by the `ts_init` field";

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[pyclass(
    frozen,
    name = "NautilusDataType",
    module = "nautilus_trader.model",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
pub struct PyNautilusDataType {
    inner: NautilusDataType,
}

impl PyNautilusDataType {
    #[must_use]
    pub const fn new(inner: NautilusDataType) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn into_inner(self) -> NautilusDataType {
        self.inner
    }

    #[must_use]
    pub fn inner(&self) -> NautilusDataType {
        self.inner.clone()
    }
}

macro_rules! define_nautilus_data_type_class_attrs {
    (
        $(($variant:ident, $type:ident, $data:ident, $batch:ident, $prefix:literal)),+ $(,)?
    ) => {
        #[pyo3_stub_gen::derive::gen_stub_pymethods]
        #[pymethods]
        impl PyNautilusDataType {
            $(
                #[classattr]
                #[expect(
                    non_snake_case,
                    reason = "Python selector attributes match Rust data type variant names"
                )]
                fn $variant() -> PyNautilusDataType {
                    Self::new(NautilusDataType::$variant)
                }
            )+
        }
    };
}

crate::for_each_data_type!(define_nautilus_data_type_class_attrs);

#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pymethods]
impl PyNautilusDataType {
    #[classattr]
    #[expect(
        non_snake_case,
        clippy::use_self,
        reason = "PyO3 stub generation expands class attributes outside the impl scope"
    )]
    fn OrderBook() -> PyNautilusDataType {
        Self::new(NautilusDataType::OrderBook)
    }

    #[cfg(feature = "defi")]
    #[classattr]
    #[expect(
        clippy::use_self,
        reason = "PyO3 stub generation needs the concrete return type"
    )]
    #[expect(
        non_snake_case,
        reason = "Python selector attributes match Rust data type variant names"
    )]
    fn Defi() -> PyNautilusDataType {
        Self::new(NautilusDataType::Defi)
    }

    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        value
            .parse::<NautilusDataType>()
            .map(Self::new)
            .map_err(to_pyruntime_err)
    }

    #[staticmethod]
    #[pyo3(name = "Custom")]
    fn py_custom(type_name: String) -> Self {
        Self::new(NautilusDataType::Custom { type_name })
    }

    #[getter]
    fn type_name(&self) -> Option<&str> {
        match &self.inner {
            NautilusDataType::Custom { type_name } => Some(type_name),
            _ => None,
        }
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        match &self.inner {
            NautilusDataType::Custom { type_name } => {
                format!("NautilusDataType.Custom({type_name:?})")
            }
            _ => format!("NautilusDataType.{}", self.inner),
        }
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        use nautilus_core::python::IntoPyObjectNautilusExt;

        match op {
            pyo3::pyclass::CompareOp::Eq => (self.inner == other.inner).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => (self.inner != other.inner).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish() as isize
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[pyclass(
    frozen,
    name = "NautilusRecordType",
    module = "nautilus_trader.model",
    skip_from_py_object
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.model")]
pub struct PyNautilusRecordType {
    inner: NautilusRecordType,
}

impl PyNautilusRecordType {
    #[must_use]
    pub const fn new(inner: NautilusRecordType) -> Self {
        Self { inner }
    }

    #[must_use]
    pub fn into_inner(self) -> NautilusRecordType {
        self.inner
    }

    #[must_use]
    pub fn inner(&self) -> NautilusRecordType {
        self.inner.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyNautilusRecordType {
    #[new]
    fn py_new(value: &str) -> PyResult<Self> {
        value
            .parse::<NautilusRecordType>()
            .map(Self::new)
            .map_err(to_pyruntime_err)
    }

    fn __str__(&self) -> String {
        self.inner.to_string()
    }

    fn __repr__(&self) -> String {
        format!("NautilusRecordType.{}", self.inner)
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        use nautilus_core::python::IntoPyObjectNautilusExt;

        match op {
            pyo3::pyclass::CompareOp::Eq => (self.inner == other.inner).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => (self.inner != other.inner).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        let mut hasher = DefaultHasher::new();
        self.inner.hash(&mut hasher);
        hasher.finish() as isize
    }
}

/// Converts a [`Data`] value into its native PyO3 object.
///
/// # Errors
///
/// Returns an error for data variants without a Python representation.
pub fn data_to_pyobject(py: Python<'_>, data: Data) -> PyResult<Py<PyAny>> {
    match data {
        Data::Instrument(instrument) => instrument_any_to_pyobject(py, *instrument),
        Data::Quote(quote) => Py::new(py, quote).map(Py::into_any),
        Data::Trade(trade) => Py::new(py, trade).map(Py::into_any),
        Data::Bar(bar) => Py::new(py, bar).map(Py::into_any),
        Data::BookDelta(delta) => Py::new(py, delta).map(Py::into_any),
        Data::BookDeltas(deltas) => Py::new(py, (*deltas).clone()).map(Py::into_any),
        Data::BookDepth(depth) => Py::new(py, *depth).map(Py::into_any),
        Data::MarkPrice(price) => Py::new(py, price).map(Py::into_any),
        Data::IndexPrice(price) => Py::new(py, price).map(Py::into_any),
        Data::FundingRate(funding) => Py::new(py, funding).map(Py::into_any),
        Data::OptionGreeks(greeks) => Py::new(py, greeks).map(Py::into_any),
        Data::InstrumentStatus(status) => Py::new(py, status).map(Py::into_any),
        Data::InstrumentClose(close) => Py::new(py, close).map(Py::into_any),
        Data::Custom(custom) => Py::new(py, custom).map(Py::into_any),
        #[cfg(feature = "defi")]
        Data::Defi(_) => Err(to_pytype_err("Unsupported DeFi data variant")),
    }
}

/// Transforms the given Python objects into a vector of [`OrderBookDelta`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_book_deltas(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<OrderBookDelta>> {
    let deltas: Vec<OrderBookDelta> = data
        .into_iter()
        .map(|obj| obj.extract::<OrderBookDelta>().map_err(PyErr::from))
        .collect::<PyResult<Vec<OrderBookDelta>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&deltas) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(deltas)
}

/// Transforms the given Python objects into a vector of [`QuoteTick`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_quotes(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<QuoteTick>> {
    let quotes: Vec<QuoteTick> = data
        .into_iter()
        .map(|obj| obj.extract::<QuoteTick>().map_err(PyErr::from))
        .collect::<PyResult<Vec<QuoteTick>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&quotes) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(quotes)
}

/// Transforms the given Python objects into a vector of [`TradeTick`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_trades(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<TradeTick>> {
    let trades: Vec<TradeTick> = data
        .into_iter()
        .map(|obj| obj.extract::<TradeTick>().map_err(PyErr::from))
        .collect::<PyResult<Vec<TradeTick>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&trades) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(trades)
}

/// Transforms the given Python objects into a vector of [`Bar`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_bars(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<Bar>> {
    let bars: Vec<Bar> = data
        .into_iter()
        .map(|obj| obj.extract::<Bar>().map_err(PyErr::from))
        .collect::<PyResult<Vec<Bar>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&bars) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(bars)
}

/// Transforms the given Python objects into a vector of [`MarkPriceUpdate`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_mark_prices(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<MarkPriceUpdate>> {
    let mark_prices: Vec<MarkPriceUpdate> = data
        .into_iter()
        .map(|obj| obj.extract::<MarkPriceUpdate>().map_err(PyErr::from))
        .collect::<PyResult<Vec<MarkPriceUpdate>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&mark_prices) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(mark_prices)
}

/// Transforms the given Python objects into a vector of [`IndexPriceUpdate`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_index_prices(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<IndexPriceUpdate>> {
    let index_prices: Vec<IndexPriceUpdate> = data
        .into_iter()
        .map(|obj| obj.extract::<IndexPriceUpdate>().map_err(PyErr::from))
        .collect::<PyResult<Vec<IndexPriceUpdate>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&index_prices) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(index_prices)
}

/// Transforms the given Python objects into a vector of [`InstrumentStatus`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_instrument_statuses(
    data: Vec<Bound<'_, PyAny>>,
) -> PyResult<Vec<InstrumentStatus>> {
    let statuses: Vec<InstrumentStatus> = data
        .into_iter()
        .map(|obj| obj.extract::<InstrumentStatus>().map_err(PyErr::from))
        .collect::<PyResult<Vec<InstrumentStatus>>>()?;

    if !is_monotonically_increasing_by_init(&statuses) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(statuses)
}

/// Transforms the given Python objects into a vector of [`OptionGreeks`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_option_greeks(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<OptionGreeks>> {
    let greeks: Vec<OptionGreeks> = data
        .into_iter()
        .map(|obj| obj.extract::<OptionGreeks>().map_err(PyErr::from))
        .collect::<PyResult<Vec<OptionGreeks>>>()?;

    if !is_monotonically_increasing_by_init(&greeks) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(greeks)
}

/// Transforms the given Python objects into a vector of [`InstrumentClose`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_instrument_closes(
    data: Vec<Bound<'_, PyAny>>,
) -> PyResult<Vec<InstrumentClose>> {
    let closes: Vec<InstrumentClose> = data
        .into_iter()
        .map(|obj| obj.extract::<InstrumentClose>().map_err(PyErr::from))
        .collect::<PyResult<Vec<InstrumentClose>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&closes) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(closes)
}

/// Deserializes custom data from JSON bytes into a PyO3 `CustomData` wrapper.
///
/// # Errors
///
/// Returns a `PyErr` if the type is not registered or JSON deserialization fails.
#[cfg(feature = "python")]
#[pyfunction]
pub fn deserialize_custom_from_json(type_name: &str, payload: &[u8]) -> PyResult<CustomData> {
    use crate::data::registry;
    let value: serde_json::Value = serde_json::from_slice(payload)
        .map_err(|e| to_pyvalue_err(format!("Invalid JSON: {e}")))?;
    let Some(Data::Custom(custom)) = registry::deserialize_custom_from_json(type_name, &value)
        .map_err(|e| to_pyvalue_err(format!("Deserialization failed: {e}")))?
    else {
        return Err(to_pyvalue_err(format!(
            "Custom data type \"{type_name}\" is not registered"
        )));
    };
    Ok(custom)
}

/// Deserializes JSON value to `CustomData` via the data class's `from_json`.
#[cfg(feature = "python")]
fn py_json_deserialize_custom_data(
    data_class: &pyo3::Py<pyo3::PyAny>,
    value: &serde_json::Value,
) -> Result<std::sync::Arc<dyn crate::data::CustomDataTrait>, anyhow::Error> {
    use std::sync::Arc;

    use crate::data::PythonCustomDataWrapper;

    pyo3::Python::attach(|py| {
        let json_str = serde_json::to_string(&value)?;
        let json_module = py
            .import("json")
            .map_err(|e| anyhow::anyhow!("Failed to import json: {e}"))?;
        let py_dict = json_module
            .call_method1("loads", (json_str,))
            .map_err(|e| anyhow::anyhow!("Failed to parse JSON: {e}"))?;

        let instance = data_class
            .bind(py)
            .call_method1("from_json", (py_dict,))
            .map_err(|e| anyhow::anyhow!("Failed to call from_json: {e}"))?;

        let wrapper = PythonCustomDataWrapper::new(py, &instance)
            .map_err(|e| anyhow::anyhow!("Failed to create wrapper: {e}"))?;

        Ok(Arc::new(wrapper) as Arc<dyn crate::data::CustomDataTrait>)
    })
}

/// Encodes `CustomData` items to `RecordBatch` via Python `encode_record_batch_py`.
#[allow(unsafe_code)]
#[cfg(all(feature = "python", feature = "arrow"))]
fn py_encode_custom_data_to_record_batch(
    items: &[std::sync::Arc<dyn crate::data::CustomDataTrait>],
) -> Result<arrow::record_batch::RecordBatch, anyhow::Error> {
    pyo3::Python::attach(|py| {
        let py_items: Result<Vec<_>, _> = items.iter().map(|item| item.to_pyobject(py)).collect();
        let py_items = py_items.map_err(|e| anyhow::anyhow!("Failed to convert to Python: {e}"))?;
        let py_list = pyo3::types::PyList::new(py, &py_items)
            .map_err(|e| anyhow::anyhow!("Failed to create list: {e}"))?;

        let first = items
            .first()
            .ok_or_else(|| anyhow::anyhow!("No items to encode"))?;
        let first_py = first.to_pyobject(py)?;

        if first_py
            .bind(py)
            .hasattr("encode_record_batch_py")
            .unwrap_or(false)
        {
            let py_batch = first_py
                .bind(py)
                .call_method1("encode_record_batch_py", (py_list,))
                .map_err(|e| anyhow::anyhow!("Failed to call encode_record_batch_py: {e}"))?;

            let mut ffi_array = arrow::ffi::FFI_ArrowArray::empty();
            let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::empty();

            py_batch.call_method1(
                "_export_to_c",
                (
                    (&raw mut ffi_array as usize),
                    (&raw mut ffi_schema as usize),
                ),
            )?;

            let schema = std::sync::Arc::new(arrow::datatypes::Schema::try_from(&ffi_schema)?);
            let struct_array_data = unsafe {
                arrow::ffi::from_ffi_and_data_type(
                    ffi_array,
                    arrow::datatypes::DataType::Struct(schema.fields().clone()),
                )?
            };
            let struct_array = arrow::array::StructArray::from(struct_array_data);
            Ok(arrow::record_batch::RecordBatch::from(&struct_array))
        } else {
            anyhow::bail!("Instances must have encode_record_batch_py method")
        }
    })
}

#[cfg(all(feature = "python", feature = "arrow"))]
fn pyarrow_schema_to_arrow_schema(
    py_schema: &pyo3::Bound<'_, pyo3::PyAny>,
) -> PyResult<arrow::datatypes::Schema> {
    let mut ffi_schema = arrow::ffi::FFI_ArrowSchema::empty();
    py_schema.call_method1("_export_to_c", ((&raw mut ffi_schema as usize),))?;
    arrow::datatypes::Schema::try_from(&ffi_schema)
        .map_err(|e| to_pyvalue_err(format!("Failed to import PyArrow schema: {e}")))
}

/// Decodes `RecordBatch` to `CustomData` via Python `decode_record_batch_py`.
#[allow(unsafe_code)]
#[cfg(all(feature = "python", feature = "arrow"))]
fn py_decode_record_batch_to_custom_data(
    data_class: &pyo3::Py<pyo3::PyAny>,
    metadata: &std::collections::HashMap<String, String>,
    batch: arrow::record_batch::RecordBatch,
) -> Result<Vec<crate::data::Data>, anyhow::Error> {
    use std::sync::Arc;

    use crate::data::PythonCustomDataWrapper;

    pyo3::Python::attach(|py| {
        let struct_array: arrow::array::StructArray = batch.into();
        let array_data = arrow::array::Array::to_data(&struct_array);
        let mut ffi_array = arrow::ffi::FFI_ArrowArray::new(&array_data);
        let fields = match arrow::array::Array::data_type(&struct_array) {
            arrow::datatypes::DataType::Struct(f) => f.clone(),
            _ => unreachable!(),
        };
        let mut ffi_schema =
            arrow::ffi::FFI_ArrowSchema::try_from(arrow::datatypes::DataType::Struct(fields))?;

        let pyarrow = py.import("pyarrow")?;
        let cls = pyarrow.getattr("RecordBatch")?;
        let py_batch = cls.call_method1(
            "_import_from_c",
            (
                (&raw mut ffi_array as usize),
                (&raw mut ffi_schema as usize),
            ),
        )?;

        let metadata_py = pyo3::types::PyDict::new(py);
        for (k, v) in metadata {
            metadata_py.set_item(k, v)?;
        }

        let py_list = data_class
            .bind(py)
            .call_method1("decode_record_batch_py", (metadata_py, py_batch))
            .map_err(|e| anyhow::anyhow!("Failed to call decode_record_batch_py: {e}"))?;

        let list = py_list
            .cast::<pyo3::types::PyList>()
            .map_err(|_| anyhow::anyhow!("Expected list from decode_record_batch_py"))?;

        let mut result = Vec::new();
        for item in list.iter() {
            let wrapper = PythonCustomDataWrapper::new(py, &item)
                .map_err(|e| anyhow::anyhow!("Failed to create wrapper: {e}"))?;
            result.push(crate::data::Data::Custom(
                crate::data::CustomData::from_arc(Arc::new(wrapper)),
            ));
        }
        Ok(result)
    })
}

/// Registers a custom data **type** (class) with the catalog registry.
///
/// Use this when you prefer to pass the class instead of a sample instance.
/// The class must have:
/// - `type_name_static()` class method or `__name__` (used as type name in storage)
/// - `decode_record_batch_py(metadata, ipc_bytes)` class method
/// - Instances must have `ts_event`, `ts_init`, and `encode_record_batch_py(items)`.
///
/// # Arguments
///
/// - `data_class` - The custom data class (e.g. `MarketTickPython` or `module.MarketTickData`)
///
/// # Errors
///
/// Returns a `PyErr` if the class lacks required methods or the type is already registered.
///
/// # Example
///
/// ```python
/// from nautilus_trader.model.custom import customdataclass
/// from nautilus_trader.model import register_custom_data_class
///
/// @customdataclass()
/// class MarketTickPython:
///     symbol: str = ""
///     price: float = 0.0
///     volume: int = 0
///
/// register_custom_data_class(MarketTickPython)
/// ```
#[cfg(feature = "python")]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
pub fn register_custom_data_class(data_class: &Bound<'_, PyAny>) -> PyResult<()> {
    use std::sync::Arc;

    use crate::data::registry;

    let _py = data_class.py();

    let type_name: String = if data_class.hasattr("type_name_static")? {
        data_class.call_method0("type_name_static")?.extract()?
    } else {
        data_class.getattr("__name__")?.extract()?
    };

    #[cfg(feature = "arrow")]
    if !data_class.hasattr("decode_record_batch_py")? {
        return Err(to_pytype_err(
            "Custom data class must have decode_record_batch_py(metadata, batch) class method",
        ));
    }

    if !data_class.hasattr("from_json")? {
        return Err(to_pytype_err(
            "Custom data class must have from_json(data) class method (Rust macro provides it)",
        ));
    }

    register_python_data_class(&type_name, data_class);

    if let Some(extractor) = registry::get_rust_extractor(&type_name) {
        let _ = registry::ensure_py_extractor_registered(&type_name, extractor);
    }

    let data_class_for_json = data_class.clone().unbind();

    let json_deserializer = Box::new(
        move |value: serde_json::Value| -> Result<Arc<dyn crate::data::CustomDataTrait>, anyhow::Error> {
            pyo3::Python::attach(|py| {
                py_json_deserialize_custom_data(&data_class_for_json.clone_ref(py), &value)
            })
        },
    );

    registry::ensure_json_deserializer_registered(&type_name, json_deserializer).map_err(|e| {
        to_pyruntime_err(format!(
            "Failed to register JSON deserializer for {type_name}: {e}"
        ))
    })?;

    #[cfg(feature = "arrow")]
    {
        let data_class_for_decode = data_class.clone().unbind();
        let pyarrow_schema = if let Ok(schema) = data_class.getattr("_schema") {
            Some(schema)
        } else if data_class.hasattr("arrow_schema_py")? {
            Some(data_class.call_method0("arrow_schema_py")?)
        } else {
            None
        }
        .filter(|schema| schema.hasattr("_export_to_c").unwrap_or(false));
        let schema = if let Some(py_schema) = pyarrow_schema {
            Arc::new(pyarrow_schema_to_arrow_schema(&py_schema)?)
        } else if let Some(schema) = registry::get_arrow_schema(&type_name) {
            schema
        } else {
            Arc::new(arrow::datatypes::Schema::empty())
        };
        registry::validate_custom_arrow_schema(&type_name, &schema, false)
            .map_err(|e| to_pyvalue_err(e.to_string()))?;

        let encoder = Box::new(
            move |items: &[Arc<dyn crate::data::CustomDataTrait>]| -> Result<
                arrow::record_batch::RecordBatch,
                anyhow::Error,
            > { py_encode_custom_data_to_record_batch(items) },
        );

        let decoder = Box::new(
            move |metadata: &std::collections::HashMap<String, String>,
                  batch: arrow::record_batch::RecordBatch|
                  -> Result<Vec<crate::data::Data>, anyhow::Error> {
                pyo3::Python::attach(|py| {
                    py_decode_record_batch_to_custom_data(
                        &data_class_for_decode.clone_ref(py),
                        metadata,
                        batch,
                    )
                })
            },
        );

        registry::ensure_arrow_registered(&type_name, schema, encoder, decoder).map_err(|e| {
            to_pyruntime_err(format!(
                "Failed to register Arrow encoder/decoder for {type_name}: {e}"
            ))
        })?;
    }

    Ok(())
}

/// Transforms the given Python objects into a vector of [`FundingRateUpdate`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_funding_rates(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<FundingRateUpdate>> {
    let funding_rates: Vec<FundingRateUpdate> = data
        .into_iter()
        .map(|obj| obj.extract::<FundingRateUpdate>().map_err(PyErr::from))
        .collect::<PyResult<Vec<FundingRateUpdate>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&funding_rates) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(funding_rates)
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use rstest::rstest;

    use super::*;
    use crate::data::{
        OrderBookDeltas, OrderBookDepth,
        stubs::{
            quote_audusd, stub_bar, stub_delta, stub_deltas, stub_depth10, stub_trade_ethusdt_buy,
        },
    };

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(Python::initialize);
    }

    #[rstest]
    fn data_to_pyobject_preserves_built_in_data() {
        ensure_python_initialized();

        let expected_delta = stub_delta();
        let expected_deltas = stub_deltas();
        let expected_depth = stub_depth10();
        let expected_quote = quote_audusd();
        let expected_trade = stub_trade_ethusdt_buy();
        let expected_bar = stub_bar();

        Python::attach(|py| {
            let py_delta = data_to_pyobject(py, Data::BookDelta(expected_delta)).unwrap();
            let py_deltas =
                data_to_pyobject(py, Data::BookDeltas(Box::new(expected_deltas.clone()))).unwrap();
            let py_depth =
                data_to_pyobject(py, Data::BookDepth(Box::new(expected_depth.clone()))).unwrap();
            let py_quote = data_to_pyobject(py, Data::Quote(expected_quote)).unwrap();
            let py_trade = data_to_pyobject(py, Data::Trade(expected_trade)).unwrap();
            let py_bar = data_to_pyobject(py, Data::Bar(expected_bar)).unwrap();

            let actual_delta = *py_delta.bind(py).cast::<OrderBookDelta>().unwrap().borrow();
            let actual_deltas = py_deltas
                .bind(py)
                .cast::<OrderBookDeltas>()
                .unwrap()
                .borrow()
                .clone();
            let actual_depth = py_depth
                .bind(py)
                .cast::<OrderBookDepth>()
                .unwrap()
                .borrow()
                .clone();
            let actual_quote = *py_quote.bind(py).cast::<QuoteTick>().unwrap().borrow();
            let actual_trade = *py_trade.bind(py).cast::<TradeTick>().unwrap().borrow();
            let actual_bar = *py_bar.bind(py).cast::<Bar>().unwrap().borrow();

            assert_eq!(actual_delta, expected_delta);
            assert_eq!(actual_deltas.instrument_id, expected_deltas.instrument_id);
            assert_eq!(actual_deltas.deltas, expected_deltas.deltas);
            assert_eq!(actual_deltas.flags, expected_deltas.flags);
            assert_eq!(actual_deltas.sequence, expected_deltas.sequence);
            assert_eq!(actual_deltas.ts_event, expected_deltas.ts_event);
            assert_eq!(actual_deltas.ts_init, expected_deltas.ts_init);
            assert_eq!(actual_depth, expected_depth);
            assert_eq!(actual_quote, expected_quote);
            assert_eq!(actual_trade, expected_trade);
            assert_eq!(actual_bar, expected_bar);
        });
    }

    #[cfg(feature = "defi")]
    #[rstest]
    fn data_to_pyobject_rejects_defi_variant() {
        use nautilus_core::UnixNanos;
        use ustr::Ustr;

        use crate::defi::{Blockchain, data::Block};

        ensure_python_initialized();

        let block = Block::new(
            "0x1234".to_string(),
            "0xabcd".to_string(),
            42,
            Ustr::from("0x0000000000000000000000000000000000000000"),
            100_000,
            50_000,
            UnixNanos::from(1_700_000_000u64),
            Some(Blockchain::Ethereum),
        );

        Python::attach(|py| {
            let e = data_to_pyobject(
                py,
                Data::Defi(Box::new(crate::defi::data::DefiData::Block(block))),
            )
            .unwrap_err();

            assert_eq!(e.to_string(), "TypeError: Unsupported DeFi data variant");
        });
    }
}
