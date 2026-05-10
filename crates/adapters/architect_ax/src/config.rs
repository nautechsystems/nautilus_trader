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

//! Configuration structures for the AX Exchange adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;

use crate::common::{credential::credential_env_vars, enums::AxEnvironment};

/// Configuration for the AX Exchange live data client.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.architect",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.architect_ax")
)]
#[derive(Clone, Debug, bon::Builder)]
pub struct AxDataClientConfig {
    /// Optional API key for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Trading environment (Sandbox or Production).
    #[builder(default)]
    pub environment: AxEnvironment,
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
    /// Heartbeat interval (seconds) for WebSocket clients.
    #[builder(default = 20)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Interval (minutes) for instrument refresh from REST.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Funding rate poll interval in minutes.
    #[builder(default = 15)]
    pub funding_rate_poll_interval_mins: u64,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for AxDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl AxDataClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| self.environment.http_url().to_string())
    }

    /// Returns the public WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public
            .clone()
            .unwrap_or_else(|| self.environment.ws_md_url().to_string())
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| self.environment.ws_orders_url().to_string())
    }
}

/// Configuration for the AX Exchange live execution client.
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.architect",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.architect_ax")
)]
#[derive(Clone, Debug, bon::Builder)]
pub struct AxExecClientConfig {
    /// The trader ID for the client.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// The account ID for the client.
    #[builder(default = AccountId::from("AX-001"))]
    pub account_id: AccountId,
    /// API key for authenticated requests.
    pub api_key: Option<String>,
    /// API secret for authenticated requests.
    pub api_secret: Option<String>,
    /// Trading environment (Sandbox or Production).
    #[builder(default)]
    pub environment: AxEnvironment,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the orders REST base URL.
    pub base_url_orders: Option<String>,
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
    /// Heartbeat interval (seconds) for WebSocket clients.
    #[builder(default = 30)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Cancel all open orders when the orders WebSocket disconnects.
    #[builder(default)]
    pub cancel_on_disconnect: bool,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for AxExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl AxExecClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| self.environment.http_url().to_string())
    }

    /// Returns the orders REST base URL, considering overrides and environment.
    #[must_use]
    pub fn orders_base_url(&self) -> String {
        self.base_url_orders
            .clone()
            .unwrap_or_else(|| self.environment.orders_url().to_string())
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| self.environment.ws_orders_url().to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::consts::{
        AX_HTTP_SANDBOX_URL, AX_HTTP_URL, AX_ORDERS_SANDBOX_URL, AX_ORDERS_URL, AX_WS_PRIVATE_URL,
        AX_WS_PUBLIC_URL, AX_WS_SANDBOX_PRIVATE_URL, AX_WS_SANDBOX_PUBLIC_URL,
    };

    #[rstest]
    fn test_data_config_sandbox_urls_match_consts() {
        let config = AxDataClientConfig::builder()
            .environment(AxEnvironment::Sandbox)
            .build();
        assert_eq!(config.http_base_url(), AX_HTTP_SANDBOX_URL);
        assert_eq!(config.ws_public_url(), AX_WS_SANDBOX_PUBLIC_URL);
        assert_eq!(config.ws_private_url(), AX_WS_SANDBOX_PRIVATE_URL);
    }

    #[rstest]
    fn test_data_config_production_urls_match_consts() {
        let config = AxDataClientConfig::builder()
            .environment(AxEnvironment::Production)
            .build();
        assert_eq!(config.http_base_url(), AX_HTTP_URL);
        assert_eq!(config.ws_public_url(), AX_WS_PUBLIC_URL);
        assert_eq!(config.ws_private_url(), AX_WS_PRIVATE_URL);
    }

    #[rstest]
    fn test_data_config_url_overrides() {
        let config = AxDataClientConfig::builder()
            .base_url_http("http://custom".to_string())
            .base_url_ws_public("ws://custom-pub".to_string())
            .base_url_ws_private("ws://custom-priv".to_string())
            .build();
        assert_eq!(config.http_base_url(), "http://custom");
        assert_eq!(config.ws_public_url(), "ws://custom-pub");
        assert_eq!(config.ws_private_url(), "ws://custom-priv");
    }

    #[rstest]
    fn test_exec_config_sandbox_urls_match_consts() {
        let config = AxExecClientConfig::builder()
            .environment(AxEnvironment::Sandbox)
            .build();
        assert_eq!(config.http_base_url(), AX_HTTP_SANDBOX_URL);
        assert_eq!(config.orders_base_url(), AX_ORDERS_SANDBOX_URL);
        assert_eq!(config.ws_private_url(), AX_WS_SANDBOX_PRIVATE_URL);
    }

    #[rstest]
    fn test_exec_config_production_urls_match_consts() {
        let config = AxExecClientConfig::builder()
            .environment(AxEnvironment::Production)
            .build();
        assert_eq!(config.http_base_url(), AX_HTTP_URL);
        assert_eq!(config.orders_base_url(), AX_ORDERS_URL);
        assert_eq!(config.ws_private_url(), AX_WS_PRIVATE_URL);
    }

    #[rstest]
    fn test_exec_config_cancel_on_disconnect_default_false() {
        let config = AxExecClientConfig::default();
        assert!(!config.cancel_on_disconnect);
    }

    #[rstest]
    fn test_exec_config_cancel_on_disconnect_enabled() {
        let config = AxExecClientConfig::builder()
            .cancel_on_disconnect(true)
            .build();
        assert!(config.cancel_on_disconnect);
    }

    #[rstest]
    fn test_default_environment_is_sandbox() {
        let data = AxDataClientConfig::default();
        assert_eq!(data.environment, AxEnvironment::Sandbox);

        let exec = AxExecClientConfig::default();
        assert_eq!(exec.environment, AxEnvironment::Sandbox);
    }
}
