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

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]

pub mod config;
pub mod factories;
pub mod http;
pub mod websocket;

use std::str::FromStr;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::{prelude::*, types::PyType};

use crate::{
    common::{
        consts::{AX, AX_CLIENT_ID, AX_VENUE},
        enums::{AxEnvironment, AxMarketDataLevel},
    },
    config::{AxDataClientConfig, AxExecutionClientConfig},
    factories::{AxDataClientFactory, AxExecutionClientFactory},
    http::client::AxHttpClient,
    python::websocket::{PyAxMdWebSocketClient, PyAxOrdersWebSocketClient},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_ax_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<AxDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract AxDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_ax_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<AxExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract AxExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_ax_data_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<AxDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract AxDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_ax_exec_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<AxExecutionClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract AxExecutionClientConfig: {e}"
        ))),
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
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
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
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
}

/// Exposed through `nautilus_trader.adapters.architect_ax`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn architect_ax(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(AX), AX)?;
    m.add(stringify!(AX_CLIENT_ID), *AX_CLIENT_ID)?;
    m.add(stringify!(AX_VENUE), *AX_VENUE)?;
    m.add_class::<AxEnvironment>()?;
    m.add_class::<AxMarketDataLevel>()?;
    m.add_class::<AxDataClientConfig>()?;
    m.add_class::<AxExecutionClientConfig>()?;
    m.add_class::<AxDataClientFactory>()?;
    m.add_class::<AxExecutionClientFactory>()?;
    m.add_class::<AxHttpClient>()?;
    m.add_class::<PyAxMdWebSocketClient>()?;
    m.add_class::<PyAxOrdersWebSocketClient>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry.register_factory_extractor(AX.to_string(), extract_ax_data_factory) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Ax data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor(AX.to_string(), extract_ax_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Ax exec factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_config_extractor("AxDataClientConfig".to_string(), extract_ax_data_config)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Ax data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "AxExecutionClientConfig".to_string(),
        extract_ax_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Ax exec config extractor: {e}"
        )));
    }

    Ok(())
}
