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

#[macro_export]
macro_rules! identifier_for_python {
    ($ty:ty) => {
        #[pymethods]
        impl $ty {
            #[new]
            fn py_new(value: &str) -> PyResult<Self> {
                match <$ty>::new(value) {
                    Ok(instance) => Ok(instance),
                    Err(e) => Err(to_pyvalue_err(e)),
                }
            }

            fn __setstate__(&mut self, py: Python, state: PyObject) -> PyResult<()> {
                let value: (&PyString,) = state.extract(py)?;
                let value_str: String = value.0.extract()?;
                self.value = Ustr::from_str(&value_str).map_err(to_pyvalue_err)?;
                Ok(())
            }

            fn __getstate__(&self, py: Python) -> PyResult<PyObject> {
                Ok((self.value.to_string(),).to_object(py))
            }

            fn __reduce__(&self, py: Python) -> PyResult<PyObject> {
                let safe_constructor = py.get_type::<Self>().getattr("_safe_constructor")?;
                let state = self.__getstate__(py)?;
                Ok((safe_constructor, PyTuple::empty(py), state).to_object(py))
            }

            #[staticmethod]
            fn _safe_constructor() -> PyResult<Self> {
                Ok(<$ty>::from_str("NULL").unwrap()) // Safe default
            }

            fn __richcmp__(&self, other: &Self, op: CompareOp, py: Python<'_>) -> Py<PyAny> {
                match op {
                    CompareOp::Eq => self.eq(other).into_py(py),
                    CompareOp::Ne => self.ne(other).into_py(py),
                    CompareOp::Ge => self.ge(other).into_py(py),
                    CompareOp::Gt => self.gt(other).into_py(py),
                    CompareOp::Le => self.le(other).into_py(py),
                    CompareOp::Lt => self.lt(other).into_py(py),
                }
            }

            fn __hash__(&self) -> isize {
                self.value.precomputed_hash() as isize
            }

            fn __str__(&self) -> &'static str {
                self.value.as_str()
            }

            fn __repr__(&self) -> String {
                format!(
                    "{}('{}')",
                    stringify!($ty).split("::").last().unwrap_or(""),
                    self.value
                )
            }

            #[getter]
            #[pyo3(name = "value")]
            fn py_value(&self) -> String {
                self.value.to_string()
            }
        }
    };
}
