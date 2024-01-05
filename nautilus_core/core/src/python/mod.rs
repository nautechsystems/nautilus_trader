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

use std::fmt;

use pyo3::{
    exceptions::{PyRuntimeError, PyTypeError, PyValueError},
    prelude::*,
    wrap_pyfunction,
};

use crate::uuid::UUID4;
pub mod casing;
pub mod datetime;
pub mod serialization;
pub mod uuid;

/// Gets the type name for the given Python `obj`.
pub fn get_pytype_name<'p>(obj: &'p PyObject, py: Python<'p>) -> PyResult<&'p str> {
    obj.as_ref(py).get_type().name()
}

/// Converts any type that implements `Display` to a Python `ValueError`.
pub fn to_pyvalue_err(e: impl fmt::Display) -> PyErr {
    PyValueError::new_err(e.to_string())
}

/// Converts any type that implements `Display` to a Python `TypeError`.
pub fn to_pytype_err(e: impl fmt::Display) -> PyErr {
    PyTypeError::new_err(e.to_string())
}

/// Converts any type that implements `Display` to a Python `RuntimeError`.
pub fn to_pyruntime_err(e: impl fmt::Display) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Loaded as nautilus_pyo3.core
#[pymodule]
pub fn core(_: Python<'_>, m: &PyModule) -> PyResult<()> {
    m.add_class::<UUID4>()?;
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
