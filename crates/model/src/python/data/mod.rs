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

pub mod bar;
pub mod bet;
pub mod close;
#[cfg(feature = "python")]
pub mod custom;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod forward;
pub mod funding;
pub mod greeks;
pub mod option_chain;
pub mod order;
pub mod prices;
pub mod quote;
pub mod status;
pub mod trade;

#[cfg(feature = "ffi")]
use nautilus_core::ffi::cvec::CVec;
#[cfg(feature = "python")]
use nautilus_core::python::{
    params::{params_to_pydict, pydict_to_params},
    to_pyruntime_err, to_pytype_err, to_pyvalue_err,
};
#[cfg(feature = "python")]
use pyo3::types::PyDict;
use pyo3::{prelude::*, types::PyCapsule};

#[cfg(feature = "cython-compat")]
use crate::data::DataFFI;
use crate::data::{
    Bar, CustomData, Data, DataType, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus,
    MarkPriceUpdate, OrderBookDelta, QuoteTick, TradeTick, close::InstrumentClose,
    is_monotonically_increasing_by_init, register_python_data_class,
};

const ERROR_MONOTONICITY: &str = "`data` was not monotonically increasing by the `ts_init` field";

#[pymethods]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pymethods)]
impl DataType {
    /// Represents a data type including metadata.
    #[new]
    #[pyo3(signature = (type_name, metadata=None, identifier=None))]
    fn py_new(
        py: Python<'_>,
        type_name: &str,
        metadata: Option<Py<PyDict>>,
        identifier: Option<String>,
    ) -> PyResult<Self> {
        let params = match metadata {
            None => None,
            Some(d) => pydict_to_params(py, d)?,
        };
        Ok(Self::new(type_name, params, identifier))
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        use nautilus_core::python::IntoPyObjectNautilusExt;

        match op {
            pyo3::pyclass::CompareOp::Eq => (self.topic() == other.topic()).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => (self.topic() != other.topic()).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        self.precomputed_hash() as isize
    }

    /// Returns the type name for the data type.
    #[getter]
    #[pyo3(name = "type_name")]
    fn py_type_name(&self) -> &str {
        self.type_name()
    }

    /// Returns the metadata for the data type.
    #[getter]
    #[pyo3(name = "metadata")]
    fn py_metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.metadata() {
            None => Ok(py.None()),
            Some(p) => Ok(params_to_pydict(py, p)?
                .bind(py)
                .clone()
                .into_any()
                .unbind()),
        }
    }

    /// Returns the messaging topic for the data type.
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&self) -> &str {
        self.topic()
    }

    /// Returns the optional catalog path identifier (can contain subdirs, e.g. `"venue//symbol"`).
    #[getter]
    #[pyo3(name = "identifier")]
    fn py_identifier(&self) -> Option<&str> {
        self.identifier()
    }
}

/// Creates a Python `PyCapsule` object containing a Rust `Data` instance.
///
/// This function takes ownership of the `Data` instance and encapsulates it within
/// a `PyCapsule` object, allowing the Rust data to be passed into the Python runtime.
///
/// # Capsule type contract
///
/// When conversion to `DataFFI` fails (e.g. for `Data::Custom`), this returns a
/// capsule containing a single `Data` value (no destructor). That capsule must
/// **never** be passed to [`drop_cvec_pycapsule`], which expects a `CVec` and
/// would cause undefined behavior. Only capsules produced by code that creates
/// `CVec` (e.g. for `capsule_to_list`) may be passed to `drop_cvec_pycapsule`.
///
/// # Panics
///
/// This function panics if the `PyCapsule` creation fails, which may occur if
/// there are issues with memory allocation or if the `Data` instance cannot be
/// properly encapsulated.
#[must_use]
pub fn data_to_pycapsule(py: Python, data: Data) -> Py<PyAny> {
    #[cfg(feature = "cython-compat")]
    {
        // For Cython compatibility, we convert to DataFFI if possible.
        if let Ok(ffi_data) = DataFFI::try_from(data.clone()) {
            let capsule = PyCapsule::new_with_destructor(py, ffi_data, None, |_, _| {})
                .expect("Error creating `PyCapsule` for `DataFFI` ");
            return capsule.into_any().unbind();
        }
    }

    // Default case for PyO3 or when conversion fails (e.g. Custom data)
    let capsule = PyCapsule::new_with_destructor(py, data, None, |_, _| {})
        .expect("Error creating `PyCapsule` for `Data` ");
    capsule.into_any().unbind()
}

