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

//! Configuration structures for the dYdX adapter.

use serde::{Deserialize, Serialize};

use crate::{
    common::{
        consts::{DYDX_CHAIN_ID, DYDX_GRPC_URLS, DYDX_TESTNET_CHAIN_ID, DYDX_WS_URL},
        enums::DydxNetwork,
        urls,
    },
    grpc::types::ChainId,
};

/// Configuration for the dYdX adapter.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxAdapterConfig {
    /// Network environment (mainnet or testnet).
    #[serde(default)]
    pub network: DydxNetwork,
    /// Base URL for the HTTP API.
    pub base_url: String,
    /// Base URL for the WebSocket API.
    pub ws_url: String,
    /// Base URL for the gRPC API (Cosmos SDK transactions).
    ///
    /// For backwards compatibility, a single URL can be provided.
    /// Consider using `grpc_urls` for fallback support.
    pub grpc_url: String,
    /// List of gRPC URLs with fallback support.
    ///
    /// If provided, the client will attempt to connect to each URL in order
    /// until a successful connection is established. This is recommended for
    /// production use in DEX environments where nodes can fail.
    #[serde(default)]
    pub grpc_urls: Vec<String>,
    /// Chain ID (e.g., "dydx-mainnet-1" for mainnet, "dydx-testnet-4" for testnet).
    pub chain_id: String,
    /// Request timeout in seconds.
    pub timeout_secs: u64,
    /// Wallet address for the account (optional, can be derived from mnemonic).
    #[serde(default)]
    pub wallet_address: Option<String>,
    /// Subaccount number (default: 0).
    #[serde(default)]
    pub subaccount: u32,
    /// Whether this is a testnet configuration.
    ///
    /// Precedence: `network` is canonical. If both `network` and `is_testnet`
    /// are provided and conflict, `network` takes precedence internally.
    /// This flag exists for backwards compatibility and may be derived from
    /// `network` in future versions.
    #[serde(default)]
    pub is_testnet: bool,
    /// Mnemonic phrase for wallet (optional, loaded from environment if not provided).
    #[serde(default)]
    pub mnemonic: Option<String>,
    /// Maximum number of retries for failed requests (default: 3).
    #[serde(default = "default_max_retries")]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds (default: 1000ms).
    #[serde(default = "default_retry_delay_initial_ms")]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds (default: 10000ms).
    #[serde(default = "default_retry_delay_max_ms")]
    pub retry_delay_max_ms: u64,
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

impl DydxAdapterConfig {
    /// Get the list of gRPC URLs to use for connection with fallback support.
    ///
    /// Returns `grpc_urls` if non-empty, otherwise falls back to a single-element
    /// vector containing `grpc_url`.
    #[must_use]
    pub fn get_grpc_urls(&self) -> Vec<String> {
        if !self.grpc_urls.is_empty() {
            self.grpc_urls.clone()
        } else {
            vec![self.grpc_url.clone()]
        }
    }

    /// Map the configured network to the underlying chain ID.
    ///
    /// This is the recommended way to get the chain ID for transaction submission.
    #[must_use]
    pub const fn get_chain_id(&self) -> ChainId {
        self.network.chain_id()
    }

    /// Convenience: compute `is_testnet` from `network`.
    ///
    /// Prefer `network` as the source of truth; this method is provided to
    /// avoid ambiguity when legacy configs include `is_testnet`.
    #[must_use]
    pub const fn compute_is_testnet(&self) -> bool {
        matches!(self.network, DydxNetwork::Testnet)
    }
}

impl Default for DydxAdapterConfig {
    fn default() -> Self {
        let network = DydxNetwork::default();
        let is_testnet = matches!(network, DydxNetwork::Testnet);
        let grpc_urls = urls::grpc_urls(is_testnet);
        Self {
            network,
            base_url: urls::http_base_url(is_testnet).to_string(),
            ws_url: urls::ws_url(is_testnet).to_string(),
            grpc_url: grpc_urls[0].to_string(),
            grpc_urls: grpc_urls.iter().map(|&s| s.to_string()).collect(),
            chain_id: if is_testnet {
                DYDX_TESTNET_CHAIN_ID
            } else {
                DYDX_CHAIN_ID
            }
            .to_string(),
            timeout_secs: 30,
            wallet_address: None,
            subaccount: 0,
            is_testnet,
            mnemonic: None,
            max_retries: default_max_retries(),
            retry_delay_initial_ms: default_retry_delay_initial_ms(),
            retry_delay_max_ms: default_retry_delay_max_ms(),
        }
    }
}

