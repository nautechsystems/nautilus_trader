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
use nautilus_core::{DurationNanos, Params, UnixNanos, datetime::get_timezone};
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

/// Compression codec for the data files a catalog writes.
///
/// Displays and serializes as the lowercase codec name, such as `zstd`; `FromStr` ignores ASCII
/// case. `lz4` writes Parquet `LZ4_RAW` and also parses from `lz4_raw`. LZO has no variant
/// because the Parquet writer cannot produce LZO files.
#[derive(
    Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Display, EnumIter, EnumString,
)]
#[serde(rename_all = "snake_case")]
#[strum(ascii_case_insensitive)]
#[strum(serialize_all = "snake_case")]
pub enum CatalogCompression {
    Uncompressed,
    Snappy,
    Gzip,
    Brotli,
    #[serde(alias = "lz4_raw")]
    #[strum(to_string = "lz4", serialize = "lz4_raw")]
    Lz4,
    Zstd,
}

/// Configuration for a catalog available to request-time historical data loading.
#[cfg_attr(
    feature = "python",
    expect(
        clippy::unsafe_derive_deserialize,
        reason = "config deserializes plain fields; unsafe methods come from generated PyO3 integration"
    )
)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, bon::Builder)]
#[builder(finish_fn(name = build_inner, vis = ""))]
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
    /// The number of rows per batch the catalog reads and writes; omit for the backend default.
    batch_size: Option<usize>,
    /// The compression codec of written data files; omit for the backend default.
    compression: Option<CatalogCompression>,
    /// The maximum number of rows per written row group; omit for the backend default.
    max_row_group_size: Option<usize>,
    /// Backend-specific catalog parameters.
    params: Option<Params>,
    #[serde(default)]
    fs_rust_storage_options: Option<ahash::AHashMap<String, String>>,
    /// Whether the catalog rejects response write-back.
    #[serde(default)]
    #[builder(default)]
    read_only: bool,
}

