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

use std::sync::Arc;

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;
use serde_json::Value;

use crate::common::credentials::HyperliquidCredentials;
use crate::websocket::client::{HyperliquidWebSocketClient, MessageHandler};

#[pymethods]
impl HyperliquidWebSocketClient {
    /// Create a new Hyperliquid WebSocket client.
    #[new]
    #[pyo3(signature = (url=None, private_key=None, wallet_address=None, testnet=false))]
    fn py_new(
        url: Option<String>,
        private_key: Option<String>,
        wallet_address: Option<String>,
        testnet: bool,
    ) -> PyResult<Self> {
        let credentials = if let Some(key) = private_key {
            Some(HyperliquidCredentials::new(key, wallet_address, testnet))
        } else {
            None
        };

        Self::new(url, credentials)
            .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
    }

    #[pyo3(name = "is_connected")]
    #[must_use]
    pub fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Set message callback handler.
    #[pyo3(name = "set_message_handler")]
    fn py_set_message_handler(&mut self, callback: PyObject) -> PyResult<()> {
        let handler: MessageHandler = Arc::new(move |msg: Value| {
            Python::with_gil(|py| {
                if let Ok(json_str) = serde_json::to_string(&msg) {
                    let _ = callback.call1(py, (json_str,));
                }
            });
        });
        
        self.set_message_handler(handler);
        Ok(())
    }

    /// Connect to the WebSocket.
    #[pyo3(name = "connect")]
    fn py_connect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.connect().await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Disconnect from the WebSocket.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.disconnect().await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Subscribe to all market mid prices.
    #[pyo3(name = "subscribe_all_mids")]
    fn py_subscribe_all_mids<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.subscribe_all_mids().await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Subscribe to L2 book for a specific coin.
    #[pyo3(name = "subscribe_l2_book")]
    fn py_subscribe_l2_book<'py>(&mut self, py: Python<'py>, coin: String) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.subscribe_l2_book(&coin).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Subscribe to trades for a specific coin.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(&mut self, py: Python<'py>, coin: String) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.subscribe_trades(&coin).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }

    /// Get reconnection attempts count.
    #[pyo3(name = "reconnect_attempts")]
    fn py_reconnect_attempts(&self) -> u32 {
        self.reconnect_attempts()
    }

    /// Get time since last heartbeat in seconds.
    #[pyo3(name = "time_since_heartbeat")]
    fn py_time_since_heartbeat(&self) -> Option<f64> {
        self.time_since_heartbeat().map(|d| d.as_secs_f64())
    }

    /// Connect with retry logic.
    #[pyo3(name = "connect_with_retry")]
    fn py_connect_with_retry<'py>(&mut self, py: Python<'py>, max_attempts: u32) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        future_into_py(py, async move {
            client.connect_with_retry(max_attempts).await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))
        })
    }
}
