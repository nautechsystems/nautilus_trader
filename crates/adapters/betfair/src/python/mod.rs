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

//! Python bindings for the Betfair adapter.

pub mod config;
pub mod factories;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    config::{BetfairDataConfig, BetfairExecConfig},
    factories::{BetfairDataClientFactory, BetfairExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_betfair_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<BetfairDataClientFactory>(py) {
        Ok(factory) => Ok(Box::new(factory)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BetfairDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_betfair_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<BetfairExecutionClientFactory>(py) {
        Ok(factory) => Ok(Box::new(factory)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BetfairExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_betfair_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BetfairDataConfig>(py) {
        Ok(config) => Ok(Box::new(config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BetfairDataConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_betfair_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<BetfairExecConfig>(py) {
        Ok(config) => Ok(Box::new(config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BetfairExecConfig: {e}"
        ))),
    }
}

/// Betfair adapter Python module.
///
/// Loaded as `nautilus_pyo3.betfair`.
///
/// # Errors
///
/// Returns an error if module initialization fails.
#[pymodule]
pub fn betfair(_py: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<BetfairDataConfig>()?;
    m.add_class::<BetfairExecConfig>()?;
    m.add_class::<BetfairDataClientFactory>()?;
    m.add_class::<BetfairExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor("BETFAIR".to_string(), extract_betfair_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Betfair data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor("BETFAIR".to_string(), extract_betfair_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Betfair exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_config_extractor("BetfairDataConfig".to_string(), extract_betfair_data_config)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Betfair data config extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_config_extractor("BetfairExecConfig".to_string(), extract_betfair_exec_config)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Betfair exec config extractor: {e}"
        )));
    }

    Ok(())
}
