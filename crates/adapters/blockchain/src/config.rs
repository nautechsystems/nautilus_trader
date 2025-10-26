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

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::defi::{DexType, SharedChain};

/// Defines filtering criteria for the DEX pool universe that the data client will operate on.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.blockchain")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct DexPoolFilters {
    /// Whether to exclude pools containing tokens with empty name or symbol fields.
    pub remove_pools_with_empty_erc20fields: bool,
}

impl DexPoolFilters {
    /// Creates a new [`DexPoolFilters`] instance.
    #[must_use]
    pub fn new(remove_pools_with_empty_erc20fields: Option<bool>) -> Self {
        Self {
            remove_pools_with_empty_erc20fields: remove_pools_with_empty_erc20fields
                .unwrap_or(true),
        }
    }
}

impl Default for DexPoolFilters {
    fn default() -> Self {
        Self {
            remove_pools_with_empty_erc20fields: true,
        }
    }
}

/// Configuration for blockchain data clients.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.blockchain")
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainDataClientConfig {
    /// The blockchain chain configuration.
    pub chain: SharedChain,
    /// List of decentralized exchange IDs to register and sync during connection.
    pub dex_ids: Vec<DexType>,
    /// Determines if the client should use Hypersync for live data streaming.
    pub use_hypersync_for_live_data: bool,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// The maximum number of Multicall calls per one RPC request.
    pub multicall_calls_per_rpc_request: u32,
    /// The WebSocket secure URL for the blockchain RPC endpoint.
    pub wss_rpc_url: Option<String>,
    /// The block from which to sync historical data.
    pub from_block: Option<u64>,
    /// Filtering criteria that define which DEX pools to include in the data universe.
    pub pool_filters: DexPoolFilters,
    /// Optional configuration for data client's Postgres cache database
    pub postgres_cache_database_config: Option<PostgresConnectOptions>,
}

impl BlockchainDataClientConfig {
    /// Creates a new [`BlockchainDataClientConfig`] instance.
    #[allow(clippy::too_many_arguments)]
    #[must_use]
    pub fn new(
        chain: SharedChain,
        dex_ids: Vec<DexType>,
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
        multicall_calls_per_rpc_request: Option<u32>,
        wss_rpc_url: Option<String>,
        use_hypersync_for_live_data: bool,
        from_block: Option<u64>,
        pools_filters: Option<DexPoolFilters>,
        postgres_cache_database_config: Option<PostgresConnectOptions>,
    ) -> Self {
        Self {
            chain,
            dex_ids,
            use_hypersync_for_live_data,
            http_rpc_url,
            rpc_requests_per_second,
            multicall_calls_per_rpc_request: multicall_calls_per_rpc_request.unwrap_or(200),
            wss_rpc_url,
            from_block,
            pool_filters: pools_filters.unwrap_or_default(),
            postgres_cache_database_config,
        }
    }
}
