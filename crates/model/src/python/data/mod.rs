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
pub mod delta;
pub mod deltas;
pub mod depth;
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
    Bar, Data, DataType, OrderBookDelta, QuoteTick, TradeTick, is_monotonically_increasing_by_init,
};

const ERROR_MONOTONICITY: &str = "`data` was not monotonically increasing by the `ts_init` field";

#[pymethods]
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
/// This function will panic if the `PyCapsule` creation fails, which may occur if
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
pub fn data_to_pycapsule(py: Python, data: Data) -> PyObject {
    let capsule = PyCapsule::new(py, data, None).expect("Error creating `PyCapsule`");
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
/// This function panics:
/// - If the capsule cannot be downcast to a `PyCapsule`, indicating a type mismatch
/// or improper capsule handling.
///
/// # Safety
///
/// This function is unsafe as it involves raw pointer dereferencing and manual memory
/// management. The caller must ensure the `PyCapsule` contains a valid `CVec` pointer.
/// Incorrect usage can lead to memory corruption or undefined behavior.
#[pyfunction]
#[cfg(feature = "ffi")]
pub fn drop_cvec_pycapsule(capsule: &Bound<'_, PyAny>) {
    let capsule: &Bound<'_, PyCapsule> = capsule
        .downcast::<PyCapsule>()
        .expect("Error on downcast to `&PyCapsule`");
    let cvec: &CVec = unsafe { &*(capsule.pointer() as *const CVec) };
    let data: Vec<Data> =
        unsafe { Vec::from_raw_parts(cvec.ptr.cast::<Data>(), cvec.len, cvec.cap) };
    drop(data);
}

#[pyfunction]
#[cfg(not(feature = "ffi"))]
pub fn drop_cvec_pycapsule(_capsule: &Bound<'_, PyAny>) {
    panic!("`ffi` feature is not enabled");
}

/// Transforms the given `data` Python objects into a vector of [`OrderBookDelta`] objects.
pub fn pyobjects_to_order_book_deltas(
    data: Vec<Bound<'_, PyAny>>,
) -> PyResult<Vec<OrderBookDelta>> {
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

/// Transforms the given `data` Python objects into a vector of [`QuoteTick`] objects.
pub fn pyobjects_to_quote_ticks(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<QuoteTick>> {
    let ticks: Vec<QuoteTick> = data
        .into_iter()
        .map(|obj| QuoteTick::from_pyobject(&obj))
        .collect::<PyResult<Vec<QuoteTick>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&ticks) {
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(ticks)
}

/// Transforms the given `data` Python objects into a vector of [`TradeTick`] objects.
pub fn pyobjects_to_trade_ticks(data: Vec<Bound<'_, PyAny>>) -> PyResult<Vec<TradeTick>> {
    let ticks: Vec<TradeTick> = data
        .into_iter()
        .map(|obj| TradeTick::from_pyobject(&obj))
        .collect::<PyResult<Vec<TradeTick>>>()?;

    // Validate monotonically increasing
    if !is_monotonically_increasing_by_init(&ticks) {
        return Err(PyValueError::new_err(ERROR_MONOTONICITY));
    }

    Ok(ticks)
}

/// Transforms the given `data` Python objects into a vector of [`Bar`] objects.
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
