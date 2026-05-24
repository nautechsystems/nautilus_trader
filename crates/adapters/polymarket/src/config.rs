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

use std::{collections::HashMap, fmt::Debug, sync::Arc};

use nautilus_model::identifiers::{AccountId, TraderId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::{
    common::{enums::SignatureType, urls},
    filters::InstrumentFilter,
};

/// Configuration for the Polymarket instrument provider.
///
/// This mirrors the Python adapter's `instrument_config` layering so scoped
/// market bootstrap can migrate naturally to the Rust/pyO3 live path.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.polymarket")
)]
pub struct PolymarketInstrumentProviderConfig {
    /// Whether all venue instruments should be loaded on startup.
    #[builder(default)]
    pub load_all: bool,
    /// Optional instrument IDs to load on startup instead of a full bootstrap.
    pub load_ids: Option<Vec<nautilus_model::identifiers::InstrumentId>>,
    /// Optional Gamma-style query filters encoded as string key/value pairs.
    pub filters: Option<HashMap<String, String>>,
    /// Optional static event slugs to resolve to markets during bootstrap.
    pub event_slugs: Option<Vec<String>>,
    /// Optional static market slugs to load directly during bootstrap.
    pub market_slugs: Option<Vec<String>>,
    /// Optional fully qualified Python callable path returning event slugs.
    ///
    /// This is provided for pyO3 compatibility with the Python Polymarket
    /// adapter. When used from Rust/pyO3, the callable is resolved and invoked
    /// from the Python runtime at bootstrap time.
    pub event_slug_builder: Option<String>,
    /// Whether provider warnings should be logged.
    #[builder(default = true)]
    pub log_warnings: bool,
    /// Compatibility field matching the Python adapter. The Rust provider
    /// already uses the Gamma API for bootstrap, so this currently has no
    /// behavioral effect beyond configuration parity.
    #[builder(default)]
    pub use_gamma_markets: bool,
}

impl Default for PolymarketInstrumentProviderConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl PolymarketInstrumentProviderConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn should_load_all(&self) -> bool {
        self.load_all
            || self
                .event_slug_builder
                .as_deref()
                .is_some_and(|s| !s.trim().is_empty())
            || self.event_slugs.as_ref().is_some_and(|s| !s.is_empty())
            || self.market_slugs.as_ref().is_some_and(|s| !s.is_empty())
    }

    #[must_use]
    pub fn has_load_ids(&self) -> bool {
        self.load_ids.as_ref().is_some_and(|ids| !ids.is_empty())
    }
}

