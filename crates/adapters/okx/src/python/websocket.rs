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

//! Python bindings for the OKX WebSocket client.
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
//! - RwLock is preferred over Mutex (many reads, few writes).

use std::str::FromStr;

use futures_util::StreamExt;
use nautilus_core::python::{to_pyruntime_err, to_pyvalue_err};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, PositionSide, TimeInForce},
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    python::{
        data::data_to_pycapsule,
        instruments::{instrument_any_to_pyobject, pyobject_to_instrument_any},
    },
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, exceptions::PyRuntimeError, prelude::*};

use crate::{
    common::enums::{OKXInstrumentType, OKXTradeMode, OKXVipLevel},
    websocket::{
        OKXWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage, OKXWebSocketError},
    },
};

#[pyo3::pymethods]
impl OKXWebSocketError {
    #[getter]
    pub fn code(&self) -> &str {
        &self.code
    }

    #[getter]
    pub fn message(&self) -> &str {
        &self.message
    }

    #[getter]
    pub fn conn_id(&self) -> Option<&str> {
        self.conn_id.as_deref()
    }

    #[getter]
    pub fn ts_event(&self) -> u64 {
        self.timestamp
    }

    fn __repr__(&self) -> String {
        format!(
            "OKXWebSocketError(code='{}', message='{}', conn_id={:?}, ts_event={})",
            self.code, self.message, self.conn_id, self.timestamp
        )
    }
}

