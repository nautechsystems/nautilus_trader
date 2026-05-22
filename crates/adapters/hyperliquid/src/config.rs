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

//! Configuration structures for the Hyperliquid adapter.

use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::{info_url, ws_url},
    enums::HyperliquidEnvironment,
};

/// Configuration for the Hyperliquid data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.hyperliquid")
)]
pub struct HyperliquidDataClientConfig {
    /// Optional private key for authenticated endpoints.
    pub private_key: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Override for the HTTP info URL.
    pub base_url_http: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The target environment (mainnet or testnet).
    #[builder(default)]
    pub environment: HyperliquidEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for HyperliquidDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl HyperliquidDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when private key is populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the WebSocket URL, respecting the environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.environment).to_string())
    }

    /// Returns the HTTP info URL, respecting the environment and overrides.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| info_url(self.environment).to_string())
    }
}

/// Configuration for the Hyperliquid execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.hyperliquid",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.hyperliquid")
)]
pub struct HyperliquidExecClientConfig {
    /// Private key for signing transactions.
    ///
    /// If not provided, falls back to environment variable:
    /// - Mainnet: `HYPERLIQUID_PK`
    /// - Testnet: `HYPERLIQUID_TESTNET_PK`
    pub private_key: Option<String>,
    /// Optional vault address for vault operations.
    pub vault_address: Option<String>,
    /// Optional main account address when using an agent wallet (API sub-key).
    /// When set, used for balance queries, position reports, and WS subscriptions
    /// instead of the address derived from the private key.
    pub account_address: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Override for the HTTP info URL.
    pub base_url_http: Option<String>,
    /// Override for the exchange API URL.
    pub base_url_exchange: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The target environment (mainnet or testnet).
    #[builder(default)]
    pub environment: HyperliquidEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for HTTP requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 100)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 5000)]
    pub retry_delay_max_ms: u64,
    /// When true, normalize order prices to 5 significant figures
    /// before submission (Hyperliquid requirement).
    #[builder(default = true)]
    pub normalize_prices: bool,
    /// Slippage buffer in basis points applied to MARKET orders and
    /// stop-to-limit trigger derivations. Can be overridden per-order via
    /// `SubmitOrder.params["market_order_slippage_bps"]`.
    #[builder(default = 50)]
    pub market_order_slippage_bps: u32,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Timeout in seconds for WebSocket post trading requests.
    #[builder(default = 10)]
    pub ws_post_timeout_secs: u64,
    /// Poll interval in seconds for `outcomeMeta` settlement detection.
    /// Disabled by default; venue `Settlement` fills drive HIP-4 settlement
    /// through the standard user-fills stream. Set to a non-zero value only
    /// when the venue fill stream is unavailable.
    #[builder(default = 0)]
    pub outcome_settlement_poll_secs: u64,
}

impl Default for HyperliquidExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl HyperliquidExecClientConfig {
    /// Returns `true` when private key is populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the WebSocket URL, respecting the environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.environment).to_string())
    }

    /// Returns the HTTP info URL, respecting the environment and overrides.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| info_url(self.environment).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_exec_config_default_account_address_is_none() {
        let config = HyperliquidExecClientConfig::default();
        assert!(config.account_address.is_none());
    }

    #[rstest]
    fn test_exec_config_with_account_address() {
        let config = HyperliquidExecClientConfig {
            account_address: Some("0x1234".to_string()),
            ..HyperliquidExecClientConfig::default()
        };
        assert_eq!(config.account_address.as_deref(), Some("0x1234"));
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: HyperliquidDataClientConfig = toml::from_str(
            r#"
environment = "testnet"
http_timeout_secs = 30
update_instruments_interval_mins = 10
transport_backend = "tungstenite"
"#,
        )
        .unwrap();

        assert_eq!(config.environment, HyperliquidEnvironment::Testnet);
        assert_eq!(config.http_timeout_secs, 30);
        assert_eq!(config.update_instruments_interval_mins, 10);
        assert_eq!(config.transport_backend, TransportBackend::Tungstenite);
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: HyperliquidExecClientConfig = toml::from_str("").unwrap();
        let expected = HyperliquidExecClientConfig::default();

        assert_eq!(config.environment, expected.environment);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(config.max_retries, expected.max_retries);
        assert_eq!(config.normalize_prices, expected.normalize_prices);
        assert_eq!(
            config.market_order_slippage_bps,
            expected.market_order_slippage_bps,
        );
        assert_eq!(config.transport_backend, expected.transport_backend);
        assert_eq!(config.ws_post_timeout_secs, expected.ws_post_timeout_secs);
        assert_eq!(
            config.outcome_settlement_poll_secs,
            expected.outcome_settlement_poll_secs,
        );
    }
}
