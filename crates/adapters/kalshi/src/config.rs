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

//! Configuration structures for the Kalshi adapter.

use nautilus_common::{
    config::{ConfigError, ConfigResult, check_unique_values},
    live::dst::time::{Duration, Instant},
};
use nautilus_core::string::secret::SecretString;
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::identifiers::{AccountId, InstrumentId};
use nautilus_network::websocket::TransportBackend;
use serde::{Deserialize, Serialize};
use url::Url;

use crate::{
    common::{
        consts::MAX_SUBACCOUNT,
        credential::validate_credentials,
        enums::KalshiEnvironment,
        parse::parse_ticker,
        urls::{kalshi_http_base_url, kalshi_ws_url},
    },
    http::query::AccountQuery,
};

/// Configuration for the Kalshi data client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader._libnautilus.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiDataClientConfig {
    /// Kalshi API key ID; supply with `private_key` or leave both absent.
    pub api_key_id: Option<SecretString>,
    /// PEM-encoded RSA-PSS or Ed25519 private key.
    pub private_key: Option<SecretString>,
    /// Target API and credential environment.
    #[builder(default)]
    pub environment: KalshiEnvironment,
    /// Optional REST base URL override.
    pub base_url_http: Option<SecretString>,
    /// Optional WebSocket URL override.
    pub base_url_ws: Option<SecretString>,
    /// Optional HTTP/HTTPS proxy URL.
    pub proxy_url: Option<SecretString>,
    /// Positive HTTP request timeout in seconds within the transport clock range.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Positive WebSocket connection timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Snapshot deadline in seconds; zero disables snapshot deadlines.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// Positive instrument metadata refresh interval in minutes.
    #[builder(default = 60)]
    pub update_instruments_interval_mins: u64,
    /// WebSocket transport backend; only Sockudo is supported.
    #[builder(default = TransportBackend::Sockudo)]
    pub transport_backend: TransportBackend,
    /// Bootstrap markets. Empty is valid for transport-only configuration; the data factory
    /// requires at least one distinct Kalshi instrument ID.
    #[builder(default)]
    pub instrument_ids: Vec<InstrumentId>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(KalshiDataClientConfig {
    environment: KalshiEnvironment,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    update_instruments_interval_mins: u64,
    book_snapshot_timeout_secs: u64,
    transport_backend: TransportBackend,
    instrument_ids: Vec<InstrumentId>,
});

impl Default for KalshiDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KalshiDataClientConfig {
    /// Creates a configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether both explicit credential fields contain non-empty values.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.api_key_id
            .as_ref()
            .is_some_and(|s| !s.expose_secret().trim().is_empty())
            && self
                .private_key
                .as_ref()
                .is_some_and(|s| !s.expose_secret().trim().is_empty())
    }

    /// Returns the REST base URL, with an explicit override taking precedence.
    #[must_use]
    pub fn http_url(&self) -> &str {
        self.base_url_http.as_ref().map_or_else(
            || kalshi_http_base_url(self.environment),
            SecretString::expose_secret,
        )
    }

    /// Returns the WebSocket URL, with an explicit override taking precedence.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        self.base_url_ws.as_ref().map_or_else(
            || kalshi_ws_url(self.environment),
            SecretString::expose_secret,
        )
    }

    /// Validates explicit credentials, endpoints, and supported timing values.
    ///
    /// This does not read credentials from the environment or validate key algorithms.
    ///
    /// # Errors
    ///
    /// Returns an error if credential fields are incomplete or invalid, URLs select the wrong
    /// venue environment or have an invalid format, the backend is unsupported, or timing values
    /// require positive values or exceed the supported range.
    pub fn validate(&self) -> ConfigResult<()> {
        validate_credentials(self.api_key_id.as_ref(), self.private_key.as_ref())?;
        validate_transport(
            self.environment,
            self.http_url(),
            self.ws_url(),
            self.proxy_url.as_ref(),
            self.http_timeout_secs,
            self.ws_timeout_secs,
            self.transport_backend,
        )?;

        if Instant::now()
            .checked_add(Duration::from_secs(self.book_snapshot_timeout_secs))
            .is_none()
        {
            return Err(ConfigError::invalid_value(
                "book_snapshot_timeout_secs",
                "exceeds the supported clock range",
            ));
        }

        validate_positive(
            "update_instruments_interval_mins",
            self.update_instruments_interval_mins,
        )?;
        validate_instrument_ids(&self.instrument_ids)?;

        if self
            .update_instruments_interval_mins
            .checked_mul(60)
            .is_none()
        {
            return Err(ConfigError::invalid_value(
                "update_instruments_interval_mins",
                "seconds overflow",
            ));
        }

        Ok(())
    }
}

