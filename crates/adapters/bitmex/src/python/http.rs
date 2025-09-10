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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    enums::{OrderSide, OrderType, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::http::client::BitmexHttpClient;

#[pymethods]
impl BitmexHttpClient {
    #[new]
    #[pyo3(signature = (api_key=None, api_secret=None, base_url=None, testnet=false, timeout_secs=None, max_retries=None, retry_delay_ms=None, retry_delay_max_ms=None))]
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
    ) -> PyResult<Self> {
        let timeout = timeout_secs.or(Some(60));

        // Try to use with_credentials if we have any credentials or need env vars
        if api_key.is_none() && api_secret.is_none() && !testnet && base_url.is_none() {
            // Try to load from environment
            match Self::with_credentials(
                None,
                None,
                base_url.map(String::from),
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            ) {
                Ok(client) => Ok(client),
                Err(_) => {
                    // Fall back to unauthenticated client
                    Self::new(
                        base_url.map(String::from),
                        None,
                        None,
                        testnet,
                        timeout,
                        max_retries,
                        retry_delay_ms,
                        retry_delay_max_ms,
                    )
                    .map_err(to_pyvalue_err)
                }
            }
        } else {
            Self::new(
                base_url.map(String::from),
                api_key.map(String::from),
                api_secret.map(String::from),
                testnet,
                timeout,
                max_retries,
                retry_delay_ms,
                retry_delay_max_ms,
            )
            .map_err(to_pyvalue_err)
        }
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

            Python::with_gil(|py| -> PyResult<PyObject> {
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

            Python::with_gil(|py| match instrument {
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

    #[pyo3(name = "request_trades")]
    #[pyo3(signature = (instrument_id, limit=None))]
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

            Python::with_gil(|py| {
                let py_trades: PyResult<Vec<_>> = trades
                    .into_iter()
                    .map(|trade| trade.into_py_any(py))
                    .collect();
                let pylist = PyList::new(py, py_trades?).unwrap().into_any().unbind();
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
                Ok(Some(report)) => Python::with_gil(|py| report.into_py_any(py)),
                Ok(None) => Ok(Python::with_gil(|py| py.None())),
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

            Python::with_gil(|py| {
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

            Python::with_gil(|py| {
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

            Python::with_gil(|py| {
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
        display_qty = None,
        post_only = false,
        reduce_only = false
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
        display_qty: Option<Quantity>,
        post_only: bool,
        reduce_only: bool,
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
                    display_qty,
                    post_only,
                    reduce_only,
                )
                .await
                .map_err(to_pyvalue_err)?;

            Python::with_gil(|py| report.into_py_any(py))
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

            Python::with_gil(|py| report.into_py_any(py))
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

            Python::with_gil(|py| {
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

            Python::with_gil(|py| {
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

            Python::with_gil(|py| report.into_py_any(py))
        })
    }

    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&mut self, py: Python, instrument: PyObject) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.add_instrument(inst_any);
        Ok(())
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

            Python::with_gil(|py| {
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

            Python::with_gil(|py| account_state.into_py_any(py).map_err(to_pyvalue_err))
        })
    }

    #[pyo3(name = "submit_orders_bulk")]
    fn py_submit_orders_bulk<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<PyObject>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _client = self.clone();

        // Convert Python objects to PostOrderParams
        let _params = Python::with_gil(|_py| {
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

            Python::with_gil(|py| -> PyResult<PyObject> {
                let py_list = PyList::new(py, Vec::<PyObject>::new())?;
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
        orders: Vec<PyObject>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let _client = self.clone();

        // Convert Python objects to PutOrderParams
        let _params = Python::with_gil(|_py| {
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

            Python::with_gil(|py| -> PyResult<PyObject> {
                let py_list = PyList::new(py, Vec::<PyObject>::new())?;
                // for report in reports {
                //     py_list.append(report.into_py_any(py)?)?;
                // }
                Ok(py_list.into())
            })
        })
    }
}
