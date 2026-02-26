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

//! Python bindings for the Binance adapter.

pub mod config;
pub mod enums;
pub mod factories;
pub mod http_futures;
pub mod http_spot;
pub mod websocket_futures;
pub mod websocket_spot;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::{
    factories::{ClientConfig, DataClientFactory, ExecutionClientFactory},
    get_global_pyo3_registry,
};
use pyo3::prelude::*;

use crate::{
    common::enums::{BinanceEnvironment, BinancePositionSide, BinanceProductType},
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
    futures::{
        http::{
            client::BinanceFuturesHttpClient,
            query::{
                BatchCancelItem as FuturesBatchCancelItem,
                BatchModifyItem as FuturesBatchModifyItem, BatchOrderItem as FuturesBatchOrderItem,
            },
        },
        websocket::client::BinanceFuturesWebSocketClient,
    },
    spot::{
        http::{
            client::BinanceSpotHttpClient,
            query::{BatchCancelItem as SpotBatchCancelItem, BatchOrderItem as SpotBatchOrderItem},
        },
        websocket::streams::client::BinanceSpotWebSocketClient,
    },
};

fn extract_binance_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<BinanceDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BinanceDataClientFactory: {e}"
        ))),
    }
}

fn extract_binance_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<BinanceExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BinanceExecutionClientFactory: {e}"
        ))),
    }
}

fn extract_binance_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BinanceDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BinanceDataClientConfig: {e}"
        ))),
    }
}

fn extract_binance_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BinanceExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BinanceExecClientConfig: {e}"
        ))),
    }
}

/// Binance adapter Python module.
///
/// Loaded as `nautilus_pyo3.binance`.
///
/// # Errors
///
/// Returns an error if module initialization fails.
#[pymodule]
pub fn binance(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BinanceProductType>()?;
    m.add_class::<BinanceEnvironment>()?;
    m.add_class::<BinancePositionSide>()?;
    m.add_class::<BinanceSpotHttpClient>()?;
    m.add_class::<BinanceFuturesHttpClient>()?;
    m.add_class::<BinanceSpotWebSocketClient>()?;
    m.add_class::<BinanceFuturesWebSocketClient>()?;
    m.add_class::<FuturesBatchOrderItem>()?;
    m.add_class::<FuturesBatchCancelItem>()?;
    m.add_class::<FuturesBatchModifyItem>()?;
    m.add_class::<SpotBatchOrderItem>()?;
    m.add_class::<SpotBatchCancelItem>()?;
    m.add_class::<BinanceDataClientConfig>()?;
    m.add_class::<BinanceExecClientConfig>()?;
    m.add_class::<BinanceDataClientFactory>()?;
    m.add_class::<BinanceExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("BINANCE".to_string(), extract_binance_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Binance data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor("BINANCE".to_string(), extract_binance_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Binance exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BinanceDataClientConfig".to_string(),
        extract_binance_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Binance data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "BinanceExecClientConfig".to_string(),
        extract_binance_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Binance exec config extractor: {e}"
        )));
    }

    Ok(())
}
