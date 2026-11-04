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

use nautilus_common::config::{ConfigResult, check_unique_values};
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
    /// Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN.
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
    /// HTTP timeout in seconds.
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
    /// Optional VIP level that unlocks additional subscriptions.
    pub vip_level: Option<OKXVipLevel>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(OKXDataClientConfig {
    instrument_types: Vec<OKXInstrumentType>,
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

    /// Validates that configured selections contain no duplicates.
    ///
    /// # Errors
    ///
    /// Returns an error if a selection repeats a value.
    pub fn validate(&self) -> ConfigResult<()> {
        check_unique_values("instrument_types", &self.instrument_types)?;

        if let Some(values) = &self.contract_types {
            check_unique_values("contract_types", values)?;
        }

        if let Some(values) = &self.instrument_families {
            check_unique_values("instrument_families", values)?;
        }

        Ok(())
    }

    /// Returns `true` when all API credential fields are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        let has_passphrase = self.api_passphrase.is_some() || std::env::var(passphrase_var).is_ok();
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
    /// Contract type filter applied to operations.
    pub contract_types: Option<Vec<OKXContractType>>,
    /// Instrument families to load (e.g., "BTC-USD", "ETH-USD").
    /// Required for OPTIONS. Optional for FUTURES/SWAP. Not applicable for SPOT/MARGIN.
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
    /// HTTP timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Whether to subscribe to spread order updates from the separate spread channel.
    #[builder(default)]
    pub load_spreads: bool,
    /// Enables mass-cancel support when true.
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
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Maximum pages per startup reconciliation history query, capped at 50.
    ///
    /// `None` keeps the default cap; `Some(0)` skips history. Pending orders,
    /// live algo orders, and positions are unaffected. Skipped or truncated
    /// history marks reports incomplete and reduces recovery coverage.
    pub max_reconciliation_history_pages: Option<u32>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(OKXExecutionClientConfig {
    account_id: AccountId,
    oms_type: Option<OmsType>,
    instrument_types: Vec<OKXInstrumentType>,
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

    /// Validates that configured selections contain no duplicates.
    ///
    /// # Errors
    ///
    /// Returns an error if a selection repeats a value.
    pub fn validate(&self) -> ConfigResult<()> {
        check_unique_values("instrument_types", &self.instrument_types)?;

        if let Some(values) = &self.contract_types {
            check_unique_values("contract_types", values)?;
        }

        if let Some(values) = &self.instrument_families {
            check_unique_values("instrument_families", values)?;
        }

        Ok(())
    }

    /// Returns `true` when all API credential fields are available (in config or env vars).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var, passphrase_var) = credential_env_vars();
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        let has_passphrase = self.api_passphrase.is_some() || std::env::var(passphrase_var).is_ok();
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
