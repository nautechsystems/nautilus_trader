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

//! Configuration structures for the Bybit adapter.

use std::{collections::HashMap, fmt::Debug};

use nautilus_common::config::{ConfigError, ConfigResult, check_unique_values};
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::string::secret::SecretString;
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::BYBIT_VENUE,
    credential::credential_env_vars,
    enums::{
        BybitEnvironment, BybitMarginMode, BybitOrderSmpType, BybitPositionMode, BybitProductType,
    },
    parse::deserialize_optional_smp_type,
    urls::{
        bybit_http_base_url, bybit_ws_private_url, bybit_ws_public_url, bybit_ws_trade_url,
        diagnostic_origin,
    },
};

/// Configuration for the Bybit live data client.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bybit")
)]
pub struct BybitDataClientConfig {
    /// API key.
    ///
    /// When omitted, the client loads `BYBIT_API_KEY`, `BYBIT_DEMO_API_KEY`, or
    /// `BYBIT_TESTNET_API_KEY` for the selected [`BybitEnvironment`]. An explicit
    /// value takes precedence. Public market-data requests stay unsigned.
    pub api_key: Option<SecretString>,
    /// API secret.
    ///
    /// When omitted, the client loads `BYBIT_API_SECRET`, `BYBIT_DEMO_API_SECRET`,
    /// or `BYBIT_TESTNET_API_SECRET` for the selected [`BybitEnvironment`]. An
    /// explicit value takes precedence. A missing key or secret leaves the client
    /// without credentials.
    pub api_secret: Option<SecretString>,
    /// Product types to subscribe to (e.g., Linear, Spot, Inverse, Option).
    #[builder(default = vec![BybitProductType::Linear])]
    pub product_types: Vec<BybitProductType>,
    /// Environment selection (Mainnet, Testnet, Demo).
    #[builder(default = BybitEnvironment::Mainnet)]
    pub environment: BybitEnvironment,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the public WebSocket URL.
    pub base_url_ws_public: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST reads.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial retry backoff for REST reads, in milliseconds.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff for REST reads, in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval in seconds for WebSocket clients.
    #[builder(default = 20)]
    pub heartbeat_interval_secs: u64,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Interval in minutes for instrument refresh from REST.
    /// When `None`, instrument refresh is disabled.
    pub update_instruments_interval_mins: Option<u64>,
    /// Interval in seconds for polling instrument definitions and status changes from REST.
    /// When `None`, instrument/status polling is disabled.
    pub instrument_poll_interval_secs: Option<u64>,
    /// Maximum time to wait for an initial, post-reconnect, or recovery order book
    /// snapshot in seconds. Set to 0 to disable.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Whether bar timestamps use the close time instead of the open time.
    #[builder(default = true)]
    pub bars_timestamp_on_close: bool,
}

pub(crate) fn diagnostic_credential(
    explicit: Option<&SecretString>,
    env_var: &str,
) -> Option<SecretString> {
    if let Some(value) = explicit {
        return Some(value.clone());
    }

    std::env::var(env_var).ok().map(SecretString::from)
}

