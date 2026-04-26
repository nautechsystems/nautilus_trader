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

//! Python bindings for the Kraken Spot HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::{
    nanos::UnixNanos,
    python::{to_pyruntime_err, to_pyvalue_err},
};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList},
};
use rust_decimal::Decimal;

use crate::{
    common::{credential::KrakenCredential, enums::KrakenEnvironment},
    http::KrakenSpotHttpClient,
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KrakenSpotHttpClient {
    /// High-level HTTP client for the Kraken Spot REST API.
    ///
    /// This client wraps the raw client and provides Nautilus domain types.
    /// It maintains an instrument cache and uses it to parse venue responses
    /// into Nautilus domain objects.
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None, demo=false, timeout_secs=60, max_retries=None, retry_delay_ms=None, retry_delay_max_ms=None, proxy_url=None, max_requests_per_second=5))]
    #[expect(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        demo: bool,
        timeout_secs: u64,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
        max_requests_per_second: u32,
    ) -> PyResult<Self> {
        let environment = if demo {
            KrakenEnvironment::Demo
        } else {
            KrakenEnvironment::Mainnet
        };

        if let Some(cred) = KrakenCredential::resolve_spot(api_key, api_secret) {
            let (k, s) = cred.into_parts();
            Self::with_credentials(
                k,
                s,
                environment,
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )
            .map_err(to_pyvalue_err)
        } else {
            Self::new(
                environment,
                base_url,
                timeout_secs,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
                max_requests_per_second,
            )
            .map_err(to_pyvalue_err)
        }
    }

    #[getter]
    #[pyo3(name = "base_url")]
    #[must_use]
    pub fn py_base_url(&self) -> String {
        self.inner.base_url().to_string()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> Option<&str> {
        self.inner.credential().map(|c| c.api_key())
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.inner.credential().map(|c| c.api_key_masked())
    }

    /// Caches an instrument for symbol lookup.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    /// Cancels all pending HTTP requests.
    #[pyo3(name = "cancel_all_requests")]
    fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Sets whether to generate position reports from wallet balances for SPOT instruments.
    #[pyo3(name = "set_use_spot_position_reports")]
    fn py_set_use_spot_position_reports(&self, value: bool) {
        self.set_use_spot_position_reports(value);
    }

    /// Sets the quote currency filter for spot position reports.
    #[pyo3(name = "set_spot_positions_quote_currency")]
    fn py_set_spot_positions_quote_currency(&self, currency: &str) {
        self.set_spot_positions_quote_currency(currency);
    }

    #[pyo3(name = "get_server_time")]
    fn py_get_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let server_time = client
                .inner
                .get_server_time()
                .await
                .map_err(to_pyruntime_err)?;

            let json_string = serde_json::to_string(&server_time)
                .map_err(|e| to_pyruntime_err(format!("Failed to serialize response: {e}")))?;

            Ok(json_string)
        })
    }

    /// Requests tradable instruments from Kraken.
    ///
    /// When `pairs` is `None` (loading all), also fetches tokenized asset pairs
    /// (xStocks) and merges them with the default currency pairs.
    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (pairs=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        pairs: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(pairs)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_instruments: PyResult<Vec<_>> = instruments
                    .into_iter()
                    .map(|inst| instrument_any_to_pyobject(py, inst))
                    .collect();
                let pylist = PyList::new(py, py_instruments?).unwrap();
                Ok(pylist.unbind())
            })
        })
    }

    /// Requests the current market status for Kraken Spot instruments.
    ///
    /// Fetches both regular and tokenized asset pairs. The call returns an error if
    /// either fetch fails so callers can avoid emitting partial snapshots that would
    /// otherwise cause the missing tokenized symbols to be diffed as removed.
    #[pyo3(name = "request_instrument_statuses")]
    #[pyo3(signature = (pairs=None))]
    fn py_request_instrument_statuses<'py>(
        &self,
        py: Python<'py>,
        pairs: Option<Vec<String>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let statuses = client
                .request_instrument_statuses(pairs)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let dict = PyDict::new(py);
                for (instrument_id, action) in statuses {
                    dict.set_item(
                        instrument_id.into_bound_py_any(py)?,
                        action.into_bound_py_any(py)?,
                    )?;
                }
                Ok(dict.into_any().unbind())
            })
        })
    }

    /// Requests historical trades for an instrument.
    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, start=None, end=None, limit=None))]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trades(instrument_id, start, end, limit)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|trade| trade.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests an order book snapshot for an instrument.
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
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| book.into_py_any(py))
        })
    }

    /// Requests historical bars/OHLC data for an instrument.
    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start, end, limit)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_bars: PyResult<Vec<_>> =
                    bars.into_iter().map(|bar| bar.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_bars?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests account state (balances) from Kraken.
    ///
    /// Returns an `AccountState` containing all currency balances.
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
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| account_state.into_pyobject(py).map(|o| o.unbind()))
        })
    }

    /// Requests order status reports from Kraken.
    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None, open_only=false))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        open_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(account_id, instrument_id, start, end, open_only)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests fill/trade reports from Kraken.
    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(account_id, instrument_id, start, end)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Requests position status reports for SPOT instruments.
    ///
    /// Returns wallet balances as position reports if `use_spot_position_reports` is enabled.
    /// Otherwise returns an empty vector (spot traditionally has no "positions").
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
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> = reports
                    .into_iter()
                    .map(|report| report.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Submits a new order to the Kraken Spot exchange.
    ///
    /// Returns the venue order ID on success. WebSocket handles all execution events.
    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (account_id, instrument_id, client_order_id, order_side, order_type, quantity, time_in_force, expire_time=None, price=None, trigger_price=None, trigger_type=None, trailing_offset=None, limit_offset=None, reduce_only=false, post_only=false, quote_quantity=false, display_qty=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<u64>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        trailing_offset: Option<String>,
        limit_offset: Option<String>,
        reduce_only: bool,
        post_only: bool,
        quote_quantity: bool,
        display_qty: Option<Quantity>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let expire_time = expire_time.map(UnixNanos::from);
        let trailing_offset = trailing_offset
            .map(|s| {
                Decimal::from_str_exact(&s)
                    .map_err(|e| to_pyvalue_err(format!("invalid trailing_offset: {e}")))
            })
            .transpose()?;
        let limit_offset = limit_offset
            .map(|s| {
                Decimal::from_str_exact(&s)
                    .map_err(|e| to_pyvalue_err(format!("invalid limit_offset: {e}")))
            })
            .transpose()?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let venue_order_id = client
                .submit_order(
                    account_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    expire_time,
                    price,
                    trigger_price,
                    trigger_type,
                    trailing_offset,
                    limit_offset,
                    reduce_only,
                    post_only,
                    quote_quantity,
                    display_qty,
                )
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| venue_order_id.into_pyobject(py).map(|o| o.unbind()))
        })
    }

    /// Cancels an order on the Kraken Spot exchange.
    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (account_id, instrument_id, client_order_id=None, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(account_id, instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .inner
                .cancel_all_orders()
                .await
                .map_err(to_pyruntime_err)?;

            Ok(response.count)
        })
    }

    /// Cancels multiple orders on the Kraken Spot exchange (batched, max 50 per request).
    #[pyo3(name = "cancel_orders_batch")]
    fn py_cancel_orders_batch<'py>(
        &self,
        py: Python<'py>,
        venue_order_ids: Vec<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_orders_batch(venue_order_ids)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Modifies an existing order on the Kraken Spot exchange using atomic amend.
    ///
    /// Uses the AmendOrder endpoint which modifies the order in-place,
    /// keeping the same order ID and queue position.
    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (instrument_id, client_order_id=None, venue_order_id=None, quantity=None, price=None, trigger_price=None))]
    #[expect(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
        trigger_price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let new_venue_order_id = client
                .modify_order(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                    trigger_price,
                )
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| new_venue_order_id.into_pyobject(py).map(|o| o.unbind()))
        })
    }
}

