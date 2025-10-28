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

//! Data types for the trading domain model.

pub mod bar;
pub mod bet;
pub mod close;
pub mod delta;
pub mod deltas;
pub mod depth;
pub mod funding;
pub mod greeks;
pub mod order;
pub mod prices;
pub mod quote;
pub mod status;
pub mod trade;

use indexmap::IndexMap;
#[cfg(feature = "ffi")]
use nautilus_core::ffi::cvec::CVec;
use pyo3::{exceptions::PyValueError, prelude::*, types::PyCapsule};

use crate::data::{
    Bar, Data, DataType, FundingRateUpdate, IndexPriceUpdate, MarkPriceUpdate, OrderBookDelta,
    QuoteTick, TradeTick, close::InstrumentClose, is_monotonically_increasing_by_init,
};

const ERROR_MONOTONICITY: &str = "`data` was not monotonically increasing by the `ts_init` field";

#[pymethods]
#[cfg_attr(feature = "python", pyo3_stub_gen::derive::gen_stub_pymethods)]
impl DataType {
    #[new]
    #[pyo3(signature = (type_name, metadata=None))]
    fn py_new(type_name: &str, metadata: Option<IndexMap<String, String>>) -> Self {
        Self::new(type_name, metadata)
    }

    #[getter]
    #[pyo3(name = "type_name")]
    fn py_type_name(&self) -> &str {
        self.type_name()
    }

    #[getter]
    #[pyo3(name = "metadata")]
    fn py_metadata(&self) -> Option<IndexMap<String, String>> {
        self.metadata().cloned()
    }

    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&self) -> &str {
        self.topic()
    }
}

/// Creates a Python `PyCapsule` object containing a Rust `Data` instance.
///
/// This function takes ownership of the `Data` instance and encapsulates it within
/// a `PyCapsule` object, allowing the Rust data to be passed into the Python runtime.
///
/// # Panics
///
/// This function panics if the `PyCapsule` creation fails, which may occur if
/// there are issues with memory allocation or if the `Data` instance cannot be
/// properly encapsulated.
///
/// # Safety
///
/// This function is safe as long as the `Data` instance does not violate Rust's
/// safety guarantees (e.g., no invalid memory access). Users of the
/// `PyCapsule` in Python must ensure they understand how to extract and use the
/// encapsulated `Data` safely, especially when converting the capsule back to a
/// Rust data structure.
#[must_use]
pub fn data_to_pycapsule(py: Python, data: Data) -> Py<PyAny> {
    // Register a destructor which simply drops the `Data` value once the
    // capsule is released by Python.
    let capsule = PyCapsule::new_with_destructor(py, data, None, |_, _| {})
        .expect("Error creating `PyCapsule`");
    capsule.into_any().unbind()
}

/// Drops a `PyCapsule` containing a `CVec` structure.
///
/// This function safely extracts and drops the `CVec` instance encapsulated within
/// a `PyCapsule` object. It is intended for cleaning up after the `Data` instances
/// have been transferred into Python and are no longer needed.
///
/// # Panics
///
/// Panics if the capsule cannot be downcast to a `PyCapsule`, indicating a type
/// mismatch or improper capsule handling.
///
/// # Safety
///
/// This function is unsafe as it involves raw pointer dereferencing and manual memory
/// management. The caller must ensure the `PyCapsule` contains a valid `CVec` pointer.
/// Incorrect usage can lead to memory corruption or undefined behavior.
#[cfg(feature = "ffi")]
#[pyfunction]
#[allow(unsafe_code)]
pub fn drop_cvec_pycapsule(capsule: &Bound<'_, PyAny>) {
    let capsule: &Bound<'_, PyCapsule> = capsule
        .downcast::<PyCapsule>()
        .expect("Error on downcast to `&PyCapsule`");
    let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
    let data: Vec<Data> =
        unsafe { Vec::from_raw_parts(cvec.ptr.cast::<Data>(), cvec.len, cvec.cap) };
    drop(data);
}

#[cfg(not(feature = "ffi"))]
#[pyfunction]
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(index_prices)
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(closes)
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
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(funding_rates)
}
