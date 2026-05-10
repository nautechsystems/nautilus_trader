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

//! Configuration structures for the Betfair adapter.

use std::any::Any;

use nautilus_common::factories::ClientConfig;
use nautilus_model::{
    identifiers::{AccountId, TraderId},
    types::{Currency, Money},
};

use crate::{
    common::{
        credential::{BetfairCredential, CredentialError},
        parse::parse_betfair_timestamp,
    },
    provider::NavigationFilter,
    stream::config::BetfairStreamConfig,
};

fn parse_currency(code: &str) -> anyhow::Result<Currency> {
    code.parse::<Currency>()
        .map_err(|_| anyhow::anyhow!("Invalid account currency code: {code}"))
}

fn make_min_notional(value: Option<f64>, currency: Currency) -> Option<Money> {
    value.map(|amount| Money::new(amount, currency))
}

fn validate_market_start_time(label: &str, value: &Option<String>) -> anyhow::Result<()> {
    if let Some(value) = value {
        parse_betfair_timestamp(value)
            .map(|_| ())
            .map_err(|e| anyhow::anyhow!("Invalid {label} '{value}': {e}"))?;
    }

    Ok(())
}

fn resolve_credential(
    username: Option<String>,
    password: Option<String>,
    app_key: Option<String>,
) -> anyhow::Result<BetfairCredential> {
    match BetfairCredential::resolve(username, password, app_key) {
        Ok(Some(credential)) => Ok(credential),
        Ok(None) => anyhow::bail!("Missing Betfair credentials in config and environment"),
        Err(e) => Err(match e {
            CredentialError::MissingPassword => anyhow::anyhow!(
                "Invalid Betfair credentials: username provided but password is missing",
            ),
            CredentialError::MissingUsername => anyhow::anyhow!(
                "Invalid Betfair credentials: password or app key provided but username is missing",
            ),
            CredentialError::MissingAppKey => {
                anyhow::anyhow!("Invalid Betfair credentials: app key is missing")
            }
        }),
    }
}

fn build_stream_config(
    stream_host: &Option<String>,
    stream_port: &Option<u16>,
    stream_heartbeat_ms: u64,
    stream_idle_timeout_ms: u64,
    stream_reconnect_delay_initial_ms: u64,
    stream_reconnect_delay_max_ms: u64,
    stream_use_tls: bool,
) -> BetfairStreamConfig {
    let defaults = BetfairStreamConfig::default();

    BetfairStreamConfig {
        host: stream_host.clone().unwrap_or(defaults.host),
        port: stream_port.unwrap_or(defaults.port),
        heartbeat_ms: stream_heartbeat_ms,
        idle_timeout_ms: stream_idle_timeout_ms,
        reconnect_delay_initial_ms: stream_reconnect_delay_initial_ms,
        reconnect_delay_max_ms: stream_reconnect_delay_max_ms,
        use_tls: stream_use_tls,
    }
}

/// Configuration for the Betfair live data client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.betfair")
)]
pub struct BetfairDataConfig {
    /// Account currency code.
    #[builder(default = "GBP".to_string())]
    pub account_currency: String,
    /// Optional Betfair username.
    pub username: Option<String>,
    /// Optional Betfair password.
    pub password: Option<String>,
    /// Optional Betfair application key.
    pub app_key: Option<String>,
    /// Optional proxy URL for HTTP requests.
    pub proxy_url: Option<String>,
    /// General HTTP request rate limit per second.
    #[builder(default = 5)]
    pub request_rate_per_second: u32,
    /// Optional default minimum notional in `account_currency`.
    pub default_min_notional: Option<f64>,
    /// Optional event type ID filter.
    pub event_type_ids: Option<Vec<String>>,
    /// Optional event type name filter.
    pub event_type_names: Option<Vec<String>>,
    /// Optional event ID filter.
    pub event_ids: Option<Vec<String>>,
    /// Optional country code filter.
    pub country_codes: Option<Vec<String>>,
    /// Optional market type filter.
    pub market_types: Option<Vec<String>>,
    /// Optional market ID filter.
    pub market_ids: Option<Vec<String>>,
    /// Optional lower bound for market start time.
    pub min_market_start_time: Option<String>,
    /// Optional upper bound for market start time.
    pub max_market_start_time: Option<String>,
    /// Optional override for stream host.
    pub stream_host: Option<String>,
    /// Optional override for stream port.
    pub stream_port: Option<u16>,
    /// Interval between stream heartbeat messages in milliseconds.
    #[builder(default = 5_000)]
    pub stream_heartbeat_ms: u64,
    /// Stream idle timeout in milliseconds.
    #[builder(default = 60_000)]
    pub stream_idle_timeout_ms: u64,
    /// Initial reconnection backoff in milliseconds.
    #[builder(default = 2_000)]
    pub stream_reconnect_delay_initial_ms: u64,
    /// Maximum reconnection backoff in milliseconds.
    #[builder(default = 30_000)]
    pub stream_reconnect_delay_max_ms: u64,
    /// Whether to use TLS for the stream connection.
    #[builder(default = true)]
    pub stream_use_tls: bool,
    /// Stream conflation setting in milliseconds. When set, Betfair batches
    /// stream updates for this interval. `None` uses Betfair defaults.
    pub stream_conflate_ms: Option<u64>,
    /// Delay in seconds before sending the initial subscription message after connecting.
    #[builder(default = 3)]
    pub subscription_delay_secs: u64,
    /// Subscribe to the race stream for Total Performance Data (TPD).
    #[builder(default)]
    pub subscribe_race_data: bool,
}