impl Debug for BybitDataClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let api_key = diagnostic_credential(self.api_key.as_ref(), key_var);
        let api_secret = diagnostic_credential(self.api_secret.as_ref(), secret_var);
        let api_key = api_key.as_ref().map(SecretString::expose_secret);
        let api_secret = api_secret.as_ref().map(SecretString::expose_secret);
        f.debug_struct(stringify!(BybitDataClientConfig))
            .field("api_key", &self.api_key)
            .field("api_secret", &self.api_secret)
            .field("product_types", &self.product_types)
            .field("environment", &self.environment)
            .field(
                "base_url_http",
                &self
                    .base_url_http
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field(
                "base_url_ws_public",
                &self
                    .base_url_ws_public
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field(
                "base_url_ws_private",
                &self
                    .base_url_ws_private
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field("proxy_url", &self.proxy_url)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("heartbeat_interval_secs", &self.heartbeat_interval_secs)
            .field("recv_window_ms", &self.recv_window_ms)
            .field(
                "update_instruments_interval_mins",
                &self.update_instruments_interval_mins,
            )
            .field(
                "instrument_poll_interval_secs",
                &self.instrument_poll_interval_secs,
            )
            .field(
                "book_snapshot_timeout_secs",
                &self.book_snapshot_timeout_secs,
            )
            .field("transport_backend", &self.transport_backend)
            .field("bars_timestamp_on_close", &self.bars_timestamp_on_close)
            .finish()
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BybitDataClientConfig {
    product_types: Vec<BybitProductType>,
    environment: BybitEnvironment,
    base_url_http: Option<String>,
    base_url_ws_public: Option<String>,
    base_url_ws_private: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    heartbeat_interval_secs: u64,
    recv_window_ms: u64,
    update_instruments_interval_mins: Option<u64>,
    book_snapshot_timeout_secs: u64,
    transport_backend: TransportBackend,
    bars_timestamp_on_close: bool,
});

impl Default for BybitDataClientConfig {
    fn default() -> Self {
        Self {
            update_instruments_interval_mins: Some(60),
            instrument_poll_interval_secs: Some(60),
            ..Self::builder().build()
        }
    }
}

impl BybitDataClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates that configured selections contain no duplicates.
    ///
    /// # Errors
    ///
    /// Returns an error if `product_types` repeats a value.
    pub fn validate(&self) -> ConfigResult<()> {
        check_unique_values("product_types", &self.product_types)
    }

    /// Returns `true` if both API key and secret fields are set.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| bybit_http_base_url(self.environment).to_string())
    }

    /// Returns the public WebSocket URL for the given product type.
    ///
    /// Falls back to the first product type in the config if multiple are configured.
    #[must_use]
    pub fn ws_public_url(&self) -> String {
        self.base_url_ws_public.clone().unwrap_or_else(|| {
            let product_type = self
                .product_types
                .first()
                .copied()
                .unwrap_or(BybitProductType::Linear);
            bybit_ws_public_url(product_type, self.environment)
        })
    }

    /// Returns the public WebSocket URL for a specific product type.
    #[must_use]
    pub fn ws_public_url_for(&self, product_type: BybitProductType) -> String {
        self.base_url_ws_public
            .clone()
            .unwrap_or_else(|| bybit_ws_public_url(product_type, self.environment))
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| bybit_ws_private_url(self.environment).to_string())
    }

    /// Returns `true` when private WebSocket connection is required.
    #[must_use]
    pub fn requires_private_ws(&self) -> bool {
        self.has_api_credentials()
    }
}

