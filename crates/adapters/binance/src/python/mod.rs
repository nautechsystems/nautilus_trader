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

pub mod arrow;
pub mod config;
pub mod enums;
pub mod factories;
pub mod types;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::data::ensure_rust_extractor_registered;
use nautilus_serialization::ensure_custom_data_registered;
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        bar::BinanceBar,
        consts::{BINANCE_NAUTILUS_FUTURES_BROKER_ID, BINANCE_NAUTILUS_SPOT_BROKER_ID},
        encoder::decode_broker_id,
        enums::{BinanceEnvironment, BinanceMarginType, BinancePositionSide, BinanceProductType},
    },
    config::{BinanceDataClientConfig, BinanceExecClientConfig},
    factories::{BinanceDataClientFactory, BinanceExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
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

#[expect(clippy::needless_pass_by_value)]
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

#[expect(clippy::needless_pass_by_value)]
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

#[expect(clippy::needless_pass_by_value)]
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

/// Decodes a Binance Spot encoded `clientOrderId` back to the original value.
///
/// Binance Spot orders placed through the Rust execution client have their
/// `ClientOrderId` encoded with a broker ID prefix for Link and Trade
/// attribution. This function reverses that encoding.
///
/// Strings without the broker prefix are returned unchanged.
#[pyfunction]
#[pyo3(name = "decode_binance_spot_client_order_id")]
fn py_decode_binance_spot_client_order_id(encoded: &str) -> String {
    decode_broker_id(encoded, BINANCE_NAUTILUS_SPOT_BROKER_ID)
}

/// Decodes a Binance Futures encoded `clientOrderId` back to the original value.
///
/// Binance Futures orders placed through the Rust execution client have their
/// `ClientOrderId` encoded with a broker ID prefix for Link and Trade
/// attribution. This function reverses that encoding.
///
/// Strings without the broker prefix are returned unchanged.
#[pyfunction]
#[pyo3(name = "decode_binance_futures_client_order_id")]
fn py_decode_binance_futures_client_order_id(encoded: &str) -> String {
    decode_broker_id(encoded, BINANCE_NAUTILUS_FUTURES_BROKER_ID)
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
    m.add_class::<BinanceMarginType>()?;
    m.add_class::<BinancePositionSide>()?;
    m.add_class::<BinanceBar>()?;
    m.add_function(wrap_pyfunction!(arrow::get_binance_arrow_schema_map, m)?)?;
    m.add_function(wrap_pyfunction!(
        arrow::py_binance_bar_to_arrow_record_batch_bytes,
        m
    )?)?;
    m.add_function(wrap_pyfunction!(
        arrow::py_binance_bar_from_arrow_record_batch_bytes,
        m
    )?)?;
    m.add_class::<BinanceDataClientConfig>()?;
    m.add_class::<BinanceExecClientConfig>()?;
    m.add_class::<BinanceDataClientFactory>()?;
    m.add_class::<BinanceExecutionClientFactory>()?;
    m.add_function(wrap_pyfunction!(py_decode_binance_spot_client_order_id, m)?)?;
    m.add_function(wrap_pyfunction!(
        py_decode_binance_futures_client_order_id,
        m
    )?)?;

    // Register BinanceBar for Arrow/JSON serialization and Python extraction
    ensure_custom_data_registered::<BinanceBar>();
    let _ = ensure_rust_extractor_registered::<BinanceBar>();

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
