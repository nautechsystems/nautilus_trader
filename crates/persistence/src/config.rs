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

//! Persistence configuration shared by live and backtest runtimes.

use std::fmt::Display;

use nautilus_common::config::{ConfigError, ConfigErrorCollector, ConfigResult};
use nautilus_core::{DurationNanos, Params, UnixNanos};
use nautilus_model::{
    data::{NautilusDataType, NautilusRecordType},
    instruments::NautilusInstrumentType,
};
use serde::{Deserialize, Serialize};

use crate::{
    catalog::factory::PARQUET_CATALOG_FACTORY_NAME, common::backend_name::backend_type,
    writer::factory::WriterBackendType,
};

backend_type!(
    /// Catalog backend used to satisfy runtime data catalog requests.
    ///
    /// Serializes as its name, so an external backend round-trips as the plain factory name that
    /// registered it, matching [`WriterBackendType`].
    CatalogBackendType {
        Parquet => PARQUET_CATALOG_FACTORY_NAME,
    }
);

/// Configuration for a catalog available to request-time historical data loading.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.persistence", from_py_object, eq)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")
)]
pub struct DataCatalogConfig {
    /// The path to the data catalog.
    path: String,
    /// The catalog registration name.
    name: Option<String>,
    /// The fsspec file system protocol for the data catalog.
    #[serde(default = "default_fs_protocol")]
    #[builder(default = default_fs_protocol())]
    fs_protocol: String,
    /// The catalog backend implementation to use.
    #[serde(default)]
    #[builder(default)]
    catalog_backend: CatalogBackendType,
    /// Backend-specific catalog parameters.
    params: Option<Params>,
    #[serde(default)]
    fs_rust_storage_options: Option<ahash::AHashMap<String, String>>,
    /// Whether the catalog rejects response write-back.
    #[serde(default)]
    #[builder(default)]
    read_only: bool,
}

impl DataCatalogConfig {
    /// Creates a new [`DataCatalogConfig`] instance.
    #[must_use]
    pub fn new(
        path: String,
        fs_protocol: Option<String>,
        catalog_backend: Option<CatalogBackendType>,
    ) -> Self {
        Self {
            path,
            name: None,
            fs_protocol: fs_protocol.unwrap_or_else(default_fs_protocol),
            catalog_backend: catalog_backend.unwrap_or_default(),
            params: None,
            fs_rust_storage_options: None,
            read_only: false,
        }
    }

    /// Sets native object-store connection options.
    #[must_use]
    pub fn with_storage_options(
        mut self,
        options: Option<ahash::AHashMap<String, String>>,
    ) -> Self {
        self.fs_rust_storage_options = options;
        self
    }

    /// Returns native object-store options.
    #[must_use]
    pub fn fs_rust_storage_options(&self) -> Option<&ahash::AHashMap<String, String>> {
        self.fs_rust_storage_options.as_ref()
    }

    /// Creates the configured catalog through the built-in factory registry.
    ///
    /// # Errors
    ///
    /// Returns an error if the backend is unavailable or its connection cannot be opened.
    pub fn create_catalog(&self) -> anyhow::Result<crate::catalog::traits::DataCatalogBox> {
        let mut connect = crate::catalog::factory::CatalogConnectConfig::from_path_and_protocol(
            &self.path,
            Some(&self.fs_protocol),
            self.fs_rust_storage_options.clone(),
        );
        connect.params.clone_from(&self.params);
        let factories = crate::backend::default_catalog_factories();
        let name = self.catalog_backend.to_string();
        factories
            .get(&name)
            .ok_or_else(|| anyhow::anyhow!("No catalog factory registered for '{name}'"))?(
            &connect
        )
    }

    /// Returns a copy with catalog registration name set.
    #[must_use]
    pub fn with_name(mut self, name: Option<String>) -> Self {
        self.name = name;
        self
    }

    /// Returns a copy with backend-specific catalog parameters set.
    #[must_use]
    pub fn with_params(mut self, params: Option<Params>) -> Self {
        self.params = params;
        self
    }

