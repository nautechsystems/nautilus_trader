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
    match dict.get_item(key)? {
        Some(value) => {
            if value.is_none() {
                Ok(None)
            } else {
                let value_str: String = value.extract()?;
                parser(value_str)
                    .map(Some)
                    .map_err(|e| to_pyvalue_err(format!("Failed to parse '{key}': {e}")))
            }
        }
        None => Ok(None),
    }
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
        .downcast_into()
        .map_err(Into::into)
}
