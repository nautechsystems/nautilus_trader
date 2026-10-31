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

//! Async Python access to the read-only prediction-market client.

use std::sync::Arc;

use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use pyo3::prelude::*;

use super::models::{
    PyFinFeedActivityBatch, PyFinFeedActivityCurrent, PyFinFeedBook, PyFinFeedBookEvent,
    PyFinFeedExchange, PyFinFeedMarket, PyFinFeedMarketsPage, PyFinFeedOhlcv, PyFinFeedOhlcvPeriod,
    PyFinFeedQuote, PyFinFeedTrade,
};
use crate::{
    config::FinFeedDataClientConfig,
    http::{
        client::FinFeedHttpClient,
        error::Error,
        query::{
            ActiveQuery, ActivityQuery, FlatFileOhlcvQuery, HistoryQuery, ListingsQuery,
            OhlcvQuery, Pagination,
        },
    },
};

/// Read-only REST access with the HTTP controls from `FinFeedDataClientConfig`.
///
/// Construction resolves credentials but sends no requests. Instrument selections and polling
/// settings are not used. Results retain raw provider semantics; `FinFeedMarket.to_instrument`
/// performs the separate checked Nautilus mapping. Timestamps use RFC 3339 strings to retain
/// sub-microsecond precision. Canceling an awaitable cancels that call; `cancel` permanently
/// cancels all pending and future calls on this client.
#[derive(Debug)]
#[pyclass(
    name = "FinFeedHttpClient",
    module = "nautilus_trader.adapters.finfeed",
    frozen
)]
#[pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.adapters.finfeed")]
pub struct PyFinFeedHttpClient {
    inner: Arc<FinFeedHttpClient>,
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl PyFinFeedHttpClient {
    /// Authenticated read-only client returning provider models with exact decimal values.
    ///
    /// Construction resolves credentials without sending requests. Callers share pacing,
    /// concurrency, retries, and a finite attempt allowance. Requests preserve provider response
    /// order and values without native instrument mapping or automatic historical continuation.
    #[new]
    fn py_new(config: FinFeedDataClientConfig) -> PyResult<Self> {
        let inner = FinFeedHttpClient::new(config.api_key, config.http).map_err(to_pyvalue_err)?;
        Ok(Self {
            inner: Arc::new(inner),
        })
    }

    /// Returns the remaining local managed attempt allowance.
    #[getter]
    fn attempts_remaining(&self) -> u64 {
        self.inner.attempts_remaining()
    }

    /// Permanently cancels pending and future requests, including admission and retry waits.
    ///
    /// Construct a new client to make further requests.
    fn cancel(&self) {
        self.inner.cancel();
    }

    /// Returns the constant representation `FinFeedHttpClient`.
    fn __repr__(&self) -> &'static str {
        "FinFeedHttpClient"
    }

