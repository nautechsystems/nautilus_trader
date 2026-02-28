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
use nautilus_model::identifiers::ClientOrderId;
use nautilus_system::{
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    get_global_pyo3_registry,
};
use pyo3::prelude::*;

use crate::{
    common::builder_fee::revoke_from_env,
    config::{HyperliquidDataClientConfig, HyperliquidExecClientConfig},
    factories::{
        HyperliquidDataClientFactory, HyperliquidExecFactoryConfig,
        HyperliquidExecutionClientFactory,
    },
    http::models::Cloid,
};

#[pyfunction]
#[pyo3(name = "revoke_hyperliquid_builder_fee", signature = (non_interactive = false))]
fn py_revoke_hyperliquid_builder_fee(non_interactive: bool) -> PyResult<bool> {
    std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .map_err(|e| to_pyruntime_err(format!("Failed to create runtime: {e}")))?;

        Ok(runtime.block_on(revoke_from_env(non_interactive)))
    })
    .join()
    .map_err(|_| to_pyruntime_err("Thread panicked"))?
}

/// Compute the cloid (hex hash) from a client_order_id.
///
/// The cloid is a keccak256 hash of the client_order_id, truncated to 16 bytes,
/// represented as a hex string with `0x` prefix.
#[pyfunction]
#[pyo3(name = "hyperliquid_cloid_from_client_order_id")]
fn py_hyperliquid_cloid_from_client_order_id(client_order_id: ClientOrderId) -> String {
    Cloid::from_client_order_id(client_order_id).to_hex()
}

/// Extract product type from a Hyperliquid symbol.
///
/// # Errors
///
/// Returns an error if the symbol does not contain a valid Hyperliquid product type suffix.
#[pyfunction]
#[pyo3(name = "hyperliquid_product_type_from_symbol")]
fn py_hyperliquid_product_type_from_symbol(
    symbol: &str,
) -> PyResult<crate::common::HyperliquidProductType> {
    crate::common::HyperliquidProductType::from_symbol(symbol).map_err(to_pyvalue_err)
}

fn extract_hyperliquid_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<HyperliquidDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract HyperliquidDataClientFactory: {e}"
        ))),
    }
}

fn extract_hyperliquid_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<HyperliquidExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract HyperliquidExecutionClientFactory: {e}"
        ))),
    }
}

fn extract_hyperliquid_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<HyperliquidDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract HyperliquidDataClientConfig: {e}"
        ))),
    }
}

fn extract_hyperliquid_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<HyperliquidExecFactoryConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract HyperliquidExecFactoryConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.hyperliquid`.
#[pymodule]
pub fn hyperliquid(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(
        "HYPERLIQUID_POST_ONLY_WOULD_MATCH",
        crate::common::consts::HYPERLIQUID_POST_ONLY_WOULD_MATCH,
    )?;
    m.add_class::<crate::http::HyperliquidHttpClient>()?;
    m.add_class::<crate::websocket::HyperliquidWebSocketClient>()?;
    m.add_class::<crate::common::enums::HyperliquidProductType>()?;
    m.add_class::<crate::common::enums::HyperliquidTpSl>()?;
    m.add_class::<crate::common::enums::HyperliquidConditionalOrderType>()?;
    m.add_class::<crate::common::enums::HyperliquidTrailingOffsetType>()?;
    m.add_function(wrap_pyfunction!(urls::py_get_hyperliquid_http_base_url, m)?)?;
    m.add_function(wrap_pyfunction!(urls::py_get_hyperliquid_ws_url, m)?)?;
    m.add_function(wrap_pyfunction!(
        py_hyperliquid_product_type_from_symbol,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        py_hyperliquid_cloid_from_client_order_id,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(py_revoke_hyperliquid_builder_fee, m)?)?;
    m.add_class::<HyperliquidDataClientConfig>()?;
    m.add_class::<HyperliquidExecClientConfig>()?;
    m.add_class::<HyperliquidExecFactoryConfig>()?;
    m.add_class::<HyperliquidDataClientFactory>()?;
    m.add_class::<HyperliquidExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry
        .register_factory_extractor("HYPERLIQUID".to_string(), extract_hyperliquid_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Hyperliquid data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_exec_factory_extractor(
        "HYPERLIQUID".to_string(),
        extract_hyperliquid_exec_factory,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Hyperliquid exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "HyperliquidDataClientConfig".to_string(),
        extract_hyperliquid_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Hyperliquid data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "HyperliquidExecFactoryConfig".to_string(),
        extract_hyperliquid_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Hyperliquid exec config extractor: {e}"
        )));
    }

    Ok(())
}