impl<S: data_catalog_config_builder::IsComplete> DataCatalogConfigBuilder<S> {
    /// Validates and builds the [`DataCatalogConfig`].
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if any field fails validation
    /// (see [`DataCatalogConfig::validate`]).
    pub fn build(self) -> ConfigResult<DataCatalogConfig> {
        let config = self.build_inner();
        config.validate()?;
        Ok(config)
    }
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
            batch_size: None,
            compression: None,
            max_row_group_size: None,
            params: None,
            fs_rust_storage_options: None,
            read_only: false,
        }
    }

    /// Validates the catalog configuration, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if `batch_size` or `max_row_group_size` is zero.
    pub fn validate(&self) -> ConfigResult<()> {
        validate_catalog_counts(self.batch_size, self.max_row_group_size)
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

    /// Returns the connection settings catalog and writer factories open this catalog with.
    #[must_use]
    pub fn connect_config(&self) -> CatalogConnectConfig {
        let mut connect = CatalogConnectConfig::from_path_and_protocol(
            &self.path,
            Some(&self.fs_protocol),
            self.fs_rust_storage_options.clone(),
        );
        connect.batch_size = self.batch_size;
        connect.compression = self.compression;
        connect.max_row_group_size = self.max_row_group_size;
        connect.params.clone_from(&self.params);
        connect
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

    /// Returns the number of rows per batch the catalog reads and writes.
    #[must_use]
    pub const fn batch_size(&self) -> Option<usize> {
        self.batch_size
    }

    /// Returns the compression codec of written data files.
    #[must_use]
    pub const fn compression(&self) -> Option<CatalogCompression> {
        self.compression
    }

    /// Returns the maximum number of rows per written row group.
    #[must_use]
    pub const fn max_row_group_size(&self) -> Option<usize> {
        self.max_row_group_size
    }

    /// Returns backend-specific catalog parameters.
    #[must_use]
    pub const fn params(&self) -> Option<&Params> {
        self.params.as_ref()
    }

    #[must_use]
    pub(crate) fn writer_backend(&self) -> WriterBackendType {
        match &self.catalog_backend {
            CatalogBackendType::Parquet => WriterBackendType::Parquet,
            CatalogBackendType::External(name) => WriterBackendType::External(name.clone()),
        }
    }
}

// The Parquet factory checks the counts too, since deserialized configs skip `validate`
pub(crate) fn validate_catalog_counts(
    batch_size: Option<usize>,
    max_row_group_size: Option<usize>,
) -> ConfigResult<()> {
    let mut errors = ConfigErrorCollector::new();

    for (field, count) in [
        ("batch_size", batch_size),
        ("max_row_group_size", max_row_group_size),
    ] {
        errors.check(
            count != Some(0),
            ConfigError::range(
                field,
                "must be a positive number of rows; omit the field for the backend default",
            ),
        );
    }

    errors.into_result()
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
        /// Time of day for rotation, in nanoseconds since midnight in `timezone`.
        schedule_ns: UnixNanos,
        /// IANA timezone name for the rotation schedule.
        #[serde(default = "default_rotation_timezone")]
        timezone: String,
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

    /// Validates the rotation parameters, collecting every field violation.
    ///
    /// # Errors
    ///
    /// Returns a [`ConfigError`] if a size or interval is zero or the timezone is unknown.
    pub fn validate(&self) -> ConfigResult<()> {
        let mut errors = ConfigErrorCollector::new();

        match self {
            Self::Size { max_size } => errors.check(
                *max_size > 0,
                ConfigError::range(
                    "rotation_config.max_size",
                    "must be a positive number of bytes",
                ),
            ),
            Self::Interval { interval_ns } => {
                errors.check(interval_ns.as_u64() > 0, positive_interval_error());
            }
            Self::ScheduledDates {
                interval_ns,
                timezone,
                ..
            } => {
                errors.check(interval_ns.as_u64() > 0, positive_interval_error());
                errors.check(
                    get_timezone(timezone).is_ok(),
                    ConfigError::invalid_value(
                        "rotation_config.timezone",
                        format!("unknown IANA timezone, was {timezone}"),
                    ),
                );
            }
            Self::NoRotation => {}
        }

        errors.into_result()
    }

    /// Converts the public streaming configuration into the writer's runtime form.
    ///
    /// # Errors
    ///
    /// Returns an error if a scheduled rotation names an unknown timezone.
    pub fn to_writer_rotation_config(
        &self,
    ) -> anyhow::Result<crate::writer::feather::RotationConfig> {
        Ok(match self {
            Self::Size { max_size } => crate::writer::feather::RotationConfig::Size {
                max_size: *max_size,
            },
            Self::Interval { interval_ns } => crate::writer::feather::RotationConfig::Interval {
                interval_ns: interval_ns.as_u64(),
            },
            Self::ScheduledDates {
                interval_ns,
                schedule_ns,
                timezone,
            } => crate::writer::feather::RotationConfig::ScheduledDates {
                interval_ns: interval_ns.as_u64(),
                rotation_time: *schedule_ns,
                rotation_timezone: get_timezone(timezone)
                    .map_err(|e| anyhow::anyhow!("Invalid rotation timezone '{timezone}': {e}"))?,
            },
            Self::NoRotation => crate::writer::feather::RotationConfig::NoRotation,
        })
    }
}

/// The rotation policy of a streaming writer, without its parameters.
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Display, FromRepr, EnumIter, EnumString)]
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

fn positive_interval_error() -> ConfigError {
    ConfigError::range(
        "rotation_config.interval_ns",
        "must be a positive number of nanoseconds",
    )
}

pub(crate) const DEFAULT_ROTATION_TIMEZONE: &str = "UTC";