/// Drops a `PyCapsule` containing a `CVec` structure.
///
/// This function safely extracts and drops the `CVec` instance encapsulated within
/// a `PyCapsule` object. It is intended for cleaning up after the `Data` instances
/// have been transferred into Python (e.g. via `capsule_to_list`) and are no longer needed.
///
/// # Capsule type contract
///
/// **Must only be called** on capsules that contain a `CVec` (pointer to `Vec<DataFFI>`).
/// Never pass a capsule from [`data_to_pycapsule`] here: when that function returns a
/// single-`Data` capsule (e.g. for `Data::Custom`), the pointer is not a `CVec`, and
/// calling this would be undefined behavior.
///
/// # Panics
///
/// Panics if the capsule cannot be downcast to a `PyCapsule`, indicating a type
/// mismatch or improper capsule handling.
///
/// This function involves raw pointer dereferencing and manual memory
/// management. The caller must ensure the `PyCapsule` contains a valid `CVec` pointer.
#[cfg(feature = "ffi")]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
#[allow(unsafe_code)]
pub fn drop_cvec_pycapsule(capsule: &Bound<'_, PyAny>) {
    let capsule: &Bound<'_, PyCapsule> = capsule
        .cast::<PyCapsule>()
        .expect("Error on downcast to `&PyCapsule`");
    let cvec: &CVec = unsafe { &*(capsule.pointer_checked(None).unwrap().as_ptr() as *const CVec) };
    let data: Vec<crate::data::DataFFI> =
        unsafe { Vec::from_raw_parts(cvec.ptr.cast::<crate::data::DataFFI>(), cvec.len, cvec.cap) };
    drop(data);
}

#[cfg(not(feature = "ffi"))]
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.model")]
/// Drops a Python `PyCapsule` containing a `CVec` when the `ffi` feature is not enabled.
///
/// # Panics
///
/// Always panics with the message "`ffi` feature is not enabled" to indicate that
/// FFI functionality is unavailable.
pub fn drop_cvec_pycapsule(_capsule: &Bound<'_, PyAny>) {
    panic!("`ffi` feature is not enabled");
}

/// Transforms the given Python objects into a vector of [`OrderBookDelta`] objects.
///
/// # Errors
///
/// Returns a `PyErr` if element conversion fails or the data is not monotonically increasing.
pub fn pyobjects_to_book_deltas(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<OrderBookDelta>> {
    let deltas: Vec<OrderBookDelta> = data
        .into_iter()
        .map(|obj| OrderBookDelta::from_pyobject(&obj))
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
        .map(|obj| QuoteTick::from_pyobject(&obj))
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
        .map(|obj| TradeTick::from_pyobject(&obj))
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
        .map(|obj| Bar::from_pyobject(&obj))
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
        .map(|obj| MarkPriceUpdate::from_pyobject(&obj))
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
        .map(|obj| IndexPriceUpdate::from_pyobject(&obj))
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
        .map(|obj| InstrumentStatus::from_pyobject(&obj))
        .collect::<PyResult<Vec<InstrumentStatus>>>()?;

    if !is_monotonically_increasing_by_init(&statuses) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(statuses)
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
        .map(|obj| InstrumentClose::from_pyobject(&obj))
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
#[cfg(feature = "python")]
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

/// Decodes `RecordBatch` to `CustomData` via Python `decode_record_batch_py`.
#[allow(unsafe_code)]
#[cfg(feature = "python")]
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
/// - Instances must have `ts_event`, `ts_init` and `encode_record_batch_py(items)`.
///
/// # Arguments
///
/// * `data_class` - The custom data class (e.g. `MarketTickPython` or `module.MarketTickData`)
///
/// # Errors
///
/// Returns a `PyErr` if the class lacks required methods or the type is already registered.
///
/// # Example
///
/// ```python
/// from nautilus_trader.model.custom import customdataclass_pyo3
/// from nautilus_trader.model import register_custom_data_class
///
/// @customdataclass_pyo3()
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

    if !data_class.hasattr("decode_record_batch_py")? {
        return Err(to_pytype_err(
            "Custom data class must have decode_record_batch_py(metadata, batch) class method",
        ));
    }

    let type_name: String = if data_class.hasattr("type_name_static")? {
        data_class.call_method0("type_name_static")?.extract()?
    } else {
        data_class.getattr("__name__")?.extract()?
    };

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
    let data_class_for_decode = data_class.clone().unbind();

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

    let schema = Arc::new(arrow::datatypes::Schema::empty());

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
        .map(|obj| FundingRateUpdate::from_pyobject(&obj))
        .collect::<PyResult<Vec<FundingRateUpdate>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&funding_rates) {
        return Err(to_pyvalue_err(ERROR_MONOTONICITY));
    }

    Ok(funding_rates)
}
