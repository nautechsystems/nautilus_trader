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
use serde::{Deserialize, Serialize};

/// Defines filtering criteria for the DEX pool universe that the data client will operate on.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
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
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainDataClientConfig {
    /// The blockchain chain configuration.
    pub chain: SharedChain,
    /// List of decentralized exchange IDs to register and sync during connection.
    #[builder(default)]
    #[serde(default)]
    pub dex_ids: Vec<DexType>,
    /// Determines if the client should use Hypersync for live data streaming.
    #[builder(default)]
    #[serde(default)]
    pub use_hypersync_for_live_data: bool,
    /// The HTTP URL for the blockchain RPC endpoint.
    pub http_rpc_url: String,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// The maximum number of Multicall calls per one RPC request.
    #[builder(default = 200)]
    #[serde(default = "default_multicall_calls_per_rpc_request")]
    pub multicall_calls_per_rpc_request: u32,
    /// The WebSocket secure URL for the blockchain RPC endpoint.
    pub wss_rpc_url: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The block from which to sync historical data.
    pub from_block: Option<u64>,
    /// Filtering criteria that define which DEX pools to include in the data universe.
    #[builder(default)]
    #[serde(default)]
    pub pool_filters: DexPoolFilters,
    /// Optional configuration for data client's Postgres cache database
    pub postgres_cache_database_config: Option<PostgresConnectOptions>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    #[serde(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainDataClientConfig {
    dex_ids: Vec<DexType>,
    multicall_calls_per_rpc_request: u32,
    pool_filters: DexPoolFilters,
    transport_backend: TransportBackend,
});

const fn default_multicall_calls_per_rpc_request() -> u32 {
    200
}

#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
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
    /// Name of the environment variable holding the signer private key.
    pub signer_private_key_env: String,
    /// Allowed SwapRouter addresses for approval and swap transactions.
    pub router_addresses: Vec<String>,
    /// Wrapped native token address for wrap operations.
    pub weth_address: String,
    /// Whether to approve routers with an unlimited allowance instead of the exact amount.
    #[builder(default)]
    #[serde(default)]
    pub unlimited_approval: bool,
    /// Hard ceiling for the derived max fee per gas in wei; conditions above it reject the
    /// transaction.
    pub max_fee_per_gas_wei: u64,
    /// Buffer in basis points applied over the latest base fee.
    pub base_fee_buffer_bps: u32,
    /// Gas ceiling in units; buffered estimates above it reject the transaction before signing
    /// (never clamp).
    pub gas_limit: u64,
    /// Buffer in basis points applied over the `eth_estimateGas` result.
    pub gas_buffer_bps: u32,
    /// Durable store for execution transaction records; the client refuses to submit any
    /// transaction without it.
    pub postgres_cache_database_config: Option<PostgresConnectOptions>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    #[serde(default)]
    pub transport_backend: TransportBackend,
}

impl ClientConfig for BlockchainExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainExecutionClientConfig {
    base_fee_buffer_bps: u32,
    gas_buffer_bps: u32,
    gas_limit: u64,
    http_rpc_url: String,
    max_fee_per_gas_wei: u64,
    router_addresses: Vec<String>,
    signer_private_key_env: String,
    tokens: Option<Vec<String>>,
    transport_backend: TransportBackend,
    unlimited_approval: bool,
    wallet_address: String,
    weth_address: String,
});

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: BlockchainDataClientConfig = toml::from_str(
            r#"
http_rpc_url = "https://eth-mainnet.example.com"

[chain]
name = "Ethereum"
chain_id = 1
hypersync_url = "https://1.hypersync.xyz"
native_currency_decimals = 18
"#,
        )
        .unwrap();

        assert_eq!(config.http_rpc_url, "https://eth-mainnet.example.com");
        assert_eq!(config.chain.chain_id, 1);
        assert!(config.dex_ids.is_empty());
        assert!(!config.use_hypersync_for_live_data);
        assert_eq!(config.multicall_calls_per_rpc_request, 200);
        assert!(config.pool_filters.remove_pools_with_empty_erc20fields);
        assert_eq!(config.transport_backend, TransportBackend::default());
    }

    #[rstest]
    fn test_execution_config_toml_minimal() {
        let config: BlockchainExecutionClientConfig = toml::from_str(
            r#"
trader_id = "TRADER-001"
client_id = "BLOCKCHAIN-001"
wallet_address = "0x0000000000000000000000000000000000000000"
http_rpc_url = "https://eth-mainnet.example.com"
signer_private_key_env = "BLOCKCHAIN_PRIVATE_KEY"
router_addresses = ["0xE592427A0AEce92De3Edee1F18E0157C05861564"]
weth_address = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
max_fee_per_gas_wei = 1000000000
base_fee_buffer_bps = 2000
gas_limit = 1000000
gas_buffer_bps = 2000

[chain]
name = "Ethereum"
chain_id = 1
hypersync_url = "https://1.hypersync.xyz"
native_currency_decimals = 18
"#,
        )
        .unwrap();

        assert_eq!(config.http_rpc_url, "https://eth-mainnet.example.com");
        assert_eq!(config.chain.chain_id, 1);
        assert_eq!(
            config.wallet_address,
            "0x0000000000000000000000000000000000000000",
        );
        assert!(config.tokens.is_none());
        assert!(config.rpc_requests_per_second.is_none());
        assert_eq!(config.signer_private_key_env, "BLOCKCHAIN_PRIVATE_KEY");
        assert_eq!(
            config.router_addresses,
            vec!["0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string()],
        );
        assert_eq!(
            config.weth_address,
            "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1",
        );
        assert!(!config.unlimited_approval);
        assert_eq!(config.max_fee_per_gas_wei, 1_000_000_000);
        assert_eq!(config.base_fee_buffer_bps, 2_000);
        assert_eq!(config.gas_limit, 1_000_000);
        assert_eq!(config.gas_buffer_bps, 2_000);
        assert!(config.postgres_cache_database_config.is_none());
        assert_eq!(config.transport_backend, TransportBackend::default());
    }

    #[rstest]
    fn test_execution_config_toml_rejects_unknown_fields() {
        let result: Result<BlockchainExecutionClientConfig, _> = toml::from_str(
            r#"
trader_id = "TRADER-001"
client_id = "BLOCKCHAIN-001"
wallet_address = "0x0000000000000000000000000000000000000000"
http_rpc_url = "https://eth-mainnet.example.com"
signer_private_key_env = "BLOCKCHAIN_PRIVATE_KEY"
router_addresses = ["0xE592427A0AEce92De3Edee1F18E0157C05861564"]
weth_address = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
max_fee_per_gas_wei = 1000000000
base_fee_buffer_bps = 2000
gas_limit = 1000000
gas_buffer_bps = 2000
unknown_field = 1

[chain]
name = "Ethereum"
chain_id = 1
hypersync_url = "https://1.hypersync.xyz"
native_currency_decimals = 18
"#,
        );

        assert!(result.is_err());
    }
}
