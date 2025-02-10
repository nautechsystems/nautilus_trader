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
}
