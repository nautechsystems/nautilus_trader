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

//! Kraken Spot data client implementation.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{get_data_event_sender, get_runtime},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, InstrumentResponse, InstrumentsResponse,
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeIndexPrices, UnsubscribeInstrumentStatus, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, Data, OrderBookDeltas, OrderBookDeltas_API},
    enums::{AggregationSource, BookType},
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

type OhlcBufferKey = (Ustr, u32);
type OhlcBuffer = Arc<Mutex<AHashMap<OhlcBufferKey, (Bar, UnixNanos)>>>;

use crate::{
    common::consts::KRAKEN_VENUE,
    config::KrakenDataClientConfig,
    http::{KrakenSpotHttpClient, spot::client::KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND},
    websocket::spot_v2::{
        client::KrakenSpotWebSocketClient,
        level_3::{
            BookOrderIdHasher, KrakenL3WsMessage,
            resync::retry_l3_resync,
            runtime::{L3Sink, L3State, process_l3_message},
        },
        messages::KrakenSpotWsMessage,
        parse::{parse_book_deltas, parse_quote_tick, parse_trade_tick, parse_ws_bar},
    },
};

/// `L3Sink` implementation that forwards deltas to the data engine.
struct DataEventSink<'a> {
    sender: &'a tokio::sync::mpsc::UnboundedSender<DataEvent>,
}

impl L3Sink for DataEventSink<'_> {
    fn emit_deltas(&mut self, deltas: OrderBookDeltas_API) {
        if let Err(e) = self.sender.send(DataEvent::Data(Data::Deltas(deltas))) {
            log::error!("Failed to send L3 deltas: {e}");
        }
    }
}

/// Kraken Spot data client.
///
/// Provides real-time market data from Kraken Spot markets through WebSocket v2.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenSpotDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: KrakenDataClientConfig,
    http: KrakenSpotHttpClient,
    ws: KrakenSpotWebSocketClient,
    ws_l3: Option<KrakenSpotWebSocketClient>,
    l3_handler_alive: Arc<AtomicBool>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
}

impl KrakenSpotDataClient {
    /// Creates a new [`KrakenSpotDataClient`] instance.
    pub fn new(client_id: ClientId, config: KrakenDataClientConfig) -> anyhow::Result<Self> {
        let cancellation_token = CancellationToken::new();

        let http = KrakenSpotHttpClient::new(
            config.environment,
            config.base_url.clone(),
            config.timeout_secs,
            None,
            None,
            None,
            config.proxy_url.clone(),
            config
                .max_requests_per_second
                .unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND),
        )?;

