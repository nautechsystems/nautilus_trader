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

//! Python bindings from `pyo3`.

pub mod config;
pub mod enums;
pub mod factories;
pub mod http;
pub mod urls;
pub mod websocket;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::{
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    get_global_pyo3_registry,
};
use pyo3::prelude::*;

use crate::{
    config::{DeribitDataClientConfig, DeribitExecClientConfig},
    factories::{DeribitDataClientFactory, DeribitExecutionClientFactory},
};

fn extract_deribit_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<DeribitDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeribitDataClientFactory: {e}"
        ))),
    }
}

fn extract_deribit_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<DeribitExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeribitExecutionClientFactory: {e}"
        ))),
    }
}

fn extract_deribit_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DeribitDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeribitDataClientConfig: {e}"
        ))),
    }
}

fn extract_deribit_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DeribitExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeribitExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.deribit`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn deribit(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<super::http::client::DeribitHttpClient>()?;
    m.add_class::<super::websocket::client::DeribitWebSocketClient>()?;
    m.add_class::<crate::common::enums::DeribitCurrency>()?;
    m.add_class::<crate::common::enums::DeribitProductType>()?;
    m.add_class::<crate::websocket::enums::DeribitUpdateInterval>()?;
    m.add_class::<DeribitDataClientConfig>()?;
    m.add_class::<DeribitExecClientConfig>()?;
    m.add_class::<DeribitDataClientFactory>()?;
    m.add_class::<DeribitExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_deribit_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_deribit_ws_url, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("DERIBIT".to_string(), extract_deribit_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Deribit data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor("DERIBIT".to_string(), extract_deribit_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Deribit exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "DeribitDataClientConfig".to_string(),
        extract_deribit_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Deribit data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "DeribitExecClientConfig".to_string(),
        extract_deribit_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Deribit exec config extractor: {e}"
        )));
    }

    Ok(())
}