/// Configuration for the Polymarket data client.
///
/// `filters` and `new_market_filter` hold `Arc<dyn InstrumentFilter>` trait objects
/// and are skipped during serialization; they default to empty/`None` and must be
/// installed programmatically after deserialization.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.polymarket")
)]
pub struct PolymarketDataClientConfig {
    pub instrument_config: Option<PolymarketInstrumentProviderConfig>,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_gamma: Option<String>,
    pub base_url_data_api: Option<String>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// WebSocket timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    #[builder(default = crate::common::consts::WS_DEFAULT_SUBSCRIPTIONS)]
    pub ws_max_subscriptions: usize,
    /// Instrument reload interval in minutes.
    pub update_instruments_interval_mins: Option<u64>,
    /// Whether to subscribe to new market discovery events via WebSocket.
    #[builder(default)]
    pub subscribe_new_markets: bool,
    /// Whether subscribe and request commands referencing an unknown instrument should
    /// trigger an ad-hoc load via the instrument provider. Concurrent misses within
    /// `auto_load_debounce_ms` are coalesced into a single batched request.
    #[builder(default = true)]
    pub auto_load_missing_instruments: bool,
    /// The window (milliseconds) over which concurrent auto-load requests are batched.
    #[builder(default = 100)]
    pub auto_load_debounce_ms: u64,
    /// Maximum retry attempts on transient auto-load failures (markets in the CLOB
    /// hydration window that return empty `clob_token_ids` from Gamma, or that are
    /// absent from the bulk response). Set to `0` to disable retry.
    #[builder(default = 12)]
    pub auto_load_max_retries: u32,
    /// Initial delay (seconds) between transient auto-load retries; backed off
    /// exponentially with positive jitter up to `auto_load_retry_delay_max_secs`.
    #[builder(default = 5.0)]
    pub auto_load_retry_delay_initial_secs: f64,
    /// Maximum delay (seconds) between transient auto-load retries.
    #[builder(default = 15.0)]
    pub auto_load_retry_delay_max_secs: f64,
    /// Instrument filters applied to all instruments during loading and discovery.
    #[builder(default)]
    #[serde(skip)]
    pub filters: Vec<Arc<dyn InstrumentFilter>>,
    /// Optional filter applied to newly discovered markets before instrument emission.
    #[serde(skip)]
    pub new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Default for PolymarketDataClientConfig {
    fn default() -> Self {
        Self {
            update_instruments_interval_mins: Some(60),
            ..Self::builder().build()
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

    #[must_use]
    pub fn data_api_url(&self) -> String {
        self.base_url_data_api
            .clone()
            .unwrap_or_else(|| "https://data-api.polymarket.com".to_string())
    }
}

/// Configuration for the Polymarket execution client.
///
/// `Debug` is implemented manually to redact secrets, so it is not part of the
/// derive list.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.polymarket",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.polymarket")
)]
pub struct PolymarketExecClientConfig {
    #[builder(default)]
    pub trader_id: TraderId,
    #[builder(default = AccountId::from("POLYMARKET-001"))]
    pub account_id: AccountId,
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
    #[builder(default = SignatureType::Eoa)]
    pub signature_type: SignatureType,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_data_api: Option<String>,
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    #[builder(default = 3)]
    pub max_retries: u32,
    #[builder(default = 1000)]
    pub retry_delay_initial_ms: u64,
    #[builder(default = 10000)]
    pub retry_delay_max_ms: u64,
    /// Timeout waiting for WS order acknowledgment (seconds).
    #[builder(default = 5)]
    pub ack_timeout_secs: u64,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

impl Debug for PolymarketExecClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketExecClientConfig))
            .field("trader_id", &self.trader_id)
            .field("account_id", &self.account_id)
            .field("private_key", &"***")
            .field("api_key", &"***")
            .field("api_secret", &"***")
            .field("passphrase", &"***")
            .field("funder", &self.funder)
            .field("signature_type", &self.signature_type)
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("base_url_data_api", &self.base_url_data_api)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("ack_timeout_secs", &self.ack_timeout_secs)
            .finish()
    }
}

impl Default for PolymarketExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
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
    pub fn data_api_url(&self) -> String {
        self.base_url_data_api
            .clone()
            .unwrap_or_else(|| "https://data-api.polymarket.com".to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: PolymarketDataClientConfig = toml::from_str(
            "
http_timeout_secs = 30
ws_max_subscriptions = 50
update_instruments_interval_mins = 5
subscribe_new_markets = true
auto_load_debounce_ms = 250
",
        )
        .unwrap();

        assert_eq!(config.http_timeout_secs, 30);
        assert_eq!(config.ws_max_subscriptions, 50);
        assert_eq!(config.update_instruments_interval_mins, Some(5));
        assert!(config.subscribe_new_markets);
        assert_eq!(config.auto_load_debounce_ms, 250);
        assert!(config.instrument_config.is_none());
        assert!(config.filters.is_empty());
        assert!(config.new_market_filter.is_none());
    }

    #[rstest]
    fn test_data_config_toml_with_instrument_config() {
        let config: PolymarketDataClientConfig = toml::from_str(
            r#"
[instrument_config]
load_all = true
event_slugs = ["btc-updown-5m-123", "eth-updown-15m-456"]
log_warnings = false
"#,
        )
        .unwrap();

        let instrument_config = config.instrument_config.expect("instrument_config");
        assert!(instrument_config.load_all);
        assert_eq!(
            instrument_config.event_slugs,
            Some(vec![
                "btc-updown-5m-123".to_string(),
                "eth-updown-15m-456".to_string(),
            ]),
        );
        assert!(!instrument_config.log_warnings);
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: PolymarketExecClientConfig = toml::from_str("").unwrap();
        let expected = PolymarketExecClientConfig::default();

        assert_eq!(config.trader_id, expected.trader_id);
        assert_eq!(config.account_id, expected.account_id);
        assert_eq!(config.signature_type, expected.signature_type);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(config.max_retries, expected.max_retries);
        assert_eq!(config.ack_timeout_secs, expected.ack_timeout_secs);
        assert_eq!(config.transport_backend, expected.transport_backend);
    }
}
