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

use nautilus_core::python::{
    IntoPyObjectNautilusExt,
    params::{params_to_pydict, pydict_to_params},
    to_pyvalue_err,
};
use pyo3::{prelude::*, types::PyDict};

use crate::data::DataType;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DataType {
    /// Represents a data type including metadata.
    #[new]
    #[pyo3(signature = (type_name, metadata=None, identifier=None))]
    fn py_new(
        py: Python<'_>,
        type_name: &str,
        metadata: Option<Py<PyDict>>,
        identifier: Option<String>,
    ) -> PyResult<Self> {
        let params = match metadata {
            None => None,
            Some(d) => pydict_to_params(py, &d)?,
        };
        Self::try_new(type_name, params, identifier).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    fn from_str(s: &str) -> PyResult<Self> {
        s.parse::<Self>().map_err(to_pyvalue_err)
    }

    fn __richcmp__(&self, other: &Self, op: pyo3::pyclass::CompareOp, py: Python<'_>) -> Py<PyAny> {
        match op {
            pyo3::pyclass::CompareOp::Eq => (self.topic() == other.topic()).into_py_any_unwrap(py),
            pyo3::pyclass::CompareOp::Ne => (self.topic() != other.topic()).into_py_any_unwrap(py),
            _ => py.NotImplemented(),
        }
    }

    fn __hash__(&self) -> isize {
        self.precomputed_hash() as isize
    }

    /// Returns the type name for the data type.
    #[getter]
    #[pyo3(name = "type_name")]
    fn py_type_name(&self) -> &str {
        self.type_name()
    }

    /// Returns the metadata for the data type.
    #[getter]
    #[pyo3(name = "metadata")]
    fn py_metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        match self.metadata() {
            None => Ok(py.None()),
            Some(p) => Ok(params_to_pydict(py, p)?
                .bind(py)
                .clone()
                .into_any()
                .unbind()),
        }
    }

    /// Returns the messaging topic for the data type.
    #[getter]
    #[pyo3(name = "topic")]
    fn py_topic(&self) -> &str {
        self.topic()
    }

    /// Returns the optional catalog path identifier.
    #[getter]
    #[pyo3(name = "identifier")]
    fn py_identifier(&self) -> Option<&str> {
        self.identifier()
    }
}
