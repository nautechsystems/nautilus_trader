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

//! Configuration structures for the Massive adapter.

use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{enums::MassiveDataFeed, urls};

/// Configuration for the Massive data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.massive", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.massive")
)]
pub struct MassiveDataClientConfig {
    /// Massive API key (falls back to the `MASSIVE_API_KEY` env var).
    pub api_key: Option<String>,
    /// Override for the REST API base URL.
    pub base_url_rest: Option<String>,
    /// Override for the WebSocket market data URL.
    pub base_url_ws: Option<String>,
    /// The market data feed to stream from (plan dependent).
    #[builder(default)]
    pub feed: MassiveDataFeed,
    /// Tickers to load as instruments on connect. When empty, every active
    /// US stocks-market ticker is loaded (several thousand instruments).
    #[builder(default)]
    pub symbols: Vec<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Request split- and dividend-adjusted aggregate bars.
    #[builder(default = true)]
    pub adjusted_bars: bool,
    /// Timestamp bars on the close of the aggregate window (Nautilus
    /// convention). When false, REST bars are timestamped on the window open.
    #[builder(default = true)]
    pub bars_timestamp_on_close: bool,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(MassiveDataClientConfig {
    base_url_rest: Option<String>,
    base_url_ws: Option<String>,
    feed: MassiveDataFeed,
    symbols: Vec<String>,
    http_timeout_secs: u64,
    adjusted_bars: bool,
    bars_timestamp_on_close: bool,
    transport_backend: TransportBackend,
});

impl Default for MassiveDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl MassiveDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns true when an API key is populated and non-empty.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.api_key
            .as_deref()
            .is_some_and(|s| !s.trim().is_empty())
    }

    /// Returns the REST API base URL, respecting overrides.
    #[must_use]
    pub fn rest_url(&self) -> String {
        self.base_url_rest
            .clone()
            .unwrap_or_else(|| urls::rest_url().to_string())
    }

    /// Returns the WebSocket market data URL, respecting feed and overrides.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| urls::ws_url(self.feed).to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_config_defaults() {
        let config = MassiveDataClientConfig::default();
        assert_eq!(config.feed, MassiveDataFeed::RealTime);
        assert_eq!(config.http_timeout_secs, 60);
        assert!(config.symbols.is_empty());
        assert!(config.adjusted_bars);
        assert!(config.bars_timestamp_on_close);
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_config_has_credentials() {
        let config = MassiveDataClientConfig {
            api_key: Some("key".to_string()),
            ..MassiveDataClientConfig::default()
        };
        assert!(config.has_credentials());
    }

    #[rstest]
    fn test_config_empty_credentials() {
        let config = MassiveDataClientConfig {
            api_key: Some("  ".to_string()),
            ..MassiveDataClientConfig::default()
        };
        assert!(!config.has_credentials());
    }

    #[rstest]
    fn test_config_urls_realtime() {
        let config = MassiveDataClientConfig::default();
        assert_eq!(config.rest_url(), "https://api.massive.com");
        assert_eq!(config.ws_url(), "wss://socket.massive.com/stocks");
    }

    #[rstest]
    fn test_config_urls_delayed() {
        let config = MassiveDataClientConfig {
            feed: MassiveDataFeed::Delayed,
            ..MassiveDataClientConfig::default()
        };
        assert_eq!(config.ws_url(), "wss://delayed.massive.com/stocks");
    }

    #[rstest]
    fn test_config_url_overrides() {
        let config = MassiveDataClientConfig {
            base_url_rest: Some("http://localhost:8080".to_string()),
            base_url_ws: Some("ws://localhost:8081".to_string()),
            ..MassiveDataClientConfig::default()
        };
        assert_eq!(config.rest_url(), "http://localhost:8080");
        assert_eq!(config.ws_url(), "ws://localhost:8081");
    }

    #[rstest]
    fn test_config_toml_minimal() {
        let config: MassiveDataClientConfig = toml::from_str(
            r#"
feed = "Delayed"
http_timeout_secs = 5
symbols = ["AAPL", "MSFT"]
"#,
        )
        .unwrap();

        assert_eq!(config.feed, MassiveDataFeed::Delayed);
        assert_eq!(config.http_timeout_secs, 5);
        assert_eq!(config.symbols, vec!["AAPL", "MSFT"]);
    }

    #[rstest]
    fn test_config_toml_empty_uses_defaults() {
        let config: MassiveDataClientConfig = toml::from_str("").unwrap();
        let expected = MassiveDataClientConfig::default();

        assert_eq!(config.feed, expected.feed);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(config.transport_backend, expected.transport_backend);
    }
}
