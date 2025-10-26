// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! PyO3 registry system for generic trait object extraction.

use std::{collections::HashMap, sync::Mutex};

use nautilus_core::MUTEX_POISONED;
use pyo3::prelude::*;

use crate::factories::{ClientConfig, DataClientFactory};

/// Function type for extracting a `Py<PyAny>` factory to a boxed `DataClientFactory` trait object.
pub type FactoryExtractor =
    fn(py: Python<'_>, factory: Py<PyAny>) -> PyResult<Box<dyn DataClientFactory>>;

/// Function type for extracting a `Py<PyAny>` config to a boxed `ClientConfig` trait object.
pub type ConfigExtractor = fn(py: Python<'_>, config: Py<PyAny>) -> PyResult<Box<dyn ClientConfig>>;

/// Registry for PyO3 factory and config extractors.
///
/// This allows each adapter to register its own extraction logic for converting
/// `Py<PyAny>s` to boxed trait objects without requiring the live crate to know
/// about specific implementations.
#[derive(Debug)]
pub struct FactoryRegistry {
    factory_extractors: Mutex<HashMap<String, FactoryExtractor>>,
    config_extractors: Mutex<HashMap<String, ConfigExtractor>>,
}

impl FactoryRegistry {
    /// Creates a new empty registry.
    #[must_use]
    pub fn new() -> Self {
        Self {
            factory_extractors: Mutex::new(HashMap::new()),
            config_extractors: Mutex::new(HashMap::new()),
        }
    }

    /// Registers a factory extractor for a specific factory name.
    ///
    /// # Errors
    ///
    /// Returns an error if a factory with the same name is already registered.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn register_factory_extractor(
        &self,
        name: String,
        extractor: FactoryExtractor,
    ) -> anyhow::Result<()> {
        let mut extractors = self.factory_extractors.lock().expect(MUTEX_POISONED);

        if extractors.contains_key(&name) {
            anyhow::bail!("Factory extractor '{name}' is already registered");
        }
        extractors.insert(name, extractor);
        Ok(())
    }

    /// Registers a config extractor for a specific config type name.
    ///
    /// # Errors
    ///
    /// Returns an error if a config with the same type name is already registered.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn register_config_extractor(
        &self,
        type_name: String,
        extractor: ConfigExtractor,
    ) -> anyhow::Result<()> {
        let mut extractors = self.config_extractors.lock().expect(MUTEX_POISONED);

        if extractors.contains_key(&type_name) {
            anyhow::bail!("Config extractor '{type_name}' is already registered");
        }
        extractors.insert(type_name, extractor);
        Ok(())
    }

    /// Extracts a `Py<PyAny>` factory to a boxed `DataClientFactory` trait object.
    ///
    /// # Errors
    ///
    /// Returns an error if no extractor is registered for the factory type or extraction fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn extract_factory(
        &self,
        py: Python<'_>,
        factory: Py<PyAny>,
    ) -> PyResult<Box<dyn DataClientFactory>> {
        // Get the factory name to find the appropriate extractor
        let factory_name = factory
            .getattr(py, "name")?
            .call0(py)?
            .extract::<String>(py)?;

        let extractors = self.factory_extractors.lock().expect(MUTEX_POISONED);
        if let Some(extractor) = extractors.get(&factory_name) {
            extractor(py, factory)
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyNotImplementedError, _>(
                format!("No factory extractor registered for '{factory_name}'"),
            ))
        }
    }

    /// Extracts a `Py<PyAny>` config to a boxed `ClientConfig` trait object.
    ///
    /// # Errors
    ///
    /// Returns an error if no extractor is registered for the config type or extraction fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal mutex is poisoned.
    pub fn extract_config(
        &self,
        py: Python<'_>,
        config: Py<PyAny>,
    ) -> PyResult<Box<dyn ClientConfig>> {
        // Get the config class name to find the appropriate extractor
        let config_type_name = config
            .getattr(py, "__class__")?
            .getattr(py, "__name__")?
            .extract::<String>(py)?;

        let extractors = self.config_extractors.lock().expect(MUTEX_POISONED);
        if let Some(extractor) = extractors.get(&config_type_name) {
            extractor(py, config)
        } else {
            Err(PyErr::new::<pyo3::exceptions::PyNotImplementedError, _>(
                format!("No config extractor registered for '{config_type_name}'"),
            ))
        }
    }
}

impl Default for FactoryRegistry {
    fn default() -> Self {
        Self::new()
    }
}

/// Global PyO3 registry instance.
static GLOBAL_PYO3_REGISTRY: std::sync::LazyLock<FactoryRegistry> =
    std::sync::LazyLock::new(FactoryRegistry::new);

/// Gets a reference to the global PyO3 registry.
#[must_use]
pub fn get_global_pyo3_registry() -> &'static FactoryRegistry {
    &GLOBAL_PYO3_REGISTRY
}
