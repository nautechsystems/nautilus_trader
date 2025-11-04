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

//! Python bindings for Lighter WebSocket client.

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::{
    common::{credential::LighterCredentials, enums::LighterWsChannel},
    websocket::LighterWebSocketClient,
};

/// Python wrapper for `LighterWebSocketClient`.
#[pyclass(module = "nautilus_trader.core.nautilus_pyo3.lighter2", name = "LighterWebSocketClient")]
pub struct PyLighterWebSocketClient {
    client: LighterWebSocketClient,
}

#[pymethods]
impl PyLighterWebSocketClient {
    #[new]
    #[pyo3(signature = (base_http_url=None, base_ws_url=None, is_testnet=false, api_key_private_key=None, eth_private_key=None, api_key_index=None, account_index=None))]
    fn new(
        base_http_url: Option<String>,
        base_ws_url: Option<String>,
        is_testnet: bool,
        api_key_private_key: Option<String>,
        eth_private_key: Option<String>,
        api_key_index: Option<u8>,
        account_index: Option<u64>,
    ) -> PyResult<Self> {
        let credentials = match (api_key_private_key, eth_private_key, api_key_index, account_index) {
            (Some(api_key), Some(eth_key), Some(key_idx), Some(acc_idx)) => {
                Some(LighterCredentials::new(api_key, eth_key, key_idx, acc_idx)
                    .map_err(|e| PyErr::new::<pyo3::exceptions::PyValueError, _>(e.to_string()))?)
            }
            _ => None,
        };

        Ok(Self {
            client: LighterWebSocketClient::new(base_http_url, base_ws_url, is_testnet, credentials),
        })
    }

    /// Subscribes to order book updates for a market.
    #[pyo3(name = "subscribe_order_book")]
    fn py_subscribe_order_book<'py>(&self, py: Python<'py>, market_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client
                .subscribe(LighterWsChannel::OrderBook { market_id })
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(())
        })
    }

    /// Subscribes to trades for a market.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(&self, py: Python<'py>, market_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client
                .subscribe(LighterWsChannel::Trades { market_id })
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(())
        })
    }

    /// Subscribes to account updates.
    #[pyo3(name = "subscribe_account")]
    fn py_subscribe_account<'py>(&self, py: Python<'py>, account_id: u64) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            client
                .subscribe(LighterWsChannel::Account { account_id })
                .await
                .map_err(|e| PyErr::new::<pyo3::exceptions::PyRuntimeError, _>(e.to_string()))?;

            Ok(())
        })
    }

    /// Gets the subscription count.
    #[pyo3(name = "subscription_count")]
    fn py_subscription_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let count = client.subscription_count().await;
            Ok(count)
        })
    }

    fn __repr__(&self) -> String {
        "LighterWebSocketClient()".to_string()
    }
}
