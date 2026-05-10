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

//! Python bindings for `BulletHttpClient`.

use nautilus_core::python::to_pyruntime_err;
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::http::client::BulletHttpClient;

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BulletHttpClient {
    /// Create a new [`BulletHttpClient`].
    #[new]
    #[pyo3(signature = (base_url, timeout_secs = 60, proxy_url = None))]
    fn py_new(
        base_url: String,
        timeout_secs: u64,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        Self::new(base_url, timeout_secs, proxy_url).map_err(to_pyruntime_err)
    }

    /// Return the base URL.
    #[getter]
    #[pyo3(name = "base_url")]
    fn py_base_url(&self) -> &str {
        self.base_url()
    }

    /// Fetch exchange info and return the raw JSON string.
    ///
    /// `GET /fapi/v1/exchangeInfo`
    #[pyo3(name = "exchange_info_json")]
    fn py_exchange_info_json<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client.exchange_info_raw().await.map_err(to_pyruntime_err)
        })
    }

    /// Fetch open orders for an address on a symbol and return raw JSON.
    ///
    /// `GET /fapi/v1/openOrders`
    #[pyo3(name = "open_orders_json")]
    #[pyo3(signature = (address, symbol))]
    fn py_open_orders_json<'py>(
        &self,
        py: Python<'py>,
        address: String,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            let orders = client.open_orders(&address, &symbol).await.map_err(to_pyruntime_err)?;
            serde_json::to_string(&orders).map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Fetch the best bid and ask prices for a symbol.
    ///
    /// Returns `(best_bid_price, best_ask_price)` as a tuple of strings, or `None` if the book
    /// is empty.  Uses `limit=1` depth request.
    ///
    /// `GET /fapi/v1/depth`
    #[pyo3(name = "best_bid_ask")]
    #[pyo3(signature = (symbol))]
    fn py_best_bid_ask<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            let book = client.depth(&symbol, Some(1)).await.map_err(to_pyruntime_err)?;
            let best_bid = book.bids.first().map(|[p, _]| p.clone());
            let best_ask = book.asks.first().map(|[p, _]| p.clone());
            Ok((best_bid, best_ask))
        })
    }

    /// Fetch account state (positions, margins, balances) for an address.
    ///
    /// Returns raw JSON string.  `GET /fapi/v3/account`
    #[pyo3(name = "account_json")]
    #[pyo3(signature = (address))]
    fn py_account_json<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            let account = client.account(&address).await.map_err(to_pyruntime_err)?;
            serde_json::to_string(&account).map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    /// Fetch per-asset balances for an address.
    ///
    /// Returns raw JSON string.  `GET /fapi/v3/balance`
    #[pyo3(name = "balances_json")]
    #[pyo3(signature = (address))]
    fn py_balances_json<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            let balances = client.balances(&address).await.map_err(to_pyruntime_err)?;
            serde_json::to_string(&balances).map_err(|e| to_pyruntime_err(e.to_string()))
        })
    }

    fn __repr__(&self) -> String {
        format!("BulletHttpClient(base_url='{}')", self.base_url())
    }
}