// Separate block to avoid pyo3_stub_gen trait bound issues with batch-order tuples.
// Stub is maintained manually in nautilus_pyo3.pyi.
#[pymethods]
impl KrakenSpotHttpClient {
    /// Submits multiple orders to the Kraken Spot exchange.
    ///
    /// Automatically groups orders by pair and chunks batch requests at the venue
    /// limit. Single-order groups fall back to `AddOrder`.
    #[pyo3(name = "submit_orders_batch")]
    #[expect(clippy::type_complexity)]
    fn py_submit_orders_batch<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<(
            InstrumentId,
            ClientOrderId,
            OrderSide,
            OrderType,
            Quantity,
            TimeInForce,
            Option<Price>,
            Option<Price>,
            Option<TriggerType>,
            bool,
            bool,
            Option<Quantity>,
        )>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let expanded_orders = orders
            .into_iter()
            .map(
                |(
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    post_only,
                    quote_quantity,
                    display_qty,
                )| {
                    (
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        None,
                        price,
                        trigger_price,
                        trigger_type,
                        None,
                        None,
                        false,
                        post_only,
                        quote_quantity,
                        display_qty,
                    )
                },
            )
            .collect();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_orders_batch(expanded_orders)
                .await
                .map_err(to_pyruntime_err)
        })
    }
}
