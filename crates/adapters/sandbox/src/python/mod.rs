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
pub mod factories;

use nautilus_common::factories::{ClientConfig, SimulatedExecutionClientFactory};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

use crate::{config::SandboxExecutionClientConfig, factory::SandboxExecutionClientFactory};

#[expect(clippy::needless_pass_by_value)]
fn extract_sandbox_exec_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn SimulatedExecutionClientFactory>> {
    match factory.extract::<SandboxExecutionClientFactory>(py) {
        Ok(f) => Ok(Box::new(f)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract SandboxExecutionClientFactory: {e}"
        ))),
    }
}

#[expect(clippy::needless_pass_by_value)]
fn extract_sandbox_exec_config(
    py: Python<'_>,
    config: Py<PyAny>,
) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<SandboxExecutionClientConfig>(py) {
        Ok(c) => Ok(Box::new(c)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract SandboxExecutionClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.sandbox`.
///
/// # Errors
///
/// Returns an error if the module registration fails or if adding functions/classes fails.
#[pymodule]
pub fn sandbox(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::SandboxExecutionClientConfig>()?;
    m.add_class::<crate::factory::SandboxExecutionClientFactory>()?;

    let registry = get_global_pyo3_registry();

    if let Err(e) = registry
        .register_sim_exec_factory_extractor("SANDBOX".to_string(), extract_sandbox_exec_factory)
    {
        return Err(to_pyruntime_err(format!(
            "Failed to register Sandbox simulated exec factory extractor: {e}"
        )));
    }

    if let Err(e) = registry.register_config_extractor(
        "SandboxExecutionClientConfig".to_string(),
        extract_sandbox_exec_config,
    ) {
        return Err(to_pyruntime_err(format!(
            "Failed to register Sandbox execution config extractor: {e}"
        )));
    }

    Ok(())
}
