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

//! Python bindings exposing OKX HTTP helper functions and data conversions.

use chrono::{DateTime, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{BarType, forward::ForwardPrice},
    enums::{OrderSide, OrderType, PositionSide, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList, PyTuple},
};

use super::{extract_optional_string, extract_optional_trigger_type};
use crate::{
    common::enums::{
        OKXEnvironment, OKXInstrumentType, OKXOrderStatus, OKXPositionMode, OKXTradeMode,
    },
    http::{
        client::OKXHttpClient,
        error::OKXHttpError,
        models::{OKXAttachAlgoOrdRequest, OKXCancelAlgoOrderRequest},
    },
};

fn parse_attach_algo_ords(
    py: Python<'_>,
    attach_algo_ords: Option<Vec<Py<PyDict>>>,
) -> PyResult<Option<Vec<OKXAttachAlgoOrdRequest>>> {
    attach_algo_ords
        .map(|items| {
            items
                .into_iter()
                .map(|item| {
                    let dict = item.bind(py);
                    Ok(OKXAttachAlgoOrdRequest {
                        attach_algo_cl_ord_id: extract_optional_string(
                            dict,
                            "attach_algo_cl_ord_id",
                        )?,
                        sl_trigger_px: extract_optional_string(dict, "sl_trigger_px")?,
                        sl_ord_px: extract_optional_string(dict, "sl_ord_px")?,
                        sl_trigger_px_type: extract_optional_trigger_type(
                            dict,
                            "sl_trigger_px_type",
                        )?,
                        tp_trigger_px: extract_optional_string(dict, "tp_trigger_px")?,
                        tp_ord_px: extract_optional_string(dict, "tp_ord_px")?,
                        tp_trigger_px_type: extract_optional_trigger_type(
                            dict,
                            "tp_trigger_px_type",
                        )?,
                    })
                })
                .collect::<PyResult<Vec<_>>>()
        })
        .transpose()
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl OKXHttpClient {
    /// Provides a higher-level HTTP client for the [OKX](https://okx.com) REST API.
    ///
    /// This client wraps the underlying `OKXHttpInnerClient` to handle conversions
    /// into the Nautilus domain model.
    #[new]
    #[pyo3(signature = (
        api_key=None,
        api_secret=None,
        api_passphrase=None,
        base_url=None,
        timeout_secs=60,
        max_retries=3,
        retry_delay_ms=1_000,
        retry_delay_max_ms=10_000,
        environment=OKXEnvironment::Live,
        proxy_url=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        environment: OKXEnvironment,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::with_credentials(
            api_key,
            api_secret,
            api_passphrase,
            base_url,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            environment,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Creates a new authenticated `OKXHttpClient` using environment variables and
    /// the default OKX HTTP base url.
    ///
    /// # Errors
    ///
    /// Returns an error if the operation fails.
    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    /// Returns the base url being used by the client.
    #[getter]
    #[pyo3(name = "base_url")]
    #[must_use]
    pub fn py_base_url(&self) -> &str {
        self.base_url()
    }

    /// Returns the public API key being used by the client.
    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> Option<&str> {
        self.api_key()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.api_key_masked()
    }

    /// Checks if the client is initialized.
    ///
    /// The client is considered initialized if any instruments have been cached from the venue.
    #[pyo3(name = "is_initialized")]
    #[must_use]
    pub fn py_is_initialized(&self) -> bool {
        self.is_initialized()
    }

    /// Returns a snapshot of all instrument symbols currently held in the
    /// internal cache.
    #[pyo3(name = "get_cached_symbols")]
    #[must_use]
    pub fn py_get_cached_symbols(&self) -> Vec<String> {
        self.get_cached_symbols()
    }

    /// Cancel all pending HTTP requests.
    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same symbols will be replaced.
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

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Sets the position mode for the account.
    ///
    /// Defaults to NetMode if no position mode is provided.
    ///
    /// # Note
    ///
    /// This endpoint only works for accounts with derivatives trading enabled.
    /// If the account only has spot trading, this will return an error.
    #[pyo3(name = "set_position_mode")]
    fn py_set_position_mode<'py>(
        &self,
        py: Python<'py>,
        position_mode: OKXPositionMode,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .set_position_mode(position_mode)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    /// Requests all instruments for the `instrument_type` from OKX.
    ///
    /// # Returns
    ///
    /// A tuple containing:
    /// - `Vec<InstrumentAny>`: The parsed instruments
    /// - `Vec<(Ustr, u64)>`: Mappings of inst_id to inst_id_code for WebSocket order operations
    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (instrument_type, instrument_family=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
        instrument_family: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let (instruments, inst_id_codes) = client
                .request_instruments(instrument_type, instrument_family)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let instruments_list = PyList::new(py, py_instruments?).unwrap();

                // Convert inst_id_codes to list of (inst_id: str, inst_id_code: int) tuples
                let py_codes: Vec<_> = inst_id_codes
                    .into_iter()
                    .map(|(inst_id, code)| (inst_id.to_string(), code))
                    .collect();
                let codes_list = PyList::new(py, py_codes).unwrap();

                let result = PyTuple::new(py, [instruments_list.as_any(), codes_list.as_any()])
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(result)
            })
        })
    }

    /// Requests a single instrument by `instrument_id` from OKX.
    ///
    /// Fetches the instrument from the API, caches it, and returns it.
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

    /// Requests the account state for the `account_id` from OKX.
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

    /// Requests trades for the `instrument_id` and `start` -> `end` time range.
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
                let pylist = PyList::new(py, trades.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests historical bars for the given bar type and time range.
    ///
    /// The aggregation source must be `EXTERNAL`. Time range validation ensures start < end.
    /// Returns bars sorted oldest to newest.
    ///
    /// # Endpoint Selection
    ///
    /// The OKX API has different endpoints with different limits:
    /// - Regular endpoint (`/api/v5/market/candles`): ≤ 300 rows/call, ≤ 40 req/2s
    ///   - Used when: start is None OR age ≤ 100 days
    /// - History endpoint (`/api/v5/market/history-candles`): ≤ 100 rows/call, ≤ 20 req/2s
    ///   - Used when: start is Some AND age > 100 days
    ///
    /// Age is calculated as `Utc::now() - start` at the time of the first request.
    ///
    /// # Supported Aggregations
    ///
    /// Maps to OKX bar query parameter:
    /// - `Second` → `{n}s`
    /// - `Minute` → `{n}m`
    /// - `Hour` → `{n}H`
    /// - `Day` → `{n}D`
    /// - `Week` → `{n}W`
    /// - `Month` → `{n}M`
    ///
    /// # Pagination
    ///
    /// - Uses `before` parameter for backwards pagination
    /// - Pages backwards from end time (or now) to start time
    /// - Stops when: limit reached, time window covered, or API returns empty
    /// - Rate limit safety: ≥ 50ms between requests
    ///
    /// # References
    ///
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks>
    /// - <https://tr.okx.com/docs-v5/en/#order-book-trading-market-data-get-candlesticks-history>
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

    /// Requests an order book snapshot as `OrderBookDeltas` for the `instrument_id`.
    #[pyo3(name = "request_orderbook_snapshot")]
    #[pyo3(signature = (instrument_id, depth=None))]
    fn py_request_orderbook_snapshot<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let deltas = client
                .request_orderbook_snapshot(instrument_id, depth)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(deltas.into_py_any_unwrap(py)))
        })
    }

    /// Requests historical funding rates for the `instrument_id`.
    #[pyo3(name = "request_funding_rates")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let rates = client
                .request_funding_rates(instrument_id, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, rates.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests forward prices for OKX options using the option summary endpoint.
    #[pyo3(name = "request_forward_prices")]
    #[pyo3(signature = (underlying, instrument_id=None))]
    fn py_request_forward_prices<'py>(
        &self,
        py: Python<'py>,
        underlying: String,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let forward_prices: Vec<ForwardPrice> = client
                .request_forward_prices(&underlying, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(
                    py,
                    forward_prices
                        .into_iter()
                        .map(|price| price.into_py_any_unwrap(py)),
                )?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests the latest mark price for the `instrument_type` from OKX.
    #[pyo3(name = "request_mark_price")]
    fn py_request_mark_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mark_price = client
                .request_mark_price(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(mark_price.into_py_any_unwrap(py)))
        })
    }

    /// Requests the latest index price for the `instrument_id` from OKX.
    #[pyo3(name = "request_index_price")]
    fn py_request_index_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let index_price = client
                .request_index_price(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(index_price.into_py_any_unwrap(py)))
        })
    }

    /// Requests historical order status reports for the given parameters.
    ///
    /// # References
    ///
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-7-days>.
    /// - <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-order-history-last-3-months>.
    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, start=None, end=None, open_only=false, limit=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(
                    account_id,
                    instrument_type,
                    instrument_id,
                    start,
                    end,
                    open_only,
                    limit,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests algo order status reports.
    #[pyo3(name = "request_algo_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, algo_id=None, algo_client_order_id=None, state=None, limit=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_algo_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        algo_id: Option<String>,
        algo_client_order_id: Option<ClientOrderId>,
        state: Option<OKXOrderStatus>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_algo_order_status_reports(
                    account_id,
                    instrument_type,
                    instrument_id,
                    algo_id,
                    algo_client_order_id,
                    state,
                    limit,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests an algo order status report by client order identifier.
    #[pyo3(name = "request_algo_order_status_report")]
    fn py_request_algo_order_status_report<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .request_algo_order_status_report(account_id, instrument_id, client_order_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| match report {
                Some(report) => Ok(report.into_py_any_unwrap(py)),
                None => Ok(py.None()),
            })
        })
    }

    /// Requests fill reports (transaction details) for the given parameters.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-get-transaction-details-last-3-days>.
    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, start=None, end=None, limit=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_fill_reports(
                    account_id,
                    instrument_type,
                    instrument_id,
                    start,
                    end,
                    limit,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist = PyList::new(py, trades.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests current position status reports for the given parameters.
    ///
    /// # Position Modes
    ///
    /// OKX supports two position modes, which affects how position data is returned:
    ///
    /// ## Net Mode (One-way)
    /// - `posSide` field will be `"net"`
    /// - `pos` field uses **signed quantities**:
    ///   - Positive value = Long position
    ///   - Negative value = Short position
    ///   - Zero = Flat/no position
    ///
    /// ## Long/Short Mode (Hedge/Dual-side)
    /// - `posSide` field will be `"long"` or `"short"`
    /// - `pos` field is **always positive** (use `posSide` to determine actual side)
    /// - Allows holding simultaneous long and short positions on the same instrument
    /// - Position IDs are suffixed with `-LONG` or `-SHORT` for uniqueness
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#trading-account-rest-api-get-positions>
    #[pyo3(name = "request_position_status_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None))]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_type: Option<OKXInstrumentType>,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports(account_id, instrument_type, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Places a regular order via HTTP.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-trade-post-place-order>
    #[pyo3(name = "place_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        td_mode,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force=None,
        price=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        position_side=None,
        attach_algo_ords=None,
        px_usd=None,
        px_vol=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_place_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        position_side: Option<PositionSide>,
        attach_algo_ords: Option<Vec<Py<PyDict>>>,
        px_usd: Option<String>,
        px_vol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let attach_algo_ords = parse_attach_algo_ords(py, attach_algo_ords)?;
        let client = self.clone();

        let _ = (trader_id, strategy_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client
                .place_order_with_domain_types(
                    instrument_id,
                    td_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    post_only,
                    reduce_only,
                    quote_quantity,
                    position_side,
                    attach_algo_ords,
                    px_usd,
                    px_vol,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let dict = PyDict::new(py);

                if let Some(ord_id) = resp.ord_id {
                    dict.set_item("ord_id", ord_id.as_str())?;
                }

                if let Some(cl_ord_id) = resp.cl_ord_id {
                    dict.set_item("cl_ord_id", cl_ord_id.as_str())?;
                }

                if let Some(s_code) = resp.s_code {
                    dict.set_item("s_code", s_code)?;
                }

                if let Some(s_msg) = resp.s_msg {
                    dict.set_item("s_msg", s_msg)?;
                }

                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Places an algo order via HTTP.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-place-algo-order>
    #[pyo3(name = "place_algo_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        td_mode,
        client_order_id,
        order_side,
        order_type,
        quantity,
        trigger_price=None,
        trigger_type=None,
        limit_price=None,
        reduce_only=None,
        close_fraction=None,
        callback_ratio=None,
        callback_spread=None,
        activation_price=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_place_algo_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        limit_price: Option<Price>,
        reduce_only: Option<bool>,
        close_fraction: Option<String>,
        callback_ratio: Option<String>,
        callback_spread: Option<String>,
        activation_price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        // Accept trader_id and strategy_id for interface standardization
        let _ = (trader_id, strategy_id);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client
                .place_algo_order_with_domain_types(
                    instrument_id,
                    td_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    trigger_price,
                    trigger_type,
                    limit_price,
                    reduce_only,
                    close_fraction,
                    callback_ratio,
                    callback_spread,
                    activation_price,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("algo_id", resp.algo_id)?;
                if let Some(algo_cl_ord_id) = resp.algo_cl_ord_id {
                    dict.set_item("algo_cl_ord_id", algo_cl_ord_id)?;
                }

                if let Some(s_code) = resp.s_code {
                    dict.set_item("s_code", s_code)?;
                }

                if let Some(s_msg) = resp.s_msg {
                    dict.set_item("s_msg", s_msg)?;
                }

                if let Some(req_id) = resp.req_id {
                    dict.set_item("req_id", req_id)?;
                }
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Cancels an algo order via HTTP.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-cancel-algo-order>
    #[pyo3(name = "cancel_algo_order")]
    fn py_cancel_algo_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        algo_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client
                .cancel_algo_order_with_domain_types(instrument_id, algo_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("algo_id", resp.algo_id)?;
                if let Some(s_code) = resp.s_code {
                    dict.set_item("s_code", s_code)?;
                }

                if let Some(s_msg) = resp.s_msg {
                    dict.set_item("s_msg", s_msg)?;
                }
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Amends an algo order via HTTP.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-amend-algo-order>
    #[expect(clippy::too_many_arguments)]
    #[pyo3(name = "amend_algo_order")]
    #[pyo3(signature = (
        instrument_id,
        algo_id,
        new_trigger_price=None,
        new_limit_price=None,
        new_quantity=None,
        new_callback_ratio=None,
        new_callback_spread=None,
        new_activation_price=None,
    ))]
    fn py_amend_algo_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        algo_id: String,
        new_trigger_price: Option<Price>,
        new_limit_price: Option<Price>,
        new_quantity: Option<Quantity>,
        new_callback_ratio: Option<String>,
        new_callback_spread: Option<String>,
        new_activation_price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let resp = client
                .amend_algo_order_with_domain_types(
                    instrument_id,
                    algo_id,
                    new_trigger_price,
                    new_limit_price,
                    new_quantity,
                    new_callback_ratio,
                    new_callback_spread,
                    new_activation_price,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("algo_id", resp.algo_id)?;
                if let Some(s_code) = resp.s_code {
                    dict.set_item("s_code", s_code)?;
                }

                if let Some(s_msg) = resp.s_msg {
                    dict.set_item("s_msg", s_msg)?;
                }
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Cancels multiple algo orders via HTTP in a single request.
    ///
    /// Items with non-zero `sCode` are logged as warnings but do not
    /// fail the entire batch.
    ///
    /// # References
    ///
    /// <https://www.okx.com/docs-v5/en/#order-book-trading-algo-trading-post-cancel-algo-order>
    #[pyo3(name = "cancel_algo_orders")]
    fn py_cancel_algo_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<(InstrumentId, String)>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let requests: Vec<_> = orders
                .into_iter()
                .map(|(instrument_id, algo_id)| OKXCancelAlgoOrderRequest {
                    inst_id: instrument_id.symbol.to_string(),
                    inst_id_code: None,
                    algo_id: Some(algo_id),
                    algo_cl_ord_id: None,
                })
                .collect();

            let responses = client
                .cancel_algo_orders(requests)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let results: Vec<_> = responses
                    .into_iter()
                    .map(|resp| {
                        let dict = PyDict::new(py);
                        dict.set_item("algo_id", resp.algo_id).expect("set algo_id");
                        if let Some(s_code) = resp.s_code {
                            dict.set_item("s_code", s_code).expect("set s_code");
                        }

                        if let Some(s_msg) = resp.s_msg {
                            dict.set_item("s_msg", s_msg).expect("set s_msg");
                        }
                        dict
                    })
                    .collect();
                let pylist = PyList::new(py, results)?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "cancel_advance_algo_order")]
    fn py_cancel_advance_algo_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        algo_id: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let request = OKXCancelAlgoOrderRequest {
                inst_id: instrument_id.symbol.to_string(),
                inst_id_code: None,
                algo_id: Some(algo_id),
                algo_cl_ord_id: None,
            };

            let mut responses = client
                .cancel_advance_algo_orders(vec![request])
                .await
                .map_err(to_pyvalue_err)?;

            let resp = responses
                .pop()
                .ok_or_else(|| to_pyvalue_err("Empty response"))?;

            Python::attach(|py| {
                let dict = PyDict::new(py);
                dict.set_item("algo_id", resp.algo_id)?;

                if let Some(s_code) = resp.s_code {
                    dict.set_item("s_code", s_code)?;
                }

                if let Some(s_msg) = resp.s_msg {
                    dict.set_item("s_msg", s_msg)?;
                }
                Ok(dict.into_py_any_unwrap(py))
            })
        })
    }

    /// Requests the current server time from OKX.
    ///
    /// Returns the OKX system time as a Unix timestamp in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP request fails or if the response cannot be parsed.
    #[pyo3(name = "get_server_time")]
    fn py_get_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timestamp = client.get_server_time().await.map_err(to_pyvalue_err)?;

            Python::attach(|py| timestamp.into_py_any(py))
        })
    }

    #[pyo3(name = "get_balance")]
    fn py_get_balance<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let accounts = client.inner.get_balance().await.map_err(to_pyvalue_err)?;

            let details: Vec<_> = accounts
                .into_iter()
                .flat_map(|account| account.details)
                .collect();

            Python::attach(|py| {
                let pylist = PyList::new(py, details)?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }
}

impl From<OKXHttpError> for PyErr {
    fn from(error: OKXHttpError) -> Self {
        match error {
            // Runtime/operational errors
            OKXHttpError::Canceled(msg) => to_pyruntime_err(format!("Request canceled: {msg}")),
            OKXHttpError::HttpClientError(e) => to_pyruntime_err(format!("Network error: {e}")),
            OKXHttpError::UnexpectedStatus { status, body } => {
                to_pyruntime_err(format!("Unexpected HTTP status code {status}: {body}"))
            }
            // Validation/configuration errors
            OKXHttpError::MissingCredentials => {
                to_pyvalue_err("Missing credentials for authenticated request")
            }
            OKXHttpError::ValidationError(msg) => {
                to_pyvalue_err(format!("Parameter validation error: {msg}"))
            }
            OKXHttpError::JsonError(msg) => to_pyvalue_err(format!("JSON error: {msg}")),
            OKXHttpError::OkxError {
                error_code,
                message,
            } => to_pyvalue_err(format!("OKX error {error_code}: {message}")),
        }
    }
}
