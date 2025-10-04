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

//! Python bindings for WebSocket client.

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::websocket::{CoinbaseWebSocketClient as RustCoinbaseWebSocketClient, Channel};

/// Python wrapper for CoinbaseWebSocketClient
#[pyclass]
#[derive(Debug)]
pub struct CoinbaseWebSocketClient {
    inner: Arc<RustCoinbaseWebSocketClient>,
}

#[pymethods]
impl CoinbaseWebSocketClient {
    /// Create a new WebSocket client for market data
    #[new]
    #[pyo3(signature = (api_key, api_secret, ws_url=None, is_user_data=false))]
    fn py_new(
        api_key: String,
        api_secret: String,
        ws_url: Option<String>,
        is_user_data: bool,
    ) -> PyResult<Self> {
        let client = if let Some(url) = ws_url {
            RustCoinbaseWebSocketClient::new_with_url(url, api_key, api_secret)
        } else if is_user_data {
            RustCoinbaseWebSocketClient::new_user_data(api_key, api_secret)
        } else {
            RustCoinbaseWebSocketClient::new_market_data(api_key, api_secret)
        };

        Ok(Self {
            inner: Arc::new(client),
        })
    }

    fn __repr__(&self) -> String {
        "CoinbaseWebSocketClient()".to_string()
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .connect()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .disconnect()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        product_ids: Vec<String>,
        channel: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let channel_enum = parse_channel(&channel)?;

            client
                .subscribe(product_ids, channel_enum)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_heartbeats")]
    fn py_subscribe_heartbeats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .subscribe_heartbeats()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        product_ids: Vec<String>,
        channel: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let channel_enum = parse_channel(&channel)?;

            client
                .unsubscribe(product_ids, channel_enum)
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_heartbeats")]
    fn py_unsubscribe_heartbeats<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_heartbeats()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "receive_message")]
    fn py_receive_message<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move {
            let message = client
                .receive_message()
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;
            Ok(message)
        })
    }

    #[pyo3(name = "is_connected")]
    fn py_is_connected<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        future_into_py(py, async move { Ok(client.is_connected().await) })
    }
}

fn parse_channel(channel: &str) -> PyResult<Channel> {
    match channel.to_lowercase().as_str() {
        "ticker" => Ok(Channel::Ticker),
        "ticker_batch" => Ok(Channel::TickerBatch),
        "level2" | "l2_data" => Ok(Channel::Level2),
        "user" => Ok(Channel::User),
        "market_trades" => Ok(Channel::MarketTrades),
        "status" => Ok(Channel::Status),
        "heartbeats" => Ok(Channel::Heartbeats),
        "candles" => Ok(Channel::Candles),
        "futures_balance_summary" => Ok(Channel::FuturesBalanceSummary),
        _ => Err(PyErr::new::<pyo3::exceptions::PyValueError, _>(format!(
            "Invalid channel: {}. Valid channels: ticker, ticker_batch, level2, user, market_trades, status, heartbeats, candles, futures_balance_summary",
            channel
        ))),
    }
}

