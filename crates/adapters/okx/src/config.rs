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

//! Configuration structures for the OKX adapter.

use nautilus_common::config::{ConfigError, ConfigResult, check_range, check_unique_values};
use nautilus_core::string::secret::SecretString;
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::{enums::OmsType, identifiers::AccountId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    credential::credential_env_vars,
    enums::{
        OKXContractType, OKXEnvironment, OKXInstrumentType, OKXMarginMode, OKXRegion, OKXVipLevel,
    },
    urls::{
        get_http_base_url, get_ws_base_url_business, get_ws_base_url_private,
        get_ws_base_url_public,
    },
};

/// Configuration for the OKX data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.okx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.okx")
)]
pub struct OKXDataClientConfig {
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<SecretString>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<SecretString>,
    /// Optional API passphrase for authenticated endpoints.
    pub api_passphrase: Option<SecretString>,
    /// Instrument types to load and subscribe to.
    #[builder(default = vec![OKXInstrumentType::Spot])]
    pub instrument_types: Vec<OKXInstrumentType>,
    /// Contract type filter applied to loaded instruments.
    pub contract_types: Option<Vec<OKXContractType>>,
    /// Whether to load spread trading instruments from the separate spread endpoint.
    #[builder(default)]
    pub load_spreads: bool,
    /// Instrument families to load (e.g., "BTC-USD", "ETH-USD").
    /// Required for OPTION; optional for FUTURES/SWAP. For EVENTS, values are series IDs.
    /// Filtered EVENTS and derivatives require separate clients. Ignored for SPOT/MARGIN.
    pub instrument_families: Option<Vec<String>>,
    /// The API environment (live or demo).
    #[builder(default)]
    pub environment: OKXEnvironment,
    /// The API region (global, EEA, or US).
    #[builder(default)]
    pub region: OKXRegion,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Optional override for the business WebSocket URL.
    pub base_url_ws_business: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// Timeout per HTTP attempt in seconds.
    /// The retry budget is the greater of this timeout and 180 seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Interval for reconciling instruments from the REST API in minutes.
    ///
    /// Set to 0 to disable periodic reconciliation. WebSocket instrument
    /// updates are always applied regardless of this interval.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// Interval for checking order book feed staleness in seconds.
    #[builder(default = 5)]
    pub book_stale_check_interval_secs: u64,
    /// Maximum time without order book updates before emitting a stale signal in seconds.
    ///
    /// Set to 0 to disable. Quiet markets can idle without book changes.
    #[builder(default = 30)]
    pub book_stale_threshold_secs: u64,
    /// Maximum time to wait for an initial, post-reconnect, or recovery order book
    /// snapshot in seconds.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// Optional VIP level that selects book channels.
    /// VIP4 and above require complete API credentials and venue permission.
    pub vip_level: Option<OKXVipLevel>,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(OKXDataClientConfig {
    instrument_types: Vec<OKXInstrumentType>,
    contract_types: Option<Vec<OKXContractType>>,
    instrument_families: Option<Vec<String>>,
    environment: OKXEnvironment,
    region: OKXRegion,
    base_url_http: Option<String>,
    base_url_ws_public: Option<String>,
    base_url_ws_business: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    update_instruments_interval_mins: u64,
    book_stale_check_interval_secs: u64,
    book_stale_threshold_secs: u64,
    book_snapshot_timeout_secs: u64,
    vip_level: Option<OKXVipLevel>,
    load_spreads: bool,
    transport_backend: TransportBackend,
});

impl Default for OKXDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl OKXDataClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates instrument selections, authentication requirements, and retry delays.
    ///
    /// # Errors
    ///
    /// Returns an error if selections are duplicated, blank, or conflicting, required settings
    /// are missing, or retry delays are invalid.
    pub fn validate(&self) -> ConfigResult<()> {
        validate_instrument_selections(
            &self.instrument_types,
            self.contract_types.as_deref(),
            self.instrument_families.as_deref(),
        )?;
        validate_retry_delays(self.retry_delay_initial_ms, self.retry_delay_max_ms)?;

        if self
            .vip_level
            .is_some_and(|level| level >= OKXVipLevel::Vip4)
            && !self.has_api_credentials()
        {
            return Err(ConfigError::dependency(
                "vip_level",
                "api_key, api_secret, api_passphrase",
                "VIP book channels require authentication",
            ));
        }

        Ok(())
    }

    /// Returns whether all credential fields have non-blank config or environment values.
    /// Explicit blank values do not fall back to environment variables.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let has_key = has_credential(self.api_key.as_ref(), key_var);
        let has_secret = has_credential(self.api_secret.as_ref(), secret_var);
        let has_passphrase = has_credential(self.api_passphrase.as_ref(), passphrase_var);
        has_key && has_secret && has_passphrase
    }

    /// Returns the HTTP base URL, falling back to the region default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.region).to_string())
    }

    /// Returns the public WebSocket URL, respecting the region, environment, and overrides.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public
            .clone()
            .unwrap_or_else(|| get_ws_base_url_public(self.region, self.environment).to_string())
    }

    /// Returns the business WebSocket URL, respecting the region, environment, and overrides.
    #[must_use]
    pub fn ws_business_url(&self) -> String {
        self.base_url_ws_business
            .clone()
            .unwrap_or_else(|| get_ws_base_url_business(self.region, self.environment).to_string())
    }

    /// Returns `true` when the business WebSocket should be instantiated.
    ///
    /// The business WebSocket carries public candle data and does not
    /// require authentication, so it is always needed.
    #[must_use]
    pub fn requires_business_ws(&self) -> bool {
        true
    }
}