/// Configuration for the Kalshi execution client.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader._libnautilus.kalshi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.kalshi")
)]
pub struct KalshiExecutionClientConfig {
    /// Nautilus account ID; use a distinct value for each subaccount and shard client in the runtime.
    #[builder(default = AccountId::from("KALSHI-001"))]
    pub account_id: AccountId,
    /// Kalshi API key ID; supply with `private_key` or leave both absent.
    pub api_key_id: Option<SecretString>,
    /// PEM-encoded RSA-PSS or Ed25519 private key.
    pub private_key: Option<SecretString>,
    /// Target API and credential environment.
    #[builder(default)]
    pub environment: KalshiEnvironment,
    /// Optional REST base URL override.
    pub base_url_http: Option<SecretString>,
    /// Optional WebSocket URL override.
    pub base_url_ws: Option<SecretString>,
    /// Optional HTTP/HTTPS proxy URL.
    pub proxy_url: Option<SecretString>,
    /// Positive HTTP request timeout in seconds within the transport clock range.
    #[builder(default = 60)]
    pub http_timeout_secs: u64,
    /// Positive WebSocket connection timeout in seconds.
    #[builder(default = 30)]
    pub ws_timeout_secs: u64,
    /// Explicit exchange shard owned by this execution client.
    #[builder(default)]
    pub exchange_index: u32,
    /// WebSocket transport backend; only Sockudo is supported.
    #[builder(default = TransportBackend::Sockudo)]
    pub transport_backend: TransportBackend,
    /// Selected subaccount in [0, 63]; zero selects the primary account.
    #[builder(default)]
    pub subaccount: u32,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(KalshiExecutionClientConfig {
    account_id: AccountId,
    environment: KalshiEnvironment,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    exchange_index: u32,
    subaccount: u32,
    transport_backend: TransportBackend,
});

impl Default for KalshiExecutionClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl KalshiExecutionClientConfig {
    /// Creates a configuration with default settings.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns whether both explicit credential fields contain non-empty values.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.api_key_id
            .as_ref()
            .is_some_and(|s| !s.expose_secret().trim().is_empty())
            && self
                .private_key
                .as_ref()
                .is_some_and(|s| !s.expose_secret().trim().is_empty())
    }

    /// Returns the REST base URL, with an explicit override taking precedence.
    #[must_use]
    pub fn http_url(&self) -> &str {
        self.base_url_http.as_ref().map_or_else(
            || kalshi_http_base_url(self.environment),
            SecretString::expose_secret,
        )
    }

    /// Returns the WebSocket URL, with an explicit override taking precedence.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        self.base_url_ws.as_ref().map_or_else(
            || kalshi_ws_url(self.environment),
            SecretString::expose_secret,
        )
    }

    /// Validates the subaccount, explicit credentials, endpoints, and positive timing values.
    ///
    /// This does not read credentials from the environment or validate key algorithms.
    ///
    /// # Errors
    ///
    /// Returns an error if credential fields are incomplete or invalid, URLs select the wrong
    /// venue environment or have an invalid format, the backend is unsupported, timing values
    /// are zero or overflow, or the subaccount is outside `[0, 63]`.
    pub fn validate(&self) -> ConfigResult<()> {
        if self.subaccount > MAX_SUBACCOUNT {
            return Err(ConfigError::invalid_value(
                "subaccount",
                "must be in [0, 63]",
            ));
        }

        validate_credentials(self.api_key_id.as_ref(), self.private_key.as_ref())?;
        validate_transport(
            self.environment,
            self.http_url(),
            self.ws_url(),
            self.proxy_url.as_ref(),
            self.http_timeout_secs,
            self.ws_timeout_secs,
            self.transport_backend,
        )?;

        Ok(())
    }

    pub(crate) const fn account_query(&self) -> AccountQuery {
        AccountQuery {
            subaccount: self.subaccount,
            exchange_index: self.exchange_index,
        }
    }
}

