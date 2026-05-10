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

//! Configuration structures for the dYdX adapter.

use std::num::NonZeroU32;

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::{ratelimiter::quota::Quota, websocket::TransportBackend};
use serde::{Deserialize, Serialize};

use crate::{
    common::{consts::DYDX_CHAIN_ID, enums::DydxNetwork, urls},
    grpc::types::ChainId,
};

/// Configuration for the dYdX adapter.
///
/// URL fields (`base_url`, `ws_url`, `grpc_url`, `grpc_urls`) default to mainnet in the
/// builder. Use [`DydxAdapterConfig::for_network`] to build a config whose URLs and chain
/// ID match the target network, or override each URL explicitly.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
pub struct DydxAdapterConfig {
    /// Network environment (mainnet or testnet).
    #[serde(default)]
    #[builder(default)]
    pub network: DydxNetwork,
    /// Base URL for the HTTP API.
    #[builder(default = urls::http_base_url(DydxNetwork::Mainnet).to_string())]
    pub base_url: String,
    /// Base URL for the WebSocket API.
    #[builder(default = urls::ws_url(DydxNetwork::Mainnet).to_string())]
    pub ws_url: String,
    /// Base URL for the gRPC API (Cosmos SDK transactions).
    ///
    /// For backwards compatibility, a single URL can be provided.
    /// Consider using `grpc_urls` for fallback support.
    #[builder(default = urls::grpc_urls(DydxNetwork::Mainnet)[0].to_string())]
    pub grpc_url: String,
    /// List of gRPC URLs with fallback support.
    ///
    /// If provided, the client will attempt to connect to each URL in order
    /// until a successful connection is established. This is recommended for
    /// production use in DEX environments where nodes can fail.
    #[serde(default)]
    #[builder(default = urls::grpc_urls(DydxNetwork::Mainnet).iter().map(|&s| s.to_string()).collect())]
    pub grpc_urls: Vec<String>,
    /// Chain ID (e.g., "dydx-mainnet-1" for mainnet, "dydx-testnet-4" for testnet).
    #[builder(default = DYDX_CHAIN_ID.to_string())]
    pub chain_id: String,
    /// Request timeout in seconds.
    #[builder(default = 30)]
    pub timeout_secs: u64,
    /// Wallet address for the account.
    ///
    /// If not provided, falls back to environment variable:
    /// - Mainnet: `DYDX_WALLET_ADDRESS`
    /// - Testnet: `DYDX_TESTNET_WALLET_ADDRESS`
    ///
    /// Use `resolve_wallet_address()` to resolve from config or environment.
    #[serde(default)]
    pub wallet_address: Option<String>,
    /// Subaccount number (default: 0).
    #[serde(default)]
    #[builder(default)]
    pub subaccount: u32,
    /// Private key (hex) for wallet signing.
    ///
    /// If not provided, falls back to environment variable:
    /// - Mainnet: `DYDX_PRIVATE_KEY`
    /// - Testnet: `DYDX_TESTNET_PRIVATE_KEY`
    ///
    /// Use `DydxCredential::resolve()` to resolve from config or environment.
    #[serde(default)]
    pub private_key: Option<String>,
    /// Authenticator IDs for permissioned key trading.
    ///
    /// When provided, transactions will include a TxExtension to enable trading
    /// via sub-accounts using delegated signing keys. This is an advanced feature
    /// for institutional setups with separated hot/cold wallet architectures.
    ///
    /// See <https://docs.dydx.xyz/concepts/trading/authenticators> for details on
    /// permissioned keys and authenticator configuration.
    #[serde(default)]
    #[builder(default)]
    pub authenticator_ids: Vec<u64>,
    /// Maximum number of retries for failed requests (default: 3).
    #[serde(default = "default_max_retries")]
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds (default: 1000ms).
    #[serde(default = "default_retry_delay_initial_ms")]
    #[builder(default = 1000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds (default: 10000ms).
    #[serde(default = "default_retry_delay_max_ms")]
    #[builder(default = 10000)]
    pub retry_delay_max_ms: u64,
    /// gRPC rate limit: maximum broadcast requests per second.
    ///
    /// Controls the rate of gRPC `broadcast_tx` calls to prevent 429 (ResourceExhausted)
    /// errors from validator nodes. Known provider limits:
    /// - Polkachu: 300 req/min (~5 req/s)
    /// - KingNodes: 250 req/min (~4.2 req/s)
    /// - AutoStake: 4 req/s
    ///
    /// Default: 4 requests per second (conservative, works across all public providers).
    /// When `None`, rate limiting is disabled.
    #[serde(default = "default_grpc_rate_limit_per_second")]
    pub grpc_rate_limit_per_second: Option<u32>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    #[serde(default)]
    pub proxy_url: Option<String>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[serde(default)]
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

fn default_max_retries() -> u32 {
    3
}

fn default_retry_delay_initial_ms() -> u64 {
    1000
}

fn default_retry_delay_max_ms() -> u64 {
    10000
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "serde default must match field type Option<u32>"
)]
fn default_grpc_rate_limit_per_second() -> Option<u32> {
    Some(4)
}

