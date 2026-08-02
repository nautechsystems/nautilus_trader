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

//! Rust-backed Python discovery and historical data facade.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    python::{
        IntoPyObjectNautilusExt, params::value_to_pyobject, to_pyruntime_err, to_pyvalue_err,
    },
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::TradeTick,
    instruments::{BinaryOption, InstrumentAny},
};
use nautilus_network::retry::RetryConfig;
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};
use serde::Serialize;
use serde_json::{Value, json};

use super::extract_string_map;
use crate::{
    http::{
        clob::PolymarketClobPublicClient,
        data_api::PolymarketDataApiHttpClient,
        error::Error as PolymarketHttpError,
        gamma::PolymarketGammaHttpClient,
        models::{ClobMarketResponse, GammaEvent, GammaMarket},
        parse::{create_instrument_from_def, parse_gamma_market},
        query::{GetGammaMarketsParams, GetSearchParams},
    },
    providers::{build_gamma_event_params_from_hashmap, build_gamma_params_from_hashmap},
};

#[pyclass(name = "PolymarketDataLoader", skip_from_py_object)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.polymarket")]
#[derive(Clone, Debug)]
pub struct PyPolymarketDataLoader {
    instrument: BinaryOption,
    token_id: String,
    condition_id: String,
    resolution_metadata: Value,
    data_api_client: PolymarketDataApiHttpClient,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyPolymarketDataLoader {
    /// Queries one Gamma market by slug.
    #[staticmethod]
    #[pyo3(signature = (slug, base_url_gamma=None, timeout_secs=10))]
    fn query_market_by_slug<'py>(
        py: Python<'py>,
        slug: String,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("slug", &slug)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let market = request_market_by_slug(&client, &slug).await?;
            Python::attach(|py| serialize_to_py(py, &market))
        })
    }

    /// Queries public CLOB market details by condition ID.
    #[staticmethod]
    #[pyo3(signature = (condition_id, base_url_http=None, timeout_secs=10))]
    fn query_market_details<'py>(
        py: Python<'py>,
        condition_id: String,
        base_url_http: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("condition_id", &condition_id)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = clob_client(base_url_http, timeout_secs)?;
            let market = client
                .get_market(&condition_id)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| serialize_to_py(py, &market))
        })
    }

    /// Queries one Gamma event by slug.
    #[staticmethod]
    #[pyo3(signature = (slug, base_url_gamma=None, timeout_secs=10))]
    fn query_event_by_slug<'py>(
        py: Python<'py>,
        slug: String,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("slug", &slug)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let event = request_event_by_slug(&client, &slug).await?;
            Python::attach(|py| serialize_to_py(py, &event))
        })
    }

    /// Lists Gamma markets with validated filters and keyset pagination.
    #[staticmethod]
    #[pyo3(signature = (filters=None, base_url_gamma=None, timeout_secs=10))]
    fn query_markets<'py>(
        py: Python<'py>,
        filters: Option<&Bound<'py, PyAny>>,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let filters = extract_filters(filters)?;
        let params = build_gamma_params_from_hashmap(&filters).map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let markets = client
                .request_markets_by_params(params)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| serialize_to_py(py, &markets))
        })
    }

    /// Lists Gamma events with validated filters and keyset pagination.
    #[staticmethod]
    #[pyo3(signature = (filters=None, base_url_gamma=None, timeout_secs=10))]
    fn query_events<'py>(
        py: Python<'py>,
        filters: Option<&Bound<'py, PyAny>>,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let filters = extract_filters(filters)?;
        let params = build_gamma_event_params_from_hashmap(&filters).map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let events = client
                .request_events_by_params(params)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| serialize_to_py(py, &events))
        })
    }

    /// Lists Gamma tags.
    #[staticmethod]
    #[pyo3(signature = (base_url_gamma=None, timeout_secs=10))]
    fn query_tags<'py>(
        py: Python<'py>,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let tags = client.request_tags().await.map_err(to_pyruntime_err)?;
            Python::attach(|py| serialize_to_py(py, &tags))
        })
    }

    /// Searches Gamma markets and events.
    #[staticmethod]
    #[pyo3(signature = (
        query,
        events_status=None,
        events_tag=None,
        sort=None,
        ascending=None,
        limit_per_type=None,
        page=None,
        keep_closed_markets=None,
        base_url_gamma=None,
        timeout_secs=10,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn query_search<'py>(
        py: Python<'py>,
        query: String,
        events_status: Option<String>,
        events_tag: Option<String>,
        sort: Option<String>,
        ascending: Option<bool>,
        limit_per_type: Option<u32>,
        page: Option<u32>,
        keep_closed_markets: Option<bool>,
        base_url_gamma: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("query", &query)?;

        if limit_per_type == Some(0) {
            return Err(to_pyvalue_err("limit_per_type must be greater than zero"));
        }
        let params = GetSearchParams {
            q: Some(query),
            events_status,
            events_tag,
            sort,
            ascending,
            limit_per_type,
            page,
            keep_closed_markets,
        };

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let client = gamma_client(base_url_gamma, timeout_secs)?;
            let response = client
                .inner()
                .get_public_search(params)
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| serialize_to_py(py, &response))
        })
    }

    /// Creates one loader from a Gamma market slug.
    #[staticmethod]
    #[pyo3(signature = (
        slug,
        token_index=0,
        base_url_http=None,
        base_url_gamma=None,
        base_url_data_api=None,
        timeout_secs=10,
    ))]
    fn from_market_slug<'py>(
        py: Python<'py>,
        slug: String,
        token_index: isize,
        base_url_http: Option<String>,
        base_url_gamma: Option<String>,
        base_url_data_api: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("slug", &slug)?;
        validate_token_index(token_index)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let gamma = gamma_client(base_url_gamma, timeout_secs)?;
            let clob = clob_client(base_url_http, timeout_secs)?;
            let data_api = data_api_client(base_url_data_api, timeout_secs)?;
            let market = request_market_by_slug(&gamma, &slug).await?;
            let loader =
                build_loader(market, token_index as usize, &gamma, &clob, data_api).await?;
            Python::attach(|py| Py::new(py, loader).map(Py::into_any))
        })
    }

    /// Creates one loader for every market in a Gamma event slug.
    #[staticmethod]
    #[pyo3(signature = (
        slug,
        token_index=0,
        base_url_http=None,
        base_url_gamma=None,
        base_url_data_api=None,
        timeout_secs=10,
    ))]
    fn from_event_slug<'py>(
        py: Python<'py>,
        slug: String,
        token_index: isize,
        base_url_http: Option<String>,
        base_url_gamma: Option<String>,
        base_url_data_api: Option<String>,
        timeout_secs: u64,
    ) -> PyResult<Bound<'py, PyAny>> {
        validate_non_empty("slug", &slug)?;
        validate_token_index(token_index)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let gamma = gamma_client(base_url_gamma, timeout_secs)?;
            let clob = clob_client(base_url_http, timeout_secs)?;
            let data_api = data_api_client(base_url_data_api, timeout_secs)?;
            let event = request_event_by_slug(&gamma, &slug).await?;
            if event.markets.is_empty() {
                return Err(to_pyvalue_err(format!(
                    "No markets found in event '{slug}'"
                )));
            }

            let mut loaders = Vec::with_capacity(event.markets.len());
            for market in event.markets {
                loaders.push(
                    build_loader(
                        market,
                        token_index as usize,
                        &gamma,
                        &clob,
                        data_api.clone(),
                    )
                    .await?,
                );
            }

            Python::attach(|py| {
                let loaders = loaders
                    .into_iter()
                    .map(|loader| Py::new(py, loader))
                    .collect::<PyResult<Vec<_>>>()?;
                Ok(PyList::new(py, loaders)?.into_py_any_unwrap(py))
            })
        })
    }

    /// Returns the normalized binary option instrument.
    #[getter]
    fn instrument(&self) -> BinaryOption {
        self.instrument.clone()
    }

    /// Returns the selected CLOB token ID.
    #[getter]
    fn token_id(&self) -> &str {
        &self.token_id
    }

    /// Returns the on-chain condition ID.
    #[getter]
    fn condition_id(&self) -> &str {
        &self.condition_id
    }

    /// Returns resolution-bearing metadata excluded from `instrument.info`.
    #[getter]
    fn resolution_metadata(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        value_to_pyobject(py, &self.resolution_metadata)
    }

    /// Loads historical trades from the Rust Data API client.
    #[pyo3(signature = (start=None, end=None, limit=None))]
    fn load_trades<'py>(
        &self,
        py: Python<'py>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        if let (Some(start), Some(end)) = (start, end)
            && start > end
        {
            return Err(to_pyvalue_err("start must not be later than end"));
        }

        if limit == Some(0) {
            return Err(to_pyvalue_err("limit must be greater than zero"));
        }

        let client = self.data_api_client.clone();
        let instrument = self.instrument.clone();
        let condition_id = self.condition_id.clone();
        let token_id = self.token_id.clone();
        let start = datetime_to_unix_nanos(start);
        let end = datetime_to_unix_nanos(end);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trade_ticks(
                    instrument.id,
                    &condition_id,
                    &token_id,
                    instrument.price_precision,
                    instrument.size_precision,
                    start,
                    end,
                    limit,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Python::attach(|py| trades_to_py(py, trades))
        })
    }
}