impl Default for BetfairDataConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl ClientConfig for BetfairDataConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl BetfairDataConfig {
    /// Returns the configured credentials or resolves them from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are incomplete or unavailable.
    pub fn credential(&self) -> anyhow::Result<BetfairCredential> {
        resolve_credential(
            self.username.clone(),
            self.password.clone(),
            self.app_key.clone(),
        )
    }

    /// Returns the configured account currency.
    ///
    /// # Errors
    ///
    /// Returns an error if the currency code is invalid.
    pub fn currency(&self) -> anyhow::Result<Currency> {
        parse_currency(&self.account_currency)
    }

    /// Returns the default instrument minimum notional.
    ///
    /// # Errors
    ///
    /// Returns an error if the account currency code is invalid.
    pub fn min_notional(&self) -> anyhow::Result<Option<Money>> {
        let currency = self.currency()?;
        Ok(make_min_notional(self.default_min_notional, currency))
    }

    /// Returns the navigation filter for instrument loading.
    #[must_use]
    pub fn navigation_filter(&self) -> NavigationFilter {
        NavigationFilter {
            event_type_ids: self.event_type_ids.clone(),
            event_type_names: self.event_type_names.clone(),
            event_ids: self.event_ids.clone(),
            country_codes: self.country_codes.clone(),
            market_types: self.market_types.clone(),
            market_ids: self.market_ids.clone(),
            min_market_start_time: self.min_market_start_time.clone(),
            max_market_start_time: self.max_market_start_time.clone(),
        }
    }

    /// Returns the stream configuration.
    #[must_use]
    pub fn stream_config(&self) -> BetfairStreamConfig {
        build_stream_config(
            &self.stream_host,
            &self.stream_port,
            self.stream_heartbeat_ms,
            self.stream_idle_timeout_ms,
            self.stream_reconnect_delay_initial_ms,
            self.stream_reconnect_delay_max_ms,
            self.stream_use_tls,
        )
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if any configured value is invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        let _ = self.currency()?;
        validate_market_start_time("min_market_start_time", &self.min_market_start_time)?;
        validate_market_start_time("max_market_start_time", &self.max_market_start_time)?;

        if self.request_rate_per_second == 0 {
            anyhow::bail!("request_rate_per_second must be greater than zero");
        }

        Ok(())
    }
}

