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

//! Configuration structures for the Bybit adapter.

use std::collections::HashMap;

use nautilus_model::identifiers::AccountId;
use nautilus_network::websocket::TransportBackend;

use crate::common::{
    enums::{BybitEnvironment, BybitMarginMode, BybitPositionMode, BybitProductType},
    urls::{bybit_http_base_url, bybit_ws_private_url, bybit_ws_public_url, bybit_ws_trade_url},
};

/// Configuration for the Bybit live data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
pub struct BybitDataClientConfig {
    /// Optional API key for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Product types to subscribe to (e.g., Linear, Spot, Inverse, Option).
    #[builder(default = vec![BybitProductType::Linear])]
    pub product_types: Vec<BybitProductType>,
    /// Environment selection (Mainnet, Testnet, Demo).
    #[builder(default = BybitEnvironment::Mainnet)]
    pub environment: BybitEnvironment,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry backoff in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval in seconds for WebSocket clients.
    #[builder(default = 20)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Interval in minutes for instrument refresh from REST.
    /// When `None`, instrument refresh is disabled.
    pub update_instruments_interval_mins: Option<u64>,
    /// Interval in seconds for polling instrument status changes.
    /// When `None`, status polling is disabled.
    pub instrument_status_poll_secs: Option<u64>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for BybitDataClientConfig {
    fn default() -> Self {
        Self {
            update_instruments_interval_mins: Some(60),
            instrument_status_poll_secs: Some(60),
            ..Self::builder().build()
        }
    }
}

impl BybitDataClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| bybit_http_base_url(self.environment).to_string())
    }

    /// Returns the public WebSocket URL for the given product type.
    ///
    /// Falls back to the first product type in the config if multiple are configured.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public.clone().unwrap_or_else(|| {
            let product_type = self
                .product_types
                .first()
                .copied()
                .unwrap_or(BybitProductType::Linear);
            bybit_ws_public_url(product_type, self.environment)
        })
    }

    /// Returns the public WebSocket URL for a specific product type.
    #[must_use]
    pub fn ws_public_url_for(&self, product_type: BybitProductType) -> String {
        self.base_url_ws_public
            .clone()
            .unwrap_or_else(|| bybit_ws_public_url(product_type, self.environment))
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| bybit_ws_private_url(self.environment).to_string())
    }

    /// Returns `true` when private WebSocket connection is required.
    #[must_use]
    pub fn requires_private_ws(&self) -> bool {
        self.has_api_credentials()
    }
}

/// Configuration for the Bybit live execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bybit")
)]
pub struct BybitExecClientConfig {
    /// API key for authenticated requests.
    pub api_key: Option<String>,
    /// API secret for authenticated requests.
    pub api_secret: Option<String>,
    /// Product types to support (e.g., Linear, Spot, Inverse, Option).
    #[builder(default = vec![BybitProductType::Linear])]
    pub product_types: Vec<BybitProductType>,
    /// Environment selection (Mainnet, Testnet, Demo).
    #[builder(default = BybitEnvironment::Mainnet)]
    pub environment: BybitEnvironment,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional override for the trade WebSocket URL.
    pub base_url_ws_trade: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry backoff in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval in seconds for WebSocket clients.
    #[builder(default = 5)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
    /// Whether to generate position reports from wallet balances for SPOT positions.
    #[builder(default)]
    pub use_spot_position_reports: bool,
    /// Leverage configuration for futures (symbol -> leverage).
    pub futures_leverages: Option<HashMap<String, u32>>,
    /// Position mode configuration for symbols (symbol -> mode).
    pub position_mode: Option<HashMap<String, BybitPositionMode>>,
    /// Unified margin mode setting.
    pub margin_mode: Option<BybitMarginMode>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for BybitExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BybitExecClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| bybit_http_base_url(self.environment).to_string())
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| bybit_ws_private_url(self.environment).to_string())
    }

    /// Returns the trade WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_trade_url(&self) -> String {
        self.base_url_ws_trade
            .clone()
            .unwrap_or_else(|| bybit_ws_trade_url(self.environment).to_string())
    }
}
#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_default() {
        let config = BybitDataClientConfig::default();

        assert!(!config.has_api_credentials());
        assert_eq!(config.product_types, vec![BybitProductType::Linear]);
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.heartbeat_interval_secs, 20);
    }

    #[rstest]
    fn test_data_config_with_credentials() {
        let config = BybitDataClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };

        assert!(config.has_api_credentials());
        assert!(config.requires_private_ws());
    }

    #[rstest]
    fn test_data_config_http_url_mainnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_testnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-testnet.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_demo() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Demo,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-demo.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_override() {
        let custom_url = "https://custom.bybit.com";
        let config = BybitDataClientConfig {
            base_url_http: Some(custom_url.to_string()),
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), custom_url);
    }

    #[rstest]
    fn test_data_config_ws_public_url() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_public_url(),
            "wss://stream.bybit.com/v5/public/linear"
        );
    }

    #[rstest]
    fn test_data_config_ws_public_url_for_spot() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_public_url_for(BybitProductType::Spot),
            "wss://stream.bybit.com/v5/public/spot"
        );
    }

    #[rstest]
    fn test_data_config_ws_private_url() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.ws_private_url(), "wss://stream.bybit.com/v5/private");
    }

    #[rstest]
    fn test_data_config_ws_private_url_testnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_private_url(),
            "wss://stream-testnet.bybit.com/v5/private"
        );
    }

    #[rstest]
    fn test_exec_config_default() {
        let config = BybitExecClientConfig::default();

        assert!(!config.has_api_credentials());
        assert_eq!(config.product_types, vec![BybitProductType::Linear]);
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.heartbeat_interval_secs, 5);
    }

    #[rstest]
    fn test_exec_config_with_credentials() {
        let config = BybitExecClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };

        assert!(config.has_api_credentials());
    }

    #[rstest]
    fn test_exec_config_urls() {
        let config = BybitExecClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api.bybit.com");
        assert_eq!(config.ws_private_url(), "wss://stream.bybit.com/v5/private");
        assert_eq!(config.ws_trade_url(), "wss://stream.bybit.com/v5/trade");
    }

    #[rstest]
    fn test_exec_config_urls_testnet() {
        let config = BybitExecClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-testnet.bybit.com");
        assert_eq!(
            config.ws_private_url(),
            "wss://stream-testnet.bybit.com/v5/private"
        );
        assert_eq!(
            config.ws_trade_url(),
            "wss://stream-testnet.bybit.com/v5/trade"
        );
    }

    #[rstest]
    fn test_exec_config_custom_urls() {
        let config = BybitExecClientConfig {
            base_url_http: Some("https://custom-http.bybit.com".to_string()),
            base_url_ws_private: Some("wss://custom-private.bybit.com".to_string()),
            base_url_ws_trade: Some("wss://custom-trade.bybit.com".to_string()),
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://custom-http.bybit.com");
        assert_eq!(config.ws_private_url(), "wss://custom-private.bybit.com");
        assert_eq!(config.ws_trade_url(), "wss://custom-trade.bybit.com");
    }
}
