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

pub mod canceller;
pub mod config;
pub mod enums;
pub mod factories;
pub mod http;
pub mod submitter;
pub mod urls;
pub mod websocket;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::{
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    get_global_pyo3_registry,
};
use pyo3::prelude::*;

use crate::{
    config::{BitmexDataClientConfig, BitmexExecClientConfig},
    factories::{BitmexDataClientFactory, BitmexExecFactoryConfig, BitmexExecutionClientFactory},
};

fn extract_bitmex_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<BitmexDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BitmexDataClientFactory: {e}"
        ))),
    }
}

fn extract_bitmex_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<BitmexExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BitmexExecutionClientFactory: {e}"
        ))),
    }
}

fn extract_bitmex_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BitmexDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BitmexDataClientConfig: {e}"
        ))),
    }
}

fn extract_bitmex_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BitmexExecFactoryConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BitmexExecFactoryConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.bitmex`.
///
/// # Errors
///
/// Returns an error if the module registration fails or if adding functions/classes fails.
#[pymodule]
pub fn bitmex(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add("BITMEX_HTTP_URL", crate::common::consts::BITMEX_HTTP_URL)?;
    m.add("BITMEX_WS_URL", crate::common::consts::BITMEX_WS_URL)?;
    m.add_class::<crate::http::client::BitmexHttpClient>()?;
    m.add_class::<crate::websocket::BitmexWebSocketClient>()?;
    m.add_class::<crate::broadcast::canceller::CancelBroadcaster>()?;
    m.add_class::<crate::broadcast::submitter::SubmitBroadcaster>()?;
    m.add_class::<BitmexDataClientConfig>()?;
    m.add_class::<BitmexExecClientConfig>()?;
    m.add_class::<BitmexExecFactoryConfig>()?;
    m.add_class::<BitmexDataClientFactory>()?;
    m.add_class::<BitmexExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(urls::get_bitmex_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::get_bitmex_ws_url, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("BITMEX".to_string(), extract_bitmex_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register BitMEX data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor("BITMEX".to_string(), extract_bitmex_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register BitMEX exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BitmexDataClientConfig".to_string(),
        extract_bitmex_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register BitMEX data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BitmexExecFactoryConfig".to_string(),
        extract_bitmex_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register BitMEX exec config extractor: {e}"
        )));
    }

    Ok(())
}
