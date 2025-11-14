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

//! Python bindings for the dYdX WebSocket client.

use std::{
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use nautilus_core::python::to_pyvalue_err;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    python::instruments::pyobject_to_instrument_any,
};
use nautilus_network::mode::ConnectionMode;
use pyo3::prelude::*;

use crate::{
    common::credential::DydxCredential,
    websocket::{client::DydxWebSocketClient, error::DydxWsError},
};

fn to_pyvalue_err_dydx(e: DydxWsError) -> PyErr {
    pyo3::exceptions::PyValueError::new_err(e.to_string())
}

#[pymethods]
impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    #[staticmethod]
    #[pyo3(name = "new_public")]
    fn py_new_public(url: String, heartbeat: Option<u64>) -> Self {
        Self::new_public(url, heartbeat)
    }

    /// Creates a new private WebSocket client for account updates.
    #[staticmethod]
    #[pyo3(name = "new_private")]
    fn py_new_private(
        url: String,
        mnemonic: String,
        account_index: u32,
        authenticator_ids: Vec<u64>,
        account_id: AccountId,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        let credential = DydxCredential::from_mnemonic(&mnemonic, account_index, authenticator_ids)
            .map_err(to_pyvalue_err)?;
        Ok(Self::new_private(url, credential, account_id, heartbeat))
    }

    /// Returns whether the client is currently connected.
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Sets the account ID for account message parsing.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Returns the current account ID if set.
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id()
    }

    /// Returns the WebSocket URL.
    #[getter]
    fn py_url(&self) -> String {
        self.url().to_string()
    }

    /// Connects the WebSocket client.
    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        // Convert Python instruments to Rust InstrumentAny
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        // Cache instruments first
        self.cache_instruments(instruments_any);

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            // Connect the WebSocket client
            client.connect().await.map_err(to_pyvalue_err_dydx)?;

            // Take the receiver for messages
            if let Some(mut rx) = client.take_receiver() {
                // Spawn task to process messages and call Python callback
                tokio::spawn(async move {
                    let _client = client; // Keep client alive in spawned task

                    while let Some(msg) = rx.recv().await {
                        match msg {
                            crate::websocket::messages::NautilusWsMessage::Data(items) => {
                                Python::attach(|py| {
                                    for data in items {
                                        use nautilus_model::python::data::data_to_pycapsule;
                                        let py_obj = data_to_pycapsule(py, data);
                                        if let Err(e) = callback.call1(py, (py_obj,)) {
                                            tracing::error!("Error calling Python callback: {e}");
                                        }
                                    }
                                });
                            }
                            crate::websocket::messages::NautilusWsMessage::Deltas(deltas) => {
                                Python::attach(|py| {
                                    use nautilus_model::{
                                        data::{Data, OrderBookDeltas_API},
                                        python::data::data_to_pycapsule,
                                    };
                                    let data = Data::Deltas(OrderBookDeltas_API::new(*deltas));
                                    let py_obj = data_to_pycapsule(py, data);
                                    if let Err(e) = callback.call1(py, (py_obj,)) {
                                        tracing::error!("Error calling Python callback: {e}");
                                    }
                                });
                            }
                            crate::websocket::messages::NautilusWsMessage::Error(err) => {
                                tracing::error!("dYdX WebSocket error: {err}");
                            }
                            crate::websocket::messages::NautilusWsMessage::Reconnected => {
                                tracing::info!("dYdX WebSocket reconnected");
                            }
                            _ => {
                                // Handle other message types if needed
                            }
                        }
                    }
                });
            }

            Ok(())
        })
    }

    /// Disconnects the WebSocket client.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Waits until the client is in an active state.
    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let connection_mode = self.connection_mode_atomic();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            let timeout = Duration::from_secs_f64(timeout_secs);
            let start = Instant::now();

            loop {
                let mode = connection_mode.load();
                let mode_u8 = mode.load(Ordering::Relaxed);
                let is_connected = matches!(
                    mode_u8,
                    x if x == ConnectionMode::Active as u8 || x == ConnectionMode::Reconnect as u8
                );

                if is_connected {
                    break;
                }

                if start.elapsed() > timeout {
                    return Err(to_pyvalue_err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        format!("Client did not become active within {timeout_secs}s"),
                    )));
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }

            Ok(())
        })
    }

    /// Caches a single instrument.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, instrument: Py<PyAny>, py: Python<'_>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    /// Caches multiple instruments.
    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(&self, instruments: Vec<Py<PyAny>>, py: Python<'_>) -> PyResult<()> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }
        self.cache_instruments(instruments_any);
        Ok(())
    }

    /// Returns whether the client is closed.
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        !self.is_connected()
    }

    /// Subscribes to public trade updates for a specific instrument.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(instrument_id)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(instrument_id)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Subscribes to orderbook updates for a specific instrument.
    #[pyo3(name = "subscribe_orderbook")]
    fn py_subscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    #[pyo3(name = "unsubscribe_orderbook")]
    fn py_unsubscribe_orderbook<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_orderbook(instrument_id)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Subscribes to bar updates for a specific instrument.
    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from bar updates for a specific instrument.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        resolution: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Subscribes to all markets updates.
    #[pyo3(name = "subscribe_markets")]
    fn py_subscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_markets()
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from all markets updates.
    #[pyo3(name = "unsubscribe_markets")]
    fn py_unsubscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_markets()
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Subscribes to subaccount updates.
    #[pyo3(name = "subscribe_subaccount")]
    fn py_subscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from subaccount updates.
    #[pyo3(name = "unsubscribe_subaccount")]
    fn py_unsubscribe_subaccount<'py>(
        &self,
        py: Python<'py>,
        address: String,
        subaccount_number: u32,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_subaccount(&address, subaccount_number)
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Subscribes to block height updates.
    #[pyo3(name = "subscribe_block_height")]
    fn py_subscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_block_height()
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }

    /// Unsubscribes from block height updates.
    #[pyo3(name = "unsubscribe_block_height")]
    fn py_unsubscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_block_height()
                .await
                .map_err(to_pyvalue_err_dydx)?;
            Ok(())
        })
    }
}
