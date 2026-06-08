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

use std::{
    collections::HashMap,
    fmt::Debug,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use nautilus_model::identifiers::{AccountId, InstrumentId, TraderId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::{
    common::{enums::SignatureType, urls},
    filters::InstrumentFilter,
};

const DEFAULT_UPDOWN_INTERVAL_MINS: u64 = 5;
const DEFAULT_UPDOWN_PERIODS: u64 = 3;

fn default_updown_assets() -> Vec<String> {
    vec!["btc".to_string()]
}

/// Rust-backed event slug builder for Polymarket Up/Down markets.
///
/// Up/Down event slugs follow the pattern
/// `{asset}-updown-{interval_mins}m-{unix_timestamp}`, where the timestamp is
/// aligned to the start of the interval. The builder emits slugs for each
/// configured asset and period.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
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
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
)]
pub struct PolymarketUpDownEventSlugConfig {
    /// Asset codes used in the slug prefix.
    #[builder(default = default_updown_assets())]
    pub assets: Vec<String>,
    /// Up/Down interval in minutes.
    #[builder(default = DEFAULT_UPDOWN_INTERVAL_MINS)]
    pub interval_mins: u64,
    /// Number of periods to generate.
    #[builder(default = DEFAULT_UPDOWN_PERIODS)]
    pub periods: u64,
    /// Offset from the current aligned period.
    #[builder(default)]
    pub start_offset_periods: i64,
}

impl Default for PolymarketUpDownEventSlugConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl PolymarketUpDownEventSlugConfig {
    /// Builds event slugs using the current system time.
    ///
    /// # Errors
    ///
    /// Returns an error if the interval or period count is zero, all assets are
    /// blank, or the configured offset resolves before the Unix epoch.
    pub fn build_event_slugs(&self) -> anyhow::Result<Vec<String>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("system clock before Unix epoch: {e}"))?
            .as_secs();
        self.build_event_slugs_at_unix_secs(now)
    }

    fn build_event_slugs_at_unix_secs(&self, unix_secs: u64) -> anyhow::Result<Vec<String>> {
        if self.interval_mins == 0 {
            anyhow::bail!("event_slug_builder.interval_mins must be positive");
        }

        if self.periods == 0 {
            anyhow::bail!("event_slug_builder.periods must be positive");
        }

        let assets = self.normalized_assets();
        if assets.is_empty() {
            anyhow::bail!("event_slug_builder.assets must include at least one non-empty asset");
        }

        let period_secs = self
            .interval_mins
            .checked_mul(60)
            .ok_or_else(|| anyhow::anyhow!("event_slug_builder.interval_mins is too large"))?;
        let period_start = (unix_secs / period_secs) * period_secs;
        let period_secs = i128::from(period_secs);
        let period_start = i128::from(period_start);
        let mut slugs = Vec::new();

        for period in 0..self.periods {
            let period_offset = i128::from(self.start_offset_periods) + i128::from(period);
            let timestamp = period_start + period_offset * period_secs;
            if timestamp < 0 {
                anyhow::bail!("event_slug_builder offset resolves before the Unix epoch");
            }

            for asset in &assets {
                slugs.push(format!(
                    "{asset}-updown-{}m-{timestamp}",
                    self.interval_mins
                ));
            }
        }

        Ok(slugs)
    }

    fn normalized_assets(&self) -> Vec<String> {
        let mut assets = Vec::new();

        for asset in &self.assets {
            let asset = asset.trim().to_ascii_lowercase();
            if asset.is_empty() || assets.contains(&asset) {
                continue;
            }
            assets.push(asset);
        }

        assets
    }
}

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
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
)]
pub struct PolymarketInstrumentProviderConfig {
    /// Whether all venue instruments should be loaded on startup.
    #[builder(default)]
    pub load_all: bool,
    /// Optional instrument IDs to load on startup instead of a full bootstrap.
    pub load_ids: Option<Vec<InstrumentId>>,
    /// Optional Gamma-style query filters encoded as string key/value pairs.
    pub filters: Option<HashMap<String, String>>,
    /// Optional static event slugs to resolve to markets during bootstrap.
    pub event_slugs: Option<Vec<String>>,
    /// Optional static market slugs to load directly during bootstrap.
    pub market_slugs: Option<Vec<String>>,
    /// Optional Rust-backed Up/Down event slug builder.
    pub event_slug_builder: Option<PolymarketUpDownEventSlugConfig>,
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
            || self.event_slug_builder.is_some()
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
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
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
    /// Maximum concurrent instrument fetches spawned from `new_market` events.
    ///
    /// This bounds adapter-side fan-out during event bursts and prevents
    /// request storms against Gamma.
    #[builder(default = 8)]
    pub new_market_fetch_max_concurrency: usize,
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
    /// Whether automatic resolve polling is enabled.
    #[builder(default = true)]
    pub resolve_poll_enabled: bool,
    /// Fixed interval between resolve poll cycles in seconds.
    #[builder(default = 30)]
    pub resolve_poll_interval_secs: u64,
    /// Grace period after expiration before a market becomes resolve poll eligible.
    #[builder(default = 10)]
    pub resolve_poll_grace_secs: u64,
    /// Maximum number of seconds to keep auto-polling after expiration before pausing.
    #[builder(default = 1800)]
    pub resolve_poll_max_wait_secs: u64,
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
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
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
    fn updown_event_slug_config_builds_aligned_slugs() {
        let config = PolymarketUpDownEventSlugConfig {
            assets: vec![
                "BTC".to_string(),
                " eth ".to_string(),
                String::new(),
                "btc".to_string(),
            ],
            interval_mins: 5,
            periods: 2,
            start_offset_periods: -1,
        };

        let slugs = config
            .build_event_slugs_at_unix_secs(1_700_000_123)
            .expect("event slugs should build");

        assert_eq!(
            slugs,
            [
                "btc-updown-5m-1699999800",
                "eth-updown-5m-1699999800",
                "btc-updown-5m-1700000100",
                "eth-updown-5m-1700000100",
            ]
        );
    }

    #[rstest]
    fn updown_event_slug_config_rejects_zero_interval() {
        let config = PolymarketUpDownEventSlugConfig {
            interval_mins: 0,
            ..PolymarketUpDownEventSlugConfig::default()
        };

        let err = config
            .build_event_slugs_at_unix_secs(1_700_000_123)
            .expect_err("zero interval should fail");

        assert!(
            err.to_string()
                .contains("event_slug_builder.interval_mins must be positive")
        );
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: PolymarketDataClientConfig = toml::from_str(
            "
http_timeout_secs = 30
ws_max_subscriptions = 50
update_instruments_interval_mins = 5
subscribe_new_markets = true
new_market_fetch_max_concurrency = 16
auto_load_debounce_ms = 250
resolve_poll_enabled = true
resolve_poll_interval_secs = 30
resolve_poll_grace_secs = 10
resolve_poll_max_wait_secs = 1800
",
        )
        .unwrap();

        assert_eq!(config.http_timeout_secs, 30);
        assert_eq!(config.ws_max_subscriptions, 50);
        assert_eq!(config.update_instruments_interval_mins, Some(5));
        assert!(config.subscribe_new_markets);
        assert_eq!(config.new_market_fetch_max_concurrency, 16);
        assert_eq!(config.auto_load_debounce_ms, 250);
        assert!(config.instrument_config.is_none());
        assert!(config.resolve_poll_enabled);
        assert_eq!(config.resolve_poll_interval_secs, 30);
        assert_eq!(config.resolve_poll_grace_secs, 10);
        assert_eq!(config.resolve_poll_max_wait_secs, 1800);
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