fn default_data_http_timeout_secs() -> u64 {
    60
}

fn default_data_max_retries() -> u64 {
    3
}

fn default_data_retry_delay_initial_ms() -> u64 {
    100
}

fn default_data_retry_delay_max_ms() -> u64 {
    5000
}

impl DydxAdapterConfig {
    /// Creates a config with URLs and chain ID resolved for the given network.
    ///
    /// Use this instead of `Default::default()` when constructing a testnet config
    /// without explicit URL overrides. Retains the non-URL defaults from
    /// [`Default::default`] (retries, timeouts, gRPC rate limit).
    #[must_use]
    pub fn for_network(network: DydxNetwork) -> Self {
        let chain_id = match network {
            DydxNetwork::Mainnet => crate::common::consts::DYDX_CHAIN_ID,
            DydxNetwork::Testnet => crate::common::consts::DYDX_TESTNET_CHAIN_ID,
        };
        Self {
            network,
            base_url: urls::http_base_url(network).to_string(),
            ws_url: urls::ws_url(network).to_string(),
            grpc_url: urls::grpc_urls(network)[0].to_string(),
            grpc_urls: urls::grpc_urls(network)
                .iter()
                .map(|&s| s.to_string())
                .collect(),
            chain_id: chain_id.to_string(),
            ..Self::default()
        }
    }

    /// Get the list of gRPC URLs to use for connection with fallback support.
    ///
    /// Returns `grpc_urls` if non-empty, otherwise falls back to a single-element
    /// vector containing `grpc_url`.
    #[must_use]
    pub fn get_grpc_urls(&self) -> Vec<String> {
        if self.grpc_urls.is_empty() {
            vec![self.grpc_url.clone()]
        } else {
            self.grpc_urls.clone()
        }
    }

    /// Map the configured network to the underlying chain ID.
    ///
    /// This is the recommended way to get the chain ID for transaction submission.
    #[must_use]
    pub const fn get_chain_id(&self) -> ChainId {
        self.network.chain_id()
    }

    /// Returns whether this is a testnet configuration.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        matches!(self.network, DydxNetwork::Testnet)
    }

    /// Returns the gRPC rate limiting quota, if configured.
    #[must_use]
    pub fn grpc_quota(&self) -> Option<Quota> {
        self.grpc_rate_limit_per_second
            .and_then(NonZeroU32::new)
            .and_then(Quota::per_second)
    }
}

impl Default for DydxAdapterConfig {
    fn default() -> Self {
        Self {
            grpc_rate_limit_per_second: default_grpc_rate_limit_per_second(),
            ..Self::builder().build()
        }
    }
}

/// Configuration for the dYdX data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.dydx")
)]
pub struct DydxDataClientConfig {
    /// Base URL for the HTTP API.
    pub base_url_http: Option<String>,
    /// Base URL for the WebSocket API.
    pub base_url_ws: Option<String>,
    /// HTTP request timeout in seconds.
    #[serde(default = "default_data_http_timeout_secs")]
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for failed HTTP requests.
    #[serde(default = "default_data_max_retries")]
    #[builder(default = 3)]
    pub max_retries: u64,
    /// Initial retry delay in milliseconds.
    #[serde(default = "default_data_retry_delay_initial_ms")]
    #[builder(default = 100)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[serde(default = "default_data_retry_delay_max_ms")]
    #[builder(default = 5000)]
    pub retry_delay_max_ms: u64,
    /// Network environment (mainnet or testnet).
    #[serde(default)]
    #[builder(default)]
    pub network: DydxNetwork,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[serde(default)]
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl DydxDataClientConfig {
    /// Returns whether this is a testnet configuration.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        matches!(self.network, DydxNetwork::Testnet)
    }
}

impl Default for DydxDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

