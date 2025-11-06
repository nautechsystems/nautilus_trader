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

//! Python bindings for Hyperliquid WebSocket client.

use crate::{
    common::HyperliquidWsChannel,
    websocket::Hyperliquid2WebSocketClient,
};
use pyo3::prelude::*;

fn to_pyerr(err: impl std::fmt::Display) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(err.to_string())
}

/// Python wrapper for Hyperliquid WebSocket client
#[pyclass(name = "Hyperliquid2WebSocketClient")]
pub struct PyHyperliquid2WebSocketClient {
    client: Hyperliquid2WebSocketClient,
}

#[pymethods]
impl PyHyperliquid2WebSocketClient {
    /// Creates a new Hyperliquid WebSocket client
    ///
    /// # Parameters
    /// - `ws_base`: Optional custom WebSocket base URL
    /// - `testnet`: Whether to use testnet (default: false)
    #[new]
    #[pyo3(signature = (ws_base=None, testnet=false))]
    fn py_new(
        ws_base: Option<String>,
        testnet: bool,
    ) -> PyResult<Self> {
        let client = Hyperliquid2WebSocketClient::new(ws_base, testnet)
            .map_err(to_pyerr)?;
        Ok(Self { client })
    }

    /// Connects to the WebSocket
    #[pyo3(name = "connect")]
    fn py_connect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyerr)
        })
    }

    /// Subscribes to all mids channel
    #[pyo3(name = "subscribe_all_mids")]
    fn py_subscribe_all_mids<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::AllMids;
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to trades channel for a specific coin
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::Trades { coin };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to L2 book channel for a specific coin
    #[pyo3(name = "subscribe_l2_book")]
    fn py_subscribe_l2_book<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::L2Book { coin };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to candle channel for a specific coin and interval
    #[pyo3(name = "subscribe_candle")]
    fn py_subscribe_candle<'py>(
        &self,
        py: Python<'py>,
        coin: String,
        interval: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::Candle { coin, interval };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to user events channel
    #[pyo3(name = "subscribe_user")]
    fn py_subscribe_user<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::User { user };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to user fills channel
    #[pyo3(name = "subscribe_user_fills")]
    fn py_subscribe_user_fills<'py>(
        &self,
        py: Python<'py>,
        user: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::UserFills { user };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Receives a message from the WebSocket
    #[pyo3(name = "receive")]
    fn py_receive<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.receive().await.map_err(to_pyerr)
        })
    }

    /// Unsubscribes from trades channel
    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::Trades { coin };
            client.unsubscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Unsubscribes from L2 book channel
    #[pyo3(name = "unsubscribe_l2_book")]
    fn py_unsubscribe_l2_book<'py>(
        &self,
        py: Python<'py>,
        coin: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let channel = HyperliquidWsChannel::L2Book { coin };
            client.unsubscribe(channel).await.map_err(to_pyerr)
        })
    }
}
