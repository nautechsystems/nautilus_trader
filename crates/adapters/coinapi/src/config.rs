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

//! Configuration for the CoinAPI data client.

use std::collections::HashMap;

use nautilus_core::string::secret::SecretString;
use nautilus_live::book::DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS;
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::{retry::RetryConfig, websocket::TransportBackend};
use serde::{Deserialize, Serialize};
use ustr::Ustr;

use crate::{
    http::{
        client::{CoinApiHttpConfig, REST_URL},
        query::SymbolsQuery,
    },
    websocket::client::{CoinApiWebSocketConfig, WS_URL},
};

/// Native instruments to load from the active symbols of one CoinAPI exchange.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.coinapi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")
)]
pub struct CoinApiInstrumentSelection {
    /// CoinAPI exchange whose active symbols are requested, for example `COINBASE`.
    pub exchange_id: Ustr,
    /// Metadata filters that bound the response size and its credit cost.
    #[serde(default)]
    pub query: SymbolsQuery,
    /// Native instrument IDs to load; every ID must map to `exchange_id`.
    pub instrument_ids: Vec<InstrumentId>,
}

/// Configuration for [`CoinApiDataClient`](crate::data::CoinApiDataClient).
#[derive(Debug, Clone, Serialize, Deserialize, bon::Builder)]
#[serde(default, deny_unknown_fields)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.adapters.coinapi", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.coinapi")
)]
pub struct CoinApiDataClientConfig {
    /// API key; `None` falls back to the `COINAPI_API_KEY` environment variable.
    pub api_key: Option<SecretString>,
    /// Optional override for the REST base URL, including its version prefix.
    pub base_url_http: Option<String>,
    /// Optional override for the WebSocket URL.
    pub base_url_ws: Option<String>,
    /// Optional explicit proxy URL for HTTP and WebSocket transports.
    pub proxy_url: Option<SecretString>,
    /// Instruments loaded on connect and served by instrument requests.
    #[builder(default)]
    pub instruments: Vec<CoinApiInstrumentSelection>,
    /// Conservative lifetime REST credit allowance for this client; see [`CoinApiHttpConfig`].
    #[builder(default = CoinApiHttpConfig::default().credit_budget)]
    pub credit_budget: u64,
    /// Maximum retries for a failed REST request.
    #[builder(default = CoinApiHttpConfig::default().retry.max_retries)]
    pub max_retries: u32,
    /// Positive per-attempt REST timeout in seconds, including concurrency admission.
    #[builder(default = CoinApiHttpConfig::default().retry.operation_timeout_ms.expect("default REST timeout is set") / 1_000)]
    pub http_timeout_secs: u64,
    /// Maximum simultaneous REST requests for this client instance.
    #[builder(default = CoinApiHttpConfig::default().max_concurrent_requests)]
    pub max_concurrent_requests: usize,
    /// Initial REST retry delay in milliseconds.
    #[builder(default = CoinApiHttpConfig::default().retry.initial_delay_ms)]
    pub retry_delay_initial_ms: u64,
    /// Maximum REST retry backoff in milliseconds; provider retry delays can exceed it.
    #[builder(default = CoinApiHttpConfig::default().retry.max_delay_ms)]
    pub retry_delay_max_ms: u64,
    /// Positive total REST elapsed budget in milliseconds, including admission and retries.
    #[builder(default = CoinApiHttpConfig::default().retry.max_elapsed_ms.expect("default REST elapsed budget is set"))]
    pub retry_max_elapsed_ms: u64,
    /// WebSocket transport backend.
    #[builder(default)]
    pub transport_backend: TransportBackend,
    /// Seconds to wait for an order book snapshot after a subscription, reconnect, or recovery
    /// request; zero disables the deadline.
    #[builder(default = DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS)]
    pub book_snapshot_timeout_secs: u64,
    /// Price precision by selected instrument, replacing the CoinAPI price resolution when it is
    /// coarser than the prices the venue quotes.
    #[builder(default)]
    pub price_precisions: HashMap<InstrumentId, u8>,
}

#[cfg(feature = "python")]
nautilus_core::impl_pyo3_config_getters!(CoinApiDataClientConfig {
    base_url_http: Option<String>,
    base_url_ws: Option<String>,
    instruments: Vec<CoinApiInstrumentSelection>,
    credit_budget: u64,
    max_retries: u32,
    http_timeout_secs: u64,
    max_concurrent_requests: usize,
    retry_delay_initial_ms: u64,
    retry_delay_max_ms: u64,
    retry_max_elapsed_ms: u64,
    transport_backend: TransportBackend,
    book_snapshot_timeout_secs: u64,
    price_precisions: HashMap<InstrumentId, u8>,
});

