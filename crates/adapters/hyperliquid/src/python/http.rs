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

use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyvalue_err};
use nautilus_model::{
    instruments::{Instrument, InstrumentAny},
    python::{
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
        orders::pyobject_to_order_any,
    },
};
use pyo3::{prelude::*, types::PyList};
use serde_json::to_string;

use crate::http::client::HyperliquidHttpClient;

#[pymethods]
impl HyperliquidHttpClient {
    #[new]
    #[pyo3(signature = (is_testnet=false, timeout_secs=None))]
    fn py_new(is_testnet: bool, timeout_secs: Option<u64>) -> Self {
        Self::new(is_testnet, timeout_secs)
    }

    /// Create an authenticated HTTP client from environment variables.
    ///
    /// Reads credentials from:
    /// - `HYPERLIQUID_PK` or `HYPERLIQUID_TESTNET_PK` (private key)
    /// - `HYPERLIQUID_VAULT` or `HYPERLIQUID_TESTNET_VAULT` (optional vault address)
    ///
    /// Returns an authenticated HyperliquidHttpClient or raises an error if credentials are missing.
    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
    }

    /// Get perpetuals metadata as a JSON string.
    #[pyo3(name = "get_perp_meta")]
    fn py_get_perp_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.load_perp_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    /// Get spot metadata as a JSON string.
    #[pyo3(name = "get_spot_meta")]
    fn py_get_spot_meta<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta = client.get_spot_meta().await.map_err(to_pyvalue_err)?;
            to_string(&meta).map_err(to_pyvalue_err)
        })
    }

    /// Load all available instruments (perps and/or spot) as Nautilus instrument objects.
    #[pyo3(name = "load_instrument_definitions", signature = (include_perp=true, include_spot=true))]
    fn py_load_instrument_definitions<'py>(
        &self,
        py: Python<'py>,
        include_perp: bool,
        include_spot: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mut instruments = client.request_instruments().await.map_err(to_pyvalue_err)?;

            if !include_perp || !include_spot {
                instruments.retain(|instrument| match instrument {
                    InstrumentAny::CryptoPerpetual(_) => include_perp,
                    InstrumentAny::CurrencyPair(_) => include_spot,
                    _ => true,
                });
            }

            instruments.sort_by_key(|instrument| instrument.id());

            Python::attach(|py| {
                let mut py_instruments = Vec::with_capacity(instruments.len());
                for instrument in instruments {
                    py_instruments.push(instrument_any_to_pyobject(py, instrument)?);
                }

                let py_list = PyList::new(py, &py_instruments)?;
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Submit a single order to the Hyperliquid exchange.
    ///
    /// Takes a Nautilus Order object and handles all conversion and serialization internally in Rust.
    /// This pushes complexity down to the Rust layer for pure Rust execution support.
    ///
    /// Returns an OrderStatusReport object.
    #[pyo3(name = "submit_order")]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        order: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Convert Python Order object to Rust OrderAny
            let order_any =
                Python::attach(|py| pyobject_to_order_any(py, order).map_err(to_pyvalue_err))?;

            let report = client
                .submit_order(&order_any)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| Ok(report.into_py_any_unwrap(py)))
        })
    }

    /// Submit multiple orders to the Hyperliquid exchange in a single request.
    ///
    /// Takes a list of Nautilus Order objects and handles all conversion and serialization internally in Rust.
    /// This pushes complexity down to the Rust layer for pure Rust execution support.
    ///
    /// Returns a list of OrderStatusReport objects.
    #[pyo3(name = "submit_orders")]
    fn py_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Convert Python Order objects to Rust OrderAny objects
            let order_anys: Vec<nautilus_model::orders::any::OrderAny> = Python::attach(|py| {
                orders
                    .into_iter()
                    .map(|order| pyobject_to_order_any(py, order))
                    .collect::<PyResult<Vec<_>>>()
                    .map_err(to_pyvalue_err)
            })?;

            // Create references for the submit_orders call
            let order_refs: Vec<&nautilus_model::orders::any::OrderAny> =
                order_anys.iter().collect();

            let reports = client
                .submit_orders(&order_refs)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Get open orders for the authenticated user.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "get_open_orders")]
    fn py_get_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let response = client
                .info_open_orders(&user_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Get clearinghouse state (balances, positions, margin) for the authenticated user.
    ///
    /// Returns the response from the exchange as a JSON string.
    #[pyo3(name = "get_clearinghouse_state")]
    fn py_get_clearinghouse_state<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let response = client
                .info_clearinghouse_state(&user_address)
                .await
                .map_err(to_pyvalue_err)?;
            to_string(&response).map_err(to_pyvalue_err)
        })
    }

    /// Add an instrument to the internal cache.
    ///
    /// This is required before calling report generation methods.
    #[pyo3(name = "add_instrument")]
    fn py_add_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.add_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    /// Set the account ID for report generation.
    ///
    /// This is required before calling report generation methods.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: &str) -> PyResult<()> {
        let account_id = nautilus_model::identifiers::AccountId::from(account_id);
        self.set_account_id(account_id);
        Ok(())
    }

    /// Request order status reports for the authenticated user.
    ///
    /// Returns a list of OrderStatusReport objects.
    #[pyo3(name = "request_order_status_reports")]
    fn py_request_order_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(nautilus_model::identifiers::InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_order_status_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request fill reports for the authenticated user.
    ///
    /// Returns a list of FillReport objects.
    #[pyo3(name = "request_fill_reports")]
    fn py_request_fill_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(nautilus_model::identifiers::InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_fill_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }

    /// Request position status reports for the authenticated user.
    ///
    /// Returns a list of PositionStatusReport objects.
    #[pyo3(name = "request_position_status_reports")]
    fn py_request_position_status_reports<'py>(
        &self,
        py: Python<'py>,
        instrument_id: Option<&str>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_id = instrument_id.map(nautilus_model::identifiers::InstrumentId::from);

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let user_address = client.get_user_address().map_err(to_pyvalue_err)?;
            let reports = client
                .request_position_status_reports(&user_address, instrument_id)
                .await
                .map_err(to_pyvalue_err)?;

            Python::attach(|py| {
                let pylist =
                    PyList::new(py, reports.into_iter().map(|r| r.into_py_any_unwrap(py)))?;
                Ok(pylist.into_py_any_unwrap(py))
            })
        })
    }
}
