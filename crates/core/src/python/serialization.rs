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

//! (De)serialization utilities bridging Rust ↔︎ Python types.

use std::{
    collections::HashMap,
    hash::{BuildHasher, Hash},
};

use indexmap::IndexMap;
use pyo3::{
    conversion::{FromPyObjectOwned, IntoPyObject},
    prelude::*,
    types::{PyAny, PyDict},
};
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
    let values = values.into_any();
    from_pyobject_pyo3(py, values.bind(py))
}

/// Convert a Python object to a Rust type that implements `DeserializeOwned`.
///
/// # Errors
///
/// Returns an error if:
/// - The Python object cannot be serialized to JSON.
/// - The JSON string cannot be deserialized to type `T`.
/// - The Python `json` module fails to import or execute.
pub fn from_pyobject_pyo3<T>(py: Python<'_>, value: &Bound<'_, PyAny>) -> Result<T, PyErr>
where
    T: DeserializeOwned,
{
    // `ensure_ascii=False` keeps non-ASCII characters as raw UTF-8 in the JSON output.
    // Without this, `\uXXXX` escapes force `serde_json` onto the owned-string path,
    // which `visit_str`-only visitors like `ustr::Ustr` reject as "expected a borrowed string".
    let kwargs = PyDict::new(py);
    kwargs.set_item("ensure_ascii", false)?;
    let json_str: String = PyModule::import(py, "json")?
        .call_method("dumps", (value,), Some(&kwargs))?
        .extract()?;

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
    let py_object = to_pyobject_pyo3(py, value)?;
    let py_dict = py_object
        .bind(py)
        .cast::<PyDict>()
        .map_err(Into::<PyErr>::into)?
        .clone()
        .unbind();
    Ok(py_dict)
}

/// Convert a Rust type that implements `Serialize` to a Python object.
///
/// # Errors
///
/// Returns an error if:
/// - The Rust value cannot be serialized to JSON.
/// - The JSON string cannot be parsed into a Python object.
/// - The Python `json` module fails to import or execute.
pub fn to_pyobject_pyo3<T>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>>
where
    T: Serialize,
{
    let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
    let py_object = PyModule::import(py, "json")?
        .call_method("loads", (json_str,), None)?
        .extract()?;
    Ok(py_object)
}

/// Convert a Python mapping to an [`IndexMap`] using typed PyO3 objects when possible.
///
/// Falls back to the generic JSON bridge when the input is not a Python dict. Individual dict
/// keys and values first try typed PyO3 extraction, then JSON/Serde extraction. This accepts both
/// `{InstrumentId(...): Price(...)}` and `{"AUD/USD.SIM": "1.23456"}` style inputs.
///
/// # Errors
///
/// Returns an error if the Python object cannot be converted to the requested map type.
pub fn indexmap_from_pyobject_pyo3<K, V>(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
) -> PyResult<IndexMap<K, V>>
where
    K: for<'py> FromPyObjectOwned<'py> + DeserializeOwned + Eq + Hash,
    V: for<'py> FromPyObjectOwned<'py> + DeserializeOwned,
    IndexMap<K, V>: DeserializeOwned,
{
    let Ok(dict) = value.cast::<PyDict>() else {
        return from_pyobject_pyo3(py, value);
    };

    let mut map = IndexMap::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        map.insert(
            extract_typed_or_json_pyo3(py, &key)?,
            extract_typed_or_json_pyo3(py, &value)?,
        );
    }
    Ok(map)
}

/// Convert a Python mapping to a [`HashMap`] using typed PyO3 objects when possible.
///
/// Falls back to the generic JSON bridge when the input is not a Python dict. Individual dict
/// keys and values first try typed PyO3 extraction, then JSON/Serde extraction.
///
/// # Errors
///
/// Returns an error if the Python object cannot be converted to the requested map type.
pub fn hashmap_from_pyobject_pyo3<K, V>(
    py: Python<'_>,
    value: &Bound<'_, PyAny>,
) -> PyResult<HashMap<K, V>>
where
    K: for<'py> FromPyObjectOwned<'py> + DeserializeOwned + Eq + Hash,
    V: for<'py> FromPyObjectOwned<'py> + DeserializeOwned,
    HashMap<K, V>: DeserializeOwned,
{
    let Ok(dict) = value.cast::<PyDict>() else {
        return from_pyobject_pyo3(py, value);
    };

    let mut map = HashMap::with_capacity(dict.len());
    for (key, value) in dict.iter() {
        map.insert(
            extract_typed_or_json_pyo3(py, &key)?,
            extract_typed_or_json_pyo3(py, &value)?,
        );
    }
    Ok(map)
}

/// Convert an [`IndexMap`] to a Python dict using typed PyO3 keys and values.
///
/// # Errors
///
/// Returns an error if any key or value cannot be converted to Python.
pub fn indexmap_to_pydict_pyo3<K, V>(py: Python<'_>, value: &IndexMap<K, V>) -> PyResult<Py<PyAny>>
where
    K: for<'py> IntoPyObject<'py> + Clone,
    V: for<'py> IntoPyObject<'py> + Clone,
{
    let dict = PyDict::new(py);
    for (key, value) in value {
        dict.set_item(key.clone(), value.clone())?;
    }
    Ok(dict.into_any().unbind())
}

/// Convert a [`HashMap`] to a Python dict using typed PyO3 keys and values.
///
/// # Errors
///
/// Returns an error if any key or value cannot be converted to Python.
pub fn hashmap_to_pydict_pyo3<K, V, S>(
    py: Python<'_>,
    value: &HashMap<K, V, S>,
) -> PyResult<Py<PyAny>>
where
    K: for<'py> IntoPyObject<'py> + Clone,
    V: for<'py> IntoPyObject<'py> + Clone,
    S: BuildHasher,
{
    let dict = PyDict::new(py);
    for (key, value) in value {
        dict.set_item(key.clone(), value.clone())?;
    }
    Ok(dict.into_any().unbind())
}

fn extract_typed_or_json_pyo3<T>(py: Python<'_>, value: &Bound<'_, PyAny>) -> PyResult<T>
where
    T: for<'py> FromPyObjectOwned<'py> + DeserializeOwned,
{
    value.extract::<T>().or_else(|typed_err| {
        let typed_err: PyErr = typed_err.into();
        from_pyobject_pyo3(py, value).map_err(|json_err| {
            to_pyvalue_err(format!(
                "typed extraction failed: {typed_err}; JSON extraction failed: {json_err}"
            ))
        })
    })
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use pyo3::types::PyDict;
    use rstest::rstest;
    use serde::{Deserialize, Serialize};

    use super::*;

    #[derive(Debug, PartialEq, Serialize, Deserialize)]
    struct Payload {
        values: HashMap<String, String>,
    }

    #[rstest]
    fn test_from_pyobject_pyo3_preserves_non_ascii_strings() {
        Python::initialize();
        Python::attach(|py| {
            let values = PyDict::new(py);
            values.set_item("clé", "café").unwrap();

            let dict = PyDict::new(py);
            dict.set_item("values", values).unwrap();

            let payload: Payload = from_pyobject_pyo3(py, dict.as_any()).unwrap();
            assert_eq!(payload.values.get("clé").unwrap(), "café");
        });
    }
}
