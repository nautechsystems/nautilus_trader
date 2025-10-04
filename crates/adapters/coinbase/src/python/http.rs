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

//! Python bindings for HTTP client.

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::{
    config::CoinbaseHttpConfig,
    http::CoinbaseHttpClient as RustCoinbaseHttpClient,
    types::*,
};

#[pymethods]
impl CoinbaseHttpClient {
    #[new]
    fn py_new(
        api_key: String,
        api_secret: String,
        base_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> PyResult<Self> {
        let mut config = CoinbaseHttpConfig::new(api_key, api_secret);
        if let Some(url) = base_url {
            config = config.with_base_url(url);
        }
        if let Some(timeout) = timeout_secs {
            config = config.with_timeout(timeout);
        }

        let client = RustCoinbaseHttpClient::new(config)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

        Ok(Self { inner: client })
    }

    fn __repr__(&self) -> String {
        "CoinbaseHttpClient()".to_string()
    }

    #[pyo3(name = "list_products")]
    fn py_list_products<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .list_products()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_product")]
    fn py_get_product<'py>(&self, py: Python<'py>, product_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_product(&product_id)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "list_accounts")]
    fn py_list_accounts<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .list_accounts()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_account")]
    fn py_get_account<'py>(&self, py: Python<'py>, account_uuid: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_account(&account_uuid)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "create_order")]
    fn py_create_order<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let request: CreateOrderRequest = serde_json::from_str(&request_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

            let response = client
                .create_order(&request)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "cancel_orders")]
    fn py_cancel_orders<'py>(&self, py: Python<'py>, order_ids: Vec<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .cancel_orders(&order_ids)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_order")]
    fn py_get_order<'py>(&self, py: Python<'py>, order_id: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_order(&order_id)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "list_orders")]
    fn py_list_orders<'py>(&self, py: Python<'py>, product_id: Option<String>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .list_orders(product_id.as_deref())
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_candles")]
    fn py_get_candles<'py>(
        &self,
        py: Python<'py>,
        product_id: String,
        granularity: u32,
        start: Option<u64>,
        end: Option<u64>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_candles(&product_id, granularity, start, end)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_market_trades")]
    fn py_get_market_trades<'py>(
        &self,
        py: Python<'py>,
        product_id: String,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_market_trades(&product_id, limit)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_product_book")]
    fn py_get_product_book<'py>(
        &self,
        py: Python<'py>,
        product_id: String,
        limit: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .get_product_book(&product_id, limit)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "get_best_bid_ask")]
    fn py_get_best_bid_ask<'py>(
        &self,
        py: Python<'py>,
        product_ids: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let product_id_refs: Vec<&str> = product_ids.iter().map(|s| s.as_str()).collect();
            let response = client
                .get_best_bid_ask(&product_id_refs)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "edit_order")]
    fn py_edit_order<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let request: EditOrderRequest = serde_json::from_str(&request_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

            let response = client
                .edit_order(&request)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "preview_order")]
    fn py_preview_order<'py>(&self, py: Python<'py>, request_json: String) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let request: PreviewOrderRequest = serde_json::from_str(&request_json)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?;

            let response = client
                .preview_order(&request)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }

    #[pyo3(name = "close_position")]
    fn py_close_position<'py>(
        &self,
        py: Python<'py>,
        client_order_id: String,
        product_id: String,
        size: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let response = client
                .close_position(&client_order_id, &product_id, size.as_deref())
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            let json = serde_json::to_string(&response)
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(json)
        })
    }
}

/// Python wrapper for CoinbaseHttpClient
#[pyclass]
#[derive(Clone, Debug)]
pub struct CoinbaseHttpClient {
    inner: RustCoinbaseHttpClient,
}

