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

//! Python bindings for the Bybit HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::{
    common::enums::{BybitMarginMode, BybitPositionMode, BybitProductType},
    http::{client::BybitHttpClient, error::BybitHttpError},
};

#[pymethods]
impl BybitHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None, demo=false, testnet=false, timeout_secs=None, max_retries=None, retry_delay_ms=None, retry_delay_max_ms=None, recv_window_ms=None, proxy_url=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        demo: bool,
        testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        recv_window_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        let timeout = timeout_secs.or(Some(60));

        // Try to get credentials from parameters or environment variables
        // Priority: demo > testnet > mainnet
        let (api_key_env, api_secret_env) = if demo {
            ("BYBIT_DEMO_API_KEY", "BYBIT_DEMO_API_SECRET")
        } else if testnet {
            ("BYBIT_TESTNET_API_KEY", "BYBIT_TESTNET_API_SECRET")
        } else {
            ("BYBIT_API_KEY", "BYBIT_API_SECRET")
        };

        let key = api_key.or_else(|| std::env::var(api_key_env).ok());
        let secret = api_secret.or_else(|| std::env::var(api_secret_env).ok());

        if let (Some(k), Some(s)) = (key, secret) {
            Self::with_credentials(
                k,
                s,
                base_url,
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                recv_window_ms,
                proxy_url,
            )
            .map_err(to_pyvalue_err)
        } else {
            Self::new(
                base_url,
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
                recv_window_ms,
                proxy_url,
            )
            .map_err(to_pyvalue_err)
        }
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
        self.credential().map(|c| c.api_key()).map(|u| u.as_str())
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> Option<String> {
        self.credential().map(|c| c.api_key_masked())
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

    #[pyo3(name = "set_use_spot_position_reports")]
    fn py_set_use_spot_position_reports(&self, use_spot_position_reports: bool) {
        self.set_use_spot_position_reports(use_spot_position_reports);
    }

    #[pyo3(name = "set_margin_mode")]
    fn py_set_margin_mode<'py>(
        &self,
        py: Python<'py>,
        margin_mode: BybitMarginMode,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .set_margin_mode(margin_mode)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "get_account_details")]
    fn py_get_account_details<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_account_details().await.map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let account_details = Py::new(py, response.result)?;
                Ok(account_details.into_any())
            })
        })
    }

    #[pyo3(name = "set_leverage")]
    #[pyo3(signature = (product_type, symbol, buy_leverage, sell_leverage))]
    fn py_set_leverage<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        symbol: String,
        buy_leverage: String,
        sell_leverage: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .set_leverage(product_type, &symbol, &buy_leverage, &sell_leverage)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "switch_mode")]
    #[pyo3(signature = (product_type, mode, symbol=None, coin=None))]
    fn py_switch_mode<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        mode: BybitPositionMode,
        symbol: Option<String>,
        coin: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .switch_mode(product_type, mode, symbol, coin)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "get_spot_borrow_amount")]
    fn py_get_spot_borrow_amount<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let borrow_amount = client
                .get_spot_borrow_amount(&coin)
                .await
                .map_err(to_pyvalue_err)?;

            Ok(borrow_amount)
        })
    }

    #[pyo3(name = "borrow_spot")]
    #[pyo3(signature = (coin, amount))]
    fn py_borrow_spot<'py>(
        &self,
        py: Python<'py>,
        coin: String,
        amount: Quantity,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .borrow_spot(&coin, amount)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "repay_spot_borrow")]
    #[pyo3(signature = (coin, amount=None))]
    fn py_repay_spot_borrow<'py>(
        &self,
        py: Python<'py>,
        coin: String,
        amount: Option<Quantity>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .repay_spot_borrow(&coin, amount)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(py.None()))
        })
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (product_type, symbol=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        symbol: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(product_type, symbol)
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

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        account_id,
        product_type,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price = None,
        reduce_only = false,
        is_leverage = false
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        reduce_only: bool,
        is_leverage: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .submit_order(
                    account_id,
                    product_type,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    reduce_only,
                    is_leverage,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        account_id,
        product_type,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
        quantity=None,
        price=None
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        quantity: Option<Quantity>,
        price: Option<Price>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .modify_order(
                    account_id,
                    product_type,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (account_id, product_type, instrument_id, client_order_id=None, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .cancel_order(
                    account_id,
                    product_type,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .cancel_all_orders(account_id, product_type, instrument_id)
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

    #[pyo3(name = "query_order")]
    #[pyo3(signature = (account_id, product_type, instrument_id, client_order_id=None, venue_order_id=None))]
    fn py_query_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            match client
                .query_order(
                    account_id,
                    product_type,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
            {
                Ok(Some(report)) => Python::attach(|py| report.into_py_any(py)),
                Ok(None) => Ok(Python::attach(|py| py.None())),
                Err(e) => Err(to_pyvalue_err(e)),
            }
        })
    }

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (product_type, instrument_id, limit=None))]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        instrument_id: InstrumentId,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client
                .request_trades(product_type, instrument_id, limit)
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
    #[pyo3(signature = (product_type, bar_type, start=None, end=None, limit=None, timestamp_on_close=true))]
    #[allow(clippy::too_many_arguments)]
    fn py_request_bars<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        bar_type: BarType,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
        timestamp_on_close: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let bars = client
                .request_bars(
                    product_type,
                    bar_type,
                    start,
                    end,
                    limit,
                    timestamp_on_close,
                )
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

    #[pyo3(name = "request_fee_rates")]
    #[pyo3(signature = (product_type, symbol=None, base_coin=None))]
    fn py_request_fee_rates<'py>(
        &self,
        py: Python<'py>,
        product_type: BybitProductType,
        symbol: Option<String>,
        base_coin: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let fee_rates = client
                .request_fee_rates(product_type, symbol, base_coin)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let py_fee_rates: PyResult<Vec<_>> = fee_rates
                    .into_iter()
                    .map(|rate| Py::new(py, rate))
                    .collect();
                let pylist = PyList::new(py, py_fee_rates?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    #[pyo3(name = "request_account_state")]
    fn py_request_account_state<'py>(
        &self,
        py: Python<'py>,
        account_type: crate::common::enums::BybitAccountType,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let account_state = client
                .request_account_state(account_type, account_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| account_state.into_py_any(py))
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, product_type, instrument_id=None, open_only=false, start=None, end=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        open_only: bool,
        start: Option<DateTime<Utc>>,
        end: Option<DateTime<Utc>>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(
                    account_id,
                    product_type,
                    instrument_id,
                    open_only,
                    start,
                    end,
                    limit,
                )
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
    #[pyo3(signature = (account_id, product_type, instrument_id=None, start=None, end=None, limit=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
        start: Option<i64>,
        end: Option<i64>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(account_id, product_type, instrument_id, start, end, limit)
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
    #[pyo3(signature = (account_id, product_type, instrument_id=None))]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        product_type: BybitProductType,
        instrument_id: Option<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports(account_id, product_type, instrument_id)
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
}

impl From<BybitHttpError> for PyErr {
    fn from(error: BybitHttpError) -> Self {
        match error {
            // Runtime/operational errors
            BybitHttpError::Canceled(msg) => to_pyruntime_err(format!("Request canceled: {msg}")),
            BybitHttpError::NetworkError(msg) => to_pyruntime_err(format!("Network error: {msg}")),
            BybitHttpError::UnexpectedStatus { status, body } => {
                to_pyruntime_err(format!("Unexpected HTTP status code {status}: {body}"))
            }
            // Validation/configuration errors
            BybitHttpError::MissingCredentials => {
                to_pyvalue_err("Missing credentials for authenticated request")
            }
            BybitHttpError::ValidationError(msg) => {
                to_pyvalue_err(format!("Parameter validation error: {msg}"))
            }
            BybitHttpError::JsonError(msg) => to_pyvalue_err(format!("JSON error: {msg}")),
            BybitHttpError::BuildError(e) => to_pyvalue_err(format!("Build error: {e}")),
            BybitHttpError::BybitError {
                error_code,
                message,
            } => to_pyvalue_err(format!("Bybit error {error_code}: {message}")),
        }
    }
}
