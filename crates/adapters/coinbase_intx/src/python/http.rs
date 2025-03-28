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

use chrono::{DateTime, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, serialization::to_dict_pyo3, to_pyvalue_err};
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, Symbol, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{prelude::*, types::PyList};

use crate::http::client::CoinbaseIntxHttpClient;

#[pymethods]
impl CoinbaseIntxHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, api_passphrase=None, base_url=None, timeout_secs=None))]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> PyResult<Self> {
        Self::with_credentials(api_key, api_secret, api_passphrase, base_url, timeout_secs)
            .map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "base_url")]
    pub fn py_base_url(&self) -> &str {
        self.base_url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    pub fn py_api_key(&self) -> Option<&str> {
        self.api_key()
    }

    #[pyo3(name = "is_initialized")]
    pub fn py_is_initialized(&self) -> bool {
        self.is_initialized()
    }

    #[pyo3(name = "get_cached_symbols")]
    pub fn py_get_cached_symbols(&self) -> Vec<String> {
        self.get_cached_symbols()
    }

    #[pyo3(name = "add_instrument")]
    pub fn py_add_instrument(&mut self, py: Python<'_>, instrument: PyObject) -> PyResult<()> {
        self.add_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "list_portfolios")]
    pub fn py_list_portfolios<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.list_portfolios().await.map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);

                for portfolio in response {
                    let dict = to_dict_pyo3(py, &portfolio)?;
                    py_list.append(dict)?;
                }

                Ok(py_list.into_any().unbind())
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

            Ok(Python::with_gil(|py| account_state.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_instruments")]
    fn py_request_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client.request_instruments().await.map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
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

    #[pyo3(name = "request_instrument")]
    fn py_request_instrument<'py>(
        &self,
        py: Python<'py>,
        symbol: Symbol,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instrument = client
                .request_instrument(&symbol)
                .await
                .map_err(to_pyvalue_err)?;

            Ok(Python::with_gil(|py| {
                instrument_any_to_pyobject(py, instrument)
                    .expect("Failed parsing instrument")
                    .into_py_any_unwrap(py)
            }))
        })
    }

    #[pyo3(name = "request_order_status_report")]
    fn py_request_order_status_report<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        venue_order_id: VenueOrderId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .request_order_status_report(account_id, venue_order_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| Ok(report.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, symbol))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        symbol: Symbol,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(account_id, symbol)
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "request_fill_reports")]
    #[pyo3(signature = (account_id, client_order_id=None, start=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        client_order_id: Option<ClientOrderId>,
        start: Option<DateTime<Utc>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(account_id, client_order_id, start)
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[pyo3(name = "request_position_status_report")]
    fn py_request_position_status_report<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        symbol: Symbol,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let report = client
                .request_position_status_report(account_id, symbol)
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| Ok(report.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_position_status_reports")]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_position_status_reports(account_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|t| t.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    #[allow(clippy::too_many_arguments)]
    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (account_id, symbol, client_order_id, order_type, order_side, quantity, time_in_force, expire_time=None, price=None, trigger_price=None, post_only=None, reduce_only=None))]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        symbol: Symbol,
        client_order_id: ClientOrderId,
        order_type: OrderType,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        expire_time: Option<DateTime<Utc>>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    account_id,
                    client_order_id,
                    symbol,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    expire_time,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        client_order_id: ClientOrderId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(account_id, client_order_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_orders")]
    #[pyo3(signature = (account_id, symbol, order_side=None))]
    fn py_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        symbol: Symbol,
        order_side: Option<OrderSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_orders(account_id, symbol, order_side)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (account_id, client_order_id, new_client_order_id, price=None, trigger_price=None, quantity=None))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        client_order_id: ClientOrderId,
        new_client_order_id: ClientOrderId,
        price: Option<Price>,
        trigger_price: Option<Price>,
        quantity: Option<Quantity>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    account_id,
                    client_order_id,
                    new_client_order_id,
                    price,
                    trigger_price,
                    quantity,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }
}
