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

//! Configuration structures for the Coinbase adapter.

use nautilus_model::enums::AccountType;

use crate::common::{
    enums::{CoinbaseEnvironment, CoinbaseMarginType},
    urls,
};

/// Configuration for the Coinbase data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseDataClientConfig {
    /// CDP API key name (falls back to `COINBASE_API_KEY` env var).
    pub api_key: Option<String>,
    /// CDP API secret in PEM format (falls back to `COINBASE_API_SECRET` env var).
    pub api_secret: Option<String>,
    /// Override for the REST API base URL.
    pub base_url_rest: Option<String>,
    /// Override for the WebSocket market data URL.
    pub base_url_ws: Option<String>,
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    pub ws_proxy_url: Option<String>,
    /// The Coinbase environment to connect to.
    #[builder(default)]
    pub environment: CoinbaseEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
}

impl Default for CoinbaseDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl CoinbaseDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when credentials are populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            && self
                .api_secret
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the REST API base URL, respecting environment and overrides.
    #[must_use]
    pub fn rest_url(&self) -> String {
        self.base_url_rest
            .clone()
            .unwrap_or_else(|| urls::rest_url(self.environment).to_string())
    }

    /// Returns the WebSocket market data URL, respecting environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_url(self.environment).to_string())
    }
}

/// Configuration for the Coinbase execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.coinbase", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.coinbase")
)]
pub struct CoinbaseExecClientConfig {
    /// CDP API key name (falls back to `COINBASE_API_KEY` env var).
    pub api_key: Option<String>,
    /// CDP API secret in PEM format (falls back to `COINBASE_API_SECRET` env var).
    pub api_secret: Option<String>,
    /// Override for the REST API base URL.
    pub base_url_rest: Option<String>,
    /// Override for the WebSocket user data URL.
    pub base_url_ws: Option<String>,
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    pub ws_proxy_url: Option<String>,
    /// The Coinbase environment to connect to.
    #[builder(default)]
    pub environment: CoinbaseEnvironment,
    /// HTTP timeout in seconds.
    #[builder(default = 10)]
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
    /// Nautilus account type for the factory. The Cash factory ignores this and
    /// hardcodes Cash; the derivatives factory sets it to Margin.
    #[builder(default = AccountType::Cash)]
    pub account_type: AccountType,
    /// Optional default margin type applied to derivatives orders. Ignored on
    /// Cash accounts.
    pub default_margin_type: Option<CoinbaseMarginType>,
    /// Optional default leverage applied to derivatives orders. Ignored on
    /// Cash accounts.
    pub default_leverage: Option<rust_decimal::Decimal>,
}

impl Default for CoinbaseExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl CoinbaseExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when credentials are populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            && self
                .api_secret
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the REST API base URL, respecting environment and overrides.
    #[must_use]
    pub fn rest_url(&self) -> String {
        self.base_url_rest
            .clone()
            .unwrap_or_else(|| urls::rest_url(self.environment).to_string())
    }

    /// Returns the WebSocket user data URL, respecting environment and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_user_url(self.environment).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_defaults() {
        let config = CoinbaseDataClientConfig::default();
        assert_eq!(config.environment, CoinbaseEnvironment::Live);
        assert_eq!(config.http_timeout_secs, 10);
        assert_eq!(config.ws_timeout_secs, 30);
        assert_eq!(config.update_instruments_interval_mins, 60);
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_data_config_has_credentials() {
        let config = CoinbaseDataClientConfig {
            api_key: Some("key".to_string()),
            api_secret: Some("secret".to_string()),
            ..CoinbaseDataClientConfig::default()
        };
        assert!(config.has_credentials());
    }

    #[rstest]
    fn test_data_config_empty_credentials() {
        let config = CoinbaseDataClientConfig {
            api_key: Some("  ".to_string()),
            api_secret: Some("secret".to_string()),
            ..CoinbaseDataClientConfig::default()
        };
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_data_config_urls_live() {
        let config = CoinbaseDataClientConfig::default();
        assert!(config.rest_url().contains("api.coinbase.com"));
        assert!(config.ws_url().contains("advanced-trade-ws.coinbase.com"));
    }

    #[rstest]
    fn test_data_config_urls_sandbox() {
        let config = CoinbaseDataClientConfig {
            environment: CoinbaseEnvironment::Sandbox,
            ..CoinbaseDataClientConfig::default()
        };
        assert!(config.rest_url().contains("sandbox"));
        assert!(config.ws_url().contains("sandbox"));
    }

    #[rstest]
    fn test_exec_config_defaults() {
        let config = CoinbaseExecClientConfig::default();
        assert_eq!(config.environment, CoinbaseEnvironment::Live);
        assert_eq!(config.http_timeout_secs, 10);
        assert_eq!(config.max_retries, 3);
    }

    #[rstest]
    fn test_exec_config_ws_url_uses_user_endpoint() {
        let config = CoinbaseExecClientConfig::default();
        assert!(config.ws_url().contains("user"));
    }
}
