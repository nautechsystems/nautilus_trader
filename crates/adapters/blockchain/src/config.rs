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

use std::{any::Any, fmt::Debug};

use nautilus_common::factories::ClientConfig;
use nautilus_core::string::secret::REDACTED;
use nautilus_infrastructure::sql::pg::PostgresConnectOptions;
use nautilus_model::{
    defi::{Chain, DexType, SharedChain},
    identifiers::AccountId,
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
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
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
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    #[serde(default)]
    pub transport_backend: TransportBackend,
}

impl Debug for BlockchainDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BlockchainDataClientConfig))
            .field("chain", &self.chain)
            .field("dex_ids", &self.dex_ids)
            .field(
                "use_hypersync_for_live_data",
                &self.use_hypersync_for_live_data,
            )
            .field("http_rpc_url", &REDACTED)
            .field("rpc_requests_per_second", &self.rpc_requests_per_second)
            .field(
                "multicall_calls_per_rpc_request",
                &self.multicall_calls_per_rpc_request,
            )
            .field("wss_rpc_url", &self.wss_rpc_url.as_ref().map(|_| REDACTED))
            .field("proxy_url", &self.proxy_url.as_ref().map(|_| REDACTED))
            .field("from_block", &self.from_block)
            .field("pool_filters", &self.pool_filters)
            .field(
                "postgres_cache_database_config",
                &self.postgres_cache_database_config,
            )
            .field("transport_backend", &self.transport_backend)
            .finish()
    }
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

/// Stable local identity for one RPC provider and its failure domains.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainProviderIdentity {
    /// Stable local provider identifier.
    pub provider_id: String,
    /// Stable local operator identifier.
    pub operator_id: String,
    /// Opaque identifiers for every known shared infrastructure failure domain.
    pub failure_domain_ids: Vec<String>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainProviderIdentity {
    failure_domain_ids: Vec<String>,
    operator_id: String,
    provider_id: String,
});

/// Configuration for one read-only verification RPC provider.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainVerificationProviderConfig {
    /// Stable provider and failure-domain identity.
    pub identity: BlockchainProviderIdentity,
    /// The read-only JSON-RPC endpoint.
    pub http_rpc_url: String,
}

impl Debug for BlockchainVerificationProviderConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BlockchainVerificationProviderConfig))
            .field("identity", &self.identity)
            .field("http_rpc_url", &REDACTED)
            .finish()
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainVerificationProviderConfig {
    http_rpc_url: String,
    identity: BlockchainProviderIdentity,
});

/// Locally trusted finalized chain checkpoint and freshness policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainChainAnchorConfig {
    /// Chain ID obtained independently from the configured providers.
    pub chain_id: u32,
    /// Chain name obtained independently from the configured providers.
    pub chain_name: String,
    /// Finalized checkpoint height.
    pub checkpoint_height: u64,
    /// Finalized checkpoint hash as a 32-byte hexadecimal string.
    pub checkpoint_hash: String,
    /// Finalized checkpoint timestamp in Unix seconds.
    pub checkpoint_timestamp: u64,
    /// Maximum permitted height difference among provider heads.
    pub max_head_skew_blocks: u64,
    /// Maximum permitted age of a decision head in seconds.
    pub max_head_age_secs: u64,
    /// Maximum permitted future drift of a decision head in seconds.
    pub max_future_drift_secs: u64,
}

/// A role assigned to one reviewed deployment contract.
#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BlockchainContractRole {
    Router,
    Factory,
    WrappedNative,
    Quote,
    Token,
    Pool,
    Implementation,
}

/// A reviewed explicit-height call used to prove a contract relationship.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainContractProbe {
    /// ABI-encoded call data.
    pub call_data: String,
    /// Exact expected ABI-encoded output.
    pub expected_output: String,
}

/// A reviewed proxy implementation binding.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainProxyManifest {
    /// Reviewed proxy kind: EIP-1967 implementation or Zeppelinos implementation.
    pub kind: String,
    /// Storage slot containing the implementation address.
    pub storage_slot: String,
    /// Exact expected 32-byte storage value.
    pub storage_value: String,
    /// Selected implementation address.
    pub target_address: String,
    /// Runtime code hash of the selected target.
    pub target_code_hash: String,
}

