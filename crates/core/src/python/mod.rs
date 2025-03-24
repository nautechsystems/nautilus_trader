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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod casing;
pub mod datetime;
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
fn is_pycapsule(obj: PyObject) -> PyResult<bool> {
    let result = unsafe {
        // PyCapsule_CheckExact checks if the object is exactly a PyCapsule
        pyo3::ffi::PyCapsule_CheckExact(obj.as_ptr()) != 0
    };

    Ok(result)
}

/// Loaded as nautilus_pyo3.core
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
