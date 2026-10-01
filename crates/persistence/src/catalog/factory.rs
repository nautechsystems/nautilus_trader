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
use serde_json::Value;

use crate::{
    catalog::traits::DataCatalog,
    common::paths::file_protocol_uri,
    config::{CatalogBackendType, CatalogCompression},
};

/// Conventional name of the Parquet catalog factory registration.
pub const PARQUET_CATALOG_FACTORY_NAME: &str = "Parquet";

/// Minimal connection-config supplied to [`CatalogFactory`].
#[derive(Debug, Clone)]
pub struct CatalogConnectConfig {
    /// Resolved URI for the catalog backend (e.g. `file:///tmp/cat`, `s3://bucket/cat`).
    pub uri: String,
    /// Optional storage-backend options (credentials, region, ...) passed to `object_store`.
    pub storage_options: Option<AHashMap<String, String>>,
    /// The number of rows per batch the catalog reads and writes.
    pub batch_size: Option<usize>,
    /// The compression codec of written data files.
    pub compression: Option<CatalogCompression>,
    /// The maximum number of rows per written row group.
    pub max_row_group_size: Option<usize>,
    /// Backend-specific catalog parameters, which a factory validates with
    /// [`validate_catalog_params`].
    pub params: Option<Params>,
}

impl CatalogConnectConfig {
    /// Creates a new [`CatalogConnectConfig`].
    #[must_use]
    pub fn new(uri: impl Into<String>, storage_options: Option<AHashMap<String, String>>) -> Self {
        Self {
            uri: uri.into(),
            storage_options,
            batch_size: None,
            compression: None,
            max_row_group_size: None,
            params: None,
        }
    }

    /// Builds a [`CatalogConnectConfig`] from `path` + optional `fs_protocol`.
    ///
    /// A `file` protocol with a Windows drive path becomes `file:///C:/...`.
    /// A path that already contains `://` is left unchanged.
    #[must_use]
    pub fn from_path_and_protocol(
        path: &str,
        fs_protocol: Option<&str>,
        storage_options: Option<AHashMap<String, String>>,
    ) -> Self {
        let uri = match fs_protocol {
            _ if path.contains("://") => path.to_string(),
            Some("file") => file_protocol_uri(path),
            Some(protocol) => format!("{protocol}://{path}"),
            None => path.to_string(),
        };

        Self::new(uri, storage_options)
    }
}

/// Factory for a named catalog backend.
pub type CatalogFactory =
    Arc<dyn Fn(&CatalogConnectConfig) -> anyhow::Result<DataCatalog> + Send + Sync>;

/// Ordered registry of catalog factories keyed by name.
pub type CatalogFactoryRegistry = IndexMap<String, CatalogFactory>;

/// Resolves a catalog backend through the factory registry and opens the catalog.
///
/// All variants resolve through the registry so built-ins and custom names share one code path.
///
/// # Errors
///
/// Returns an error if the backend's factory is not registered or opening the catalog fails.
pub fn create_catalog(
    backend: &CatalogBackendType,
    config: &CatalogConnectConfig,
    factories: &CatalogFactoryRegistry,
) -> anyhow::Result<DataCatalog> {
    let name = backend.to_string();
    factories
        .get(&name)
        .ok_or_else(|| anyhow::anyhow!("No catalog factory registered for '{name}'"))?(config)
}

/// The JSON type a catalog param holds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CatalogParamKind {
    /// An object of named values.
    Object,
    /// A non-negative integer.
    Count,
    /// A string.
    Text,
    /// An array.
    List,
}

impl CatalogParamKind {
    fn accepts(self, value: &Value) -> bool {
        match self {
            Self::Object => value.is_object(),
            Self::Count => value.is_u64(),
            Self::Text => value.is_string(),
            Self::List => value.is_array(),
        }
    }

    const fn expected(self) -> &'static str {
        match self {
            Self::Object => "an object",
            Self::Count => "a non-negative integer",
            Self::Text => "a string",
            Self::List => "a list",
        }
    }
}

