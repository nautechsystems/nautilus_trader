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

//! Configuration types for the BitMEX adapter clients.

use nautilus_model::identifiers::AccountId;

use crate::common::consts::{
    BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL, BITMEX_WS_TESTNET_URL, BITMEX_WS_URL,
};

/// Configuration for the BitMEX live data client.
#[derive(Clone, Debug)]
pub struct BitmexDataClientConfig {
    /// Optional API key used for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret used for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional REST timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for REST requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry backoff in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry backoff in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Optional heartbeat interval (seconds) for the WebSocket client.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional receive window in milliseconds for signed requests (default 10_000).
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
    pub recv_window_ms: Option<u64>,
    /// When `true`, only active instruments are requested during bootstrap.
    pub active_only: bool,
    /// Optional interval (minutes) for instrument refresh from REST.
    pub update_instruments_interval_mins: Option<u64>,
    /// When `true`, use BitMEX testnet endpoints by default.
    pub use_testnet: bool,
    /// Maximum number of requests per second (burst limit).
    pub max_requests_per_second: Option<u32>,
    /// Maximum number of requests per minute (rolling window).
    pub max_requests_per_minute: Option<u32>,
}

impl Default for BitmexDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            base_url_http: None,
            base_url_ws: None,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            heartbeat_interval_secs: None,
            recv_window_ms: Some(10_000),
            active_only: true,
            update_instruments_interval_mins: None,
            use_testnet: false,
            max_requests_per_second: Some(10),
            max_requests_per_minute: Some(120),
        }
    }
}

impl BitmexDataClientConfig {
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

    /// Returns the REST base URL, considering overrides and the testnet flag.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.use_testnet {
                BITMEX_HTTP_TESTNET_URL.to_string()
            } else {
                BITMEX_HTTP_URL.to_string()
            }
        })
    }

    /// Returns the WebSocket URL, considering overrides and the testnet flag.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws.clone().unwrap_or_else(|| {
            if self.use_testnet {
                BITMEX_WS_TESTNET_URL.to_string()
            } else {
                BITMEX_WS_URL.to_string()
            }
        })
    }
}

/// Configuration for the BitMEX live execution client.
#[derive(Clone, Debug)]
pub struct BitmexExecClientConfig {
    /// API key used for authenticated requests.
    pub api_key: Option<String>,
    /// API secret used for authenticated requests.
    pub api_secret: Option<String>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional REST timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for REST requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry backoff in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry backoff in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Optional heartbeat interval (seconds) for the WebSocket client.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional receive window in milliseconds for signed requests (default 10000).
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
    pub recv_window_ms: Option<u64>,
    /// When `true`, only active instruments are requested during bootstrap.
    pub active_only: bool,
    /// When `true`, use BitMEX testnet endpoints by default.
    pub use_testnet: bool,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
    /// Maximum number of requests per second (burst limit).
    pub max_requests_per_second: Option<u32>,
    /// Maximum number of requests per minute (rolling window).
    pub max_requests_per_minute: Option<u32>,
}

impl Default for BitmexExecClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            base_url_http: None,
            base_url_ws: None,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            heartbeat_interval_secs: Some(5),
            recv_window_ms: Some(10_000),
            active_only: true,
            use_testnet: false,
            account_id: None,
            max_requests_per_second: Some(10),
            max_requests_per_minute: Some(120),
        }
    }
}

impl BitmexExecClientConfig {
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

    /// Returns the REST base URL, considering overrides and the testnet flag.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.use_testnet {
                BITMEX_HTTP_TESTNET_URL.to_string()
            } else {
                BITMEX_HTTP_URL.to_string()
            }
        })
    }

    /// Returns the WebSocket URL, considering overrides and the testnet flag.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws.clone().unwrap_or_else(|| {
            if self.use_testnet {
                BITMEX_WS_TESTNET_URL.to_string()
            } else {
                BITMEX_WS_URL.to_string()
            }
        })
    }
}
