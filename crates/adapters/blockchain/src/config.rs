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

use std::any::Any;

use nautilus_common::factories::ClientConfig;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{Chain, DexType, SharedChain},
    identifiers::{AccountId, TraderId},
};
use nautilus_network::websocket::TransportBackend;

/// Defines filtering criteria for the DEX pool universe that the data client will operate on.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.blockchain")
)]
pub struct DexPoolFilters {
    /// Whether to exclude pools containing tokens with empty name or symbol fields.
    #[builder(default = true)]
    pub remove_pools_with_empty_erc20fields: bool,
}

impl Default for DexPoolFilters {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Configuration for blockchain data clients.
#[derive(Debug, Clone, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.blockchain")
)]
pub struct BlockchainDataClientConfig {
    /// The blockchain chain configuration.
    pub chain: SharedChain,
    /// List of decentralized exchange IDs to register and sync during connection.
    #[builder(default)]
    pub dex_ids: Vec<DexType>,
    /// Determines if the client should use Hypersync for live data streaming.
    #[builder(default)]
    pub use_hypersync_for_live_data: bool,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// The maximum number of Multicall calls per one RPC request.
    #[builder(default = 200)]
    pub multicall_calls_per_rpc_request: u32,
    /// The WebSocket secure URL for the blockchain RPC endpoint.
    pub wss_rpc_url: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The block from which to sync historical data.
    pub from_block: Option<u64>,
    /// Filtering criteria that define which DEX pools to include in the data universe.
    #[builder(default)]
    pub pool_filters: DexPoolFilters,
    /// Optional configuration for data client's Postgres cache database
    pub postgres_cache_database_config: Option<PostgresConnectOptions>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[derive(Debug, Clone, bon::Builder)]
pub struct BlockchainExecutionClientConfig {
    /// The trader ID for the client.
    pub trader_id: TraderId,
    /// The account ID for the client.
    pub client_id: AccountId,
    /// The blockchain chain configuration.
    pub chain: Chain,
    /// The wallet address of the execution client.
    pub wallet_address: String,
    /// Token universe: set of ERC-20 token addresses to monitor for balance tracking.
    pub tokens: Option<Vec<String>>,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl ClientConfig for BlockchainExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
