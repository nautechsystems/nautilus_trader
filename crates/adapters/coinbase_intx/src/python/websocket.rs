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

//! Python bindings for the Coinbase Intx WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc<RwLock<T>>`:
//!
//! - `inner: Arc<RwLock<Option<WebSocketClient>>>` - The WebSocket connection.
//!
//! Without shared state, clones would be independent, causing:
//! - Lost WebSocket messages.
//! - Missing instrument data.
//! - Connection state desynchronization.
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
//! - `RwLock` is preferred over Mutex (many reads, few writes).

use futures_util::StreamExt;
use nautilus_core::python::{IntoPyObjectNautilusExt, to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::BarType,
    identifiers::InstrumentId,
    python::{
        data::data_to_pycapsule,
        events::order::order_event_to_pyobject,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
};
use pyo3::{exceptions::PyRuntimeError, prelude::*};

use crate::websocket::{CoinbaseIntxWebSocketClient, messages::NautilusWsMessage};

#[pymethods]
impl CoinbaseIntxWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, heartbeat=None))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(url, api_key, api_secret, api_passphrase, heartbeat).map_err(to_pyvalue_err)
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub const fn py_url(&self) -> &str {
        self.url()
    }

    #[getter]
    #[pyo3(name = "api_key")]
    #[must_use]
    pub fn py_api_key(&self) -> &str {
        self.api_key()
    }

    #[pyo3(name = "is_active")]
    fn py_is_active(&mut self) -> bool {
        self.is_active()
    }

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&mut self) -> bool {
        self.is_closed()
    }

    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        let channels = self.get_subscriptions(instrument_id);

        // Convert to Coinbase channel names
        channels
            .iter()
            .map(|c| {
                serde_json::to_value(c)
                    .ok()
                    .and_then(|v| v.as_str().map(String::from))
                    .unwrap_or_else(|| c.to_string())
            })
            .collect()
    }

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

        self.initialize_instruments_cache(instruments_any);

        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            tokio::spawn(async move {
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        NautilusWsMessage::Instrument(inst) => Python::attach(|py| {
                            let py_obj = instrument_any_to_pyobject(py, inst)
                                .expect("Failed to create instrument");
                            call_python(py, &callback, py_obj);
                        }),
                        NautilusWsMessage::Data(data) => Python::attach(|py| {
                            let py_obj = data_to_pycapsule(py, data);
                            call_python(py, &callback, py_obj);
                        }),
                        NautilusWsMessage::DataVec(data_vec) => Python::attach(|py| {
                            for data in data_vec {
                                let py_obj = data_to_pycapsule(py, data);
                                call_python(py, &callback, py_obj);
                            }
                        }),
                        NautilusWsMessage::Deltas(deltas) => Python::attach(|py| {
                            call_python(py, &callback, deltas.into_py_any_unwrap(py));
                        }),
                        NautilusWsMessage::MarkPrice(mark_price) => Python::attach(|py| {
                            call_python(py, &callback, mark_price.into_py_any_unwrap(py));
                        }),
                        NautilusWsMessage::IndexPrice(index_price) => Python::attach(|py| {
                            call_python(py, &callback, index_price.into_py_any_unwrap(py));
                        }),
                        NautilusWsMessage::MarkAndIndex((mark_price, index_price)) => {
                            Python::attach(|py| {
                                call_python(py, &callback, mark_price.into_py_any_unwrap(py));
                                call_python(py, &callback, index_price.into_py_any_unwrap(py));
                            });
                        }
                        NautilusWsMessage::OrderEvent(msg) => Python::attach(|py| {
                            let py_obj =
                                order_event_to_pyobject(py, msg).expect("Failed to create event");
                            call_python(py, &callback, py_obj);
                        }),
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
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instruments")]
    #[pyo3(signature = (instrument_ids=None))]
    fn py_subscribe_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Option<Vec<InstrumentId>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        let instrument_ids = instrument_ids.unwrap_or_default();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments(instrument_ids).await {
                log::error!("Failed to subscribe to instruments: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book")]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book(instrument_ids).await {
                log::error!("Failed to subscribe to order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_quotes(instrument_ids).await {
                log::error!("Failed to subscribe to quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_ids).await {
                log::error!("Failed to subscribe to trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_mark_prices(instrument_ids).await {
                log::error!("Failed to subscribe to mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_index_prices(instrument_ids).await {
                log::error!("Failed to subscribe to index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_bars(bar_type).await {
                log::error!("Failed to subscribe to bars: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_instruments")]
    fn py_unsubscribe_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_instruments(instrument_ids).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book(instrument_ids).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_quotes(instrument_ids).await {
                log::error!("Failed to unsubscribe from quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_ids).await {
                log::error!("Failed to unsubscribe from trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_mark_prices(instrument_ids).await {
                log::error!("Failed to unsubscribe from mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_ids: Vec<InstrumentId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_index_prices(instrument_ids).await {
                log::error!("Failed to unsubscribe from index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_bars(bar_type).await {
                log::error!("Failed to unsubscribe from bars: {e}");
            }
            Ok(())
        })
    }
}

pub fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}
