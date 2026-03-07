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

//! Python bindings for Ax WebSocket clients.
//!
//! [`PyAxMdWebSocketClient`] and [`PyAxOrdersWebSocketClient`] wrap the Rust clients
//! and add instrument caches at the Python boundary. The inner clients are pure network
//! components that emit venue-specific types; these wrappers parse them into Nautilus
//! domain objects before passing them to Python callbacks.

use std::{fmt::Debug, sync::Arc};

use ahash::AHashMap;
use dashmap::DashMap;
use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    UUID4, UnixNanos,
    python::{call_python_threadsafe, to_pyruntime_err},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{BarType, Data, OrderBookDeltas_API},
    enums::{OrderSide, OrderType, TimeInForce},
    events::OrderCancelRejected,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{Price, Quantity},
};
use pyo3::{IntoPyObjectExt, prelude::*};
use ustr::Ustr;

use crate::{
    common::enums::{AxCandleWidth, AxMarketDataLevel},
    execution::{
        cleanup_terminal_order_tracking, create_order_accepted, create_order_canceled,
        create_order_expired, create_order_filled, create_order_rejected,
    },
    http::models::AxOrderRejectReason,
    websocket::{
        data::{
            AxMdWebSocketClient,
            client::SymbolDataTypes,
            parse::{
                parse_book_l1_quote, parse_book_l2_deltas, parse_book_l3_deltas, parse_candle_bar,
                parse_trade_tick,
            },
        },
        messages::{AxDataWsMessage, AxMdCandle, AxMdMessage, AxOrdersWsMessage, AxWsOrderEvent},
        orders::{AxOrdersWebSocketClient, OrdersCaches},
    },
};

/// Python wrapper around [`AxMdWebSocketClient`] that holds an instrument cache
/// at the Python boundary for parsing venue messages into Nautilus domain types.
#[pyclass(
    name = "AxMdWebSocketClient",
    module = "nautilus_trader.core.nautilus_pyo3.architect"
)]
pub struct PyAxMdWebSocketClient {
    inner: AxMdWebSocketClient,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
}

impl Debug for PyAxMdWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyAxMdWebSocketClient))
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PyAxMdWebSocketClient {
    #[new]
    #[pyo3(signature = (url, auth_token, heartbeat=None))]
    fn py_new(url: String, auth_token: String, heartbeat: Option<u64>) -> Self {
        Self {
            inner: AxMdWebSocketClient::new(url, auth_token, heartbeat),
            instruments_cache: Arc::new(DashMap::new()),
        }
    }

    #[staticmethod]
    #[pyo3(name = "without_auth")]
    #[pyo3(signature = (url, heartbeat=None))]
    fn py_without_auth(url: String, heartbeat: Option<u64>) -> Self {
        Self {
            inner: AxMdWebSocketClient::without_auth(url, heartbeat),
            instruments_cache: Arc::new(DashMap::new()),
        }
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.inner.url()
    }

    #[pyo3(name = "is_active")]
    #[must_use]
    pub fn py_is_active(&self) -> bool {
        self.inner.is_active()
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    pub fn py_is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    #[pyo3(name = "subscription_count")]
    #[must_use]
    pub fn py_subscription_count(&self) -> usize {
        self.inner.subscription_count()
    }

    #[pyo3(name = "set_auth_token")]
    fn py_set_auth_token(&mut self, token: String) {
        self.inner.set_auth_token(token);
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        let inst = pyobject_to_instrument_any(py, instrument)?;
        let symbol = inst.symbol().inner();
        self.instruments_cache.insert(symbol, inst);
        Ok(())
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        callback: Py<PyAny>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let clock = get_atomic_clock_realtime();
        let instruments = Arc::clone(&self.instruments_cache);
        let symbol_data_types = self.inner.symbol_data_types();
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyruntime_err)?;

            let stream = client.stream();

            get_runtime().spawn(async move {
                let _client = client;
                tokio::pin!(stream);

                let mut book_sequences: AHashMap<Ustr, u64> = AHashMap::new();
                let mut candle_cache: AHashMap<(Ustr, AxCandleWidth), AxMdCandle> = AHashMap::new();

                while let Some(msg) = stream.next().await {
                    let ts_init = clock.get_time_ns();

                    match msg {
                        AxDataWsMessage::MdMessage(md_msg) => {
                            handle_md_message(
                                md_msg,
                                &instruments,
                                &symbol_data_types,
                                &mut book_sequences,
                                &mut candle_cache,
                                ts_init,
                                &call_soon,
                                &callback,
                            );
                        }
                        AxDataWsMessage::Reconnected => {
                            candle_cache.clear();
                            log::info!("AX WebSocket reconnected");
                        }
                        AxDataWsMessage::CandleUnsubscribed { symbol, width } => {
                            candle_cache.remove(&(symbol, width));
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "subscribe_book_deltas")]
    fn py_subscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
        level: AxMarketDataLevel,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_book_deltas(&symbol, level)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_quotes")]
    fn py_subscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_quotes(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_trades")]
    fn py_subscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_trades(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_book_deltas")]
    fn py_unsubscribe_book_deltas<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_book_deltas(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = AxCandleWidth::try_from(&bar_type.spec()).map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_candles(&symbol, width)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_quotes")]
    fn py_unsubscribe_quotes<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_quotes(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_trades")]
    fn py_unsubscribe_trades<'py>(
        &self,
        py: Python<'py>,
        instrument_id: InstrumentId,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = instrument_id.symbol.to_string();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_trades(&symbol)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = AxCandleWidth::try_from(&bar_type.spec()).map_err(to_pyruntime_err)?;

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(&symbol, width)
                .await
                .map_err(to_pyruntime_err)
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await;
            Ok(())
        })
    }
}

/// Python wrapper around [`AxOrdersWebSocketClient`] that handles order event
/// parsing at the Python boundary.
#[pyclass(
    name = "AxOrdersWebSocketClient",
    module = "nautilus_trader.core.nautilus_pyo3.architect"
)]
pub struct PyAxOrdersWebSocketClient {
    inner: AxOrdersWebSocketClient,
}

impl Debug for PyAxOrdersWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(PyAxOrdersWebSocketClient))
            .field("inner", &self.inner)
            .finish_non_exhaustive()
    }
}

