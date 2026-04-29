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

use nautilus_core::serialization::default_true;
use nautilus_model::identifiers::ClientId;
use serde::{Deserialize, Serialize};

/// Configuration for `ExecutionEngine` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.execution",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.execution")
)]
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
pub struct ExecutionEngineConfig {
    /// If the cache should be loaded on initialization.
    #[serde(default = "default_true")]
    #[builder(default = true)]
    pub load_cache: bool,
    /// If the execution engine should maintain own/user order books based on commands and events.
    #[serde(default)]
    #[builder(default)]
    pub manage_own_order_books: bool,
    /// If order state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at every order state update (when events are applied).
    #[serde(default)]
    #[builder(default)]
    pub snapshot_orders: bool,
    /// If position state snapshot lists are persisted to a backing database.
    /// Snapshots will be taken at position opened, changed and closed (when events are applied).
    #[serde(default)]
    #[builder(default)]
    pub snapshot_positions: bool,
    /// The interval (seconds) at which additional position state snapshots are persisted.
    /// If `None` then no additional snapshots will be taken.
    #[serde(default)]
    pub snapshot_positions_interval_secs: Option<f64>,
    /// If order fills exceeding order quantity are allowed (logs warning instead of raising).
    /// Useful when position reconciliation races with exchange fill events.
    #[serde(default)]
    #[builder(default)]
    pub allow_overfills: bool,
    /// The client IDs declared for external stream processing.
    ///
    /// The execution engine will not attempt to send trading commands to these
    /// client IDs, assuming an external process will consume the serialized
    /// command messages from the bus and handle execution.
    #[serde(default)]
    pub external_clients: Option<Vec<ClientId>>,
    /// The interval (minutes) between purging closed orders from the in-memory cache.
    #[serde(default)]
    pub purge_closed_orders_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed orders can be purged.
    #[serde(default)]
    pub purge_closed_orders_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging closed positions from the in-memory cache.
    #[serde(default)]
    pub purge_closed_positions_interval_mins: Option<u32>,
    /// The time buffer (minutes) before closed positions can be purged.
    #[serde(default)]
    pub purge_closed_positions_buffer_mins: Option<u32>,
    /// The interval (minutes) between purging account events from the in-memory cache.
    #[serde(default)]
    pub purge_account_events_interval_mins: Option<u32>,
    /// The time buffer (minutes) before account events can be purged.
    #[serde(default)]
    pub purge_account_events_lookback_mins: Option<u32>,
    /// If purge operations should also delete from the backing database.
    #[serde(default)]
    #[builder(default)]
    pub purge_from_database: bool,
    /// If debug mode is active (will provide extra debug logging).
    #[serde(default)]
    #[builder(default)]
    pub debug: bool,
}

impl Default for ExecutionEngineConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}
