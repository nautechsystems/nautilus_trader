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

//! Python bindings for the dYdX WebSocket client.

use std::{
    str::FromStr,
    sync::atomic::Ordering,
    time::{Duration, Instant},
};

use ahash::AHashMap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_core::{
    UUID4,
    python::{call_python_threadsafe, to_pyvalue_err},
    time::get_atomic_clock_realtime,
};
use nautilus_model::{
    data::{
        Bar, BarType, Data, FundingRateUpdate, IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate,
        OrderBookDeltas, OrderBookDeltas_API,
    },
    enums::{AccountType, MarketStatusAction, OrderSide, OrderStatus, OrderType},
    events::{AccountState, OrderAccepted, OrderCanceled},
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    python::{data::data_to_pycapsule, instruments::pyobject_to_instrument_any},
    types::{AccountBalance, Currency, Money},
};
use nautilus_network::mode::ConnectionMode;
use pyo3::{IntoPyObjectExt, prelude::*, types::PyDict};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        consts::DYDX_VENUE,
        credential::DydxCredential,
        enums::{DydxCandleResolution, DydxMarketStatus},
        parse::{extract_raw_symbol, parse_price},
    },
    execution::types::OrderContext,
    http::{client::DydxHttpClient, parse::parse_account_state},
    python::encoder::PyDydxClientOrderIdEncoder,
    websocket::{
        DydxWsDispatchState, OrderIdentity,
        client::DydxWebSocketClient,
        enums::DydxWsOutputMessage,
        fill_report_to_order_filled, parse as ws_parse,
        parse::{parse_ws_fill_report, parse_ws_order_report, parse_ws_position_report},
    },
};

