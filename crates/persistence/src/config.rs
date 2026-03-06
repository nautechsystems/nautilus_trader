// Under development
#![allow(dead_code)]
#![allow(unused_variables)]

use crate::backend::feather::RotationConfig;

/// Configuration for streaming live or backtest runs to the catalog in feather format.
#[derive(Debug, Clone)]
pub struct StreamingConfig {
    /// The path to the data catalog.
    catalog_path: String,
    /// The `fsspec` filesystem protocol for the catalog.
    fst_protocol: String,
    /// The flush interval (milliseconds) for writing chunks.
    flush_interval_ms: u64,
    /// If any existing feather files should be replaced.
    replace_existing: bool,
    /// Rotation config
    rotation_config: RotationConfig,
}

impl StreamingConfig {
    /// Create a new streaming configuration.
    #[must_use]
    pub const fn new(
        catalog_path: String,
        fst_protocol: String,
        flush_interval_ms: u64,
        replace_existing: bool,
        rotation_config: RotationConfig,
    ) -> Self {
        Self {
            catalog_path,
            fst_protocol,
            flush_interval_ms,
            replace_existing,
            rotation_config,
        }
    }
}

/// Configuration for a data catalog.
pub struct DataCatalogConfig {
    /// The path to the data catalog.
    path: String,
    /// The fsspec file system protocol for the data catalog.
    fs_protocol: String,
}

impl DataCatalogConfig {
    /// Create a new data catalog configuration.
    #[must_use]
    pub const fn new(path: String, fs_protocol: String) -> Self {
        Self { path, fs_protocol }
    }
}
