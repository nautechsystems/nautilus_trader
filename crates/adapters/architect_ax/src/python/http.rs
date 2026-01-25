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
use nautilus_model::identifiers::AccountId;
use pyo3::{IntoPyObjectExt, prelude::*, types::PyList};

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

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
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
                .request_order_status_reports(account_id)
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