fn gamma_client(
    base_url: Option<String>,
    timeout_secs: u64,
) -> PyResult<PolymarketGammaHttpClient> {
    PolymarketGammaHttpClient::new(base_url, timeout_secs, RetryConfig::default())
        .map_err(to_pyruntime_err)
}

fn clob_client(
    base_url: Option<String>,
    timeout_secs: u64,
) -> PyResult<PolymarketClobPublicClient> {
    PolymarketClobPublicClient::new(base_url, timeout_secs).map_err(to_pyruntime_err)
}

fn data_api_client(
    base_url: Option<String>,
    timeout_secs: u64,
) -> PyResult<PolymarketDataApiHttpClient> {
    PolymarketDataApiHttpClient::new(base_url, timeout_secs).map_err(to_pyruntime_err)
}

async fn request_market_by_slug(
    client: &PolymarketGammaHttpClient,
    slug: &str,
) -> PyResult<GammaMarket> {
    match client.inner().get_gamma_market_by_slug(slug).await {
        Ok(market) => Ok(market),
        Err(PolymarketHttpError::Http { status: 404, .. }) => Err(to_pyvalue_err(format!(
            "Market with slug '{slug}' not found"
        ))),
        Err(e) => Err(to_pyruntime_err(e)),
    }
}

async fn request_event_by_slug(
    client: &PolymarketGammaHttpClient,
    slug: &str,
) -> PyResult<GammaEvent> {
    client
        .inner()
        .get_gamma_events_by_slug(slug)
        .await
        .map_err(to_pyruntime_err)?
        .into_iter()
        .next()
        .ok_or_else(|| to_pyvalue_err(format!("Event with slug '{slug}' not found")))
}