/// One code-bearing contract pinned by the deployment manifest.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainContractManifest {
    /// Contract address.
    pub address: String,
    /// Contract role.
    pub role: BlockchainContractRole,
    /// Keccak-256 runtime code hash.
    pub runtime_code_hash: String,
    /// Proxy implementation binding when the contract is upgradeable.
    pub proxy: Option<BlockchainProxyManifest>,
    /// Role-specific identity probes.
    #[serde(default)]
    pub probes: Vec<BlockchainContractProbe>,
}

/// Locally reviewed token identity and asset orientation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainTokenManifest {
    pub address: String,
    pub name: String,
    pub symbol: String,
    pub decimals: u8,
    /// `base`, `quote`, or `both` for the supported pool set.
    pub asset_role: String,
}

/// One supported pool definition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainPoolManifest {
    pub address: String,
    pub token0: String,
    pub token1: String,
    pub fee: u32,
    pub factory: String,
    pub quote_contract: String,
}

/// One permitted internal call edge for a transaction purpose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainCallEdgeManifest {
    /// `wrap`, `approve`, `swap_sell`, or `swap_buy`.
    pub purpose: String,
    pub caller: String,
    pub target: String,
    /// `call`, `staticcall`, `delegatecall`, or `callcode`.
    pub call_type: String,
}

/// Reviewed deployment and call-graph manifest for one chain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BlockchainDeploymentManifest {
    pub version: String,
    pub chain_id: u32,
    pub chain_name: String,
    pub contracts: Vec<BlockchainContractManifest>,
    pub tokens: Vec<BlockchainTokenManifest>,
    pub pools: Vec<BlockchainPoolManifest>,
    pub call_edges: Vec<BlockchainCallEdgeManifest>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainChainAnchorConfig {
    chain_id: u32,
    chain_name: String,
    checkpoint_hash: String,
    checkpoint_height: u64,
    checkpoint_timestamp: u64,
    max_future_drift_secs: u64,
    max_head_age_secs: u64,
    max_head_skew_blocks: u64,
});

/// Independent verification topology and reviewed local deployment identity.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainVerificationConfig {
    /// Identity of the authoritative provider in `http_rpc_url`.
    pub authoritative: BlockchainProviderIdentity,
    /// Exactly two read-only providers.
    pub verifiers: Vec<BlockchainVerificationProviderConfig>,
    /// Locally trusted chain checkpoint and freshness policy.
    pub chain_anchor: BlockchainChainAnchorConfig,
    /// Reviewed deployment manifest version.
    pub manifest_version: String,
    /// Digest of the canonical reviewed deployment manifest.
    pub manifest_digest: String,
    /// Reviewed deployment identities and permitted call graph.
    pub deployment_manifest: BlockchainDeploymentManifest,
}

impl Debug for BlockchainVerificationConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BlockchainVerificationConfig))
            .field("authoritative", &self.authoritative)
            .field("verifiers", &self.verifiers)
            .field("chain_anchor", &self.chain_anchor)
            .field("manifest_version", &self.manifest_version)
            .field("manifest_digest", &self.manifest_digest)
            .field("deployment_manifest", &REDACTED)
            .finish()
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainVerificationConfig {
    authoritative: BlockchainProviderIdentity,
    chain_anchor: BlockchainChainAnchorConfig,
    manifest_digest: String,
    manifest_version: String,
    verifiers: Vec<BlockchainVerificationProviderConfig>,
});

/// Defines the maximum quote-token spend for a directed BUY swap pair.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct QuoteSpendLimit {
    /// The swap input-token address used as the directed pair key.
    pub token_in: String,
    /// The swap output-token address used as the directed pair key.
    pub token_out: String,
    /// The token address that denominates `max_amount`.
    pub spend_token: String,
    /// The decimals of `spend_token` used to interpret its raw units.
    pub spend_token_decimals: u8,
    /// The maximum raw input amount as a base-10 unsigned integer string.
    pub max_amount: String,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(QuoteSpendLimit {
    max_amount: String,
    spend_token: String,
    spend_token_decimals: u8,
    token_in: String,
    token_out: String,
});

