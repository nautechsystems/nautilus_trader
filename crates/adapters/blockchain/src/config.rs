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

use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{Chain, DexType, SharedChain},
    identifiers::{AccountId, TraderId, Venue},
};
use nautilus_system::ClientConfig;

/// Defines filtering criteria for the DEX pool universe that the data client will operate on.
#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
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
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
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
    /// Optional HTTP proxy URL for RPC requests.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL for RPC connections.
    ///
    /// Note: WebSocket proxy support is not yet implemented. This field is reserved
    /// for future functionality. Use `http_proxy_url` for REST API proxy support.
    pub ws_proxy_url: Option<String>,
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
            http_proxy_url: None,
            ws_proxy_url: None,
            from_block,
            pool_filters: pools_filters.unwrap_or_default(),
            postgres_cache_database_config,
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.blockchain",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainExecutionClientConfig {
    /// The trader ID for the client.
    pub trader_id: TraderId,
    /// The account ID for the client.
    pub client_id: AccountId,
    /// The execution venue used for routing in the execution engine.
    pub venue: Venue,
    /// The blockchain chain configuration.
    pub chain: Chain,
    /// The wallet address of the execution client.
    pub wallet_address: String,
    /// Token universe: set of ERC-20 token addresses to monitor for balance tracking.
    pub tokens: Option<Vec<String>>,
    /// Additional ERC-20 token addresses tracked in wallet snapshots.
    pub wallet_extra_tokens: Vec<String>,
    /// Wrapped-native token contract address (e.g., WBNB/WETH) tracked in snapshots.
    pub wallet_wnative_address: Option<String>,
    /// Spenders for allowance snapshots (e.g. router, Permit2).
    pub wallet_allowance_spenders: Vec<String>,
    /// Maximum age for cached wallet snapshots before refresh.
    pub wallet_snapshot_ttl_secs: u32,
    /// Max number of tracked tokens allowed per wallet refresh cycle.
    pub wallet_max_tokens_per_refresh: u32,
    /// Refresh wallet state during client connect.
    pub wallet_refresh_on_connect: bool,
    /// Maximum ERC-20 calls batched per multicall during wallet refresh.
    pub multicall_max_batch_size: u32,
    /// Minimum ERC-20 calls per batch when adaptively splitting multicall failures.
    pub multicall_min_batch_size: u32,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// Optional remote signer endpoint URL.
    pub signer_endpoint: Option<String>,
    /// Remote signer route path.
    pub signer_route: String,
    /// Remote signer request timeout in milliseconds.
    pub signer_timeout_ms: u64,
    /// Enforce HTTPS endpoint policy for signer requests.
    pub signer_require_tls: bool,
    /// Optional signer wallet override (defaults to wallet_address).
    pub signer_wallet_address: Option<String>,
    /// Optional router address override used for swap execution.
    pub execution_router_address: Option<String>,
    /// Slippage bound in basis points for exact-in swaps.
    pub execution_default_slippage_bps: u32,
    /// Deadline TTL in seconds for swap transaction intents.
    pub execution_default_deadline_secs: u64,
    /// Required block confirmations before terminalizing execution.
    pub execution_confirmations_required: u64,
    /// Maximum receipt polling attempts per transaction.
    pub execution_receipt_max_polls: u32,
    /// Receipt polling interval in milliseconds.
    pub execution_receipt_poll_interval_ms: u64,
    /// Per-wallet in-flight transaction budget (MVP currently enforces 1).
    pub execution_max_inflight_txs_per_wallet: u32,
    /// If true, skip automatic approve flow and require pre-approved allowance.
    pub execution_require_preapproved_allowance: bool,
    /// EIP-1559 max fee per gas used for swap transactions.
    pub execution_max_fee_per_gas: u64,
    /// EIP-1559 max priority fee per gas used for swap transactions.
    pub execution_max_priority_fee_per_gas: u64,
    /// Optional JSONL journal path for idempotency and replay state.
    pub execution_journal_path: Option<String>,
    /// Tokens that are explicitly blocked from fill decoding in MVP.
    pub execution_unsupported_token_addresses: Vec<String>,
}

impl BlockchainExecutionClientConfig {
    pub fn new(
        trader_id: TraderId,
        client_id: AccountId,
        venue: Venue,
        chain: Chain,
        wallet_address: String,
        tokens: Option<Vec<String>>,
        http_rpc_url: String,
        rpc_requests_per_second: Option<u32>,
    ) -> Self {
        Self {
            trader_id,
            client_id,
            venue,
            chain,
            wallet_address,
            tokens,
            wallet_extra_tokens: Vec::new(),
            wallet_wnative_address: None,
            wallet_allowance_spenders: Vec::new(),
            wallet_snapshot_ttl_secs: 30,
            wallet_max_tokens_per_refresh: 256,
            wallet_refresh_on_connect: true,
            multicall_max_batch_size: 64,
            multicall_min_batch_size: 4,
            http_rpc_url,
            rpc_requests_per_second,
            signer_endpoint: None,
            signer_route: "/sign/eth".to_string(),
            signer_timeout_ms: 5_000,
            signer_require_tls: true,
            signer_wallet_address: None,
            execution_router_address: None,
            execution_default_slippage_bps: 100,
            execution_default_deadline_secs: 120,
            execution_confirmations_required: 1,
            execution_receipt_max_polls: 60,
            execution_receipt_poll_interval_ms: 1_000,
            execution_max_inflight_txs_per_wallet: 1,
            execution_require_preapproved_allowance: true,
            execution_max_fee_per_gas: 1_000_000_000,
            execution_max_priority_fee_per_gas: 1_000_000_000,
            execution_journal_path: None,
            execution_unsupported_token_addresses: Vec::new(),
        }
    }
}

impl ClientConfig for BlockchainExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}
