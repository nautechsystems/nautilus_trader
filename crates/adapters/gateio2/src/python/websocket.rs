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

//! Python bindings for Gate.io WebSocket client.

use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::common::{credential::GateioCredentials, enums::GateioWsChannel};
use crate::websocket::GateioWebSocketClient;

#[pyclass(name = "GateioWebSocketClient")]
#[derive(Clone)]
pub struct PyGateioWebSocketClient {
    client: GateioWebSocketClient,
}

#[pymethods]
impl PyGateioWebSocketClient {
    #[new]
    #[pyo3(signature = (base_http_url=None, base_ws_spot_url=None, base_ws_futures_url=None, base_ws_options_url=None, api_key=None, api_secret=None))]
    fn py_new(
        base_http_url: Option<String>,
        base_ws_spot_url: Option<String>,
        base_ws_futures_url: Option<String>,
        base_ws_options_url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
    ) -> PyResult<Self> {
        let credentials = match (api_key, api_secret) {
            (Some(key), Some(secret)) => {
                Some(GateioCredentials::new(key, secret).map_err(to_pyerr)?)
            }
            _ => None,
        };

        let client = GateioWebSocketClient::new(
            base_http_url,
            base_ws_spot_url,
            base_ws_futures_url,
            base_ws_options_url,
            credentials,
        );

        Ok(Self { client })
    }

    /// Subscribes to a spot ticker channel.
    #[pyo3(name = "subscribe_spot_ticker")]
    fn py_subscribe_spot_ticker<'py>(
        &self,
        py: Python<'py>,
        currency_pair: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = GateioWsChannel::SpotTicker { currency_pair };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to a spot order book channel.
    #[pyo3(name = "subscribe_spot_order_book")]
    fn py_subscribe_spot_order_book<'py>(
        &self,
        py: Python<'py>,
        currency_pair: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = GateioWsChannel::SpotOrderBook { currency_pair };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to a spot trades channel.
    #[pyo3(name = "subscribe_spot_trades")]
    fn py_subscribe_spot_trades<'py>(
        &self,
        py: Python<'py>,
        currency_pair: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = GateioWsChannel::SpotTrades { currency_pair };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to futures ticker channel.
    #[pyo3(name = "subscribe_futures_ticker")]
    fn py_subscribe_futures_ticker<'py>(
        &self,
        py: Python<'py>,
        contract: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = GateioWsChannel::FuturesTicker { contract };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Subscribes to futures order book channel.
    #[pyo3(name = "subscribe_futures_order_book")]
    fn py_subscribe_futures_order_book<'py>(
        &self,
        py: Python<'py>,
        contract: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move {
            let channel = GateioWsChannel::FuturesOrderBook { contract };
            client.subscribe(channel).await.map_err(to_pyerr)
        })
    }

    /// Returns the number of active subscriptions.
    #[pyo3(name = "subscription_count")]
    fn py_subscription_count<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move { Ok(client.subscription_count().await) })
    }

    /// Returns all active subscriptions.
    #[pyo3(name = "subscriptions")]
    fn py_subscriptions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.client.clone();
        future_into_py(py, async move { Ok(client.subscriptions().await) })
    }
}

fn to_pyerr<E: std::fmt::Display>(err: E) -> PyErr {
    pyo3::exceptions::PyRuntimeError::new_err(format!("{}", err))
}
