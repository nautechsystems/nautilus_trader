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

use nautilus_common::config::{ConfigError, ConfigResult, check_range, check_unique_values};
use nautilus_core::{datetime::SECONDS_IN_MINUTE, string::secret::SecretString};
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_network::{
    transport::TransportError,
    websocket::{TransportBackend, proxy::ProxyUrl},
};
use serde::{Deserialize, Serialize};

use crate::{
    common::{
        credential::{Credential, credential_env_vars},
        enums::{PolymarketSignatureType, PolymarketSignerType},
        urls,
    },
    filters::InstrumentFilter,
    providers::build_gamma_params_from_hashmap,
};

const DEFAULT_UPDOWN_INTERVAL_MINS: u64 = 5;
const DEFAULT_UPDOWN_PERIODS: u64 = 3;

fn validated_proxy_url(value: Option<&SecretString>) -> Result<Option<ProxyUrl>, TransportError> {
    value
        .map(|value| ProxyUrl::parse(value.expose_secret().to_owned()))
        .transpose()
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
    /// Validates assets, interval, and period count.
    ///
    /// Exact duplicates include blank entries. Nonblank assets must also be distinct after
    /// trimming and lowercasing. At least one nonblank asset is required.
    ///
    /// # Errors
    ///
    /// Returns an error if assets are empty or repeated, the interval or period count is zero,
    /// or the interval overflows seconds.
    pub fn validate(&self) -> ConfigResult<()> {
        self.validated_assets().map(|_| ())
    }

    /// Builds event slugs using the current system time.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation fails, the system clock precedes the Unix
    /// epoch, or an offset resolves before the Unix epoch.
    pub fn build_event_slugs(&self) -> anyhow::Result<Vec<String>> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| anyhow::anyhow!("system clock before Unix epoch: {e}"))?
            .as_secs();
        self.build_event_slugs_at_unix_secs(now)
    }

    fn build_event_slugs_at_unix_secs(&self, unix_secs: u64) -> anyhow::Result<Vec<String>> {
        let assets = self.validated_assets()?;
        let period_secs = self.interval_mins * SECONDS_IN_MINUTE;
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

    fn validated_assets(&self) -> ConfigResult<Vec<String>> {
        check_range(
            "event_slug_builder.interval_mins",
            self.interval_mins > 0 && self.interval_mins <= u64::MAX / SECONDS_IN_MINUTE,
            "must be positive and fit in seconds",
        )?;
        check_range(
            "event_slug_builder.periods",
            self.periods > 0,
            "must be positive",
        )?;

        check_unique_values("event_slug_builder.assets", &self.assets)?;
        let mut assets = Vec::new();

        for asset in &self.assets {
            let asset = asset.trim().to_ascii_lowercase();
            if asset.is_empty() {
                continue;
            }
            assets.push(asset);
        }

        check_unique_values("event_slug_builder.assets", &assets)?;
        if assets.is_empty() {
            return Err(ConfigError::empty_field("event_slug_builder.assets"));
        }

        Ok(assets)
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

    /// Validates that configured selections contain no duplicates.
    ///
    /// # Errors
    ///
    /// Returns an error if selections contain duplicates or Gamma filters are invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(values) = &self.load_ids {
            check_unique_values("load_ids", values)?;
        }

        if let Some(values) = &self.event_slugs {
            check_unique_values("event_slugs", values)?;
        }

        if let Some(values) = &self.market_slugs {
            check_unique_values("market_slugs", values)?;
        }

        if let Some(values) = &self.series_ids {
            check_unique_values("series_ids", values)?;
        }

        if let Some(builder) = &self.event_slug_builder {
            builder.validate()?;
        }

        if let Some(filters) = &self.filters {
            let params = build_gamma_params_from_hashmap(filters)?;
            check_unique_values("filters.id", params.id.as_deref().unwrap_or_default())?;
            check_unique_values(
                "filters.tag_id",
                params.tag_id.as_deref().unwrap_or_default(),
            )?;

            for (field, values) in [
                ("filters.slug", params.slug.as_deref()),
                ("filters.condition_ids", params.condition_ids.as_deref()),
                ("filters.clob_token_ids", params.clob_token_ids.as_deref()),
                ("filters.question_ids", params.question_ids.as_deref()),
                (
                    "filters.market_maker_address",
                    params.market_maker_address.as_deref(),
                ),
                (
                    "filters.sports_market_types",
                    params.sports_market_types.as_deref(),
                ),
            ] {
                check_unique_values(field, values.unwrap_or_default())?;
            }
        }

        Ok(())
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
pub struct PolymarketDataClientConfig {
    pub instrument_config: Option<PolymarketInstrumentProviderConfig>,
    /// Instrument filters applied to all instruments during loading and discovery.
    #[builder(default)]
    #[serde(skip)]
    pub filters: Vec<Arc<dyn InstrumentFilter>>,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_reference_prices: Option<String>,
    pub base_url_gamma: Option<String>,
    /// CLOB API key for PolyBolt reference-price channels; falls back to `POLYMARKET_API_KEY`.
    pub api_key: Option<SecretString>,
    /// CLOB API secret for PolyBolt reference-price channels; falls back to
    /// `POLYMARKET_API_SECRET`.
    pub api_secret: Option<SecretString>,
    /// CLOB API passphrase for PolyBolt reference-price channels; falls back to
    /// `POLYMARKET_PASSPHRASE`.
    pub passphrase: Option<SecretString>,
    pub base_url_data_api: Option<String>,
    /// Optional HTTP or HTTPS proxy URL for all HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Timeout in seconds per market and reference-price WebSocket connection attempt,
    /// including reconnects, and for sends waiting for reconnection. Must be positive;
    /// keep it above the 5-second maximum reconnect backoff.
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
    /// Whether to emit only the net changes from book snapshots against the local
    /// book, at an additional CPU and memory cost.
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
    /// Maximum wait for an initial, post-reconnect, or recovery book snapshot, in seconds.
    ///
    /// Set to 0 to disable initial and reconnect monitors, and per-attempt snapshot deadlines
    /// within the retry budget. Updates without a baseline can still request recovery.
    /// During recovery, book output stays gated until a valid snapshot arrives.
    /// Retry-budget and retry-ceiling limits still apply.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// Interval for checking order book feed staleness in seconds.
    #[builder(default = 5)]
    pub book_stale_check_interval_secs: u64,
    /// Maximum time without order book updates before emitting a stale signal in seconds.
    ///
    /// Set to 0 to disable. Prediction markets go quiet for long stretches.
    #[builder(default)]
    pub book_stale_threshold_secs: u64,
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
    base_url_reference_prices: Option<String>,
    transport_backend: TransportBackend,
    drop_quotes_missing_side: bool,
    compute_effective_deltas: bool,
    book_snapshot_timeout_secs: u64,
    book_stale_check_interval_secs: u64,
    book_stale_threshold_secs: u64,
});

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

    /// Validates the WebSocket timeout and instrument selections.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket timeout is zero, nested instrument selections contain
    /// duplicates, or Gamma filters are invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        check_range(
            "ws_timeout_secs",
            self.ws_timeout_secs > 0,
            "must be positive",
        )?;

        if let Some(config) = &self.instrument_config {
            config.validate()?;
        }

        Ok(())
    }

    /// Returns the validated proxy URL, if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is malformed, has no host, or does not use HTTP or HTTPS.
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
    pub fn reference_prices_url(&self) -> String {
        self.base_url_reference_prices
            .clone()
            .unwrap_or_else(|| urls::polybolt_ws_url().to_string())
    }

    /// Resolves the CLOB API credential used for PolyBolt reference-price channels.
    ///
    /// Explicit `api_key`, `api_secret`, or `passphrase` fields are validated strictly.
    /// Otherwise the `POLYMARKET_*` environment variables are used only when all three are
    /// set and non-empty; a partial set is ignored with a warning so a data client that never
    /// subscribes to reference prices still starts. Returns `None` when no credential applies.
    ///
    /// # Errors
    ///
    /// Returns an error if an explicitly configured credential is incomplete or malformed,
    /// or if a complete environment credential is malformed.
    pub fn polybolt_credential(&self) -> anyhow::Result<Option<Credential>> {
        let explicit =
            self.api_key.is_some() || self.api_secret.is_some() || self.passphrase.is_some();

        if !explicit {
            let (api_key_var, api_secret_var, passphrase_var, _, _) = credential_env_vars();
            let present: Vec<&str> = [api_key_var, api_secret_var, passphrase_var]
                .into_iter()
                .filter(|var| std::env::var(var).is_ok_and(|value| !value.trim().is_empty()))
                .collect();

            if present.is_empty() {
                return Ok(None);
            }

            if present.len() < 3 {
                log::warn!(
                    "Ignoring partial Polymarket CLOB credentials in the environment \
                     (set: {present:?}); PolyBolt price subscriptions will be rejected"
                );
                return Ok(None);
            }
        }

        Credential::resolve(
            self.api_key.clone(),
            self.api_secret.clone(),
            self.passphrase.clone(),
        )
        .map(Some)
        .map_err(|e| anyhow::anyhow!("Invalid Polymarket CLOB credentials for PolyBolt: {e}"))
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
            .unwrap_or_else(|| urls::data_api_url().to_string())
    }
}

