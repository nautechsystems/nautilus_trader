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

//! Python bindings for the Ax HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::{datetime::datetime_to_unix_nanos, python::to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyList};
use rust_decimal::Decimal;

use crate::{
    common::{
        enums::{AxCandleWidth, AxOrderSide},
        parse::quantity_to_contracts,
    },
    http::{client::AxHttpClient, error::AxHttpError, models::PreviewAggressiveLimitOrderRequest},
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl AxHttpClient {
    /// High-level HTTP client for the Ax REST API.
    ///
    /// This client wraps the underlying `AxRawHttpClient` to provide a convenient
    /// interface for Python bindings and instrument caching.
    #[new]
    #[pyo3(signature = (
        base_url=None,
        orders_base_url=None,
        timeout_secs=60,
        max_retries=3,
        retry_delay_ms=1000,
        retry_delay_max_ms=10_000,
        proxy_url=None,
    ))]
    fn py_new(
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            base_url,
            orders_base_url,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Creates a new `AxHttpClient` configured with credentials.
    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    #[pyo3(signature = (
        api_key,
        api_secret,
        base_url=None,
        orders_base_url=None,
        timeout_secs=60,
        max_retries=3,
        retry_delay_ms=1000,
        retry_delay_max_ms=10_000,
        proxy_url=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: u64,
        max_retries: u32,
        retry_delay_ms: u64,
        retry_delay_max_ms: u64,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::with_credentials(
            api_key,
            api_secret,
            base_url,
            orders_base_url,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    /// Returns the base URL for this client.
    #[getter]
    #[pyo3(name = "base_url")]
    #[must_use]
    pub fn py_base_url(&self) -> &str {
        self.base_url()
    }

    /// Returns a masked version of the API key for logging purposes.
    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> String {
        self.api_key_masked()
    }

    /// Cancel all pending HTTP requests.
    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Cancels all open orders for an instrument.
    #[pyo3(name = "cancel_all_orders")]
    pub fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_all_orders(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Authenticates with Ax using API credentials.
    ///
    /// On success, the session token is automatically stored for subsequent authenticated requests.
    #[pyo3(name = "authenticate")]
    #[pyo3(signature = (api_key, api_secret, expiration_seconds=86400))]
    fn py_authenticate<'py>(
        &self,
        py: Python<'py>,
        api_key: String,
        api_secret: String,
        expiration_seconds: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate(&api_key, &api_secret, expiration_seconds)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Authenticates using stored credentials or environment variables.
    ///
    /// # Credential Resolution
    ///
    /// Credentials are resolved in the following order:
    /// 1. Stored credentials (from `with_credentials` constructor)
    /// 2. Environment variables (`AX_API_KEY` and `AX_API_SECRET`)
    ///
    /// On success, the session token is automatically stored for subsequent authenticated requests.
    #[pyo3(name = "authenticate_auto")]
    #[pyo3(signature = (expiration_seconds=86400))]
    fn py_authenticate_auto<'py>(
        &self,
        py: Python<'py>,
        expiration_seconds: i32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate_auto(expiration_seconds)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Requests all instruments from Ax.
    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (maker_fee=None, taker_fee=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        maker_fee: Option<Decimal>,
        taker_fee: Option<Decimal>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(maker_fee, taker_fee)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let pylist = PyList::new(py, py_instruments?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests recent trades from Ax and parses them to Nautilus `TradeTick`.
    ///
    /// The AX trades endpoint does not accept time range parameters, so
    /// `start` and `end` are applied as client-side filters after fetching.
    ///
    /// Requires the instrument to be cached.
    #[pyo3(name = "request_trade_ticks")]
    #[pyo3(signature = (instrument_id, limit=None, start=None, end=None))]
    fn py_request_trade_ticks<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        limit: Option<i32>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = instrument_id.symbol.inner();
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trade_ticks(symbol, limit, start_nanos, end_nanos)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|trade| trade.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests historical bars from Ax and parses them to Nautilus Bar types.
    ///
    /// Requires the instrument to be cached (call `request_instruments` first).
    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let symbol = bar_type.instrument_id().symbol.inner();
        let width = AxCandleWidth::try_from(&bar_type.spec()).map_err(to_pyvalue_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(symbol, start, end, width)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_bars: PyResult<Vec<_>> =
                    bars.into_iter().map(|bar| bar.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_bars?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests funding rates from Ax and parses them to Nautilus types.
    #[pyo3(name = "request_funding_rates")]
    #[pyo3(signature = (instrument_id, start=None, end=None))]
    fn py_request_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let funding_rates = client
                .request_funding_rates(instrument_id, start, end)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_rates: PyResult<Vec<_>> = funding_rates
                    .into_iter()
                    .map(|rate| rate.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_rates?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests account state from Ax and parses to a Nautilus `AccountState`.
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

            Python::attach(|py| account_state.into_py_any(py))
        })
    }

    /// Queries a single order by venue order ID or client order ID using the
    /// dedicated `/order-status` endpoint, which works for any order state.
    ///
    /// The caller must supply `order_side`, `order_type`, and `time_in_force`
    /// because the endpoint does not return these fields.
    #[pyo3(name = "request_order_status")]
    #[pyo3(signature = (
        account_id,
        instrument_id,
        order_side,
        order_type,
        time_in_force,
        client_order_id=None,
        venue_order_id=None,
    ))]
    #[expect(clippy::too_many_arguments)]
    fn py_request_order_status<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        order_type: OrderType,
        time_in_force: TimeInForce,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .request_order_status(
                    account_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    order_side,
                    order_type,
                    time_in_force,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    /// Requests open orders from Ax and parses them to Nautilus `OrderStatusReport`.
    ///
    /// Requires instruments to be cached for parsing order details.
    ///
    /// The `cid_resolver` parameter is an optional function that resolves a `cid` (u64)
    /// to a `ClientOrderId`. This is needed for correlating orders submitted via WebSocket.
    #[pyo3(name = "request_order_status_reports")]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(account_id, None::<fn(u64) -> Option<ClientOrderId>>)
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

    /// Requests fills from Ax and parses them to Nautilus `FillReport`.
    ///
    /// Requires instruments to be cached for parsing fill details.
    #[pyo3(name = "request_fill_reports")]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(account_id)
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

    /// Requests positions from Ax and parses them to Nautilus `PositionStatusReport`.
    ///
    /// Requires instruments to be cached for parsing position details.
    #[pyo3(name = "request_position_reports")]
    fn py_request_position_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_reports(account_id)
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

    #[pyo3(name = "preview_aggressive_limit_order")]
    fn py_preview_aggressive_limit_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        quantity: Quantity,
        side: OrderSide,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = instrument_id.symbol.inner();
        let ax_side = AxOrderSide::try_from(side).map_err(to_pyvalue_err)?;
        let qty_contracts = quantity_to_contracts(quantity).map_err(to_pyvalue_err)?;

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let request = PreviewAggressiveLimitOrderRequest::new(symbol, qty_contracts, ax_side);
            let response = client
                .inner
                .preview_aggressive_limit_order(&request)
                .await
                .map_err(to_pyvalue_err)?;

            let price = response
                .limit_price
                .map(|p| Price::from(p.to_string().as_str()));

            Python::attach(|py| price.into_py_any(py))
        })
    }
}

impl From<AxHttpError> for PyErr {
    fn from(error: AxHttpError) -> Self {
        to_pyvalue_err(error)
    }
}
