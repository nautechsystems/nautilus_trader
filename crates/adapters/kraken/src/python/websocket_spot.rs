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

//! Python bindings for the Kraken WebSocket client.
//!
//! # Design Pattern: Clone and Share State
//!
//! The WebSocket client must be cloned for async operations because PyO3's `future_into_py`
//! requires `'static` futures (cannot borrow from `self`). To ensure clones share the same
//! connection state, key fields use `Arc`:
//!
//! - `ws_client: Option<Arc<WebSocketClient>>` - The WebSocket connection.
//! - `subscriptions: Arc<DashMap<String, KrakenWsChannel>>` - Subscription tracking.
//!
//! Without shared state, clones would be independent, causing:
//! - Lost WebSocket messages.
//! - Missing subscription data.
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

use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    AtomicMap,
    python::{call_python_threadsafe, to_pyruntime_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas, OrderBookDeltas_API},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    reports::{FillReport, OrderStatusReport},
};
use pyo3::{IntoPyObjectExt, prelude::*};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::KRAKEN_VENUE,
        enums::{KrakenEnvironment, KrakenProductType},
        urls::get_kraken_ws_private_url,
    },
    config::KrakenDataClientConfig,
    websocket::spot_v2::{
        client::KrakenSpotWebSocketClient,
        messages::KrakenSpotWsMessage,
        parse::{
            parse_book_deltas, parse_quote_tick, parse_trade_tick, parse_ws_bar,
            parse_ws_fill_report, parse_ws_order_status_report,
        },
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KrakenSpotWebSocketClient {
    /// WebSocket client for the Kraken Spot v2 streaming API.
    #[new]
    #[pyo3(signature = (environment=None, private=false, base_url=None, heartbeat_secs=None, api_key=None, api_secret=None, proxy_url=None))]
    fn py_new(
        environment: Option<KrakenEnvironment>,
        private: bool,
        base_url: Option<String>,
        heartbeat_secs: Option<u64>,
        api_key: Option<String>,
        api_secret: Option<String>,
        proxy_url: Option<String>,
    ) -> Self {
        let env = environment.unwrap_or(KrakenEnvironment::Mainnet);

        let (resolved_api_key, resolved_api_secret) =
            crate::common::credential::KrakenCredential::resolve_spot(api_key, api_secret)
                .map(|c| c.into_parts())
                .map_or((None, None), |(k, s)| (Some(k), Some(s)));

        let (ws_public_url, ws_private_url) = if private {
            // Use provided URL or default to the private endpoint
            let private_url = base_url.unwrap_or_else(|| {
                get_kraken_ws_private_url(KrakenProductType::Spot, env).to_string()
            });
            (None, Some(private_url))
        } else {
            (base_url, None)
        };

        let config = KrakenDataClientConfig {
            environment: env,
            ws_public_url,
            ws_private_url,
            heartbeat_interval_secs: heartbeat_secs
                .unwrap_or(KrakenDataClientConfig::default().heartbeat_interval_secs),
            api_key: resolved_api_key,
            api_secret: resolved_api_secret,
            proxy_url: proxy_url.clone(),
            ..Default::default()
        };

        let token = CancellationToken::new();

        Self::new(config, token, proxy_url)
    }

    /// Returns the WebSocket URL.
    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    /// Returns true if connected (not closed).
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Returns true if the connection is active.
    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    /// Returns true if the connection is closed.
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Returns all active subscriptions.
    #[pyo3(name = "get_subscriptions")]
    fn py_get_subscriptions(&self) -> Vec<String> {
        self.get_subscriptions()
    }

    /// Cancels all pending requests.
    #[pyo3(name = "cancel_all_requests")]
    fn py_cancel_all_requests(&self) {
        self.cancel_all_requests();
    }

    /// Connects to the WebSocket server.
    #[pyo3(name = "connect")]
    #[expect(clippy::needless_pass_by_value)]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let instruments_map = Arc::new(AtomicMap::<InstrumentId, InstrumentAny>::new());

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_map.insert(inst_any.id(), inst_any);
        }

        let account_id = self.account_id_shared().clone();
        let truncated_id_map = self.truncated_id_map().clone();
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream().map_err(to_pyruntime_err)?;
            let clock = get_atomic_clock_realtime();
            let book_sequence = Arc::new(AtomicU64::new(0));

            get_runtime().spawn(async move {
                tokio::pin!(stream);
                let order_qty_cache: Arc<AtomicMap<String, f64>> =
                    Arc::new(AtomicMap::new());

                while let Some(msg) = stream.next().await {
                    let ts_init = clock.get_time_ns();

                    match msg {
                        KrakenSpotWsMessage::Ticker(tickers) => {
                            for ticker in &tickers {
                                let instrument_id = InstrumentId::new(
                                    Symbol::new(ticker.symbol.as_str()),
                                    *KRAKEN_VENUE,
                                );
                                let instrument =
                                    instruments_map.load().get(&instrument_id).cloned();

                                if let Some(ref inst) = instrument {
                                    match parse_quote_tick(ticker, inst, ts_init) {
                                        Ok(quote) => {
                                            Python::attach(|py| {
                                                let py_obj =
                                                    data_to_pycapsule(py, Data::Quote(quote));
                                                call_python_threadsafe(
                                                    py, &call_soon, &callback, py_obj,
                                                );
                                            });
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse quote tick: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        KrakenSpotWsMessage::Trade(trades) => {
                            for trade in &trades {
                                let instrument_id = InstrumentId::new(
                                    Symbol::new(trade.symbol.as_str()),
                                    *KRAKEN_VENUE,
                                );
                                let instrument =
                                    instruments_map.load().get(&instrument_id).cloned();

                                if let Some(ref inst) = instrument {
                                    match parse_trade_tick(trade, inst, ts_init) {
                                        Ok(tick) => {
                                            Python::attach(|py| {
                                                let py_obj =
                                                    data_to_pycapsule(py, Data::Trade(tick));
                                                call_python_threadsafe(
                                                    py, &call_soon, &callback, py_obj,
                                                );
                                            });
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse trade tick: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        KrakenSpotWsMessage::Book {
                            data,
                            is_snapshot: _,
                        } => {
                            for book in &data {
                                let instrument_id = InstrumentId::new(
                                    Symbol::new(book.symbol.as_str()),
                                    *KRAKEN_VENUE,
                                );
                                let instrument =
                                    instruments_map.load().get(&instrument_id).cloned();

                                if let Some(ref inst) = instrument {
                                    let sequence = book_sequence.fetch_add(1, Ordering::Relaxed);
                                    match parse_book_deltas(book, inst, sequence, ts_init) {
                                        Ok(delta_vec) => {
                                            if delta_vec.is_empty() {
                                                continue;
                                            }
                                            let deltas = OrderBookDeltas::new(inst.id(), delta_vec);
                                            Python::attach(|py| {
                                                let py_obj = data_to_pycapsule(
                                                    py,
                                                    Data::Deltas(OrderBookDeltas_API::new(deltas)),
                                                );
                                                call_python_threadsafe(
                                                    py, &call_soon, &callback, py_obj,
                                                );
                                            });
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse book deltas: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        KrakenSpotWsMessage::Ohlc(ohlc_data) => {
                            for ohlc in &ohlc_data {
                                let instrument_id = InstrumentId::new(
                                    Symbol::new(ohlc.symbol.as_str()),
                                    *KRAKEN_VENUE,
                                );
                                let instrument =
                                    instruments_map.load().get(&instrument_id).cloned();

                                if let Some(ref inst) = instrument {
                                    match parse_ws_bar(ohlc, inst, ts_init) {
                                        Ok(bar) => {
                                            Python::attach(|py| {
                                                let py_obj = data_to_pycapsule(py, Data::Bar(bar));
                                                call_python_threadsafe(
                                                    py, &call_soon, &callback, py_obj,
                                                );
                                            });
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse bar: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        KrakenSpotWsMessage::Execution(executions) => {
                            let acct_id = account_id.read().ok().and_then(|g| *g);
                            let Some(acct_id) = acct_id else {
                                log::trace!(
                                    "Execution message received but no account_id set (data-only client)"
                                );
                                continue;
                            };

                            for exec in &executions {
                                let symbol = match &exec.symbol {
                                    Some(s) => s.as_str(),
                                    None => {
                                        log::debug!(
                                            "Execution without symbol: exec_type={:?}, order_id={}",
                                            exec.exec_type,
                                            exec.order_id
                                        );
                                        continue;
                                    }
                                };

                                let instrument_id = InstrumentId::new(
                                    Symbol::new(symbol),
                                    *KRAKEN_VENUE,
                                );
                                let instrument =
                                    instruments_map.load().get(&instrument_id).cloned();

                                let Some(ref inst) = instrument else {
                                    log::warn!("No instrument for symbol: {symbol}");
                                    continue;
                                };

                                let cached_qty = exec.cl_ord_id.as_ref().and_then(|id| {
                                    order_qty_cache.load().get(id).copied()
                                });

                                if let (Some(qty), Some(cl_ord_id)) =
                                    (exec.order_qty, &exec.cl_ord_id)
                                {
                                    order_qty_cache.insert(cl_ord_id.clone(), qty);
                                }

                                match parse_ws_order_status_report(
                                    exec, inst, acct_id, cached_qty, ts_init,
                                ) {
                                    Ok(mut report) => {
                                        if let Some(ref cl_ord_id) = exec.cl_ord_id {
                                            let full_id = truncated_id_map
                                                .load()
                                                .get(cl_ord_id)
                                                .copied()
                                                .unwrap_or_else(|| ClientOrderId::new(cl_ord_id));
                                            report = report.with_client_order_id(full_id);
                                        }
                                        dispatch_order_status_report(
                                            report, &call_soon, &callback,
                                        );
                                    }
                                    Err(e) => {
                                        log::error!("Failed to parse order status report: {e}");
                                    }
                                }

                                if exec.exec_id.is_some() {
                                    match parse_ws_fill_report(exec, inst, acct_id, ts_init) {
                                        Ok(mut report) => {
                                            if let Some(ref cl_ord_id) = exec.cl_ord_id {
                                                let full_id = truncated_id_map
                                                    .load()
                                                    .get(cl_ord_id)
                                                    .copied()
                                                    .unwrap_or_else(|| {
                                                        ClientOrderId::new(cl_ord_id)
                                                    });
                                                report.client_order_id = Some(full_id);
                                            }
                                            dispatch_fill_report(report, &call_soon, &callback);
                                        }
                                        Err(e) => {
                                            log::error!("Failed to parse fill report: {e}");
                                        }
                                    }
                                }
                            }
                        }
                        KrakenSpotWsMessage::Reconnected => {
                            log::info!("WebSocket reconnected");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    /// Waits until the connection is active or timeout.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Authenticates with the Kraken API to enable private subscriptions.
    #[pyo3(name = "authenticate")]
    fn py_authenticate<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.authenticate().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Returns true if the WebSocket is authenticated for private subscriptions.
    #[pyo3(name = "is_authenticated")]
    fn py_is_authenticated(&self) -> bool {
        self.is_authenticated()
    }

    /// Waits until the WebSocket is authenticated or the timeout elapses.
    ///
    /// Returns an error on timeout or explicit auth failure.
    #[pyo3(name = "wait_until_authenticated")]
    fn py_wait_until_authenticated<'py>(
        &self,
        py: Python<'py>,
        timeout_secs: f64,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .wait_until_authenticated(timeout_secs)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Disconnects from the WebSocket server.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Sends a ping message to keep the connection alive.
    #[pyo3(name = "send_ping")]
    fn py_send_ping<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.send_ping().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Closes the WebSocket connection.
    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Sets the account ID for execution report parsing.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Caches an instrument for execution report parsing.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python, instrument: Py<PyAny>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    /// Caches a client order for truncated ID resolution.
    #[pyo3(name = "cache_client_order")]
    fn py_cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) {
        self.cache_client_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            trader_id,
            strategy_id,
        );
    }

    /// Subscribes to order book updates for the given instrument.
    #[pyo3(name = "subscribe_book")]
    fn py_subscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book(instrument_id, depth)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to quote updates for the given instrument.
    ///
    /// Uses the Ticker channel with `event_trigger: "bbo"` for updates only on
    /// best bid/offer changes.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to trade updates for the given instrument.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to bar/OHLC updates for the given bar type.
    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to execution updates (order and fill events).
    ///
    /// Requires authentication - call `authenticate()` first.
    #[pyo3(name = "subscribe_executions")]
    #[pyo3(signature = (snap_orders=true, snap_trades=true))]
    fn py_subscribe_executions<'py>(
        &self,
        py: Python<'py>,
        snap_orders: bool,
        snap_trades: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_executions(snap_orders, snap_trades)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from order book updates for the given instrument.
    #[pyo3(name = "unsubscribe_book")]
    fn py_unsubscribe_book<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from quote updates for the given instrument.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from trade updates for the given instrument.
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
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from bar/OHLC updates for the given bar type.
    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_bars(bar_type)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}

fn dispatch_order_status_report(
    report: OrderStatusReport,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    Python::attach(|py| match report.into_py_any(py) {
        Ok(py_obj) => {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
        Err(e) => {
            log::error!("Failed to convert OrderStatusReport to Python: {e}");
        }
    });
}

fn dispatch_fill_report(report: FillReport, call_soon: &Py<PyAny>, callback: &Py<PyAny>) {
    Python::attach(|py| match report.into_py_any(py) {
        Ok(py_obj) => {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
        Err(e) => {
            log::error!("Failed to convert FillReport to Python: {e}");
        }
    });
}