fn validate_transport(
    environment: KalshiEnvironment,
    http_url: &str,
    ws_url: &str,
    proxy_url: Option<&SecretString>,
    http_timeout_secs: u64,
    ws_timeout_secs: u64,
    transport_backend: TransportBackend,
) -> ConfigResult<()> {
    if transport_backend != TransportBackend::Sockudo {
        return Err(ConfigError::invalid_value(
            "transport_backend",
            "Kalshi requires the Sockudo WebSocket backend",
        ));
    }

    validate_endpoint("base_url_http", http_url, &["http", "https"], environment)?;
    validate_endpoint("base_url_ws", ws_url, &["ws", "wss"], environment)?;

    if let Some(proxy) = proxy_url {
        let url = Url::parse(proxy.expose_secret()).map_err(|_| {
            ConfigError::invalid_value("proxy_url", "expected an HTTP/HTTPS proxy URL")
        })?;

        let is_valid = matches!(url.scheme(), "http" | "https") && url.host_str().is_some();
        // URL parsing duplicates potentially credential-bearing input
        let serialization = String::from(url);
        let _serialization = SecretString::from(serialization);

        if !is_valid {
            return Err(ConfigError::invalid_value(
                "proxy_url",
                "expected an HTTP/HTTPS proxy URL",
            ));
        }
    }

    validate_positive("http_timeout_secs", http_timeout_secs)?;

    if Instant::now()
        .checked_add(Duration::from_secs(http_timeout_secs))
        .is_none()
    {
        return Err(ConfigError::invalid_value(
            "http_timeout_secs",
            "exceeds the supported clock range",
        ));
    }

    validate_positive("ws_timeout_secs", ws_timeout_secs)?;

    if ws_timeout_secs.checked_mul(1_000).is_none() {
        return Err(ConfigError::invalid_value(
            "ws_timeout_secs",
            "milliseconds overflow",
        ));
    }

    Ok(())
}

fn validate_endpoint(
    field: &str,
    value: &str,
    schemes: &[&str],
    environment: KalshiEnvironment,
) -> ConfigResult<()> {
    let url = Url::parse(value)
        .map_err(|_| ConfigError::invalid_value(field, "expected an absolute transport URL"))?;

    let is_invalid = !schemes.contains(&url.scheme())
        || url.host_str().is_none()
        || !url.username().is_empty()
        || url.password().is_some()
        || url.query().is_some()
        || url.fragment().is_some();

    let host = url.host_str().unwrap_or_default();

    let venue_environment = match host.strip_suffix('.').unwrap_or(host) {
        "external-api.kalshi.com" | "external-api-ws.kalshi.com" | "api.elections.kalshi.com" => {
            Some(KalshiEnvironment::Production)
        }
        "external-api.demo.kalshi.co" | "external-api-ws.demo.kalshi.co" | "demo-api.kalshi.co" => {
            Some(KalshiEnvironment::Demo)
        }
        _ => None,
    };

    let is_insecure_venue = venue_environment.is_some() && !matches!(url.scheme(), "https" | "wss");

    // URL parsing duplicates potentially credential-bearing input
    let serialization = String::from(url);
    let _serialization = SecretString::from(serialization);

    if is_invalid {
        return Err(ConfigError::invalid_value(
            field,
            "expected an absolute transport URL without user information, query, or fragment",
        ));
    }

    if is_insecure_venue {
        return Err(ConfigError::invalid_value(
            field,
            "known venue hosts require an encrypted transport",
        ));
    }

    if venue_environment.is_some_and(|selected| selected != environment) {
        return Err(ConfigError::invalid_value(
            field,
            "venue host does not match environment",
        ));
    }

    Ok(())
}

fn validate_instrument_ids(ids: &[InstrumentId]) -> ConfigResult<()> {
    check_unique_values("instrument_ids", ids)?;

    for id in ids {
        if let Err(e) = parse_ticker(*id) {
            return Err(ConfigError::invalid_value("instrument_ids", e.to_string()));
        }
    }

    Ok(())
}