#[pymethods]
impl PyAxOrdersWebSocketClient {
    #[new]
    #[pyo3(signature = (url, account_id, trader_id, heartbeat=None))]
    fn py_new(
        url: String,
        account_id: AccountId,
        trader_id: TraderId,
        heartbeat: Option<u64>,
    ) -> Self {
        Self {
            inner: AxOrdersWebSocketClient::new(url, account_id, trader_id, heartbeat),
        }
    }

    #[getter]
    #[pyo3(name = "url")]
    #[must_use]
    pub fn py_url(&self) -> &str {
        self.inner.url()
    }

    #[getter]
    #[pyo3(name = "account_id")]
    #[must_use]
    pub fn py_account_id(&self) -> AccountId {
        self.inner.account_id()
    }

    #[pyo3(name = "is_active")]
    #[must_use]
    pub fn py_is_active(&self) -> bool {
        self.inner.is_active()
    }

    #[pyo3(name = "is_closed")]
    #[must_use]
    pub fn py_is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, py: Python<'_>, instrument: Py<PyAny>) -> PyResult<()> {
        self.inner
            .cache_instrument(pyobject_to_instrument_any(py, instrument)?);
        Ok(())
    }

    #[pyo3(name = "register_external_order")]
    fn py_register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
    ) -> bool {
        self.inner.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
        )
    }

    #[pyo3(name = "connect")]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        callback: Py<PyAny>,
        bearer_token: String,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon: Py<PyAny> = loop_.getattr(py, "call_soon_threadsafe")?;

        let clock = get_atomic_clock_realtime();
        let account_id = self.inner.account_id();
        let caches = self.inner.caches().clone();
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .connect(&bearer_token)
                .await
                .map_err(to_pyruntime_err)?;

            let stream = client.stream();

            get_runtime().spawn(async move {
                let _client = client;
                tokio::pin!(stream);

                while let Some(msg) = stream.next().await {
                    match msg {
                        AxOrdersWsMessage::Event(event) => {
                            handle_order_event(
                                *event, &caches, account_id, clock, &call_soon, &callback,
                            );
                        }
                        AxOrdersWsMessage::PlaceOrderResponse(resp) => {
                            log::debug!(
                                "Place order response: rid={}, oid={}",
                                resp.rid,
                                resp.res.oid
                            );
                        }
                        AxOrdersWsMessage::CancelOrderResponse(resp) => {
                            log::debug!(
                                "Cancel order response: rid={}, received={}",
                                resp.rid,
                                resp.res.cxl_rx
                            );
                        }
                        AxOrdersWsMessage::OpenOrdersResponse(resp) => {
                            log::debug!(
                                "Open orders response: rid={}, count={}",
                                resp.rid,
                                resp.res.len()
                            );
                        }
                        AxOrdersWsMessage::Error(err) => {
                            log::error!(
                                "AX orders WebSocket error: code={:?}, message={}, rid={:?}",
                                err.code,
                                err.message,
                                err.request_id
                            );
                        }
                        AxOrdersWsMessage::Reconnected => {
                            log::info!("AX orders WebSocket reconnected");
                        }
                        AxOrdersWsMessage::Authenticated => {
                            log::info!("AX orders WebSocket authenticated");
                        }
                    }
                }
            });

            Ok(())
        })
    }

    #[pyo3(name = "submit_order")]
    #[pyo3(signature = (
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        order_side,
        order_type,
        quantity,
        time_in_force,
        price=None,
        trigger_price=None,
        post_only=false,
    ))]
    #[allow(clippy::too_many_arguments)]
    fn py_submit_order<'py>(
        &self,
        py: Python<'py>,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        price: Option<Price>,
        trigger_price: Option<Price>,
        post_only: bool,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    post_only,
                )
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "cancel_order")]
    #[pyo3(signature = (client_order_id, venue_order_id=None))]
    fn py_cancel_order<'py>(
        &self,
        py: Python<'py>,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .cancel_order(client_order_id, venue_order_id)
                .await
                .map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "get_open_orders")]
    fn py_get_open_orders<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.get_open_orders().await.map_err(to_pyruntime_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await;
            Ok(())
        })
    }

    #[pyo3(name = "close")]
    fn py_close<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.inner.clone();

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.close().await;
            Ok(())
        })
    }
}

