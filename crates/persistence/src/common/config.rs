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
use nautilus_core::{Params, UnixNanos};
use nautilus_model::{
    data::{NautilusDataType, NautilusRecordType},
    instruments::NautilusInstrumentType,
};
use serde::{Deserialize, Serialize};
use strum::{Display, EnumIter, EnumString, FromRepr};

use crate::{
    catalog::factory::{CatalogConnectConfig, PARQUET_CATALOG_FACTORY_NAME},
    common::{backend_name::backend_type, paths::local_writer_directory},
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
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.persistence", from_py_object)
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
            read_only: false,
        }
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

    /// Returns the connection settings catalog and writer factories open this catalog with.
    #[must_use]
    pub fn connect_config(&self) -> CatalogConnectConfig {
        let fs_protocol = match self.fs_protocol.as_str() {
            "file" => None,
            protocol => Some(protocol),
        };
        let mut connect =
            CatalogConnectConfig::from_path_and_protocol(&self.path, fs_protocol, None);
        connect.params.clone_from(&self.params);
        connect
    }

    /// Returns the streaming writer backend that promotes into this catalog.
    #[must_use]
    pub fn writer_backend(&self) -> WriterBackendType {
        match &self.catalog_backend {
            CatalogBackendType::Parquet => WriterBackendType::Parquet,
            CatalogBackendType::External(name) => WriterBackendType::External(name.clone()),
        }
    }
}

/// The rotation policy of a streaming writer, without its parameters.
#[repr(C)]
#[derive(Clone, Copy, Debug, Display, Eq, Hash, PartialEq, FromRepr, EnumIter, EnumString)]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "SCREAMING_SNAKE_CASE")]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        frozen,
        eq,
        eq_int,
        module = "nautilus_trader.persistence",
        from_py_object,
        rename_all = "SCREAMING_SNAKE_CASE",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass_enum(module = "nautilus_trader.persistence")
)]
pub enum RotationMode {
    Size,
    Interval,
    ScheduledDates,
    NoRotation,
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
        interval_ns: u64,
    },
    /// Rotate based on scheduled dates.
    ScheduledDates {
        /// Interval in nanoseconds.
        interval_ns: u64,
        /// Start of the scheduled rotation period.
        schedule_ns: UnixNanos,
    },
    /// No automatic rotation.
    NoRotation,
}

impl RotationConfig {
    /// Returns the rotation policy without its parameters.
    #[must_use]
    pub const fn mode(&self) -> RotationMode {
        match self {
            Self::Size { .. } => RotationMode::Size,
            Self::Interval { .. } => RotationMode::Interval,
            Self::ScheduledDates { .. } => RotationMode::ScheduledDates,
            Self::NoRotation => RotationMode::NoRotation,
        }
    }

