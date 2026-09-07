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

//! Configuration types for the BitMEX adapter clients.

use nautilus_core::{correctness::check_in_range_inclusive_usize, string::secret::SecretString};
use nautilus_model::identifiers::AccountId;
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::{BITMEX_HTTP_TESTNET_URL, BITMEX_HTTP_URL, BITMEX_WS_TESTNET_URL, BITMEX_WS_URL},
    credential::credential_env_vars,
    enums::BitmexEnvironment,
};

pub(crate) const MAX_BROADCASTER_POOL_SIZE: usize = 16;

/// Validates a BitMEX broadcaster pool size.
///
/// # Errors
///
/// Returns an error if `pool_size` is outside `[1, 16]`.
pub(crate) fn validate_broadcaster_pool_size(
    pool_size: usize,
    parameter: &str,
) -> anyhow::Result<()> {
    check_in_range_inclusive_usize(pool_size, 1, MAX_BROADCASTER_POOL_SIZE, parameter)?;
    Ok(())
}

/// Configuration for the BitMEX live data client.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bitmex")
)]
pub struct BitmexDataClientConfig {
    /// Optional API key used for authenticated REST/WebSocket requests.
    pub api_key: Option<SecretString>,
    /// Optional API secret used for authenticated REST/WebSocket requests.
    pub api_secret: Option<SecretString>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry backoff in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Optional heartbeat interval (seconds) for the WebSocket client.
    pub heartbeat_interval_secs: Option<u64>,
    /// Optional WebSocket authentication timeout (seconds), defaulting to
    /// `AUTHENTICATION_TIMEOUT_SECS` when unset.
    pub auth_timeout_secs: Option<u64>,
    /// Receive window in milliseconds for signed requests.
    ///
    /// This value determines how far in the future the `api-expires` timestamp will be set
    /// for signed REST requests. BitMEX uses seconds-granularity Unix timestamps in the
    /// `api-expires` header, calculated as: `current_timestamp + (recv_window_ms / 1000)`.
    ///
    /// **Note**: This parameter is specified in milliseconds for consistency with other
    /// adapter configurations (e.g., Bybit's `recv_window_ms`), but BitMEX only supports
    /// seconds-granularity timestamps. The value is converted via integer division, so
    /// 10000ms becomes 10 seconds, 15500ms becomes 15 seconds, etc.
    ///
    /// A larger window provides more tolerance for clock skew and network latency, but
    /// increases the replay attack window. The default of 10 seconds should be sufficient
    /// for most deployments. Consider increasing this value (e.g., to 30_000ms = 30s) if you
    /// experience request expiration errors due to clock drift or high network latency.
    #[builder(default = 10_000)]
    pub recv_window_ms: u64,
    /// When `true`, only active instruments are requested during bootstrap.
    #[builder(default = true)]
    pub active_only: bool,
    /// Optional interval (minutes) for instrument refresh from REST.
    pub update_instruments_interval_mins: Option<u64>,
    /// BitMEX environment (mainnet or testnet).
    #[builder(default)]
    pub environment: BitmexEnvironment,
    /// Maximum number of requests per second (burst limit).
    #[builder(default = 10)]
    pub max_requests_per_second: u32,
    /// Maximum number of requests per minute (rolling window).
    #[builder(default = 120)]
    pub max_requests_per_minute: u32,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BitmexDataClientConfig {
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    heartbeat_interval_secs: Option<u64>,
    auth_timeout_secs: Option<u64>,
    recv_window_ms: u64,
    active_only: bool,
    update_instruments_interval_mins: Option<u64>,
    environment: BitmexEnvironment,
    max_requests_per_second: u32,
    max_requests_per_minute: u32,
    transport_backend: TransportBackend,
});

impl Default for BitmexDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitmexDataClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns `true` if both API key and secret are available
    /// (either explicitly set or resolvable from environment variables).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and the environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_HTTP_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_HTTP_URL.to_string(),
            })
    }

    /// Returns the WebSocket URL, considering overrides and the environment.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_WS_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_WS_URL.to_string(),
            })
    }
}