    /// Returns a copy with read-only response write-back behavior set.
    #[must_use]
    pub const fn with_read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Returns the path to the data catalog.
    #[must_use]
    pub fn path(&self) -> &str {
        &self.path
    }

    /// Returns the catalog registration name.
    #[must_use]
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    /// Returns whether the catalog rejects response write-back.
    #[must_use]
    pub const fn read_only(&self) -> bool {
        self.read_only
    }

    /// Returns the fsspec file system protocol for the data catalog.
    #[must_use]
    pub fn fs_protocol(&self) -> &str {
        &self.fs_protocol
    }

    /// Returns the catalog backend implementation to use.
    #[must_use]
    pub const fn catalog_backend(&self) -> &CatalogBackendType {
        &self.catalog_backend
    }

    /// Returns backend-specific catalog parameters.
    #[must_use]
    pub const fn params(&self) -> Option<&Params> {
        self.params.as_ref()
    }
}

/// Configuration for file rotation in streaming output.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RotationConfig {
    /// Rotate based on file size.
    Size {
        /// Maximum buffer size in bytes before rotation.
        max_size: u64,
    },
    /// Rotate based on a time interval.
    Interval {
        /// Interval in nanoseconds.
        interval_ns: DurationNanos,
    },
    /// Rotate based on scheduled dates.
    ScheduledDates {
        /// Interval in nanoseconds.
        interval_ns: DurationNanos,
        /// Start of the scheduled rotation period.
        schedule_ns: UnixNanos,
    },
    /// No automatic rotation.
    NoRotation,
}

impl RotationConfig {
    /// Converts the public streaming configuration into the writer's runtime form.
    #[must_use]
    pub fn to_writer_rotation_config(&self) -> crate::writer::feather::RotationConfig {
        match self {
            Self::Size { max_size } => crate::writer::feather::RotationConfig::Size {
                max_size: *max_size,
            },
            Self::Interval { interval_ns } => crate::writer::feather::RotationConfig::Interval {
                interval_ns: interval_ns.as_u64(),
            },
            Self::ScheduledDates {
                interval_ns,
                schedule_ns,
            } => crate::writer::feather::RotationConfig::scheduled_utc(
                interval_ns.as_u64(),
                *schedule_ns,
            ),
            Self::NoRotation => crate::writer::feather::RotationConfig::NoRotation,
        }
    }
}

/// Record filter entry for streaming persistence.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StreamingRecordFilterConfig {
    /// Record family to write.
    pub record_type: NautilusRecordType,
    /// Optional identifiers within the record family.
    pub identifiers: Option<Vec<String>>,
}

/// Configuration streaming live or backtest runs to a persistence writer.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
pub struct StreamingConfig {
    /// Path to the data catalog.
    pub catalog_path: String,
    /// Filesystem protocol for the catalog.
    pub fs_protocol: String,
    /// Flush interval in milliseconds.
    pub flush_interval_ms: u64,
    /// Whether to replace existing files.
    pub replace_existing: bool,
    /// Rotation configuration.
    pub rotation_config: RotationConfig,
    /// Writer backend (`Feather`, `Parquet`, or external factory name).
    #[serde(default)]
    #[builder(default)]
    pub writer_backend: WriterBackendType,
    /// Optional data families to write.
    pub data_types: Option<Vec<NautilusDataType>>,
    /// Optional record families to write.
    pub record_types: Option<Vec<NautilusRecordType>>,
    /// Optional instrument families to write.
    pub instrument_types: Option<Vec<NautilusInstrumentType>>,
    /// Optional record family and identifier filters.
    pub record_filters: Option<Vec<StreamingRecordFilterConfig>>,
    /// Backend-specific writer parameters.
    pub params: Option<Params>,
}

impl<S: streaming_config_builder::IsComplete> StreamingConfigBuilder<S> {
    /// Validates and builds the [`StreamingConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`StreamingConfig::validate`]).
    pub fn build(self) -> ConfigResult<StreamingConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
}

