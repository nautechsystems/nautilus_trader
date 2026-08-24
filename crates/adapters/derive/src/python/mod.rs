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
    common::{
        consts::{DERIVE, DERIVE_CLIENT_ID, DERIVE_VENUE},
        enums::DeriveEnvironment,
    },
    config::{DeriveDataClientConfig, DeriveExecutionClientConfig},
    factories::{DeriveDataClientFactory, DeriveExecutionClientFactory},
};

#[expect(clippy::needless_pass_by_value)]
fn extract_derive_data_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<DeriveDataClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeriveDataClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_derive_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn ExecutionClientFactory>> {
    match factory.extract::<DeriveExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeriveExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_derive_data_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DeriveDataClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeriveDataClientConfig: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_derive_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<DeriveExecutionClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract DeriveExecutionClientConfig: {e}"
        ))),
    }
}

/// Exposed through `nautilus_trader.adapters.derive`.
///
/// # Errors
///
/// Returns an error if any bindings fail to register with the Python module.
#[pymodule]
pub fn derive(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add(stringify!(DERIVE), DERIVE)?;
    m.add(stringify!(DERIVE_CLIENT_ID), *DERIVE_CLIENT_ID)?;
    m.add(stringify!(DERIVE_VENUE), *DERIVE_VENUE)?;
    m.add_class::<DeriveEnvironment>()?;
    m.add_class::<DeriveDataClientConfig>()?;
    m.add_class::<DeriveExecutionClientConfig>()?;
    m.add_class::<DeriveDataClientFactory>()?;
    m.add_class::<DeriveExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) =
        registry.register_factory_extractor(DERIVE.to_string(), extract_derive_data_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Derive data factory extractor: {e}"
        )));
    }

    if let Err(e) =
        registry.register_exec_factory_extractor(DERIVE.to_string(), extract_derive_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Derive exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "DeriveDataClientConfig".to_string(),
        extract_derive_data_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Derive data config extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "DeriveExecutionClientConfig".to_string(),
        extract_derive_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Derive exec config extractor: {e}"
        )));
    }

    Ok(())
}