#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.blockchain", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.blockchain")
)]
pub struct BlockchainExecutionClientConfig {
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
    /// Independent provider topology, chain checkpoint, and deployment manifest identity.
    #[serde(default)]
    pub verification: Option<BlockchainVerificationConfig>,
    /// The maximum number of RPC requests allowed per second.
    pub rpc_requests_per_second: Option<u32>,
    /// Name of the environment variable holding the signer private key.
    pub signer_private_key_env: String,
    /// Name of the environment variable holding the active transaction payload sealing key.
    #[serde(default)]
    pub payload_key_env: Option<String>,
    /// Names of environment variables holding retired payload keys used only for unsealing.
    #[builder(default)]
    #[serde(default)]
    pub payload_key_retired_env: Vec<String>,
    /// Stable identifier bound to this execution database's sealed payloads.
    #[serde(default)]
    pub payload_deployment_id: Option<String>,
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
    /// Allowed (input token, output token) address pairs for swaps.
    pub allowed_token_pairs: Option<Vec<(String, String)>>,
    /// Pair-specific maximum quote-token spends for BUY swaps.
    pub quote_spend_limits: Option<Vec<QuoteSpendLimit>>,
    /// Default slippage in basis points applied to derive the swap minimum output.
    pub slippage_bps: Option<u32>,
    /// Maximum slippage in basis points accepted from a per-order parameter override.
    pub max_slippage_bps: Option<u32>,
    /// Per-order ceiling for the submitted base quantity, in raw units of the pool's base token.
    pub max_order_amount: Option<u64>,
    /// Swap deadline offset in seconds from the latest block timestamp.
    pub deadline_seconds: Option<u64>,
    /// Maximum age of the local pool state in blocks for a quote to be usable.
    pub max_quote_age_blocks: Option<u64>,
    /// Inclusion timeout in seconds before a broadcast transaction is treated as dropped.
    pub receipt_timeout_secs: Option<u64>,
    /// Durable store for execution transaction records; the client refuses to submit any
    /// transaction without it.
    pub postgres_cache_database_config: Option<PostgresConnectOptions>,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    #[serde(default)]
    pub transport_backend: TransportBackend,
}

impl Debug for BlockchainExecutionClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BlockchainExecutionClientConfig))
            .field("client_id", &self.client_id)
            .field("chain", &self.chain)
            .field("wallet_address", &self.wallet_address)
            .field("tokens", &self.tokens)
            .field("http_rpc_url", &REDACTED)
            .field("verification", &self.verification)
            .field("rpc_requests_per_second", &self.rpc_requests_per_second)
            .field("signer_private_key_env", &self.signer_private_key_env)
            .field("payload_key_env", &self.payload_key_env)
            .field("payload_key_retired_env", &self.payload_key_retired_env)
            .field("payload_deployment_id", &self.payload_deployment_id)
            .field("router_addresses", &self.router_addresses)
            .field("weth_address", &self.weth_address)
            .field("unlimited_approval", &self.unlimited_approval)
            .field("max_fee_per_gas_wei", &self.max_fee_per_gas_wei)
            .field("base_fee_buffer_bps", &self.base_fee_buffer_bps)
            .field("gas_limit", &self.gas_limit)
            .field("gas_buffer_bps", &self.gas_buffer_bps)
            .field("allowed_token_pairs", &self.allowed_token_pairs)
            .field("quote_spend_limits", &self.quote_spend_limits)
            .field("slippage_bps", &self.slippage_bps)
            .field("max_slippage_bps", &self.max_slippage_bps)
            .field("max_order_amount", &self.max_order_amount)
            .field("deadline_seconds", &self.deadline_seconds)
            .field("max_quote_age_blocks", &self.max_quote_age_blocks)
            .field("receipt_timeout_secs", &self.receipt_timeout_secs)
            .field(
                "postgres_cache_database_config",
                &self.postgres_cache_database_config,
            )
            .field("transport_backend", &self.transport_backend)
            .finish()
    }
}

