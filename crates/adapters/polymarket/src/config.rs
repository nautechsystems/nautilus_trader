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

use nautilus_core::string::secret::REDACTED;
use nautilus_model::identifiers::{AccountId, InstrumentId, TraderId};
use nautilus_network::{
    transport::TransportError,
    websocket::{TransportBackend, proxy::ProxyUrl},
};
use serde::{Deserialize, Serialize};

use crate::{
    common::{enums::SignatureType, urls},
    filters::InstrumentFilter,
};

const DEFAULT_UPDOWN_INTERVAL_MINS: u64 = 5;
const DEFAULT_UPDOWN_PERIODS: u64 = 3;

fn validated_proxy_url(value: Option<&String>) -> Result<Option<ProxyUrl>, TransportError> {
    value.cloned().map(ProxyUrl::parse).transpose()
}

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
    pyo3::pyclass(module = "nautilus_trader.adapters.polymarket", from_py_object)
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

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(PolymarketUpDownEventSlugConfig {
    assets: Vec<String>,
    interval_mins: u64,
    periods: u64,
    start_offset_periods: i64,
});

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
    pyo3::pyclass(module = "nautilus_trader.adapters.polymarket", from_py_object)
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
    /// Optional Gamma series IDs whose active events resolve to markets during bootstrap.
    pub series_ids: Option<Vec<u64>>,
    /// Whether provider warnings should be logged.
    #[builder(default = true)]
    pub log_warnings: bool,
    /// Compatibility field matching the Python adapter. The Rust provider
    /// already uses the Gamma API for bootstrap, so this currently has no
    /// behavioral effect beyond configuration parity.
    #[builder(default)]
    pub use_gamma_markets: bool,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(PolymarketInstrumentProviderConfig {
    load_all: bool,
    load_ids: Option<Vec<InstrumentId>>,
    filters: Option<HashMap<String, String>>,
    event_slugs: Option<Vec<String>>,
    market_slugs: Option<Vec<String>>,
    event_slug_builder: Option<PolymarketUpDownEventSlugConfig>,
    series_ids: Option<Vec<u64>>,
    log_warnings: bool,
    use_gamma_markets: bool,
});

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

    /// Returns whether any configured scope drives a bootstrap load.
    #[must_use]
    pub fn should_load_all(&self) -> bool {
        self.load_all || self.has_explicit_scope() || self.has_nonempty_filters()
    }

    /// Returns whether any explicit bootstrap scope (slug, builder, or series) is configured.
    #[must_use]
    pub fn has_explicit_scope(&self) -> bool {
        self.event_slug_builder.is_some()
            || self.event_slugs.as_ref().is_some_and(|s| !s.is_empty())
            || self.market_slugs.as_ref().is_some_and(|s| !s.is_empty())
            || self.has_series_ids()
    }

    #[must_use]
    pub fn has_series_ids(&self) -> bool {
        self.series_ids.as_ref().is_some_and(|ids| !ids.is_empty())
    }

    #[must_use]
    pub fn has_nonempty_filters(&self) -> bool {
        self.filters.as_ref().is_some_and(|map| !map.is_empty())
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
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.polymarket", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")
)]
pub struct PolymarketDataClientConfig {
    pub instrument_config: Option<PolymarketInstrumentProviderConfig>,
    /// Instrument filters applied to all instruments during loading and discovery.
    #[builder(default)]
    #[serde(skip)]
    pub filters: Vec<Arc<dyn InstrumentFilter>>,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_rtds: Option<String>,
    pub base_url_gamma: Option<String>,
    pub base_url_data_api: Option<String>,
    /// Optional HTTP or HTTPS proxy URL for all HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
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
    /// Whether to subscribe to new-market discovery, resolution, and best-bid/ask events.
    #[builder(default)]
    pub subscribe_new_markets: bool,
    /// Optional filter applied to newly discovered markets before instrument emission.
    #[serde(skip)]
    pub new_market_filter: Option<Arc<dyn InstrumentFilter>>,
    /// Maximum concurrent instrument fetches spawned from `new_market` events.
    ///
    /// This bounds adapter-side fan-out during event bursts and prevents
    /// request storms against Gamma.
    #[builder(default = 8)]
    pub new_market_fetch_max_concurrency: usize,
    /// Whether to drop quote ticks when bid or ask prices are missing.
    #[builder(default = true)]
    pub drop_quotes_missing_side: bool,
    /// Whether to maintain local book state and emit only the net changes from
    /// book snapshots, at an additional CPU and memory cost.
    #[builder(default)]
    pub compute_effective_deltas: bool,
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
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(PolymarketDataClientConfig {
    instrument_config: Option<PolymarketInstrumentProviderConfig>,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    base_url_gamma: Option<String>,
    base_url_data_api: Option<String>,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    ws_max_subscriptions: usize,
    update_instruments_interval_mins: Option<u64>,
    subscribe_new_markets: bool,
    auto_load_missing_instruments: bool,
    auto_load_debounce_ms: u64,
    auto_load_max_retries: u32,
    auto_load_retry_delay_initial_secs: f64,
    auto_load_retry_delay_max_secs: f64,
    new_market_fetch_max_concurrency: usize,
    resolve_poll_enabled: bool,
    resolve_poll_interval_secs: u64,
    resolve_poll_grace_secs: u64,
    resolve_poll_max_wait_secs: u64,
    base_url_rtds: Option<String>,
    transport_backend: TransportBackend,
    drop_quotes_missing_side: bool,
    compute_effective_deltas: bool,
});

impl Default for PolymarketDataClientConfig {
    fn default() -> Self {
        Self {
            update_instruments_interval_mins: Some(60),
            ..Self::builder().build()
        }
    }
}

impl Debug for PolymarketDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PolymarketDataClientConfig))
            .field("instrument_config", &self.instrument_config)
            .field("filters", &self.filters)
            .field("base_url_http", &self.base_url_http)
            .field("base_url_ws", &self.base_url_ws)
            .field("base_url_rtds", &self.base_url_rtds)
            .field("base_url_gamma", &self.base_url_gamma)
            .field("base_url_data_api", &self.base_url_data_api)
            .field("proxy_url", &self.proxy_url.as_ref().map(|_| REDACTED))
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("ws_timeout_secs", &self.ws_timeout_secs)
            .field("ws_max_subscriptions", &self.ws_max_subscriptions)
            .field(
                "update_instruments_interval_mins",
                &self.update_instruments_interval_mins,
            )
            .field("subscribe_new_markets", &self.subscribe_new_markets)
            .field("new_market_filter", &self.new_market_filter)
            .field(
                "new_market_fetch_max_concurrency",
                &self.new_market_fetch_max_concurrency,
            )
            .field("drop_quotes_missing_side", &self.drop_quotes_missing_side)
            .field("compute_effective_deltas", &self.compute_effective_deltas)
            .field(
                "auto_load_missing_instruments",
                &self.auto_load_missing_instruments,
            )
            .field("auto_load_debounce_ms", &self.auto_load_debounce_ms)
            .field("auto_load_max_retries", &self.auto_load_max_retries)
            .field(
                "auto_load_retry_delay_initial_secs",
                &self.auto_load_retry_delay_initial_secs,
            )
            .field(
                "auto_load_retry_delay_max_secs",
                &self.auto_load_retry_delay_max_secs,
            )
            .field("resolve_poll_enabled", &self.resolve_poll_enabled)
            .field(
                "resolve_poll_interval_secs",
                &self.resolve_poll_interval_secs,
            )
            .field("resolve_poll_grace_secs", &self.resolve_poll_grace_secs)
            .field(
                "resolve_poll_max_wait_secs",
                &self.resolve_poll_max_wait_secs,
            )
            .field("transport_backend", &self.transport_backend)
            .finish()
    }
}

