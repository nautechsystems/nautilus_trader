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

//! Configuration structures for the Hyperliquid adapter.

use crate::common::consts::{info_url, ws_url};

/// Configuration for the Hyperliquid data client.
#[derive(Clone, Debug)]
pub struct HyperliquidDataClientConfig {
    /// Optional private key for authenticated endpoints.
    pub private_key: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Override for the HTTP info URL.
    pub base_url_http: Option<String>,
    /// When true the client will use Hyperliquid testnet endpoints.
    pub is_testnet: bool,
    /// HTTP timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// WebSocket timeout in seconds.
    pub ws_timeout_secs: Option<u64>,
    /// Optional interval for refreshing instruments.
    pub update_instruments_interval_mins: Option<u64>,
}

impl Default for HyperliquidDataClientConfig {
    fn default() -> Self {
        Self {
            private_key: None,
            base_url_ws: None,
            base_url_http: None,
            is_testnet: false,
            http_timeout_secs: Some(60),
            ws_timeout_secs: Some(30),
            update_instruments_interval_mins: Some(60),
        }
    }
}

impl HyperliquidDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` when private key is populated.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key.is_some()
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
#[derive(Clone, Debug)]
pub struct HyperliquidExecClientConfig {
    /// Private key for signing transactions (required for execution).
    pub private_key: String,
    /// Optional vault address for vault operations.
    pub vault_address: Option<String>,
    /// Override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Override for the HTTP info URL.
    pub base_url_http: Option<String>,
    /// Override for the exchange API URL.
    pub base_url_exchange: Option<String>,
    /// When true the client will use Hyperliquid testnet endpoints.
    pub is_testnet: bool,
    /// HTTP timeout in seconds.
    pub http_timeout_secs: u64,
    /// Maximum number of retry attempts for HTTP requests.
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    pub retry_delay_max_ms: u64,
}

impl Default for HyperliquidExecClientConfig {
    fn default() -> Self {
        Self {
            private_key: String::new(),
            vault_address: None,
            base_url_ws: None,
            base_url_http: None,
            base_url_exchange: None,
            is_testnet: false,
            http_timeout_secs: 60,
            max_retries: 3,
            retry_delay_initial_ms: 100,
            retry_delay_max_ms: 5000,
        }
    }
}

impl HyperliquidExecClientConfig {
    /// Creates a new configuration with the provided private key.
    #[must_use]
    pub fn new(private_key: String) -> Self {
        Self {
            private_key,
            ..Self::default()
        }
    }

    /// Returns `true` when private key is populated.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        !self.private_key.is_empty()
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