impl StreamingConfig {
    /// Creates new [`StreamingConfig`] instance.
    #[must_use]
    pub fn new(
        catalog_path: String,
        fs_protocol: String,
        flush_interval_ms: u64,
        replace_existing: bool,
        rotation_config: RotationConfig,
    ) -> Self {
        Self {
            catalog_path,
            fs_protocol,
            flush_interval_ms,
            replace_existing,
            rotation_config,
            writer_backend: WriterBackendType::default(),
            data_types: None,
            record_types: None,
            instrument_types: None,
            record_filters: None,
            params: None,
        }
    }

    /// Validates the streaming configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] (a [`ConfigError::Multiple`] when more than one field is
    /// invalid) if any field fails validation.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        errors.check(
            !self.catalog_path.trim().is_empty(),
            ConfigError::empty_field("catalog_path"),
        );
        errors.check(
            !self.fs_protocol.trim().is_empty(),
            ConfigError::empty_field("fs_protocol"),
        );

        let flush_interval_ms = self.flush_interval_ms;
        errors.check(
            flush_interval_ms > 0,
            ConfigError::range(
                "flush_interval_ms",
                format!("must be a positive number of milliseconds, was {flush_interval_ms}"),
            ),
        );

        if let Some(data_types) = &self.data_types {
            errors.check(
                !data_types.is_empty(),
                ConfigError::invalid_value(
                    "data_types",
                    "must not be an empty list; omit the field for unfiltered streaming",
                ),
            );
        }

        if let Some(record_types) = &self.record_types {
            errors.check(
                !record_types.is_empty(),
                ConfigError::invalid_value(
                    "record_types",
                    "must not be an empty list; omit the field for unfiltered streaming",
                ),
            );
        }

        if let Some(instrument_types) = &self.instrument_types {
            errors.check(
                !instrument_types.is_empty(),
                ConfigError::invalid_value(
                    "instrument_types",
                    "must not be an empty list; omit the field for unfiltered streaming",
                ),
            );
        }

        if let Some(record_filters) = &self.record_filters {
            errors.check(
                !record_filters.is_empty(),
                ConfigError::invalid_value(
                    "record_filters",
                    "must not be an empty list; omit the field for unfiltered streaming",
                ),
            );
        }

        errors.into_result()
    }
}