/// Configuration for the BitMEX live execution client.
///
/// The submit and cancel broadcaster pools must each contain `[1, 15]` clients, with a combined
/// size in `[2, 16]`.
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bitmex", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bitmex")
)]
pub struct BitmexExecutionClientConfig {
    /// API key used for authenticated requests.
    pub api_key: Option<SecretString>,
    /// API secret used for authenticated requests.
    pub api_secret: Option<SecretString>,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST requests.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry backoff in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval (seconds) for the WebSocket client.
    #[builder(default = 5)]
    pub heartbeat_interval_secs: u64,
    /// Optional WebSocket authentication timeout (seconds), defaulting to
    /// `AUTHENTICATION_TIMEOUT_SECS` when unset.
    pub auth_timeout_secs: Option<u64>,
    /// Receive window in milliseconds for signed requests.
    ///
    /// This value determines how far in the future the `api-expires` timestamp will be set
    /// for signed REST requests. BitMEX uses seconds-granularity Unix timestamps in the
    /// `api-expires` header, calculated as: `current_timestamp + (recv_window_ms / 1000)`.
    ///
    /// **Note**: This parameter is specified in milliseconds for consistency with other
    /// adapter configurations (e.g., Bybit's `recv_window_ms`), but BitMEX only supports
    /// seconds-granularity timestamps. The value is converted via integer division, so
    /// 10000ms becomes 10 seconds, 15500ms becomes 15 seconds, etc.
    ///
    /// A larger window provides more tolerance for clock skew and network latency, but
    /// increases the replay attack window. The default of 10 seconds should be sufficient
    /// for most deployments. Consider increasing this value (e.g., to 30000ms = 30s) if you
    /// experience request expiration errors due to clock drift or high network latency.
    #[builder(default = 10_000)]
    pub recv_window_ms: u64,
    /// When `true`, only active instruments are requested during bootstrap.
    #[builder(default = true)]
    pub active_only: bool,
    /// BitMEX environment (mainnet or testnet).
    #[builder(default)]
    pub environment: BitmexEnvironment,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
    /// Maximum number of requests per second (burst limit).
    #[builder(default = 10)]
    pub max_requests_per_second: u32,
    /// Maximum number of requests per minute (rolling window).
    #[builder(default = 120)]
    pub max_requests_per_minute: u32,
    /// Number of HTTP clients in the submit broadcaster pool
    /// (effective range `[1, 15]`, defaults to 1).
    pub submitter_pool_size: Option<usize>,
    /// Number of HTTP clients in the cancel broadcaster pool
    /// (effective range `[1, 15]`, defaults to 1).
    pub canceller_pool_size: Option<usize>,
    /// Optional list of proxy URLs for submit broadcaster pool (path diversity).
    pub submitter_proxy_urls: Option<Vec<SecretString>>,
    /// Optional list of proxy URLs for cancel broadcaster pool (path diversity).
    pub canceller_proxy_urls: Option<Vec<SecretString>>,
    /// Optional dead man's switch timeout in seconds.
    ///
    /// When set, a background task periodically calls the BitMEX `cancelAllAfter` endpoint
    /// to keep a server-side timer alive. If the client loses connectivity the timer expires
    /// and BitMEX cancels all open orders. Calling with `timeout=0` disarms the switch.
    /// The refresh interval is derived as `timeout / 4` (minimum 1 second).
    pub deadmans_switch_timeout_secs: Option<u64>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BitmexExecutionClientConfig {
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    heartbeat_interval_secs: u64,
    auth_timeout_secs: Option<u64>,
    recv_window_ms: u64,
    active_only: bool,
    environment: BitmexEnvironment,
    account_id: Option<AccountId>,
    max_requests_per_second: u32,
    max_requests_per_minute: u32,
    submitter_pool_size: Option<usize>,
    canceller_pool_size: Option<usize>,
    deadmans_switch_timeout_secs: Option<u64>,
    transport_backend: TransportBackend,
});

