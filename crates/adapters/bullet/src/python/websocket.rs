// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

//! Python bindings for `BulletWebSocketClient`.

use nautilus_common::live::get_runtime;
use nautilus_core::{
    python::{call_python_threadsafe, to_pyruntime_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    identifiers::InstrumentId,
    instruments::Instrument,
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
};
use pyo3::prelude::*;
use pyo3_async_runtimes::tokio::future_into_py;

use crate::websocket::{
    client::BulletWebSocketClient,
    messages::ServerMessage,
    parse,
};

fn raw_symbol(instrument_id: &InstrumentId) -> String {
    instrument_id
        .symbol
        .as_str()
        .trim_end_matches("-PERP")
        .to_string()
}

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl BulletWebSocketClient {
    /// Create a new disconnected [`BulletWebSocketClient`].
    #[new]
    fn py_new(url: String) -> Self {
        Self::new(url)
    }

    /// The WebSocket URL.
    #[getter]
    #[pyo3(name = "url")]
    fn py_url(&self) -> &str {
        self.url()
    }

    /// Whether the reconnect loop is running (may be between connections).
    #[pyo3(name = "is_started")]
    fn py_is_started(&self) -> bool {
        self.is_started()
    }

    /// Whether there is an active WebSocket connection right now.
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Establish the WebSocket connection and start streaming Nautilus data objects to `callback`.
    ///
    /// `instruments` is a list of instrument objects whose precision metadata is needed to parse
    /// market data frames.  Each market data message is dispatched as a Nautilus PyCapsule
    /// (zero-copy), except for order-update messages which are dispatched as JSON strings.
    #[pyo3(name = "connect")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_connect<'py>(
        &self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            self.cache_instrument(inst_any);
        }

        let client = self.clone();

        future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            get_runtime().spawn(async move {
                let clock = get_atomic_clock_realtime();

                loop {
                    match client.next_event().await {
                        Some(msg) => {
                            let ts_init = clock.get_time_ns();

                            match msg {
                                ServerMessage::BookTicker(update) => {
                                    if let Some(inst) = client.get_instrument(&update.symbol) {
                                        match parse::book_ticker_to_quote(&update, &inst, ts_init) {
                                            Ok(quote) => {
                                                Python::attach(|py| {
                                                    let py_obj =
                                                        data_to_pycapsule(py, Data::Quote(quote));
                                                    call_python_threadsafe(
                                                        py,
                                                        &call_soon,
                                                        &callback,
                                                        py_obj,
                                                    );
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!("book_ticker parse error: {e}");
                                            }
                                        }
                                    } else {
                                        tracing::debug!(
                                            symbol = %update.symbol,
                                            "no cached instrument for book ticker"
                                        );
                                    }
                                }
                                ServerMessage::AggTrade(update) => {
                                    if let Some(inst) = client.get_instrument(&update.symbol) {
                                        match parse::agg_trade_to_trade(&update, &inst, ts_init) {
                                            Ok(trade) => {
                                                Python::attach(|py| {
                                                    let py_obj =
                                                        data_to_pycapsule(py, Data::Trade(trade));
                                                    call_python_threadsafe(
                                                        py,
                                                        &call_soon,
                                                        &callback,
                                                        py_obj,
                                                    );
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!("agg_trade parse error: {e}");
                                            }
                                        }
                                    } else {
                                        tracing::debug!(
                                            symbol = %update.symbol,
                                            "no cached instrument for agg trade"
                                        );
                                    }
                                }
                                ServerMessage::DepthUpdate(update) => {
                                    if let Some(inst) = client.get_instrument(&update.symbol) {
                                        match parse::depth_to_deltas(&update, &inst, ts_init) {
                                            Ok(deltas) => {
                                                Python::attach(|py| {
                                                    let py_obj = data_to_pycapsule(
                                                        py,
                                                        Data::Deltas(OrderBookDeltas_API::new(
                                                            deltas,
                                                        )),
                                                    );
                                                    call_python_threadsafe(
                                                        py,
                                                        &call_soon,
                                                        &callback,
                                                        py_obj,
                                                    );
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!("depth parse error: {e}");
                                            }
                                        }
                                    } else {
                                        tracing::debug!(
                                            symbol = %update.symbol,
                                            "no cached instrument for depth update"
                                        );
                                    }
                                }
                                ServerMessage::MarkPrice(update) => {
                                    if let Some(inst) = client.get_instrument(&update.symbol) {
                                        match parse::mark_price_to_update(
                                            &update,
                                            inst.id(),
                                            inst.price_precision(),
                                            ts_init,
                                        ) {
                                            Ok(mp) => {
                                                Python::attach(|py| {
                                                    let py_obj = data_to_pycapsule(
                                                        py,
                                                        Data::MarkPriceUpdate(mp),
                                                    );
                                                    call_python_threadsafe(
                                                        py,
                                                        &call_soon,
                                                        &callback,
                                                        py_obj,
                                                    );
                                                });
                                            }
                                            Err(e) => {
                                                tracing::warn!("mark_price parse error: {e}");
                                            }
                                        }
                                    } else {
                                        tracing::debug!(
                                            symbol = %update.symbol,
                                            "no cached instrument for mark price"
                                        );
                                    }
                                }
                                ServerMessage::OrderUpdate(update) => {
                                    // Flatten to a simple JSON object for the exec client.
                                    match serde_json::to_string(&update.to_flat_json()) {
                                        Ok(json) => {
                                            Python::attach(|py| {
                                                let py_str =
                                                    pyo3::types::PyString::new(py, &json);
                                                call_python_threadsafe(
                                                    py,
                                                    &call_soon,
                                                    &callback,
                                                    py_str.into_any().into(),
                                                );
                                            });
                                        }
                                        Err(e) => {
                                            tracing::warn!("OrderUpdate serialize failed: {e}");
                                        }
                                    }
                                }
                                ServerMessage::Result(_) => {} // subscribe ack — ignore
                                ServerMessage::Unknown(v) => {
                                    tracing::debug!(raw = %v, "unknown Bullet WS message");
                                }
                            }
                        }
                        None => {
                            tracing::debug!("BulletWebSocketClient: stream closed");
                            break;
                        }
                    }
                }
            });

            Ok(())
        })
    }

    /// Wait until the WebSocket is actively connected, or until `timeout_secs` elapse.
    #[pyo3(name = "wait_until_active")]
    fn py_wait_until_active<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client
                .wait_until_active(timeout_secs)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Close the WebSocket connection and stop the reconnect loop.
    #[pyo3(name = "close")]
    fn py_close<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client.disconnect();
            Ok(())
        })
    }

    /// Subscribe to best bid/ask (book ticker) for an instrument.
    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .subscribe_quotes_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Unsubscribe from best bid/ask for an instrument.
    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_quotes_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Subscribe to aggregated trades for an instrument.
    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .subscribe_trades_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Unsubscribe from aggregated trades for an instrument.
    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_trades_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Subscribe to L2 depth deltas (top-20) for an instrument.
    #[pyo3(name = "subscribe_book")]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .subscribe_book_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Unsubscribe from L2 depth deltas for an instrument.
    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_book_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Subscribe to mark price + funding rate for an instrument.
    #[pyo3(name = "subscribe_mark_prices")]
    fn py_subscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .subscribe_mark_price_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Unsubscribe from mark price for an instrument.
    #[pyo3(name = "unsubscribe_mark_prices")]
    fn py_unsubscribe_mark_prices<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let symbol = raw_symbol(&instrument_id);
        let client = self.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_mark_price_for_symbol(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Subscribe to authenticated order updates for an address.
    #[pyo3(name = "subscribe_order_updates")]
    fn py_subscribe_order_updates<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client
                .subscribe_order_updates_for_address(&address)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    /// Unsubscribe from order updates for an address.
    #[pyo3(name = "unsubscribe_order_updates")]
    fn py_unsubscribe_order_updates<'py>(
        &self,
        py: Python<'py>,
        address: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        future_into_py(py, async move {
            client
                .unsubscribe_order_updates_for_address(&address)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    fn __repr__(&self) -> String {
        format!(
            "BulletWebSocketClient(url='{}', connected={})",
            self.url(),
            self.is_connected()
        )
    }
}