/// Configuration for the Polymarket execution client.
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
pub struct PolymarketExecutionClientConfig {
    #[builder(default = AccountId::from("POLYMARKET-001"))]
    pub account_id: AccountId,
    /// Falls back to `POLYMARKET_PK` env var.
    pub private_key: Option<SecretString>,
    /// Falls back to `POLYMARKET_API_KEY` env var.
    pub api_key: Option<SecretString>,
    /// Falls back to `POLYMARKET_API_SECRET` env var.
    pub api_secret: Option<SecretString>,
    /// Falls back to `POLYMARKET_PASSPHRASE` env var.
    pub passphrase: Option<SecretString>,
    /// Falls back to `POLYMARKET_FUNDER` env var.
    pub funder: Option<String>,
    #[builder(default = PolymarketSignatureType::Eoa)]
    pub signature_type: PolymarketSignatureType,
    /// Selects owner or delegated session signing.
    #[builder(default)]
    pub signer_type: PolymarketSignerType,
    pub base_url_http: Option<String>,
    pub base_url_ws: Option<String>,
    pub base_url_data_api: Option<String>,
    /// Optional HTTP or HTTPS proxy URL for all HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
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
nautilus_core::impl_pyo3_config_getters!(PolymarketExecutionClientConfig {
    account_id: AccountId,
    funder: Option<String>,
    signature_type: PolymarketSignatureType,
    signer_type: PolymarketSignerType,
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

impl Default for PolymarketExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl PolymarketExecutionClientConfig {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates the configured instrument selections.
    ///
    /// # Errors
    ///
    /// Returns an error if nested instrument selections contain duplicates or Gamma filters
    /// are invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        if let Some(config) = &self.instrument_config {
            config.validate()?;
        }

        Ok(())
    }

    pub(crate) fn validate_signer(&self) -> anyhow::Result<()> {
        if self.signer_type == PolymarketSignerType::Session {
            anyhow::ensure!(
                self.signature_type == PolymarketSignatureType::Poly1271,
                "Session signers require POLY_1271"
            );

            for (name, value) in [
                ("private_key", self.private_key.as_ref()),
                ("api_key", self.api_key.as_ref()),
                ("api_secret", self.api_secret.as_ref()),
                ("passphrase", self.passphrase.as_ref()),
            ] {
                anyhow::ensure!(
                    value.is_some_and(|value| !value.expose_secret().trim().is_empty()),
                    "Session signers require explicit {name}; environment fallback is disabled"
                );
            }

            anyhow::ensure!(
                self.funder
                    .as_ref()
                    .is_some_and(|value| !value.trim().is_empty()),
                "Session signers require an explicit Deposit Wallet funder"
            );
        }

        Ok(())
    }

    /// Returns the validated proxy URL, if configured.
    ///
    /// # Errors
    ///
    /// Returns an error if the URL is malformed, has no host, or does not use HTTP or HTTPS.
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
            .as_ref()
            .map(SecretString::expose_secret)
            .is_some_and(|s| !s.trim().is_empty())
            || self
                .api_key
                .as_ref()
                .map(SecretString::expose_secret)
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
            .unwrap_or_else(|| urls::data_api_url().to_string())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::config::ConfigError;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn updown_event_slug_config_builds_aligned_slugs() {
        let config = PolymarketUpDownEventSlugConfig {
            assets: vec!["BTC".to_string(), " eth ".to_string(), String::new()],
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

        assert_eq!(
            err.downcast_ref::<ConfigError>(),
            Some(&ConfigError::range(
                "event_slug_builder.interval_mins",
                "must be positive and fit in seconds"
            )),
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
    fn data_config_book_sync_defaults_match_documented_values() {
        let config = PolymarketDataClientConfig::default();

        assert_eq!(config.book_snapshot_timeout_secs, 10);
        assert_eq!(config.book_stale_check_interval_secs, 5);
        assert_eq!(
            config.book_stale_threshold_secs, 0,
            "stale monitor must stay disabled by default"
        );
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
        let config: PolymarketExecutionClientConfig = toml::from_str("").unwrap();
        let expected = PolymarketExecutionClientConfig::default();
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
        let config: PolymarketExecutionClientConfig = toml::from_str(
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
        assert!(debug.contains("proxy_url: Some(<redacted>)"));
        assert!(!debug.contains(SECRET));
    }

    #[rstest]
    fn test_exec_config_proxy_url_validates_and_redacts_debug() {
        const SECRET: &str = "exec-proxy-secret";
        let proxy_url = format!("https://exec-user:{SECRET}@127.0.0.1:18082");
        let config: PolymarketExecutionClientConfig =
            toml::from_str(&format!("proxy_url = \"{proxy_url}\""))
                .expect("deserialize execution config");
        let validated = config
            .validated_proxy_url()
            .expect("validate execution proxy")
            .expect("execution proxy configured");
        let debug = format!("{config:?}");

        assert_eq!(validated.expose(), proxy_url);
        assert!(config.has_proxy_url());
        assert!(debug.contains("proxy_url: Some(<redacted>)"));
        assert!(!debug.contains(SECRET));
    }

    #[rstest]
    fn test_proxy_url_unset_preserves_direct_configuration() {
        let data_config = PolymarketDataClientConfig::default();
        let exec_config = PolymarketExecutionClientConfig::default();

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
            proxy_url: Some(format!("http://proxy-user:{SECRET}@[::1").into()),
            ..PolymarketDataClientConfig::default()
        };
        let error = config
            .validated_proxy_url()
            .expect_err("malformed proxy URL should fail");

        assert!(!error.to_string().contains(SECRET));
    }

    #[rstest]
    fn test_socks_proxy_url_is_rejected_for_consistent_routing() {
        let config = PolymarketExecutionClientConfig {
            proxy_url: Some("socks5://127.0.0.1:1080".into()),
            ..PolymarketExecutionClientConfig::default()
        };
        let error = config
            .validated_proxy_url()
            .expect_err("SOCKS proxy should fail validation");

        assert_eq!(
            error.to_string(),
            "invalid URL: SOCKS proxy scheme 'socks5' is not yet supported for WebSocket connections; use an http:// or https:// proxy"
        );
    }

    #[rstest]
    #[case("private_key")]
    #[case("api_key")]
    #[case("api_secret")]
    #[case("passphrase")]
    #[case("funder")]
    fn test_session_requires_explicit_credentials(#[case] missing: &str) {
        let mut config = PolymarketExecutionClientConfig {
            signer_type: PolymarketSignerType::Session,
            signature_type: PolymarketSignatureType::Poly1271,
            private_key: Some("key".into()),
            api_key: Some("api".into()),
            api_secret: Some("secret".into()),
            passphrase: Some("pass".into()),
            funder: Some("wallet".into()),
            ..Default::default()
        };

        assert!(config.validate_signer().is_ok());

        match missing {
            "private_key" => config.private_key = None,
            "api_key" => config.api_key = None,
            "api_secret" => config.api_secret = None,
            "passphrase" => config.passphrase = Some(" ".into()),
            "funder" => config.funder = Some(" ".into()),
            _ => unreachable!(),
        }

        assert!(config.validate_signer().is_err());
    }

    #[rstest]
    #[case(PolymarketSignatureType::Eoa)]
    #[case(PolymarketSignatureType::PolyProxy)]
    #[case(PolymarketSignatureType::PolyGnosisSafe)]
    fn test_session_rejects_other_signature_types(#[case] signature_type: PolymarketSignatureType) {
        let config = PolymarketExecutionClientConfig {
            signer_type: PolymarketSignerType::Session,
            signature_type,
            ..Default::default()
        };

        assert_eq!(
            config.validate_signer().unwrap_err().to_string(),
            "Session signers require POLY_1271"
        );
    }

    #[rstest]
    fn polybolt_credential_resolves_explicit_fields() {
        let config = PolymarketDataClientConfig {
            api_key: Some(SecretString::from("fixture-key")),
            api_secret: Some(SecretString::from("Zml4dHVyZS1zZWNyZXQ=")),
            passphrase: Some(SecretString::from("fixture-passphrase")),
            ..PolymarketDataClientConfig::default()
        };

        let credential = config
            .polybolt_credential()
            .expect("explicit credentials resolve")
            .expect("credential present");

        assert_eq!(credential.api_key_str(), "fixture-key");
    }

    #[rstest]
    fn polybolt_credential_rejects_malformed_explicit_secret() {
        let config = PolymarketDataClientConfig {
            api_key: Some(SecretString::from("fixture-key")),
            api_secret: Some(SecretString::from("not base64!")),
            passphrase: Some(SecretString::from("fixture-passphrase")),
            ..PolymarketDataClientConfig::default()
        };

        let error = config.polybolt_credential().unwrap_err().to_string();

        assert!(
            error.starts_with("Invalid Polymarket CLOB credentials for PolyBolt"),
            "{error}"
        );
    }

    #[rstest]
    #[case("load_ids", serde_json::json!(["token-one.POLYMARKET", "token-two.POLYMARKET"]))]
    #[case("event_slugs", serde_json::json!(["event-one", "event-two"]))]
    #[case("market_slugs", serde_json::json!(["market-one", "market-two"]))]
    #[case("series_ids", serde_json::json!([1, 2]))]
    fn test_instrument_provider_selection_uniqueness(
        #[case] field: &str,
        #[case] mut values: serde_json::Value,
        #[values(false, true)] duplicate: bool,
    ) {
        if duplicate {
            let entries = values.as_array_mut().unwrap();
            entries.push(entries[0].clone());
        }

        let mut input =
            serde_json::to_value(PolymarketInstrumentProviderConfig::default()).unwrap();
        input[field] = values;
        let config: PolymarketInstrumentProviderConfig = serde_json::from_value(input).unwrap();
        let original = serde_json::to_value(&config).unwrap();

        let expected = if duplicate {
            Err(ConfigError::duplicate(field, None))
        } else {
            Ok(())
        };

        let result = config
            .validate()
            .map_err(|e| e.downcast::<ConfigError>().unwrap());

        assert_eq!(result, expected);
        assert_eq!(serde_json::to_value(&config).unwrap(), original);
    }

    #[rstest]
    #[case::exact(vec!["btc".to_string(), "btc".to_string()])]
    #[case::normalized(vec!["BTC".to_string(), " btc ".to_string()])]
    fn test_slug_assets_reject_duplicates(#[case] assets: Vec<String>) {
        let config = PolymarketUpDownEventSlugConfig {
            assets,
            ..Default::default()
        };

        let error = config
            .build_event_slugs_at_unix_secs(1_700_000_123)
            .unwrap_err();

        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate("event_slug_builder.assets", None))
        );
    }

    #[rstest]
    #[case("slug", "market-a, market-b,market-a")]
    #[case("slug", " market-a,market-a ")]
    #[case("condition_ids", "condition-a,condition-b,condition-a")]
    #[case("clob_token_ids", "token-a,token-b,token-a")]
    #[case("question_ids", "question-a,question-b,question-a")]
    #[case("market_maker_address", "address-a,address-b,address-a")]
    #[case("sports_market_types", "type-a,type-b,type-a")]
    #[case("tag_id", "1,01")]
    #[case("id", "1,2,1")]
    #[case("id", "1,01")]
    fn test_gamma_filter_selection_duplicates(#[case] key: &str, #[case] values: &str) {
        let config = PolymarketInstrumentProviderConfig {
            filters: Some(std::collections::HashMap::from([(
                key.to_string(),
                values.to_string(),
            )])),
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate(format!("filters.{key}"), None))
        );
    }

    #[rstest]
    fn test_execution_config_rejects_duplicate_provider_selections() {
        let config = PolymarketExecutionClientConfig {
            instrument_config: Some(PolymarketInstrumentProviderConfig {
                load_ids: Some(vec![
                    InstrumentId::from("one.POLYMARKET"),
                    InstrumentId::from("one.POLYMARKET"),
                ]),
                ..Default::default()
            }),
            ..Default::default()
        };

        let error = config.validate().unwrap_err();
        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate("load_ids", None))
        );
    }

    #[rstest]
    #[case::interval_zero(0, 1, vec!["btc".to_string()], Err(ConfigError::range("event_slug_builder.interval_mins", "must be positive and fit in seconds")))]
    #[case::interval_overflow(u64::MAX, 1, vec!["btc".to_string()], Err(ConfigError::range("event_slug_builder.interval_mins", "must be positive and fit in seconds")))]
    #[case::periods_zero(5, 0, vec!["btc".to_string()], Err(ConfigError::range("event_slug_builder.periods", "must be positive")))]
    #[case::assets_empty(5, 1, vec![], Err(ConfigError::empty_field("event_slug_builder.assets")))]
    #[case::assets_blank(5, 1, vec![" ".to_string()], Err(ConfigError::empty_field("event_slug_builder.assets")))]
    #[case::interval_max(u64::MAX / SECONDS_IN_MINUTE, 1, vec!["btc".to_string()], Ok(()))]
    fn test_slug_builder_config_static_validation(
        #[case] interval_mins: u64,
        #[case] periods: u64,
        #[case] assets: Vec<String>,
        #[case] expected: ConfigResult<()>,
    ) {
        let config = PolymarketUpDownEventSlugConfig {
            assets,
            interval_mins,
            periods,
            ..Default::default()
        };

        assert_eq!(config.validate(), expected);
    }
}
