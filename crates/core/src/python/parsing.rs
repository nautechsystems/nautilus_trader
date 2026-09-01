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

//! JSON / string parsing helpers for Python inputs.

use pyo3::{
    prelude::*,
    types::{PyDict, PyList},
};

use super::{to_pykey_err, to_pyvalue_err};

/// Helper function to get a required string value from a Python dictionary.
///
/// # Returns
///
/// Returns the extracted string value or a `PyErr` if the key is missing or extraction fails.
///
/// # Errors
///
/// Returns `PyErr` if the key is missing or value extraction fails.
pub fn get_required_string(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<String> {
    dict.get_item(key)?
        .ok_or_else(|| to_pykey_err(format!("Missing required key: {key}")))?
        .extract()
}

/// Helper function to get a required value from a Python dictionary and extract it.
///
/// # Returns
///
/// Returns the extracted value or a `PyErr` if the key is missing or extraction fails.
///
/// # Errors
///
/// Returns `PyErr` if the key is missing or value extraction fails.
pub fn get_required<T>(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<T>
where
    T: for<'a, 'py> FromPyObject<'a, 'py>,
    for<'a, 'py> PyErr: From<<T as FromPyObject<'a, 'py>>::Error>,
{
    dict.get_item(key)?
        .ok_or_else(|| to_pykey_err(format!("Missing required key: {key}")))?
        .extract()
        .map_err(PyErr::from)
}

/// Helper function to get an optional value from a Python dictionary.
///
/// # Returns
///
/// Returns Some(value) if the key exists and extraction succeeds, None if the key is missing
/// or if the value is Python None, or a `PyErr` if extraction fails.
///
/// # Errors
///
/// Returns `PyErr` if value extraction fails (but not if the key is missing or value is None).
#[inline]
pub fn get_optional<T>(dict: &Bound<'_, PyDict>, key: &str) -> PyResult<Option<T>>
where
    T: for<'a, 'py> FromPyObject<'a, 'py>,
    for<'a, 'py> PyErr: From<<T as FromPyObject<'a, 'py>>::Error>,
{
    match dict.get_item(key)? {
        Some(value) => {
            if value.is_none() {
                Ok(None)
            } else {
                value.extract().map(Some).map_err(PyErr::from)
            }
        }
        None => Ok(None),
    }
}

/// Helper function to get a required value, parse it with a closure, and handle parse errors.
///
/// # Returns
///
/// Returns the parsed value or a `PyErr` if the key is missing, extraction fails, or parsing fails.
///
/// # Errors
///
/// Returns `PyErr` if the key is missing, value extraction fails, or parsing fails.
pub fn get_required_parsed<T, F>(dict: &Bound<'_, PyDict>, key: &str, parser: F) -> PyResult<T>
where
    F: FnOnce(String) -> Result<T, String>,
{
    let value_str = get_required_string(dict, key)?;
    parser(value_str).map_err(|e| to_pyvalue_err(format!("Failed to parse '{key}': {e}")))
}

/// Helper function to get an optional value, parse it with a closure, and handle parse errors.
///
/// # Returns
///
/// Returns `Some(parsed_value)` if the key exists and parsing succeeds, None if the key is missing
/// or if the value is Python None, or a `PyErr` if extraction or parsing fails.
///
/// # Errors
///
/// Returns `PyErr` if value extraction or parsing fails (but not if the key is missing or value is None).
pub fn get_optional_parsed<T, F>(
    dict: &Bound<'_, PyDict>,
    key: &str,
    parser: F,
) -> PyResult<Option<T>>
where
    F: FnOnce(String) -> Result<T, String>,
{
    get_optional::<String>(dict, key)?
        .map(parser)
        .transpose()
        .map_err(|e| to_pyvalue_err(format!("Failed to parse '{key}': {e}")))
}

/// Helper function to get a required `PyList` from a Python dictionary.
///
/// # Returns
///
/// Returns the extracted `PyList` or a `PyErr` if the key is missing or extraction fails.
///
/// # Errors
///
/// Returns `PyErr` if the key is missing or value extraction fails.
pub fn get_required_list<'py>(
    dict: &Bound<'py, PyDict>,
    key: &str,
) -> PyResult<Bound<'py, PyList>> {
    dict.get_item(key)?
        .ok_or_else(|| to_pykey_err(format!("Missing required key: {key}")))?
        .cast_into()
        .map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use std::sync::Once;

    use pyo3::exceptions::{PyKeyError, PyValueError};
    use rstest::rstest;

    use super::*;

    fn ensure_python_initialized() {
        static INIT: Once = Once::new();
        INIT.call_once(Python::initialize);
    }

    #[rstest]
    fn test_get_required_string() {
        ensure_python_initialized();

        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("name", "nautilus").unwrap();

            let value = get_required_string(&dict, "name").unwrap();
            let error = get_required_string(&dict, "missing").unwrap_err();

            assert_eq!(value, "nautilus");
            assert!(error.is_instance_of::<PyKeyError>(py));
            assert_eq!(
                error.value(py).to_string(),
                "'Missing required key: missing'"
            );
        });
    }

    #[rstest]
    fn test_get_optional_parsed() {
        ensure_python_initialized();

        Python::attach(|py| {
            let dict = PyDict::new(py);
            dict.set_item("value", "42").unwrap();
            let parsed = get_optional_parsed(&dict, "value", |value| {
                value.parse::<u64>().map_err(|e| e.to_string())
            })
            .unwrap();
            let missing = get_optional_parsed(&dict, "missing", |value| {
                value.parse::<u64>().map_err(|e| e.to_string())
            })
            .unwrap();

            dict.set_item("value", py.None()).unwrap();
            let none = get_optional_parsed(&dict, "value", |value| {
                value.parse::<u64>().map_err(|e| e.to_string())
            })
            .unwrap();

            dict.set_item("value", "invalid").unwrap();
            let error =
                get_optional_parsed::<u64, _>(&dict, "value", |_| Err("not a number".to_string()))
                    .unwrap_err();

            assert_eq!(parsed, Some(42));
            assert_eq!(missing, None);
            assert_eq!(none, None);
            assert!(error.is_instance_of::<PyValueError>(py));
            assert_eq!(
                error.value(py).to_string(),
                "Failed to parse 'value': not a number"
            );
        });
    }
}
