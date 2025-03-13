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