/// Configuration for the Betfair live execution client.
#[derive(Clone, Debug, bon::Builder)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.betfair", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.betfair")
)]
pub struct BetfairExecConfig {
    /// Trader ID for the client core.
    #[builder(default = TraderId::from("TRADER-001"))]
    pub trader_id: TraderId,
    /// Account ID for the client core.
    #[builder(default = AccountId::from("BETFAIR-001"))]
    pub account_id: AccountId,
    /// Account currency code.
    #[builder(default = "GBP".to_string())]
    pub account_currency: String,
    /// Optional Betfair username.
    pub username: Option<String>,
    /// Optional Betfair password.
    pub password: Option<String>,
    /// Optional Betfair application key.
    pub app_key: Option<String>,
    /// Optional proxy URL for HTTP requests.
    pub proxy_url: Option<String>,
    /// General HTTP request rate limit per second.
    #[builder(default = 5)]
    pub request_rate_per_second: u32,
    /// Order HTTP request rate limit per second.
    #[builder(default = 20)]
    pub order_request_rate_per_second: u32,
    /// Optional override for stream host.
    pub stream_host: Option<String>,
    /// Optional override for stream port.
    pub stream_port: Option<u16>,
    /// Interval between stream heartbeat messages in milliseconds.
    #[builder(default = 5_000)]
    pub stream_heartbeat_ms: u64,
    /// Stream idle timeout in milliseconds.
    #[builder(default = 60_000)]
    pub stream_idle_timeout_ms: u64,
    /// Initial reconnection backoff in milliseconds.
    #[builder(default = 2_000)]
    pub stream_reconnect_delay_initial_ms: u64,
    /// Maximum reconnection backoff in milliseconds.
    #[builder(default = 30_000)]
    pub stream_reconnect_delay_max_ms: u64,
    /// Whether to use TLS for the stream connection.
    #[builder(default = true)]
    pub stream_use_tls: bool,
    /// Market IDs to filter on the order stream. When set, OCM updates for
    /// markets not in this list are skipped. `None` processes all markets.
    pub stream_market_ids_filter: Option<Vec<String>>,
    /// When true, silently ignore orders from OCM that are not tracked in the local cache.
    #[builder(default)]
    pub ignore_external_orders: bool,
    /// Whether to poll account state periodically.
    #[builder(default = true)]
    pub calculate_account_state: bool,
    /// Interval in seconds between account state polls.
    #[builder(default = 300)]
    pub request_account_state_secs: u64,
    /// When true, reconciliation only requests orders matching `reconcile_market_ids`.
    #[builder(default)]
    pub reconcile_market_ids_only: bool,
    /// Market IDs to restrict reconciliation to.
    pub reconcile_market_ids: Option<Vec<String>>,
    /// When true, attach the latest market version to placeOrders and replaceOrders requests.
    #[builder(default)]
    pub use_market_version: bool,
}

