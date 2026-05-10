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

//! Configuration structures for the Polymarket adapter.

use crate::common::{enums::SignatureType, urls};

/// Configuration for the Polymarket data client.
#[derive(Clone, Debug)]
pub struct PolymarketDataClientConfig {
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_gamma: Option<String>,
    pub http_timeout_secs: Option<u64>,
    pub ws_timeout_secs: Option<u64>,
    pub ws_max_subscriptions: usize,
    /// Instrument reload interval in minutes.
    pub update_instruments_interval_mins: Option<u64>,
}

impl Default for PolymarketDataClientConfig {
    fn default() -> Self {
        Self {
            base_url_http: None,
            base_url_ws: None,
            base_url_gamma: None,
            http_timeout_secs: Some(60),
            ws_timeout_secs: Some(30),
            ws_max_subscriptions: crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS,
            update_instruments_interval_mins: Some(60),
        }
    }
}

impl PolymarketDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| urls::clob_http_url().to_string())
    }

    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::clob_ws_url().to_string())
    }

    #[must_use]
    pub fn gamma_url(&self) -> String {
        self.base_url_gamma
            .clone()
            .unwrap_or_else(|| urls::gamma_api_url().to_string())
    }
}

/// Configuration for the Polymarket execution client.
#[derive(Clone, Debug)]
pub struct PolymarketExecClientConfig {
    /// Falls back to `POLYMARKET_PK` env var.
    pub private_key: Option<String>,
    /// Falls back to `POLYMARKET_API_KEY` env var.
    pub api_key: Option<String>,
    /// Falls back to `POLYMARKET_API_SECRET` env var.
    pub api_secret: Option<String>,
    /// Falls back to `POLYMARKET_PASSPHRASE` env var.
    pub passphrase: Option<String>,
    /// Falls back to `POLYMARKET_FUNDER` env var.
    pub funder: Option<String>,
    pub signature_type: SignatureType,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_gamma: Option<String>,
    pub http_timeout_secs: u64,
    pub max_retries: u32,
    pub retry_delay_initial_ms: u64,
    pub retry_delay_max_ms: u64,
    /// Timeout waiting for WS order acknowledgment (seconds).
    pub ack_timeout_secs: u64,
}

impl Default for PolymarketExecClientConfig {
    fn default() -> Self {
        Self {
            private_key: None,
            api_key: None,
            api_secret: None,
            passphrase: None,
            funder: None,
            signature_type: SignatureType::Eoa,
            base_url_http: None,
            base_url_ws: None,
            base_url_gamma: None,
            http_timeout_secs: 60,
            max_retries: 3,
            retry_delay_initial_ms: 1000,
            retry_delay_max_ms: 10000,
            ack_timeout_secs: 5,
        }
    }
}

impl PolymarketExecClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.private_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .api_key
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
    }

    #[must_use]
    pub fn http_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| urls::clob_http_url().to_string())
    }

    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::clob_ws_url().to_string())
    }

    #[must_use]
    pub fn gamma_url(&self) -> String {
        self.base_url_gamma
            .clone()
            .unwrap_or_else(|| urls::gamma_api_url().to_string())
    }
}