pub(crate) fn default_fs_protocol() -> String {
    "file".to_string()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    #[rstest]
    fn catalog_backend_type_preserves_external_factory_case() {
        assert_eq!(
            "CaseSensitiveExternal"
                .parse::<CatalogBackendType>()
                .unwrap(),
            CatalogBackendType::External("CaseSensitiveExternal".to_string()),
        );
    }

    #[rstest]
    fn catalog_backend_type_accepts_parquet_without_changing_default() {
        assert_eq!(
            "parquet".parse::<CatalogBackendType>().unwrap(),
            CatalogBackendType::Parquet
        );
        assert_eq!(CatalogBackendType::Parquet.to_string(), "Parquet");
        assert_eq!(CatalogBackendType::default(), CatalogBackendType::Parquet);
    }

    #[rstest]
    fn catalog_backend_type_rejects_empty_name() {
        assert!("".parse::<CatalogBackendType>().is_err());
    }

    #[rstest]
    fn catalog_backend_type_display_roundtrips_built_ins() {
        assert_eq!(CatalogBackendType::Parquet.to_string(), "Parquet");
        assert_eq!(CatalogBackendType::Parquet.to_string(), "Parquet");
        assert_eq!(
            "Parquet".parse::<CatalogBackendType>().unwrap(),
            CatalogBackendType::Parquet,
        );
        assert_eq!(
            "Parquet".parse::<CatalogBackendType>().unwrap(),
            CatalogBackendType::Parquet,
        );
    }

    #[rstest]
    fn data_catalog_config_preserves_backend_params() {
        let mut params = Params::new();
        params.insert("batch_size".to_string(), json!(1024));
        let config = DataCatalogConfig::new(
            "/data/catalog".to_string(),
            Some("file".to_string()),
            Some(CatalogBackendType::Parquet),
        )
        .with_params(Some(params));

        assert_eq!(
            config.params().and_then(|p| p.get_u64("batch_size")),
            Some(1024)
        );
    }

    #[rstest]
    fn data_catalog_config_toml_defaults_backend_fields() {
        let config: DataCatalogConfig = toml::from_str(
            r#"
path = "/data/catalog"
"#,
        )
        .unwrap();

        assert_eq!(config.path(), "/data/catalog");
        assert_eq!(config.fs_protocol(), "file");
        assert_eq!(config.catalog_backend(), &CatalogBackendType::Parquet);
        assert_eq!(config.params(), None);
        assert!(!config.read_only());
    }

    #[rstest]
    fn data_catalog_config_reads_read_only_flag() {
        let config: DataCatalogConfig = toml::from_str(
            r#"
path = "/data/catalog"
read_only = true
"#,
        )
        .unwrap();

        assert!(config.read_only());
    }

    #[rstest]
    fn streaming_config_builder_valid() {
        let config = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(config.is_ok());
    }

    #[rstest]
    fn streaming_config_builder_preserves_backend_params() {
        let mut params = Params::new();
        params.insert("promote_on_close".to_string(), json!(true));
        let config = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .params(params)
            .build()
            .unwrap();

        assert_eq!(
            config
                .params
                .as_ref()
                .and_then(|p| p.get_bool("promote_on_close")),
            Some(true)
        );
    }

    #[rstest]
    fn streaming_config_zero_flush_interval_rejected() {
        let result = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(0)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "flush_interval_ms")
        );
    }

    #[rstest]
    fn streaming_config_empty_catalog_path_rejected() {
        let result = StreamingConfig::builder()
            .catalog_path(String::new())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::EmptyField { field }) if field == "catalog_path")
        );
    }

    #[rstest]
    fn streaming_config_empty_filter_lists_rejected() {
        let result = StreamingConfig::builder()
            .catalog_path("/data/catalog".to_string())
            .fs_protocol("file".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .data_types(vec![])
            .record_types(vec![])
            .instrument_types(vec![])
            .record_filters(vec![])
            .build();

        match result.unwrap_err() {
            ConfigError::Multiple { errors } => {
                assert_eq!(errors.len(), 4);
                assert!(errors.iter().all(|e| matches!(
                    e,
                    ConfigError::InvalidValue { field, .. }
                        if field == "data_types"
                            || field == "record_types"
                            || field == "instrument_types"
                            || field == "record_filters"
                )));
            }
            error => panic!("Expected multiple config errors, received {error:?}"),
        }
    }

    #[rstest]
    fn streaming_config_toml_round_trip() {
        let config: StreamingConfig = toml::from_str(
            r#"
catalog_path = "/data/catalog"
fs_protocol = "file"
flush_interval_ms = 1000
replace_existing = false

[rotation_config.size]
max_size = 1048576
"#,
        )
        .unwrap();

        assert_eq!(config.catalog_path, "/data/catalog");
        assert_eq!(config.fs_protocol, "file");
        assert_eq!(config.flush_interval_ms, 1000);
        assert!(!config.replace_existing);
        assert_eq!(config.params, None);
        assert!(matches!(
            config.rotation_config,
            RotationConfig::Size {
                max_size: 1_048_576
            }
        ));
    }

    #[rstest]
    fn streaming_config_with_no_rotation_toml() {
        let config: StreamingConfig = toml::from_str(
            r#"
catalog_path = "/data/catalog"
fs_protocol = "file"
flush_interval_ms = 500
replace_existing = true
rotation_config = "no_rotation"
"#,
        )
        .unwrap();

        assert!(matches!(config.rotation_config, RotationConfig::NoRotation));
        assert!(config.replace_existing);
    }
}
