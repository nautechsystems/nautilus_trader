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

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    identifiers::{AccountId, ClientOrderId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{IntoPyObjectExt, prelude::*, types::PyList};
use rust_decimal::Decimal;

use crate::http::{client::AxHttpClient, error::AxHttpError};

#[pymethods]
impl AxHttpClient {
    #[new]
    #[pyo3(signature = (
        base_url=None,
        orders_base_url=None,
        timeout_secs=None,
        max_retries=None,
        retry_delay_ms=None,
        retry_delay_max_ms=None,
        proxy_url=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_new(
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
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

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    #[pyo3(signature = (
        api_key,
        api_secret,
        base_url=None,
        orders_base_url=None,
        timeout_secs=None,
        max_retries=None,
        retry_delay_ms=None,
        retry_delay_max_ms=None,
        proxy_url=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_with_credentials(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        orders_base_url: Option<String>,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
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

    #[getter]
    #[pyo3(name = "base_url")]
    #[must_use]
    pub fn py_base_url(&self) -> &str {
        self.base_url()
    }

    #[getter]
    #[pyo3(name = "api_key_masked")]
    #[must_use]
    pub fn py_api_key_masked(&self) -> String {
        self.api_key_masked()
    }

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Caches a single instrument for use in parsing responses.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument cannot be converted from Python.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Authenticates with Ax and stores the session token for subsequent requests.
    ///
    /// Returns the session token string.
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
    /// Credentials are resolved in the following order:
    /// 1. Stored credentials (from `with_credentials` constructor)
    /// 2. Environment variables (`AX_API_KEY` and `AX_API_SECRET`)
    ///
    /// Returns the session token string.
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

    /// Request all instruments from Ax.
    ///
    /// Returns a list of instrument definitions.
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
                let pylist = PyList::new(py, py_instruments?)
                    .unwrap()
                    .into_any()
                    .unbind();
                Ok(pylist)
            })
        })
    }

    /// Request account state from Ax.
    ///
    /// Returns an `AccountState` with current balances and margins.
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

    /// Request order status reports from Ax.
    ///
    /// Returns a list of `OrderStatusReport` for all open orders.
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
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Request fill reports from Ax.
    ///
    /// Returns a list of `FillReport` for recent fills.
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
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }

    /// Request position reports from Ax.
    ///
    /// Returns a list of `PositionStatusReport` for current positions.
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
                let pylist = PyList::new(py, py_reports?).unwrap().into_any().unbind();
                Ok(pylist)
            })
        })
    }
}

impl From<AxHttpError> for PyErr {
    fn from(error: AxHttpError) -> Self {
        to_pyvalue_err(error)
    }
}