impl ClientConfig for BlockchainExecutionClientConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BlockchainExecutionClientConfig {
    base_fee_buffer_bps: u32,
    deadline_seconds: Option<u64>,
    gas_buffer_bps: u32,
    gas_limit: u64,
    http_rpc_url: String,
    verification: Option<BlockchainVerificationConfig>,
    max_fee_per_gas_wei: u64,
    max_order_amount: Option<u64>,
    max_quote_age_blocks: Option<u64>,
    max_slippage_bps: Option<u32>,
    quote_spend_limits: Option<Vec<QuoteSpendLimit>>,
    receipt_timeout_secs: Option<u64>,
    router_addresses: Vec<String>,
    payload_deployment_id: Option<String>,
    payload_key_env: Option<String>,
    payload_key_retired_env: Vec<String>,
    signer_private_key_env: String,
    slippage_bps: Option<u32>,
    tokens: Option<Vec<String>>,
    transport_backend: TransportBackend,
    unlimited_approval: bool,
    wallet_address: String,
    weth_address: String,
});

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nautilus_model::defi::chain::chains;
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
client_id = "BLOCKCHAIN-001"
wallet_address = "0x0000000000000000000000000000000000000000"
http_rpc_url = "https://eth-mainnet.example.com"
signer_private_key_env = "BLOCKCHAIN_PRIVATE_KEY"
payload_key_env = "BLOCKCHAIN_PAYLOAD_KEY"
payload_key_retired_env = ["BLOCKCHAIN_PAYLOAD_KEY_OLD"]
payload_deployment_id = "primary-execution"
router_addresses = ["0xE592427A0AEce92De3Edee1F18E0157C05861564"]
weth_address = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
max_fee_per_gas_wei = 1000000000
base_fee_buffer_bps = 2000
gas_limit = 1000000
gas_buffer_bps = 2000
allowed_token_pairs = [
    ["0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"],
    ["0xaf88d065e77c8cC2239327C5EDb3A432268e5831", "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"],
]
slippage_bps = 50
max_slippage_bps = 200
max_order_amount = 1000000000000000000
deadline_seconds = 300
max_quote_age_blocks = 100
receipt_timeout_secs = 60

