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

use indexmap::IndexMap;
use nautilus_core::python::IntoPyObjectNautilusExt;
use pyo3::{
    conversion::{IntoPyObject, IntoPyObjectExt},
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyList, PyNone},
};
use serde_json::Value;
use strum::IntoEnumIterator;

use crate::types::{Currency, Money};

pub const PY_MODULE_MODEL: &str = "nautilus_trader.core.nautilus_pyo3.model";

/// Python iterator over the variants of an enum.
#[pyclass]
pub struct EnumIterator {
    // Type erasure for code reuse, generic types can't be exposed to Python
    iter: Box<dyn Iterator<Item = PyObject> + Send + Sync>,
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
    pub fn new<'py, E>(py: Python<'py>) -> Self
    where
        E: strum::IntoEnumIterator + IntoPyObject<'py>,
        <E as IntoEnumIterator>::Iterator: Send,
    {
        Self {
            iter: Box::new(
                E::iter()
                    .map(|var| var.into_py_any_unwrap(py))
                    // Force eager evaluation because `py` isn't `Send`
                    .collect::<Vec<_>>()
                    .into_iter(),
            ),
        }
    }
}

pub fn value_to_pydict(py: Python<'_>, val: &Value) -> PyResult<Py<PyAny>> {
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

    dict.into_py_any(py)
}

pub fn value_to_pyobject(py: Python<'_>, val: &Value) -> PyResult<PyObject> {
    match val {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => b.into_py_any(py),
        Value::String(s) => s.into_py_any(py),
        Value::Number(n) => {
            if n.is_i64() {
                n.as_i64().unwrap().into_py_any(py)
            } else if n.is_f64() {
                n.as_f64().unwrap().into_py_any(py)
            } else {
                Err(PyValueError::new_err("Unsupported JSON number type"))
            }
        }
        Value::Array(arr) => {
            let py_list = PyList::new(py, &[] as &[PyObject]).expect("Invalid `ExactSizeIterator`");
            for item in arr {
                let py_item = value_to_pyobject(py, item)?;
                py_list.append(py_item)?;
            }
            py_list.into_py_any(py)
        }
        Value::Object(_) => value_to_pydict(py, val),
    }
}

pub fn commissions_from_vec(py: Python<'_>, commissions: Vec<Money>) -> PyResult<Bound<'_, PyAny>> {
    let mut values = Vec::new();

    for value in commissions {
        values.push(value.to_string());
    }

    if values.is_empty() {
        Ok(PyNone::get(py).to_owned().into_any())
    } else {
        values.sort();
        // SAFETY: Reasonable to expect `ExactSizeIterator` should be correctly implemented
        Ok(PyList::new(py, &values).unwrap().into_any())
    }
}

pub fn commissions_from_indexmap(
    py: Python<'_>,
    commissions: IndexMap<Currency, Money>,
) -> PyResult<Bound<'_, PyAny>> {
    commissions_from_vec(py, commissions.values().cloned().collect())
}

#[cfg(test)]
mod tests {
    use pyo3::{
        prelude::*,
        prepare_freethreaded_python,
        types::{PyBool, PyInt, PyString},
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
            let py_dict = py_dict_ref.bind(py);

            assert_eq!(
                py_dict
                    .get_item("type")
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
                    .downcast::<PyInt>()
                    .unwrap()
                    .extract::<i64>()
                    .unwrap(),
                42
            );
            assert!(
                !py_dict
                    .get_item("is_reconciliation")
                    .unwrap()
                    .downcast::<PyBool>()
                    .unwrap()
                    .is_true()
            );
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
            let py_list: &Bound<'_, PyList> = binding.bind(py).downcast::<PyList>().unwrap();

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
