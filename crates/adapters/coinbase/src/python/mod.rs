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

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    common::consts::COINBASE,
    config::{CoinbaseDataClientConfig, CoinbaseExecClientConfig},
    factories::{CoinbaseDataClientFactory, CoinbaseExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_coinbase_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<CoinbaseDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinbaseDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_coinbase_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<CoinbaseExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinbaseExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_coinbase_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<CoinbaseDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinbaseDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_coinbase_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<CoinbaseExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract CoinbaseExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.coinbase`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn coinbase(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(COINBASE), COINBASE)?;
    m.add_class::<crate::common::enums::CoinbaseEnvironment>()?;
    m.add_class::<crate::common::enums::CoinbaseMarginType>()?;
    m.add_class::<CoinbaseDataClientConfig>()?;
    m.add_class::<CoinbaseExecClientConfig>()?;
    m.add_class::<CoinbaseDataClientFactory>()?;
    m.add_class::<CoinbaseExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(COINBASE.to_string(), extract_coinbase_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Coinbase data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor(COINBASE.to_string(), extract_coinbase_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Coinbase exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "CoinbaseDataClientConfig".to_string(),
        extract_coinbase_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Coinbase data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "CoinbaseExecClientConfig".to_string(),
        extract_coinbase_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Coinbase exec config extractor: {e}"
        )));
    }

    Ok(())
}