[[quote_spend_limits]]
token_in = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
token_out = "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1"
spend_token = "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"
spend_token_decimals = 6
max_amount = "1000000000"

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
            config.payload_key_env.as_deref(),
            Some("BLOCKCHAIN_PAYLOAD_KEY")
        );
        assert_eq!(
            config.payload_key_retired_env,
            vec!["BLOCKCHAIN_PAYLOAD_KEY_OLD".to_string()]
        );
        assert_eq!(
            config.payload_deployment_id.as_deref(),
            Some("primary-execution")
        );
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
        assert_eq!(
            config.allowed_token_pairs,
            Some(vec![
                (
                    "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                    "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                ),
                (
                    "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                    "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                )
            ]),
        );
        assert_eq!(
            config.quote_spend_limits,
            Some(vec![QuoteSpendLimit {
                token_in: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                token_out: "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                spend_token: "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
                spend_token_decimals: 6,
                max_amount: "1000000000".to_string(),
            }]),
        );
        assert_eq!(config.slippage_bps, Some(50));
        assert_eq!(config.max_slippage_bps, Some(200));
        assert_eq!(config.max_order_amount, Some(1_000_000_000_000_000_000));
        assert_eq!(config.deadline_seconds, Some(300));
        assert_eq!(config.max_quote_age_blocks, Some(100));
        assert_eq!(config.receipt_timeout_secs, Some(60));
        assert!(config.postgres_cache_database_config.is_none());
        assert_eq!(config.transport_backend, TransportBackend::default());
    }

    #[rstest]
    fn test_execution_config_toml_accepts_legacy_shape_without_transaction_limits() {
        let config: BlockchainExecutionClientConfig = toml::from_str(
            r#"
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

        assert!(config.allowed_token_pairs.is_none());
        assert!(config.payload_key_env.is_none());
        assert!(config.payload_key_retired_env.is_empty());
        assert!(config.payload_deployment_id.is_none());
        assert!(config.quote_spend_limits.is_none());
        assert!(config.slippage_bps.is_none());
        assert!(config.max_slippage_bps.is_none());
        assert!(config.max_order_amount.is_none());
        assert!(config.deadline_seconds.is_none());
        assert!(config.max_quote_age_blocks.is_none());
        assert!(config.receipt_timeout_secs.is_none());
    }

    #[rstest]
    fn test_execution_config_toml_rejects_unknown_fields() {
        let result: Result<BlockchainExecutionClientConfig, _> = toml::from_str(
            r#"
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
allowed_token_pairs = [["0x82aF49447D8a07e3bd95BD0d56f35241523fBab1", "0xaf88d065e77c8cC2239327C5EDb3A432268e5831"]]
slippage_bps = 50
max_slippage_bps = 200
max_order_amount = 1000000000000000000
deadline_seconds = 300
max_quote_age_blocks = 100
receipt_timeout_secs = 60
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

    #[rstest]
    fn test_data_config_debug_redacts_rpc_urls() {
        const HTTP_USERINFO_SECRET: &str = "data-http-userinfo-secret";
        const WSS_QUERY_SECRET: &str = "data-wss-query-secret";
        let http_rpc_url = format!(
            "https://rpc-user:{HTTP_USERINFO_SECRET}@rpc.example.com/data-http-path-secret"
        );
        let wss_rpc_url = format!("wss://rpc.example.com/ws?api_key={WSS_QUERY_SECRET}");
        let config = BlockchainDataClientConfig::builder()
            .chain(Arc::new(chains::ETHEREUM.clone()))
            .http_rpc_url(http_rpc_url.clone())
            .wss_rpc_url(wss_rpc_url.clone())
            .build();

        let debug = format!("{config:?}");

        assert!(debug.contains("http_rpc_url: \"<redacted>\""));
        assert!(debug.contains("wss_rpc_url: Some(\"<redacted>\")"));
        assert!(!debug.contains(HTTP_USERINFO_SECRET));
        assert!(!debug.contains(WSS_QUERY_SECRET));
        assert!(!debug.contains(&http_rpc_url));
        assert!(!debug.contains(&wss_rpc_url));
    }

    #[rstest]
    fn test_execution_config_debug_redacts_rpc_url() {
        const PATH_SECRET: &str = "execution-http-path-secret";
        const QUERY_SECRET: &str = "execution-http-query-secret";
        let http_rpc_url = format!("https://rpc.example.com/{PATH_SECRET}?api_key={QUERY_SECRET}");
        let config = BlockchainExecutionClientConfig::builder()
            .client_id(AccountId::from("BLOCKCHAIN-001"))
            .chain(chains::ETHEREUM.clone())
            .wallet_address("0x0000000000000000000000000000000000000000".to_string())
            .http_rpc_url(http_rpc_url.clone())
            .signer_private_key_env("BLOCKCHAIN_PRIVATE_KEY".to_string())
            .router_addresses(vec![
                "0xE592427A0AEce92De3Edee1F18E0157C05861564".to_string(),
            ])
            .weth_address("0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string())
            .max_fee_per_gas_wei(1_000_000_000)
            .base_fee_buffer_bps(2_000)
            .gas_limit(1_000_000)
            .gas_buffer_bps(2_000)
            .allowed_token_pairs(vec![(
                "0x82aF49447D8a07e3bd95BD0d56f35241523fBab1".to_string(),
                "0xaf88d065e77c8cC2239327C5EDb3A432268e5831".to_string(),
            )])
            .slippage_bps(50)
            .max_slippage_bps(200)
            .max_order_amount(1_000_000_000_000_000_000)
            .deadline_seconds(300)
            .max_quote_age_blocks(100)
            .receipt_timeout_secs(60)
            .build();

        let debug = format!("{config:?}");

        assert!(debug.contains("http_rpc_url: \"<redacted>\""));
        assert!(!debug.contains(PATH_SECRET));
        assert!(!debug.contains(QUERY_SECRET));
        assert!(!debug.contains(&http_rpc_url));
    }
}