/// Configuration for the Bybit live execution client.
#[derive(Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.bybit", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.bybit")
)]
pub struct BybitExecutionClientConfig {
    /// API key for authenticated requests.
    pub api_key: Option<SecretString>,
    /// API secret for authenticated requests.
    pub api_secret: Option<SecretString>,
    /// Product types to support (e.g., Linear, Spot, Inverse, Option).
    #[builder(default = vec![BybitProductType::Linear])]
    pub product_types: Vec<BybitProductType>,
    /// Environment selection (Mainnet, Testnet, Demo).
    #[builder(default = BybitEnvironment::Mainnet)]
    pub environment: BybitEnvironment,
    /// Optional override for the REST base URL.
    pub base_url_http: Option<String>,
    /// Optional override for the private WebSocket URL.
    pub base_url_ws_private: Option<String>,
    /// Optional override for the trade WebSocket URL.
    pub base_url_ws_trade: Option<String>,
    /// Optional proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// REST timeout in seconds.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Maximum retry attempts for REST reads.
    #[builder(default = 3)]
    pub max_retries: u32,
    /// Initial REST read retry backoff, in milliseconds. Also the cancel-all polling interval.
    #[builder(default = 1_000)]
    pub retry_delay_initial_ms: u64,
    /// Maximum retry backoff for REST reads, in milliseconds.
    #[builder(default = 10_000)]
    pub retry_delay_max_ms: u64,
    /// Heartbeat interval in seconds for WebSocket clients.
    #[builder(default = 20)]
    pub heartbeat_interval_secs: u64,
    /// Optional WebSocket authentication wait timeout (seconds), defaulting to
    /// the client default when unset.
    pub auth_timeout_secs: Option<u64>,
    /// Receive window in milliseconds for signed requests.
    #[builder(default = 5_000)]
    pub recv_window_ms: u64,
    /// Optional account identifier to associate with the execution client.
    pub account_id: Option<AccountId>,
    /// Whether scoped execution-client SPOT position requests derive positions from wallet
    /// balances. The HTTP client rejects enabled unscoped SPOT requests because balances cannot be
    /// attributed to pairs. The execution client omits SPOT from bulk requests and reports its bulk
    /// coverage as unavailable.
    #[builder(default)]
    pub use_spot_position_reports: bool,
    /// Whether to automatically repay SPOT margin borrows after BUY orders tracked by
    /// this client and reported on the standard `execution` channel (not `execution.fast`)
    /// fully fill.
    #[builder(default)]
    pub auto_repay_spot_borrows: bool,
    /// Whether to subscribe to the lower-latency `execution.fast` channel instead of the
    /// standard `execution` channel. Fast fills derive commissions from cached account fee
    /// rates; derived values are not venue-authoritative. The client skips a fast fill whose
    /// commission it cannot derive. Demo mode and the Option product type do not support
    /// this option.
    #[builder(default)]
    pub use_ws_execution_fast: bool,
    /// Leverage configuration for futures (symbol -> leverage).
    pub futures_leverages: Option<HashMap<String, u32>>,
    /// Position mode configuration for symbols (symbol -> mode).
    pub position_mode: Option<HashMap<String, BybitPositionMode>>,
    /// Unified margin mode setting.
    pub margin_mode: Option<BybitMarginMode>,
    /// Self-match prevention type sent on every submitted order. The `smp_type` order parameter
    /// overrides it, and leaving both unset omits the field so the venue default applies.
    #[serde(deserialize_with = "deserialize_optional_smp_type")]
    pub smp_type: Option<BybitOrderSmpType>,
    /// WebSocket transport backend (defaults to `Tungstenite`).
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Instruments included in reconciliation. `None` or empty includes all configured product types.
    /// IDs must use the `BYBIT` venue, an uppercase symbol, and a configured product-type suffix.
    pub load_ids: Option<Vec<InstrumentId>>,
}