impl Default for CoinApiDataClientConfig {
    fn default() -> Self {
        Self::builder().build()
    }
}

impl CoinApiDataClientConfig {
    /// Returns the REST transport configuration, applying overrides to the defaults.
    #[must_use]
    pub fn http_config(&self) -> CoinApiHttpConfig {
        let default = CoinApiHttpConfig::default();

        CoinApiHttpConfig {
            base_url: self
                .base_url_http
                .clone()
                .unwrap_or_else(|| REST_URL.into()),
            credit_budget: self.credit_budget,
            proxy_url: self.proxy_url.clone(),
            max_concurrent_requests: self.max_concurrent_requests,
            retry: RetryConfig {
                max_retries: self.max_retries,
                operation_timeout_ms: Some(self.http_timeout_secs.saturating_mul(1_000)),
                initial_delay_ms: self.retry_delay_initial_ms,
                max_delay_ms: self.retry_delay_max_ms,
                max_elapsed_ms: Some(self.retry_max_elapsed_ms),
                ..default.retry
            },
        }
    }

    /// Returns the WebSocket transport configuration, applying overrides to the defaults.
    #[must_use]
    pub fn websocket_config(&self) -> CoinApiWebSocketConfig {
        CoinApiWebSocketConfig {
            url: self.base_url_ws.clone().unwrap_or_else(|| WS_URL.into()),
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn default_resolves_transport_defaults() {
        let config = CoinApiDataClientConfig::default();
        let http = config.http_config();
        let websocket = config.websocket_config();
        let http_default = CoinApiHttpConfig::default();

        assert!(config.api_key.is_none());
        assert!(config.instruments.is_empty());
        assert!(config.price_precisions.is_empty());
        assert_eq!(http.base_url, REST_URL);
        assert_eq!(
            http.max_concurrent_requests,
            http_default.max_concurrent_requests
        );
        assert_eq!(http.credit_budget, http_default.credit_budget);
        // RetryConfig has no PartialEq, so compare its complete Debug form
        assert_eq!(
            format!("{:?}", http.retry),
            format!("{:?}", http_default.retry)
        );
        assert_eq!(websocket.url, WS_URL);
        assert_eq!(websocket.backend, TransportBackend::Sockudo);
        assert!(config.proxy_url.is_none());
        assert!(http.proxy_url.is_none());
        assert!(websocket.proxy_url.is_none());
        assert_eq!(
            config.book_snapshot_timeout_secs,
            DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS
        );
    }

    #[rstest]
    #[case(13, 13_000)]
    #[case(0, 0)]
    #[case(u64::MAX, u64::MAX)]
    fn overrides_reach_transport_configs(#[case] timeout_secs: u64, #[case] timeout_ms: u64) {
        let config = CoinApiDataClientConfig::builder()
            .base_url_http("http://127.0.0.1:1/v1/".into())
            .base_url_ws("ws://127.0.0.1:2/v1/".into())
            .proxy_url("http://user:proxy-secret@127.0.0.1:3".into())
            .http_timeout_secs(timeout_secs)
            .max_concurrent_requests(4)
            .retry_delay_initial_ms(29)
            .retry_delay_max_ms(53)
            .retry_max_elapsed_ms(71_000)
            .credit_budget(7)
            .max_retries(0)
            .transport_backend(TransportBackend::Tungstenite)
            .build();
        let http = config.http_config();
        let websocket = config.websocket_config();
        let http_default = CoinApiHttpConfig::default();

        assert_eq!(http.base_url, "http://127.0.0.1:1/v1/");
        assert_eq!(http.credit_budget, 7);
        assert_eq!(
            format!("{:?}", http.retry),
            format!(
                "{:?}",
                RetryConfig {
                    max_retries: 0,
                    operation_timeout_ms: Some(timeout_ms),
                    initial_delay_ms: 29,
                    max_delay_ms: 53,
                    max_elapsed_ms: Some(71_000),
                    ..http_default.retry
                }
            )
        );
        assert_eq!(http.max_concurrent_requests, 4);
        assert_eq!(
            http.proxy_url.as_ref().unwrap().expose_secret(),
            "http://user:proxy-secret@127.0.0.1:3"
        );
        assert_eq!(
            websocket.proxy_url.as_ref().unwrap().expose_secret(),
            "http://user:proxy-secret@127.0.0.1:3"
        );
        assert!(!format!("{config:?} {http:?} {websocket:?}").contains("proxy-secret"));
        assert_eq!(websocket.url, "ws://127.0.0.1:2/v1/");
        assert_eq!(websocket.backend, TransportBackend::Tungstenite);
    }

    #[rstest]
    #[case(0, 120_000, 30)]
    #[case(1, 0, 30)]
    #[case(1, 120_000, 0)]
    fn transport_settings_reject_unbounded_admission(
        #[case] concurrency: usize,
        #[case] elapsed_ms: u64,
        #[case] timeout_secs: u64,
    ) {
        let config = CoinApiDataClientConfig::builder()
            .max_concurrent_requests(concurrency)
            .retry_max_elapsed_ms(elapsed_ms)
            .http_timeout_secs(timeout_secs)
            .build();
        let error =
            crate::http::client::CoinApiHttpClient::new("test-key".into(), config.http_config())
                .unwrap_err();
        assert!(
            matches!(error, crate::http::error::Error::Validation(message)
            if message == "concurrency, credits, elapsed budget, and configured timeout must be positive and bounded")
        );
    }

    #[rstest]
    #[case(0, 10_000, "delay_initial must be non-zero")]
    #[case(1_000, 500, "delay_max must be >= delay_initial")]
    #[case(
        1_000,
        u64::MAX,
        "delay_max exceeds maximum representable duration (≈584 years)"
    )]
    fn transport_settings_reject_invalid_backoff(
        #[case] initial_ms: u64,
        #[case] max_ms: u64,
        #[case] expected: &str,
    ) {
        let config = CoinApiDataClientConfig::builder()
            .max_retries(0)
            .retry_delay_initial_ms(initial_ms)
            .retry_delay_max_ms(max_ms)
            .build();
        let error =
            crate::http::client::CoinApiHttpClient::new("test-key".into(), config.http_config())
                .unwrap_err();
        assert!(
            matches!(error, crate::http::error::Error::Validation(message) if message == expected)
        );
    }