impl Default for BitmexExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BitmexExecutionClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates the individual and combined broadcaster pool sizes.
    ///
    /// # Errors
    ///
    /// Returns an error if either pool is outside `[1, 15]` or their combined size is outside
    /// `[2, 16]`.
    pub(crate) fn validate_broadcaster_pool_sizes(&self) -> anyhow::Result<()> {
        let submitter_pool_size = self.submitter_pool_size.unwrap_or(1);
        let canceller_pool_size = self.canceller_pool_size.unwrap_or(1);
        validate_broadcaster_pool_size(submitter_pool_size, "submitter_pool_size")?;
        validate_broadcaster_pool_size(canceller_pool_size, "canceller_pool_size")?;
        let combined_pool_size = submitter_pool_size
            .checked_add(canceller_pool_size)
            .ok_or_else(|| anyhow::anyhow!("combined BitMEX broadcaster pool size overflow"))?;
        check_in_range_inclusive_usize(
            combined_pool_size,
            2,
            MAX_BROADCASTER_POOL_SIZE,
            "combined_pool_size",
        )?;
        Ok(())
    }

    /// Returns `true` if both API key and secret are available
    /// (either explicitly set or resolvable from environment variables).
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let has_key = self.api_key.is_some() || std::env::var(key_var).is_ok();
        let has_secret = self.api_secret.is_some() || std::env::var(secret_var).is_ok();
        has_key && has_secret
    }

    /// Returns the REST base URL, considering overrides and the environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_HTTP_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_HTTP_URL.to_string(),
            })
    }

    /// Returns the WebSocket URL, considering overrides and the environment.
    #[must_use]
    pub fn ws_url(&self) -> String {
        self.base_url_ws
            .clone()
            .unwrap_or_else(|| match self.environment {
                BitmexEnvironment::Testnet => BITMEX_WS_TESTNET_URL.to_string(),
                BitmexEnvironment::Mainnet => BITMEX_WS_URL.to_string(),
            })
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(1)]
    #[case(3)]
    #[case(MAX_BROADCASTER_POOL_SIZE)]
    fn test_validate_broadcaster_pool_size_accepts_supported_values(#[case] pool_size: usize) {
        assert!(validate_broadcaster_pool_size(pool_size, "pool_size").is_ok());
    }

    #[rstest]
    #[case(0)]
    #[case(MAX_BROADCASTER_POOL_SIZE + 1)]
    #[case(usize::MAX)]
    fn test_validate_broadcaster_pool_size_rejects_invalid_values(#[case] pool_size: usize) {
        assert!(validate_broadcaster_pool_size(pool_size, "pool_size").is_err());
    }

    #[rstest]
    #[case(Some(1), Some(1))]
    #[case(Some(MAX_BROADCASTER_POOL_SIZE - 1), Some(1))]
    #[case(Some(1), Some(MAX_BROADCASTER_POOL_SIZE - 1))]
    fn test_execution_config_accepts_supported_combined_pool_size(
        #[case] submitter_pool_size: Option<usize>,
        #[case] canceller_pool_size: Option<usize>,
    ) {
        let config = BitmexExecutionClientConfig {
            submitter_pool_size,
            canceller_pool_size,
            ..Default::default()
        };

        assert!(config.validate_broadcaster_pool_sizes().is_ok());
    }

    #[rstest]
    #[case(Some(0), Some(1))]
    #[case(Some(1), Some(0))]
    #[case(Some(MAX_BROADCASTER_POOL_SIZE), Some(1))]
    #[case(Some(usize::MAX), Some(1))]
    fn test_execution_config_rejects_invalid_pool_sizes(
        #[case] submitter_pool_size: Option<usize>,
        #[case] canceller_pool_size: Option<usize>,
    ) {
        let config = BitmexExecutionClientConfig {
            submitter_pool_size,
            canceller_pool_size,
            ..Default::default()
        };

        assert!(config.validate_broadcaster_pool_sizes().is_err());
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: BitmexDataClientConfig = toml::from_str(
            r#"
environment = "testnet"
http_timeout_secs = 30
active_only = false
max_requests_per_second = 5
"#,
        )
        .unwrap();

        assert_eq!(config.environment, BitmexEnvironment::Testnet);
        assert_eq!(config.http_timeout_secs, 30);
        assert!(!config.active_only);
        assert_eq!(config.max_requests_per_second, 5);
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: BitmexExecutionClientConfig = toml::from_str("").unwrap();
        let expected = BitmexExecutionClientConfig::default();

        assert_eq!(config.environment, expected.environment);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(
            config.heartbeat_interval_secs,
            expected.heartbeat_interval_secs,
        );
        assert_eq!(config.recv_window_ms, expected.recv_window_ms);
        assert_eq!(config.active_only, expected.active_only);
        assert_eq!(
            config.max_requests_per_second,
            expected.max_requests_per_second,
        );
        assert_eq!(config.transport_backend, expected.transport_backend);
    }

    #[rstest]
    fn test_config_auth_timeout_secs() {
        assert_eq!(BitmexDataClientConfig::default().auth_timeout_secs, None);
        assert_eq!(
            BitmexExecutionClientConfig::default().auth_timeout_secs,
            None
        );

        let data = BitmexDataClientConfig::builder()
            .auth_timeout_secs(3)
            .build();
        assert_eq!(data.auth_timeout_secs, Some(3));

        let exec = BitmexExecutionClientConfig::builder()
            .auth_timeout_secs(4)
            .build();
        assert_eq!(exec.auth_timeout_secs, Some(4));

        let data: BitmexDataClientConfig = toml::from_str("auth_timeout_secs = 7\n").unwrap();
        assert_eq!(data.auth_timeout_secs, Some(7));

        let exec: BitmexExecutionClientConfig = toml::from_str("auth_timeout_secs = 8\n").unwrap();
        assert_eq!(exec.auth_timeout_secs, Some(8));
    }

    #[rstest]
    fn test_config_debug_redacts_credentials() {
        let data = BitmexDataClientConfig {
            api_key: Some("data-api-key".into()),
            api_secret: Some("data-api-secret".into()),
            proxy_url: Some("http://data-user:data-password@localhost".into()),
            ..Default::default()
        };
        let execution = BitmexExecutionClientConfig {
            api_key: Some("execution-api-key".into()),
            api_secret: Some("execution-api-secret".into()),
            proxy_url: Some("http://execution-user:execution-password@localhost".into()),
            submitter_proxy_urls: Some(vec!["http://submit-user:submit-password@localhost".into()]),
            canceller_proxy_urls: Some(vec!["http://cancel-user:cancel-password@localhost".into()]),
            ..Default::default()
        };

        let debug = format!("{data:?} {execution:?}");

        assert!(!debug.contains("data-api-key"));
        assert!(!debug.contains("data-api-secret"));
        assert!(!debug.contains("data-password"));
        assert!(!debug.contains("execution-api-key"));
        assert!(!debug.contains("execution-api-secret"));
        assert!(!debug.contains("execution-password"));
        assert!(!debug.contains("submit-password"));
        assert!(!debug.contains("cancel-password"));
    }
}
