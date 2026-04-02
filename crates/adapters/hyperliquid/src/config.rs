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

use crate::common::consts::{info_url, ws_url};

/// Configuration for the Hyperliquid data client.
#[derive(Clone, Debug, bon::Builder)]
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
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    ///
    /// Note: WebSocket proxy support is not yet implemented. This field is reserved
    /// for future functionality. Use `http_proxy_url` for REST API proxy support.
    pub ws_proxy_url: Option<String>,
    /// When true the client will use Hyperliquid testnet endpoints.
    #[builder(default)]
    pub is_testnet: bool,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Interval for refreshing instruments in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
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

    /// Returns the WebSocket URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.is_testnet).to_string())
    }

    /// Returns the HTTP info URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| info_url(self.is_testnet).to_string())
    }
}

/// Configuration for the Hyperliquid execution client.
#[derive(Clone, Debug, bon::Builder)]
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
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    ///
    /// Note: WebSocket proxy support is not yet implemented. This field is reserved
    /// for future functionality. Use `http_proxy_url` for REST API proxy support.
    pub ws_proxy_url: Option<String>,
    /// When true the client will use Hyperliquid testnet endpoints.
    #[builder(default)]
    pub is_testnet: bool,
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
}

impl Default for HyperliquidExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl HyperliquidExecClientConfig {
    /// Creates a new configuration with the provided private key.
    #[must_use]
    pub fn new(private_key: Option<String>) -> Self {
        Self {
            private_key,
            ..Self::default()
        }
    }

    /// Returns `true` when private key is populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the WebSocket URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| ws_url(self.is_testnet).to_string())
    }

    /// Returns the HTTP info URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| info_url(self.is_testnet).to_string())
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
}