#[allow(clippy::too_many_arguments)]
fn handle_md_message(
    message: AxMdMessage,
    instruments: &Arc<DashMap<Ustr, InstrumentAny>>,
    symbol_data_types: &Arc<DashMap<String, SymbolDataTypes>>,
    book_sequences: &mut AHashMap<Ustr, u64>,
    candle_cache: &mut AHashMap<(Ustr, AxCandleWidth), AxMdCandle>,
    ts_init: UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    match message {
        AxMdMessage::BookL1(book) => {
            let l1_subscribed = symbol_data_types
                .get(book.s.as_str())
                .is_some_and(|e| e.quotes || e.book_level == Some(AxMarketDataLevel::Level1));

            if !l1_subscribed {
                return;
            }

            let Some(instrument) = instruments.get(&book.s) else {
                log::warn!("Instrument cache miss for L1 book symbol={}", book.s);
                return;
            };

            match parse_book_l1_quote(&book, &instrument, ts_init) {
                Ok(quote) => {
                    send_data_to_python(Data::Quote(quote), call_soon, callback);
                }
                Err(e) => log::error!("Failed to parse L1 quote: {e}"),
            }
        }
        AxMdMessage::BookL2(book) => {
            let Some(instrument) = instruments.get(&book.s) else {
                log::warn!("Instrument cache miss for L2 book symbol={}", book.s);
                return;
            };

            let sequence = book_sequences
                .entry(book.s)
                .and_modify(|s| *s += 1)
                .or_insert(1);

            match parse_book_l2_deltas(&book, &instrument, *sequence, ts_init) {
                Ok(deltas) => {
                    send_data_to_python(
                        Data::Deltas(OrderBookDeltas_API::new(deltas)),
                        call_soon,
                        callback,
                    );
                }
                Err(e) => log::error!("Failed to parse L2 deltas: {e}"),
            }
        }
        AxMdMessage::BookL3(book) => {
            let Some(instrument) = instruments.get(&book.s) else {
                log::warn!("Instrument cache miss for L3 book symbol={}", book.s);
                return;
            };

            let sequence = book_sequences
                .entry(book.s)
                .and_modify(|s| *s += 1)
                .or_insert(1);

            match parse_book_l3_deltas(&book, &instrument, *sequence, ts_init) {
                Ok(deltas) => {
                    send_data_to_python(
                        Data::Deltas(OrderBookDeltas_API::new(deltas)),
                        call_soon,
                        callback,
                    );
                }
                Err(e) => log::error!("Failed to parse L3 deltas: {e}"),
            }
        }
        AxMdMessage::Trade(trade) => {
            let trades_subscribed = symbol_data_types
                .get(trade.s.as_str())
                .is_some_and(|e| e.trades);

            if !trades_subscribed {
                return;
            }

            let Some(instrument) = instruments.get(&trade.s) else {
                log::warn!("Instrument cache miss for trade symbol={}", trade.s);
                return;
            };

            match parse_trade_tick(&trade, &instrument, ts_init) {
                Ok(tick) => {
                    send_data_to_python(Data::Trade(tick), call_soon, callback);
                }
                Err(e) => log::error!("Failed to parse trade: {e}"),
            }
        }
        AxMdMessage::Candle(candle) => {
            let cache_key = (candle.symbol, candle.width);

            let closed_candle = if let Some(cached) = candle_cache.get(&cache_key) {
                if cached.ts == candle.ts {
                    None
                } else {
                    Some(cached.clone())
                }
            } else {
                None
            };

            candle_cache.insert(cache_key, candle);

            if let Some(closed) = closed_candle {
                let Some(instrument) = instruments.get(&closed.symbol) else {
                    log::warn!("Instrument cache miss for candle symbol={}", closed.symbol);
                    return;
                };

                match parse_candle_bar(&closed, &instrument, ts_init) {
                    Ok(bar) => {
                        send_data_to_python(Data::Bar(bar), call_soon, callback);
                    }
                    Err(e) => log::error!("Failed to parse candle: {e}"),
                }
            }
        }
        AxMdMessage::Ticker(_) => {}
        AxMdMessage::Heartbeat(_) => {}
        AxMdMessage::SubscriptionResponse(_) => {}
        AxMdMessage::Error(err) => {
            log::error!("AX market data error: {err:?}");
        }
    }
}

