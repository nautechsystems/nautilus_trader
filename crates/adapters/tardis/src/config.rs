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

use serde::{Deserialize, Serialize};

use super::machine::types::ReplayNormalizedRequestOptions;

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

/// Provides a configuration for a Tarid Machine -> Nautilus data -> Parquet replay run.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TardisReplayConfig {
    /// The Tardis Machine websocket url.
    pub tardis_ws_url: Option<String>,
    /// If symbols should be normalized with Nautilus conventions.
    pub normalize_symbols: Option<bool>,
    /// The output directory for writing Nautilus format Parquet files.
    pub output_path: Option<String>,
    /// The Tardis Machine replay options.
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