/// Checks that `params` holds only the `accepted` keys, each with its declared kind.
///
/// A catalog factory calls this with the keys it reads. A `null` value satisfies any declared
/// kind, but an undeclared key fails whatever its value.
///
/// # Errors
///
/// Returns an error naming the catalog and the key if a key is not accepted or its value has the
/// wrong kind.
pub fn validate_catalog_params(
    catalog: &str,
    params: Option<&Params>,
    accepted: &[(&str, CatalogParamKind)],
) -> anyhow::Result<()> {
    let Some(params) = params else {
        return Ok(());
    };

    for (key, value) in params {
        let Some((_, kind)) = accepted.iter().find(|(name, _)| name == key) else {
            if accepted.is_empty() {
                anyhow::bail!(
                    "Unknown {catalog} catalog param '{key}': this catalog takes no params"
                );
            }

            let names = accepted
                .iter()
                .map(|(name, _)| *name)
                .collect::<Vec<_>>()
                .join(", ");
            anyhow::bail!("Unknown {catalog} catalog param '{key}', expected one of {names}");
        };

        anyhow::ensure!(
            value.is_null() || kind.accepts(value),
            "Invalid {catalog} catalog param '{key}': expected {}",
            kind.expected()
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

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

    #[rstest]
    #[case(r"C:\data\catalog", "file:///C:/data/catalog")]
    #[case("C:/data/catalog", "file:///C:/data/catalog")]
    #[case(r"D:\", "file:///D:/")]
    fn from_path_and_protocol_normalizes_windows_drive_paths(
        #[case] path: &str,
        #[case] expected: &str,
    ) {
        let cfg = CatalogConnectConfig::from_path_and_protocol(path, Some("file"), None);
        assert_eq!(cfg.uri, expected);
    }

    #[rstest]
    fn from_path_and_protocol_keeps_non_drive_file_joins() {
        let cfg = CatalogConnectConfig::from_path_and_protocol("/tmp/cat", Some("file"), None);
        assert_eq!(cfg.uri, "file:///tmp/cat");

        let cfg = CatalogConnectConfig::from_path_and_protocol("data/cat", Some("file"), None);
        assert_eq!(cfg.uri, "file://data/cat");

        let cfg = CatalogConnectConfig::from_path_and_protocol(
            r"\\server\share\catalog",
            Some("file"),
            None,
        );
        assert_eq!(cfg.uri, r"file://\\server\share\catalog");
    }

    #[rstest]
    fn create_catalog_rejects_unregistered_backend() {
        let config = CatalogConnectConfig::new("/tmp/catalog", None);

        let error = create_catalog(
            &CatalogBackendType::External("Missing".to_string()),
            &config,
            &CatalogFactoryRegistry::new(),
        )
        .expect_err("an unregistered backend should fail");

        assert_eq!(
            error.to_string(),
            "No catalog factory registered for 'Missing'"
        );
    }

    const ACCEPTED: &[(&str, CatalogParamKind)] = &[
        ("options", CatalogParamKind::Object),
        ("rows", CatalogParamKind::Count),
        ("codec", CatalogParamKind::Text),
        ("rules", CatalogParamKind::List),
    ];

    fn params(entries: &[(&str, Value)]) -> Params {
        let mut params = Params::new();

        for (key, value) in entries {
            params.insert((*key).to_string(), value.clone());
        }

        params
    }

    #[rstest]
    #[case::absent(None)]
    #[case::every_kind(Some(params(&[
        ("options", json!({"region": "eu-west-1"})),
        ("rows", json!(1024)),
        ("codec", json!("zstd")),
        ("rules", json!([])),
    ])))]
    #[case::null_counts_as_absent(Some(params(&[("rows", Value::Null)])))]
    fn validate_catalog_params_accepts_declared_keys(#[case] params: Option<Params>) {
        assert!(validate_catalog_params("Test", params.as_ref(), ACCEPTED).is_ok());
    }

    #[rstest]
    #[case::unknown(
        "no_such_param",
        json!(1),
        "Unknown Test catalog param 'no_such_param', expected one of options, rows, codec, rules"
    )]
    #[case::unknown_null(
        "no_such_param",
        Value::Null,
        "Unknown Test catalog param 'no_such_param', expected one of options, rows, codec, rules"
    )]
    #[case::text_for_count(
        "rows",
        json!("big"),
        "Invalid Test catalog param 'rows': expected a non-negative integer"
    )]
    #[case::negative_count(
        "rows",
        json!(-1),
        "Invalid Test catalog param 'rows': expected a non-negative integer"
    )]
    #[case::text_for_object(
        "options",
        json!("region=eu-west-1"),
        "Invalid Test catalog param 'options': expected an object"
    )]
    #[case::count_for_text(
        "codec",
        json!(6),
        "Invalid Test catalog param 'codec': expected a string"
    )]
    #[case::object_for_list(
        "rules",
        json!({}),
        "Invalid Test catalog param 'rules': expected a list"
    )]
    fn validate_catalog_params_rejects_unknown_or_mistyped_key(
        #[case] key: &str,
        #[case] value: Value,
        #[case] expected: &str,
    ) {
        let error =
            validate_catalog_params("Test", Some(&params(&[(key, value)])), ACCEPTED).unwrap_err();

        assert_eq!(error.to_string(), expected);
    }

    #[rstest]
    fn validate_catalog_params_rejects_every_key_when_none_accepted() {
        let error =
            validate_catalog_params("Test", Some(&params(&[("rows", json!(1))])), &[]).unwrap_err();

        assert_eq!(
            error.to_string(),
            "Unknown Test catalog param 'rows': this catalog takes no params"
        );
    }
}