    /// Retrieves supported exchanges.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Admission, transport, HTTP status, or response decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedExchange]]",
            imports = ("typing",),
        ),
    )]
    fn get_exchanges<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_exchanges()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedExchange::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves one exchange.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedExchange]",
            imports = ("typing",),
        ),
    )]
    fn get_exchange<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_exchange(&exchange_id)
                .await
                .map(PyFinFeedExchange::from)
                .map_err(python_error)
        })
    }

    /// Retrieves one cursor listing page.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedMarketsPage]",
            imports = ("typing",),
        ),
    )]
    fn get_listings<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        query: ListingsQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_listings(&exchange_id, &query)
                .await
                .map(PyFinFeedMarketsPage::from)
                .map_err(python_error)
        })
    }

    /// Retrieves one page of active IDs; absence alone does not establish resolution.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[str]]",
            imports = ("typing",),
        ),
    )]
    fn get_active<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        query: ActiveQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_active(&exchange_id, &query)
                .await
                .map_err(python_error)
        })
    }

    /// Retrieves an outcome definition without native instrument mapping.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedMarket]",
            imports = ("typing",),
        ),
    )]
    fn get_market<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_market(&exchange_id, &market_id)
                .await
                .map(PyFinFeedMarket::from)
                .map_err(python_error)
        })
    }

    /// Retrieves the latest trade and quote independently.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedActivityCurrent]",
            imports = ("typing",),
        ),
    )]
    fn get_current<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_current(&exchange_id, &market_id)
                .await
                .map(PyFinFeedActivityCurrent::from)
                .map_err(python_error)
        })
    }

    /// Retrieves bounded activity with no continuation or continuity guarantee.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedActivityBatch]",
            imports = ("typing",),
        ),
    )]
    fn get_latest<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: ActivityQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_latest(&exchange_id, &market_id, &query)
                .await
                .map(PyFinFeedActivityBatch::from)
                .map_err(python_error)
        })
    }

    /// Retrieves one raw trade-history response without completeness or short-window guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedTrade]]",
            imports = ("typing",),
        ),
    )]
    fn get_trades_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: HistoryQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_trades_history(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedTrade::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves one raw quote-history response without completeness or short-window guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedQuote]]",
            imports = ("typing",),
        ),
    )]
    fn get_quotes_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: HistoryQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_quotes_history(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedQuote::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves a current book snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[FinFeedBook]",
            imports = ("typing",),
        ),
    )]
    fn get_book<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_book(&exchange_id, &market_id)
                .await
                .map(PyFinFeedBook::from)
                .map_err(python_error)
        })
    }

    /// Retrieves one bounded array of raw historical book events in response order.
    ///
    /// Date/time selectors use provider processing time, which may be absent from the events.
    /// Numeric actions remain uninterpreted. A saturated response may be truncated; a short or
    /// empty response does not establish coverage. No snapshot or continuation is inferred.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedBookEvent]]",
            imports = ("typing",),
        ),
    )]
    fn get_book_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: HistoryQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_book_history(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedBookEvent::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves supported OHLCV periods.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedOhlcvPeriod]]",
            imports = ("typing",),
        ),
    )]
    fn get_periods<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_periods()
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedOhlcvPeriod::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves one historical market OHLCV response without completeness or short-window
    /// guarantees.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedOhlcv]]",
            imports = ("typing",),
        ),
    )]
    fn get_ohlcv_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: OhlcvQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_ohlcv_history(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedOhlcv::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves OHLCV from the provider's flat-file activity source.
    ///
    /// The provider describes this source as trade-generated. Returns one response without filtering,
    /// pagination, or fallback to DB-backed OHLCV. A saturated response may be truncated; no
    /// continuation or bound inclusion is inferred. Short or empty responses do not establish
    /// source coverage or completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedOhlcv]]",
            imports = ("typing",),
        ),
    )]
    fn get_ohlcv_flat_file_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: FlatFileOhlcvQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_ohlcv_flat_file_history(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedOhlcv::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves one exchange-wide OHLCV response with market identifiers retained.
    ///
    /// Both time bounds are required, at most 24 hours apart, with a limit in `[1, 10000]`.
    /// Returns every response row in provider order, without time filtering or native mapping.
    /// Callers partition longer windows explicitly; saturation may truncate markets at the
    /// same timestamp, so advancing by the last timestamp cannot establish completeness.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested row limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedOhlcv]]",
            imports = ("typing",),
        ),
    )]
    fn get_ohlcv_exchange_history<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        query: OhlcvQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_ohlcv_exchange_history(&exchange_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedOhlcv::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Retrieves the newest market OHLCV bars without time bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Request validation, transport, HTTP status, or decoding fails.
    /// - The local allowance is exhausted, a deadline expires, or the request is canceled.
    /// - A response exceeds the requested record limit.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedOhlcv]]",
            imports = ("typing",),
        ),
    )]
    fn get_ohlcv_latest<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        market_id: String,
        query: OhlcvQuery,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_ohlcv_latest(&exchange_id, &market_id, &query)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedOhlcv::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Collects listings until a null or omitted cursor, rejecting cycles and exceeded traversal
    /// bounds.
    ///
    /// # Errors
    ///
    /// Returns an error if a page fails, a cursor is empty or repeats, or a pagination bound
    /// is exceeded.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[FinFeedMarket]]",
            imports = ("typing",),
        ),
    )]
    fn get_listings_all<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        query: ListingsQuery,
        bounds: Pagination,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_listings_all(&exchange_id, &query, bounds)
                .await
                .map(|records| {
                    records
                        .into_iter()
                        .map(PyFinFeedMarket::from)
                        .collect::<Vec<_>>()
                })
                .map_err(python_error)
        })
    }

    /// Collects active IDs until an empty page, rejecting duplicate IDs and exceeded bounds.
    ///
    /// A short page alone does not establish exhaustion. Results are observations of a changing
    /// catalog, not a consistent snapshot.
    ///
    /// # Errors
    ///
    /// Returns an error if a page fails, an ID is empty or repeats, or a pagination bound
    /// is exceeded.
    #[gen_stub(
        override_return_type(
            type_repr = "typing.Awaitable[list[str]]",
            imports = ("typing",),
        ),
    )]
    fn get_active_all<'py>(
        &self,
        py: Python<'py>,
        exchange_id: String,
        query: ActiveQuery,
        bounds: Pagination,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .get_active_all(&exchange_id, &query, bounds)
                .await
                .map_err(python_error)
        })
    }
}

fn python_error(e: Error) -> PyErr {
    match e {
        Error::Validation(_) | Error::Definition { .. } | Error::Unavailable(_) => {
            to_pyvalue_err(e)
        }
        _ => to_pyruntime_err(e),
    }
}

pub(super) fn register(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyFinFeedHttpClient>()?;
    Ok(())
}
