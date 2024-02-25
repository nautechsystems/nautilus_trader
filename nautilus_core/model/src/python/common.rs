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

use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyList},
};
use serde_json::Value;
use strum::IntoEnumIterator;

pub const PY_MODULE_MODEL: &str = "nautilus_trader.core.nautilus_pyo3.model";

/// Python iterator over the variants of an enum.
#[pyclass]
pub struct EnumIterator {
    // Type erasure for code reuse. Generic types can't be exposed to Python.
    iter: Box<dyn Iterator<Item = PyObject> + Send>,
}

#[pymethods]
impl EnumIterator {
    fn __iter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    fn __next__(mut slf: PyRefMut<'_, Self>) -> Option<PyObject> {
        slf.iter.next()
    }
}

impl EnumIterator {
    #[must_use]
    pub fn new<E>(py: Python<'_>) -> Self
    where
        E: strum::IntoEnumIterator + IntoPy<Py<PyAny>>,
        <E as IntoEnumIterator>::Iterator: Send,
    {
        Self {
            iter: Box::new(
                E::iter()
                    .map(|var| var.into_py(py))
                    // Force eager evaluation because `py` isn't `Send`
                    .collect::<Vec<_>>()
                    .into_iter(),
            ),
        }
    }
}

pub fn value_to_pydict(py: Python<'_>, val: &Value) -> PyResult<Py<PyDict>> {
    let dict = PyDict::new(py);

    match val {
        Value::Object(map) => {
            for (key, value) in map {
                let py_value = value_to_pyobject(py, value)?;
                dict.set_item(key, py_value)?;
            }
        }
        // This shouldn't be reached in this function, but we include it for completeness
        _ => return Err(PyValueError::new_err("Expected JSON object")),
    }

    Ok(dict.into_py(py))
}

pub fn value_to_pyobject(py: Python<'_>, val: &Value) -> PyResult<PyObject> {
    match val {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok(b.into_py(py)),
        Value::String(s) => Ok(s.into_py(py)),
        Value::Number(n) => {
            if n.is_i64() {
                Ok(n.as_i64().unwrap().into_py(py))
            } else if n.is_f64() {
                Ok(n.as_f64().unwrap().into_py(py))
            } else {
                Err(PyValueError::new_err("Unsupported JSON number type"))
            }
        }
        Value::Array(arr) => {
            let py_list = PyList::new(py, &[] as &[PyObject]);
            for item in arr {
                let py_item = value_to_pyobject(py, item)?;
                py_list.append(py_item)?;
            }
            Ok(py_list.into())
        }
        Value::Object(_) => {
            let py_dict = value_to_pydict(py, val)?;
            Ok(py_dict.into())
        }
    }
}

#[cfg(test)]
mod tests {
    use pyo3::{
        prelude::*,
        prepare_freethreaded_python,
        types::{PyBool, PyInt, PyList, PyString},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;

    #[rstest]
    fn test_value_to_pydict() {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let json_str = r#"
        {
            "type": "OrderAccepted",
            "ts_event": 42,
            "is_reconciliation": false
        }
        "#;

            let val: Value = serde_json::from_str(json_str).unwrap();
            let py_dict_ref = value_to_pydict(py, &val).unwrap();
            let py_dict = py_dict_ref.as_ref(py);

            assert_eq!(
                py_dict
                    .get_item("type")
                    .unwrap()
                    .unwrap()
                    .downcast::<PyString>()
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "OrderAccepted"
            );
            assert_eq!(
                py_dict
                    .get_item("ts_event")
                    .unwrap()
                    .unwrap()
                    .downcast::<PyInt>()
                    .unwrap()
                    .extract::<i64>()
                    .unwrap(),
                42
            );
            assert!(!py_dict
                .get_item("is_reconciliation")
                .unwrap()
                .unwrap()
                .downcast::<PyBool>()
                .unwrap()
                .is_true());
        });
    }

    #[rstest]
    fn test_value_to_pyobject_string() {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let val = Value::String("Hello, world!".to_string());
            let py_obj = value_to_pyobject(py, &val).unwrap();

            assert_eq!(py_obj.extract::<&str>(py).unwrap(), "Hello, world!");
        });
    }

    #[rstest]
    fn test_value_to_pyobject_bool() {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let val = Value::Bool(true);
            let py_obj = value_to_pyobject(py, &val).unwrap();

            assert!(py_obj.extract::<bool>(py).unwrap());
        });
    }

    #[rstest]
    fn test_value_to_pyobject_array() {
        prepare_freethreaded_python();
        Python::with_gil(|py| {
            let val = Value::Array(vec![
                Value::String("item1".to_string()),
                Value::String("item2".to_string()),
            ]);
            let binding = value_to_pyobject(py, &val).unwrap();
            let py_list = binding.downcast::<PyList>(py).unwrap();

            assert_eq!(py_list.len(), 2);
            assert_eq!(
                py_list.get_item(0).unwrap().extract::<&str>().unwrap(),
                "item1"
            );
            assert_eq!(
                py_list.get_item(1).unwrap().extract::<&str>().unwrap(),
                "item2"
            );
        });
    }
}
