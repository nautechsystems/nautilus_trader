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

//! Python bindings for the Ax adapter.

pub mod http;
pub mod websocket;

use std::str::FromStr;

use nautilus_core::python::to_pyvalue_err;
use pyo3::{prelude::*, types::PyType};

use crate::{
    common::enums::{AxEnvironment, AxMarketDataLevel},
    http::client::AxHttpClient,
    websocket::data::AxMdWebSocketClient,
};

#[pymethods]
impl AxEnvironment {
    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(AxEnvironment),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "SANDBOX")]
    const fn py_sandbox() -> Self {
        Self::Sandbox
    }

    #[classattr]
    #[pyo3(name = "PRODUCTION")]
    const fn py_production() -> Self {
        Self::Production
    }
}

#[pymethods]
impl AxMarketDataLevel {
    fn __repr__(&self) -> String {
        format!(
            "<{}.{}: '{}'>",
            stringify!(AxMarketDataLevel),
            self.name(),
            self.value(),
        )
    }

    fn __str__(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn name(&self) -> String {
        self.to_string()
    }

    #[getter]
    #[must_use]
    pub fn value(&self) -> u8 {
        *self as u8
    }

    #[classmethod]
    #[pyo3(name = "from_str")]
    fn py_from_str(_: &Bound<'_, PyType>, data: &Bound<'_, PyAny>) -> PyResult<Self> {
        let data_str: &str = data.extract()?;
        let tokenized = data_str.to_uppercase();
        Self::from_str(&tokenized).map_err(to_pyvalue_err)
    }

    #[classattr]
    #[pyo3(name = "LEVEL_1")]
    const fn py_level1() -> Self {
        Self::Level1
    }

    #[classattr]
    #[pyo3(name = "LEVEL_2")]
    const fn py_level2() -> Self {
        Self::Level2
    }

    #[classattr]
    #[pyo3(name = "LEVEL_3")]
    const fn py_level3() -> Self {
        Self::Level3
    }
}

/// Loaded as `nautilus_pyo3.architect`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn architect(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<AxEnvironment>()?;
    m.add_class::<AxMarketDataLevel>()?;
    m.add_class::<AxHttpClient>()?;
    m.add_class::<AxMdWebSocketClient>()?;

    Ok(())
}