/// Configuration for the OKX execution client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.okx", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.okx")
)]
pub struct OKXExecutionClientConfig {
    /// The account ID for the client.
    #[builder(default = AccountId::from("OKX-001"))]
    pub account_id: AccountId,
    /// Optional OMS override for the client.
    /// Defaults to netting with any derivative instrument type, otherwise hedging.
    pub oms_type: Option<OmsType>,
    /// Optional API key for authenticated endpoints.
    pub api_key: Option<SecretString>,
    /// Optional API secret for authenticated endpoints.
    pub api_secret: Option<SecretString>,
    /// Optional API passphrase for authenticated endpoints.
    pub api_passphrase: Option<SecretString>,
    /// Instrument types the execution client should support.
    #[builder(default = vec![OKXInstrumentType::Spot])]
    pub instrument_types: Vec<OKXInstrumentType>,
    /// Contract type filter applied to instrument discovery.
    pub contract_types: Option<Vec<OKXContractType>>,
    /// Instrument families to load (e.g., "BTC-USD", "ETH-USD").
    /// Required for OPTION; optional for FUTURES/SWAP. For EVENTS, values are series IDs.
    /// Filtered EVENTS and derivatives require separate clients. Ignored for SPOT/MARGIN.
    pub instrument_families: Option<Vec<String>>,
    /// The API environment (live or demo).
    #[builder(default)]
    pub environment: OKXEnvironment,
    /// The API region (global, EEA, or US).
    #[builder(default)]
    pub region: OKXRegion,
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional override for the business WebSocket URL.
    pub base_url_ws_business: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// Timeout per HTTP attempt in seconds.
    /// The retry budget is the greater of this timeout and 180 seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Whether to subscribe to spread order updates from the separate spread channel.
    #[builder(default)]
    pub load_spreads: bool,
    /// Selects MMP-only family cancellation, which is unsupported for ordinary orders.
    /// Must remain false; true fails validation.
    #[builder(default)]
    pub use_mm_mass_cancel: bool,
    /// Maximum retry attempts for requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry delay in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry delay in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Optional margin mode (CROSS or ISOLATED) for margin/derivative accounts.
    pub margin_mode: Option<OKXMarginMode>,
    /// Enables margin/leverage for SPOT trading when true.
    /// Selecting MARGIN in `instrument_types` also enables spot margin.
    #[builder(default)]
    pub use_spot_margin: bool,
    /// Optional SPOT `tradeQuoteCcy` override sent on order payloads.
    ///
    /// Unset omits the field so OKX uses the quote currency in `instId` (USDC
    /// on `Crypto-USDC` instruments). Set to `"USD"` to keep trading in USD
    /// after the USD-to-USDC spot migration. The value must appear in that
    /// instrument's `tradeQuoteCcyList`.
    pub spot_trade_quote_ccy: Option<String>,
    /// Optional WebSocket authentication timeout (seconds), defaulting to
    /// `AUTHENTICATION_TIMEOUT_SECS` when unset.
    pub auth_timeout_secs: Option<u64>,
    /// WebSocket transport backend (defaults to `Sockudo`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Maximum pages per startup reconciliation history query, capped at 50.
    ///
    /// `None` keeps the default cap; `Some(0)` skips history. Pending orders,
    /// live algo orders, and positions are unaffected. Skipped or truncated
    /// history marks reports incomplete and reduces recovery coverage.
    pub max_reconciliation_history_pages: Option<u32>,
    /// Optional TTL (milliseconds) for WebSocket place and amend requests, sent to OKX as
    /// `expTime`.
    ///
    /// Each request carries `expTime = now + ttl`, computed from the local clock when the
    /// request is built, so time spent waiting for rate limits or a reconnect counts against
    /// it. OKX does not process a request once its server time is past `expTime`. Allow for
    /// clock skew between the local host and OKX. `None` omits `expTime`. Must be positive.
    pub order_request_ttl_ms: Option<u64>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(OKXExecutionClientConfig {
    account_id: AccountId,
    oms_type: Option<OmsType>,
    instrument_types: Vec<OKXInstrumentType>,
    contract_types: Option<Vec<OKXContractType>>,
    instrument_families: Option<Vec<String>>,
    use_spot_margin: bool,
    environment: OKXEnvironment,
    region: OKXRegion,
    base_url_http: Option<String>,
    base_url_ws_private: Option<String>,
    base_url_ws_business: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    margin_mode: Option<OKXMarginMode>,
    load_spreads: bool,
    auth_timeout_secs: Option<u64>,
    transport_backend: TransportBackend,
    spot_trade_quote_ccy: Option<String>,
    max_reconciliation_history_pages: Option<u32>,
    order_request_ttl_ms: Option<u64>,
});

impl Default for OKXExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl OKXExecutionClientConfig {
    /// Creates a new configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates instrument selections, cancellation support, and retry delays.
    ///
    /// # Errors
    ///
    /// Returns an error if selections are duplicated, blank, or conflicting, required settings
    /// are missing, retry delays or the order request TTL are invalid, or MMP cancellation
    /// is enabled.
    pub fn validate(&self) -> ConfigResult<()> {
        validate_instrument_selections(
            &self.instrument_types,
            self.contract_types.as_deref(),
            self.instrument_families.as_deref(),
        )?;
        validate_retry_delays(self.retry_delay_initial_ms, self.retry_delay_max_ms)?;

        if let Some(ttl_ms) = self.order_request_ttl_ms {
            check_range("order_request_ttl_ms", ttl_ms > 0, "must be positive")?;
            check_range(
                "order_request_ttl_ms",
                ttl_ms <= MAX_ORDER_REQUEST_TTL_MS,
                format!("must be at most {MAX_ORDER_REQUEST_TTL_MS}"),
            )?;
        }

        if self.use_mm_mass_cancel {
            return Err(ConfigError::unsupported_value(
                "use_mm_mass_cancel",
                "MMP-only family cancellation does not cancel ordinary orders",
            ));
        }

        Ok(())
    }

