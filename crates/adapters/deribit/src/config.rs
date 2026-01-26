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

use crate::{
    common::urls::{get_http_base_url, get_ws_url},
    http::models::DeribitInstrumentKind,
};

/// Configuration for the Deribit data client.
#[derive(Clone, Debug)]
pub struct DeribitDataClientConfig {
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Instrument kinds to load (e.g., Future, Option, Spot).
    pub instrument_kinds: Vec<DeribitInstrumentKind>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// When true the client will use Deribit testnet endpoints.
    pub use_testnet: bool,
    /// Optional HTTP timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry delay in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry delay in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
    /// Optional heartbeat interval in seconds for WebSocket connection.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional interval for refreshing instruments (in minutes).
    pub update_instruments_interval_mins: Option<u64>,
}

impl Default for DeribitDataClientConfig {
    fn default() -> Self {
        Self {
            api_key: None,
            api_secret: None,
            instrument_kinds: vec![DeribitInstrumentKind::Future],
            base_url_http: None,
            base_url_ws: None,
            use_testnet: false,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
            heartbeat_interval_secs: Some(30),
            update_instruments_interval_mins: Some(60),
        }
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
        let (key_env, secret_env) = if self.use_testnet {
            ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
        } else {
            ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
        };

        let has_key = self.api_key.is_some() || std::env::var(key_env).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_env).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.use_testnet).to_string())
    }

    /// Returns the WebSocket URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| get_ws_url(self.use_testnet).to_string())
    }
}

/// Configuration for the Deribit execution client.
#[derive(Clone, Debug)]
pub struct DeribitExecClientConfig {
    /// The trader ID for this client.
    pub trader_id: TraderId,
    /// The account ID for this client.
    pub account_id: AccountId,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<String>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<String>,
    /// Instrument kinds to load (e.g., Future, Option, Spot).
    pub instrument_kinds: Vec<DeribitInstrumentKind>,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// When true the client will use Deribit testnet endpoints.
    pub use_testnet: bool,
    /// Optional HTTP timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// Optional maximum retry attempts for requests.
    pub max_retries: Option<u32>,
    /// Optional initial retry delay in milliseconds.
    pub retry_delay_initial_ms: Option<u64>,
    /// Optional maximum retry delay in milliseconds.
    pub retry_delay_max_ms: Option<u64>,
}

impl Default for DeribitExecClientConfig {
    fn default() -> Self {
        Self {
            trader_id: TraderId::default(),
            account_id: AccountId::from("DERIBIT-001"),
            api_key: None,
            api_secret: None,
            instrument_kinds: vec![DeribitInstrumentKind::Future],
            base_url_http: None,
            base_url_ws: None,
            use_testnet: false,
            http_timeout_secs: Some(60),
            max_retries: Some(3),
            retry_delay_initial_ms: Some(1_000),
            retry_delay_max_ms: Some(10_000),
        }
    }
}

impl DeribitExecClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new(trader_id: TraderId, account_id: AccountId) -> Self {
        Self {
            trader_id,
            account_id,
            ..Default::default()
        }
    }

    /// Returns `true` when API credentials are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_env, secret_env) = if self.use_testnet {
            ("DERIBIT_TESTNET_API_KEY", "DERIBIT_TESTNET_API_SECRET")
        } else {
            ("DERIBIT_API_KEY", "DERIBIT_API_SECRET")
        };

        let has_key = self.api_key.is_some() || std::env::var(key_env).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_env).is_ok();
        has_key && has_secret
    }

    /// Returns the HTTP base URL, falling back to the default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.use_testnet).to_string())
    }

    /// Returns the WebSocket URL, respecting the testnet flag and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| get_ws_url(self.use_testnet).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_default_config() {
        let config = DeribitDataClientConfig::default();
        assert!(!config.use_testnet);
        assert_eq!(config.instrument_kinds.len(), 1);
        assert_eq!(config.http_timeout_secs, Some(60));
    }

    #[rstest]
    fn test_http_base_url_default() {
        let config = DeribitDataClientConfig::default();
        assert_eq!(config.http_base_url(), "https://www.deribit.com");
    }

    #[rstest]
    fn test_http_base_url_testnet() {
        let config = DeribitDataClientConfig {
            use_testnet: true,
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
            use_testnet: true,
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