impl Default for BetfairExecConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl ClientConfig for BetfairExecConfig {
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl BetfairExecConfig {
    /// Returns the configured credentials or resolves them from the environment.
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are incomplete or unavailable.
    pub fn credential(&self) -> anyhow::Result<BetfairCredential> {
        resolve_credential(
            self.username.clone(),
            self.password.clone(),
            self.app_key.clone(),
        )
    }

    /// Returns the configured account currency.
    ///
    /// # Errors
    ///
    /// Returns an error if the currency code is invalid.
    pub fn currency(&self) -> anyhow::Result<Currency> {
        parse_currency(&self.account_currency)
    }

    /// Returns the stream configuration.
    #[must_use]
    pub fn stream_config(&self) -> BetfairStreamConfig {
        build_stream_config(
            &self.stream_host,
            &self.stream_port,
            self.stream_heartbeat_ms,
            self.stream_idle_timeout_ms,
            self.stream_reconnect_delay_initial_ms,
            self.stream_reconnect_delay_max_ms,
            self.stream_use_tls,
        )
    }

    /// Validates the configuration.
    ///
    /// # Errors
    ///
    /// Returns an error if any configured value is invalid.
    pub fn validate(&self) -> anyhow::Result<()> {
        let _ = self.currency()?;

        if self.request_rate_per_second == 0 {
            anyhow::bail!("request_rate_per_second must be greater than zero");
        }

        if self.order_request_rate_per_second == 0 {
            anyhow::bail!("order_request_rate_per_second must be greater than zero");
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_data_config_default() {
        let config = BetfairDataConfig::default();

        assert_eq!(config.account_currency, "GBP");
        assert_eq!(config.request_rate_per_second, 5);
        assert!(config.market_ids.is_none());
        assert_eq!(config.stream_heartbeat_ms, 5_000);
        assert!(config.stream_conflate_ms.is_none());
        assert_eq!(config.subscription_delay_secs, 3);
        assert!(!config.subscribe_race_data);
    }

    #[rstest]
    fn test_data_config_navigation_filter() {
        let config = BetfairDataConfig {
            event_type_names: Some(vec!["Horse Racing".to_string()]),
            market_ids: Some(vec!["1.234567".to_string()]),
            ..Default::default()
        };

        let filter = config.navigation_filter();

        assert_eq!(
            filter.event_type_names,
            Some(vec!["Horse Racing".to_string()])
        );
        assert_eq!(filter.market_ids, Some(vec!["1.234567".to_string()]));
    }

    #[rstest]
    fn test_data_config_stream_config() {
        let config = BetfairDataConfig {
            stream_host: Some("localhost".to_string()),
            stream_port: Some(9443),
            stream_heartbeat_ms: 2_500,
            stream_idle_timeout_ms: 30_000,
            stream_reconnect_delay_initial_ms: 500,
            stream_reconnect_delay_max_ms: 5_000,
            stream_use_tls: false,
            ..Default::default()
        };

        let stream_config = config.stream_config();

        assert_eq!(stream_config.host, "localhost");
        assert_eq!(stream_config.port, 9443);
        assert_eq!(stream_config.heartbeat_ms, 2_500);
        assert_eq!(stream_config.idle_timeout_ms, 30_000);
        assert_eq!(stream_config.reconnect_delay_initial_ms, 500);
        assert_eq!(stream_config.reconnect_delay_max_ms, 5_000);
        assert!(!stream_config.use_tls);
    }

    #[rstest]
    fn test_data_config_stream_config_uses_defaults() {
        let config = BetfairDataConfig::default();

        let stream_config = config.stream_config();

        assert_eq!(stream_config.host, BetfairStreamConfig::default().host);
        assert_eq!(stream_config.port, BetfairStreamConfig::default().port);
    }

    #[rstest]
    fn test_data_config_credential_rejects_partial_credentials() {
        let config = BetfairDataConfig {
            username: Some("testuser".to_string()),
            ..Default::default()
        };

        let result = config.credential();

        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("password is missing")
        );
    }

    #[rstest]
    fn test_exec_config_default() {
        let config = BetfairExecConfig::default();

        assert_eq!(config.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(config.account_id, AccountId::from("BETFAIR-001"));
        assert_eq!(config.account_currency, "GBP");
        assert_eq!(config.request_rate_per_second, 5);
        assert_eq!(config.order_request_rate_per_second, 20);
        assert!(config.stream_market_ids_filter.is_none());
        assert!(!config.ignore_external_orders);
        assert!(config.calculate_account_state);
        assert_eq!(config.request_account_state_secs, 300);
        assert!(!config.reconcile_market_ids_only);
        assert!(config.reconcile_market_ids.is_none());
        assert!(!config.use_market_version);
    }

    #[rstest]
    fn test_exec_config_with_market_filter() {
        let config = BetfairExecConfig {
            stream_market_ids_filter: Some(vec!["1.234567".to_string(), "1.890123".to_string()]),
            ..Default::default()
        };

        let filter = config.stream_market_ids_filter.as_ref().unwrap();
        assert_eq!(filter.len(), 2);
        assert!(filter.contains(&"1.234567".to_string()));
    }

    #[rstest]
    fn test_exec_config_external_orders_ignored() {
        let config = BetfairExecConfig {
            ignore_external_orders: true,
            ..Default::default()
        };

        assert!(config.ignore_external_orders);
    }

    #[rstest]
    fn test_exec_config_account_state_disabled() {
        let config = BetfairExecConfig {
            calculate_account_state: false,
            ..Default::default()
        };

        assert!(!config.calculate_account_state);
    }

    #[rstest]
    fn test_exec_config_reconcile_market_ids() {
        let config = BetfairExecConfig {
            reconcile_market_ids_only: true,
            reconcile_market_ids: Some(vec!["1.234567".to_string()]),
            ..Default::default()
        };

        assert!(config.reconcile_market_ids_only);
        assert_eq!(config.reconcile_market_ids.as_ref().unwrap().len(), 1);
    }

    #[rstest]
    fn test_exec_config_use_market_version() {
        let config = BetfairExecConfig {
            use_market_version: true,
            ..Default::default()
        };

        assert!(config.use_market_version);
    }

    #[rstest]
    fn test_exec_config_validate_rejects_zero_order_rate_limit() {
        let config = BetfairExecConfig {
            order_request_rate_per_second: 0,
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("order_request_rate_per_second")
        );
    }

    #[rstest]
    fn test_exec_config_validate_rejects_invalid_currency() {
        let config = BetfairExecConfig {
            account_currency: "INVALID".to_string(),
            ..Default::default()
        };

        let result = config.validate();

        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("Invalid account currency")
        );
    }

    #[rstest]
    fn test_data_config_validate_rejects_bad_market_start_time() {
        let config = BetfairDataConfig {
            min_market_start_time: Some("not-a-timestamp".to_string()),
            ..Default::default()
        };

        let result = config.validate();
        assert!(result.is_err());
        assert!(
            result
                .err()
                .unwrap()
                .to_string()
                .contains("min_market_start_time")
        );
    }

    #[rstest]
    fn test_data_config_min_notional() {
        let config = BetfairDataConfig {
            default_min_notional: Some(2.0),
            ..Default::default()
        };

        let min_notional = config.min_notional().unwrap();
        assert_eq!(min_notional, Some(Money::new(2.0, Currency::GBP())));
    }
}