/// Configuration for the dYdX execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.dydx")
)]
pub struct DydxExecClientConfig {
    /// The trader ID for the client.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// The account ID for the client.
    #[builder(default = AccountId::from("DYDX-001"))]
    pub account_id: AccountId,
    /// Network environment (mainnet or testnet).
    #[serde(default)]
    #[builder(default)]
    pub network: DydxNetwork,
    /// gRPC endpoint URL (optional, uses default for network if not provided).
    pub grpc_endpoint: Option<String>,
    /// Additional gRPC URLs for fallback support.
    #[serde(default)]
    #[builder(default)]
    pub grpc_urls: Vec<String>,
    /// WebSocket endpoint URL (optional, uses default for network if not provided).
    pub ws_endpoint: Option<String>,
    /// HTTP endpoint URL (optional, uses default for network if not provided).
    pub http_endpoint: Option<String>,
    /// Private key (hex) for wallet signing.
    ///
    /// If not provided, falls back to environment variable:
    /// - Mainnet: `DYDX_PRIVATE_KEY`
    /// - Testnet: `DYDX_TESTNET_PRIVATE_KEY`
    pub private_key: Option<String>,
    /// Wallet address.
    ///
    /// If not provided, falls back to environment variable:
    /// - Mainnet: `DYDX_WALLET_ADDRESS`
    /// - Testnet: `DYDX_TESTNET_WALLET_ADDRESS`
    pub wallet_address: Option<String>,
    /// Subaccount number (default: 0).
    #[serde(default)]
    #[builder(default)]
    pub subaccount_number: u32,
    /// Authenticator IDs for permissioned key trading.
    #[serde(default)]
    #[builder(default)]
    pub authenticator_ids: Vec<u64>,
    /// HTTP request timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Maximum number of retry attempts.
    pub max_retries: Option<u32>,
    /// Initial retry delay in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Maximum retry delay in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// gRPC rate limit: maximum broadcast requests per second.
    /// When `None`, rate limiting is disabled.
    #[serde(default = "default_grpc_rate_limit_per_second")]
    pub grpc_rate_limit_per_second: Option<u32>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[serde(default)]
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for DydxExecClientConfig {
    fn default() -> Self {
        Self {
            grpc_rate_limit_per_second: default_grpc_rate_limit_per_second(),
            ..Self::builder().build()
        }
    }
}

impl DydxExecClientConfig {
    /// Returns the gRPC URLs to use, with fallback support.
    ///
    /// Returns `grpc_urls` if non-empty, otherwise uses `grpc_endpoint` if provided,
    /// otherwise uses the default URLs for the configured network.
    #[must_use]
    pub fn get_grpc_urls(&self) -> Vec<String> {
        if !self.grpc_urls.is_empty() {
            return self.grpc_urls.clone();
        }

        if let Some(ref endpoint) = self.grpc_endpoint {
            return vec![endpoint.clone()];
        }
        urls::grpc_urls(self.network)
            .iter()
            .map(|&s| s.to_string())
            .collect()
    }

    /// Returns the WebSocket URL for the configured network.
    #[must_use]
    pub fn get_ws_url(&self) -> String {
        self.ws_endpoint
            .clone()
            .unwrap_or_else(|| urls::ws_url(self.network).to_string())
    }

    /// Returns the HTTP URL for the configured network.
    #[must_use]
    pub fn get_http_url(&self) -> String {
        self.http_endpoint
            .clone()
            .unwrap_or_else(|| urls::http_base_url(self.network).to_string())
    }

    /// Returns the chain ID for the configured network.
    #[must_use]
    pub const fn get_chain_id(&self) -> ChainId {
        self.network.chain_id()
    }

    /// Returns whether this is a testnet configuration.
    #[must_use]
    pub const fn is_testnet(&self) -> bool {
        matches!(self.network, DydxNetwork::Testnet)
    }

