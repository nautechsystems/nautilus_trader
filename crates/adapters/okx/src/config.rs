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

use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::AccountId;
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
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Optional override for the business WebSocket URL.
    pub base_url_ws_business: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// The API environment (live or demo).
    #[builder(default)]
    pub environment: OKXEnvironment,
    /// The API region (global, EEA, or US).
    #[builder(default)]
    pub region: OKXRegion,
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
    /// Maximum time to wait for a post-reconnect order book snapshot in seconds.
    #[builder(default = 3)]
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
    /// Optional override for the HTTP base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional override for the business WebSocket URL.
    pub base_url_ws_business: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// The API environment (live or demo).
    #[builder(default)]
    pub environment: OKXEnvironment,
    /// The API region (global, EEA, or US).
    #[builder(default)]
    pub region: OKXRegion,
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
    /// Optional WebSocket authentication timeout (seconds), defaulting to
    /// `AUTHENTICATION_TIMEOUT_SECS` when unset.
    pub auth_timeout_secs: Option<u64>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(OKXExecutionClientConfig {
    account_id: AccountId,
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

#[cfg(test)]
mod tests {
    use nautilus_core::string::secret::REDACTED;
    use rstest::rstest;

    use super::*;

    const DATA_API_KEY: &str = "okx-data-api-key-sentinel";
    const DATA_API_SECRET: &str = "okx-data-api-secret-sentinel";
    const DATA_API_PASSPHRASE: &str = "okx-data-api-passphrase-sentinel";
    const EXEC_API_KEY: &str = "okx-exec-api-key-sentinel";
    const EXEC_API_SECRET: &str = "okx-exec-api-secret-sentinel";
    const EXEC_API_PASSPHRASE: &str = "okx-exec-api-passphrase-sentinel";

    #[rstest]
    fn test_data_config_debug_redacts_credentials() {
        let config = OKXDataClientConfig {
            api_key: Some(DATA_API_KEY.into()),
            api_secret: Some(DATA_API_SECRET.into()),
            api_passphrase: Some(DATA_API_PASSPHRASE.into()),
            environment: OKXEnvironment::Demo,
            http_timeout_secs: 71,
            ..Default::default()
        };

        let debug_output = format!("{config:?}");
        let redacted = format!("Some({REDACTED})");

        assert!(!debug_output.contains(DATA_API_KEY));
        assert!(!debug_output.contains(DATA_API_SECRET));
        assert!(!debug_output.contains(DATA_API_PASSPHRASE));
        assert!(debug_output.contains(&format!("api_key: {redacted}")));
        assert!(debug_output.contains(&format!("api_secret: {redacted}")));
        assert!(debug_output.contains(&format!("api_passphrase: {redacted}")));
        assert!(debug_output.contains("environment: Demo"));
        assert!(debug_output.contains("http_timeout_secs: 71"));
    }

    #[rstest]
    fn test_exec_config_debug_redacts_credentials() {
        let config = OKXExecutionClientConfig {
            account_id: AccountId::from("OKX-042"),
            api_key: Some(EXEC_API_KEY.into()),
            api_secret: Some(EXEC_API_SECRET.into()),
            api_passphrase: Some(EXEC_API_PASSPHRASE.into()),
            max_retries: 13,
            ..Default::default()
        };

        let debug_output = format!("{config:?}");
        let redacted = format!("Some({REDACTED})");

        assert!(!debug_output.contains(EXEC_API_KEY));
        assert!(!debug_output.contains(EXEC_API_SECRET));
        assert!(!debug_output.contains(EXEC_API_PASSPHRASE));
        assert!(debug_output.contains(&format!("api_key: {redacted}")));
        assert!(debug_output.contains(&format!("api_secret: {redacted}")));
        assert!(debug_output.contains(&format!("api_passphrase: {redacted}")));
        assert!(debug_output.contains("OKX-042"));
        assert!(debug_output.contains("max_retries: 13"));
    }

    #[rstest]
    fn test_config_debug_handles_unset_and_partial_credentials() {
        let data_debug = format!("{:?}", OKXDataClientConfig::default());
        let exec_debug = format!(
            "{:?}",
            OKXExecutionClientConfig {
                api_secret: Some(String::new().into()),
                ..Default::default()
            }
        );

        assert!(data_debug.contains("api_key: None"));
        assert!(data_debug.contains("api_secret: None"));
        assert!(data_debug.contains("api_passphrase: None"));
        assert!(exec_debug.contains("api_key: None"));
        assert!(exec_debug.contains(&format!("api_secret: Some({REDACTED})")));
        assert!(exec_debug.contains("api_passphrase: None"));
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: OKXDataClientConfig = toml::from_str(
            r#"
environment = "demo"
instrument_types = ["SPOT", "SWAP"]
http_timeout_secs = 90
"#,
        )
        .unwrap();

        assert_eq!(config.environment, OKXEnvironment::Demo);
        assert_eq!(
            config.instrument_types,
            vec![OKXInstrumentType::Spot, OKXInstrumentType::Swap]
        );
        assert_eq!(config.http_timeout_secs, 90);
        assert!(!config.load_spreads);
        assert_eq!(config.book_stale_check_interval_secs, 5);
        assert_eq!(config.book_stale_threshold_secs, 30);
        assert_eq!(config.book_snapshot_timeout_secs, 3);
    }

    #[rstest]
    fn test_data_config_toml_load_spreads() {
        let config: OKXDataClientConfig = toml::from_str(
            "
load_spreads = true
",
        )
        .unwrap();

        assert!(config.load_spreads);
    }

    #[rstest]
    fn test_data_config_toml_book_stale_settings() {
        let config: OKXDataClientConfig = toml::from_str(
            "
book_stale_check_interval_secs = 2
book_stale_threshold_secs = 7
book_snapshot_timeout_secs = 4
",
        )
        .unwrap();

        assert_eq!(config.book_stale_check_interval_secs, 2);
        assert_eq!(config.book_stale_threshold_secs, 7);
        assert_eq!(config.book_snapshot_timeout_secs, 4);
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: OKXExecutionClientConfig = toml::from_str("").unwrap();
        let expected = OKXExecutionClientConfig::default();
        assert_eq!(config.account_id, expected.account_id);
        assert_eq!(config.environment, expected.environment);
        assert_eq!(config.instrument_types, expected.instrument_types);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(config.load_spreads, expected.load_spreads);
        assert_eq!(config.use_mm_mass_cancel, expected.use_mm_mass_cancel);
        assert_eq!(config.transport_backend, expected.transport_backend);
    }

    #[rstest]
    fn test_exec_config_toml_rejects_removed_fills_channel_key() {
        // use_fills_channel was removed: strict decoding must reject stale configs
        let result: Result<OKXExecutionClientConfig, _> =
            toml::from_str("use_fills_channel = true\n");
        assert!(result.is_err());
    }

    #[rstest]
    fn test_exec_config_toml_load_spreads() {
        let config: OKXExecutionClientConfig = toml::from_str(
            "
load_spreads = true
",
        )
        .unwrap();

        assert!(config.load_spreads);
    }

    #[rstest]
    fn test_data_config_default_region_is_global() {
        let config = OKXDataClientConfig::default();

        assert_eq!(config.region, OKXRegion::Global);
        assert_eq!(config.http_base_url(), "https://www.okx.com");
        assert_eq!(config.ws_public_url(), "wss://ws.okx.com:8443/ws/v5/public");
    }

    #[rstest]
    fn test_data_config_eea_region_urls() {
        let config = OKXDataClientConfig::builder()
            .region(OKXRegion::Eea)
            .build();

        assert_eq!(config.http_base_url(), "https://eea.okx.com");
        assert_eq!(
            config.ws_public_url(),
            "wss://wseea.okx.com:8443/ws/v5/public"
        );
        assert_eq!(
            config.ws_business_url(),
            "wss://wseea.okx.com:8443/ws/v5/business"
        );
    }

    #[rstest]
    fn test_exec_config_eea_region_urls() {
        let config = OKXExecutionClientConfig::builder()
            .region(OKXRegion::Eea)
            .build();

        assert_eq!(config.http_base_url(), "https://eea.okx.com");
        assert_eq!(
            config.ws_private_url(),
            "wss://wseea.okx.com:8443/ws/v5/private"
        );
        assert_eq!(
            config.ws_business_url(),
            "wss://wseea.okx.com:8443/ws/v5/business"
        );
    }

    #[rstest]
    fn test_config_region_override_takes_precedence() {
        let config = OKXDataClientConfig::builder()
            .region(OKXRegion::Eea)
            .base_url_http("https://custom.proxy".to_string())
            .build();

        assert_eq!(config.http_base_url(), "https://custom.proxy");
    }

    #[rstest]
    fn test_data_config_toml_region() {
        let config: OKXDataClientConfig = toml::from_str(
            r#"
region = "eea"
"#,
        )
        .unwrap();

        assert_eq!(config.region, OKXRegion::Eea);
    }

    #[rstest]
    fn test_exec_config_auth_timeout_secs() {
        assert_eq!(OKXExecutionClientConfig::default().auth_timeout_secs, None);

        let exec = OKXExecutionClientConfig::builder()
            .auth_timeout_secs(4)
            .build();
        assert_eq!(exec.auth_timeout_secs, Some(4));

        let exec: OKXExecutionClientConfig = toml::from_str("auth_timeout_secs = 8\n").unwrap();
        assert_eq!(exec.auth_timeout_secs, Some(8));
    }
}
