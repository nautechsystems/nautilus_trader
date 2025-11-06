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

//! Python bindings for Hyperliquid HTTP client.

use crate::http::Hyperliquid2HttpClient;
use pyo3::prelude::*;

fn to_pyerr(err: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(err.to_string())
}

/// Python wrapper for Hyperliquid HTTP client
#[pyclass(name = "Hyperliquid2HttpClient")]
pub struct PyHyperliquid2HttpClient {
    client: Hyperliquid2HttpClient,
}

#[pymethods]
impl PyHyperliquid2HttpClient {
    /// Creates a new Hyperliquid HTTP client
    ///
    /// # Parameters
    /// - `private_key`: Optional Ethereum private key for authenticated requests
    /// - `http_base`: Optional custom HTTP base URL
    /// - `testnet`: Whether to use testnet (default: false)
    #[new]
    #[pyo3(signature = (private_key=None, http_base=None, testnet=false))]
    fn py_new(
        private_key: Option<String>,
        http_base: Option<String>,
        testnet: bool,
    ) -> PyResult<Self> {
        let client = Hyperliquid2HttpClient::new(private_key, http_base, testnet)
            .map_err(to_pyerr)?;
        Ok(Self { client })
    }

    /// Loads all instruments from Hyperliquid
    ///
    /// Returns the count of loaded instruments.
    /// Actual instruments are cached internally and accessed via InstrumentProvider.
    #[pyo3(name = "load_instruments")]
    fn py_load_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .load_instruments()
                .await
                .map(|instruments| instruments.len())
                .map_err(to_pyerr)
        })
    }

    /// Fetches meta information (universe of assets)
    #[pyo3(name = "request_meta_info")]
    fn py_request_meta_info<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let meta_info = client.request_meta_info().await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&meta_info).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches all mids (mid prices for all assets)
    #[pyo3(name = "request_all_mids")]
    fn py_request_all_mids<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let mids = client.request_all_mids().await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&mids).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches L2 order book for a specific coin
    #[pyo3(name = "request_l2_book")]
    fn py_request_l2_book<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let book = client.request_l2_book(&coin).await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&book).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches recent trades for a specific coin
    #[pyo3(name = "request_trades")]
    fn py_request_trades<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let trades = client.request_trades(&coin).await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&trades).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches user state (positions, balances)
    #[pyo3(name = "request_user_state")]
    fn py_request_user_state<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let state = client.request_user_state(&user).await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&state).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches open orders for a user
    #[pyo3(name = "request_open_orders")]
    fn py_request_open_orders<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let orders = client.request_open_orders(&user).await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&orders).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }

    /// Fetches user fills (trade history)
    #[pyo3(name = "request_user_fills")]
    fn py_request_user_fills<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let fills = client.request_user_fills(&user).await.map_err(to_pyerr)?;
            let json_str = serde_json::to_string(&fills).map_err(to_pyerr)?;
            Ok(json_str)
        })
    }
}
