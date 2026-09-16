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
//!
//! The Kalshi Python surface exposes configuration, environment selection, and factories.
//! Clients are created by a `TradingNode` through the registered factory extractors.

pub mod config;
pub mod enums;
pub mod factories;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::identifiers::{ClientId, Venue};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        consts::{KALSHI, KALSHI_CLIENT_ID, KALSHI_VENUE},
        enums::{KalshiEnvironment, KalshiSelfTradePrevention},
    },
    config::{KalshiDataClientConfig, KalshiExecClientConfig},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_kalshi_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<KalshiDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KalshiDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kalshi_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<KalshiExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KalshiExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kalshi_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<KalshiDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KalshiDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_kalshi_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<KalshiExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KalshiExecutionClientConfig: {e}"
        ))),
    }
}

/// Exposed through `nautilus_trader.adapters.kalshi`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn kalshi(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(KALSHI), KALSHI)?;
    m.add(
        stringify!(KALSHI_CLIENT_ID),
        ClientId::from(KALSHI_CLIENT_ID),
    )?;
    m.add(stringify!(KALSHI_VENUE), Venue::from(KALSHI_VENUE))?;
    m.add_class::<KalshiEnvironment>()?;
    m.add_class::<KalshiSelfTradePrevention>()?;
    m.add_class::<KalshiDataClientConfig>()?;
    m.add_class::<KalshiDataClientFactory>()?;
    m.add_class::<KalshiExecClientConfig>()?;
    m.add_class::<KalshiExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(KALSHI.to_string(), extract_kalshi_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kalshi data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor(KALSHI.to_string(), extract_kalshi_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kalshi exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "KalshiDataClientConfig".to_string(),
        extract_kalshi_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kalshi data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "KalshiExecutionClientConfig".to_string(),
        extract_kalshi_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Kalshi exec config extractor: {e}"
        )));
    }

    Ok(())
}
