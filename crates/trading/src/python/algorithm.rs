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

//! Python bindings for execution algorithm configuration.

use std::collections::HashMap;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{prelude::*, types::PyDict};

use crate::algorithm::ImportableExecAlgorithmConfig;

#[pyo3::pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl ImportableExecAlgorithmConfig {
    /// Configuration for creating execution algorithms from importable paths.
    #[new]
    #[expect(clippy::needless_pass_by_value)]
    fn py_new(
        exec_algorithm_path: String,
        config_path: String,
        config: Py<PyDict>,
    ) -> PyResult<Self> {
        let json_config = Python::attach(|py| -> PyResult<HashMap<String, serde_json::Value>> {
            let kwargs = PyDict::new(py);
            kwargs.set_item("default", py.eval(pyo3::ffi::c_str!("str"), None, None)?)?;
            let json_str: String = PyModule::import(py, "json")?
                .call_method("dumps", (config.bind(py),), Some(&kwargs))?
                .extract()?;

            let json_value: serde_json::Value =
                serde_json::from_str(&json_str).map_err(to_pyvalue_err)?;

            if let serde_json::Value::Object(map) = json_value {
                Ok(map.into_iter().collect())
            } else {
                Err(to_pyvalue_err("Config must be a dictionary"))
            }
        })?;

        Ok(Self {
            exec_algorithm_path,
            config_path,
            config: json_config,
        })
    }

    #[getter]
    fn exec_algorithm_path(&self) -> &String {
        &self.exec_algorithm_path
    }

    #[getter]
    fn config_path(&self) -> &String {
        &self.config_path
    }

    #[getter]
    fn config(&self, py: Python<'_>) -> PyResult<Py<PyDict>> {
        let py_dict = PyDict::new(py);

        for (key, value) in &self.config {
            let json_str = serde_json::to_string(value).map_err(to_pyvalue_err)?;
            let py_value = PyModule::import(py, "json")?.call_method("loads", (json_str,), None)?;
            py_dict.set_item(key, py_value)?;
        }
        Ok(py_dict.unbind())
    }
}
