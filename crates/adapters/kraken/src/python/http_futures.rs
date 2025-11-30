// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for the Kraken Futures HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{BarType, Data},
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::{common::enums::KrakenEnvironment, http::KrakenFuturesHttpClient};

#[pymethods]
impl KrakenFuturesHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None, testnet=false, timeout_secs=None, max_retries=None, retry_delay_ms=None, retry_delay_max_ms=None, proxy_url=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        let timeout = timeout_secs.or(Some(60));

        let environment = if testnet {
            KrakenEnvironment::Testnet
        } else {
            KrakenEnvironment::Mainnet
        };

        if let (Some(k), Some(s)) = (api_key, api_secret) {
            Self::with_credentials(
                k,
                s,
                environment,
                base_url,
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
            )
            .map_err(to_pyvalue_err)
        } else {
            Self::from_env(
                environment,
                base_url,
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                proxy_url,
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

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    #[pyo3(name = "cancel_all_requests")]
    fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

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
                .map_err(to_pyruntime_err)?;

            Ok(mark_price)
        })
    }

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
                .map_err(to_pyruntime_err)?;

            Ok(index_price)
        })
    }

    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments()
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
                let py_trades: Vec<_> = trades
                    .into_iter()
                    .map(|trade| data_to_pycapsule(py, Data::Trade(trade)))
                    .collect();
                let pylist = PyList::new(py, py_trades).unwrap();
                Ok(pylist.unbind())
            })
        })
    }

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
                let py_bars: Vec<_> = bars
                    .into_iter()
                    .map(|bar| data_to_pycapsule(py, Data::Bar(bar)))
                    .collect();
                let pylist = PyList::new(py, py_bars).unwrap();
                Ok(pylist.unbind())
            })
        })
    }

    #[pyo3(name = "request_bars_with_tick_type")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, tick_type=None))]
    fn py_request_bars_with_tick_type<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u64>,
        tick_type: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let tick_type_ref = tick_type.as_deref();
            let bars = client
                .request_bars_with_tick_type(bar_type, start, end, limit, tick_type_ref)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| {
                let py_bars: Vec<_> = bars
                    .into_iter()
                    .map(|bar| data_to_pycapsule(py, Data::Bar(bar)))
                    .collect();
                let pylist = PyList::new(py, py_bars).unwrap();
                Ok(pylist.unbind())
            })
        })
    }

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

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (account_id, instrument_id, client_order_id, order_side, order_type, quantity, time_in_force, price=None, trigger_price=None, reduce_only=false, post_only=false))]
    #[allow(clippy::too_many_arguments)]
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
        price: Option<Price>,
        trigger_price: Option<Price>,
        reduce_only: bool,
        post_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .submit_order(
                    account_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    reduce_only,
                    post_only,
                )
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| report.into_pyobject(py).map(|o| o.unbind()))
        })
    }

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
            let report = client
                .cancel_order(account_id, instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyruntime_err)?;

            Python::attach(|py| report.into_pyobject(py).map(|o| o.unbind()))
        })
    }

    #[pyo3(name = "cancel_all_orders")]
    #[pyo3(signature = (instrument_id=None))]
    fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let symbol = instrument_id.map(|id| id.symbol.to_string());
            let response = client
                .inner
                .cancel_all_orders(symbol)
                .await
                .map_err(to_pyruntime_err)?;

            Ok(response.cancel_status.cancelled_orders.len())
        })
    }
}
