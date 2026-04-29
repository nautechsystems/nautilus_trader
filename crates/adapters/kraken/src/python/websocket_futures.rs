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

//! Python bindings for the Kraken Futures WebSocket client.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicU64, Ordering},
};

use ahash::AHashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    AtomicMap, UnixNanos,
    python::{call_python_threadsafe, to_pyruntime_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{Data, OrderBookDeltas, OrderBookDeltas_API, QuoteTick},
    enums::{BookType, OrderSide, OrderStatus, OrderType, TimeInForce},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    reports::{FillReport, OrderStatusReport},
    types::Quantity,
};
use nautilus_network::websocket::{SubscriptionState, TransportBackend};
use pyo3::{IntoPyObjectExt, prelude::*};

use crate::{
    common::{
        consts::KRAKEN_VENUE,
        credential::KrakenCredential,
        enums::{KrakenEnvironment, KrakenProductType},
        urls::get_kraken_ws_public_url,
    },
    websocket::futures::{
        client::KrakenFuturesWebSocketClient,
        messages::{
            KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesFillsDelta,
            KrakenFuturesOpenOrdersCancel, KrakenFuturesOpenOrdersDelta, KrakenFuturesTickerData,
            KrakenFuturesTradeData, KrakenFuturesWsMessage,
        },
        parse::{
            parse_futures_ws_book_delta, parse_futures_ws_book_snapshot_deltas,
            parse_futures_ws_fill_report, parse_futures_ws_funding_rate,
            parse_futures_ws_index_price, parse_futures_ws_mark_price,
            parse_futures_ws_order_status_report, parse_futures_ws_trade_tick,
        },
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl KrakenFuturesWebSocketClient {
    /// WebSocket client for the Kraken Futures v1 streaming API.
    #[new]
    #[pyo3(signature = (environment=None, base_url=None, heartbeat_secs=60, api_key=None, api_secret=None, proxy_url=None))]
    fn py_new(
        environment: Option<KrakenEnvironment>,
        base_url: Option<String>,
        heartbeat_secs: u64,
        api_key: Option<String>,
        api_secret: Option<String>,
        proxy_url: Option<String>,
    ) -> Self {
        let env = environment.unwrap_or(KrakenEnvironment::Mainnet);
        let demo = env == KrakenEnvironment::Demo;
        let url = base_url.unwrap_or_else(|| {
            get_kraken_ws_public_url(KrakenProductType::Futures, env).to_string()
        });
        let credential = KrakenCredential::resolve_futures(api_key, api_secret, demo);

        Self::with_credentials(
            url,
            heartbeat_secs,
            credential,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Returns true if the client has API credentials set.
    #[getter]
    #[pyo3(name = "has_credentials")]
    #[must_use]
    pub fn py_has_credentials(&self) -> bool {
        self.has_credentials()
    }

    /// Returns the WebSocket URL.
    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.url()
    }

    /// Returns true if the connection is closed.
    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        self.is_closed()
    }

    /// Returns true if the connection is active.
    #[pyo3(name = "is_active")]
    fn py_is_active(&self) -> bool {
        self.is_active()
    }

    /// Waits until the WebSocket connection is active or timeout.
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

    /// Returns true if the WebSocket is authenticated for private feeds.
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

    /// Authenticates the WebSocket connection for private feeds.
    ///
    /// Sends a challenge request and waits for the handler to parse the response,
    /// sign it, and mark the `AuthTracker` successful. Private subscriptions gate
    /// on the stored challenge / signed-challenge pair.
    #[pyo3(name = "authenticate")]
    fn py_authenticate<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.authenticate().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
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

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            self.cache_instrument(inst_any);
        }

        let instruments_map = self.instruments_shared().clone();
        let subscriptions = self.subscriptions().clone();
        let account_id = self.account_id_shared().clone();
        let truncated_id_map = self.truncated_id_map().clone();
        let order_instrument_map = self.order_instrument_map().clone();
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            if let Some(mut rx) = client.take_output_rx() {
                let clock = get_atomic_clock_realtime();
                let book_sequence = Arc::new(AtomicU64::new(0));

                get_runtime().spawn(async move {
                    let mut order_books: AHashMap<InstrumentId, OrderBook> = AHashMap::new();
                    let mut last_quotes: AHashMap<InstrumentId, QuoteTick> = AHashMap::new();
                    let venue_client_map: Arc<AtomicMap<String, ClientOrderId>> =
                        Arc::new(AtomicMap::new());
                    let venue_order_qty: Arc<AtomicMap<String, Quantity>> =
                        Arc::new(AtomicMap::new());

                    while let Some(msg) = rx.recv().await {
                        let ts_init = clock.get_time_ns();

                        match msg {
                            KrakenFuturesWsMessage::OpenOrdersDelta(delta) => {
                                handle_open_orders_delta(
                                    &delta,
                                    &instruments_map,
                                    &account_id,
                                    &truncated_id_map,
                                    &order_instrument_map,
                                    &venue_client_map,
                                    &venue_order_qty,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::OpenOrdersCancel(cancel) => {
                                handle_open_orders_cancel(
                                    &cancel,
                                    &account_id,
                                    &truncated_id_map,
                                    &order_instrument_map,
                                    &venue_client_map,
                                    &venue_order_qty,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::FillsDelta(fills_delta) => {
                                handle_fills_delta(
                                    &fills_delta,
                                    &instruments_map,
                                    &account_id,
                                    &truncated_id_map,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::Ticker(ref ticker) => {
                                handle_ticker(
                                    ticker,
                                    &instruments_map,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::Trade(ref trade) => {
                                handle_trade(
                                    trade,
                                    &instruments_map,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::BookSnapshot(ref snapshot) => {
                                handle_book_snapshot(
                                    snapshot,
                                    &instruments_map,
                                    &subscriptions,
                                    &mut order_books,
                                    &mut last_quotes,
                                    &book_sequence,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::BookDelta(ref delta) => {
                                handle_book_delta(
                                    delta,
                                    &instruments_map,
                                    &subscriptions,
                                    &mut order_books,
                                    &mut last_quotes,
                                    &book_sequence,
                                    ts_init,
                                    &call_soon,
                                    &callback,
                                );
                            }
                            KrakenFuturesWsMessage::Challenge(_)
                            | KrakenFuturesWsMessage::Reconnected => {}
                        }
                    }
                });
            }

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

    /// Caches multiple instruments for execution report parsing.
    #[pyo3(name = "cache_instruments")]
    fn py_cache_instruments(&self, py: Python, instruments: Vec<Py<PyAny>>) -> PyResult<()> {
        let mut inst_vec = Vec::with_capacity(instruments.len());
        for inst in instruments {
            inst_vec.push(pyobject_to_instrument_any(py, inst)?);
        }
        self.cache_instruments(&inst_vec);
        Ok(())
    }

    /// Caches a client order for truncated ID resolution and instrument lookup.
    ///
    /// Kraken Futures limits client order IDs to 18 characters, so orders with
    /// longer IDs are truncated. This method stores the mapping from truncated
    /// to full ID, and from venue order ID to instrument ID for cancel messages.
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

    /// Disconnects from the WebSocket server.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyruntime_err)?;
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

    /// Subscribes to order book updates for the given instrument.
    ///
    /// Note: The `depth` parameter is accepted for API compatibility with spot client but is
    /// not used by Kraken Futures (full book is always returned).
    #[pyo3(name = "subscribe_book")]
    #[pyo3(signature = (instrument_id, depth=None))]
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
    /// Uses the order book channel for low-latency top-of-book quotes.
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

    /// Subscribes to mark price updates for the given instrument.
    #[pyo3(name = "subscribe_mark_price")]
    fn py_subscribe_mark_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_mark_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to index price updates for the given instrument.
    #[pyo3(name = "subscribe_index_price")]
    fn py_subscribe_index_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_index_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to funding rate updates for the given instrument.
    #[pyo3(name = "subscribe_funding_rate")]
    fn py_subscribe_funding_rate<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_funding_rate(instrument_id)
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

    /// Unsubscribes from mark price updates for the given instrument.
    #[pyo3(name = "unsubscribe_mark_price")]
    fn py_unsubscribe_mark_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_mark_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from index price updates for the given instrument.
    #[pyo3(name = "unsubscribe_index_price")]
    fn py_unsubscribe_index_price<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_index_price(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from funding rate updates for the given instrument.
    #[pyo3(name = "unsubscribe_funding_rate")]
    fn py_unsubscribe_funding_rate<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_funding_rate(instrument_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Sign a challenge with the API credentials.
    ///
    /// Returns the signed challenge on success.
    #[pyo3(name = "sign_challenge")]
    fn py_sign_challenge(&self, challenge: &str) -> PyResult<String> {
        self.sign_challenge(challenge).map_err(to_pyruntime_err)
    }

    /// Complete authentication with a received challenge.
    #[pyo3(name = "authenticate_with_challenge")]
    fn py_authenticate_with_challenge<'py>(
        &self,
        py: Python<'py>,
        challenge: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .authenticate_with_challenge(&challenge)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Set authentication credentials directly (for when challenge is obtained externally).
    #[pyo3(name = "set_auth_credentials")]
    fn py_set_auth_credentials<'py>(
        &self,
        py: Python<'py>,
        original_challenge: String,
        signed_challenge: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .set_auth_credentials(original_challenge, signed_challenge)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to open orders feed (private, requires authentication).
    #[pyo3(name = "subscribe_open_orders")]
    fn py_subscribe_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_open_orders()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to fills feed (private, requires authentication).
    #[pyo3(name = "subscribe_fills")]
    fn py_subscribe_fills<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_fills().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    /// Subscribes to both open orders and fills (convenience method).
    #[pyo3(name = "subscribe_executions")]
    fn py_subscribe_executions<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_executions()
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }
}

fn lookup_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    product_id: &str,
) -> Option<InstrumentAny> {
    let instrument_id = InstrumentId::new(Symbol::new(product_id), *KRAKEN_VENUE);
    instruments.load().get(&instrument_id).cloned()
}

fn resolve_client_order_id(
    truncated: &str,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
) -> ClientOrderId {
    truncated_id_map
        .load()
        .get(truncated)
        .copied()
        .unwrap_or_else(|| ClientOrderId::new(truncated))
}

fn dispatch_report_to_python(
    report: OrderStatusReport,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    Python::attach(|py| match report.into_py_any(py) {
        Ok(py_obj) => call_python_threadsafe(py, call_soon, callback, py_obj),
        Err(e) => log::error!("Failed to convert OrderStatusReport to Python: {e}"),
    });
}

fn dispatch_fill_to_python(report: FillReport, call_soon: &Py<PyAny>, callback: &Py<PyAny>) {
    Python::attach(|py| match report.into_py_any(py) {
        Ok(py_obj) => call_python_threadsafe(py, call_soon, callback, py_obj),
        Err(e) => log::error!("Failed to convert FillReport to Python: {e}"),
    });
}

#[expect(clippy::too_many_arguments)]
fn handle_open_orders_delta(
    delta: &KrakenFuturesOpenOrdersDelta,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    account_id: &Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: &Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: &Arc<AtomicMap<String, Quantity>>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    // The fills delta carries the real fill; skip the cancel-shaped delta
    // Kraken emits when an order leaves the book because it filled.
    if delta.is_fill_driven_cancel() {
        log::debug!(
            "Skipping fill-driven open_orders delta: order_id={}, reason={:?}",
            delta.order.order_id,
            delta.reason,
        );
        return;
    }

    let product_id = delta.order.instrument.as_str();

    let Some(instrument) = lookup_instrument(instruments, product_id) else {
        log::warn!("No instrument for product_id: {product_id}");
        return;
    };

    let Some(acct_id) = account_id.read().ok().and_then(|g| *g) else {
        log::warn!("Account ID not set, cannot process order delta");
        return;
    };

    order_instrument_map.insert(delta.order.order_id.clone(), instrument.id());

    let qty = Quantity::new(delta.order.qty, instrument.size_precision());
    venue_order_qty.insert(delta.order.order_id.clone(), qty);

    match parse_futures_ws_order_status_report(
        &delta.order,
        delta.is_cancel,
        delta.reason.as_deref(),
        &instrument,
        acct_id,
        ts_init,
    ) {
        Ok(mut report) => {
            if let Some(ref cl_ord_id) = delta.order.cli_ord_id {
                let full_id = resolve_client_order_id(cl_ord_id, truncated_id_map);
                report = report.with_client_order_id(full_id);

                venue_client_map.insert(delta.order.order_id.clone(), full_id);
            }
            dispatch_report_to_python(report, call_soon, callback);
        }
        Err(e) => log::error!("Failed to parse futures order status report: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_open_orders_cancel(
    cancel: &KrakenFuturesOpenOrdersCancel,
    account_id: &Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: &Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: &Arc<AtomicMap<String, Quantity>>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if let Some(ref reason) = cancel.reason
        && (reason == "full_fill" || reason == "partial_fill")
    {
        log::debug!(
            "Skipping fill-driven cancel: order_id={}, reason={reason}",
            cancel.order_id,
        );
        return;
    }

    let Some(acct_id) = account_id.read().ok().and_then(|g| *g) else {
        log::warn!("Account ID not set, cannot process order cancel");
        return;
    };

    let venue_order_id = VenueOrderId::new(&cancel.order_id);

    let instrument_id = order_instrument_map.load().get(&cancel.order_id).copied();

    let Some(instrument_id) = instrument_id else {
        log::warn!(
            "Cannot resolve instrument for cancel: order_id={}, \
             order not seen in previous delta",
            cancel.order_id
        );
        return;
    };

    let client_order_id = cancel
        .cli_ord_id
        .as_ref()
        .map(|id| resolve_client_order_id(id, truncated_id_map))
        .or_else(|| venue_client_map.load().get(&cancel.order_id).copied());

    let Some(quantity) = venue_order_qty.load().get(&cancel.order_id).copied() else {
        log::warn!(
            "Cannot resolve quantity for cancel: order_id={}, skipping",
            cancel.order_id
        );
        return;
    };

    let report = OrderStatusReport::new(
        acct_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        OrderSide::NoOrderSide,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        quantity,
        Quantity::zero(0),
        ts_init,
        ts_init,
        ts_init,
        None,
    );

    let report = if let Some(ref reason) = cancel.reason
        && !reason.is_empty()
    {
        report.with_cancel_reason(reason.clone())
    } else {
        report
    };

    dispatch_report_to_python(report, call_soon, callback);
}

fn handle_fills_delta(
    fills_delta: &KrakenFuturesFillsDelta,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    account_id: &Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(acct_id) = account_id.read().ok().and_then(|g| *g) else {
        log::warn!("Account ID not set, cannot process fills");
        return;
    };

    for fill in &fills_delta.fills {
        let product_id = match &fill.instrument {
            Some(id) => id.as_str(),
            None => {
                log::warn!("Fill missing instrument field: fill_id={}", fill.fill_id);
                continue;
            }
        };

        let Some(instrument) = lookup_instrument(instruments, product_id) else {
            log::warn!("No instrument for product_id: {product_id}");
            continue;
        };

        match parse_futures_ws_fill_report(fill, &instrument, acct_id, ts_init) {
            Ok(mut report) => {
                if let Some(ref cl_ord_id) = fill.cli_ord_id {
                    let full_id = resolve_client_order_id(cl_ord_id, truncated_id_map);
                    report.client_order_id = Some(full_id);
                }
                dispatch_fill_to_python(report, call_soon, callback);
            }
            Err(e) => log::error!("Failed to parse futures fill report: {e}"),
        }
    }
}

fn handle_ticker(
    ticker: &KrakenFuturesTickerData,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = lookup_instrument(instruments, ticker.product_id.as_str()) else {
        return;
    };

    if let Some(mark_price) = parse_futures_ws_mark_price(ticker, &instrument, ts_init) {
        Python::attach(|py| {
            let py_obj = data_to_pycapsule(py, Data::MarkPriceUpdate(mark_price));
            call_python_threadsafe(py, call_soon, callback, py_obj);
        });
    }

    if let Some(index_price) = parse_futures_ws_index_price(ticker, &instrument, ts_init) {
        Python::attach(|py| {
            let py_obj = data_to_pycapsule(py, Data::IndexPriceUpdate(index_price));
            call_python_threadsafe(py, call_soon, callback, py_obj);
        });
    }

    if let Some(funding_rate) = parse_futures_ws_funding_rate(ticker, &instrument, ts_init) {
        Python::attach(|py| match funding_rate.into_py_any(py) {
            Ok(py_obj) => call_python_threadsafe(py, call_soon, callback, py_obj),
            Err(e) => log::error!("Failed to convert FundingRateUpdate to Python: {e}"),
        });
    }
}

fn handle_trade(
    trade: &KrakenFuturesTradeData,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = lookup_instrument(instruments, trade.product_id.as_str()) else {
        return;
    };

    match parse_futures_ws_trade_tick(trade, &instrument, ts_init) {
        Ok(tick) => {
            Python::attach(|py| {
                let py_obj = data_to_pycapsule(py, Data::Trade(tick));
                call_python_threadsafe(py, call_soon, callback, py_obj);
            });
        }
        Err(e) => log::error!("Failed to parse futures trade tick: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_book_snapshot(
    snapshot: &KrakenFuturesBookSnapshot,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    subscriptions: &SubscriptionState,
    order_books: &mut AHashMap<InstrumentId, OrderBook>,
    last_quotes: &mut AHashMap<InstrumentId, QuoteTick>,
    book_sequence: &Arc<AtomicU64>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = lookup_instrument(instruments, snapshot.product_id.as_str()) else {
        return;
    };
    let instrument_id = instrument.id();

    let sequence = book_sequence.fetch_add(
        (snapshot.bids.len() + snapshot.asks.len() + 1) as u64,
        Ordering::Relaxed,
    );

    match parse_futures_ws_book_snapshot_deltas(snapshot, &instrument, sequence, ts_init) {
        Ok(delta_vec) => {
            if delta_vec.is_empty() {
                return;
            }
            let deltas = OrderBookDeltas::new(instrument_id, delta_vec);

            let quotes_key = format!("quotes:{}", snapshot.product_id);
            if subscriptions.get_reference_count(&quotes_key) > 0 {
                let book = order_books
                    .entry(instrument_id)
                    .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

                if let Err(e) = book.apply_deltas(&deltas) {
                    log::error!("Failed to apply snapshot deltas to order book: {e}");
                } else {
                    maybe_emit_quote(
                        book,
                        instrument_id,
                        last_quotes,
                        ts_init,
                        call_soon,
                        callback,
                    );
                }
            }

            let deltas_key = format!("deltas:{}", snapshot.product_id);
            if subscriptions.get_reference_count(&deltas_key) > 0 {
                Python::attach(|py| {
                    let py_obj =
                        data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                    call_python_threadsafe(py, call_soon, callback, py_obj);
                });
            }
        }
        Err(e) => log::error!("Failed to parse futures book snapshot: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_book_delta(
    delta: &KrakenFuturesBookDelta,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    subscriptions: &SubscriptionState,
    order_books: &mut AHashMap<InstrumentId, OrderBook>,
    last_quotes: &mut AHashMap<InstrumentId, QuoteTick>,
    book_sequence: &Arc<AtomicU64>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let Some(instrument) = lookup_instrument(instruments, delta.product_id.as_str()) else {
        return;
    };
    let instrument_id = instrument.id();

    let sequence = book_sequence.fetch_add(1, Ordering::Relaxed);

    match parse_futures_ws_book_delta(delta, &instrument, sequence, ts_init) {
        Ok(book_delta) => {
            let deltas = OrderBookDeltas::new(instrument_id, vec![book_delta]);

            let quotes_key = format!("quotes:{}", delta.product_id);
            if subscriptions.get_reference_count(&quotes_key) > 0
                && let Some(book) = order_books.get_mut(&instrument_id)
            {
                if let Err(e) = book.apply_deltas(&deltas) {
                    log::error!("Failed to apply delta to order book: {e}");
                } else {
                    maybe_emit_quote(
                        book,
                        instrument_id,
                        last_quotes,
                        ts_init,
                        call_soon,
                        callback,
                    );
                }
            }

            let deltas_key = format!("deltas:{}", delta.product_id);
            if subscriptions.get_reference_count(&deltas_key) > 0 {
                Python::attach(|py| {
                    let py_obj =
                        data_to_pycapsule(py, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                    call_python_threadsafe(py, call_soon, callback, py_obj);
                });
            }
        }
        Err(e) => log::error!("Failed to parse futures book delta: {e}"),
    }
}

fn maybe_emit_quote(
    book: &OrderBook,
    instrument_id: InstrumentId,
    last_quotes: &mut AHashMap<InstrumentId, QuoteTick>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    let (Some(bid_price), Some(ask_price)) = (book.best_bid_price(), book.best_ask_price()) else {
        return;
    };
    let (Some(bid_size), Some(ask_size)) = (book.best_bid_size(), book.best_ask_size()) else {
        return;
    };

    let bid = bid_price.as_f64();
    let ask = ask_price.as_f64();
    if bid > 0.0 && (ask - bid) / bid > 0.25 {
        log::debug!("Filtered quote with wide spread: bid={bid}, ask={ask}");
        return;
    }

    let quote = QuoteTick::new(
        instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        ts_init,
        ts_init,
    );

    if matches!(last_quotes.get(&instrument_id), Some(prev) if *prev == quote) {
        return;
    }

    last_quotes.insert(instrument_id, quote);

    Python::attach(|py| {
        let py_obj = data_to_pycapsule(py, Data::Quote(quote));
        call_python_threadsafe(py, call_soon, callback, py_obj);
    });
}