#[pymethods]
impl OKXWebSocketClient {
    #[new]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None))]
    fn py_new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::new(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
        )
        .map_err(to_pyvalue_err)
    }

    #[staticmethod]
    #[pyo3(name = "with_credentials")]
    #[pyo3(signature = (url=None, api_key=None, api_secret=None, api_passphrase=None, account_id=None, heartbeat=None))]
    fn py_with_credentials(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        api_passphrase: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> PyResult<Self> {
        Self::with_credentials(
            url,
            api_key,
            api_secret,
            api_passphrase,
            account_id,
            heartbeat,
        )
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
    pub fn py_url(&self) -> &str {
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

    #[pyo3(name = "cancel_all_requests")]
    pub fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        let channels = self.get_subscriptions(instrument_id);

        // Convert to OKX channel names
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

    /// Sets the VIP level for this client.
    ///
    /// The VIP level determines which WebSocket channels are available.
    #[pyo3(name = "set_vip_level")]
    fn py_set_vip_level(&self, vip_level: OKXVipLevel) {
        self.set_vip_level(vip_level);
    }

    /// Gets the current VIP level.
    #[pyo3(name = "vip_level")]
    #[getter]
    fn py_vip_level(&self) -> OKXVipLevel {
        self.vip_level()
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
                        NautilusWsMessage::FundingRates(msg) => {
                            for data in msg {
                                call_python_with_data(&callback, |py| data.into_py_any(py));
                            }
                        }
                        NautilusWsMessage::OrderRejected(msg) => {
                            call_python_with_data(&callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderCancelRejected(msg) => {
                            call_python_with_data(&callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::OrderModifyRejected(msg) => {
                            call_python_with_data(&callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::ExecutionReports(msg) => {
                            for report in msg {
                                match report {
                                    ExecutionReport::Order(report) => {
                                        call_python_with_data(&callback, |py| {
                                            report.into_py_any(py)
                                        });
                                    }
                                    ExecutionReport::Fill(report) => {
                                        call_python_with_data(&callback, |py| {
                                            report.into_py_any(py)
                                        });
                                    }
                                };
                            }
                        }
                        NautilusWsMessage::Deltas(msg) => Python::attach(|py| {
                            let py_obj =
                                data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(msg)));
                            call_python(py, &callback, py_obj);
                        }),
                        NautilusWsMessage::AccountUpdate(msg) => {
                            call_python_with_data(&callback, |py| msg.into_py_any(py));
                        }
                        NautilusWsMessage::Reconnected => {} // Nothing to handle
                        NautilusWsMessage::Error(msg) => {
                            call_python_with_data(&callback, |py| msg.into_py_any(py));
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
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.close().await {
                log::error!("Error on close: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_instruments")]
    fn py_subscribe_instruments<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_instruments(instrument_type).await {
                log::error!("Failed to subscribe to instruments '{instrument_type}': {e}");
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
                log::error!("Failed to subscribe to instrument {instrument_id}: {e}");
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
            client
                .subscribe_book(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_book50_l2_tbt")]
    fn py_subscribe_book50_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book50_l2_tbt(instrument_id).await {
                log::error!("Failed to subscribe to book50_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_l2_tbt")]
    fn py_subscribe_book_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_l2_tbt(instrument_id).await {
                log::error!("Failed to subscribe to books_l2_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_with_depth")]
    fn py_subscribe_book_with_depth<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: u16,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_with_depth(instrument_id, depth)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "subscribe_book_depth5")]
    fn py_subscribe_book_depth5<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_book_depth5(instrument_id).await {
                log::error!("Failed to subscribe to books5: {e}");
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
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to subscribe to trades: {e}");
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

    #[pyo3(name = "unsubscribe_book_depth5")]
    fn py_unsubscribe_book_depth5<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_depth5(instrument_id).await {
                log::error!("Failed to unsubscribe from books5: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book50_l2_tbt")]
    fn py_unsubscribe_book50_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book50_l2_tbt(instrument_id).await {
                log::error!("Failed to unsubscribe from books50_l2_tbt: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_book_l2_tbt")]
    fn py_unsubscribe_book_l2_tbt<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_book_l2_tbt(instrument_id).await {
                log::error!("Failed to unsubscribe from books_l2_tbt: {e}");
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
        aggregated: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_trades(instrument_id, aggregated).await {
                log::error!("Failed to unsubscribe from trades: {e}");
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

    #[pyo3(name = "subscribe_ticker")]
    fn py_subscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_ticker(instrument_id).await {
                log::error!("Failed to subscribe to ticker: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_ticker")]
    fn py_unsubscribe_ticker<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_ticker(instrument_id).await {
                log::error!("Failed to unsubscribe from ticker: {e}");
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

    #[pyo3(name = "subscribe_funding_rates")]
    fn py_subscribe_funding_rates<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_funding_rates(instrument_id).await {
                log::error!("Failed to subscribe to funding rates: {e}");
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

    #[pyo3(name = "subscribe_orders")]
    fn py_subscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders(instrument_type).await {
                log::error!("Failed to subscribe to orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders")]
    fn py_unsubscribe_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders(instrument_type).await {
                log::error!("Failed to unsubscribe from orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_orders_algo")]
    fn py_subscribe_orders_algo<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_orders_algo(instrument_type).await {
                log::error!("Failed to subscribe to algo orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_orders_algo")]
    fn py_unsubscribe_orders_algo<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_orders_algo(instrument_type).await {
                log::error!("Failed to unsubscribe from algo orders '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_fills")]
    fn py_subscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_fills(instrument_type).await {
                log::error!("Failed to subscribe to fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_fills")]
    fn py_unsubscribe_fills<'py>(
        &self,
        py: Python<'py>,
        instrument_type: OKXInstrumentType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_fills(instrument_type).await {
                log::error!("Failed to unsubscribe from fills '{instrument_type}': {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_account")]
    fn py_subscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.subscribe_account().await {
                log::error!("Failed to subscribe to account: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_account")]
    fn py_unsubscribe_account<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            if let Err(e) = client.unsubscribe_account().await {
                log::error!("Failed to unsubscribe from account: {e}");
            }
            Ok(())
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        td_mode,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force=None,
        price=None,
        trigger_price=None,
        post_only=None,
        reduce_only=None,
        quote_quantity=None,
        position_side=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        td_mode: OKXTradeMode,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: Option<TimeInForce>,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: Option<bool>,
        reduce_only: Option<bool>,
        quote_quantity: Option<bool>,
        position_side: Option<PositionSide>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    td_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                    reduce_only,
                    quote_quantity,
                    position_side,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "cancel_order", signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "modify_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id=None,
        venue_order_id=None,
        price=None,
        quantity=None,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_modify_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: Option<ClientOrderId>,
        venue_order_id: Option<VenueOrderId>,
        price: Option<Price>,
        quantity: Option<Quantity>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .modify_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    price,
                    quantity,
                    venue_order_id,
                )
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[allow(clippy::type_complexity)]
    #[pyo3(name = "batch_submit_orders")]
    fn py_batch_submit_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                td_mode,
                client_order_id,
                order_side,
                order_type,
                quantity,
                position_side,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ): (
                OKXInstrumentType,
                InstrumentId,
                OKXTradeMode,
                ClientOrderId,
                OrderSide,
                OrderType,
                Quantity,
                Option<PositionSide>,
                Option<Price>,
                Option<Price>,
                Option<bool>,
                Option<bool>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;

            domain_orders.push((
                instrument_type,
                instrument_id,
                td_mode,
                client_order_id,
                order_side,
                position_side,
                order_type,
                quantity,
                price,
                trigger_price,
                post_only,
                reduce_only,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_submit_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    /// Cancels multiple orders via WebSocket.
    #[pyo3(name = "batch_cancel_orders")]
    fn py_batch_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        cancels: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut batched_cancels = Vec::with_capacity(cancels.len());

        for obj in cancels {
            let (instrument_id, client_order_id, order_id): (
                InstrumentId,
                Option<ClientOrderId>,
                Option<VenueOrderId>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            batched_cancels.push((instrument_id, client_order_id, order_id));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_cancel_orders(batched_cancels)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "batch_modify_orders")]
    fn py_batch_modify_orders<'py>(
        &self,
        py: Python<'py>,
        orders: Vec<Py<PyAny>>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let mut domain_orders = Vec::with_capacity(orders.len());

        for obj in orders {
            let (
                instrument_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
            ): (
                String,
                InstrumentId,
                ClientOrderId,
                ClientOrderId,
                Option<Price>,
                Option<Quantity>,
            ) = obj
                .extract(py)
                .map_err(|e| PyRuntimeError::new_err(e.to_string()))?;
            let inst_type =
                OKXInstrumentType::from_str(&instrument_type).map_err(to_pyvalue_err)?;
            domain_orders.push((
                inst_type,
                instrument_id,
                client_order_id,
                new_client_order_id,
                price,
                quantity,
            ));
        }

        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .batch_modify_orders(domain_orders)
                .await
                .map_err(to_pyvalue_err)
        })
    }

    #[pyo3(name = "mass_cancel_orders")]
    fn py_mass_cancel_orders<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .mass_cancel_orders(instrument_id)
                .await
                .map_err(to_pyvalue_err)
        })
    }
}

pub fn call_python(py: Python, callback: &Py<PyAny>, py_obj: Py<PyAny>) {
    if let Err(e) = callback.call1(py, (py_obj,)) {
        tracing::error!("Error calling Python: {e}");
    }
}

fn call_python_with_data<F>(callback: &Py<PyAny>, data_converter: F)
where
    F: FnOnce(Python) -> PyResult<Py<PyAny>>,
{
    Python::attach(|py| match data_converter(py) {
        Ok(py_obj) => call_python(py, callback, py_obj),
        Err(e) => tracing::error!("Failed to convert data to Python object: {e}"),
    });
}
