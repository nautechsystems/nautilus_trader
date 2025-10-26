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

//! Python bindings exposing OKX HTTP helper functions and data conversions.

use chrono::{DateTime, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TriggerType},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{
    conversion::IntoPyObjectExt,
    prelude::*,
    types::{PyDict, PyList},
};

use crate::{
    common::enums::{OKXInstrumentType, OKXOrderStatus, OKXPositionMode, OKXTradeMode},
    http::{client::OKXHttpClient, error::OKXHttpError},
};

#[pymethods]
impl OKXHttpClient {
    #[new]
    #[pyo3(signature = (
        api_key=None,
        api_secret=None,
        api_passphrase=None,
        base_url=None,
        timeout_secs=None,
        max_retries=None,
        retry_delay_ms=None,
        retry_delay_max_ms=None,
        is_demo=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        is_demo: bool,
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
            is_demo,
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

    #[pyo3(name = "is_initialized")]
    #[must_use]
    pub const fn py_is_initialized(&self) -> bool {
        self.is_initialized()
    }

    #[pyo3(name = "get_cached_symbols")]
    #[must_use]
    pub fn py_get_cached_symbols(&self) -> Vec<String> {
        self.get_cached_symbols()
    }

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// # Errors
    ///
    /// Returns a Python exception if adding the instrument to the cache fails.
    #[pyo3(name = "add_instrument")]
    pub fn py_add_instrument(&mut self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.add_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Sets the position mode for the account.
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
            Ok(Python::attach(|py| py.None()))
        })
    }

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
            let instruments = client
                .request_instruments(instrument_type, instrument_family)
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
            Ok(Python::attach(|py| account_state.into_py_any_unwrap(py)))
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
                let pylist = PyList::new(py, trades.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
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
            Ok(Python::attach(|py| mark_price.into_py_any_unwrap(py)))
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
                .map_err(to_pyvalue_err)?;
            Ok(Python::attach(|py| index_price.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, start=None, end=None, open_only=false, limit=None))]
    #[allow(clippy::too_many_arguments)]
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

    #[pyo3(name = "request_algo_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, algo_id=None, algo_client_order_id=None, state=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
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

    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (account_id, instrument_type=None, instrument_id=None, start=None, end=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
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
        trigger_price,
        trigger_type=None,
        limit_price=None,
        reduce_only=None,
    ))]
    #[allow(clippy::too_many_arguments)]
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
        trigger_price: Price,
        trigger_type: Option<TriggerType>,
        limit_price: Option<Price>,
        reduce_only: Option<bool>,
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

    #[pyo3(name = "http_get_balance")]
    fn py_http_get_balance<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let accounts = client
                .inner
                .http_get_balance()
                .await
                .map_err(to_pyvalue_err)?;

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
