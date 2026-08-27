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

//! Configuration types for persistence backends.

use ahash::AHashMap;
#[cfg(feature = "python")]
use nautilus_core::python::to_pyvalue_err;
use serde::{Deserialize, Serialize};

use crate::backend::catalog::ParquetDataCatalog;

/// Configuration for an existing Parquet data catalog.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.persistence", from_py_object, frozen, eq)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")
)]
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DataCatalogConfig {
    /// The path to the data catalog.
    pub path: String,
    /// The filesystem protocol for the data catalog.
    pub fs_protocol: Option<String>,
    /// Storage options for the Rust object-store backend.
    pub fs_rust_storage_options: Option<AHashMap<String, String>>,
    /// The catalog name used by the data engine.
    pub name: Option<String>,
}

impl DataCatalogConfig {
    /// Creates a new [`DataCatalogConfig`] instance.
    #[must_use]
    pub const fn new(
        path: String,
        fs_protocol: Option<String>,
        fs_rust_storage_options: Option<AHashMap<String, String>>,
        name: Option<String>,
    ) -> Self {
        Self {
            path,
            fs_protocol,
            fs_rust_storage_options,
            name,
        }
    }

    /// Creates the configured [`ParquetDataCatalog`].
    ///
    /// # Errors
    ///
    /// Returns an error if the configured URI or object-store options are invalid.
    pub fn create_catalog(&self) -> anyhow::Result<ParquetDataCatalog> {
        let uri = match self.fs_protocol.as_deref() {
            Some("file") => self.path.clone(),
            Some(protocol) if !self.path.contains("://") => {
                format!("{protocol}://{}", self.path)
            }
            _ => self.path.clone(),
        };

        ParquetDataCatalog::from_uri(&uri, self.fs_rust_storage_options.clone(), None, None, None)
    }
}

#[cfg(feature = "python")]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
#[pyo3::pymethods]
impl DataCatalogConfig {
    /// Creates a configuration for an existing Parquet data catalog.
    #[new]
    #[pyo3(signature = (path, fs_protocol=None, fs_rust_storage_options=None, name=None))]
    fn py_new(
        path: String,
        fs_protocol: Option<String>,
        fs_rust_storage_options: Option<std::collections::HashMap<String, String>>,
        name: Option<String>,
    ) -> pyo3::PyResult<Self> {
        if path.trim().is_empty() {
            return Err(to_pyvalue_err("path must not be empty"));
        }

        if fs_protocol
            .as_ref()
            .is_some_and(|value| value.trim().is_empty())
        {
            return Err(to_pyvalue_err("fs_protocol must not be empty"));
        }

        if name.as_ref().is_some_and(|value| value.trim().is_empty()) {
            return Err(to_pyvalue_err("name must not be empty"));
        }

        Ok(Self::new(
            path,
            fs_protocol,
            fs_rust_storage_options.map(|values| values.into_iter().collect()),
            name,
        ))
    }

    #[getter]
    fn path(&self) -> &str {
        &self.path
    }

    #[getter]
    fn fs_protocol(&self) -> Option<&str> {
        self.fs_protocol.as_deref()
    }

    #[getter]
    fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns the sorted Rust storage-option keys without exposing secret values.
    #[getter]
    fn fs_rust_storage_option_keys(&self) -> Option<Vec<String>> {
        self.fs_rust_storage_options.as_ref().map(|options| {
            let mut keys = options.keys().cloned().collect::<Vec<_>>();
            keys.sort_unstable();
            keys
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "DataCatalogConfig(path='{}', fs_protocol={:?}, name={:?})",
            self.path, self.fs_protocol, self.name
        )
    }
}