#[pymethods]
#[pyo3_stub_gen::derive::gen_stub_pymethods]
impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    ///
    /// This creates a new independent instrument cache. To share a cache with
    /// the HTTP client, use `Self.new_public_with_cache` instead.
    #[staticmethod]
    #[pyo3(name = "new_public")]
    #[pyo3(signature = (url, heartbeat=None, proxy_url=None))]
    fn py_new_public(url: String, heartbeat: Option<u64>, proxy_url: Option<String>) -> Self {
        Self::new_public(url, heartbeat, proxy_url)
    }

    /// Creates a new private WebSocket client for account updates.
    ///
    /// This creates a new independent instrument cache. To share a cache with
    /// the HTTP client, use `Self.new_private_with_cache` instead.
    #[staticmethod]
    #[pyo3(name = "new_private")]
    #[pyo3(signature = (url, private_key, authenticator_ids, account_id, heartbeat=None, proxy_url=None))]
    fn py_new_private(
        url: String,
        private_key: &str,
        authenticator_ids: Vec<u64>,
        account_id: AccountId,
        heartbeat: Option<u64>,
        proxy_url: Option<String>,
    ) -> PyResult<Self> {
        let credential = DydxCredential::from_private_key(private_key, authenticator_ids)
            .map_err(to_pyvalue_err)?;
        Ok(Self::new_private(
            url, credential, account_id, heartbeat, proxy_url,
        ))
    }

    /// Returns `true` when the client is connected.
    #[pyo3(name = "is_connected")]
    fn py_is_connected(&self) -> bool {
        self.is_connected()
    }

    /// Sets the account ID for account message parsing.
    #[pyo3(name = "set_account_id")]
    fn py_set_account_id(&mut self, account_id: AccountId) {
        self.set_account_id(account_id);
    }

    /// Sets whether bar timestamps use the close time.
    #[pyo3(name = "set_bars_timestamp_on_close")]
    fn py_set_bars_timestamp_on_close(&self, value: bool) {
        self.set_bars_timestamp_on_close(value);
    }

    /// Shares the HTTP client's instrument cache with this WebSocket client.
    ///
    /// The HTTP client's cache includes CLOB pair ID and market ticker indices
    /// needed for parsing SubaccountsChannelData into typed execution events.
    /// Must be called before `connect()`.
    #[pyo3(name = "share_instrument_cache")]
    fn py_share_instrument_cache(&mut self, http_client: &DydxHttpClient) {
        self.set_instrument_cache(http_client.instrument_cache().clone());
    }

    #[pyo3(name = "register_order_identity")]
    fn py_register_order_identity(
        &self,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        order_side: OrderSide,
        order_type: OrderType,
    ) {
        self.ws_dispatch_state().order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
            },
        );
    }

    #[pyo3(name = "remove_order_identity")]
    fn py_remove_order_identity(&self, client_order_id: ClientOrderId) {
        self.ws_dispatch_state()
            .order_identities
            .remove(&client_order_id);
    }

    /// Returns the account ID if set.
    #[pyo3(name = "account_id")]
    fn py_account_id(&self) -> Option<AccountId> {
        self.account_id()
    }

    /// Returns a reference to the shared client order ID encoder.
    #[pyo3(name = "encoder")]
    fn py_encoder(&self) -> PyDydxClientOrderIdEncoder {
        PyDydxClientOrderIdEncoder::from_arc(self.encoder().clone())
    }

    /// Returns the URL of this WebSocket client.
    #[getter]
    fn py_url(&self) -> String {
        self.url().to_string()
    }

    /// Connects the websocket client in handler mode with automatic reconnection.
    ///
    /// Spawns a background handler task that owns the WebSocketClient and processes
    /// raw messages into venue-specific `DydxWsOutputMessage` values.
    #[pyo3(name = "connect")]
    #[pyo3(signature = (loop_, instruments, callback, trader_id=None))]
    #[expect(clippy::needless_pass_by_value)]
    fn py_connect<'py>(
        &mut self,
        py: Python<'py>,
        loop_: Py<PyAny>,
        instruments: Vec<Py<PyAny>>,
        callback: Py<PyAny>,
        trader_id: Option<TraderId>,
    ) -> PyResult<Bound<'py, PyAny>> {
        let call_soon = loop_.getattr(py, "call_soon_threadsafe")?;

        let mut instruments_any = Vec::new();

        for inst in instruments {
            let inst_any = pyobject_to_instrument_any(py, inst)?;
            instruments_any.push(inst_any);
        }

        self.cache_instruments(instruments_any);

        let mut client = self.clone();
        let bar_types = self.bar_types().clone();
        let dispatch_state = self.ws_dispatch_state().clone();
        let trader_id = trader_id.unwrap_or(TraderId::from("TRADER-000"));

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.connect().await.map_err(to_pyvalue_err)?;

            if let Some(mut rx) = client.take_receiver() {
                get_runtime().spawn(async move {
                    let _client = client; // Keep client alive in spawned task
                    let clock = get_atomic_clock_realtime();
                    let order_contexts: DashMap<u32, OrderContext> = DashMap::new();
                    let order_id_map: DashMap<String, (u32, u32)> = DashMap::new();
                    let bars_timestamp_on_close = _client.bars_timestamp_on_close();
                    let mut pending_bars: AHashMap<String, Bar> = AHashMap::new();
                    let mut seen_tickers: ahash::AHashSet<Ustr> = ahash::AHashSet::new();

                    while let Some(msg) = rx.recv().await {
                        let ts_init = clock.get_time_ns();

                        match msg {
                            DydxWsOutputMessage::Trades { id, contents } => {
                                let Some(instrument) = _client.instrument_cache().get_by_market(&id) else {
                                    log::warn!("No instrument cached for market {id}");
                                    continue;
                                };
                                let instrument_id = instrument.id();

                                match ws_parse::parse_trade_ticks(instrument_id, &instrument, &contents, ts_init) {
                                    Ok(items) => {
                                        Python::attach(|py| {
                                            for data in items {
                                                let py_obj = data_to_pycapsule(py, data);
                                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                            }
                                        });
                                    }
                                    Err(e) => log::error!("Failed to parse trade ticks for {id}: {e}"),
                                }
                            }
                            DydxWsOutputMessage::OrderbookSnapshot { id, contents } => {
                                let Some(instrument) = _client.instrument_cache().get_by_market(&id) else {
                                    log::warn!("No instrument cached for market {id}");
                                    continue;
                                };
                                let instrument_id = instrument.id();
                                let price_precision = instrument.price_precision();
                                let size_precision = instrument.size_precision();

                                match ws_parse::parse_orderbook_snapshot(
                                    &instrument_id,
                                    &contents,
                                    price_precision,
                                    size_precision,
                                    ts_init,
                                ) {
                                    Ok(deltas) => {
                                        Python::attach(|py| {
                                            let data = Data::Deltas(OrderBookDeltas_API::new(deltas));
                                            let py_obj = data_to_pycapsule(py, data);
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        });
                                    }
                                    Err(e) => log::error!("Failed to parse orderbook snapshot for {id}: {e}"),
                                }
                            }
                            DydxWsOutputMessage::OrderbookUpdate { id, contents } => {
                                let Some(instrument) = _client.instrument_cache().get_by_market(&id) else {
                                    log::warn!("No instrument cached for market {id}");
                                    continue;
                                };
                                let instrument_id = instrument.id();
                                let price_precision = instrument.price_precision();
                                let size_precision = instrument.size_precision();

                                match ws_parse::parse_orderbook_deltas(
                                    &instrument_id,
                                    &contents,
                                    price_precision,
                                    size_precision,
                                    ts_init,
                                ) {
                                    Ok(deltas) => {
                                        Python::attach(|py| {
                                            let data = Data::Deltas(OrderBookDeltas_API::new(deltas));
                                            let py_obj = data_to_pycapsule(py, data);
                                            call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                        });
                                    }
                                    Err(e) => log::error!("Failed to parse orderbook deltas for {id}: {e}"),
                                }
                            }
                            DydxWsOutputMessage::OrderbookBatch { id, updates } => {
                                let Some(instrument) = _client.instrument_cache().get_by_market(&id) else {
                                    log::warn!("No instrument cached for market {id}");
                                    continue;
                                };
                                let instrument_id = instrument.id();
                                let price_precision = instrument.price_precision();
                                let size_precision = instrument.size_precision();

                                let mut all_deltas = Vec::new();
                                let last_idx = updates.len().saturating_sub(1);
                                let mut parse_ok = true;

                                for (idx, update) in updates.iter().enumerate() {
                                    if idx < last_idx {
                                        match ws_parse::parse_orderbook_deltas_with_flag(
                                            &instrument_id,
                                            update,
                                            price_precision,
                                            size_precision,
                                            ts_init,
                                            false,
                                        ) {
                                            Ok(deltas) => all_deltas.extend(deltas),
                                            Err(e) => {
                                                log::error!("Failed to parse batch orderbook deltas for {id}: {e}");
                                                parse_ok = false;
                                                break;
                                            }
                                        }
                                    } else {
                                        match ws_parse::parse_orderbook_deltas(
                                            &instrument_id,
                                            update,
                                            price_precision,
                                            size_precision,
                                            ts_init,
                                        ) {
                                            Ok(last_deltas) => all_deltas.extend(last_deltas.deltas),
                                            Err(e) => {
                                                log::error!("Failed to parse batch orderbook deltas for {id}: {e}");
                                                parse_ok = false;
                                                break;
                                            }
                                        }
                                    }
                                }

                                if parse_ok && !all_deltas.is_empty() {
                                    let combined = OrderBookDeltas::new(instrument_id, all_deltas);
                                    Python::attach(|py| {
                                        let data = Data::Deltas(OrderBookDeltas_API::new(combined));
                                        let py_obj = data_to_pycapsule(py, data);
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    });
                                }
                            }
                            DydxWsOutputMessage::Candles { id, contents } => {
                                let ticker = id.split('/').next().unwrap_or(&id);

                                let Some(bar_type) = bar_types.get(&id).map(|r| *r) else {
                                    log::debug!("No bar type registered for candle topic {id}");
                                    continue;
                                };

                                let Some(instrument) = _client.instrument_cache().get_by_market(ticker) else {
                                    log::warn!("No instrument cached for market {ticker}");
                                    continue;
                                };

                                match ws_parse::parse_candle_bar(
                                    bar_type,
                                    &instrument,
                                    &contents,
                                    bars_timestamp_on_close,
                                    ts_init,
                                ) {
                                    Ok(bar) => {
                                        if let Some(prev_bar) = pending_bars.get(&id) {
                                            if bar.ts_event == prev_bar.ts_event {
                                                pending_bars.insert(id, bar);
                                            } else {
                                                let emit_bar = *prev_bar;
                                                pending_bars.insert(id.clone(), bar);
                                                Python::attach(|py| {
                                                    let py_obj = data_to_pycapsule(py, Data::Bar(emit_bar));
                                                    call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                });
                                            }
                                        } else {
                                            pending_bars.insert(id, bar);
                                        }
                                    }
                                    Err(e) => log::error!("Failed to parse candle bar for {id}: {e}"),
                                }
                            }
                            DydxWsOutputMessage::Markets(contents) => {
                                if let Some(ref oracle_prices) = contents.oracle_prices {
                                    for (ticker, oracle_data) in oracle_prices {
                                        let Some(instrument) = _client.instrument_cache().get_by_market(ticker) else {
                                            continue;
                                        };
                                        let instrument_id = instrument.id();

                                        let Ok(price) = parse_price(&oracle_data.oracle_price, "oracle_price") else {
                                            log::warn!("Failed to parse oracle price for {ticker}");
                                            continue;
                                        };

                                        let mark_price = MarkPriceUpdate::new(
                                            instrument_id,
                                            price,
                                            ts_init,
                                            ts_init,
                                        );
                                        Python::attach(|py| {
                                            match mark_price.into_py_any(py) {
                                                Ok(py_obj) => {
                                                    call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                }
                                                Err(e) => log::error!("Failed to convert MarkPriceUpdate to Python: {e}"),
                                            }
                                        });

                                        let index_price = IndexPriceUpdate::new(
                                            instrument_id,
                                            price,
                                            ts_init,
                                            ts_init,
                                        );
                                        Python::attach(|py| {
                                            match index_price.into_py_any(py) {
                                                Ok(py_obj) => {
                                                    call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                }
                                                Err(e) => log::error!("Failed to convert IndexPriceUpdate to Python: {e}"),
                                            }
                                        });
                                    }
                                }

                                handle_markets_trading_data(
                                    contents.trading.as_ref(),
                                    _client.instrument_cache(),
                                    &mut seen_tickers,
                                    &call_soon,
                                    &callback,
                                    ts_init,
                                );
                                handle_markets_trading_data(
                                    contents.markets.as_ref(),
                                    _client.instrument_cache(),
                                    &mut seen_tickers,
                                    &call_soon,
                                    &callback,
                                    ts_init,
                                );

                                // Parse oracle prices from initial snapshot markets entries
                                if let Some(ref markets_map) = contents.markets {
                                    for (ticker, update) in markets_map {
                                        if let Some(ref oracle_price_str) = update.oracle_price {
                                            let Some(instrument) = _client.instrument_cache().get_by_market(ticker) else {
                                                continue;
                                            };
                                            let instrument_id = instrument.id();
                                            let Ok(price) = parse_price(oracle_price_str, "oracle_price") else {
                                                log::warn!("Failed to parse oracle price for {ticker}");
                                                continue;
                                            };

                                            let mark_price = MarkPriceUpdate::new(
                                                instrument_id,
                                                price,
                                                ts_init,
                                                ts_init,
                                            );
                                            Python::attach(|py| {
                                                match mark_price.into_py_any(py) {
                                                    Ok(py_obj) => {
                                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                    }
                                                    Err(e) => log::error!("Failed to convert MarkPriceUpdate to Python: {e}"),
                                                }
                                            });

                                            let index_price = IndexPriceUpdate::new(
                                                instrument_id,
                                                price,
                                                ts_init,
                                                ts_init,
                                            );
                                            Python::attach(|py| {
                                                match index_price.into_py_any(py) {
                                                    Ok(py_obj) => {
                                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                    }
                                                    Err(e) => log::error!("Failed to convert IndexPriceUpdate to Python: {e}"),
                                                }
                                            });
                                        }
                                    }
                                }
                            }
                            DydxWsOutputMessage::SubaccountSubscribed(data) => {
                                let Some(account_id) = _client.account_id() else {
                                    log::warn!("Cannot parse subaccount subscription: account_id not set");
                                    continue;
                                };

                                let instrument_cache = _client.instrument_cache();

                                let inst_map = instrument_cache.to_instrument_id_map();
                                let oracle_map = instrument_cache.to_oracle_prices_map();

                                if let Some(ref subaccount) = data.contents.subaccount {
                                    match parse_account_state(
                                        subaccount,
                                        account_id,
                                        &inst_map,
                                        &oracle_map,
                                        ts_init,
                                        ts_init,
                                    ) {
                                        Ok(account_state) => {
                                            Python::attach(|py| {
                                                match account_state.into_py_any(py) {
                                                    Ok(py_obj) => {
                                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                                    }
                                                    Err(e) => log::error!("Failed to convert AccountState to Python: {e}"),
                                                }
                                            });
                                        }
                                    Err(e) => log::error!("Failed to parse account state: {e}"),
                                }

                                if let Some(ref positions) = subaccount.open_perpetual_positions {
                                    for (market, ws_position) in positions {
                                        match parse_ws_position_report(
                                            ws_position,
                                            instrument_cache,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                Python::attach(|py| {
                                                    match pyo3::Py::new(py, report) {
                                                        Ok(py_obj) => {
                                                            call_python_threadsafe(py, &call_soon, &callback, py_obj.into_any());
                                                        }
                                                        Err(e) => log::error!("Failed to convert PositionStatusReport to Python: {e}"),
                                                    }
                                                });
                                            }
                                            Err(e) => log::error!("Failed to parse position for {market}: {e}"),
                                        }
                                    }
                                }
                                } else {
                                    log::warn!("Subaccount subscription without initial state (new/empty subaccount)");

                                    // Emit zero-balance account state so account gets registered
                                    let currency = Currency::get_or_create_crypto_with_context("USDC", None);
                                    let zero = Money::zero(currency);
                                    let balance = AccountBalance::new_checked(zero, zero, zero)
                                        .expect("zero balance should always be valid");
                                    let account_state = AccountState::new(
                                        account_id,
                                        AccountType::Margin,
                                        vec![balance],
                                        vec![],
                                        true,
                                        UUID4::new(),
                                        ts_init,
                                        ts_init,
                                        None,
                                    );
                                    Python::attach(|py| {
                                        match account_state.into_py_any(py) {
                                            Ok(py_obj) => {
                                                call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                            }
                                            Err(e) => log::error!("Failed to convert AccountState to Python: {e}"),
                                        }
                                    });
                                }
                            }
                            DydxWsOutputMessage::SubaccountsChannelData(data) => {
                                let Some(account_id) = _client.account_id() else {
                                    log::warn!("Cannot parse SubaccountsChannelData: account_id not set");
                                    continue;
                                };

                                let instrument_cache = _client.instrument_cache();
                                let encoder = _client.encoder();

                                let mut terminal_orders: Vec<(u32, u32, String)> = Vec::new();
                                let mut cum_fill_totals: AHashMap<VenueOrderId, (Decimal, Decimal)> = AHashMap::new();

                                // Phase 1: Parse orders and build order_id_map (needed for fill correlation)
                                let mut pending_order_reports = Vec::new();

                                if let Some(ref orders) = data.contents.orders {
                                    for ws_order in orders {
                                        if let Ok(client_id_u32) = ws_order.client_id.parse::<u32>() {
                                            let client_meta = ws_order.client_metadata
                                                .as_ref()
                                                .and_then(|s| s.parse::<u32>().ok())
                                                .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                            order_id_map.insert(ws_order.id.clone(), (client_id_u32, client_meta));
                                        }

                                        match parse_ws_order_report(
                                            ws_order,
                                            instrument_cache,
                                            &order_contexts,
                                            encoder,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                if !report.order_status.is_open()
                                                    && let Ok(cid) = ws_order.client_id.parse::<u32>()
                                                {
                                                    let meta = ws_order.client_metadata
                                                        .as_ref()
                                                        .and_then(|s| s.parse::<u32>().ok())
                                                        .unwrap_or(crate::grpc::DEFAULT_RUST_CLIENT_METADATA);
                                                    terminal_orders.push((cid, meta, ws_order.id.clone()));
                                                }
                                                pending_order_reports.push(report);
                                            }
                                            Err(e) => log::error!("Failed to parse WS order: {e}"),
                                        }
                                    }
                                }

                                // Phase 2: Process fills (tracked get OrderFilled, untracked get FillReport)
                                if let Some(ref fills) = data.contents.fills {
                                    for ws_fill in fills {
                                        match parse_ws_fill_report(
                                            ws_fill,
                                            instrument_cache,
                                            &order_id_map,
                                            &order_contexts,
                                            encoder,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                let identity = report.client_order_id.and_then(|cid| {
                                                    dispatch_state.order_identities.get(&cid).map(|r| (cid, r.clone()))
                                                });

                                                if let Some((cid, ident)) = identity {
                                                    ensure_accepted_to_python(
                                                        cid,
                                                        account_id,
                                                        report.venue_order_id,
                                                        &ident,
                                                        &dispatch_state,
                                                        trader_id,
                                                        ts_init,
                                                        &call_soon,
                                                        &callback,
                                                    );
                                                    dispatch_state.insert_filled(cid);
                                                    let quote_currency = instrument_cache
                                                        .get(&report.instrument_id)
                                                        .map_or_else(Currency::USD, |i: InstrumentAny| i.quote_currency());
                                                    let filled = fill_report_to_order_filled(
                                                        &report, trader_id, &ident, quote_currency,
                                                    );
                                                    send_to_python(filled, &call_soon, &callback);
                                                } else {
                                                    let entry = cum_fill_totals
                                                        .entry(report.venue_order_id)
                                                        .or_default();
                                                    let qty = report.last_qty.as_decimal();
                                                    entry.0 += report.last_px.as_decimal() * qty;
                                                    entry.1 += qty;
                                                    send_to_python(report, &call_soon, &callback);
                                                }
                                            }
                                            Err(e) => log::error!("Failed to parse WS fill: {e}"),
                                        }
                                    }
                                }

                                // Phase 3: Process order status updates
                                for report in &mut pending_order_reports {
                                    if let Some((notional, total_qty)) =
                                        cum_fill_totals.get(&report.venue_order_id)
                                        && !total_qty.is_zero()
                                    {
                                        report.avg_px = Some(notional / total_qty);
                                    }
                                }

                                for report in pending_order_reports {
                                    let identity = report.client_order_id.and_then(|cid| {
                                        dispatch_state.order_identities.get(&cid).map(|r| (cid, r.clone()))
                                    });

                                    if let Some((cid, ident)) = identity {
                                        match report.order_status {
                                            OrderStatus::Accepted => {
                                                if dispatch_state.emitted_accepted.contains(&cid)
                                                    || dispatch_state.filled_orders.contains(&cid)
                                                {
                                                    log::debug!("Skipping duplicate Accepted for {cid}");
                                                    continue;
                                                }
                                                dispatch_state.insert_accepted(cid);
                                                let accepted = OrderAccepted::new(
                                                    trader_id,
                                                    ident.strategy_id,
                                                    ident.instrument_id,
                                                    cid,
                                                    report.venue_order_id,
                                                    account_id,
                                                    UUID4::new(),
                                                    report.ts_last,
                                                    ts_init,
                                                    false,
                                                );
                                                send_to_python(accepted, &call_soon, &callback);
                                            }
                                            OrderStatus::Canceled => {
                                                ensure_accepted_to_python(
                                                    cid,
                                                    account_id,
                                                    report.venue_order_id,
                                                    &ident,
                                                    &dispatch_state,
                                                    trader_id,
                                                    ts_init,
                                                    &call_soon,
                                                    &callback,
                                                );
                                                let canceled = OrderCanceled::new(
                                                    trader_id,
                                                    ident.strategy_id,
                                                    ident.instrument_id,
                                                    cid,
                                                    UUID4::new(),
                                                    report.ts_last,
                                                    ts_init,
                                                    false,
                                                    Some(report.venue_order_id),
                                                    Some(account_id),
                                                );
                                                send_to_python(canceled, &call_soon, &callback);
                                                dispatch_state.cleanup_terminal(&cid);
                                            }
                                            OrderStatus::Filled => {
                                                dispatch_state.cleanup_terminal(&cid);
                                            }
                                            _ => {
                                                send_to_python(report, &call_soon, &callback);
                                            }
                                        }
                                    } else {
                                        send_to_python(report, &call_soon, &callback);
                                    }
                                }

                                // Deferred cleanup after fills are correlated
                                for (client_id, client_metadata, order_id) in terminal_orders {
                                    order_contexts.remove(&client_id);
                                    encoder.remove(client_id, client_metadata);
                                    order_id_map.remove(&order_id);
                                }
                            }
                            DydxWsOutputMessage::BlockHeight { height, time } => {
                                Python::attach(|py| {
                                    let dict = PyDict::new(py);
                                    let _ = dict.set_item("type", "block_height");
                                    let _ = dict.set_item("height", height);
                                    let _ = dict.set_item("time", time.to_rfc3339());
                                    if let Ok(py_obj) = dict.into_py_any(py) {
                                        call_python_threadsafe(py, &call_soon, &callback, py_obj);
                                    }
                                });
                            }
                            DydxWsOutputMessage::Error(err) => {
                                log::error!("dYdX WebSocket error: {err}");
                            }
                            DydxWsOutputMessage::Reconnected => {
                                log::info!("dYdX WebSocket reconnected");
                                pending_bars.clear();
                            }
                        }
                    }
                });
            }

            Ok(())
        })
    }

    /// Disconnects the websocket client gracefully.
    ///
    /// Sends a disconnect command to the handler, sets the stop signal, then
    /// awaits the handler task with a timeout before aborting.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client cannot be accessed.
    #[pyo3(name = "disconnect")]
    fn py_disconnect<'py>(&mut self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let mut client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.disconnect().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

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
    ///
    /// Any existing instrument with the same ID will be replaced.
    #[pyo3(name = "cache_instrument")]
    fn py_cache_instrument(&self, instrument: Py<PyAny>, py: Python<'_>) -> PyResult<()> {
        let inst_any = pyobject_to_instrument_any(py, instrument)?;
        self.cache_instrument(inst_any);
        Ok(())
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same IDs will be replaced.
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

    #[pyo3(name = "is_closed")]
    fn py_is_closed(&self) -> bool {
        !self.is_connected()
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#trades-channel>
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
                .map_err(to_pyvalue_err)?;
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
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#orderbook-channel>
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
                .map_err(to_pyvalue_err)?;
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
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "subscribe_bars")]
    fn py_subscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let spec = bar_type.spec();
        let resolution = DydxCandleResolution::from_bar_spec(&spec).map_err(to_pyvalue_err)?;
        let resolution = resolution.to_string();

        let client = self.clone();
        let instrument_id = bar_type.instrument_id();
        let bar_types = self.bar_types().clone();

        // Build topic for bar type registration (e.g., "ETH-USD/1MIN")
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            bar_types.insert(topic, bar_type);

            client
                .subscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    #[pyo3(name = "unsubscribe_bars")]
    fn py_unsubscribe_bars<'py>(
        &self,
        py: Python<'py>,
        bar_type: BarType,
    ) -> PyResult<Bound<'py, PyAny>> {
        let spec = bar_type.spec();
        let resolution = DydxCandleResolution::from_bar_spec(&spec).map_err(to_pyvalue_err)?;
        let resolution = resolution.to_string();

        let client = self.clone();
        let instrument_id = bar_type.instrument_id();
        let bar_types = self.bar_types().clone();

        // Build topic for unregistration
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");

        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_candles(instrument_id, &resolution)
                .await
                .map_err(to_pyvalue_err)?;

            bar_types.remove(&topic);

            Ok(())
        })
    }

    /// Subscribes to market updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#markets-channel>
    #[pyo3(name = "subscribe_markets")]
    fn py_subscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.subscribe_markets().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from market updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    #[pyo3(name = "unsubscribe_markets")]
    fn py_unsubscribe_markets<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client.unsubscribe_markets().await.map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Subscribes to subaccount updates (orders, fills, positions, balances).
    ///
    /// This requires authentication and will only work for private WebSocket clients
    /// created with `Self.new_private`.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#subaccounts-channel>
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
                .map_err(to_pyvalue_err)?;
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
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Subscribes to block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#block-height-channel>
    #[pyo3(name = "subscribe_block_height")]
    fn py_subscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .subscribe_block_height()
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }

    /// Unsubscribes from block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    #[pyo3(name = "unsubscribe_block_height")]
    fn py_unsubscribe_block_height<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyAny>> {
        let client = self.clone();
        pyo3_async_runtimes::tokio::future_into_py(py, async move {
            client
                .unsubscribe_block_height()
                .await
                .map_err(to_pyvalue_err)?;
            Ok(())
        })
    }
}