fn default_rotation_timezone() -> String {
    DEFAULT_ROTATION_TIMEZONE.to_string()
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
/// `{backtest|sandbox|live}/{instance_id}` directory per run. With a `catalog`, the writer for
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
    /// Flush interval in milliseconds.
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
        errors.collect(self.rotation_config.validate());

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
                    ConfigError::dependency(field, "catalog", "promotion needs a catalog"),
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
        params.insert("snapshot_id".to_string(), json!(1024));
        let config = DataCatalogConfig::new(
            "/data/catalog".to_string(),
            Some("file".to_string()),
            Some(CatalogBackendType::Parquet),
        )
        .with_params(Some(params));

        assert_eq!(
            config.params().and_then(|p| p.get_u64("snapshot_id")),
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
        assert_eq!(config.batch_size(), None);
        assert_eq!(config.compression(), None);
        assert_eq!(config.max_row_group_size(), None);
        assert_eq!(config.params(), None);
        assert!(!config.read_only());
    }

    #[rstest]
    fn data_catalog_config_toml_reads_typed_settings() {
        let config: DataCatalogConfig = toml::from_str(
            r#"
path = "/data/catalog"
batch_size = 512
compression = "gzip"
max_row_group_size = 2048
"#,
        )
        .unwrap();

        assert_eq!(config.batch_size(), Some(512));
        assert_eq!(config.compression(), Some(CatalogCompression::Gzip));
        assert_eq!(config.max_row_group_size(), Some(2048));
    }

    #[rstest]
    #[case::lzo("lzo")]
    #[case::unknown("zip")]
    #[case::numeric_code("3")]
    fn data_catalog_config_toml_rejects_unsupported_compression(#[case] compression: &str) {
        let error = toml::from_str::<DataCatalogConfig>(&format!(
            "path = \"/data/catalog\"\ncompression = \"{compression}\"\n"
        ))
        .unwrap_err();

        assert!(
            error.to_string().contains(&format!(
                "unknown variant `{compression}`, expected one of `uncompressed`, `snappy`, \
                 `gzip`, `brotli`, `lz4`, `lz4_raw`, `zstd`"
            )),
            "{error}"
        );
    }

    #[rstest]
    #[case::uncompressed("uncompressed", CatalogCompression::Uncompressed)]
    #[case::snappy("snappy", CatalogCompression::Snappy)]
    #[case::gzip("gzip", CatalogCompression::Gzip)]
    #[case::brotli("brotli", CatalogCompression::Brotli)]
    #[case::lz4("lz4", CatalogCompression::Lz4)]
    #[case::zstd("zstd", CatalogCompression::Zstd)]
    fn catalog_compression_round_trips_codec_name(
        #[case] name: &str,
        #[case] compression: CatalogCompression,
    ) {
        assert_eq!(name.parse::<CatalogCompression>().unwrap(), compression);
        assert_eq!(
            name.to_ascii_uppercase()
                .parse::<CatalogCompression>()
                .unwrap(),
            compression
        );
        assert_eq!(compression.to_string(), name);
    }

    #[rstest]
    fn catalog_compression_accepts_lz4_raw_alias() {
        let config: DataCatalogConfig = toml::from_str(
            r#"
path = "/data/catalog"
compression = "lz4_raw"
"#,
        )
        .unwrap();

        assert_eq!(
            "LZ4_RAW".parse::<CatalogCompression>().unwrap(),
            CatalogCompression::Lz4
        );
        assert_eq!(config.compression(), Some(CatalogCompression::Lz4));
        assert_eq!(
            serde_json::to_string(&CatalogCompression::Lz4).unwrap(),
            "\"lz4\""
        );
    }

    #[rstest]
    #[case::lzo("lzo")]
    #[case::unknown("zip")]
    fn catalog_compression_rejects_unsupported_codec(#[case] name: &str) {
        assert_eq!(
            name.parse::<CatalogCompression>(),
            Err(strum::ParseError::VariantNotFound)
        );
    }

    #[rstest]
    fn data_catalog_config_builder_sets_typed_settings() {
        let config = DataCatalogConfig::builder()
            .path("/data/catalog".to_string())
            .batch_size(512)
            .compression(CatalogCompression::Snappy)
            .max_row_group_size(2048)
            .build()
            .unwrap();

        assert_eq!(config.batch_size(), Some(512));
        assert_eq!(config.compression(), Some(CatalogCompression::Snappy));
        assert_eq!(config.max_row_group_size(), Some(2048));
    }

    #[rstest]
    fn data_catalog_config_builder_rejects_zero_counts() {
        let error = DataCatalogConfig::builder()
            .path("/data/catalog".to_string())
            .batch_size(0)
            .max_row_group_size(0)
            .build()
            .unwrap_err();

        assert_eq!(
            error,
            ConfigError::multiple(vec![
                ConfigError::range(
                    "batch_size",
                    "must be a positive number of rows; omit the field for the backend default",
                ),
                ConfigError::range(
                    "max_row_group_size",
                    "must be a positive number of rows; omit the field for the backend default",
                ),
            ])
        );
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
    fn data_catalog_config_connect_config_carries_protocol_options_settings_and_params() {
        let mut params = Params::new();
        params.insert("snapshot_id".to_string(), json!(1024));
        let options = ahash::AHashMap::from([("region".to_string(), "eu-west-1".to_string())]);
        let config = DataCatalogConfig::builder()
            .path("bucket/catalog".to_string())
            .fs_protocol("s3".to_string())
            .batch_size(512)
            .compression(CatalogCompression::Brotli)
            .max_row_group_size(2048)
            .params(params.clone())
            .fs_rust_storage_options(options.clone())
            .build()
            .unwrap();

        let connect = config.connect_config();

        assert_eq!(connect.uri, "s3://bucket/catalog");
        assert_eq!(connect.storage_options, Some(options));
        assert_eq!(connect.batch_size, Some(512));
        assert_eq!(connect.compression, Some(CatalogCompression::Brotli));
        assert_eq!(connect.max_row_group_size, Some(2048));
        assert_eq!(connect.params, Some(params));
    }

    #[rstest]
    #[case(CatalogBackendType::Parquet, WriterBackendType::Parquet)]
    #[case(
        CatalogBackendType::External("DuckLake".to_string()),
        WriterBackendType::External("DuckLake".to_string())
    )]
    fn data_catalog_config_writer_backend_follows_catalog_backend(
        #[case] catalog_backend: CatalogBackendType,
        #[case] expected: WriterBackendType,
    ) {
        let config =
            DataCatalogConfig::new("/data/catalog".to_string(), None, Some(catalog_backend));

        assert_eq!(config.writer_backend(), expected);
    }

    fn streaming_config(catalog: Option<DataCatalogConfig>) -> StreamingConfig {
        StreamingConfig::new(
            "/data/stream".to_string(),
            catalog,
            1_000,
            false,
            RotationConfig::NoRotation,
        )
    }

    fn local_catalog() -> DataCatalogConfig {
        DataCatalogConfig::new("/data/catalog".to_string(), None, None)
    }

    #[rstest]
    fn streaming_config_builder_defaults_promotion_options() {
        let config = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .build()
            .unwrap();

        assert_eq!(config.writer_path, "/data/stream");
        assert!(config.catalog.is_none());
        assert_eq!(config.promotion_interval_ms, None);
        assert!(config.promote_on_close);
        assert!(!config.delete_feather_after_promotion);
        assert!(!config.use_ts_event_for_ts_init);
        assert_eq!(config.writer_backend(), WriterBackendType::Feather);
    }

    #[rstest]
    fn streaming_config_builder_preserves_promotion_options_and_params() {
        let mut params = Params::new();
        params.insert("batch_size".to_string(), json!(512));
        let config = StreamingConfig::builder()
            .writer_path("/data/stream".to_string())
            .catalog(local_catalog())
            .flush_interval_ms(1_000)
            .replace_existing(false)
            .rotation_config(RotationConfig::NoRotation)
            .promotion_interval_ms(5_000)
            .promote_on_close(false)
            .delete_feather_after_promotion(true)
            .use_ts_event_for_ts_init(true)
            .params(params)
            .build()
            .unwrap();

        assert_eq!(config.catalog.as_ref().unwrap().path(), "/data/catalog");
        assert_eq!(config.promotion_interval_ms, Some(5_000));
        assert!(!config.promote_on_close);
        assert!(config.delete_feather_after_promotion);
        assert!(config.use_ts_event_for_ts_init);
        assert_eq!(
            config.params.as_ref().and_then(|p| p.get_u64("batch_size")),
            Some(512)
        );
        assert_eq!(config.writer_backend(), WriterBackendType::Parquet);
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
    #[case("s3://bucket/stream")]
    #[case("gs://bucket/stream")]
    fn streaming_config_remote_writer_path_rejected(#[case] writer_path: &str) {
        let mut config = streaming_config(None);
        config.writer_path = writer_path.to_string();

        assert_eq!(
            config.validate().unwrap_err().to_string(),
            format!(
                "invalid writer_path: must be a local path because streaming appends to local \
                 files, was {writer_path}"
            ),
        );
    }

    #[rstest]
    fn streaming_config_accepts_remote_catalog() {
        let config = streaming_config(Some(DataCatalogConfig::new(
            "bucket/catalog".to_string(),
            Some("s3".to_string()),
            None,
        )));

        config.validate().unwrap();
        assert_eq!(
            config.catalog.unwrap().connect_config().uri,
            "s3://bucket/catalog"
        );
    }

    #[rstest]
    fn streaming_config_promotion_options_require_catalog() {
        let mut config = streaming_config(None);
        config.promotion_interval_ms = Some(1_000);
        config.delete_feather_after_promotion = true;
        config.use_ts_event_for_ts_init = true;

        assert_eq!(
            config.validate().unwrap_err().to_string(),
            "multiple config validation errors: \
             1. promotion_interval_ms requires catalog: promotion needs a catalog; \
             2. delete_feather_after_promotion requires catalog: promotion needs a catalog; \
             3. use_ts_event_for_ts_init requires catalog: promotion needs a catalog",
        );
    }

    #[rstest]
    fn streaming_config_zero_promotion_interval_rejected() {
        let mut config = streaming_config(Some(local_catalog()));
        config.promotion_interval_ms = Some(0);

        assert!(
            matches!(config.validate(), Err(ConfigError::Range { field, .. }) if field == "promotion_interval_ms")
        );
    }

    #[rstest]
    #[case(RotationConfig::Size { max_size: 0 }, "rotation_config.max_size")]
    #[case(
        RotationConfig::Interval { interval_ns: DurationNanos::new(0) },
        "rotation_config.interval_ns"
    )]
    #[case(
        RotationConfig::ScheduledDates {
            interval_ns: DurationNanos::new(0),
            schedule_ns: UnixNanos::default(),
            timezone: "UTC".to_string(),
        },
        "rotation_config.interval_ns"
    )]
    #[case(
        RotationConfig::ScheduledDates {
            interval_ns: DurationNanos::new(1),
            schedule_ns: UnixNanos::default(),
            timezone: "Mars/Olympus_Mons".to_string(),
        },
        "rotation_config.timezone"
    )]
    fn streaming_config_invalid_rotation_rejected(
        #[case] rotation_config: RotationConfig,
        #[case] expected_field: &str,
    ) {
        let mut config = streaming_config(None);
        config.rotation_config = rotation_config;

        assert!(matches!(
            config.validate(),
            Err(ConfigError::Range { field, .. } | ConfigError::InvalidValue { field, .. })
                if field == expected_field
        ));
    }

    #[rstest]
    fn rotation_config_converts_to_writer_form() {
        let cases = [
            (
                RotationConfig::Size { max_size: 17 },
                "Size { max_size: 17 }",
            ),
            (
                RotationConfig::Interval {
                    interval_ns: DurationNanos::new(23),
                },
                "Interval { interval_ns: 23 }",
            ),
            (RotationConfig::NoRotation, "NoRotation"),
        ];

        for (config, expected) in cases {
            assert_eq!(
                format!("{:?}", config.to_writer_rotation_config().unwrap()),
                expected
            );
        }
    }

    #[rstest]
    fn rotation_config_scheduled_dates_keeps_timezone() {
        let config = RotationConfig::ScheduledDates {
            interval_ns: DurationNanos::new(31),
            schedule_ns: UnixNanos::from(37),
            timezone: "Australia/Sydney".to_string(),
        };

        let crate::writer::feather::RotationConfig::ScheduledDates {
            interval_ns,
            rotation_time,
            rotation_timezone,
        } = config.to_writer_rotation_config().unwrap()
        else {
            panic!("expected scheduled rotation")
        };

        assert_eq!(interval_ns, 31);
        assert_eq!(rotation_time, UnixNanos::from(37));
        assert_eq!(rotation_timezone.iana_name(), Some("Australia/Sydney"));
    }

    #[rstest]
    #[case::size(RotationConfig::Size { max_size: 1 }, RotationMode::Size)]
    #[case::interval(
        RotationConfig::Interval {
            interval_ns: DurationNanos::new(1),
        },
        RotationMode::Interval,
    )]
    #[case::scheduled_dates(
        RotationConfig::ScheduledDates {
            interval_ns: DurationNanos::new(1),
            schedule_ns: UnixNanos::from(1),
            timezone: DEFAULT_ROTATION_TIMEZONE.to_string(),
        },
        RotationMode::ScheduledDates,
    )]
    #[case::no_rotation(RotationConfig::NoRotation, RotationMode::NoRotation)]
    fn rotation_config_mode_names_its_policy(
        #[case] config: RotationConfig,
        #[case] expected: RotationMode,
    ) {
        assert_eq!(config.mode(), expected);
    }

    #[rstest]
    fn rotation_config_unknown_timezone_fails_conversion() {
        let config = RotationConfig::ScheduledDates {
            interval_ns: DurationNanos::new(31),
            schedule_ns: UnixNanos::from(37),
            timezone: "Mars/Olympus_Mons".to_string(),
        };

        assert!(
            config
                .to_writer_rotation_config()
                .unwrap_err()
                .to_string()
                .starts_with("Invalid rotation timezone 'Mars/Olympus_Mons'")
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

[catalog]
path = "bucket/catalog"
fs_protocol = "s3"

[rotation_config.size]
max_size = 1048576
"#,
        )
        .unwrap();

        assert_eq!(config.writer_path, "/data/stream");
        assert_eq!(config.catalog.as_ref().unwrap().path(), "bucket/catalog");
        assert_eq!(config.catalog.as_ref().unwrap().fs_protocol(), "s3");
        assert_eq!(config.flush_interval_ms, 1000);
        assert!(!config.replace_existing);
        assert_eq!(config.promotion_interval_ms, Some(5_000));
        assert!(config.promote_on_close);
        assert!(!config.delete_feather_after_promotion);
        assert!(!config.use_ts_event_for_ts_init);
        assert_eq!(config.params, None);
        assert!(matches!(
            config.rotation_config,
            RotationConfig::Size {
                max_size: 1_048_576
            }
        ));
    }

    #[rstest]
    fn streaming_config_scheduled_dates_toml_defaults_timezone_to_utc() {
        let config: StreamingConfig = toml::from_str(
            r#"
writer_path = "/data/stream"
flush_interval_ms = 1000
replace_existing = false

[rotation_config.scheduled_dates]
interval_ns = 86400000000000
schedule_ns = 3600000000000
"#,
        )
        .unwrap();

        let RotationConfig::ScheduledDates {
            interval_ns,
            schedule_ns,
            timezone,
        } = config.rotation_config
        else {
            panic!("expected scheduled rotation")
        };

        assert_eq!(interval_ns.as_u64(), 86_400_000_000_000);
        assert_eq!(schedule_ns, UnixNanos::from(3_600_000_000_000));
        assert_eq!(timezone, "UTC");
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
    }
}