fn validate_positive(field: &str, value: u64) -> ConfigResult<()> {
    if value == 0 {
        return Err(ConfigError::invalid_value(field, "must be positive"));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(0, true)]
    #[case(7, true)]
    #[case(63, true)]
    #[case(64, false)]
    #[case(u32::MAX, false)]
    fn test_subaccount_scope(#[case] subaccount: u32, #[case] valid: bool) {
        let config = KalshiExecutionClientConfig {
            subaccount,
            exchange_index: 3,
            ..Default::default()
        };

        assert_eq!(config.validate().is_ok(), valid);
        let decoded: KalshiExecutionClientConfig =
            serde_json::from_value(serde_json::to_value(&config).unwrap()).unwrap();
        assert_eq!(decoded, config);
        assert_eq!(config.account_query().subaccount, subaccount);
        assert_eq!(config.account_query().exchange_index, 3);
    }

    #[rstest]
    fn test_defaults_have_one_source() {
        let data = KalshiDataClientConfig::default();
        let execution = KalshiExecutionClientConfig::default();
        assert_eq!(data, KalshiDataClientConfig::builder().build());
        assert_eq!(data, KalshiDataClientConfig::new());
        assert_eq!(data, toml::from_str("").unwrap());
        assert_eq!(execution, KalshiExecutionClientConfig::builder().build());
        assert_eq!(execution, KalshiExecutionClientConfig::new());
        assert_eq!(execution, toml::from_str("").unwrap());
        assert_eq!(
            data,
            KalshiDataClientConfig {
                api_key_id: None,
                private_key: None,
                environment: KalshiEnvironment::Production,
                base_url_http: None,
                base_url_ws: None,
                proxy_url: None,
                http_timeout_secs: 60,
                ws_timeout_secs: 30,
                update_instruments_interval_mins: 60,
                book_snapshot_timeout_secs: DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS,
                transport_backend: TransportBackend::Sockudo,
                instrument_ids: Vec::new(),
            }
        );
        assert_eq!(
            execution,
            KalshiExecutionClientConfig {
                account_id: AccountId::from("KALSHI-001"),
                api_key_id: None,
                private_key: None,
                environment: KalshiEnvironment::Production,
                base_url_http: None,
                base_url_ws: None,
                proxy_url: None,
                http_timeout_secs: 60,
                ws_timeout_secs: 30,
                exchange_index: 0,
                subaccount: 0,
                transport_backend: TransportBackend::Sockudo,
            }
        );
        assert!(!data.has_credentials());
        assert!(!execution.has_credentials());
        assert_eq!(data.validate(), Ok(()));
        assert_eq!(execution.validate(), Ok(()));
    }

    #[rstest]
    fn test_instrument_ids_reject_duplicates_and_foreign_venues() {
        let duplicate = KalshiDataClientConfig {
            instrument_ids: vec![
                InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI"),
                InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI"),
            ],
            ..Default::default()
        };

        let foreign = KalshiDataClientConfig {
            instrument_ids: vec![InstrumentId::from("BTC-USD.BINANCE")],
            ..Default::default()
        };

        let selected = KalshiDataClientConfig {
            instrument_ids: vec![InstrumentId::from("KXHIGHNY-26OCT31-T70.KALSHI")],
            ..Default::default()
        };

        assert_eq!(
            duplicate.validate(),
            Err(ConfigError::duplicate("instrument_ids", None))
        );
        assert_eq!(
            foreign.validate(),
            Err(ConfigError::invalid_value(
                "instrument_ids",
                "instrument venue must be KALSHI",
            ))
        );
        assert_eq!(selected.validate(), Ok(()));
        assert_eq!(KalshiDataClientConfig::default().validate(), Ok(()));
    }

    #[rstest]
    fn test_tungstenite_backend_is_rejected() {
        let data = KalshiDataClientConfig {
            transport_backend: TransportBackend::Tungstenite,
            ..Default::default()
        };

        let execution = KalshiExecutionClientConfig {
            transport_backend: TransportBackend::Tungstenite,
            ..Default::default()
        };

        let expected = ConfigError::invalid_value(
            "transport_backend",
            "Kalshi requires the Sockudo WebSocket backend",
        );

        assert_eq!(data.validate(), Err(expected.clone()));
        assert_eq!(execution.validate(), Err(expected));
    }

    #[rstest]
    fn test_decode_all_fields_and_round_trip() {
        let data: KalshiDataClientConfig =
            serde_json::from_str(include_str!("../test_data/config_data.json")).unwrap();
        let execution: KalshiExecutionClientConfig =
            serde_json::from_str(include_str!("../test_data/config_execution.json")).unwrap();
        let expected_data = KalshiDataClientConfig::builder()
            .api_key_id(SecretString::from("data-key"))
            .private_key(SecretString::from("data-private-key"))
            .environment(KalshiEnvironment::Demo)
            .base_url_http(SecretString::from("http://localhost:14001/custom-api"))
            .base_url_ws(SecretString::from("ws://localhost:14002/custom-ws"))
            .proxy_url(SecretString::from(
                "http://proxy-user:data-password@localhost:14003",
            ))
            .http_timeout_secs(17)
            .ws_timeout_secs(19)
            .update_instruments_interval_mins(23)
            .book_snapshot_timeout_secs(31)
            .transport_backend(TransportBackend::Sockudo)
            .instrument_ids(vec![InstrumentId::from("KXBTC15M-26OCT31-T100000.KALSHI")])
            .build();
        let expected_execution = KalshiExecutionClientConfig::builder()
            .account_id(AccountId::from("KALSHI-TEST"))
            .api_key_id(SecretString::from("execution-key"))
            .private_key(SecretString::from("execution-private-key"))
            .environment(KalshiEnvironment::Demo)
            .base_url_http(SecretString::from("http://localhost:14001/custom-api"))
            .base_url_ws(SecretString::from("ws://localhost:14002/custom-ws"))
            .proxy_url(SecretString::from(
                "http://proxy-user:execution-password@localhost:14003",
            ))
            .http_timeout_secs(17)
            .ws_timeout_secs(19)
            .exchange_index(3)
            .transport_backend(TransportBackend::Sockudo)
            .build();
        let round_trip_data: KalshiDataClientConfig =
            serde_json::from_str(&serde_json::to_string(&data).unwrap()).unwrap();
        let round_trip_execution: KalshiExecutionClientConfig =
            serde_json::from_str(&serde_json::to_string(&execution).unwrap()).unwrap();
        assert_eq!(data, expected_data);
        assert_eq!(execution, expected_execution);
        assert_eq!(data, round_trip_data);
        assert_eq!(execution, round_trip_execution);
        assert!(data.has_credentials());
        assert!(execution.has_credentials());
        assert_eq!(data.validate(), Ok(()));
        assert_eq!(execution.validate(), Ok(()));
        assert_eq!(data.http_url(), "http://localhost:14001/custom-api");
        assert_eq!(data.ws_url(), "ws://localhost:14002/custom-ws");
        assert_eq!(execution.http_url(), "http://localhost:14001/custom-api");
        assert_eq!(execution.ws_url(), "ws://localhost:14002/custom-ws");
    }

    #[rstest]
    #[case(
        KalshiEnvironment::Production,
        "https://external-api.kalshi.com/trade-api/v2",
        "wss://external-api-ws.kalshi.com/trade-api/ws/v2"
    )]
    #[case(
        KalshiEnvironment::Demo,
        "https://external-api.demo.kalshi.co/trade-api/v2",
        "wss://external-api-ws.demo.kalshi.co/trade-api/ws/v2"
    )]
    fn test_environment_endpoints(
        #[case] environment: KalshiEnvironment,
        #[case] http_url: &str,
        #[case] ws_url: &str,
    ) {
        let data = KalshiDataClientConfig {
            environment,
            ..Default::default()
        };

        let execution = KalshiExecutionClientConfig {
            environment,
            ..Default::default()
        };

        assert_eq!(data.http_url(), http_url);
        assert_eq!(data.ws_url(), ws_url);
        assert_eq!(execution.http_url(), http_url);
        assert_eq!(execution.ws_url(), ws_url);
    }

    #[rstest]
    #[case("unknown_field = true")]
    #[case("http_timeout_secs = -1")]
    #[case("http_timeout_secs = 'slow'")]
    #[case("environment = 'sandbox'")]
    #[case("transport_backend = 'other'")]
    #[case("trader_id = 'TRADER-001'")]
    fn test_strict_decode(#[case] input: &str) {
        assert!(toml::from_str::<KalshiDataClientConfig>(input).is_err());
        assert!(toml::from_str::<KalshiExecutionClientConfig>(input).is_err());
    }

    #[rstest]
    #[case("http_timeout_secs = 0", "http_timeout_secs")]
    #[case("ws_timeout_secs = 0", "ws_timeout_secs")]
    #[case("api_key_id = 'key'", "api_key_id")]
    #[case("private_key = 'private'", "api_key_id")]
    #[case(
        "api_key_id = 'key with spaces'\nprivate_key = 'private'",
        "api_key_id"
    )]
    #[case("api_key_id = 'key'\nprivate_key = ' '", "private_key")]
    #[case("base_url_http = 'invalid-secret-url'", "base_url_http")]
    #[case("base_url_http = 'wss://localhost/path'", "base_url_http")]
    #[case("base_url_ws = 'https://localhost/path'", "base_url_ws")]
    #[case(
        "base_url_http = 'https://external-api.demo.kalshi.co/trade-api/v2'",
        "base_url_http"
    )]
    #[case(
        "environment = 'demo'\nbase_url_http = 'https://api.elections.kalshi.com/trade-api/v2'",
        "base_url_http"
    )]
    #[case(
        "environment = 'demo'\nbase_url_ws = 'wss://external-api-ws.kalshi.com/trade-api/ws/v2'",
        "base_url_ws"
    )]
    #[case(
        "base_url_ws = 'wss://demo-api.kalshi.co/trade-api/ws/v2'",
        "base_url_ws"
    )]
    #[case("base_url_http = 'http://user:secret@localhost/path'", "base_url_http")]
    #[case(
        "base_url_http = 'http://localhost/path?token=secret'",
        "base_url_http"
    )]
    #[case("base_url_ws = 'ws://localhost/path#secret'", "base_url_ws")]
    #[case("proxy_url = 'socks5://localhost:1234'", "proxy_url")]
    #[case(
        "base_url_http = 'https://external-api.demo.kalshi.co./trade-api/v2'",
        "base_url_http"
    )]
    #[case(
        "environment = 'demo'\nbase_url_ws = 'wss://external-api-ws.kalshi.com./trade-api/ws/v2'",
        "base_url_ws"
    )]
    #[case("proxy_url = 'secret-bad-proxy'", "proxy_url")]
    fn test_invalid_values(#[case] input: &str, #[case] field: &str) {
        let data: KalshiDataClientConfig = toml::from_str(input).unwrap();
        let execution: KalshiExecutionClientConfig = toml::from_str(input).unwrap();
        let data_error = data.validate().unwrap_err();
        let execution_error = execution.validate().unwrap_err();

        match &data_error {
            ConfigError::InvalidValue { field: actual, .. }
            | ConfigError::EmptyField { field: actual } => assert_eq!(actual, field),
            other => panic!("unexpected config error {other:?}"),
        }

        assert_eq!(data_error, execution_error);

        for output in [format!("{data_error:?}"), data_error.to_string()] {
            for secret in [
                "invalid-secret-url",
                "user:secret",
                "token=secret",
                "secret-bad-proxy",
            ] {
                assert!(!output.contains(secret));
            }
        }
    }

    #[rstest]
    #[case(KalshiEnvironment::Production, "external-api.kalshi.com")]
    #[case(KalshiEnvironment::Production, "external-api-ws.kalshi.com")]
    #[case(KalshiEnvironment::Production, "api.elections.kalshi.com")]
    #[case(KalshiEnvironment::Demo, "external-api.demo.kalshi.co")]
    #[case(KalshiEnvironment::Demo, "external-api-ws.demo.kalshi.co")]
    #[case(KalshiEnvironment::Demo, "demo-api.kalshi.co")]
    fn test_venue_endpoints_require_encryption(
        #[case] environment: KalshiEnvironment,
        #[case] host: &str,
    ) {
        for suffix in ["", "."] {
            for (field, scheme, secure_scheme) in [
                ("base_url_http", "http", "https"),
                ("base_url_ws", "ws", "wss"),
            ] {
                let input = format!(
                    "environment = '{environment}'\n{field} = '{scheme}://{host}{suffix}/path'"
                );
                let data: KalshiDataClientConfig = toml::from_str(&input).unwrap();
                let execution: KalshiExecutionClientConfig = toml::from_str(&input).unwrap();
                let expected = ConfigError::invalid_value(
                    field,
                    "known venue hosts require an encrypted transport",
                );
                assert_eq!(data.validate(), Err(expected.clone()));
                assert_eq!(execution.validate(), Err(expected));

                let input = format!(
                    "environment = '{environment}'\n{field} = '{secure_scheme}://{host}{suffix}/path'"
                );
                let data: KalshiDataClientConfig = toml::from_str(&input).unwrap();
                let execution: KalshiExecutionClientConfig = toml::from_str(&input).unwrap();
                assert_eq!(data.validate(), Ok(()));
                assert_eq!(execution.validate(), Ok(()));
            }
        }
    }

    #[rstest]
    fn test_http_timeout_overflow() {
        let data = KalshiDataClientConfig {
            http_timeout_secs: u64::MAX,
            ..Default::default()
        };

        let execution = KalshiExecutionClientConfig {
            http_timeout_secs: u64::MAX,
            ..Default::default()
        };

        let expected =
            ConfigError::invalid_value("http_timeout_secs", "exceeds the supported clock range");
        assert_eq!(data.validate(), Err(expected.clone()));
        assert_eq!(execution.validate(), Err(expected));
    }

    #[rstest]
    fn test_overflow_and_refresh_validation() {
        let config = KalshiDataClientConfig {
            ws_timeout_secs: u64::MAX,
            ..Default::default()
        };

        assert_eq!(
            config.validate(),
            Err(ConfigError::invalid_value(
                "ws_timeout_secs",
                "milliseconds overflow"
            ))
        );

        for interval in [0, u64::MAX] {
            let config = KalshiDataClientConfig {
                update_instruments_interval_mins: interval,
                ..Default::default()
            };

            assert!(config.validate().is_err());
        }
    }

    #[rstest]
    #[case(None, None, false)]
    #[case(Some("key"), None, false)]
    #[case(None, Some("private"), false)]
    #[case(Some(""), Some("private"), false)]
    #[case(Some("  "), Some("private"), false)]
    #[case(Some("key"), Some("  "), false)]
    #[case(Some("key"), Some("private"), true)]
    fn test_credential_presence(
        #[case] api_key_id: Option<&str>,
        #[case] private_key: Option<&str>,
        #[case] expected: bool,
    ) {
        let data = KalshiDataClientConfig {
            api_key_id: api_key_id.map(SecretString::from),
            private_key: private_key.map(SecretString::from),
            ..Default::default()
        };

        let execution = KalshiExecutionClientConfig {
            api_key_id: data.api_key_id.clone(),
            private_key: data.private_key.clone(),
            ..Default::default()
        };

        assert_eq!(data.has_credentials(), expected);
        assert_eq!(execution.has_credentials(), expected);
    }

    #[rstest]
    fn test_debug_redacts_all_secret_owners() {
        let data = KalshiDataClientConfig {
            api_key_id: Some("distinct-key".into()),
            private_key: Some("distinct-private".into()),
            base_url_http: Some("http://secret-host/path".into()),
            base_url_ws: Some("ws://secret-ws/path".into()),
            proxy_url: Some("http://user:distinct-proxy@localhost".into()),
            ..Default::default()
        };

        let execution = KalshiExecutionClientConfig {
            account_id: AccountId::from("KALSHI-LOCAL"),
            api_key_id: data.api_key_id.clone(),
            private_key: data.private_key.clone(),
            base_url_http: data.base_url_http.clone(),
            base_url_ws: data.base_url_ws.clone(),
            proxy_url: data.proxy_url.clone(),
            ..Default::default()
        };

        for formatted in [format!("{data:?}"), format!("{execution:?}")] {
            assert_eq!(formatted.matches("<redacted>").count(), 5);

            for secret in [
                "distinct-key",
                "distinct-private",
                "secret-host",
                "secret-ws",
                "distinct-proxy",
            ] {
                assert!(!formatted.contains(secret));
            }
        }
    }

    #[rstest]
    #[case(0)]
    #[case(1)]
    #[case(31)]
    fn book_snapshot_timeout_round_trips_and_accepts_zero(#[case] seconds: u64) {
        let config: KalshiDataClientConfig =
            serde_json::from_value(serde_json::json!({"book_snapshot_timeout_secs": seconds}))
                .unwrap();
        assert_eq!(config.book_snapshot_timeout_secs, seconds);
        assert_eq!(config.validate(), Ok(()));
        assert_eq!(
            serde_json::to_value(&config).unwrap()["book_snapshot_timeout_secs"],
            seconds
        );
    }

    #[rstest]
    fn book_snapshot_timeout_rejects_clock_overflow() {
        let config = KalshiDataClientConfig::builder()
            .book_snapshot_timeout_secs(u64::MAX)
            .build();
        assert_eq!(
            config.validate(),
            Err(ConfigError::invalid_value(
                "book_snapshot_timeout_secs",
                "exceeds the supported clock range"
            ))
        );
    }
}
