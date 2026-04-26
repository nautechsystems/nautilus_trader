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

#![expect(
    clippy::missing_errors_doc,
    reason = "errors documented on underlying Rust methods"
)]

pub mod config;
pub mod encoder;
pub mod enums;
pub mod factories;
pub mod grpc;
pub mod http;
pub mod submitter;
pub mod types;
pub mod urls;
pub mod wallet;
pub mod websocket;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    config::{DydxDataClientConfig, DydxExecClientConfig},
    factories::{DydxDataClientFactory, DydxExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_dydx_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<DydxDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DydxDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_dydx_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<DydxExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DydxExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_dydx_data_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DydxDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DydxDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_dydx_exec_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DydxExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DydxExecClientConfig: {e}"
        ))),
    }
}

#[pymodule]
pub fn dydx(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::http::client::DydxHttpClient>()?;
    m.add_class::<crate::websocket::client::DydxWebSocketClient>()?;
    m.add_class::<crate::common::enums::DydxNetwork>()?;
    m.add_class::<crate::common::enums::DydxOrderSide>()?;
    m.add_class::<crate::common::enums::DydxOrderType>()?;
    m.add_class::<crate::types::DydxOraclePrice>()?;
    m.add_class::<wallet::PyDydxWallet>()?;
    m.add_class::<grpc::PyDydxGrpcClient>()?;
    m.add_class::<submitter::PyDydxOrderSubmitter>()?;
    m.add_class::<encoder::PyDydxClientOrderIdEncoder>()?;
    m.add_class::<DydxDataClientConfig>()?;
    m.add_class::<DydxExecClientConfig>()?;
    m.add_class::<DydxDataClientFactory>()?;
    m.add_class::<DydxExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_dydx_grpc_urls, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_dydx_grpc_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_dydx_http_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_dydx_ws_url, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("DYDX".to_string(), extract_dydx_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register dYdX data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor("DYDX".to_string(), extract_dydx_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register dYdX exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_config_extractor("DydxDataClientConfig".to_string(), extract_dydx_data_config)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register dYdX data config extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_config_extractor("DydxExecClientConfig".to_string(), extract_dydx_exec_config)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register dYdX exec config extractor: {e}"
        )));
    }

    Ok(())
}
