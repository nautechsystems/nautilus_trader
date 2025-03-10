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

use pyo3::{prelude::*, types::PyDict};
use serde::{Serialize, de::DeserializeOwned};

use crate::python::to_pyvalue_err;

/// Convert a Python dictionary to a Rust type that implements `DeserializeOwned`.
///
/// # Errors
///
/// Returns an error if:
/// - The Python dictionary cannot be serialized to JSON.
/// - The JSON string cannot be deserialized to type `T`.
/// - The Python `json` module fails to import or execute.
pub fn from_dict_pyo3<T>(py: Python<'_>, values: Py<PyDict>) -> Result<T, PyErr>
where
    T: DeserializeOwned,
{
    // Extract to JSON bytes
    use crate::python::to_pyvalue_err;
    let json_str: String = PyModule::import(py, "json")?
        .call_method("dumps", (values,), None)?
        .extract()?;

    // Deserialize to object
    let instance = serde_json::from_str(&json_str).map_err(to_pyvalue_err)?;
    Ok(instance)
}

/// Convert a Rust type that implements `Serialize` to a Python dictionary.
///
/// # Errors
///
/// Returns an error if:
/// - The Rust value cannot be serialized to JSON.
/// - The JSON string cannot be parsed into a Python dictionary.
/// - The Python `json` module fails to import or execute.
pub fn to_dict_pyo3<T>(py: Python<'_>, value: &T) -> PyResult<Py<PyDict>>
where
    T: Serialize,
{
    let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;

    // Parse JSON into a Python dictionary
    let py_dict: Py<PyDict> = PyModule::import(py, "json")?
        .call_method("loads", (json_str,), None)?
        .extract()?;
    Ok(py_dict)
}