impl Debug for BybitExecutionClientConfig {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (key_var, secret_var) = credential_env_vars(self.environment);
        let api_key = diagnostic_credential(self.api_key.as_ref(), key_var);
        let api_secret = diagnostic_credential(self.api_secret.as_ref(), secret_var);
        let api_key = api_key.as_ref().map(SecretString::expose_secret);
        let api_secret = api_secret.as_ref().map(SecretString::expose_secret);
        f.debug_struct(stringify!(BybitExecutionClientConfig))
            .field("api_key", &self.api_key)
            .field("api_secret", &self.api_secret)
            .field("product_types", &self.product_types)
            .field("environment", &self.environment)
            .field(
                "base_url_http",
                &self
                    .base_url_http
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field(
                "base_url_ws_private",
                &self
                    .base_url_ws_private
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field(
                "base_url_ws_trade",
                &self
                    .base_url_ws_trade
                    .as_deref()
                    .map(|url| diagnostic_origin(url, api_key, api_secret)),
            )
            .field("proxy_url", &self.proxy_url)
            .field("http_timeout_secs", &self.http_timeout_secs)
            .field("max_retries", &self.max_retries)
            .field("retry_delay_initial_ms", &self.retry_delay_initial_ms)
            .field("retry_delay_max_ms", &self.retry_delay_max_ms)
            .field("heartbeat_interval_secs", &self.heartbeat_interval_secs)
            .field("auth_timeout_secs", &self.auth_timeout_secs)
            .field("recv_window_ms", &self.recv_window_ms)
            .field("account_id", &self.account_id)
            .field("use_spot_position_reports", &self.use_spot_position_reports)
            .field("auto_repay_spot_borrows", &self.auto_repay_spot_borrows)
            .field("use_ws_execution_fast", &self.use_ws_execution_fast)
            .field("futures_leverages", &self.futures_leverages)
            .field("position_mode", &self.position_mode)
            .field("margin_mode", &self.margin_mode)
            .field("smp_type", &self.smp_type)
            .field("transport_backend", &self.transport_backend)
            .field("load_ids", &self.load_ids)
            .finish()
    }
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(BybitExecutionClientConfig {
    product_types: Vec<BybitProductType>,
    environment: BybitEnvironment,
    base_url_http: Option<String>,
    base_url_ws_private: Option<String>,
    base_url_ws_trade: Option<String>,
    http_timeout_secs: u64,
    max_retries: u32,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    heartbeat_interval_secs: u64,
    auth_timeout_secs: Option<u64>,
    recv_window_ms: u64,
    account_id: Option<AccountId>,
    use_spot_position_reports: bool,
    auto_repay_spot_borrows: bool,
    use_ws_execution_fast: bool,
    margin_mode: Option<BybitMarginMode>,
    transport_backend: TransportBackend,
    load_ids: Option<Vec<InstrumentId>>,
});

impl Default for BybitExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl BybitExecutionClientConfig {
    /// Creates a configuration with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Validates configured product types, reconciliation scope, and execution-stream selection.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `product_types` repeats a value.
    /// - Fast execution is enabled in Demo mode.
    /// - Fast execution is enabled with the Option product type.
    /// - `load_ids` contains duplicate or invalid instruments.
    pub fn validate(&self) -> ConfigResult<()> {
        if self.use_ws_execution_fast && self.environment == BybitEnvironment::Demo {
            return Err(ConfigError::unsupported_value(
                "use_ws_execution_fast",
                "execution.fast is not supported in demo mode",
            ));
        }

        if self.use_ws_execution_fast && self.product_types.contains(&BybitProductType::Option) {
            return Err(ConfigError::unsupported_value(
                "use_ws_execution_fast",
                "execution.fast carries no fee inputs for the Option product type",
            ));
        }

        check_unique_values("product_types", &self.product_types)?;

        if let Some(load_ids) = &self.load_ids {
            check_unique_values("load_ids", load_ids)?;

            for instrument_id in load_ids {
                let product_type = BybitProductType::from_suffix(instrument_id.symbol.as_str());
                let configured = product_type.is_some_and(|product_type| {
                    instrument_id.symbol.as_str().len() > product_type.suffix().len()
                        && if self.product_types.is_empty() {
                            product_type == BybitProductType::Linear
                        } else {
                            self.product_types.contains(&product_type)
                        }
                });

                if instrument_id.venue != *BYBIT_VENUE
                    || instrument_id
                        .symbol
                        .as_str()
                        .bytes()
                        .any(|b| b.is_ascii_lowercase())
                    || !configured
                {
                    return Err(ConfigError::invalid_value(
                        "load_ids",
                        format!(
                            "{instrument_id} is not an uppercase BYBIT instrument in a configured product type"
                        ),
                    ));
                }
            }
        }

        Ok(())
    }

    /// Returns `true` if both API key and secret are available.
    #[must_use]
    pub fn has_api_credentials(&self) -> bool {
        self.api_key.is_some() && self.api_secret.is_some()
    }

    /// Returns the REST base URL, considering overrides and environment.
    #[must_use]
    pub fn http_base_url(&self) -> String {
        self.base_url_http
            .clone()
            .unwrap_or_else(|| bybit_http_base_url(self.environment).to_string())
    }

    /// Returns the private WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_private_url(&self) -> String {
        self.base_url_ws_private
            .clone()
            .unwrap_or_else(|| bybit_ws_private_url(self.environment).to_string())
    }

    /// Returns the trade WebSocket URL, considering overrides and environment.
    #[must_use]
    pub fn ws_trade_url(&self) -> String {
        self.base_url_ws_trade
            .clone()
            .unwrap_or_else(|| bybit_ws_trade_url(self.environment).to_string())
    }
}
#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{cache::Cache, config::ConfigError};
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        enums::{AccountType, OmsType},
        identifiers::{ClientId, TraderId, Venue},
    };
    use rstest::rstest;

    use super::*;
    use crate::{data::BybitDataClient, execution::BybitExecutionClient};

    #[rstest]
    #[case::none(None, vec![BybitProductType::Linear], true)]
    #[case::empty(Some(vec![]), vec![], true)]
    #[case::linear(Some(vec!["BTCUSDT-LINEAR.BYBIT"]), vec![], true)]
    #[case::spot(Some(vec!["ETHUSDT-SPOT.BYBIT"]), vec![BybitProductType::Spot], true)]
    #[case::foreign_venue(Some(vec!["BTCUSDT-LINEAR.BINANCE"]), vec![], false)]
    #[case::unconfigured(Some(vec!["ETHUSDT-SPOT.BYBIT"]), vec![], false)]
    #[case::no_suffix(Some(vec!["BTCUSDT.BYBIT"]), vec![], false)]
    #[case::empty_linear_symbol(Some(vec!["-LINEAR.BYBIT"]), vec![], false)]
    #[case::empty_spot_symbol(Some(vec!["-SPOT.BYBIT"]), vec![BybitProductType::Spot], false)]
    #[case::empty_inverse_symbol(Some(vec!["-INVERSE.BYBIT"]), vec![BybitProductType::Inverse], false)]
    #[case::empty_option_symbol(Some(vec!["-OPTION.BYBIT"]), vec![BybitProductType::Option], false)]
    #[case::lowercase(Some(vec!["btcusdt-LINEAR.BYBIT"]), vec![], false)]
    #[case::duplicate(Some(vec!["BTCUSDT-LINEAR.BYBIT", "BTCUSDT-LINEAR.BYBIT"]), vec![], false)]
    fn test_execution_reconciliation_scope_validation(
        #[case] ids: Option<Vec<&str>>,
        #[case] product_types: Vec<BybitProductType>,
        #[case] valid: bool,
    ) {
        let load_ids = ids.map(|ids| ids.into_iter().map(InstrumentId::from).collect());

        let config = BybitExecutionClientConfig {
            product_types,
            load_ids,
            ..Default::default()
        };

        let result = config.validate();

        let expected = if valid {
            Ok(())
        } else if config.load_ids.as_ref().unwrap().len() > 1 {
            Err(ConfigError::duplicate("load_ids", None))
        } else {
            let instrument_id = config.load_ids.as_ref().unwrap()[0];
            Err(ConfigError::invalid_value(
                "load_ids",
                format!(
                    "{instrument_id} is not an uppercase BYBIT instrument in a configured product type"
                ),
            ))
        };

        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_data_client_rejects_duplicate_types() {
        let config = BybitDataClientConfig {
            product_types: vec![BybitProductType::Linear, BybitProductType::Linear],
            ..Default::default()
        };

        let error = BybitDataClient::new(ClientId::from("BYBIT"), config)
            .expect_err("duplicate config must fail before client setup");

        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate("product_types", None)),
        );
    }

    #[rstest]
    #[case::empty(vec![], false)]
    #[case::distinct(vec![BybitProductType::Spot, BybitProductType::Linear], false)]
    #[case::adjacent(vec![BybitProductType::Linear, BybitProductType::Linear], true)]
    #[case::separated(vec![BybitProductType::Linear, BybitProductType::Spot, BybitProductType::Linear], true)]
    fn test_config_duplicate_types(#[case] values: Vec<BybitProductType>, #[case] duplicate: bool) {
        let data = BybitDataClientConfig {
            product_types: values.clone(),
            ..Default::default()
        };

        let execution = BybitExecutionClientConfig {
            product_types: values.clone(),
            ..Default::default()
        };

        let expected = if duplicate {
            Err(ConfigError::duplicate("product_types", None))
        } else {
            Ok(())
        };

        let data_result = data.validate();
        let execution_result = execution.validate();

        assert_eq!(data_result, expected);
        assert_eq!(execution_result, expected);
        assert_eq!(data.product_types, values);
        assert_eq!(execution.product_types, values);
    }

    #[rstest]
    #[case::demo_fast(
        BybitEnvironment::Demo,
        vec![BybitProductType::Linear],
        true,
        Some("execution.fast is not supported in demo mode")
    )]
    #[case::demo_standard(BybitEnvironment::Demo, vec![BybitProductType::Linear], false, None)]
    #[case::mainnet_fast(BybitEnvironment::Mainnet, vec![BybitProductType::Linear], true, None)]
    #[case::testnet_fast(BybitEnvironment::Testnet, vec![BybitProductType::Linear], true, None)]
    #[case::option_fast(
        BybitEnvironment::Mainnet,
        vec![BybitProductType::Linear, BybitProductType::Option],
        true,
        Some("execution.fast carries no fee inputs for the Option product type")
    )]
    #[case::option_standard(
        BybitEnvironment::Mainnet,
        vec![BybitProductType::Linear, BybitProductType::Option],
        false,
        None
    )]
    fn test_exec_config_fast_validation(
        #[case] environment: BybitEnvironment,
        #[case] product_types: Vec<BybitProductType>,
        #[case] use_ws_execution_fast: bool,
        #[case] rejection: Option<&str>,
    ) {
        let config = BybitExecutionClientConfig {
            product_types,
            environment,
            use_ws_execution_fast,
            ..Default::default()
        };

        let result = config.validate();

        let expected = rejection.map_or(Ok(()), |reason| {
            Err(ConfigError::unsupported_value(
                "use_ws_execution_fast",
                reason,
            ))
        });

        assert_eq!(result, expected);
    }

    #[rstest]
    fn test_config_debug_redacts_credentials() {
        let data = BybitDataClientConfig {
            api_key: Some("data-api-key".into()),
            api_secret: Some("data-api-secret".into()),
            proxy_url: Some("http://user:data-proxy@localhost".into()),
            ..Default::default()
        };
        let execution = BybitExecutionClientConfig {
            api_key: Some("exec-api-key".into()),
            api_secret: Some("exec-api-secret".into()),
            proxy_url: Some("http://user:exec-proxy@localhost".into()),
            ..Default::default()
        };

        let formatted = format!("{data:?} {execution:?}");

        assert_eq!(formatted.matches(REDACTED).count(), 6);

        for secret in [
            "data-api-key",
            "data-api-secret",
            "data-proxy",
            "exec-api-key",
            "exec-api-secret",
            "exec-proxy",
        ] {
            assert!(!formatted.contains(secret));
        }
    }

    #[rstest]
    fn test_config_debug_shows_public_origin_and_redacts_secret_host() {
        let data = BybitDataClientConfig {
            api_key: Some("AbCdEfGh12345678".into()),
            base_url_http: Some(
                "https://user:http-secret@api.example.com:8443/http-path?token=http-query#http-frag"
                    .to_string(),
            ),
            base_url_ws_public: Some(
                "wss://AbCdEfGh12345678.example.com/ws-path?token=ws-query".to_string(),
            ),
            base_url_ws_private: Some("not a url".to_string()),
            http_timeout_secs: 41,
            ..Default::default()
        };

        let execution = BybitExecutionClientConfig {
            base_url_ws_trade: Some(
                "wss://trade.example.com/trade-path?token=trade-query".to_string(),
            ),
            recv_window_ms: 43,
            ..Default::default()
        };

        let data_debug = format!("{data:?}");
        let execution_debug = format!("{execution:?}");

        assert!(data_debug.contains("base_url_http: Some(\"https://api.example.com:8443\")"));
        assert!(data_debug.contains("base_url_ws_public: Some(\"<redacted>\")"));
        assert!(data_debug.contains("base_url_ws_private: Some(\"<redacted>\")"));
        assert!(data_debug.contains("http_timeout_secs: 41"));
        assert!(execution_debug.contains("base_url_ws_trade: Some(\"wss://trade.example.com\")"));
        assert!(execution_debug.contains("recv_window_ms: 43"));

        for secret in [
            "http-secret",
            "http-path",
            "http-query",
            "http-frag",
            "AbCdEfGh12345678",
            "ws-path",
            "ws-query",
            "trade-path",
            "trade-query",
            "not a url",
        ] {
            assert!(!data_debug.contains(secret), "{secret} in {data_debug}");
            assert!(
                !execution_debug.contains(secret),
                "{secret} in {execution_debug}"
            );
        }
    }

    #[rstest]
    fn config_debug_redacts_a_host_containing_only_the_configured_key() {
        let data = BybitDataClientConfig {
            api_key: Some("secret99".into()),
            base_url_ws_public: Some("wss://gateway-secret99.example.com/public".to_string()),
            ..Default::default()
        };

        let data_debug = format!("{data:?}");

        assert!(data_debug.contains("base_url_ws_public: Some(\"<redacted>\")"));
        assert!(!data_debug.contains("secret99"), "{data_debug}");
    }

    #[rstest]
    #[case("None", BybitOrderSmpType::None)]
    #[case("CancelMaker", BybitOrderSmpType::CancelMaker)]
    #[case("CancelTaker", BybitOrderSmpType::CancelTaker)]
    #[case("CancelBoth", BybitOrderSmpType::CancelBoth)]
    fn test_exec_config_deserializes_smp_type(
        #[case] value: &str,
        #[case] expected: BybitOrderSmpType,
    ) {
        let json = format!(r#"{{"smp_type": "{value}"}}"#);
        let config: BybitExecutionClientConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(config.smp_type, Some(expected));
    }

    #[rstest]
    #[case(r#"{"smp_type": "Other"}"#)]
    #[case(r#"{"smp_type": "cancel-maker"}"#)]
    #[case(r#"{"smp_type": ""}"#)]
    fn test_exec_config_rejects_invalid_smp_type(#[case] json: &str) {
        let err = serde_json::from_str::<BybitExecutionClientConfig>(json).unwrap_err();

        assert!(
            err.to_string().contains("invalid Bybit smp_type"),
            "expected an smp_type rejection, was '{err}'"
        );
    }

    #[rstest]
    fn test_exec_config_smp_type_defaults_to_none() {
        let config: BybitExecutionClientConfig = serde_json::from_str("{}").unwrap();

        assert_eq!(config.smp_type, None);
    }

    #[rstest]
    fn test_data_config_default() {
        let config = BybitDataClientConfig::default();

        assert!(config.bars_timestamp_on_close);
        assert!(!config.has_api_credentials());
        assert_eq!(config.product_types, vec![BybitProductType::Linear]);
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.heartbeat_interval_secs, 20);
        assert_eq!(config.book_snapshot_timeout_secs, 10);
    }

    #[rstest]
    fn test_data_config_bar_timestamp_builder() {
        assert!(
            BybitDataClientConfig::builder()
                .build()
                .bars_timestamp_on_close
        );
        assert!(
            serde_json::from_str::<BybitDataClientConfig>("{}")
                .unwrap()
                .bars_timestamp_on_close
        );
        let config = BybitDataClientConfig::builder()
            .bars_timestamp_on_close(false)
            .build();
        assert!(!config.bars_timestamp_on_close);
        let config: BybitDataClientConfig =
            serde_json::from_str(r#"{"bars_timestamp_on_close":false}"#).unwrap();
        assert!(!config.bars_timestamp_on_close);
    }

    #[rstest]
    fn test_data_config_with_credentials() {
        let config = BybitDataClientConfig {
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            ..Default::default()
        };

        assert!(config.has_api_credentials());
        assert!(config.requires_private_ws());
    }

    #[rstest]
    fn test_data_config_http_url_mainnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_testnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-testnet.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_demo() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Demo,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-demo.bybit.com");
    }

    #[rstest]
    fn test_data_config_http_url_override() {
        let custom_url = "https://custom.bybit.com";
        let config = BybitDataClientConfig {
            base_url_http: Some(custom_url.to_string()),
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), custom_url);
    }

    #[rstest]
    fn test_data_config_ws_public_url() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_public_url(),
            "wss://stream.bybit.com/v5/public/linear"
        );
    }

    #[rstest]
    fn test_data_config_ws_public_url_for_spot() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_public_url_for(BybitProductType::Spot),
            "wss://stream.bybit.com/v5/public/spot"
        );
    }

    #[rstest]
    fn test_data_config_ws_private_url() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.ws_private_url(), "wss://stream.bybit.com/v5/private");
    }

    #[rstest]
    fn test_data_config_ws_private_url_testnet() {
        let config = BybitDataClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(
            config.ws_private_url(),
            "wss://stream-testnet.bybit.com/v5/private"
        );
    }

    #[rstest]
    fn test_exec_config_default() {
        let config = BybitExecutionClientConfig::default();

        assert!(!config.has_api_credentials());
        assert_eq!(config.product_types, vec![BybitProductType::Linear]);
        assert_eq!(config.http_timeout_secs, 60);
        assert_eq!(config.heartbeat_interval_secs, 20);
    }

    #[rstest]
    fn test_exec_config_with_credentials() {
        let config = BybitExecutionClientConfig {
            api_key: Some("test_key".into()),
            api_secret: Some("test_secret".into()),
            ..Default::default()
        };

        assert!(config.has_api_credentials());
    }

    #[rstest]
    fn test_exec_config_urls() {
        let config = BybitExecutionClientConfig {
            environment: BybitEnvironment::Mainnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api.bybit.com");
        assert_eq!(config.ws_private_url(), "wss://stream.bybit.com/v5/private");
        assert_eq!(config.ws_trade_url(), "wss://stream.bybit.com/v5/trade");
    }

    #[rstest]
    fn test_exec_config_urls_testnet() {
        let config = BybitExecutionClientConfig {
            environment: BybitEnvironment::Testnet,
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://api-testnet.bybit.com");
        assert_eq!(
            config.ws_private_url(),
            "wss://stream-testnet.bybit.com/v5/private"
        );
        assert_eq!(
            config.ws_trade_url(),
            "wss://stream-testnet.bybit.com/v5/trade"
        );
    }

    #[rstest]
    fn test_exec_config_custom_urls() {
        let config = BybitExecutionClientConfig {
            base_url_http: Some("https://custom-http.bybit.com".to_string()),
            base_url_ws_private: Some("wss://custom-private.bybit.com".to_string()),
            base_url_ws_trade: Some("wss://custom-trade.bybit.com".to_string()),
            ..Default::default()
        };

        assert_eq!(config.http_base_url(), "https://custom-http.bybit.com");
        assert_eq!(config.ws_private_url(), "wss://custom-private.bybit.com");
        assert_eq!(config.ws_trade_url(), "wss://custom-trade.bybit.com");
    }

    #[rstest]
    fn test_data_config_toml_minimal() {
        let config: BybitDataClientConfig = toml::from_str(
            r#"
environment = "testnet"
product_types = ["spot", "linear"]
http_timeout_secs = 45
"#,
        )
        .unwrap();

        assert_eq!(config.environment, BybitEnvironment::Testnet);
        assert_eq!(
            config.product_types,
            vec![BybitProductType::Spot, BybitProductType::Linear]
        );
        assert_eq!(config.http_timeout_secs, 45);
    }

    #[rstest]
    fn test_exec_config_toml_empty_uses_defaults() {
        let config: BybitExecutionClientConfig = toml::from_str("").unwrap();
        let expected = BybitExecutionClientConfig::default();

        assert_eq!(config.environment, expected.environment);
        assert_eq!(config.product_types, expected.product_types);
        assert_eq!(config.http_timeout_secs, expected.http_timeout_secs);
        assert_eq!(
            config.heartbeat_interval_secs,
            expected.heartbeat_interval_secs,
        );
        assert_eq!(config.recv_window_ms, expected.recv_window_ms);
        assert_eq!(config.transport_backend, expected.transport_backend);
    }

    #[rstest]
    fn test_execution_client_rejects_duplicate_types() {
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("BYBIT"),
            Venue::from("BYBIT"),
            OmsType::Netting,
            AccountId::from("BYBIT-001"),
            AccountType::Margin,
            None,
            Rc::new(RefCell::new(Cache::default())),
        );

        let config = BybitExecutionClientConfig {
            product_types: vec![BybitProductType::Linear, BybitProductType::Linear],
            ..Default::default()
        };

        let error = BybitExecutionClient::new(core, config)
            .expect_err("duplicate config must fail before credentials or client setup");

        assert_eq!(
            error.downcast_ref::<ConfigError>(),
            Some(&ConfigError::duplicate("product_types", None))
        );
    }
}