fn handle_order_event(
    event: AxWsOrderEvent,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    match event {
        AxWsOrderEvent::Heartbeat => {}
        AxWsOrderEvent::Acknowledged(msg) => {
            if let Some(event) = create_order_accepted(&msg.o, msg.ts, caches, account_id, clock) {
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::PartiallyFilled(msg) => {
            if let Some(event) =
                create_order_filled(&msg.o, &msg.xs, msg.ts, caches, account_id, clock)
            {
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::Filled(msg) => {
            if let Some(event) =
                create_order_filled(&msg.o, &msg.xs, msg.ts, caches, account_id, clock)
            {
                cleanup_terminal_order_tracking(&msg.o, caches);
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::Canceled(msg) => {
            if let Some(event) = create_order_canceled(&msg.o, msg.ts, caches, account_id, clock) {
                cleanup_terminal_order_tracking(&msg.o, caches);
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::Rejected(msg) => {
            let known_reason = msg.r.filter(|r| !matches!(r, AxOrderRejectReason::Unknown));
            let reason = known_reason
                .as_ref()
                .map(AsRef::as_ref)
                .or(msg.txt.as_deref())
                .unwrap_or("UNKNOWN");

            if let Some(event) =
                create_order_rejected(&msg.o, reason, msg.ts, caches, account_id, clock)
            {
                cleanup_terminal_order_tracking(&msg.o, caches);
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::Expired(msg) => {
            if let Some(event) = create_order_expired(&msg.o, msg.ts, caches, account_id, clock) {
                cleanup_terminal_order_tracking(&msg.o, caches);
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::Replaced(msg) => {
            if let Some(event) = create_order_accepted(&msg.o, msg.ts, caches, account_id, clock) {
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::DoneForDay(msg) => {
            if let Some(event) = create_order_expired(&msg.o, msg.ts, caches, account_id, clock) {
                cleanup_terminal_order_tracking(&msg.o, caches);
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            }
        }
        AxWsOrderEvent::CancelRejected(msg) => {
            let venue_order_id = VenueOrderId::new(&msg.oid);
            if let Some(client_order_id) = caches.venue_to_client_id.get(&venue_order_id)
                && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
            {
                let event = OrderCancelRejected::new(
                    metadata.trader_id,
                    metadata.strategy_id,
                    metadata.instrument_id,
                    metadata.client_order_id,
                    Ustr::from(msg.r.as_ref()),
                    UUID4::new(),
                    clock.get_time_ns(),
                    metadata.ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                call_python_with_event(call_soon, callback, move |py| event.into_py_any(py));
            } else {
                log::warn!(
                    "Could not find metadata for cancel rejected order {}",
                    msg.oid
                );
            }
        }
    }
}

fn send_data_to_python(data: Data, call_soon: &Py<PyAny>, callback: &Py<PyAny>) {
    Python::attach(|py| {
        let py_obj = data_to_pycapsule(py, data);
        call_python_threadsafe(py, call_soon, callback, py_obj);
    });
}

fn call_python_with_event<F>(call_soon: &Py<PyAny>, callback: &Py<PyAny>, event_fn: F)
where
    F: FnOnce(Python<'_>) -> PyResult<Py<PyAny>> + Send + 'static,
{
    Python::attach(|py| match event_fn(py) {
        Ok(py_obj) => {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
        Err(e) => {
            log::error!("Error converting event to Python: {e}");
        }
    });
}
