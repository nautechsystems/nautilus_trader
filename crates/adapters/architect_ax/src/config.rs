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

use nautilus_model::identifiers::AccountId;

/// Configuration for the AX Exchange live data client.
#[derive(Clone, Debug)]
pub struct AxDataClientConfig {
    /// Optional API key for authenticated REST/WebSocket requests.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated REST/WebSocket requests.
    pub api_secret: Option<String>,
    /// Use sandbox environment (default: false).
    pub is_sandbox: bool,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    pub ws_proxy_url: Option<String>,
    /// Optional REST timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for REST requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry backoff in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry backoff in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Optional heartbeat interval (seconds) for WebSocket clients.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional receive window in milliseconds for signed requests.
    pub recv_window_ms: Option<u64>,
    /// Optional interval (minutes) for instrument refresh from REST.
    pub update_instruments_interval_mins: Option<u64>,
}

impl Default for AxDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            is_sandbox: false,
            base_url_http: None,
            base_url_ws_public: None,
            base_url_ws_private: None,
            http_proxy_url: None,
            ws_proxy_url: None,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            heartbeat_interval_secs: Some(20),
            recv_window_ms: Some(5_000),
            update_instruments_interval_mins: Some(60),
        }
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
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.is_sandbox {
                "https://gateway.sandbox.architect.exchange/api".to_string()
            } else {
                "https://gateway.architect.exchange/api".to_string()
            }
        })
    }

    /// Returns the public WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public.clone().unwrap_or_else(|| {
            if self.is_sandbox {
                "wss://gateway.sandbox.architect.exchange/ws/public".to_string()
            } else {
                "wss://gateway.architect.exchange/ws/public".to_string()
            }
        })
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private.clone().unwrap_or_else(|| {
            if self.is_sandbox {
                "wss://gateway.sandbox.architect.exchange/ws/private".to_string()
            } else {
                "wss://gateway.architect.exchange/ws/private".to_string()
            }
        })
    }

    /// Returns `true` when private WebSocket connection is required.
    #[must_use]
    pub fn requires_private_ws(&self) -> bool {
        self.has_api_credentials()
    }
}

/// Configuration for the AX Exchange live execution client.
#[derive(Clone, Debug)]
pub struct AxExecClientConfig {
    /// API key for authenticated requests.
    pub api_key: Option<String>,
    /// API secret for authenticated requests.
    pub api_secret: Option<String>,
    /// Use sandbox environment (default: false).
    pub is_sandbox: bool,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional HTTP proxy URL.
    pub http_proxy_url: Option<String>,
    /// Optional WebSocket proxy URL.
    pub ws_proxy_url: Option<String>,
    /// Optional REST timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for REST requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry backoff in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry backoff in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Optional heartbeat interval (seconds) for WebSocket clients.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional receive window in milliseconds for signed requests.
    pub recv_window_ms: Option<u64>,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
}

impl Default for AxExecClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            is_sandbox: false,
            base_url_http: None,
            base_url_ws_private: None,
            http_proxy_url: None,
            ws_proxy_url: None,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            heartbeat_interval_secs: Some(5),
            recv_window_ms: Some(5_000),
            account_id: None,
        }
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
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http.clone().unwrap_or_else(|| {
            if self.is_sandbox {
                "https://gateway.sandbox.architect.exchange/api".to_string()
            } else {
                "https://gateway.architect.exchange/api".to_string()
            }
        })
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private.clone().unwrap_or_else(|| {
            if self.is_sandbox {
                "wss://gateway.sandbox.architect.exchange/ws/private".to_string()
            } else {
                "wss://gateway.architect.exchange/ws/private".to_string()
            }
        })
    }
}
