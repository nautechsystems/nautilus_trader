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

use serde::{Deserialize, Serialize};

use super::machine::types::{ReplayNormalizedRequestOptions, StreamNormalizedRequestOptions};

/// Determines the output format for Tardis `book_snapshot_*` messages.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BookSnapshotOutput {
    /// Convert book snapshots to `OrderBookDeltas` and write to `order_book_deltas/`.
    #[default]
    Deltas,
    /// Convert book snapshots to `OrderBookDepth10` and write to `order_book_depths/`.
    Depth10,
}

/// Provides a configuration for a Tardis Machine -> Nautilus data -> Parquet replay run.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
pub struct TardisReplayConfig {
    /// The Tardis Machine websocket url.
    pub tardis_ws_url: Option<String>,
    /// If symbols should be normalized with Nautilus conventions.
    pub normalize_symbols: Option<bool>,
    /// The output directory for writing Nautilus format Parquet files.
    pub output_path: Option<String>,
    /// The Tardis Machine replay options.
    #[builder(default)]
    pub options: Vec<ReplayNormalizedRequestOptions>,
    /// Optional WebSocket proxy URL.
    ///
    /// Note: WebSocket proxy support is not yet implemented. This field is reserved
    /// for future functionality.
    pub ws_proxy_url: Option<String>,
    /// The output format for `book_snapshot_*` messages.
    ///
    /// - `deltas`: Convert to `OrderBookDeltas` and write to `order_book_deltas/` (default).
    /// - `depth10`: Convert to `OrderBookDepth10` and write to `order_book_depths/`.
    pub book_snapshot_output: Option<BookSnapshotOutput>,
}

/// Configuration for the Tardis data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.tardis", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.tardis")
)]
pub struct TardisDataClientConfig {
    /// Tardis API key for HTTP instrument fetching.
    /// Falls back to `TARDIS_API_KEY` env var if not set.
    pub api_key: Option<String>,
    /// Tardis Machine Server WebSocket URL.
    /// Falls back to `TARDIS_MACHINE_WS_URL` env var if not set.
    pub tardis_ws_url: Option<String>,
    /// Whether to normalize symbols to Nautilus conventions.
    #[builder(default = true)]
    pub normalize_symbols: bool,
    /// Output format for `book_snapshot_*` messages.
    #[builder(default)]
    pub book_snapshot_output: BookSnapshotOutput,
    /// Replay options defining exchanges, symbols, date ranges, and data types.
    /// When non-empty the client connects to `ws-replay-normalized`.
    #[builder(default)]
    pub options: Vec<ReplayNormalizedRequestOptions>,
    /// Live stream options defining exchanges, symbols, and data types.
    /// When non-empty (and `options` is empty) the client connects to
    /// `ws-stream-normalized` with automatic reconnection.
    #[builder(default)]
    pub stream_options: Vec<StreamNormalizedRequestOptions>,
}

impl Default for TardisDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config_values() {
        let config = TardisDataClientConfig::default();
        assert!(config.api_key.is_none());
        assert!(config.tardis_ws_url.is_none());
        assert!(config.normalize_symbols);
        assert!(matches!(
            config.book_snapshot_output,
            BookSnapshotOutput::Deltas
        ));
        assert!(config.options.is_empty());
        assert!(config.stream_options.is_empty());
    }

    #[rstest]
    fn test_book_snapshot_output_default_is_deltas() {
        assert!(matches!(
            BookSnapshotOutput::default(),
            BookSnapshotOutput::Deltas
        ));
    }

    #[rstest]
    fn test_book_snapshot_output_serde_roundtrip_deltas() {
        let json = serde_json::to_string(&BookSnapshotOutput::Deltas).unwrap();
        assert_eq!(json, "\"deltas\"");

        let deserialized: BookSnapshotOutput = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, BookSnapshotOutput::Deltas));
    }

    #[rstest]
    fn test_book_snapshot_output_serde_roundtrip_depth10() {
        let json = serde_json::to_string(&BookSnapshotOutput::Depth10).unwrap();
        assert_eq!(json, "\"depth10\"");

        let deserialized: BookSnapshotOutput = serde_json::from_str(&json).unwrap();
        assert!(matches!(deserialized, BookSnapshotOutput::Depth10));
    }
}