fn instrument_id_from_ticker(ticker: &str) -> InstrumentId {
    let symbol = format!("{ticker}-PERP");
    InstrumentId::new(Symbol::new(&symbol), *DYDX_VENUE)
}

fn handle_markets_trading_data(
    trading: Option<
        &std::collections::HashMap<String, crate::websocket::messages::DydxMarketTradingUpdate>,
    >,
    instrument_cache: &std::sync::Arc<crate::common::instrument_cache::InstrumentCache>,
    seen_tickers: &mut ahash::AHashSet<Ustr>,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
    ts_init: nautilus_core::UnixNanos,
) {
    let Some(trading_map) = trading else {
        return;
    };

    for (ticker, update) in trading_map {
        let instrument_id = instrument_id_from_ticker(ticker);

        if let Some(status) = &update.status {
            let action = MarketStatusAction::from(*status);
            let is_trading = matches!(status, DydxMarketStatus::Active);

            let instrument_status = InstrumentStatus::new(
                instrument_id,
                action,
                ts_init,
                ts_init,
                None,
                None,
                Some(is_trading),
                None,
                None,
            );

            if instrument_cache.get_by_market(ticker).is_some() {
                Python::attach(|py| match instrument_status.into_py_any(py) {
                    Ok(py_obj) => {
                        call_python_threadsafe(py, call_soon, callback, py_obj);
                    }
                    Err(e) => log::error!("Failed to convert InstrumentStatus to Python: {e}"),
                });
            }
        }

        let ticker_ustr = Ustr::from(ticker.as_str());
        if !seen_tickers.contains(&ticker_ustr) {
            let is_active = update
                .status
                .as_ref()
                .is_none_or(|s| matches!(s, crate::common::enums::DydxMarketStatus::Active));
            if instrument_cache.get_by_market(ticker).is_some() {
                seen_tickers.insert(ticker_ustr);
            } else if is_active {
                seen_tickers.insert(ticker_ustr);
                log::info!("New instrument discovered via WebSocket: {ticker}");
                Python::attach(|py| {
                    let dict = PyDict::new(py);
                    let _ = dict.set_item("type", "new_instrument_discovered");
                    let _ = dict.set_item("ticker", ticker);
                    if let Ok(py_obj) = dict.into_py_any(py) {
                        call_python_threadsafe(py, call_soon, callback, py_obj);
                    }
                });
            }
        }

        if let Some(ref rate_str) = update.next_funding_rate {
            if let Ok(rate) = Decimal::from_str(rate_str) {
                let funding_rate = FundingRateUpdate {
                    instrument_id,
                    rate,
                    interval: Some(60),
                    next_funding_ns: None,
                    ts_event: ts_init,
                    ts_init,
                };
                Python::attach(|py| match funding_rate.into_py_any(py) {
                    Ok(py_obj) => {
                        call_python_threadsafe(py, call_soon, callback, py_obj);
                    }
                    Err(e) => log::error!("Failed to convert FundingRateUpdate to Python: {e}"),
                });
            } else {
                log::warn!("Failed to parse next_funding_rate for {ticker}: {rate_str}");
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn ensure_accepted_to_python(
    client_order_id: ClientOrderId,
    account_id: AccountId,
    venue_order_id: VenueOrderId,
    identity: &OrderIdentity,
    state: &DydxWsDispatchState,
    trader_id: TraderId,
    ts_init: nautilus_core::UnixNanos,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    if state.emitted_accepted.contains(&client_order_id) {
        return;
    }
    state.insert_accepted(client_order_id);
    let accepted = OrderAccepted::new(
        trader_id,
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_init,
        ts_init,
        false,
    );
    send_to_python(accepted, call_soon, callback);
}

fn send_to_python<T: for<'py> IntoPyObjectExt<'py>>(
    value: T,
    call_soon: &Py<PyAny>,
    callback: &Py<PyAny>,
) {
    Python::attach(|py| match value.into_py_any(py) {
        Ok(py_obj) => {
            call_python_threadsafe(py, call_soon, callback, py_obj);
        }
        Err(e) => log::error!("Failed to convert to Python: {e}"),
    });
}
