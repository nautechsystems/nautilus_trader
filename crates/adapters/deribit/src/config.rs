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

//! Configuration structures for the Deribit adapter.

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;

use crate::{
    common::{
        credential::credential_env_vars,
        enums::DeribitEnvironment,
        urls::{get_http_base_url, get_ws_url},
    },
    http::models::DeribitProductType,
};

/// Configuration for the Deribit data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.deribit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.deribit")
)]
pub struct DeribitDataClientConfig {
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Product types to load (e.g., Future, Option, Spot).
    #[builder(default = vec![DeribitProductType::Future])]
    pub product_types: Vec<DeribitProductType>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The Deribit environment (mainnet or testnet).
    #[builder(default)]
    pub environment: DeribitEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval in seconds for WebSocket connection.
    #[builder(default = 30)]
    pub heartbeat_interval_secs: u64,
    /// Interval for refreshing instruments (in minutes).
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for DeribitDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl DeribitDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_env, secret_env) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_env).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_env).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL, respecting the environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| get_ws_url(self.environment).to_string())
    }
}

/// Configuration for the Deribit execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.deribit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.deribit")
)]
pub struct DeribitExecClientConfig {
    /// The trader ID for this client.
    #[builder(default)]
    pub trader_id: TraderId,
    /// The account ID for this client.
    #[builder(default = AccountId::from("DERIBIT-001"))]
    pub account_id: AccountId,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Product types to load (e.g., Future, Option, Spot).
    #[builder(default = vec![DeribitProductType::Future])]
    pub product_types: Vec<DeribitProductType>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    /// The Deribit environment (mainnet or testnet).
    #[builder(default)]
    pub environment: DeribitEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for DeribitExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl DeribitExecClientConfig {
    /// Returns `true` when API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_env, secret_env) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_env).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_env).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.environment).to_string())
    }

    /// Returns the WebSocket URL, respecting the environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| get_ws_url(self.environment).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config() {
        let config = DeribitDataClientConfig::default();
        assert_eq!(config.environment, DeribitEnvironment::Mainnet);
        assert_eq!(config.product_types.len(), 1);
        assert_eq!(config.http_timeout_secs, 60);
    }

    #[rstest]
    fn test_http_base_url_default() {
        let config = DeribitDataClientConfig::default();
        assert_eq!(config.http_base_url(), "https://www.deribit.com");
    }

    #[rstest]
    fn test_http_base_url_testnet() {
        let config = DeribitDataClientConfig {
            environment: DeribitEnvironment::Testnet,
            ..Default::default()
        };
        assert_eq!(config.http_base_url(), "https://test.deribit.com");
    }

    #[rstest]
    fn test_ws_url_default() {
        let config = DeribitDataClientConfig::default();
        assert_eq!(config.ws_url(), "wss://www.deribit.com/ws/api/v2");
    }

    #[rstest]
    fn test_ws_url_testnet() {
        let config = DeribitDataClientConfig {
            environment: DeribitEnvironment::Testnet,
            ..Default::default()
        };
        assert_eq!(config.ws_url(), "wss://test.deribit.com/ws/api/v2");
    }

    #[rstest]
    fn test_has_api_credentials_in_config() {
        let config = DeribitDataClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            ..Default::default()
        };
        assert!(config.has_api_credentials());
    }
}
