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

//! Python bindings for the `BitmEX` WebSocket client.
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
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::bar::BarType,
    identifiers::{AccountId, InstrumentId},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::{conversion::IntoPyObjectExt, exceptions::PyRuntimeError, prelude::*};

use crate::websocket::{BitmexWebSocketClient, messages::NautilusWsMessage};

#[pymethods]
impl BitmexWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, account_id=None, heartbeat=None, testnet=false))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        testnet: bool,
    ) -> PyResult<Self> {
        // If both api_key and api_secret are None, try to load from environment
        let (final_api_key, final_api_secret) = if api_key.is_none() && api_secret.is_none() {
            // Choose environment variables based on testnet flag
            let (key_var, secret_var) = if testnet {
                ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
            } else {
                ("BITMEX_API_KEY", "BITMEX_API_SECRET")
            };

            let env_key = std::env::var(key_var).ok();
            let env_secret = std::env::var(secret_var).ok();
            (env_key, env_secret)
        } else {
            (api_key, api_secret)
        };

        Self::new(url, final_api_key, final_api_secret, account_id, heartbeat)
            .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "from_env")]
    fn py_from_env() -> PyResult<Self> {
        Self::from_env().map_err(to_pyvalue_err)
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
    pub fn py_api_key(&self) -> Option<&str> {
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
        self.get_subscriptions(instrument_id)
    }

    #[pyo3(name = "set_account_id")]
    pub fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
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
                    Python::attach(|py| match msg {
                        NautilusWsMessage::Data(data_vec) => {
                            for data in data_vec {
                                let py_obj = data_to_pycapsule(py, data);
                                call_python(py, &callback, py_obj);
                            }
                        }
                        NautilusWsMessage::OrderStatusReports(reports) => {
                            for report in reports {
                                if let Ok(py_obj) = report.into_py_any(py) {
                                    call_python(py, &callback, py_obj);
                                }
                            }
                        }
                        NautilusWsMessage::FillReports(reports) => {
                            for report in reports {
                                if let Ok(py_obj) = report.into_py_any(py) {
                                    call_python(py, &callback, py_obj);
                                }
                            }
                        }
                        NautilusWsMessage::PositionStatusReport(report) => {
                            if let Ok(py_obj) = report.into_py_any(py) {
                                call_python(py, &callback, py_obj);
                            }
                        }
                        NautilusWsMessage::FundingRateUpdates(updates) => {
                            for update in updates {
                                if let Ok(py_obj) = update.into_py_any(py) {
                                    call_python(py, &callback, py_obj);
                                }
                            }
                        }
                        NautilusWsMessage::AccountState(account_state) => {
                            if let Ok(py_obj) = account_state.into_py_any(py) {
                                call_python(py, &callback, py_obj);
                            }
                        }
                        NautilusWsMessage::OrderUpdated(event) => {
                            if let Ok(py_obj) = event.into_py_any(py) {
                                call_python(py, &callback, py_obj);
                            }
                        }
                        NautilusWsMessage::Reconnected => {} // Nothing to handle
                    });
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
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instruments")]
    fn py_subscribe_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments().await {
                log::error!("Failed to subscribe to instruments: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instrument")]
    fn py_subscribe_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instrument(instrument_id).await {
                log::error!("Failed to subscribe to instrument: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book")]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book(instrument_id).await {
                log::error!("Failed to subscribe to order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_25")]
    fn py_subscribe_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_25(instrument_id).await {
                log::error!("Failed to subscribe to order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_depth10")]
    fn py_subscribe_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_depth10(instrument_id).await {
                log::error!("Failed to subscribe to order book depth 10: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id).await {
                log::error!("Failed to subscribe to trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_mark_prices(instrument_id).await {
                log::error!("Failed to subscribe to mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_index_prices")]
    fn py_subscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_index_prices(instrument_id).await {
                log::error!("Failed to subscribe to index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_funding_rates")]
    fn py_subscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_funding_rates(instrument_id).await {
                log::error!("Failed to subscribe to funding: {e}");
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
    fn py_unsubscribe_instruments<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_instruments().await {
                log::error!("Failed to unsubscribe from instruments: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_instrument")]
    fn py_unsubscribe_instrument<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_instrument(instrument_id).await {
                log::error!("Failed to unsubscribe from instrument: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book(instrument_id).await {
                log::error!("Failed to unsubscribe from order book: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_25")]
    fn py_unsubscribe_book_25<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_25(instrument_id).await {
                log::error!("Failed to unsubscribe from order book 25: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_depth10")]
    fn py_unsubscribe_book_depth10<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_depth10(instrument_id).await {
                log::error!("Failed to unsubscribe from order book depth 10: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from quotes: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id).await {
                log::error!("Failed to unsubscribe from trades: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_mark_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from mark prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_index_prices")]
    fn py_unsubscribe_index_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_index_prices(instrument_id).await {
                log::error!("Failed to unsubscribe from index prices: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_funding_rates")]
    fn py_unsubscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_funding_rates(instrument_id).await {
                log::error!("Failed to unsubscribe from funding rates: {e}");
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

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders().await {
                log::error!("Failed to subscribe to orders: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_executions")]
    fn py_subscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_executions().await {
                log::error!("Failed to subscribe to executions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_positions")]
    fn py_subscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_positions().await {
                log::error!("Failed to subscribe to positions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_margin")]
    fn py_subscribe_margin<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_margin().await {
                log::error!("Failed to subscribe to margin: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_wallet")]
    fn py_subscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_wallet().await {
                log::error!("Failed to subscribe to wallet: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders().await {
                log::error!("Failed to unsubscribe from orders: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_executions")]
    fn py_unsubscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_executions().await {
                log::error!("Failed to unsubscribe from executions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_positions")]
    fn py_unsubscribe_positions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_positions().await {
                log::error!("Failed to unsubscribe from positions: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_margin")]
    fn py_unsubscribe_margin<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_margin().await {
                log::error!("Failed to unsubscribe from margin: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_wallet")]
    fn py_unsubscribe_wallet<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_wallet().await {
                log::error!("Failed to unsubscribe from wallet: {e}");
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
