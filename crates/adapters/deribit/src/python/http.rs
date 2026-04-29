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

//! Python bindings for Deribit HTTP client.

use std::collections::HashSet;

use chrono::{DateTime, Utc};
use nautilus_core::{
    UnixNanos,
    python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::{BarType, forward::ForwardPrice},
    identifiers::{AccountId, InstrumentId, Symbol},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::{
    common::{consts::DERIBIT_VENUE, enums::DeribitEnvironment},
    http::{
        client::DeribitHttpClient,
        error::DeribitHttpError,
        models::{DeribitCurrency, DeribitProductType},
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DeribitHttpClient {
    /// High-level Deribit HTTP client with domain-level abstractions.
    ///
    /// This client wraps the raw HTTP client and provides methods that use Nautilus
    /// domain types. It maintains an instrument cache for efficient lookups.
    #[new]
    #[pyo3(signature = (
        api_key=None,
        api_secret=None,
        base_url=None,
        environment=DeribitEnvironment::Mainnet,
        timeout_secs=10,
        max_retries=3,
        retry_delay_ms=1000,
        retry_delay_max_ms=10_000,
        proxy_url=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    #[allow(unused_variables)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        environment: DeribitEnvironment,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new_with_env(
            api_key,
            api_secret,
            base_url,
            environment,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns whether this client is connected to testnet.
    #[getter]
    #[pyo3(name = "is_testnet")]
    #[must_use]
    pub fn py_is_testnet(&self) -> bool {
        self.is_testnet()
    }

    #[pyo3(name = "is_initialized")]
    #[must_use]
    pub fn py_is_initialized(&self) -> bool {
        self.is_cache_initialized()
    }

    /// Caches instruments for later retrieval.
    #[pyo3(name = "cache_instruments")]
    pub fn py_cache_instruments(
        &self,
        py: Python<'_>,
        instruments: Vec<Py<PyAny>>,
    ) -> PyResult<()> {
        let instruments: Result<Vec<_>, _> = instruments
            .into_iter()
            .map(|inst| pyobject_to_instrument_any(py, inst))
            .collect();
        self.cache_instruments(&instruments?);
        Ok(())
    }

    /// # Errors
    ///
    /// Returns a Python exception if adding the instrument to the cache fails.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instruments(std::slice::from_ref(&inst));
        Ok(())
    }

    /// Requests instruments for a specific currency.
    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (currency, product_type=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        currency: DeribitCurrency,
        product_type: Option<DeribitProductType>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(currency, product_type)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let pylist = PyList::new(py, py_instruments?)
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests a specific instrument by its Nautilus instrument ID.
    ///
    /// This is a high-level method that fetches the raw instrument data from Deribit
    /// and converts it to a Nautilus `InstrumentAny` type.
    #[pyo3(name = "request_instrument")]
    fn py_request_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instrument = client
                .request_instrument(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| instrument_any_to_pyobject(py, instrument))
        })
    }

    /// Requests account state for all currencies.
    ///
    /// Fetches account balance and margin information for all currencies from Deribit
    /// and converts it to Nautilus `AccountState` event.
    #[pyo3(name = "request_account_state")]
    fn py_request_account_state<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_state = client
                .request_account_state(account_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(account_state.into_py_any_unwrap(py)))
        })
    }

    /// Requests historical trades for an instrument within a time range.
    ///
    /// Fetches trade ticks from Deribit and converts them to Nautilus `TradeTick` objects.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to fetch trades for
    /// * `start` - Optional start time filter
    /// * `end` - Optional end time filter
    /// * `limit` - Optional limit on number of trades (max 1000)
    ///
    /// # Pagination
    ///
    /// When `limit` is `None`, this function automatically paginates through all available
    /// trades in the time range using the `has_more` field from the API response.
    /// When `limit` is specified, pagination stops once that many trades are collected.
    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trades(instrument_id, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(
                    py,
                    trades.into_iter().map(|trade| trade.into_py_any_unwrap(py)),
                )?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests historical bars (OHLCV) for an instrument.
    ///
    /// Uses the `public/get_tradingview_chart_data` endpoint to fetch candlestick data.
    ///
    /// # Supported Resolutions
    ///
    /// Deribit supports: 1, 3, 5, 10, 15, 30, 60, 120, 180, 360, 720 minutes, and 1D (daily)
    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, bars.into_iter().map(|bar| bar.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests a snapshot of the order book for an instrument.
    ///
    /// Fetches the order book from Deribit and converts it to a Nautilus `OrderBook`.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to fetch the order book for
    /// * `depth` - Optional depth limit (valid values: 1, 5, 10, 20, 50, 100, 1000, 10000)
    #[pyo3(name = "request_book_snapshot")]
    #[pyo3(signature = (instrument_id, depth=None))]
    fn py_request_book_snapshot<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let book = client
                .request_book_snapshot(instrument_id, depth)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(book.into_py_any_unwrap(py)))
        })
    }

    /// Requests order status reports for reconciliation.
    ///
    /// Fetches order statuses from Deribit and converts them to Nautilus `OrderStatusReport`.
    ///
    /// # Strategy
    /// - Uses `/private/get_open_orders` for all open orders (single efficient API call)
    /// - Uses `/private/get_open_orders_by_instrument` when specific instrument is provided
    /// - For historical orders (when `open_only=false`), iterates over currencies
    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None, open_only=true))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
        open_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(
                    account_id,
                    instrument_id,
                    start.map(nautilus_core::UnixNanos::from),
                    end.map(nautilus_core::UnixNanos::from),
                    open_only,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests fill reports for reconciliation.
    ///
    /// Fetches user trades from Deribit and converts them to Nautilus `FillReport`.
    /// Automatically paginates through all results using time-cursor advancement.
    ///
    /// # Strategy
    /// - Uses `/private/get_user_trades_by_instrument_and_time` when instrument is provided
    /// - Otherwise iterates over currencies using `/private/get_user_trades_by_currency_and_time`
    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(
                    account_id,
                    instrument_id,
                    start.map(nautilus_core::UnixNanos::from),
                    end.map(nautilus_core::UnixNanos::from),
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests position status reports for reconciliation.
    ///
    /// Fetches positions from Deribit and converts them to Nautilus `PositionStatusReport`.
    ///
    /// # Strategy
    /// - Uses `currency=any` to fetch all positions in one call
    /// - Filters by instrument_id if provided
    #[pyo3(name = "request_position_status_reports")]
    #[pyo3(signature = (account_id, instrument_id=None))]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports(account_id, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Request forward prices for option chain ATM determination.
    ///
    /// Single-instrument path (1 HTTP call) if `instrument_id` is provided,
    /// otherwise bulk path via book summaries.
    #[pyo3(name = "request_forward_prices")]
    #[pyo3(signature = (currency, instrument_id=None))]
    fn py_request_forward_prices<'py>(
        &self,
        py: Python<'py>,
        currency: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let forward_prices = if let Some(inst_id) = instrument_id {
                // Single-instrument path: 1 HTTP call to public/ticker
                let instrument_name = inst_id.symbol.to_string();
                let ticker = client
                    .request_ticker(&instrument_name)
                    .await
                    .map_err(to_pyvalue_err)?;

                let ts = UnixNanos::default();
                ticker
                    .underlying_price
                    .map(|up| {
                        vec![ForwardPrice::new(
                            inst_id,
                            up,
                            ticker.underlying_index.filter(|s| !s.is_empty()),
                            ts,
                            ts,
                        )]
                    })
                    .unwrap_or_default()
            } else {
                // Bulk path: fetch all book summaries
                let summaries = client
                    .request_book_summaries(&currency)
                    .await
                    .map_err(to_pyvalue_err)?;

                let ts = nautilus_core::UnixNanos::default();
                let mut seen_indices = HashSet::new();
                summaries
                    .into_iter()
                    .filter_map(|s| {
                        let up = s.underlying_price?;
                        let idx = s.underlying_index.clone().unwrap_or_default();
                        if !seen_indices.insert(idx.clone()) {
                            return None;
                        }
                        Some(ForwardPrice::new(
                            InstrumentId::new(Symbol::new(&s.instrument_name), *DERIBIT_VENUE),
                            up,
                            Some(idx).filter(|s| !s.is_empty()),
                            ts,
                            ts,
                        ))
                    })
                    .collect()
            };

            Python::attach(|py| {
                let py_prices: PyResult<Vec<_>> = forward_prices
                    .into_iter()
                    .map(|fp| Py::new(py, fp))
                    .collect();
                let pylist = PyList::new(py, py_prices?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }
}

impl From<DeribitHttpError> for PyErr {
    fn from(error: DeribitHttpError) -> Self {
        match error {
            // Runtime/operational errors
            DeribitHttpError::Canceled(msg) => to_pyruntime_err(format!("Request canceled: {msg}")),
            DeribitHttpError::NetworkError(msg) => {
                to_pyruntime_err(format!("Network error: {msg}"))
            }
            DeribitHttpError::UnexpectedStatus { status, body } => {
                to_pyruntime_err(format!("Unexpected HTTP status code {status}: {body}"))
            }
            DeribitHttpError::Timeout(msg) => to_pyruntime_err(format!("Request timeout: {msg}")),
            // Validation/configuration errors
            DeribitHttpError::MissingCredentials => {
                to_pyvalue_err("Missing credentials for authenticated request")
            }
            DeribitHttpError::ValidationError(msg) => {
                to_pyvalue_err(format!("Parameter validation error: {msg}"))
            }
            DeribitHttpError::JsonError(msg) => to_pyvalue_err(format!("JSON error: {msg}")),
            DeribitHttpError::DeribitError {
                error_code,
                message,
            } => to_pyvalue_err(format!("Deribit error {error_code}: {message}")),
        }
    }
}
