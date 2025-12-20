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

//! Python bindings for the Deribit WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc<RwLock<T>>`:
//!
//! - Connection mode and signal are shared via Arc.
//!
//! ## Connection Flow
//!
//! 1. Clone the client for async operation.
//! 2. Connect and populate shared state on the clone.
//! 3. Spawn stream handler as background task.
//! 4. Return immediately (non-blocking).
//!
//! ## Important Notes
//!
//! - Never use `block_on()` - it blocks the runtime.
//! - Always clone before async blocks for lifetime requirements.

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
};
use pyo3::{exceptions::PyRuntimeError, prelude::*};

use crate::websocket::{
    client::DeribitWebSocketClient, enums::DeribitUpdateInterval, messages::NautilusWsMessage,
};

/// Helper function to call Python callback with data conversion.
fn call_python_with_data<F>(callback: &Py<PyAny>, f: F)
where
    F: for<'py> FnOnce(Python<'py>) -> PyResult<Py<PyAny>>,
{
    Python::attach(|py| {
        let result = f(py);
        match result {
            Ok(obj) => {
                if let Err(e) = callback.call1(py, (obj,)) {
                    tracing::error!("Error calling Python callback: {e}");
                }
            }
            Err(e) => {
                tracing::error!("Error converting to Python object: {e}");
            }
        }
    });
}

/// Helper function to call Python callback with a PyObject.
fn call_python(py: Python<'_>, callback: &Py<PyAny>, obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (obj,)) {
        tracing::error!("Error calling Python callback: {e}");
    }
}

