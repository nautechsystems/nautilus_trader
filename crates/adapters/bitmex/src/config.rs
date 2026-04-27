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

//! Configuration types for the BitMEX adapter clients.

use nautilus_model::identifiers::AccountId;
use nautilus_network::websocket::TransportBackend;

use crate::common::{
    consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL, BITMEX_WS_TESTNET_URL, BITMEX_WS_URL},
    credential::credential_env_vars,
    enums::BitmexEnvironment,
};

/// Configuration for the BitMEX live data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bitmex")
)]
pub struct BitmexDataClientConfig {
    /// Optional API key used for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret used for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
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
    /// Optional heartbeat interval (seconds) for the WebSocket client.
    pub heartbeat_interval_secs: Option<u64>,
    /// Receive window in milliseconds for signed requests.
    ///
    /// This value determines how far in the future the `api-expires` timestamp will be set
    /// for signed REST requests. BitMEX uses seconds-granularity Unix timestamps in the
    /// `api-expires` header, calculated as: `current_timestamp + (recv_window_ms / 1000)`.
    ///
    /// **Note**: This parameter is specified in milliseconds for consistency with other
    /// adapter configurations (e.g., Bybit's `recv_window_ms`), but BitMEX only supports
    /// seconds-granularity timestamps. The value is converted via integer division, so
    /// 10000ms becomes 10 seconds, 15500ms becomes 15 seconds, etc.
    ///
    /// A larger window provides more tolerance for clock skew and network latency, but
    /// increases the replay attack window. The default of 10 seconds should be sufficient
    /// for most deployments. Consider increasing this value (e.g., to 30_000ms = 30s) if you
    /// experience request expiration errors due to clock drift or high network latency.
    #[builder(default = 10_000)]
    pub recv_window_ms: u64,
    /// When `true`, only active instruments are requested during bootstrap.
    #[builder(default = true)]
    pub active_only: bool,
    /// Optional interval (minutes) for instrument refresh from REST.
    pub update_instruments_interval_mins: Option<u64>,
    /// BitMEX environment (mainnet or testnet).
    #[builder(default)]
    pub environment: BitmexEnvironment,
    /// Maximum number of requests per second (burst limit).
    #[builder(default = 10)]
    pub max_requests_per_second: u32,
    /// Maximum number of requests per minute (rolling window).
    #[builder(default = 120)]
    pub max_requests_per_minute: u32,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for BitmexDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitmexDataClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available
    /// (either explicitly set or resolvable from environment variables).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and the environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_HTTP_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_HTTP_URL.to_string(),
            })
    }

    /// Returns the WebSocket URL, considering overrides and the environment.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_WS_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_WS_URL.to_string(),
            })
    }
}

/// Configuration for the BitMEX live execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bitmex")
)]
pub struct BitmexExecClientConfig {
    /// API key used for authenticated requests.
    pub api_key: Option<String>,
    /// API secret used for authenticated requests.
    pub api_secret: Option<String>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
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
    /// Heartbeat interval (seconds) for the WebSocket client.
    #[builder(default = 5)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    ///
    /// This value determines how far in the future the `api-expires` timestamp will be set
    /// for signed REST requests. BitMEX uses seconds-granularity Unix timestamps in the
    /// `api-expires` header, calculated as: `current_timestamp + (recv_window_ms / 1000)`.
    ///
    /// **Note**: This parameter is specified in milliseconds for consistency with other
    /// adapter configurations (e.g., Bybit's `recv_window_ms`), but BitMEX only supports
    /// seconds-granularity timestamps. The value is converted via integer division, so
    /// 10000ms becomes 10 seconds, 15500ms becomes 15 seconds, etc.
    ///
    /// A larger window provides more tolerance for clock skew and network latency, but
    /// increases the replay attack window. The default of 10 seconds should be sufficient
    /// for most deployments. Consider increasing this value (e.g., to 30000ms = 30s) if you
    /// experience request expiration errors due to clock drift or high network latency.
    #[builder(default = 10_000)]
    pub recv_window_ms: u64,
    /// When `true`, only active instruments are requested during bootstrap.
    #[builder(default = true)]
    pub active_only: bool,
    /// BitMEX environment (mainnet or testnet).
    #[builder(default)]
    pub environment: BitmexEnvironment,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
    /// Maximum number of requests per second (burst limit).
    #[builder(default = 10)]
    pub max_requests_per_second: u32,
    /// Maximum number of requests per minute (rolling window).
    #[builder(default = 120)]
    pub max_requests_per_minute: u32,
    /// Number of HTTP clients in the submit broadcaster pool (defaults to 1).
    pub submitter_pool_size: Option<usize>,
    /// Number of HTTP clients in the cancel broadcaster pool (defaults to 1).
    pub canceller_pool_size: Option<usize>,
    /// Optional list of proxy URLs for submit broadcaster pool (path diversity).
    pub submitter_proxy_urls: Option<Vec<String>>,
    /// Optional list of proxy URLs for cancel broadcaster pool (path diversity).
    pub canceller_proxy_urls: Option<Vec<String>>,
    /// Optional dead man's switch timeout in seconds.
    ///
    /// When set, a background task periodically calls the BitMEX `cancelAllAfter` endpoint
    /// to keep a server-side timer alive. If the client loses connectivity the timer expires
    /// and BitMEX cancels all open orders. Calling with `timeout=0` disarms the switch.
    /// The refresh interval is derived as `timeout / 4` (minimum 1 second).
    pub deadmans_switch_timeout_secs: Option<u64>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for BitmexExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitmexExecClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available
    /// (either explicitly set or resolvable from environment variables).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and the environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_HTTP_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_HTTP_URL.to_string(),
            })
    }

    /// Returns the WebSocket URL, considering overrides and the environment.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_WS_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_WS_URL.to_string(),
            })
    }
}
