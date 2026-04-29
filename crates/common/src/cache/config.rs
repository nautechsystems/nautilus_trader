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

use crate::{enums::SerializationEncoding, msgbus::database::DatabaseConfig};

/// Configuration for `Cache` instances.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.common", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.common")
)]
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
pub struct CacheConfig {
    /// The configuration for the cache backing database.
    pub database: Option<DatabaseConfig>,
    /// The encoding for database operations, controls the type of serializer used.
    #[builder(default = SerializationEncoding::MsgPack)]
    pub encoding: SerializationEncoding,
    /// If timestamps should be persisted as ISO 8601 strings.
    #[builder(default)]
    pub timestamps_as_iso8601: bool,
    /// The buffer interval (milliseconds) between pipelined/batched transactions.
    pub buffer_interval_ms: Option<usize>,
    /// The batch size for bulk read operations (e.g., MGET).
    /// If set, bulk reads will be batched into chunks of this size.
    pub bulk_read_batch_size: Option<usize>,
    /// If a 'trader-' prefix is used for keys.
    #[builder(default = true)]
    pub use_trader_prefix: bool,
    /// If the trader's instance ID is used for keys.
    #[builder(default)]
    pub use_instance_id: bool,
    /// If the database should be flushed on start.
    #[builder(default)]
    pub flush_on_start: bool,
    /// If instrument data should be dropped from the cache's memory on reset.
    #[builder(default = true)]
    pub drop_instruments_on_reset: bool,
    /// The maximum length for internal tick deques.
    #[builder(default = 10_000)]
    pub tick_capacity: usize,
    /// The maximum length for internal bar deques.
    #[builder(default = 10_000)]
    pub bar_capacity: usize,
    /// If account events should be persisted to a backing database.
    #[builder(default = true)]
    pub persist_account_events: bool,
    /// If market data should be persisted to disk.
    #[builder(default)]
    pub save_market_data: bool,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl CacheConfig {
    /// Creates a new [`CacheConfig`] instance.
    #[expect(clippy::too_many_arguments)]
    #[must_use]
    pub const fn new(
        database: Option<DatabaseConfig>,
        encoding: SerializationEncoding,
        timestamps_as_iso8601: bool,
        buffer_interval_ms: Option<usize>,
        bulk_read_batch_size: Option<usize>,
        use_trader_prefix: bool,
        use_instance_id: bool,
        flush_on_start: bool,
        drop_instruments_on_reset: bool,
        tick_capacity: usize,
        bar_capacity: usize,
        persist_account_events: bool,
        save_market_data: bool,
    ) -> Self {
        Self {
            database,
            encoding,
            timestamps_as_iso8601,
            buffer_interval_ms,
            bulk_read_batch_size,
            use_trader_prefix,
            use_instance_id,
            flush_on_start,
            drop_instruments_on_reset,
            tick_capacity,
            bar_capacity,
            persist_account_events,
            save_market_data,
        }
    }
}
