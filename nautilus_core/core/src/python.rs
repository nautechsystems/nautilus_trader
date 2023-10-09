// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
};

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
