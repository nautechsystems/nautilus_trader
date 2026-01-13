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

//! Python bindings for Deribit HTTP client.

use chrono::{DateTime, Utc};
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    python::instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
};
use pyo3::{conversion::IntoPyObjectExt, prelude::*, types::PyList};

use crate::http::{
    client::DeribitHttpClient,
    error::DeribitHttpError,
    models::{DeribitCurrency, DeribitInstrumentKind},
};

#[pymethods]
impl DeribitHttpClient {
    #[new]
    #[pyo3(signature = (
        api_key=None,
        api_secret=None,
        base_url=None,
        is_testnet=false,
        timeout_secs=None,
        max_retries=None,
        retry_delay_ms=None,
        retry_delay_max_ms=None,
        proxy_url=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    #[allow(unused_variables)]
    fn py_new(
        api_key: Option<String>,
        api_secret: Option<String>,
        base_url: Option<String>,
        is_testnet: bool,
        timeout_secs: Option<u64>,
        max_retries: Option<u32>,
        retry_delay_ms: Option<u64>,
        retry_delay_max_ms: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new_with_env(
            api_key,
            api_secret,
            is_testnet,
            timeout_secs,
            max_retries,
            retry_delay_ms,
            retry_delay_max_ms,
            proxy_url,
        )
        .map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "is_testnet")]
    #[must_use]
    pub fn py_is_testnet(&self) -> bool {
        self.is_testnet()
    }

    #[pyo3(name = "is_initialized")]
    #[must_use]
    pub fn py_is_initialized(&self) -> bool {
        self.is_cache_initialized()
    }

    /// # Errors
    ///
    /// Returns a Python exception if adding the instruments to the cache fails.
    #[pyo3(name = "cache_instruments")]
    pub fn py_cache_instruments(
        &self,
        py: Python<'_>,
        instruments: Vec<Py<PyAny>>,
    ) -> PyResult<()> {
        let instruments: Result<Vec<_>, _> = instruments
            .into_iter()
            .map(|inst| pyobject_to_instrument_any(py, inst))
            .collect();
        self.cache_instruments(instruments?);
        Ok(())
    }

    /// # Errors
    ///
    /// Returns a Python exception if adding the instrument to the cache fails.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instruments(vec![inst]);
        Ok(())
    }

    #[pyo3(name = "request_instruments")]
    #[pyo3(signature = (currency, kind=None))]
    fn py_request_instruments<'py>(
        &self,
        py: Python<'py>,
        currency: DeribitCurrency,
        kind: Option<DeribitInstrumentKind>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let instruments = client
                .request_instruments(currency, kind)
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

            Python::attach(|py| instrument_any_to_pyobject(py, instrument))
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

            Python::attach(|py| Ok(account_state.into_py_any_unwrap(py)))
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
                let pylist = PyList::new(
                    py,
                    trades.into_iter().map(|trade| trade.into_py_any_unwrap(py)),
                )?;
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
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(book.into_py_any_unwrap(py)))
        })
    }

    #[pyo3(name = "request_order_status_reports")]
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None, open_only=true))]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
        open_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_order_status_reports(
                    account_id,
                    instrument_id,
                    start.map(nautilus_core::UnixNanos::from),
                    end.map(nautilus_core::UnixNanos::from),
                    open_only,
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
    #[pyo3(signature = (account_id, instrument_id=None, start=None, end=None))]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let reports = client
                .request_fill_reports(
                    account_id,
                    instrument_id,
                    start.map(nautilus_core::UnixNanos::from),
                    end.map(nautilus_core::UnixNanos::from),
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

impl From<DeribitHttpError> for PyErr {
    fn from(error: DeribitHttpError) -> Self {
        match error {
            // Runtime/operational errors
            DeribitHttpError::Canceled(msg) => to_pyruntime_err(format!("Request canceled: {msg}")),
            DeribitHttpError::NetworkError(msg) => {
                to_pyruntime_err(format!("Network error: {msg}"))
            }
            DeribitHttpError::UnexpectedStatus { status, body } => {
                to_pyruntime_err(format!("Unexpected HTTP status code {status}: {body}"))
            }
            DeribitHttpError::Timeout(msg) => to_pyruntime_err(format!("Request timeout: {msg}")),
            // Validation/configuration errors
            DeribitHttpError::MissingCredentials => {
                to_pyvalue_err("Missing credentials for authenticated request")
            }
            DeribitHttpError::ValidationError(msg) => {
                to_pyvalue_err(format!("Parameter validation error: {msg}"))
            }
            DeribitHttpError::JsonError(msg) => to_pyvalue_err(format!("JSON error: {msg}")),
            DeribitHttpError::DeribitError {
                error_code,
                message,
            } => to_pyvalue_err(format!("Deribit error {error_code}: {message}")),
        }
    }
}