    pub(crate) fn has_instrument_filters(&self) -> bool {
        self.instrument_families
            .as_ref()
            .is_some_and(|values| !values.is_empty())
            || self
                .contract_types
                .as_ref()
                .is_some_and(|values| !values.is_empty())
    }

    pub(crate) fn uses_spot_margin(&self) -> bool {
        self.use_spot_margin || self.instrument_types.contains(&OKXInstrumentType::Margin)
    }

    /// Returns whether all credential fields have non-blank config or environment values.
    /// Explicit blank values do not fall back to environment variables.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let has_key = has_credential(self.api_key.as_ref(), key_var);
        let has_secret = has_credential(self.api_secret.as_ref(), secret_var);
        let has_passphrase = has_credential(self.api_passphrase.as_ref(), passphrase_var);
        has_key && has_secret && has_passphrase
    }

    /// Returns the HTTP base URL, falling back to the region default when unset.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| get_http_base_url(self.region).to_string())
    }

    /// Returns the private WebSocket URL, respecting the region, environment, and overrides.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| get_ws_base_url_private(self.region, self.environment).to_string())
    }

    /// Returns the business WebSocket URL, respecting the region, environment, and overrides.
    #[must_use]
    pub fn ws_business_url(&self) -> String {
        self.base_url_ws_business
            .clone()
            .unwrap_or_else(|| get_ws_base_url_business(self.region, self.environment).to_string())
    }
}

fn validate_instrument_selections(
    instrument_types: &[OKXInstrumentType],
    contract_types: Option<&[OKXContractType]>,
    instrument_families: Option<&[String]>,
) -> ConfigResult<()> {
    check_unique_values("instrument_types", instrument_types)?;

    if let Some(values) = contract_types {
        check_unique_values("contract_types", values)?;
    }

    if let Some(values) = instrument_families {
        check_unique_values("instrument_families", values)?;
        if values.iter().any(|value| value.trim().is_empty()) {
            return Err(ConfigError::invalid_value(
                "instrument_families",
                "entries must not be blank",
            ));
        }
    }

    if instrument_types.contains(&OKXInstrumentType::Option)
        && instrument_families.is_none_or(<[String]>::is_empty)
    {
        return Err(ConfigError::dependency(
            "instrument_types",
            "instrument_families",
            "OPTION requires at least one family",
        ));
    }

    if instrument_types.contains(&OKXInstrumentType::Events)
        && instrument_types.iter().any(|value| {
            matches!(
                value,
                OKXInstrumentType::Option | OKXInstrumentType::Swap | OKXInstrumentType::Futures
            )
        })
        && instrument_families.is_some_and(|values| !values.is_empty())
    {
        return Err(ConfigError::invalid_value(
            "instrument_families",
            "EVENTS series and derivative families require separate clients",
        ));
    }

    Ok(())
}

/// Upper bound for `order_request_ttl_ms`.
const MAX_ORDER_REQUEST_TTL_MS: u64 = 9_999_999_999_999;

pub(crate) fn validate_retry_delays(initial_ms: u64, max_ms: u64) -> ConfigResult<()> {
    check_range("retry_delay_initial_ms", initial_ms > 0, "must be positive")?;
    check_range(
        "retry_delay_max_ms",
        max_ms >= initial_ms,
        "must be >= retry_delay_initial_ms",
    )?;
    check_range(
        "retry_delay_max_ms",
        max_ms <= u64::MAX / 1_000_000,
        "exceeds maximum representable backoff duration",
    )
}

fn has_credential(value: Option<&SecretString>, env_var: &str) -> bool {
    value.map_or_else(
        || std::env::var(env_var).is_ok_and(|value| !value.trim().is_empty()),
        |value| !value.expose_secret().trim().is_empty(),
    )
}
