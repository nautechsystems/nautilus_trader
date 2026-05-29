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

//! Python bindings for [`Params`] type conversion.

use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList, PyModule},
};
use serde_json::Value;

use crate::{
    params::Params,
    python::{serialization::from_pyobject_pyo3, to_pyvalue_err},
};

/// Converts a Python dict to `Params` (IndexMap<String, Value>).
///
/// # Errors
///
/// Returns a `PyErr` if:
/// - the dict cannot be serialized to JSON
/// - the JSON is not a valid object
pub fn pydict_to_params(py: Python<'_>, dict: &Py<PyDict>) -> PyResult<Option<Params>> {
    let dict_bound = dict.bind(py);
    if dict_bound.is_empty() {
        return Ok(None);
    }

    from_pyobject_pyo3(py, dict_bound.as_any()).map(Some)
}

/// Converts a `serde_json::Value` to a Python object.
///
/// This is a common conversion pattern used when converting `Params` to Python dicts.
///
/// # Errors
///
/// Returns a `PyErr` if the value type is unsupported, numeric extraction fails,
/// or conversion fails.
pub fn value_to_pyobject(py: Python<'_>, val: &Value) -> PyResult<Py<PyAny>> {
    match val {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => b.into_py_any(py),
        Value::String(s) => s.into_py_any(py),
        Value::Number(n) => {
            if n.is_i64() {
                n.as_i64()
                    .ok_or_else(|| to_pyvalue_err("JSON number could not be read as i64"))?
                    .into_py_any(py)
            } else if n.is_u64() {
                n.as_u64()
                    .ok_or_else(|| to_pyvalue_err("JSON number could not be read as u64"))?
                    .into_py_any(py)
            } else if n.is_f64() {
                n.as_f64()
                    .ok_or_else(|| to_pyvalue_err("JSON number could not be read as f64"))?
                    .into_py_any(py)
            } else {
                Err(to_pyvalue_err("Unsupported JSON number type"))
            }
        }
        Value::Array(arr) => {
            let py_list = PyList::new(py, &[] as &[Py<PyAny>])?;
            for item in arr {
                let py_item = value_to_pyobject(py, item)?;
                py_list.append(py_item)?;
            }
            py_list.into_py_any(py)
        }
        Value::Object(_) => {
            // For nested objects, convert to dict recursively
            let json_str = serde_json::to_string(val).map_err(to_pyvalue_err)?;
            let py_dict: Py<PyDict> = PyModule::import(py, "json")?
                .call_method("loads", (json_str,), None)?
                .extract()?;
            py_dict.into_py_any(py)
        }
    }
}

/// Converts `Params` (IndexMap<String, Value>) to a Python dict.
///
/// # Errors
///
/// Returns a `PyErr` if conversion of any value fails.
pub fn params_to_pydict(py: Python<'_>, params: &Params) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);
    for (key, value) in params {
        let py_value = value_to_pyobject(py, value)?;
        dict.set_item(key, py_value)?;
    }
    Ok(dict.into())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[derive(Debug, Clone, Copy)]
    enum ExpectedNumber {
        I64(i64),
        U64(u64),
        F64(f64),
    }

    #[rstest]
    #[case(json!(-100_i64), ExpectedNumber::I64(-100))]
    #[case(json!(42_u64), ExpectedNumber::U64(42))]
    #[case(json!(2.5_f64), ExpectedNumber::F64(2.5))]
    fn test_value_to_pyobject_number_branches(
        #[case] value: Value,
        #[case] expected: ExpectedNumber,
    ) {
        Python::initialize();
        Python::attach(|py| {
            let py_obj = value_to_pyobject(py, &value).unwrap();

            match expected {
                ExpectedNumber::I64(expected) => {
                    assert_eq!(py_obj.extract::<i64>(py).unwrap(), expected);
                }
                ExpectedNumber::U64(expected) => {
                    assert_eq!(py_obj.extract::<u64>(py).unwrap(), expected);
                }
                ExpectedNumber::F64(expected) => {
                    let actual = py_obj.extract::<f64>(py).unwrap();
                    assert!((actual - expected).abs() < f64::EPSILON);
                }
            }
        });
    }
}
