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

//! Configuration for the Kalshi adapter.

use crate::common::urls;

/// Configuration for the Kalshi data client.
#[derive(Clone, Debug)]
pub struct KalshiDataClientConfig {
    /// REST base URL. Defaults to production.
    pub base_url: Option<String>,
    /// WebSocket base URL. Defaults to production.
    pub ws_url: Option<String>,
    /// HTTP request timeout in seconds. Default: 60.
    pub http_timeout_secs: u64,
    /// WebSocket connection timeout in seconds. Default: 30.
    pub ws_timeout_secs: u64,
    /// Series tickers to include, e.g. `["KXBTC", "PRES-2024"]`.
    pub series_tickers: Vec<String>,
    /// Optional additional filter by event ticker.
    pub event_tickers: Vec<String>,
    /// How often to refresh instruments (minutes).
    pub instrument_reload_interval_mins: u64,
    /// REST requests per second. Default: 20 (Basic tier).
    pub rate_limit_rps: u32,
    /// Kalshi API key ID. Falls back to `KALSHI_API_KEY_ID` env var.
    pub api_key_id: Option<String>,
    /// RSA private key in PEM format. Falls back to `KALSHI_PRIVATE_KEY_PEM` env var.
    pub private_key_pem: Option<String>,
}

impl Default for KalshiDataClientConfig {
    fn default() -> Self {
        Self {
            base_url: None,
            ws_url: None,
            http_timeout_secs: 60,
            ws_timeout_secs: 30,
            series_tickers: Vec::new(),
            event_tickers: Vec::new(),
            instrument_reload_interval_mins: 60,
            rate_limit_rps: 20,
            api_key_id: None,
            private_key_pem: None,
        }
    }
}

impl KalshiDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url
            .clone()
            .unwrap_or_else(|| urls::rest_base_url().to_string())
    }

    #[must_use]
    pub fn websocket_url(&self) -> String {
        self.ws_url
            .clone()
            .unwrap_or_else(|| urls::ws_base_url().to_string())
    }

    /// Resolve credentials from config fields or environment variables.
    #[must_use]
    pub fn resolved_api_key_id(&self) -> Option<String> {
        self.api_key_id
            .clone()
            .or_else(|| std::env::var("KALSHI_API_KEY_ID").ok())
    }

    /// Resolve credentials from config fields or environment variables.
    #[must_use]
    pub fn resolved_private_key_pem(&self) -> Option<String> {
        self.private_key_pem
            .clone()
            .or_else(|| std::env::var("KALSHI_PRIVATE_KEY_PEM").ok())
    }
}