#[pymethods]
impl DeribitWebSocketClient {
    #[new]
    #[pyo3(signature = (
        url=None,
        api_key=None,
        api_secret=None,
        heartbeat_interval=None,
        is_testnet=false,
    ))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat_interval: Option<u64>,
        is_testnet: bool,
    ) -> PyResult<Self> {
        Self::new(url, api_key, api_secret, heartbeat_interval, is_testnet).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "new_public")]
    fn py_new_public(is_testnet: bool) -> PyResult<Self> {
        Self::new_public(is_testnet).map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    fn py_with_credentials(is_testnet: bool) -> PyResult<Self> {
        Self::with_credentials(is_testnet).map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> String {
        self.url().to_string()
    }

    #[getter]
    #[pyo3(name = "is_testnet")]
    #[must_use]
    pub fn py_is_testnet(&self) -> bool {
        // Check if the URL contains "test"
        self.url().contains("test")
    }

    #[pyo3(name = "is_active")]
    #[must_use]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "has_credentials")]
    #[must_use]
    fn py_has_credentials(&self) -> bool {
        self.has_credentials()
    }

    #[pyo3(name = "is_authenticated")]
    #[must_use]
    fn py_is_authenticated(&self) -> bool {
        self.is_authenticated()
    }

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Caches instruments for use during message parsing.
    ///
    /// # Errors
    ///
    /// Returns a Python exception if converting instruments fails.
    #[pyo3(name = "cache_instruments")]
    pub fn py_cache_instruments(
        &self,
        py: Python<'_>,
        instruments: Vec<Py<PyAny>>,
    ) -> PyResult<()> {
        let instruments: Result<Vec<_>, _> = instruments
            .into_iter()
            .map(|inst| pyobject_to_instrument_any(py, inst))
            .collect();
        self.cache_instruments(instruments?);
        Ok(())
    }

    /// Caches a single instrument.
    ///
    /// # Errors
    ///
    /// Returns a Python exception if converting the instrument fails.
    #[pyo3(name = "cache_instrument")]
    pub fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst);
        Ok(())
    }

    /// Connects to the Deribit WebSocket and starts processing messages.
    ///
    /// This is a non-blocking call that spawns a background task for message processing.
    /// Messages are dispatched to the provided callback function.
    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut instruments_any = Vec::new();
        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        self.cache_instruments(instruments_any);

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            // Keep client alive in the spawned task to prevent handler from dropping
            get_runtime().spawn(async move {
                let _client = client;
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        NautilusWsMessage::Instrument(msg) => {
                            call_python_with_data(&callback, |py| {
                                instrument_any_to_pyobject(py, *msg)
                            });
                        }
                        NautilusWsMessage::Data(msg) => Python::attach(|py| {
                            for data in msg {
                                let py_obj = data_to_pycapsule(py, data);
                                call_python(py, &callback, py_obj);
                            }
                        }),
                        NautilusWsMessage::Deltas(msg) => Python::attach(|py| {
                            let py_obj =
                                data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(msg)));
                            call_python(py, &callback, py_obj);
                        }),
                        NautilusWsMessage::Error(err) => {
                            tracing::error!("Deribit WebSocket error: {err}");
                        }
                        NautilusWsMessage::Reconnected => {
                            tracing::info!("Deribit WebSocket reconnected");
                        }
                        NautilusWsMessage::Authenticated(auth_result) => {
                            tracing::info!(
                                "Deribit WebSocket authenticated (scope: {})",
                                auth_result.scope
                            );
                        }
                        NautilusWsMessage::Raw(msg) => {
                            tracing::debug!("Received raw message, skipping: {msg}");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .wait_until_active(timeout_secs)
                .await
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                tracing::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    /// Authenticates the WebSocket session with Deribit.
    ///
    /// Uses the `client_signature` grant type with HMAC-SHA256 signature.
    /// This must be called before subscribing to raw data streams.
    #[pyo3(name = "authenticate")]
    #[pyo3(signature = (session_name=None))]
    fn py_authenticate<'py>(
        &self,
        py: Python<'py>,
        session_name: Option<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate(session_name.as_deref())
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Authenticates with session scope using default session name.
    ///
    /// Convenience method equivalent to `authenticate(Some("nautilus"))`.
    #[pyo3(name = "authenticate_session")]
    fn py_authenticate_session<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate_session()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    // ------------------------------------------------------------------------------------------------
    // Subscription Methods
    // ------------------------------------------------------------------------------------------------

    /// Subscribes to trade updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to.
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_trades")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to raw trade updates (requires authentication).
    #[pyo3(name = "subscribe_trades_raw")]
    fn py_subscribe_trades_raw<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades_raw(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from trade updates for an instrument.
    #[pyo3(name = "unsubscribe_trades")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to order book updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to.
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_book")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to raw order book updates (requires authentication).
    #[pyo3(name = "subscribe_book_raw")]
    fn py_subscribe_book_raw<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_raw(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from order book updates for an instrument.
    #[pyo3(name = "unsubscribe_book")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to ticker updates for an instrument.
    ///
    /// # Arguments
    ///
    /// * `instrument_id` - The instrument to subscribe to.
    /// * `interval` - Update interval. Defaults to `Ms100` (100ms). `Raw` requires authentication.
    #[pyo3(name = "subscribe_ticker")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to raw ticker updates (requires authentication).
    #[pyo3(name = "subscribe_ticker_raw")]
    fn py_subscribe_ticker_raw<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_ticker_raw(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from ticker updates for an instrument.
    #[pyo3(name = "unsubscribe_ticker")]
    #[pyo3(signature = (instrument_id, interval=None))]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        interval: Option<DeribitUpdateInterval>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_ticker(instrument_id, interval)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to quote (best bid/ask) updates for an instrument.
    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_quotes(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from quote updates for an instrument.
    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_quotes(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Subscribes to multiple channels at once.
    #[pyo3(name = "subscribe")]
    fn py_subscribe<'py>(
        &self,
        py: Python<'py>,
        channels: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe(channels).await.map_err(to_pyvalue_err)
        })
    }

    /// Unsubscribes from multiple channels at once.
    #[pyo3(name = "unsubscribe")]
    fn py_unsubscribe<'py>(
        &self,
        py: Python<'py>,
        channels: Vec<String>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe(channels).await.map_err(to_pyvalue_err)
        })
    }
}