    #[rstest]
    fn transport_settings_survive_serde_roundtrip() {
        let config = CoinApiDataClientConfig::builder()
            .proxy_url("http://user:proxy-secret@localhost:1234".into())
            .http_timeout_secs(13)
            .max_concurrent_requests(4)
            .retry_delay_initial_ms(29)
            .retry_delay_max_ms(53)
            .retry_max_elapsed_ms(71_000)
            .transport_backend(TransportBackend::Tungstenite)
            .build();
        let json = serde_json::to_string(&config).unwrap();
        let decoded: CoinApiDataClientConfig = serde_json::from_str(&json).unwrap();

        assert_eq!(
            decoded.proxy_url.as_ref().unwrap().expose_secret(),
            config.proxy_url.as_ref().unwrap().expose_secret()
        );
        assert_eq!(decoded.http_timeout_secs, 13);
        assert_eq!(decoded.max_concurrent_requests, 4);
        assert_eq!(decoded.retry_delay_initial_ms, 29);
        assert_eq!(decoded.retry_delay_max_ms, 53);
        assert_eq!(decoded.retry_max_elapsed_ms, 71_000);
        assert_eq!(decoded.transport_backend, TransportBackend::Tungstenite);
    }

    #[rstest]
    fn deserializes_selection_and_fills_defaults() {
        let config: CoinApiDataClientConfig = serde_json::from_str(
            r#"{"instruments":[{"exchange_id":"COINBASE","query":{"filter_symbol_id":"COINBASE_SPOT_BTC_USD"},"instrument_ids":["BTC-USD.COINBASE"]}],"credit_budget":3,"book_snapshot_timeout_secs":0,"price_precisions":{"BTC-USD.COINBASE":3}}"#,
        )
        .unwrap();

        assert_eq!(config.credit_budget, 3);
        assert_eq!(config.book_snapshot_timeout_secs, 0);
        assert_eq!(
            config.price_precisions,
            HashMap::from([(InstrumentId::from("BTC-USD.COINBASE"), 3)])
        );
        assert_eq!(
            config.max_retries,
            CoinApiHttpConfig::default().retry.max_retries
        );
        assert_eq!(config.instruments.len(), 1);
        assert_eq!(config.instruments[0].exchange_id, "COINBASE");
        assert_eq!(
            config.instruments[0].query.filter_symbol_id.as_deref(),
            Some("COINBASE_SPOT_BTC_USD")
        );
        assert_eq!(config.instruments[0].query.filter_asset_id, None);
        assert_eq!(
            config.instruments[0].instrument_ids,
            vec![InstrumentId::from("BTC-USD.COINBASE")]
        );
    }

    #[rstest]
    #[case(r#"{"credit_budgett":3}"#)]
    #[case(r#"{"instruments":[{"exchange_id":"COINBASE","instrument_ids":[],"extra":1}]}"#)]
    #[case(r#"{"instruments":[{"exchange_id":"COINBASE","query":{"filter":"X"},"instrument_ids":[]}]}"#)]
    fn rejects_unknown_fields(#[case] json: &str) {
        assert!(serde_json::from_str::<CoinApiDataClientConfig>(json).is_err());
    }
}
