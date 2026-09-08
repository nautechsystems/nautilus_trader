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

//! Catalog factory registry primitives.

use std::sync::Arc;

use ahash::AHashMap;
use indexmap::IndexMap;
use nautilus_core::Params;

use crate::catalog::traits::DataCatalogBox;

/// Conventional name of the Parquet catalog factory registration.
pub const PARQUET_CATALOG_FACTORY_NAME: &str = "Parquet";

/// Minimal connection-config supplied to [`CatalogFactory`].
#[derive(Debug, Clone)]
pub struct CatalogConnectConfig {
    /// Resolved URI for the catalog backend (e.g. `file:///tmp/cat`, `s3://bucket/cat`).
    pub uri: String,
    /// Optional storage-backend options (credentials, region, ...) passed to `object_store`.
    pub storage_options: Option<AHashMap<String, String>>,
    /// Backend-specific catalog parameters.
    pub params: Option<Params>,
}

impl CatalogConnectConfig {
    /// Creates a new [`CatalogConnectConfig`].
    #[must_use]
    pub fn new(uri: impl Into<String>, storage_options: Option<AHashMap<String, String>>) -> Self {
        Self {
            uri: uri.into(),
            storage_options,
            params: None,
        }
    }

    /// Builds a [`CatalogConnectConfig`] from `path` + optional `fs_protocol`.
    #[must_use]
    pub fn from_path_and_protocol(
        path: &str,
        fs_protocol: Option<&str>,
        storage_options: Option<AHashMap<String, String>>,
    ) -> Self {
        let uri = match fs_protocol {
            _ if path.contains("://") => path.to_string(),
            Some(protocol) => format!("{protocol}://{path}"),
            None => path.to_string(),
        };
        Self::new(uri, storage_options)
    }
}

/// Factory for a named catalog backend.
pub type CatalogFactory =
    Arc<dyn Fn(&CatalogConnectConfig) -> anyhow::Result<DataCatalogBox> + Send + Sync>;

/// Ordered registry of catalog factories keyed by name.
pub type CatalogFactoryRegistry = IndexMap<String, CatalogFactory>;

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn from_path_and_protocol_joins_scheme() {
        let cfg = CatalogConnectConfig::from_path_and_protocol("bucket/cat", Some("s3"), None);
        assert_eq!(cfg.uri, "s3://bucket/cat");

        let cfg = CatalogConnectConfig::from_path_and_protocol("/tmp/cat", None, None);
        assert_eq!(cfg.uri, "/tmp/cat");

        let cfg = CatalogConnectConfig::from_path_and_protocol(
            "postgres://user:pass@localhost/catalog",
            Some("file"),
            None,
        );
        assert_eq!(cfg.uri, "postgres://user:pass@localhost/catalog");
    }
}