    /// Converts the public streaming configuration into the writer's runtime form.
    #[must_use]
    pub fn to_writer_rotation_config(&self) -> crate::writer::feather::RotationConfig {
        match self {
            Self::Size { max_size } => crate::writer::feather::RotationConfig::Size {
                max_size: *max_size,
            },
            Self::Interval { interval_ns } => crate::writer::feather::RotationConfig::Interval {
                interval_ns: *interval_ns,
            },
            Self::ScheduledDates {
                interval_ns,
                schedule_ns,
            } => crate::writer::feather::RotationConfig::scheduled_utc(*interval_ns, *schedule_ns),
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
///
/// The writer appends Feather files under the local `writer_path`, in one
/// `{backtest|live|sandbox}/{instance_id}` directory per run. With a `catalog`, the writer for
/// that catalog's backend also promotes the files into it; without one, the Feather files are the
/// only output.
#[expect(
    clippy::struct_excessive_bools,
    reason = "the booleans are independent writer lifecycle and promotion options"
)]
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(frozen, module = "nautilus_trader.persistence", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.persistence")
)]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
#[serde(deny_unknown_fields)]
pub struct StreamingConfig {
    /// Local directory the writer appends Feather files to.
    pub writer_path: String,
    /// Catalog that receives promoted data; omit to keep only the Feather files.
    pub catalog: Option<DataCatalogConfig>,
    /// Interval in milliseconds for flushing open files to disk.
    pub flush_interval_ms: u64,
    /// Whether to replace existing files.
    pub replace_existing: bool,
    /// Rotation configuration.
    pub rotation_config: RotationConfig,
    /// Interval in milliseconds for promoting sealed files into `catalog`; omit for no interval.
    pub promotion_interval_ms: Option<u64>,
    /// Whether closing the writer promotes remaining files into `catalog`.
    #[serde(default = "default_promote_on_close")]
    #[builder(default = default_promote_on_close())]
    pub promote_on_close: bool,
    /// Whether Feather files are deleted after a successful promotion.
    #[serde(default)]
    #[builder(default)]
    pub delete_feather_after_promotion: bool,
    /// Whether promotion replaces `ts_init` with `ts_event`.
    #[serde(default)]
    #[builder(default)]
    pub use_ts_event_for_ts_init: bool,
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
        writer_path: String,
        catalog: Option<DataCatalogConfig>,
        flush_interval_ms: u64,
        replace_existing: bool,
        rotation_config: RotationConfig,
    ) -> Self {
        Self {
            writer_path,
            catalog,
            flush_interval_ms,
            replace_existing,
            rotation_config,
            promotion_interval_ms: None,
            promote_on_close: default_promote_on_close(),
            delete_feather_after_promotion: false,
            use_ts_event_for_ts_init: false,
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

        if self.writer_path.trim().is_empty() {
            errors.push(ConfigError::empty_field("writer_path"));
        } else {
            errors.check(
                local_writer_directory(&self.writer_path).is_ok(),
                ConfigError::invalid_value(
                    "writer_path",
                    format!(
                        "must be a local path because streaming appends to local files, was {}",
                        self.writer_path
                    ),
                ),
            );
        }

        let flush_interval_ms = self.flush_interval_ms;
        errors.check(
            flush_interval_ms > 0,
            ConfigError::range(
                "flush_interval_ms",
                format!("must be a positive number of milliseconds, was {flush_interval_ms}"),
            ),
        );

        if let Some(promotion_interval_ms) = self.promotion_interval_ms {
            errors.check(
                promotion_interval_ms > 0,
                ConfigError::range(
                    "promotion_interval_ms",
                    "must be a positive number of milliseconds; omit the field for no interval",
                ),
            );
        }

        if self.catalog.is_none() {
            for (field, is_set) in [
                (
                    "promotion_interval_ms",
                    self.promotion_interval_ms.is_some(),
                ),
                (
                    "delete_feather_after_promotion",
                    self.delete_feather_after_promotion,
                ),
                ("use_ts_event_for_ts_init", self.use_ts_event_for_ts_init),
            ] {
                errors.check(
                    !is_set,
                    ConfigError::invalid_value(field, "requires a catalog to promote into"),
                );
            }
        }

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

    /// Returns the writer backend: `Feather` without a catalog, otherwise the catalog's backend.
    #[must_use]
    pub fn writer_backend(&self) -> WriterBackendType {
        self.catalog.as_ref().map_or(
            WriterBackendType::Feather,
            DataCatalogConfig::writer_backend,
        )
    }
}

const fn default_promote_on_close() -> bool {
    true
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
            .writer_path("/data/stream".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(config.is_ok());
    }

    #[rstest]
    fn streaming_config_builder_preserves_backend_params() {
        let mut params = Params::new();
        params.insert("maintenance_interval_ms".to_string(), json!(60_000));
        let config = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
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
                .and_then(|p| p.get_u64("maintenance_interval_ms")),
            Some(60_000)
        );
    }

    #[rstest]
    fn streaming_config_zero_flush_interval_rejected() {
        let result = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .flush_interval_ms(0)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "flush_interval_ms")
        );
    }

    #[rstest]
    fn streaming_config_empty_writer_path_rejected() {
        let result = StreamingConfig::builder()
            .writer_path(String::new())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(matches!(result, Err(ConfigError::EmptyField { field }) if field == "writer_path"));
    }

    #[rstest]
    #[case::s3("s3://bucket/stream")]
    #[case::memory("memory://stream")]
    fn streaming_config_remote_writer_path_rejected(#[case] writer_path: &str) {
        let result = StreamingConfig::builder()
            .writer_path(writer_path.to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build();

        assert!(
            matches!(result, Err(ConfigError::InvalidValue { field, .. }) if field == "writer_path")
        );
    }

    #[rstest]
    #[case::feather(None, WriterBackendType::Feather)]
    #[case::parquet(Some(CatalogBackendType::Parquet), WriterBackendType::Parquet)]
    #[case::external(
        Some(CatalogBackendType::External("CustomSink".to_string())),
        WriterBackendType::External("CustomSink".to_string())
    )]
    fn streaming_config_writer_backend_follows_catalog(
        #[case] catalog_backend: Option<CatalogBackendType>,
        #[case] expected: WriterBackendType,
    ) {
        let catalog = catalog_backend.map(|backend| {
            DataCatalogConfig::new("/data/catalog".to_string(), None, Some(backend))
        });
        let config = StreamingConfig::new(
            "/data/stream".to_string(),
            catalog,
            1_000,
            false,
            RotationConfig::NoRotation,
        );

        assert_eq!(config.writer_backend(), expected);
    }

    #[rstest]
    fn data_catalog_config_connect_config_applies_protocol_and_params() {
        let mut params = Params::new();
        params.insert("metadata_url".to_string(), json!("postgres://meta"));
        let config =
            DataCatalogConfig::new("bucket/catalog".to_string(), Some("s3".to_string()), None)
                .with_params(Some(params.clone()));
        let local = DataCatalogConfig::new("/data/catalog".to_string(), None, None);

        let connect = config.connect_config();
        let local_connect = local.connect_config();

        assert_eq!(connect.uri, "s3://bucket/catalog");
        assert_eq!(connect.params, Some(params));
        assert_eq!(connect.storage_options, None);
        assert_eq!(local_connect.uri, "/data/catalog");
        assert_eq!(local_connect.params, None);
    }

    #[rstest]
    fn streaming_config_promotion_defaults() {
        let config = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build()
            .unwrap();

        assert_eq!(config.promotion_interval_ms, None);
        assert!(config.promote_on_close);
        assert!(!config.delete_feather_after_promotion);
        assert!(!config.use_ts_event_for_ts_init);
    }

    #[rstest]
    fn streaming_config_promotion_settings_require_catalog() {
        let result = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .promotion_interval_ms(1_000)
            .delete_feather_after_promotion(true)
            .use_ts_event_for_ts_init(true)
            .build();

        match result.unwrap_err() {
            ConfigError::Multiple { errors } => {
                let fields = errors
                    .iter()
                    .map(|e| match e {
                        ConfigError::InvalidValue { field, .. } => field.as_str(),
                        error => panic!("Expected invalid value, received {error:?}"),
                    })
                    .collect::<Vec<_>>();
                assert_eq!(
                    fields,
                    vec![
                        "promotion_interval_ms",
                        "delete_feather_after_promotion",
                        "use_ts_event_for_ts_init",
                    ]
                );
            }
            error => panic!("Expected multiple config errors, received {error:?}"),
        }
    }

    #[rstest]
    fn streaming_config_zero_promotion_interval_rejected() {
        let result = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .catalog(DataCatalogConfig::new(
                "/data/catalog".to_string(),
                None,
                None,
            ))
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .promotion_interval_ms(0)
            .build();

        assert!(
            matches!(result, Err(ConfigError::Range { field, .. }) if field == "promotion_interval_ms")
        );
    }

    #[rstest]
    fn streaming_config_empty_filter_lists_rejected() {
        let result = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
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
writer_path = "/data/stream"
flush_interval_ms = 1000
replace_existing = false

promotion_interval_ms = 5000
promote_on_close = false
delete_feather_after_promotion = true
use_ts_event_for_ts_init = true

[catalog]
path = "/data/catalog"
catalog_backend = "Parquet"

[rotation_config.size]
max_size = 1048576
"#,
        )
        .unwrap();

        assert_eq!(config.writer_path, "/data/stream");
        let catalog = config.catalog.as_ref().unwrap();
        assert_eq!(catalog.path(), "/data/catalog");
        assert_eq!(catalog.catalog_backend(), &CatalogBackendType::Parquet);
        assert_eq!(config.writer_backend(), WriterBackendType::Parquet);
        assert_eq!(config.promotion_interval_ms, Some(5_000));
        assert!(!config.promote_on_close);
        assert!(config.delete_feather_after_promotion);
        assert!(config.use_ts_event_for_ts_init);
        assert!(config.validate().is_ok());
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
writer_path = "/data/stream"
flush_interval_ms = 500
replace_existing = true
rotation_config = "no_rotation"
"#,
        )
        .unwrap();

        assert!(matches!(config.rotation_config, RotationConfig::NoRotation));
        assert!(config.replace_existing);
        assert!(config.catalog.is_none());
        assert_eq!(config.writer_backend(), WriterBackendType::Feather);
    }
}
