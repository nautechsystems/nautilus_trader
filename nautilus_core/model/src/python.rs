use pyo3::{
    exceptions::PyValueError,
    prelude::*,
    types::{PyDict, PyList},
};
use serde_json::Value;
use strum::IntoEnumIterator;

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
            for (key, value) in map.iter() {
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
            for item in arr.iter() {
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
