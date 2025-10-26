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

use nautilus_model::identifiers::ClientId;
use serde::{Deserialize, Serialize};

/// Configuration for `ExecutionEngine` instances.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEngineConfig {
    /// If the cache should be loaded on initialization.
    #[serde(default = "default_true")]
    pub load_cache: bool,
    /// If the execution engine should maintain own/user order books based on commands and events.
    #[serde(default)]
    pub manage_own_order_books: bool,
    /// If order state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at every order state update (when events are applied).
    #[serde(default)]
    pub snapshot_orders: bool,
    /// If position state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at position opened, changed and closed (when events are applied).
    #[serde(default)]
    pub snapshot_positions: bool,
    /// The interval (seconds) at which additional position state snapshots are persisted.
    /// If None then no additional snapshots will be taken.
    #[serde(default)]
    pub snapshot_positions_interval_secs: Option<f64>,
    /// If quote-denominated order quantities should be converted to base units before submission.
    #[serde(default = "default_true")]
    pub convert_quote_qty_to_base: bool,
    /// The client IDs declared for external stream processing.
    ///
    /// The execution engine will not attempt to send trading commands to these
    /// client IDs, assuming an external process will consume the serialized
    /// command messages from the bus and handle execution.
    #[serde(default)]
    pub external_clients: Option<Vec<ClientId>>,
    /// If debug mode is active (will provide extra debug logging).
    #[serde(default)]
    pub debug: bool,
}

const fn default_true() -> bool {
    true
}

impl Default for ExecutionEngineConfig {
    fn default() -> Self {
        Self {
            load_cache: true,
            manage_own_order_books: false,
            snapshot_orders: false,
            snapshot_positions: false,
            snapshot_positions_interval_secs: None,
            convert_quote_qty_to_base: true,
            external_clients: None,
            debug: false,
        }
    }
}
