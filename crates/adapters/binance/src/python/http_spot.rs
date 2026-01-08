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

//! Python bindings for the Binance Spot HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::instrument_any_to_pyobject,
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyList};

use crate::{common::enums::BinanceEnvironment, spot::http::client::BinanceSpotHttpClient};

#[pymethods]
impl BinanceSpotHttpClient {
    #[new]
    #[pyo3(signature = (
        environment=BinanceEnvironment::Mainnet,
        api_key=None,
        api_secret=None,
        base_url=None,
        recv_window=None,
        timeout_secs=None,
        proxy_url=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        recv_window: Option<u64>,
        timeout_secs: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(
            environment,
            api_key,
            api_secret,
            base_url,
            recv_window,
            timeout_secs,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "schema_id")]
    #[must_use]
    pub fn py_schema_id(&self) -> u16 {
        Self::schema_id()
    }

    #[getter]
    #[pyo3(name = "schema_version")]
    #[must_use]
    pub fn py_schema_version(&self) -> u16 {
        Self::schema_version()
    }

    #[pyo3(name = "ping")]
    fn py_ping<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.ping().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "server_time")]
    fn py_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timestamp = client.server_time().await.map_err(to_pyvalue_err)?;
            Python::attach(|py| Ok(timestamp.into_pyobject(py)?.into_any().unbind()))
        })
    }

    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client.request_instruments().await.map_err(to_pyvalue_err)?;

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

    #[pyo3(name = "request_trades", signature = (instrument_id, limit=None))]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trades(instrument_id, limit)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|tick| tick.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_order_status")]
    #[pyo3(signature = (
        account_id,
        instrument_id,
        venue_order_id=None,
        client_order_id=None,
    ))]
    fn py_request_order_status<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .request_order_status(account_id, instrument_id, venue_order_id, client_order_id)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (
        account_id,
        instrument_id=None,
        start=None,
        end=None,
        open_only=false,
        limit=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
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
                    instrument_id,
                    start,
                    end,
                    open_only,
                    limit,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> =
                    reports.into_iter().map(|r| r.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_reports?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (
        account_id,
        instrument_id,
        venue_order_id=None,
        start=None,
        end=None,
        limit=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(account_id, instrument_id, venue_order_id, start, end, limit)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                let py_reports: PyResult<Vec<_>> =
                    reports.into_iter().map(|r| r.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_reports?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (
        bar_type,
        start=None,
        end=None,
        limit=None,
    ))]
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
                let py_bars: PyResult<Vec<_>> =
                    bars.into_iter().map(|b| b.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_bars?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "submit_order", signature = (account_id, instrument_id, client_order_id, order_side, order_type, quantity, time_in_force, price=None, trigger_price=None, post_only=false))]
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
                    post_only,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "modify_order", signature = (account_id, instrument_id, venue_order_id, client_order_id, order_side, order_type, quantity, time_in_force, price=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .modify_order(
                    account_id,
                    instrument_id,
                    venue_order_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                )
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (
        instrument_id,
        venue_order_id=None,
        client_order_id=None,
    ))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
        client_order_id: Option<ClientOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_id = client
                .cancel_order(instrument_id, venue_order_id, client_order_id)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| order_id.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_ids = client
                .cancel_all_orders(instrument_id)
                .await
                .map_err(to_pyvalue_err)?;
            Python::attach(|py| {
                let py_ids: PyResult<Vec<_>> =
                    order_ids.into_iter().map(|id| id.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_ids?)?.into_any().unbind();
                Ok(pylist)
            })
        })
    }
}
