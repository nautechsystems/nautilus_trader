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

use parquet::basic::{Compression, ZstdLevel};
use serde::{Deserialize, Serialize};

use crate::enums::{CryptoHFTDataExchange, CryptoHFTDataType, GapPolicy};

/// Default CryptoHFTData API base URL.
pub const CRYPTOHFTDATA_BASE_URL: &str = "https://api.cryptohftdata.com";

/// Default environment variable for the CHD API key.
pub const CRYPTOHFTDATA_API_KEY_ENV: &str = "CRYPTOHFTDATA_API_KEY";

/// Determines the compression codec for Parquet files written by CHD ingest.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ParquetCompression {
    /// Use Zstandard compression with level 3.
    #[default]
    Zstd,
    /// Use Snappy compression.
    Snappy,
    /// Write uncompressed Parquet files.
    Uncompressed,
}

impl ParquetCompression {
    /// Converts this config value to a Parquet compression value.
    #[must_use]
    pub fn as_parquet_compression(&self) -> Compression {
        match self {
            Self::Zstd => Compression::ZSTD(ZstdLevel::try_new(3).expect("zstd level 3 is valid")),
            Self::Snappy => Compression::SNAPPY,
            Self::Uncompressed => Compression::UNCOMPRESSED,
        }
    }
}

/// Configuration for a CHD HTTP client.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.cryptohftdata",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
pub struct CryptoHFTDataClientConfig {
    /// CHD API key. Falls back to `CRYPTOHFTDATA_API_KEY` when unset.
    pub api_key: Option<String>,
    /// CHD API base URL.
    pub base_url: Option<String>,
    /// Whether to request and use short-lived JWT bearer tokens.
    pub use_jwt: Option<bool>,
    /// Optional proxy URL.
    pub proxy_url: Option<String>,
    /// Request timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Download rate limit per second.
    pub rate_limit_per_sec: Option<usize>,
}

impl Default for CryptoHFTDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            base_url: Some(CRYPTOHFTDATA_BASE_URL.to_string()),
            use_jwt: Some(true),
            proxy_url: None,
            timeout_secs: Some(60),
            rate_limit_per_sec: Some(8),
        }
    }
}

/// Configuration for direct CHD -> Nautilus catalog ingestion.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.cryptohftdata",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.cryptohftdata")
)]
pub struct CryptoHFTDataCatalogIngestConfig {
    /// CHD exchange identifier.
    pub exchange: CryptoHFTDataExchange,
    /// Raw CHD symbols to ingest.
    pub symbols: Vec<String>,
    /// CHD data types to ingest.
    pub data_types: Vec<CryptoHFTDataType>,
    /// Inclusive UTC start date in `YYYY-MM-DD` format.
    pub from: String,
    /// Inclusive UTC end date in `YYYY-MM-DD` format.
    pub to: String,
    /// Optional catalog root path. Falls back to `$NAUTILUS_PATH/catalog`.
    pub output_path: Option<String>,
    /// Optional cache directory for downloaded compressed CHD files.
    pub cache_dir: Option<String>,
    /// Optional CHD API key. Falls back to `CRYPTOHFTDATA_API_KEY`.
    pub api_key: Option<String>,
    /// Optional CHD API base URL.
    pub base_url: Option<String>,
    /// Whether to request and use short-lived JWT bearer tokens.
    pub use_jwt: Option<bool>,
    /// Optional proxy URL.
    pub proxy_url: Option<String>,
    /// Request timeout in seconds.
    pub timeout_secs: Option<u64>,
    /// Max concurrent hourly downloads.
    pub max_concurrent_downloads: Option<usize>,
    /// Data catalog batch size.
    pub batch_size: Option<usize>,
    /// Data catalog max Parquet row group size.
    pub max_row_group_size: Option<usize>,
    /// Parquet compression for Nautilus catalog files.
    pub compression: Option<ParquetCompression>,
    /// Handling policy for order book update sequence gaps.
    pub gap_policy: Option<GapPolicy>,
}

impl CryptoHFTDataCatalogIngestConfig {
    /// Returns a defaulted HTTP client config derived from this ingest config.
    #[must_use]
    pub fn client_config(&self) -> CryptoHFTDataClientConfig {
        let defaults = CryptoHFTDataClientConfig::default();
        CryptoHFTDataClientConfig {
            api_key: self.api_key.clone(),
            base_url: self.base_url.clone().or(defaults.base_url),
            use_jwt: self.use_jwt.or(defaults.use_jwt),
            proxy_url: self.proxy_url.clone(),
            timeout_secs: self.timeout_secs.or(defaults.timeout_secs),
            rate_limit_per_sec: defaults.rate_limit_per_sec,
        }
    }
}