async fn build_loader(
    mut market: GammaMarket,
    token_index: usize,
    gamma: &PolymarketGammaHttpClient,
    clob: &PolymarketClobPublicClient,
    data_api_client: PolymarketDataApiHttpClient,
) -> PyResult<PyPolymarketDataLoader> {
    if market.condition_id.trim().is_empty() {
        return Err(to_pyvalue_err("Gamma market has an empty condition ID"));
    }

    if market.fee_schedule.is_none() {
        market.fee_schedule = gamma
            .request_markets_by_params(GetGammaMarketsParams {
                condition_ids: Some(vec![market.condition_id.clone()]),
                max_markets: Some(1),
                ..Default::default()
            })
            .await
            .map_err(to_pyruntime_err)?
            .into_iter()
            .find(|candidate| candidate.condition_id == market.condition_id)
            .and_then(|candidate| candidate.fee_schedule);
    }

    let details = clob
        .get_market(&market.condition_id)
        .await
        .map_err(to_pyruntime_err)?;
    build_loader_from_details(market, &details, token_index, data_api_client)
}

fn build_loader_from_details(
    mut market: GammaMarket,
    details: &ClobMarketResponse,
    token_index: usize,
    data_api_client: PolymarketDataApiHttpClient,
) -> PyResult<PyPolymarketDataLoader> {
    validate_market_details(details, &market.condition_id, token_index)?;

    market.clob_token_ids = serde_json::to_string(
        &details
            .tokens
            .iter()
            .map(|token| token.token_id.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(to_pyruntime_err)?;
    market.outcomes = serde_json::to_string(
        &details
            .tokens
            .iter()
            .map(|token| token.outcome.as_str())
            .collect::<Vec<_>>(),
    )
    .map_err(to_pyruntime_err)?;

    let def = parse_gamma_market(&market)
        .map_err(to_pyvalue_err)?
        .into_iter()
        .nth(token_index)
        .ok_or_else(|| to_pyvalue_err("Selected token has no instrument definition"))?;
    let instrument =
        match create_instrument_from_def(&def, get_atomic_clock_realtime().get_time_ns())
            .map_err(to_pyvalue_err)?
        {
            InstrumentAny::BinaryOption(instrument) => instrument,
            _ => return Err(to_pyruntime_err("Expected a BinaryOption instrument")),
        };

    let resolution_metadata = resolution_metadata(&market, details);
    let token_id = details.tokens[token_index].token_id.clone();
    let condition_id = market.condition_id;

    Ok(PyPolymarketDataLoader {
        instrument,
        token_id,
        condition_id,
        resolution_metadata,
        data_api_client,
    })
}

fn validate_market_details(
    details: &ClobMarketResponse,
    condition_id: &str,
    token_index: usize,
) -> PyResult<()> {
    if details.condition_id != condition_id {
        return Err(to_pyvalue_err(format!(
            "CLOB market condition ID '{}' does not match Gamma condition ID '{condition_id}'",
            details.condition_id
        )));
    }

    if details.tokens.is_empty() {
        return Err(to_pyvalue_err(format!(
            "No tokens found for market '{condition_id}'"
        )));
    }

    if token_index >= details.tokens.len() {
        return Err(to_pyvalue_err(format!(
            "Token index {token_index} out of range for market '{condition_id}' with {} tokens",
            details.tokens.len()
        )));
    }

    if details.tokens[token_index].token_id.trim().is_empty() {
        return Err(to_pyvalue_err(format!(
            "Token index {token_index} for market '{condition_id}' has an empty token ID"
        )));
    }

    if details.tokens[token_index].outcome.trim().is_empty() {
        return Err(to_pyvalue_err(format!(
            "Token index {token_index} for market '{condition_id}' has an empty outcome"
        )));
    }
    Ok(())
}

fn resolution_metadata(market: &GammaMarket, details: &ClobMarketResponse) -> Value {
    json!({
        "closed": details.closed,
        "closedTime": market.closed_time,
        "umaResolutionStatus": market.uma_resolution_status,
        "resolutionSource": market.resolution_source,
        "tokens": details.tokens,
    })
}

fn validate_non_empty(name: &str, value: &str) -> PyResult<()> {
    if value.trim().is_empty() {
        Err(to_pyvalue_err(format!("{name} cannot be empty")))
    } else {
        Ok(())
    }
}

fn validate_token_index(token_index: isize) -> PyResult<()> {
    if token_index < 0 {
        Err(to_pyvalue_err(format!(
            "Token index {token_index} cannot be negative"
        )))
    } else {
        Ok(())
    }
}

fn extract_filters(filters: Option<&Bound<'_, PyAny>>) -> PyResult<HashMap<String, String>> {
    filters
        .map(extract_string_map)
        .transpose()
        .map(Option::unwrap_or_default)
}

fn serialize_to_py<T: Serialize>(py: Python<'_>, value: &T) -> PyResult<Py<PyAny>> {
    let value = serde_json::to_value(value).map_err(to_pyruntime_err)?;
    value_to_pyobject(py, &value)
}

fn trades_to_py(py: Python<'_>, trades: Vec<TradeTick>) -> PyResult<Py<PyAny>> {
    let trades = trades
        .into_iter()
        .map(|trade| trade.into_py_any(py))
        .collect::<PyResult<Vec<_>>>()?;
    Ok(PyList::new(py, trades)?.into_py_any_unwrap(py))
}

#[cfg(test)]
mod tests {
    use pyo3::exceptions::PyValueError;
    use rstest::rstest;
    use serde_json::json;

    use super::*;

    fn gamma_market() -> GammaMarket {
        serde_json::from_value(json!({
            "id": "100001",
            "conditionId": "0xcondition",
            "questionID": "0xquestion",
            "clobTokenIds": "[]",
            "outcomes": "[]",
            "question": "Will the test pass?",
            "description": "Test market",
            "startDate": "2026-01-01T00:00:00Z",
            "endDate": "2026-12-31T00:00:00Z",
            "active": false,
            "closed": true,
            "closedTime": "2026-06-01T00:00:00Z",
            "umaResolutionStatus": "resolved",
            "resolutionSource": "https://example.com/result",
            "acceptingOrders": false,
            "enableOrderBook": true,
            "orderPriceMinTickSize": 0.01,
            "slug": "test-market",
            "negRisk": false,
            "feeSchedule": {
                "exponent": 2.0,
                "rate": 0.02,
                "takerOnly": true,
                "rebateRate": 0.0
            },
            "events": []
        }))
        .expect("valid Gamma market")
    }

    fn clob_market() -> ClobMarketResponse {
        serde_json::from_value(json!({
            "condition_id": "0xcondition",
            "closed": true,
            "tokens": [
                {"token_id": "yes-token", "outcome": "Yes", "winner": true},
                {"token_id": "no-token", "outcome": "No", "winner": false}
            ]
        }))
        .expect("valid CLOB market")
    }

    fn data_api() -> PolymarketDataApiHttpClient {
        PolymarketDataApiHttpClient::new(Some("http://127.0.0.1:1".to_string()), 1)
            .expect("valid test client")
    }

    #[rstest]
    fn build_loader_selects_token_and_separates_resolution_metadata() {
        let loader = build_loader_from_details(gamma_market(), &clob_market(), 1, data_api())
            .expect("loader should build");
        let info = loader.instrument.info.as_ref().expect("instrument info");

        assert_eq!(loader.token_id, "no-token");
        assert_eq!(loader.condition_id, "0xcondition");
        assert_eq!(
            loader.instrument.outcome.map(|value| value.to_string()),
            Some("No".to_string())
        );
        assert_eq!(loader.instrument.taker_fee.to_string(), "0.02");
        assert_eq!(loader.resolution_metadata["closed"], true);
        assert_eq!(loader.resolution_metadata["tokens"][0]["winner"], true);
        assert!(!info.contains_key("closed"));
        assert!(!info.contains_key("closedTime"));
        assert!(!info.contains_key("umaResolutionStatus"));
        assert!(!info.contains_key("resolutionSource"));
        assert!(!info.contains_key("winner"));

        Python::initialize();
        Python::attach(|py| {
            let py_loader = Py::new(py, loader).expect("Python loader");
            assert_eq!(
                py_loader
                    .getattr(py, "token_id")
                    .expect("token ID getter")
                    .extract::<String>(py)
                    .expect("token ID string"),
                "no-token",
            );
            assert_eq!(
                py_loader
                    .getattr(py, "condition_id")
                    .expect("condition ID getter")
                    .extract::<String>(py)
                    .expect("condition ID string"),
                "0xcondition",
            );
            let metadata = py_loader
                .getattr(py, "resolution_metadata")
                .expect("resolution metadata getter");
            assert!(
                metadata
                    .bind(py)
                    .get_item("closed")
                    .expect("closed metadata")
                    .extract::<bool>()
                    .expect("closed bool"),
            );
        });
    }

    #[rstest]
    #[case(-1, "cannot be negative")]
    fn validate_token_index_rejects_negative_values(#[case] index: isize, #[case] message: &str) {
        let error = validate_token_index(index).expect_err("index should be rejected");

        Python::initialize();
        Python::attach(|py| assert!(error.is_instance_of::<PyValueError>(py)));
        assert!(error.to_string().contains(message));
    }

    #[rstest]
    fn build_loader_rejects_out_of_range_index() {
        Python::initialize();

        let error = build_loader_from_details(gamma_market(), &clob_market(), 2, data_api())
            .expect_err("index should be rejected");

        assert!(error.to_string().contains("Token index 2 out of range"));
    }

    #[rstest]
    fn build_loader_rejects_empty_token_list() {
        Python::initialize();

        let mut details = clob_market();
        details.tokens.clear();

        let error = build_loader_from_details(gamma_market(), &details, 0, data_api())
            .expect_err("empty tokens should be rejected");

        assert!(error.to_string().contains("No tokens found"));
    }

    #[rstest]
    fn build_loader_rejects_transient_empty_token_id() {
        Python::initialize();

        let mut details = clob_market();
        details.tokens[0].token_id.clear();

        let error = build_loader_from_details(gamma_market(), &details, 0, data_api())
            .expect_err("empty token ID should be rejected");

        assert!(error.to_string().contains("has an empty token ID"));
    }

    #[rstest]
    fn build_loader_rejects_malformed_non_binary_token_payload() {
        Python::initialize();

        let mut details = clob_market();
        details.tokens.truncate(1);

        let error = build_loader_from_details(gamma_market(), &details, 0, data_api())
            .expect_err("non-binary tokens should be rejected");

        assert!(error.to_string().contains("Expected 2 token IDs"));
    }

    #[rstest]
    fn build_loader_rejects_mismatched_condition_id() {
        Python::initialize();

        let mut details = clob_market();
        details.condition_id = "0xdifferent".to_string();

        let error = build_loader_from_details(gamma_market(), &details, 0, data_api())
            .expect_err("condition mismatch should be rejected");

        assert!(
            error
                .to_string()
                .contains("does not match Gamma condition ID")
        );
    }
}
