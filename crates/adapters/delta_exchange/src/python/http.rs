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

//! Python bindings for Delta Exchange HTTP client.

use nautilus_core::python::serialization::to_dict_pyo3;
use pyo3::{prelude::*, types::PyList};


use super::{
    config::PyDeltaExchangeHttpConfig,
    error::http_error_to_py_err,
};
use crate::http::client::{CreateOrderRequest, DeltaExchangeHttpClient, ModifyOrderRequest};

/// Python wrapper for Delta Exchange HTTP client.
#[pyclass(name = "DeltaExchangeHttpClient")]
#[derive(Debug, Clone)]
pub struct PyDeltaExchangeHttpClient {
    pub inner: DeltaExchangeHttpClient,
}

#[pymethods]
impl PyDeltaExchangeHttpClient {
    #[new]
    #[pyo3(signature = (config=None, api_key=None, api_secret=None))]
    fn py_new(
        config: Option<PyDeltaExchangeHttpConfig>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let (base_url, timeout_secs) = if let Some(cfg) = config {
            (cfg.base_url.clone(), Some(cfg.timeout_secs))
        } else {
            (crate::common::consts::DELTA_EXCHANGE_REST_URL.to_string(), None)
        };

        let client = DeltaExchangeHttpClient::new(base_url, api_key, api_secret, timeout_secs)
            .map_err(http_error_to_py_err)?;

        Ok(Self { inner: client })
    }

    /// Create client for testnet environment.
    #[staticmethod]
    #[pyo3(name = "testnet")]
    fn py_testnet(api_key: Option<String>, api_secret: Option<String>) -> PyResult<Self> {
        let client = DeltaExchangeHttpClient::new(
            crate::common::consts::DELTA_EXCHANGE_TESTNET_REST_URL.to_string(),
            api_key,
            api_secret,
            None,
        )
        .map_err(http_error_to_py_err)?;

        Ok(Self { inner: client })
    }

    // Public API methods

    /// Get all available assets.
    #[pyo3(name = "get_assets")]
    fn py_get_assets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_assets().await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for asset in response {
                    let dict = to_dict_pyo3(py, &asset)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get all available products.
    #[pyo3(name = "get_products")]
    fn py_get_products<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_products().await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for product in response {
                    let dict = to_dict_pyo3(py, &product)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get ticker for a specific product.
    #[pyo3(name = "get_ticker")]
    fn py_get_ticker<'py>(&self, py: Python<'py>, symbol: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_ticker(&symbol).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Get all tickers.
    #[pyo3(name = "get_tickers")]
    fn py_get_tickers<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_tickers().await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for ticker in response {
                    let dict = to_dict_pyo3(py, &ticker)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get order book for a specific product.
    #[pyo3(name = "get_orderbook")]
    fn py_get_orderbook<'py>(&self, py: Python<'py>, symbol: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_orderbook(&symbol).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Get recent trades for a specific product.
    #[pyo3(name = "get_trades")]
    fn py_get_trades<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_trades(&symbol, limit).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for trade in response {
                    let dict = to_dict_pyo3(py, &trade)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get historical candles for a specific product.
    #[pyo3(name = "get_candles")]
    fn py_get_candles<'py>(
        &self,
        py: Python<'py>,
        symbol: String,
        resolution: String,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .get_candles(&symbol, &resolution, start, end)
                .await
                .map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for candle in response {
                    let dict = to_dict_pyo3(py, &candle)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    // Authenticated API methods

    /// Get wallet balances.
    #[pyo3(name = "get_wallet")]
    fn py_get_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_wallet().await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for balance in response {
                    let dict = to_dict_pyo3(py, &balance)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get all orders.
    #[pyo3(name = "get_orders")]
    fn py_get_orders<'py>(
        &self,
        py: Python<'py>,
        product_id: Option<u64>,
        state: Option<String>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .get_orders(product_id, state.as_deref(), limit)
                .await
                .map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for order in response {
                    let dict = to_dict_pyo3(py, &order)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get a specific order by ID.
    #[pyo3(name = "get_order")]
    fn py_get_order<'py>(&self, py: Python<'py>, order_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_order(order_id).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Create a new order.
    #[pyo3(name = "create_order")]
    fn py_create_order<'py>(
        &self,
        py: Python<'py>,
        product_id: u64,
        size: String,
        side: String,
        order_type: String,
        limit_price: Option<String>,
        stop_price: Option<String>,
        time_in_force: Option<String>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        client_order_id: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let order_request = CreateOrderRequest {
                product_id,
                size,
                side,
                order_type,
                limit_price,
                stop_price,
                time_in_force,
                post_only,
                reduce_only,
                client_order_id,
            };

            let response = client.create_order(&order_request).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Modify an existing order.
    #[pyo3(name = "modify_order")]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        order_id: u64,
        size: Option<String>,
        limit_price: Option<String>,
        stop_price: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let modify_request = ModifyOrderRequest {
                size,
                limit_price,
                stop_price,
            };

            let response = client
                .modify_order(order_id, &modify_request)
                .await
                .map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Cancel an order.
    #[pyo3(name = "cancel_order")]
    fn py_cancel_order<'py>(&self, py: Python<'py>, order_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.cancel_order(order_id).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let dict = to_dict_pyo3(py, &response)?;
                Ok(dict.unbind())
            })
        })
    }

    /// Cancel all orders for a product.
    #[pyo3(name = "cancel_all_orders")]
    fn py_cancel_all_orders<'py>(&self, py: Python<'py>, product_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.cancel_all_orders(product_id).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for order in response {
                    let dict = to_dict_pyo3(py, &order)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get positions.
    #[pyo3(name = "get_positions")]
    fn py_get_positions<'py>(&self, py: Python<'py>, product_id: Option<u64>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client.get_positions(product_id).await.map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for position in response {
                    let dict = to_dict_pyo3(py, &position)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    /// Get fills/trades.
    #[pyo3(name = "get_fills")]
    fn py_get_fills<'py>(
        &self,
        py: Python<'py>,
        product_id: Option<u64>,
        order_id: Option<u64>,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let response = client
                .get_fills(product_id, order_id, limit)
                .await
                .map_err(http_error_to_py_err)?;

            Python::with_gil(|py| {
                let py_list = PyList::empty(py);
                for fill in response {
                    let dict = to_dict_pyo3(py, &fill)?;
                    py_list.append(dict)?;
                }
                Ok(py_list.into_any().unbind())
            })
        })
    }

    fn __str__(&self) -> String {
        "DeltaExchangeHttpClient".to_string()
    }

    fn __repr__(&self) -> String {
        "DeltaExchangeHttpClient()".to_string()
    }
}
