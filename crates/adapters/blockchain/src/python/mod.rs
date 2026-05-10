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

//! Python bindings from [PyO3](https://pyo3.rs).

pub mod config;

#[cfg(feature = "hypersync")]
pub mod factories;

#[cfg(feature = "hypersync")]
use nautilus_common::factories::{ClientConfig, DataClientFactory};
#[cfg(feature = "hypersync")]
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
#[cfg(feature = "hypersync")]
use nautilus_system::get_global_pyo3_registry;
use pyo3::prelude::*;

/// Extractor function for `BlockchainDataClientFactory`.
#[cfg(feature = "hypersync")]
#[expect(clippy::needless_pass_by_value)] // Must match FactoryExtractor function pointer signature
fn extract_blockchain_factory(
    py: Python<'_>,
    factory: Py<PyAny>,
) -> PyResult<Box<dyn DataClientFactory>> {
    match factory.extract::<crate::factories::BlockchainDataClientFactory>(py) {
        Ok(concrete_factory) => Ok(Box::new(concrete_factory)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainDataClientFactory: {e}"
        ))),
    }
}

/// Extractor function for `BlockchainDataClientConfig`.
#[cfg(feature = "hypersync")]
#[expect(clippy::needless_pass_by_value)] // Must match ConfigExtractor function pointer signature
fn extract_blockchain_config(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>> {
    match config.extract::<crate::config::BlockchainDataClientConfig>(py) {
        Ok(concrete_config) => Ok(Box::new(concrete_config)),
        Err(e) => Err(to_pyvalue_err(format!(
            "Failed to extract BlockchainDataClientConfig: {e}"
        ))),
    }
}

/// Loaded as `nautilus_pyo3.blockchain`.
///
/// # Errors
///
/// Returns a `PyErr` if registering any module components fails.
#[pymodule]
pub fn blockchain(_: Python<'_>, m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<crate::config::BlockchainDataClientConfig>()?;
    m.add_class::<crate::config::DexPoolFilters>()?;
    #[cfg(feature = "hypersync")]
    m.add_class::<crate::factories::BlockchainDataClientFactory>()?;

    // Register extractors with the global registry
    #[cfg(feature = "hypersync")]
    {
        let registry = get_global_pyo3_registry();

        if let Err(e) = registry
            .register_factory_extractor("BLOCKCHAIN".to_string(), extract_blockchain_factory)
        {
            return Err(to_pyruntime_err(format!(
                "Failed to register blockchain factory extractor: {e}"
            )));
        }

        if let Err(e) = registry.register_config_extractor(
            "BlockchainDataClientConfig".to_string(),
            extract_blockchain_config,
        ) {
            return Err(to_pyruntime_err(format!(
                "Failed to register blockchain config extractor: {e}"
            )));
        }
    }

    Ok(())
}