    /// Returns the gRPC rate limiting quota, if configured.
    #[must_use]
    pub fn grpc_quota(&self) -> Option<Quota> {
        self.grpc_rate_limit_per_second
            .and_then(NonZeroU32::new)
            .and_then(Quota::per_second)
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_config_get_chain_id_mainnet() {
        let config = DydxAdapterConfig {
            network: DydxNetwork::Mainnet,
            ..Default::default()
        };
        assert_eq!(config.get_chain_id(), ChainId::Mainnet1);
    }

    #[rstest]
    fn test_config_get_chain_id_testnet() {
        let config = DydxAdapterConfig {
            network: DydxNetwork::Testnet,
            ..Default::default()
        };
        assert_eq!(config.get_chain_id(), ChainId::Testnet4);
    }

    #[rstest]
    fn test_config_is_testnet() {
        let mainnet_config = DydxAdapterConfig {
            network: DydxNetwork::Mainnet,
            ..Default::default()
        };
        assert!(!mainnet_config.is_testnet());

        let testnet_config = DydxAdapterConfig {
            network: DydxNetwork::Testnet,
            ..Default::default()
        };
        assert!(testnet_config.is_testnet());
    }

    #[rstest]
    fn test_config_default_uses_mainnet() {
        let config = DydxAdapterConfig::default();
        assert_eq!(config.network, DydxNetwork::Mainnet);
        assert!(!config.is_testnet());
    }

    #[rstest]
    fn test_config_serde_backwards_compat() {
        // Test that configs missing network field can deserialize with default
        let json = r#"{"base_url":"https://indexer.dydx.trade","ws_url":"wss://indexer.dydx.trade/v4/ws","grpc_url":"https://dydx-ops-grpc.kingnodes.com:443","grpc_urls":[],"chain_id":"dydx-mainnet-1","timeout_secs":30,"subaccount":0,"max_retries":3,"retry_delay_initial_ms":1000,"retry_delay_max_ms":10000}"#;

        let config: Result<DydxAdapterConfig, _> = serde_json::from_str(json);
        assert!(config.is_ok());
        let config = config.unwrap();
        // Should default to Mainnet when network field is missing
        assert_eq!(config.network, DydxNetwork::Mainnet);
    }

    #[rstest]
    fn test_config_get_grpc_urls_fallback() {
        let config = DydxAdapterConfig {
            grpc_url: "https://primary.example.com".to_string(),
            grpc_urls: vec![],
            ..Default::default()
        };

        let urls = config.get_grpc_urls();
        assert_eq!(urls.len(), 1);
        assert_eq!(urls[0], "https://primary.example.com");
    }

    #[rstest]
    fn test_config_get_grpc_urls_multiple() {
        let config = DydxAdapterConfig {
            grpc_url: "https://primary.example.com".to_string(),
            grpc_urls: vec![
                "https://fallback1.example.com".to_string(),
                "https://fallback2.example.com".to_string(),
            ],
            ..Default::default()
        };

        let urls = config.get_grpc_urls();
        assert_eq!(urls.len(), 2);
        assert_eq!(urls[0], "https://fallback1.example.com");
        assert_eq!(urls[1], "https://fallback2.example.com");
    }

    #[rstest]
    fn test_for_network_mainnet_resolves_urls_and_chain_id() {
        let config = DydxAdapterConfig::for_network(DydxNetwork::Mainnet);

        assert_eq!(config.network, DydxNetwork::Mainnet);
        assert_eq!(config.base_url, urls::http_base_url(DydxNetwork::Mainnet));
        assert_eq!(config.ws_url, urls::ws_url(DydxNetwork::Mainnet));
        assert_eq!(config.grpc_url, urls::grpc_urls(DydxNetwork::Mainnet)[0]);
        let expected_grpc: Vec<String> = urls::grpc_urls(DydxNetwork::Mainnet)
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(config.grpc_urls, expected_grpc);
        assert_eq!(config.chain_id, crate::common::consts::DYDX_CHAIN_ID);
        assert_eq!(config.get_chain_id(), ChainId::Mainnet1);
    }

    #[rstest]
    fn test_for_network_testnet_resolves_urls_and_chain_id() {
        let config = DydxAdapterConfig::for_network(DydxNetwork::Testnet);

        assert_eq!(config.network, DydxNetwork::Testnet);
        assert_eq!(config.base_url, urls::http_base_url(DydxNetwork::Testnet));
        assert_eq!(config.ws_url, urls::ws_url(DydxNetwork::Testnet));
        assert_eq!(config.grpc_url, urls::grpc_urls(DydxNetwork::Testnet)[0]);
        let expected_grpc: Vec<String> = urls::grpc_urls(DydxNetwork::Testnet)
            .iter()
            .map(|s| (*s).to_string())
            .collect();
        assert_eq!(config.grpc_urls, expected_grpc);
        assert_eq!(
            config.chain_id,
            crate::common::consts::DYDX_TESTNET_CHAIN_ID,
        );
        assert_eq!(config.get_chain_id(), ChainId::Testnet4);
    }

    #[rstest]
    #[case(DydxNetwork::Mainnet)]
    #[case(DydxNetwork::Testnet)]
    fn test_for_network_preserves_grpc_rate_limit_default(#[case] network: DydxNetwork) {
        // Regression guard: earlier implementations spread `..Self::builder().build()`,
        // which returned `None` and silently disabled gRPC throttling. The helper must
        // retain the `Some(4)` default from `Default::default()`.
        let config = DydxAdapterConfig::for_network(network);
        assert_eq!(config.grpc_rate_limit_per_second, Some(4));
        assert!(config.grpc_quota().is_some());
    }
}
