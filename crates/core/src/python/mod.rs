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

//! Python bindings and interoperability helpers built on top of
//! [`PyO3`](https://pyo3.rs).
//!
//! This sub-module groups together the Rust code that is *only* required when compiling the
//! `python` feature flag. It provides thin adapters so that NautilusTrader functionality can be
//! consumed from the `nautilus_trader` Python package without sacrificing type-safety or
//! performance.

pub mod casing;
pub mod datetime;
pub mod enums;
pub mod parsing;
pub mod serialization;
pub mod uuid;
pub mod version;

use pyo3::{
    conversion::IntoPyObjectExt,
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    types::PyString,
    wrap_pyfunction,
};

use crate::{
    UUID4,
    consts::{NAUTILUS_USER_AGENT, NAUTILUS_VERSION},
    datetime::{
        MILLISECONDS_IN_SECOND, NANOSECONDS_IN_MICROSECOND, NANOSECONDS_IN_MILLISECOND,
        NANOSECONDS_IN_SECOND,
    },
};

/// Extend `IntoPyObjectExt` helper trait to unwrap `PyObject` after conversion.
pub trait IntoPyObjectNautilusExt<'py>: IntoPyObjectExt<'py> {
    /// Convert `self` into a [`PyObject`] while *panicking* if the conversion fails.
    ///
    /// This is a convenience wrapper around [`IntoPyObjectExt::into_py_any`] that avoids the
    /// cumbersome `Result` handling when we are certain that the conversion cannot fail (for
    /// instance when we are converting primitives or other types that already implement the
    /// necessary PyO3 traits).
    #[inline]
    fn into_py_any_unwrap(self, py: Python<'py>) -> PyObject {
        self.into_py_any(py)
            .expect("Failed to convert type to PyObject")
    }
}

impl<'py, T> IntoPyObjectNautilusExt<'py> for T where T: IntoPyObjectExt<'py> {}

/// Gets the type name for the given Python `obj`.
///
/// # Errors
///
/// Returns a error if accessing the type name fails.
pub fn get_pytype_name<'py>(obj: &Bound<'py, PyAny>) -> PyResult<Bound<'py, PyString>> {
    obj.get_type().name()
}

/// Converts any type that implements `Display` to a Python `ValueError`.
///
/// # Errors
///
/// Returns a Python error with the error string.
pub fn to_pyvalue_err(e: impl std::fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Converts any type that implements `Display` to a Python `TypeError`.
///
/// # Errors
///
/// Returns a Python error with the error string.
pub fn to_pytype_err(e: impl std::fmt::Display) -> PyErr {
    PyTypeError::new_err(e.to_string())
}

/// Converts any type that implements `Display` to a Python `RuntimeError`.
///
/// # Errors
///
/// Returns a Python error with the error string.
pub fn to_pyruntime_err(e: impl std::fmt::Display) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

#[pyfunction]
#[allow(clippy::needless_pass_by_value)]
#[allow(unsafe_code)]
fn is_pycapsule(obj: PyObject) -> bool {
    unsafe {
        // PyCapsule_CheckExact checks if the object is exactly a PyCapsule
        pyo3::ffi::PyCapsule_CheckExact(obj.as_ptr()) != 0
    }
}

/// Loaded as `nautilus_pyo3.core`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
#[rustfmt::skip]
pub fn core(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(NAUTILUS_VERSION), NAUTILUS_VERSION)?;
    m.add(stringify!(NAUTILUS_USER_AGENT), NAUTILUS_USER_AGENT)?;
    m.add(stringify!(MILLISECONDS_IN_SECOND), MILLISECONDS_IN_SECOND)?;
    m.add(stringify!(NANOSECONDS_IN_SECOND), NANOSECONDS_IN_SECOND)?;
    m.add(stringify!(NANOSECONDS_IN_MILLISECOND), NANOSECONDS_IN_MILLISECOND)?;
    m.add(stringify!(NANOSECONDS_IN_MICROSECOND), NANOSECONDS_IN_MICROSECOND)?;
    m.add_class::<UUID4>()?;
    m.add_function(wrap_pyfunction!(is_pycapsule, m)?)?;
    m.add_function(wrap_pyfunction!(casing::py_convert_to_snake_case, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_secs_to_nanos, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_secs_to_millis, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_millis_to_nanos, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_micros_to_nanos, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_nanos_to_secs, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_nanos_to_millis, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_nanos_to_micros, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_unix_nanos_to_iso8601, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_last_weekday_nanos, m)?)?;
    m.add_function(wrap_pyfunction!(datetime::py_is_within_last_24_hours, m)?)?;
    Ok(())
}
