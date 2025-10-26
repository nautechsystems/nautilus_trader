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

//! Python bindings for the BitMEX HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{ContingencyType, OrderSide, OrderType, TimeInForce, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, OrderListId, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::http::{client::BitmexHttpClient, error::BitmexHttpError};

#[pymethods]
impl BitmexHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None, testnet=false, timeout_secs=None, max_retries=None, retry_delay_ms=None, retry_delay_max_ms=None, recv_window_ms=None, max_requests_per_second=None, max_requests_per_minute=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<&str>,
        api_secret: Option<&str>,
        base_url: Option<&str>,
        testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        recv_window_ms: Option<u64>,
        max_requests_per_second: Option<u32>,
        max_requests_per_minute: Option<u32>,
    ) -> PyResult<Self> {
        let timeout = timeout_secs.or(Some(60));

        // If credentials not provided, try to load from environment
        let (final_api_key, final_api_secret) = if api_key.is_none() && api_secret.is_none() {
            // Choose environment variables based on testnet flag
            let (key_var, secret_var) = if testnet {
                ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
            } else {
                ("BITMEX_API_KEY", "BITMEX_API_SECRET")
            };

            let env_key = std::env::var(key_var).ok();
            let env_secret = std::env::var(secret_var).ok();
            (env_key, env_secret)
        } else {
            (api_key.map(String::from), api_secret.map(String::from))
        };

        Self::new(
            base_url.map(String::from),
            final_api_key,
            final_api_secret,
            testnet,
            timeout,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            recv_window_ms,
            max_requests_per_second,
            max_requests_per_minute,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "base_url")]
    #[must_use]
    pub fn py_base_url(&self) -> &str {
        self.base_url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> Option<&str> {
        self.api_key()
    }

    #[pyo3(name = "update_position_leverage")]
    fn py_update_position_leverage<'py>(
        &self,
        py: Python<'py>,
        _symbol: String,
        _leverage: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Call the leverage update method once it's implemented
            // let report = client.update_position_leverage(&symbol, leverage)
            //     .await
            //     .map_err(to_pyvalue_err)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                // report.into_py_any(py).map_err(to_pyvalue_err)
                Ok(py.None())
            })
        })
    }

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

            Python::attach(|py| match instrument {
                Some(inst) => instrument_any_to_pyobject(py, inst),
                None => Ok(py.None()),
            })
        })
    }

    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        active_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(active_only)
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
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|trade| trade.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_bars")]
    #[pyo3(signature = (bar_type, start=None, end=None, limit=None, partial=false))]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        partial: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(bar_type, start, end, limit, partial)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_bars: PyResult<Vec<_>> =
                    bars.into_iter().map(|bar| bar.into_py_any(py)).collect();
                let pylist = PyList::new(py, py_bars?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "query_order")]
    #[pyo3(signature = (instrument_id, client_order_id=None, venue_order_id=None))]
    fn py_query_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match client
                .query_order(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(Some(report)) => Python::attach(|py| report.into_py_any(py)),
                Ok(None) => Ok(Python::attach(|py| py.None())),
                Err(e) => Err(to_pyvalue_err(e)),
            }
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (instrument_id=None, open_only=false, limit=None))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(instrument_id, open_only, limit)
                .await
                .map_err(to_pyvalue_err)?;

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
    #[pyo3(signature = (instrument_id=None, limit=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<InstrumentId>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(instrument_id, limit)
                .await
                .map_err(to_pyvalue_err)?;

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
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports()
                .await
                .map_err(to_pyvalue_err)?;

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
    #[pyo3(signature = (
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price = None,
        trigger_price = None,
        trigger_type = None,
        display_qty = None,
        post_only = false,
        reduce_only = false,
        order_list_id = None,
        contingency_type = None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        trigger_type: Option<TriggerType>,
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
        order_list_id: Option<OrderListId>,
        contingency_type: Option<ContingencyType>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .submit_order(
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    display_qty,
                    post_only,
                    reduce_only,
                    order_list_id,
                    contingency_type,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (instrument_id, client_order_id=None, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .cancel_order(instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_orders")]
    #[pyo3(signature = (instrument_id, client_order_ids=None, venue_order_ids=None))]
    fn py_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        client_order_ids: Option<Vec<ClientOrderId>>,
        venue_order_ids: Option<Vec<VenueOrderId>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .cancel_orders(instrument_id, client_order_ids, venue_order_ids)
                .await
                .map_err(to_pyvalue_err)?;

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

    #[pyo3(name = "cancel_all_orders")]
    #[pyo3(signature = (instrument_id, order_side))]
    fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        order_side: Option<OrderSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .cancel_all_orders(instrument_id, order_side)
                .await
                .map_err(to_pyvalue_err)?;

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

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
        quantity=None,
        price=None,
        trigger_price=None
    ))]
    #[allow(clippy::too_many_arguments)]
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
            let report = client
                .modify_order(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                    trigger_price,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(inst_any);
        Ok(())
    }

    #[pyo3(name = "cancel_all_requests")]
    fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    #[pyo3(name = "http_get_margin")]
    fn py_http_get_margin<'py>(
        &self,
        py: Python<'py>,
        currency: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let margin = client
                .http_get_margin(&currency)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                // Create a simple Python object with just the account field we need
                // We can expand this if more fields are needed
                let account = margin.account;
                account.into_py_any(py)
            })
        })
    }

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

            Python::attach(|py| account_state.into_py_any(py).map_err(to_pyvalue_err))
        })
    }

    #[pyo3(name = "submit_orders_bulk")]
    fn py_submit_orders_bulk<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _client = self.clone();

        // Convert Python objects to PostOrderParams
        let _params = Python::attach(|_py| {
            orders
                .into_iter()
                .map(|obj| {
                    // Extract order parameters from Python dict
                    // This is a placeholder - actual implementation would need proper conversion
                    Ok(obj)
                })
                .collect::<PyResult<Vec<_>>>()
        })?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Call the bulk order method once it's implemented
            // let reports = client.submit_orders_bulk(params).await.map_err(to_pyvalue_err)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let py_list = PyList::new(py, Vec::<Py<PyAny>>::new())?;
                // for report in reports {
                //     py_list.append(report.into_py_any(py)?)?;
                // }
                Ok(py_list.into())
            })
        })
    }

    #[pyo3(name = "modify_orders_bulk")]
    fn py_modify_orders_bulk<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _client = self.clone();

        // Convert Python objects to PutOrderParams
        let _params = Python::attach(|_py| {
            orders
                .into_iter()
                .map(|obj| {
                    // Extract order parameters from Python dict
                    // This is a placeholder - actual implementation would need proper conversion
                    Ok(obj)
                })
                .collect::<PyResult<Vec<_>>>()
        })?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Call the bulk amend method once it's implemented
            // let reports = client.modify_orders_bulk(params).await.map_err(to_pyvalue_err)?;

            Python::attach(|py| -> PyResult<Py<PyAny>> {
                let py_list = PyList::new(py, Vec::<Py<PyAny>>::new())?;
                // for report in reports {
                //     py_list.append(report.into_py_any(py)?)?;
                // }
                Ok(py_list.into())
            })
        })
    }

    #[pyo3(name = "http_get_server_time")]
    fn py_http_get_server_time<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timestamp = client
                .http_get_server_time()
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| timestamp.into_py_any(py))
        })
    }
}

impl From<BitmexHttpError> for PyErr {
    fn from(error: BitmexHttpError) -> Self {
        match error {
            // Runtime/operational errors
            BitmexHttpError::Canceled(msg) => to_pyruntime_err(format!("Request canceled: {msg}")),
            BitmexHttpError::NetworkError(msg) => to_pyruntime_err(format!("Network error: {msg}")),
            BitmexHttpError::UnexpectedStatus { status, body } => {
                to_pyruntime_err(format!("Unexpected HTTP status code {status}: {body}"))
            }
            // Validation/configuration errors
            BitmexHttpError::MissingCredentials => {
                to_pyvalue_err("Missing credentials for authenticated request")
            }
            BitmexHttpError::ValidationError(msg) => {
                to_pyvalue_err(format!("Parameter validation error: {msg}"))
            }
            BitmexHttpError::JsonError(msg) => to_pyvalue_err(format!("JSON error: {msg}")),
            BitmexHttpError::BuildError(e) => to_pyvalue_err(format!("Build error: {e}")),
            BitmexHttpError::BitmexError {
                error_name,
                message,
            } => to_pyvalue_err(format!("BitMEX error {error_name}: {message}")),
        }
    }
}
