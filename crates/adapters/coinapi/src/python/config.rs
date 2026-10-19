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

//! Python bindings for CoinAPI configuration.

use std::collections::HashMap;

use nautilus_core::string::secret::SecretString;
use nautilus_model::identifiers::InstrumentId;
use nautilus_network::websocket::TransportBackend;
use pyo3::pymethods;
use ustr::Ustr;

use crate::{
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    http::query::SymbolsQuery,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinApiInstrumentSelection {
    /// Native instruments to load from the active symbols of one CoinAPI exchange.
    ///
    /// The optional filters bound the symbol metadata response and its credit cost.
    #[new]
    #[pyo3(signature = (exchange_id, instrument_ids, filter_symbol_id = None, filter_asset_id = None))]
    fn py_new(
        exchange_id: &str,
        instrument_ids: Vec<InstrumentId>,
        filter_symbol_id: Option<String>,
        filter_asset_id: Option<String>,
    ) -> Self {
        Self {
            exchange_id: Ustr::from(exchange_id),
            query: SymbolsQuery {
                filter_symbol_id,
                filter_asset_id,
            },
            instrument_ids,
        }
    }

    #[getter]
    fn exchange_id(&self) -> &str {
        self.exchange_id.as_str()
    }

    #[getter]
    fn instrument_ids(&self) -> Vec<InstrumentId> {
        self.instrument_ids.clone()
    }

    #[getter]
    fn filter_symbol_id(&self) -> Option<String> {
        self.query.filter_symbol_id.clone()
    }

    #[getter]
    fn filter_asset_id(&self) -> Option<String> {
        self.query.filter_asset_id.clone()
    }
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl CoinApiDataClientConfig {
    /// Configuration for the CoinAPI live data client.
    ///
    /// When `api_key` is `None`, the client reads `COINAPI_API_KEY` at construction.
    #[new]
    #[pyo3(signature = (
        api_key = None,
        base_url_http = None,
        base_url_ws = None,
        instruments = None,
        credit_budget = None,
        max_retries = None,
        transport_backend = None,
        book_snapshot_timeout_secs = None,
        price_precisions = None,
        proxy_url = None,
        http_timeout_secs = None,
        max_concurrent_requests = None,
        retry_delay_initial_ms = None,
        retry_delay_max_ms = None,
        retry_max_elapsed_ms = None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        base_url_http: Option<String>,
        base_url_ws: Option<String>,
        instruments: Option<Vec<CoinApiInstrumentSelection>>,
        credit_budget: Option<u64>,
        max_retries: Option<u32>,
        transport_backend: Option<TransportBackend>,
        book_snapshot_timeout_secs: Option<u64>,
        price_precisions: Option<HashMap<InstrumentId, u8>>,
        proxy_url: Option<String>,
        http_timeout_secs: Option<u64>,
        max_concurrent_requests: Option<usize>,
        retry_delay_initial_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        retry_max_elapsed_ms: Option<u64>,
    ) -> Self {
        let defaults = Self::default();

        Self {
            api_key: api_key.map(SecretString::from),
            base_url_http,
            base_url_ws,
            instruments: instruments.unwrap_or(defaults.instruments),
            credit_budget: credit_budget.unwrap_or(defaults.credit_budget),
            max_retries: max_retries.unwrap_or(defaults.max_retries),
            transport_backend: transport_backend.unwrap_or(defaults.transport_backend),
            book_snapshot_timeout_secs: book_snapshot_timeout_secs
                .unwrap_or(defaults.book_snapshot_timeout_secs),
            price_precisions: price_precisions.unwrap_or(defaults.price_precisions),
            proxy_url: proxy_url.map(SecretString::from),
            http_timeout_secs: http_timeout_secs.unwrap_or(defaults.http_timeout_secs),
            max_concurrent_requests: max_concurrent_requests
                .unwrap_or(defaults.max_concurrent_requests),
            retry_delay_initial_ms: retry_delay_initial_ms
                .unwrap_or(defaults.retry_delay_initial_ms),
            retry_delay_max_ms: retry_delay_max_ms.unwrap_or(defaults.retry_delay_max_ms),
            retry_max_elapsed_ms: retry_max_elapsed_ms.unwrap_or(defaults.retry_max_elapsed_ms),
        }
    }

    #[getter]
    const fn has_proxy_url(&self) -> bool {
        self.proxy_url.is_some()
    }

    fn __repr__(&self) -> String {
        stringify!(CoinApiDataClientConfig).to_string()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case(TransportBackend::Tungstenite)]
    #[case(TransportBackend::Sockudo)]
    fn constructors_preserve_selection_and_configuration_fields(#[case] backend: TransportBackend) {
        let instrument_id = InstrumentId::from("BTC-USD.COINBASE");
        let selection = CoinApiInstrumentSelection::py_new(
            "COINBASE",
            vec![instrument_id],
            Some("COINBASE_SPOT_BTC_USD".into()),
            Some("BTC".into()),
        );
        assert_eq!(selection.exchange_id(), "COINBASE");
        assert_eq!(selection.instrument_ids(), [instrument_id]);
        assert_eq!(
            selection.filter_symbol_id().as_deref(),
            Some("COINBASE_SPOT_BTC_USD")
        );
        assert_eq!(selection.filter_asset_id().as_deref(), Some("BTC"));
        let config = CoinApiDataClientConfig::py_new(
            Some("test-key".into()),
            Some("http://localhost:8123".into()),
            Some("ws://localhost:8124".into()),
            Some(vec![selection.clone()]),
            Some(17),
            Some(2),
            Some(backend),
            Some(7),
            Some(HashMap::from([(instrument_id, 3)])),
            Some("http://user:proxy-secret@localhost:8125".into()),
            Some(13),
            Some(4),
            Some(29),
            Some(53),
            Some(71_000),
        );

        assert_eq!(config.api_key.as_ref().unwrap().expose_secret(), "test-key");
        assert_eq!(
            config.base_url_http.as_deref(),
            Some("http://localhost:8123")
        );
        assert_eq!(config.base_url_ws.as_deref(), Some("ws://localhost:8124"));
        assert_eq!(config.instruments.len(), 1);
        assert_eq!(config.instruments[0].exchange_id, selection.exchange_id);
        assert_eq!(
            config.instruments[0].instrument_ids,
            selection.instrument_ids
        );
        assert_eq!(
            config.instruments[0].query.filter_symbol_id,
            selection.query.filter_symbol_id
        );
        assert_eq!(
            config.instruments[0].query.filter_asset_id,
            selection.query.filter_asset_id
        );
        assert_eq!(
            config.proxy_url.as_ref().unwrap().expose_secret(),
            "http://user:proxy-secret@localhost:8125"
        );
        assert_eq!(config.http_timeout_secs, 13);
        assert_eq!(config.max_concurrent_requests, 4);
        assert_eq!(config.retry_delay_initial_ms, 29);
        assert_eq!(config.retry_delay_max_ms, 53);
        assert_eq!(config.retry_max_elapsed_ms, 71_000);
        assert_eq!(config.credit_budget, 17);
        assert_eq!(config.max_retries, 2);
        assert_eq!(config.transport_backend, backend);
        assert_eq!(config.book_snapshot_timeout_secs, 7);
        assert_eq!(config.price_precisions, HashMap::from([(instrument_id, 3)]));
    }
}
