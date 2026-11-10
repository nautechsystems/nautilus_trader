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

mod config;
mod factories;

use std::sync::OnceLock;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::{
        consts::{KALSHI, KALSHI_CLIENT_ID, KALSHI_VENUE},
        enums::KalshiEnvironment,
    },
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    factories::{KalshiDataClientFactory, KalshiExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_kalshi_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<KalshiDataClientFactory>(py) {
        Ok(factory) => Ok(Box::new(factory)),
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
        Ok(factory) => Ok(Box::new(factory)),
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
        Ok(config) => Ok(Box::new(config)),
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
    match config.extract::<KalshiExecutionClientConfig>(py) {
        Ok(config) => Ok(Box::new(config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract KalshiExecutionClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_trader._libnautilus.kalshi`.
///
/// # Errors
///
/// Returns an error if a binding or extractor fails to register.
#[pymodule]
pub fn kalshi(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(KALSHI), KALSHI)?;
    m.add(stringify!(KALSHI_CLIENT_ID), *KALSHI_CLIENT_ID)?;
    m.add(stringify!(KALSHI_VENUE), *KALSHI_VENUE)?;
    m.add_class::<KalshiEnvironment>()?;
    m.add_class::<KalshiDataClientConfig>()?;
    m.add_class::<KalshiDataClientFactory>()?;
    m.add_class::<KalshiExecutionClientConfig>()?;
    m.add_class::<KalshiExecutionClientFactory>()?;
    register_extractors()
}

fn register_extractors() -> PyResult<()> {
    static REGISTERED: OnceLock<Result<(), String>> = OnceLock::new();
    REGISTERED
        .get_or_init(register_extractors_once)
        .clone()
        .map_err(to_pyruntime_err)
}

fn register_extractors_once() -> Result<(), String> {
    let registry = get_global_pyo3_registry();
    registry
        .register_factory_extractor(KALSHI.to_string(), extract_kalshi_data_factory)
        .map_err(|e| format!("Failed to register Kalshi data factory extractor: {e}"))?;
    registry
        .register_exec_factory_extractor(KALSHI.to_string(), extract_kalshi_exec_factory)
        .map_err(|e| format!("Failed to register Kalshi exec factory extractor: {e}"))?;
    registry
        .register_config_extractor(
            "KalshiDataClientConfig".to_string(),
            extract_kalshi_data_config,
        )
        .map_err(|e| format!("Failed to register Kalshi data config extractor: {e}"))?;
    registry
        .register_config_extractor(
            "KalshiExecutionClientConfig".to_string(),
            extract_kalshi_exec_config,
        )
        .map_err(|e| format!("Failed to register Kalshi exec config extractor: {e}"))?;

    Ok(())
}