/// Configuration for the dYdX data client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DydxDataClientConfig {
    /// Base URL for the HTTP API.
    pub base_url_http: Option<String>,
    /// Base URL for the WebSocket API.
    pub base_url_ws: Option<String>,
    /// HTTP request timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Maximum number of retry attempts for failed HTTP requests.
    pub max_retries: Option<u64>,
    /// Initial retry delay in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Maximum retry delay in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Whether this is a testnet configuration.
    pub is_testnet: bool,
    /// HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// WebSocket proxy URL.
    pub ws_proxy_url: Option<String>,
    /// Orderbook snapshot refresh interval in seconds (prevents stale books from missed messages).
    /// Set to None to disable periodic refresh. Default: 60 seconds.
    pub orderbook_refresh_interval_secs: Option<u64>,
    /// Instrument refresh interval in seconds (updates instrument definitions periodically).
    /// Set to None to disable periodic refresh. Default: 3600 seconds (60 minutes).
    pub instrument_refresh_interval_secs: Option<u64>,
}

impl Default for DydxDataClientConfig {
    fn default() -> Self {
        Self {
            base_url_http: None,
            base_url_ws: None,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(100),
            retry_delay_max_ms: Some(5000),
            is_testnet: false,
            http_proxy_url: None,
            ws_proxy_url: None,
            orderbook_refresh_interval_secs: Some(60),
            instrument_refresh_interval_secs: Some(3600),
        }
    }
}

/// Configuration for the dYdX execution client.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DYDXExecClientConfig {
    /// gRPC endpoint URL.
    pub grpc_endpoint: String,
    /// WebSocket endpoint URL.
    pub ws_endpoint: String,
    /// Wallet mnemonic for signing transactions.
    pub mnemonic: Option<String>,
    /// Wallet address.
    pub wallet_address: Option<String>,
    /// Subaccount number (default: 0).
    pub subaccount_number: u32,
    /// HTTP request timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Maximum number of retry attempts.
    pub max_retries: Option<u64>,
    /// Initial retry delay in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Maximum retry delay in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Whether this is a testnet configuration.
    pub is_testnet: bool,
}

impl Default for DYDXExecClientConfig {
    fn default() -> Self {
        Self {
            grpc_endpoint: DYDX_GRPC_URLS[0].to_string(),
            ws_endpoint: DYDX_WS_URL.to_string(),
            mnemonic: None,
            wallet_address: None,
            subaccount_number: 0,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(100),
            retry_delay_max_ms: Some(5000),
            is_testnet: false,
        }
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
    fn test_config_compute_is_testnet() {
        let mainnet_config = DydxAdapterConfig {
            network: DydxNetwork::Mainnet,
            ..Default::default()
        };
        assert!(!mainnet_config.compute_is_testnet());

        let testnet_config = DydxAdapterConfig {
            network: DydxNetwork::Testnet,
            ..Default::default()
        };
        assert!(testnet_config.compute_is_testnet());
    }

    #[rstest]
    fn test_config_default_uses_mainnet() {
        let config = DydxAdapterConfig::default();
        assert_eq!(config.network, DydxNetwork::Mainnet);
        assert!(!config.is_testnet);
    }

    #[rstest]
    fn test_config_network_canonical_over_is_testnet() {
        // When network=mainnet but is_testnet=true, get_chain_id uses network
        let config = DydxAdapterConfig {
            network: DydxNetwork::Mainnet,
            is_testnet: true, // Conflicting value
            ..Default::default()
        };
        assert_eq!(config.get_chain_id(), ChainId::Mainnet1); // network wins
        assert!(!config.compute_is_testnet()); // compute_is_testnet derives from network
    }

    #[rstest]
    fn test_config_serde_backwards_compat() {
        // Test that configs missing network field can deserialize with default
        let json = r#"{"base_url":"https://indexer.dydx.trade","ws_url":"wss://indexer.dydx.trade/v4/ws","grpc_url":"https://dydx-ops-grpc.kingnodes.com:443","grpc_urls":[],"chain_id":"dydx-mainnet-1","timeout_secs":30,"subaccount":0,"is_testnet":false,"max_retries":3,"retry_delay_initial_ms":1000,"retry_delay_max_ms":10000}"#;

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
}
