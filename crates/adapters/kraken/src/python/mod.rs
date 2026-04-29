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

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::enums::{KrakenEnvironment, KrakenProductType},
    config::{KrakenDataClientConfig, KrakenExecClientConfig},
    factories::{KrakenDataClientFactory, KrakenExecutionClientFactory},
    http::{KrakenFuturesHttpClient, KrakenSpotHttpClient},
    websocket::{
        futures::client::KrakenFuturesWebSocketClient, spot_v2::client::KrakenSpotWebSocketClient,
    },
};

pub mod config;
pub mod enums;
pub mod factories;
pub mod http_futures;
pub mod http_spot;
pub mod websocket_futures;
pub mod websocket_spot;

/// Determines the product type from a Kraken symbol.
///
/// Futures symbols have the following prefixes:
/// - `PI_` - Perpetual Inverse futures (e.g., `PI_XBTUSD`)
/// - `PF_` - Perpetual Fixed-margin futures (e.g., `PF_XBTUSD`)
/// - `FI_` - Fixed maturity Inverse futures (e.g., `FI_XBTUSD_230929`)
/// - `FF_` - Flex futures
///
/// All other symbols are considered spot.
#[pyfunction]
#[pyo3_stub_gen::derive::gen_stub_pyfunction(module = "nautilus_trader.kraken")]
#[pyo3(name = "kraken_product_type_from_symbol")]
fn py_kraken_product_type_from_symbol(symbol: &str) -> KrakenProductType {
    crate::common::enums::product_type_from_symbol(symbol)
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kraken_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<KrakenDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KrakenDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kraken_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<KrakenExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KrakenExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kraken_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<KrakenDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KrakenDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kraken_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<KrakenExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KrakenExecClientConfig: {e}"
        ))),
    }
}

#[pymodule]
pub fn kraken(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<KrakenEnvironment>()?;
    m.add_class::<KrakenProductType>()?;
    m.add_class::<KrakenSpotHttpClient>()?;
    m.add_class::<KrakenFuturesHttpClient>()?;
    m.add_class::<KrakenSpotWebSocketClient>()?;
    m.add_class::<KrakenFuturesWebSocketClient>()?;
    m.add_class::<KrakenDataClientConfig>()?;
    m.add_class::<KrakenExecClientConfig>()?;
    m.add_class::<KrakenDataClientFactory>()?;
    m.add_class::<KrakenExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(py_kraken_product_type_from_symbol, m)?)?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("KRAKEN".to_string(), extract_kraken_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kraken data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor("KRAKEN".to_string(), extract_kraken_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kraken exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "KrakenDataClientConfig".to_string(),
        extract_kraken_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kraken data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "KrakenExecClientConfig".to_string(),
        extract_kraken_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kraken exec config extractor: {e}"
        )));
    }

    Ok(())
}