impl PolymarketDataClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the validated proxy URL, if configured.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL is malformed, has no host, or does not use HTTP or HTTPS.
    pub fn validated_proxy_url(&self) -> Result<Option<ProxyUrl>, TransportError> {
        validated_proxy_url(self.proxy_url.as_ref())
    }

    #[must_use]
    pub const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
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
    pub fn rtds_url(&self) -> String {
        self.base_url_rtds
            .clone()
            .unwrap_or_else(|| urls::rtds_ws_url().to_string())
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
    pyo3::pyclass(module = "nautilus_trader.adapters.polymarket", from_py_object)
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
    /// Optional HTTP or HTTPS proxy URL for all HTTP and WebSocket transports.
    pub proxy_url: Option<String>,
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    #[builder(default = 3)]
    pub max_retries: u32,
    #[builder(default = 1000)]
    pub retry_delay_initial_ms: u64,
    #[builder(default = 10000)]
    pub retry_delay_max_ms: u64,
    /// Enables authenticated order-safety heartbeats.
    #[builder(default)]
    pub heartbeat_enabled: bool,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Same instrument provider configuration used by the data client.
    ///
    /// Reconciliation classifies unmapped records from `load_ids` on this
    /// config. When that set is non-empty, venue records for other instruments
    /// are out of scope. When this field is unset, or `load_ids` is unset or
    /// empty, every record is in scope.
    pub instrument_config: Option<PolymarketInstrumentProviderConfig>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(PolymarketExecClientConfig {
    trader_id: TraderId,
    account_id: AccountId,
    funder: Option<String>,
    signature_type: SignatureType,
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    base_url_data_api: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    heartbeat_enabled: bool,
    transport_backend: TransportBackend,
    instrument_config: Option<PolymarketInstrumentProviderConfig>,
});

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
            .field("proxy_url", &self.proxy_url.as_ref().map(|_| REDACTED))
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("heartbeat_enabled", &self.heartbeat_enabled)
            .field("instrument_config", &self.instrument_config)
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

    /// Returns the validated proxy URL, if configured.
    ///
    /// # Errors
    ///
    /// Returns an error when the URL is malformed, has no host, or does not use HTTP or HTTPS.
    pub fn validated_proxy_url(&self) -> Result<Option<ProxyUrl>, TransportError> {
        validated_proxy_url(self.proxy_url.as_ref())
    }

    #[must_use]
    pub const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
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

    /// Returns provider `load_ids` used to classify unmapped reconciliation records.
    #[must_use]
    pub fn reconciliation_load_ids(&self) -> Option<&[InstrumentId]> {
        self.instrument_config
            .as_ref()
            .and_then(|config| config.load_ids.as_deref())
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
    fn provider_config_series_ids_trigger_load_all() {
        let config = PolymarketInstrumentProviderConfig {
            series_ids: Some(vec![10684]),
            ..PolymarketInstrumentProviderConfig::default()
        };

        assert!(config.has_series_ids());
        assert!(config.should_load_all());
    }

    #[rstest]
    fn provider_config_empty_series_ids_do_not_trigger_load_all() {
        let config = PolymarketInstrumentProviderConfig {
            series_ids: Some(Vec::new()),
            ..PolymarketInstrumentProviderConfig::default()
        };

        assert!(!config.has_series_ids());
        assert!(!config.should_load_all());
    }

    #[rstest]
    fn provider_config_filters_trigger_load_all() {
        let config = PolymarketInstrumentProviderConfig {
            filters: Some(HashMap::from([("tag_id".to_string(), "84".to_string())])),
            ..PolymarketInstrumentProviderConfig::default()
        };

        assert!(config.has_nonempty_filters());
        assert!(!config.has_explicit_scope());
        assert!(config.should_load_all());
    }

    #[rstest]
    fn provider_config_empty_filters_do_not_trigger_load_all() {
        let config = PolymarketInstrumentProviderConfig {
            filters: Some(HashMap::new()),
            ..PolymarketInstrumentProviderConfig::default()
        };

        assert!(!config.has_nonempty_filters());
        assert!(!config.should_load_all());
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

        assert!(config.instrument_config.is_none());
        assert!(config.filters.is_empty());
        assert_eq!(config.http_timeout_secs, 30);
        assert_eq!(config.ws_max_subscriptions, 50);
        assert_eq!(config.update_instruments_interval_mins, Some(5));
        assert!(config.subscribe_new_markets);
        assert!(config.new_market_filter.is_none());
        assert_eq!(config.new_market_fetch_max_concurrency, 16);
        assert_eq!(config.auto_load_debounce_ms, 250);
        assert!(config.resolve_poll_enabled);
        assert_eq!(config.resolve_poll_interval_secs, 30);
        assert_eq!(config.resolve_poll_grace_secs, 10);
        assert_eq!(config.resolve_poll_max_wait_secs, 1800);
        assert!(config.drop_quotes_missing_side);
        assert!(!config.compute_effective_deltas);
    }

    #[rstest]
    fn test_data_config_toml_sets_compute_effective_deltas() {
        let config: PolymarketDataClientConfig =
            toml::from_str("compute_effective_deltas = true").unwrap();

        assert!(config.compute_effective_deltas);
    }

    #[rstest]
    fn test_data_config_toml_sets_drop_quotes_missing_side_false() {
        let config: PolymarketDataClientConfig =
            toml::from_str("drop_quotes_missing_side = false").unwrap();

        assert!(!config.drop_quotes_missing_side);
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
        assert!(!config.heartbeat_enabled);
        assert_eq!(config.transport_backend, expected.transport_backend);
        assert!(config.instrument_config.is_none());
        assert!(config.reconciliation_load_ids().is_none());
    }

    #[rstest]
    fn test_exec_config_reconciliation_load_ids_come_from_instrument_config() {
        let scoped = InstrumentId::from("0xabc-123.POLYMARKET");
        let config: PolymarketExecClientConfig = toml::from_str(
            r#"
[instrument_config]
load_ids = ["0xabc-123.POLYMARKET"]
"#,
        )
        .unwrap();

        assert_eq!(config.reconciliation_load_ids(), Some([scoped].as_slice()));
    }

    #[rstest]
    fn test_data_config_proxy_url_validates_and_redacts_debug() {
        const SECRET: &str = "data-proxy-secret";
        let proxy_url = format!("http://data-user:{SECRET}@127.0.0.1:18081");
        let config: PolymarketDataClientConfig =
            toml::from_str(&format!("proxy_url = \"{proxy_url}\""))
                .expect("deserialize data config");
        let validated = config
            .validated_proxy_url()
            .expect("validate data proxy")
            .expect("data proxy configured");
        let debug = format!("{config:?}");

        assert_eq!(validated.expose(), proxy_url);
        assert!(config.has_proxy_url());
        assert!(debug.contains("proxy_url: Some(\"<redacted>\")"));
        assert!(!debug.contains(SECRET));
    }

    #[rstest]
    fn test_exec_config_proxy_url_validates_and_redacts_debug() {
        const SECRET: &str = "exec-proxy-secret";
        let proxy_url = format!("https://exec-user:{SECRET}@127.0.0.1:18082");
        let config: PolymarketExecClientConfig =
            toml::from_str(&format!("proxy_url = \"{proxy_url}\""))
                .expect("deserialize execution config");
        let validated = config
            .validated_proxy_url()
            .expect("validate execution proxy")
            .expect("execution proxy configured");
        let debug = format!("{config:?}");

        assert_eq!(validated.expose(), proxy_url);
        assert!(config.has_proxy_url());
        assert!(debug.contains("proxy_url: Some(\"<redacted>\")"));
        assert!(!debug.contains(SECRET));
    }

    #[rstest]
    fn test_proxy_url_unset_preserves_direct_configuration() {
        let data_config = PolymarketDataClientConfig::default();
        let exec_config = PolymarketExecClientConfig::default();

        assert_eq!(data_config.proxy_url, None);
        assert_eq!(exec_config.proxy_url, None);
        assert_eq!(data_config.validated_proxy_url().unwrap(), None);
        assert_eq!(exec_config.validated_proxy_url().unwrap(), None);
        assert!(!data_config.has_proxy_url());
        assert!(!exec_config.has_proxy_url());
    }

    #[rstest]
    fn test_invalid_proxy_url_error_redacts_credentials() {
        const SECRET: &str = "invalid-proxy-secret";
        let config = PolymarketDataClientConfig {
            proxy_url: Some(format!("http://proxy-user:{SECRET}@[::1")),
            ..PolymarketDataClientConfig::default()
        };
        let error = config
            .validated_proxy_url()
            .expect_err("malformed proxy URL should fail");

        assert!(!error.to_string().contains(SECRET));
    }

    #[rstest]
    fn test_socks_proxy_url_is_rejected_for_consistent_routing() {
        let config = PolymarketExecClientConfig {
            proxy_url: Some("socks5://127.0.0.1:1080".to_string()),
            ..PolymarketExecClientConfig::default()
        };
        let error = config
            .validated_proxy_url()
            .expect_err("SOCKS proxy should fail validation");

        assert_eq!(
            error.to_string(),
            "invalid URL: SOCKS proxy scheme 'socks5' is not yet supported for WebSocket connections; use an http:// or https:// proxy"
        );
    }
}
