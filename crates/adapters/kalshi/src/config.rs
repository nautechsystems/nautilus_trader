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

//! Configuration for the Kalshi data and execution clients.

use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::AccountId;
use serde::{Deserialize, Serialize};

use crate::common::{
    consts::KALSHI_ACCOUNT_ID,
    enums::{KalshiEnvironment, KalshiSelfTradePrevention},
};

/// The interval between order polls used when the configuration does not set one.
const DEFAULT_POLL_INTERVAL_MILLIS: u64 = 2_000;

/// Configuration for the Kalshi data client.
#[derive(Clone, Debug, bon::Builder, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiDataClientConfig {
    /// The API environment to connect to.
    #[builder(default)]
    pub environment: KalshiEnvironment,
    /// The REST base URL, which overrides the environment's endpoint when set.
    pub base_url: Option<String>,
    /// The API key ID, which falls back to the `KALSHI_API_KEY_ID` environment variable.
    pub api_key_id: Option<String>,
    /// The API key's PEM-encoded RSA private key, which falls back to `KALSHI_API_KEY_PEM`.
    pub api_key_pem: Option<SecretString>,
    /// The request timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// The proxy URL for HTTP requests.
    pub proxy_url: Option<SecretString>,
    /// Event tickers to load instruments for. Empty loads every open market.
    #[builder(default)]
    pub event_tickers: Vec<String>,
    /// A series ticker to load instruments for.
    pub series_ticker: Option<String>,
    /// The interval in milliseconds between market polls.
    ///
    /// Kalshi publishes market data over REST, so a faster poll trades request rate for freshness.
    /// Defaults to 2000 milliseconds.
    pub poll_interval_millis: Option<u64>,
}

impl Default for KalshiDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KalshiDataClientConfig {
    /// Creates a new [`KalshiDataClientConfig`] with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the REST base URL, which the configuration overrides when set.
    #[must_use]
    pub fn base_url(&self) -> String {
        match &self.base_url {
            Some(base_url) => base_url.clone(),
            None => self.environment.rest_url().to_string(),
        }
    }
}

/// Configuration for the Kalshi execution client.
#[derive(Clone, Debug, bon::Builder, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        name = "KalshiExecutionClientConfig",
        module = "nautilus_trader.adapters.kalshi",
        from_py_object
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiExecClientConfig {
    /// The API environment to connect to.
    #[builder(default)]
    pub environment: KalshiEnvironment,
    /// The REST base URL, which overrides the environment's endpoint when set.
    pub base_url: Option<String>,
    /// The API key ID, which falls back to the `KALSHI_API_KEY_ID` environment variable.
    pub api_key_id: Option<String>,
    /// The API key's PEM-encoded RSA private key, which falls back to `KALSHI_API_KEY_PEM`.
    pub api_key_pem: Option<SecretString>,
    /// The request timeout in seconds.
    pub http_timeout_secs: Option<u64>,
    /// The proxy URL for HTTP requests.
    pub proxy_url: Option<SecretString>,
    /// Whether to reconcile open orders and positions on start.
    #[builder(default = true)]
    pub reconciliation: bool,
    /// The account identifier the execution client reports under.
    #[builder(default = AccountId::from(KALSHI_ACCOUNT_ID))]
    pub account_id: AccountId,
    /// The self-trade prevention the exchange applies to submitted orders.
    #[builder(default)]
    pub self_trade_prevention: KalshiSelfTradePrevention,
    /// Whether the exchange cancels an order when trading pauses. Unset uses the exchange default.
    pub cancel_order_on_pause: Option<bool>,
    /// The interval in milliseconds between order polls.
    ///
    /// Kalshi reports order state over REST, so a faster poll trades request rate for latency.
    /// Defaults to 2000 milliseconds.
    pub poll_interval_millis: Option<u64>,
}

impl Default for KalshiExecClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KalshiExecClientConfig {
    /// Creates a new [`KalshiExecClientConfig`] with default values.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns the REST base URL, which the configuration overrides when set.
    #[must_use]
    pub fn base_url(&self) -> String {
        match &self.base_url {
            Some(base_url) => base_url.clone(),
            None => self.environment.rest_url().to_string(),
        }
    }

    /// Returns the interval between order polls.
    #[must_use]
    pub fn poll_interval(&self) -> std::time::Duration {
        std::time::Duration::from_millis(
            self.poll_interval_millis
                .unwrap_or(DEFAULT_POLL_INTERVAL_MILLIS),
        )
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::common::urls;

    #[rstest]
    fn test_configs_default_to_the_demo_exchange() {
        let data = KalshiDataClientConfig::new();
        let exec = KalshiExecClientConfig::new();

        assert_eq!(data.environment, KalshiEnvironment::Demo);
        assert_eq!(data.base_url(), urls::DEMO_REST_URL);
        assert_eq!(exec.base_url(), urls::DEMO_REST_URL);
        assert!(exec.reconciliation);
        assert!(data.event_tickers.is_empty());
    }

    #[rstest]
    fn test_exec_config_defaults_are_explicit() {
        let exec = KalshiExecClientConfig::new();

        assert_eq!(exec.account_id, AccountId::from(KALSHI_ACCOUNT_ID));
        assert_eq!(
            exec.self_trade_prevention,
            KalshiSelfTradePrevention::TakerAtCross
        );
        assert!(exec.cancel_order_on_pause.is_none());
        assert_eq!(exec.poll_interval().as_millis(), 2_000);
    }

    #[rstest]
    fn test_exec_config_honors_the_configured_poll_interval() {
        let exec = KalshiExecClientConfig::builder()
            .poll_interval_millis(250)
            .build();

        assert_eq!(exec.poll_interval().as_millis(), 250);
    }

    #[rstest]
    fn test_production_requires_an_explicit_environment() {
        let config = KalshiDataClientConfig::builder()
            .environment(KalshiEnvironment::Prod)
            .build();

        assert_eq!(config.base_url(), urls::PROD_REST_URL);
        assert!(config.environment.is_production());
    }

    #[rstest]
    fn test_config_deserializes_from_json_and_rejects_unknown_keys() {
        let config: KalshiExecClientConfig =
            serde_json::from_str(r#"{"environment": "prod", "reconciliation": false}"#).unwrap();

        assert_eq!(config.environment, KalshiEnvironment::Prod);
        assert!(!config.reconciliation);

        let error =
            serde_json::from_str::<KalshiExecClientConfig>(r#"{"nope": true}"#).unwrap_err();

        assert!(error.to_string().contains("unknown field"), "{error}");
    }

    #[rstest]
    fn test_config_debug_does_not_expose_the_private_key() {
        let config = KalshiDataClientConfig::builder()
            .api_key_pem(SecretString::from("super-secret-pem"))
            .build();

        let debug = format!("{config:?}");

        assert!(!debug.contains("super-secret-pem"), "{debug}");
    }
}