        let ws = KrakenSpotWebSocketClient::new(
            config.clone(),
            cancellation_token.clone(),
            config.proxy_url.clone(),
        );

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            config,
            http,
            ws,
            ws_l3: None,
            l3_handler_alive: Arc::new(AtomicBool::new(false)),
            is_connected: AtomicBool::new(false),
            cancellation_token,
            tasks: Vec::new(),
            instruments: Arc::new(AtomicMap::new()),
            data_sender: get_data_event_sender(),
        })
    }

    /// Returns the cached instruments.
    #[must_use]
    pub fn instruments(&self) -> Vec<InstrumentAny> {
        self.instruments.load().values().cloned().collect()
    }

    /// Returns a cached instrument by ID.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instruments.load().get(instrument_id).cloned()
    }

    async fn load_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http
            .request_instruments(None)
            .await
            .context("Failed to load spot instruments")?;

        self.instruments.rcu(|m| {
            for instrument in &instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        self.http.cache_instruments(&instruments);

        log::info!(
            "Loaded instruments: client_id={}, count={}",
            self.client_id,
            instruments.len()
        );

        Ok(instruments)
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    fn subscribe_l3_book(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let symbol_ustr = instrument_id.symbol.inner();
        let depth = cmd.depth.map_or(1000, |d| d.get() as u32);

        if !matches!(depth, 10 | 100 | 1000) {
            anyhow::bail!("Invalid L3 depth {depth} for Kraken Spot, valid values: 10, 100, 1000");
        }

        if !self.config.has_api_credentials() {
            anyhow::bail!(
                "L3 order book requires API credentials; configure api_key and api_secret"
            );
        }

        let handler_dead = !self.l3_handler_alive.load(Ordering::Relaxed);
        if self.ws_l3.is_none() || handler_dead {
            if let Some(dead) = self.ws_l3.take() {
                get_runtime().spawn(async move {
                    let mut dead = dead;
                    let _ = dead.close().await;
                });
            }

            let ws_l3 = KrakenSpotWebSocketClient::l3(
                self.config.clone(),
                self.cancellation_token.clone(),
                self.config.proxy_url.clone(),
            );

            self.spawn_l3_handler_task(ws_l3.clone());
            self.ws_l3 = Some(ws_l3);
        }

        let ws_l3 = self
            .ws_l3
            .as_ref()
            .expect("ws_l3 initialised above")
            .clone();

        self.spawn_ws(
            async move {
                ws_l3
                    .wait_until_active(10.0)
                    .await
                    .map_err(|e| anyhow::anyhow!("L3 WebSocket failed to become active: {e}"))?;
                ws_l3
                    .wait_until_authenticated(10.0)
                    .await
                    .map_err(|e| anyhow::anyhow!("L3 WebSocket failed to authenticate: {e}"))?;
                ws_l3
                    .subscribe_book_l3(symbol_ustr, depth)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe l3 book",
        );

        log::info!("Subscribed to L3 book: instrument_id={instrument_id}");
        Ok(())
    }

    fn spawn_l3_handler_task(&mut self, handler_client: KrakenSpotWebSocketClient) {
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;
        let alive = self.l3_handler_alive.clone();

        alive.store(true, Ordering::Relaxed);

        let handle = get_runtime().spawn(async move {
            struct AliveGuard(Arc<AtomicBool>);
            impl Drop for AliveGuard {
                fn drop(&mut self) {
                    self.0.store(false, Ordering::Relaxed);
                }
            }
            let _alive_guard = AliveGuard(alive);

            let mut handler_client = handler_client;

            if let Err(e) = handler_client.connect().await {
                log::error!("L3 WebSocket connect failed: {e}");
                return;
            }

            if let Err(e) = handler_client.wait_until_active(10.0).await {
                log::error!("L3 WebSocket failed to become active: {e}");
                return;
            }

            if let Err(e) = handler_client.authenticate().await {
                log::error!("L3 WebSocket authentication failed: {e}");
                return;
            }

            let stream = match handler_client.stream() {
                Ok(s) => s,
                Err(e) => {
                    log::error!("L3 stream() failed: {e}");
                    return;
                }
            };
            tokio::pin!(stream);

            let mut states: AHashMap<String, L3State> = AHashMap::new();
            let hasher = BookOrderIdHasher::new();
            let l3_depths = handler_client.l3_depths_handle();
            let validate_checksum = handler_client.validate_l3_checksum();
            let resync_client = handler_client.clone();

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => break,
                    msg = stream.next() => {
                        let Some(msg) = msg else { break };
                        let ts_init = clock.get_time_ns();

                        let runtime_msg = match msg {
                            KrakenSpotWsMessage::L3Snapshot(snap) => {
                                KrakenL3WsMessage::Snapshot(snap)
                            }
                            KrakenSpotWsMessage::L3Update(update) => {
                                KrakenL3WsMessage::Update(update)
                            }
                            KrakenSpotWsMessage::Reconnected => {
                                log::info!("L3 WebSocket reconnected");

                                for state in states.values_mut() {
                                    state.open_orders.clear();
                                    state.awaiting_snapshot = true;
                                }
                                continue;
                            }
                            _ => continue,
                        };

                        let mut sink = DataEventSink { sender: &data_sender };
                        let resync = process_l3_message(
                            runtime_msg,
                            &mut sink,
                            &instruments,
                            &l3_depths,
                            &mut states,
                            &hasher,
                            validate_checksum,
                            ts_init,
                        );

                        if let Some(request) = resync {
                            log::warn!(
                                "Resyncing Kraken L3 book: symbol={}, depth={}, reason={}",
                                request.symbol,
                                request.depth,
                                request.reason,
                            );
                            let symbol_ustr = Ustr::from(&request.symbol);
                            let client_for_resync = resync_client.clone();

                            get_runtime().spawn(async move {
                                retry_l3_resync(
                                    &client_for_resync,
                                    symbol_ustr,
                                    request.depth,
                                )
                                .await;
                            });
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let stream = self.ws.stream().map_err(|e| anyhow::anyhow!("{e}"))?;
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let book_sequence = Arc::new(AtomicU64::new(0));
        let ohlc_buffer: OhlcBuffer = Arc::new(Mutex::new(AHashMap::new()));
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Spot message handler cancelled");
                        Self::flush_ohlc_buffer(&ohlc_buffer, &data_sender);
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(
                                    ws_msg,
                                    &data_sender,
                                    &instruments,
                                    &book_sequence,
                                    &ohlc_buffer,
                                    clock,
                                );
                            }
                            None => {
                                log::debug!("Spot WebSocket stream ended");
                                Self::flush_ohlc_buffer(&ohlc_buffer, &data_sender);
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
        Ok(())
    }

    fn lookup_instrument(
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        symbol: &str,
    ) -> Option<InstrumentAny> {
        let instrument_id = InstrumentId::new(Symbol::new(symbol), *KRAKEN_VENUE);
        instruments.load().get(&instrument_id).cloned()
    }

    fn flush_ohlc_buffer(
        ohlc_buffer: &OhlcBuffer,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) {
        let Ok(mut buffer) = ohlc_buffer.lock() else {
            return;
        };
        let bars: Vec<Bar> = buffer.drain().map(|(_, (bar, _))| bar).collect();
        for bar in bars {
            if let Err(e) = sender.send(DataEvent::Data(Data::Bar(bar))) {
                log::error!("Failed to send buffered bar: {e}");
            }
        }
    }

    fn handle_ws_message(
        msg: KrakenSpotWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        book_sequence: &Arc<AtomicU64>,
        ohlc_buffer: &OhlcBuffer,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();

        match msg {
            KrakenSpotWsMessage::Ticker(tickers) => {
                for ticker in &tickers {
                    let Some(instrument) =
                        Self::lookup_instrument(instruments, ticker.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", ticker.symbol);
                        continue;
                    };

                    match parse_quote_tick(ticker, &instrument, ts_init) {
                        Ok(quote) => {
                            if let Err(e) = sender.send(DataEvent::Data(Data::Quote(quote))) {
                                log::error!("Failed to send quote: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse quote tick: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Trade(trades) => {
                for trade in &trades {
                    let Some(instrument) =
                        Self::lookup_instrument(instruments, trade.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", trade.symbol);
                        continue;
                    };

                    match parse_trade_tick(trade, &instrument, ts_init) {
                        Ok(tick) => {
                            if let Err(e) = sender.send(DataEvent::Data(Data::Trade(tick))) {
                                log::error!("Failed to send trade: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse trade tick: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Book {
                data,
                is_snapshot: _,
            } => {
                for book in &data {
                    let Some(instrument) =
                        Self::lookup_instrument(instruments, book.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", book.symbol);
                        continue;
                    };
                    let sequence = book_sequence.load(Ordering::Relaxed);
                    match parse_book_deltas(book, &instrument, sequence, ts_init) {
                        Ok(delta_vec) => {
                            if delta_vec.is_empty() {
                                continue;
                            }
                            book_sequence.fetch_add(delta_vec.len() as u64, Ordering::Relaxed);
                            let deltas = OrderBookDeltas::new(instrument.id(), delta_vec);
                            let api_deltas = OrderBookDeltas_API::new(deltas);
                            if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(api_deltas))) {
                                log::error!("Failed to send deltas: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse book deltas: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Ohlc(ohlc_data) => {
                let Ok(mut buffer) = ohlc_buffer.lock() else {
                    log::error!("OHLC buffer lock poisoned");
                    return;
                };

                for ohlc in &ohlc_data {
                    let Some(instrument) =
                        Self::lookup_instrument(instruments, ohlc.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", ohlc.symbol);
                        continue;
                    };

                    match parse_ws_bar(ohlc, &instrument, ts_init) {
                        Ok(new_bar) => {
                            let key: (Ustr, u32) = (ohlc.symbol, ohlc.interval);
                            let new_interval_begin = UnixNanos::from(
                                ohlc.interval_begin.timestamp_nanos_opt().unwrap_or(0) as u64,
                            );

                            if let Some((buffered_bar, buffered_begin)) = buffer.get(&key)
                                && new_interval_begin != *buffered_begin
                                && let Err(e) =
                                    sender.send(DataEvent::Data(Data::Bar(*buffered_bar)))
                            {
                                log::error!("Failed to send bar: {e}");
                            }

                            buffer.insert(key, (new_bar, new_interval_begin));
                        }
                        Err(e) => log::error!("Failed to parse bar: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Execution(_) => {}
            KrakenSpotWsMessage::OrderResponse(_) => {}
            KrakenSpotWsMessage::L3Snapshot(_) => {}
            KrakenSpotWsMessage::L3Update(_) => {}
            KrakenSpotWsMessage::Reconnected => {
                log::info!("Spot WebSocket reconnected");
            }
        }
    }
}

#[async_trait(?Send)]
impl DataClient for KrakenSpotDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*KRAKEN_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Spot data client: client_id={}, environment={:?}",
            self.client_id,
            self.config.environment
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Spot data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Spot data client: {}", self.client_id);
        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            task.abort();
        }

        let mut ws = self.ws.clone();
        get_runtime().spawn(async move {
            let _ = ws.close().await;
        });

        if let Some(mut ws_l3) = self.ws_l3.take() {
            get_runtime().spawn(async move {
                let _ = ws_l3.close().await;
            });
        }

        self.instruments.store(ahash::AHashMap::new());

        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing Spot data client: {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        let instruments = self.load_instruments().await?;

        self.ws
            .connect()
            .await
            .context("Failed to connect spot WebSocket")?;
        self.ws
            .wait_until_active(10.0)
            .await
            .context("Spot WebSocket failed to become active")?;

        self.spawn_message_handler()?;

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::error!("Failed to send instrument: {e}");
            }
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}, product_type=Spot", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();
        let _ = self.ws.close().await;

        if let Some(mut ws_l3) = self.ws_l3.take() {
            let _ = ws_l3.close().await;
        }

        for handle in self.tasks.drain(..) {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e:?}");
            }
        }

        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Relaxed);

        log::info!("Disconnected: client_id={}", self.client_id);
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("subscribe_instruments: Kraken instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("subscribe_instrument: Kraken instruments are fetched via HTTP on connect");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let depth = cmd.depth;

        match cmd.book_type {
            BookType::L2_MBP => {}
            BookType::L3_MBO => return self.subscribe_l3_book(&cmd),
            other => {
                log::warn!("Unsupported BookType {other:?} for Kraken Spot, skipping");
                return Ok(());
            }
        }

        if let Some(d) = depth {
            let d_val = d.get();
            if !matches!(d_val, 10 | 25 | 100 | 500 | 1000) {
                log::warn!("Invalid depth {d_val} for Kraken Spot, valid: 10, 25, 100, 500, 1000");
                return Ok(());
            }
        }

        let ws = self.ws.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_book(instrument_id, depth.map(|d| d.get() as u32))
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe book",
        );

        log::info!("Subscribed to book: instrument_id={instrument_id}, depth={depth:?}");
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.subscribe_quotes(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe quotes",
        );

        log::info!("Subscribed to quotes: instrument_id={instrument_id}");
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe trades",
        );

        log::info!("Subscribed to trades: instrument_id={instrument_id}");
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        log::warn!(
            "Mark price subscription not supported for Spot instrument {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        log::warn!(
            "Index price subscription not supported for Spot instrument {}",
            cmd.instrument_id
        );
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;

        if bar_type.aggregation_source() != AggregationSource::External {
            log::warn!("Cannot subscribe to {bar_type} bars: only EXTERNAL bars supported");
            return Ok(());
        }

        if !bar_type.spec().is_time_aggregated() {
            log::warn!("Cannot subscribe to {bar_type} bars: only time-based bars supported");
            return Ok(());
        }

        let ws = self.ws.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_bars(bar_type)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "subscribe bars",
        );

        log::info!("Subscribed to bars: bar_type={bar_type}");
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::info!(
            "subscribe_instrument_status: {} (status changes detected via periodic instrument polling)",
            cmd.instrument_id,
        );
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if self.ws_l3.as_ref().is_some_and(|ws| {
            ws.subscriptions_contains(&format!("level3:{}", instrument_id.symbol))
        }) {
            let symbol_ustr = instrument_id.symbol.inner();

            if let Some(ws_l3) = self.ws_l3.clone() {
                self.spawn_ws(
                    async move {
                        ws_l3
                            .unsubscribe_book_l3(symbol_ustr)
                            .await
                            .map_err(|e| anyhow::anyhow!("{e}"))?;
                        log::info!("Unsubscribed from L3 book: instrument_id={instrument_id}");
                        Ok(())
                    },
                    "unsubscribe l3 book",
                );
            }
            return Ok(());
        }

        let ws = self.ws.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_book(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe book",
        );

        log::info!("Unsubscribed from book: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_quotes(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe quotes",
        );

        log::info!("Unsubscribed from quotes: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe trades",
        );

        log::info!("Unsubscribed from trades: instrument_id={instrument_id}");
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, _cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let ws = self.ws.clone();

        self.spawn_ws(
            async move {
                ws.unsubscribe_bars(bar_type)
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))
            },
            "unsubscribe bars",
        );

        log::info!("Unsubscribed from bars: bar_type={bar_type}");
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        _cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instruments_cache = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *KRAKEN_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(None).await {
                Ok(instruments) => {
                    instruments_cache.rcu(|m| {
                        for instrument in &instruments {
                            m.insert(instrument.id(), instrument.clone());
                        }
                    });
                    http.cache_instruments(&instruments);

                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        instruments,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instruments response: {e}");
                    }
                }
                Err(e) => log::error!("Instruments request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            if let Some(instrument) = instruments.load().get(&instrument_id) {
                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request_id,
                    client_id,
                    instrument.id(),
                    instrument.clone(),
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    params,
                )));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send instrument response: {e}");
                }
                return;
            }

            match http.request_instruments(None).await {
                Ok(all_instruments) => {
                    instruments.rcu(|m| {
                        for instrument in &all_instruments {
                            m.insert(instrument.id(), instrument.clone());
                        }
                    });
                    http.cache_instruments(&all_instruments);

                    let instrument = all_instruments
                        .into_iter()
                        .find(|i| i.id() == instrument_id);

                    if let Some(instrument) = instrument {
                        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                            request_id,
                            client_id,
                            instrument.id(),
                            instrument,
                            start_nanos,
                            end_nanos,
                            clock.get_time_ns(),
                            params,
                        )));

                        if let Err(e) = sender.send(DataEvent::Response(response)) {
                            log::error!("Failed to send instrument response: {e}");
                        }
                    } else {
                        log::error!("Instrument not found: {instrument_id}");
                    }
                }
                Err(e) => log::error!("Instrument request failed: {e:?}"),
            }
        });

        Ok(())
    }
    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u64);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_trades(instrument_id, start, end, limit).await {
                Ok(trades) => {
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        trades,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => log::error!("Trades request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u64);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_bars(bar_type, start, end, limit).await {
                Ok(bars) => {
                    let response = DataResponse::Bars(BarsResponse::new(
                        request_id,
                        client_id,
                        bar_type,
                        bars,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => log::error!("Bars request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let depth = request.depth.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_book_snapshot(instrument_id, depth).await {
                Ok(book) => {
                    let response = DataResponse::Book(BookResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        book,
                        None,
                        None,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send book snapshot response: {e}");
                    }
                }
                Err(e) => log::error!("Book snapshot request failed: {e:?}"),
            }
        });

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::{live::runner::set_data_event_sender, messages::DataEvent};
    use nautilus_model::{
        enums::BookAction,
        instruments::{InstrumentAny, currency_pair::CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::consts::KRAKEN_CLIENT_ID, config::KrakenDataClientConfig,
        websocket::spot_v2::level_3::messages::KrakenL3Snapshot,
    };

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    fn make_instrument() -> InstrumentAny {
        InstrumentAny::CurrencyPair(CurrencyPair::new(
            InstrumentId::from("BTC/USD.KRAKEN"),
            Symbol::from("BTC/USD"),
            Currency::BTC(),
            Currency::USD(),
            1,
            8,
            Price::from("0.1"),
            Quantity::from("0.00000001"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    #[rstest]
    fn test_spot_data_client_new() {
        setup_test_env();
        let config = KrakenDataClientConfig::default();
        let client = KrakenSpotDataClient::new(*KRAKEN_CLIENT_ID, config);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.client_id(), *KRAKEN_CLIENT_ID);
        assert_eq!(client.venue(), Some(*KRAKEN_VENUE));
        assert!(!client.is_connected());
        assert!(client.is_disconnected());
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_l3_snapshot_checksum_mismatch_emits_clear_and_requests_resync() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let instruments = Arc::new(AtomicMap::new());
        let instrument = make_instrument();
        instruments.insert(instrument.id(), instrument);

        let depths = Arc::new(Mutex::new(AHashMap::new()));
        depths
            .lock()
            .expect("depths lock poisoned")
            .insert("BTC/USD".to_string(), 1000);

        let snapshot: KrakenL3Snapshot = serde_json::from_str(
            r#"{
                "symbol": "BTC/USD",
                "bids": [{
                    "order_id": "order-bid-1",
                    "limit_price": 4199.0,
                    "order_qty": 3.00000000,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "asks": [{
                    "order_id": "order-ask-1",
                    "limit_price": 4200.0,
                    "order_qty": 0.01000000,
                    "timestamp": "2024-01-01T00:00:00Z"
                }],
                "checksum": 1,
                "timestamp": "2024-01-01T00:00:00Z"
            }"#,
        )
        .unwrap();

        let mut states = AHashMap::new();
        let hasher = BookOrderIdHasher::new();
        let mut sink = DataEventSink { sender: &sender };
        let request = process_l3_message(
            KrakenL3WsMessage::Snapshot(snapshot),
            &mut sink,
            &instruments,
            &depths,
            &mut states,
            &hasher,
            true,
            get_atomic_clock_realtime().get_time_ns(),
        )
        .expect("expected resync request");

        assert_eq!(request.symbol, "BTC/USD");
        assert_eq!(request.depth, 1000);
        assert_eq!(request.reason, "snapshot checksum mismatch");

        let event = receiver.try_recv().expect("expected clear event");
        let DataEvent::Data(Data::Deltas(deltas)) = event else {
            panic!("expected deltas event");
        };

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(states["BTC/USD"].awaiting_snapshot);
        assert!(states["BTC/USD"].open_orders.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_spot_data_client_start_stop() {
        setup_test_env();
        let config = KrakenDataClientConfig::default();
        let mut client = KrakenSpotDataClient::new(*KRAKEN_CLIENT_ID, config).unwrap();

        assert!(client.start().is_ok());
        assert!(client.stop().is_ok());
        assert!(client.is_disconnected());
    }
}
