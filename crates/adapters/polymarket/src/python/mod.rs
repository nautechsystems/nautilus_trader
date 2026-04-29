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
pub mod factories;

use nautilus_common::factories::{ClientConfig, DataClientFactory, ExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{
    config::{PolymarketDataClientConfig, PolymarketExecClientConfig},
    factories::{PolymarketDataClientFactory, PolymarketExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<PolymarketDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<PolymarketExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<PolymarketDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_polymarket_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<PolymarketExecClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract PolymarketExecClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.polymarket`.
#[pymodule]
pub fn polymarket(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::common::enums::SignatureType>()?;
    m.add_class::<PolymarketDataClientConfig>()?;
    m.add_class::<PolymarketExecClientConfig>()?;
    m.add_class::<PolymarketDataClientFactory>()?;
    m.add_class::<PolymarketExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry
        .register_factory_extractor("POLYMARKET".to_string(), extract_polymarket_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket data factory extractor: {e}"
        )));
    }

    if let Err(e) = registry
        .register_exec_factory_extractor("POLYMARKET".to_string(), extract_polymarket_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "PolymarketDataClientConfig".to_string(),
        extract_polymarket_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "PolymarketExecClientConfig".to_string(),
        extract_polymarket_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Polymarket exec config extractor: {e}"
        )));
    }

    Ok(())
}
