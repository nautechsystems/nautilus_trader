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
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{get_data_event_sender, sender::EventSender},
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
use nautilus_live::{
    SocketControlFactory,
    task::{TaskGroup, TaskRef, TaskSpawner},
};
use nautilus_model::{
    data::{Bar, Data, OrderBookDeltas},
    enums::{AggregationSource, BookType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{consts::KRAKEN_VENUE, lookup_instrument_in_snapshot},
    config::KrakenDataClientConfig,
    http::{KrakenSpotHttpClient, spot::client::KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND},
    websocket::spot_v2::{
        client::KrakenSpotWebSocketClient,
        level_2::{
            L2BookRequest, L2BookRequests, L2BookState, L2Depths, L2ResyncRequest, L2Subscription,
            clear_deltas,
        },
        level_3::{
            BookOrderIdHasher, KrakenL3WsMessage,
            resync::retry_l3_resync,
            runtime::{L3Sink, L3State, process_l3_message},
        },
        messages::KrakenSpotWsMessage,
        parse::{parse_quote_tick, parse_trade_tick, parse_ws_bar},
    },
};

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
    socket_factory: SocketControlFactory,
    l3_handler_task: Option<TaskRef>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    data_sender: EventSender<DataEvent>,
}

impl KrakenSpotDataClient {
    /// Creates a new [`KrakenSpotDataClient`] instance.
    pub fn new(client_id: ClientId, config: KrakenDataClientConfig) -> anyhow::Result<Self> {
        let session_tasks = TaskGroup::new();
        let cancellation_token = session_tasks.cancellation_token();
        let command_tasks = TaskGroup::new();
        let socket_factory = SocketControlFactory::new(client_id, Some(*KRAKEN_VENUE));
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let max_requests_per_second = config
            .max_requests_per_second
            .unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND);
        let http = match (&config.api_key, &config.api_secret) {
            (Some(api_key), Some(api_secret)) => KrakenSpotHttpClient::with_credentials(
                api_key.expose_secret().to_owned(),
                api_secret.expose_secret().to_owned(),
                config.environment,
                config.base_url.clone(),
                config.timeout_secs,
                None,
                None,
                None,
                proxy_url.clone(),
                max_requests_per_second,
            )?,
            _ => KrakenSpotHttpClient::new(
                config.environment,
                config.base_url.clone(),
                config.timeout_secs,
                None,
                None,
                None,
                proxy_url.clone(),
                max_requests_per_second,
            )?,
        };

        let ws =
            KrakenSpotWebSocketClient::new(config.clone(), cancellation_token.clone(), proxy_url)
                .with_socket_control(socket_factory.control("kraken-spot-data-streams"));

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            config,
            http,
            ws,
            ws_l3: None,
            socket_factory,
            l3_handler_task: None,
            is_connected: AtomicBool::new(false),
            cancellation_token,
            session_tasks,
            command_tasks,
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

        log::debug!(
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
        let future = async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        };

        if let Err(e) = self.command_tasks.spawn(future) {
            log::warn!("Skipping Kraken Spot {context} after shutdown began: {e}");
        }
    }

    fn spawn_command<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Err(e) = self.command_tasks.spawn(future) {
            log::warn!("Skipping Kraken Spot data command after shutdown began: {e}");
        }
    }

    async fn finish_tasks(&self) -> anyhow::Result<()> {
        let (session_result, command_result) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
            self.command_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
        );
        session_result.context("failed to finish Kraken Spot data session tasks")?;
        command_result.context("failed to finish Kraken Spot data command tasks")?;
        Ok(())
    }

    async fn prepare_task_groups(&mut self) -> anyhow::Result<()> {
        if !self.session_tasks.is_open() || !self.command_tasks.is_open() {
            self.session_tasks.begin_shutdown();
            self.command_tasks.begin_shutdown();
            self.ws
                .close()
                .await
                .context("failed to close prior Kraken Spot WebSocket")?;

            if let Some(ws_l3) = self.ws_l3.as_mut() {
                ws_l3
                    .close()
                    .await
                    .context("failed to close prior Kraken Spot L3 WebSocket")?;
                self.ws_l3 = None;
                self.l3_handler_task = None;
            }
            self.finish_tasks().await?;
            self.session_tasks
                .start_generation()
                .context("failed to start Kraken Spot data session task generation")?;
            self.command_tasks
                .start_generation()
                .context("failed to start Kraken Spot data command task generation")?;
            self.cancellation_token = self.session_tasks.cancellation_token();
            self.ws = KrakenSpotWebSocketClient::new(
                self.config.clone(),
                self.cancellation_token.clone(),
                self.config
                    .proxy_url
                    .as_ref()
                    .map(|value| value.expose_secret().to_owned()),
            )
            .with_socket_control(self.socket_factory.control("kraken-spot-data-streams"));
        }
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        let ws_result = self.ws.close().await;
        let ws_l3_result = if let Some(ws_l3) = self.ws_l3.as_mut() {
            ws_l3.close().await
        } else {
            Ok(())
        };

        if ws_l3_result.is_ok() {
            self.ws_l3 = None;
            self.l3_handler_task = None;
        }
        let tasks_result = self.finish_tasks().await;
        self.is_connected.store(false, Ordering::Release);
        tasks_result?;
        ws_result?;
        Ok(ws_l3_result?)
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

        let handler_finished = self
            .l3_handler_task
            .as_ref()
            .is_none_or(TaskRef::is_finished);

        if self.ws_l3.is_none() {
            let ws_l3 = KrakenSpotWebSocketClient::l3(
                self.config.clone(),
                self.cancellation_token.clone(),
                self.config
                    .proxy_url
                    .as_ref()
                    .map(|value| value.expose_secret().to_owned()),
            )
            .with_socket_control(self.socket_factory.control("kraken-spot-l3-data-streams"));

            self.l3_handler_task = self.spawn_l3_handler_task(ws_l3.clone(), false);
            self.ws_l3 = Some(ws_l3);
        } else if handler_finished && let Some(ws_l3) = self.ws_l3.as_ref() {
            let ws_l3 = ws_l3.clone();
            self.l3_handler_task = self.spawn_l3_handler_task(ws_l3, true);
        }

        let ws_l3 = self
            .ws_l3
            .as_ref()
            .expect("ws_l3 initialized above")
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

        Ok(())
    }

    fn spawn_l3_handler_task(
        &self,
        handler_client: KrakenSpotWebSocketClient,
        restart: bool,
    ) -> Option<TaskRef> {
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;
        let session_spawner = match self.session_tasks.spawner() {
            Ok(spawner) => spawner,
            Err(e) => {
                log::warn!("Skipping Kraken L3 handler after shutdown began: {e}");
                return None;
            }
        };

        let future = async move {
            let mut handler_client = handler_client;

            if restart && let Err(e) = handler_client.close().await {
                log::error!("Failed to close prior L3 WebSocket generation: {e}");
                return;
            }

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
                            log::info!(
                                "Resyncing Kraken L3 book: symbol={}, depth={}, reason={}",
                                request.symbol,
                                request.depth,
                                request.reason,
                            );
                            let symbol_ustr = Ustr::from(&request.symbol);
                            let client_for_resync = resync_client.clone();

                            if let Err(e) = session_spawner.spawn_named(
                                "kraken-spot-l3-resync",
                                async move {
                                    retry_l3_resync(
                                        &client_for_resync,
                                        symbol_ustr,
                                        request.depth,
                                    )
                                    .await;
                                },
                            ) {
                                log::warn!("Skipping Kraken L3 resync after shutdown began: {e}");
                            }
                        }
                    }
                }
            }
        };

        match self
            .session_tasks
            .spawn_named("kraken-spot-l3-handler", future)
        {
            Ok(task) => Some(task),
            Err(e) => {
                log::warn!("Skipping Kraken L3 handler after shutdown began: {e}");
                None
            }
        }
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let stream = self.ws.stream().map_err(|e| anyhow::anyhow!("{e}"))?;
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let book_sequence = Arc::new(AtomicU64::new(0));
        let ohlc_buffer: OhlcBuffer = Arc::new(Mutex::new(AHashMap::new()));
        let l2_depths = self.ws.l2_depths_handle();
        let book_requests = self.ws.book_requests_handle();
        let validate_l2_checksum = self.ws.validate_l2_checksum();
        let resync_client = self.ws.clone();
        let session_spawner = self
            .session_tasks
            .spawner()
            .context("failed to acquire a task spawner for Kraken Spot L2 resync")?;
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;

        let future = async move {
            tokio::pin!(stream);
            let mut l2_books = L2BookState::new(validate_l2_checksum);
            // The venue answers a `book` subscribe with a snapshot; a send the transport dropped or
            // a subscribe the venue rejected leaves a cleared book waiting for one that never comes
            // while other traffic keeps the connection alive, so the wait is checked on a timer.
            let mut watchdog = tokio::time::interval(Duration::from_secs(1));
            watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);

            loop {
                let context = SpotMessageContext {
                    sender: &data_sender,
                    instruments: &instruments,
                    book_sequence: &book_sequence,
                    l2_depths: &l2_depths,
                    book_requests: &book_requests,
                    ohlc_buffer: &ohlc_buffer,
                    clock,
                };

                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Spot message handler cancelled");
                        Self::flush_ohlc_buffer(&ohlc_buffer, &data_sender);
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                let resyncs =
                                    Self::handle_ws_message(ws_msg, &context, &mut l2_books);

                                for request in &resyncs {
                                    log::info!(
                                        "Resyncing Kraken L2 book after checksum mismatch: {}",
                                        request.instrument_id
                                    );
                                }

                                Self::spawn_l2_resyncs(&session_spawner, &resync_client, resyncs);
                            }
                            None => {
                                log::debug!("Spot WebSocket stream ended");
                                Self::flush_ohlc_buffer(&ohlc_buffer, &data_sender);
                                break;
                            }
                        }
                    }
                    _ = watchdog.tick() => {
                        let resyncs = Self::check_l2_snapshots(&context, &mut l2_books);
                        Self::spawn_l2_resyncs(&session_spawner, &resync_client, resyncs);
                    }
                }
            }
        };

        self.session_tasks
            .spawn(future)
            .context("failed to register Kraken Spot message handler")
    }

    /// Sends each recovery once: `resync_book` fails only when the command channel is closed,
    /// which no retry can mend, and the snapshot watchdog asks again for a snapshot that does not
    /// arrive, so a retry chain here would only overlap its requests.
    fn spawn_l2_resyncs(
        spawner: &TaskSpawner,
        client: &KrakenSpotWebSocketClient,
        requests: Vec<L2ResyncRequest>,
    ) {
        for request in requests {
            let client = client.clone();

            if let Err(e) = spawner.spawn_named("kraken-spot-l2-resync", async move {
                if let Err(e) = client
                    .resync_book(request.instrument_id, request.generation, request.epoch)
                    .await
                {
                    log::error!(
                        "Failed to send the L2 resync for {}: {e}; the snapshot watchdog asks \
                         again if the book stays cleared",
                        request.instrument_id
                    );
                }
            }) {
                log::warn!("Skipping Kraken L2 resync after shutdown began: {e}");
            }
        }
    }

    /// Requests the snapshot again for every held `book` subscription whose book is overdue, and
    /// clears the consumer's book for every held instrument whose shadow book the check dropped.
    ///
    /// A held symbol without an instrument cannot be processed when its snapshot arrives either,
    /// so it is left out.
    fn check_l2_snapshots(
        context: &SpotMessageContext,
        l2_books: &mut L2BookState,
    ) -> Vec<L2ResyncRequest> {
        let now = context.clock.get_time_ns();
        let held = Self::held_l2_subscriptions(context);
        let check = l2_books.overdue_snapshots(now, &held);

        for instrument_id in check.cleared {
            Self::emit_book_clear(context, instrument_id, now);
        }

        check.requests
    }

    /// Every held `book` subscription by instrument; a held symbol without an instrument is left
    /// out.
    fn held_l2_subscriptions(context: &SpotMessageContext) -> Vec<(InstrumentId, L2Subscription)> {
        let instruments = context.instruments.load();
        context
            .l2_depths
            .held()
            .into_iter()
            .filter_map(|(symbol, subscription)| {
                lookup_instrument_in_snapshot(&instruments, &symbol)
                    .map(|instrument| (instrument.id(), subscription))
            })
            .collect()
    }

    /// Clears the consumer's book for `instrument_id` once its shadow book is dropped off the
    /// frame path, under the shared book sequence like the deltas the `book` arm sends.
    fn emit_book_clear(context: &SpotMessageContext, instrument_id: InstrumentId, now: UnixNanos) {
        let sequence = context.book_sequence.load(Ordering::Relaxed);
        let (deltas, next_sequence) = clear_deltas(instrument_id, sequence, now, now);
        context
            .book_sequence
            .store(next_sequence, Ordering::Relaxed);

        if let Err(e) = context
            .sender
            .send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
        {
            log::error!("Failed to send deltas: {e}");
        }
    }

    fn flush_ohlc_buffer(ohlc_buffer: &OhlcBuffer, sender: &EventSender<DataEvent>) {
        let mut buffer = ohlc_buffer.lock();
        let bars: Vec<Bar> = buffer.drain().map(|(_, (bar, _))| bar).collect();
        for bar in bars {
            if let Err(e) = sender.send(DataEvent::Data(Data::Bar(bar))) {
                log::error!("Failed to send buffered bar: {e}");
            }
        }
    }

    fn handle_ws_message(
        msg: KrakenSpotWsMessage,
        context: &SpotMessageContext,
        l2_books: &mut L2BookState,
    ) -> Vec<L2ResyncRequest> {
        let mut resyncs = Vec::new();
        let ts_init = context.clock.get_time_ns();

        match msg {
            KrakenSpotWsMessage::Ticker(tickers) => {
                let instruments = context.instruments.load();

                for ticker in &tickers {
                    let Some(instrument) =
                        lookup_instrument_in_snapshot(&instruments, ticker.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", ticker.symbol);
                        continue;
                    };

                    match parse_quote_tick(ticker, instrument, ts_init) {
                        Ok(quote) => {
                            if let Err(e) = context.sender.send(DataEvent::Data(Data::Quote(quote)))
                            {
                                log::error!("Failed to send quote: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse quote tick: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Trade(trades) => {
                let instruments = context.instruments.load();

                for trade in &trades {
                    let Some(instrument) =
                        lookup_instrument_in_snapshot(&instruments, trade.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", trade.symbol);
                        continue;
                    };

                    match parse_trade_tick(trade, instrument, ts_init) {
                        Ok(tick) => {
                            if let Err(e) = context.sender.send(DataEvent::Data(Data::Trade(tick)))
                            {
                                log::error!("Failed to send trade: {e}");
                            }
                        }
                        Err(e) => log::error!("Failed to parse trade tick: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Book { data, is_snapshot } => {
                let instruments = context.instruments.load();

                for book in &data {
                    let Some(instrument) =
                        lookup_instrument_in_snapshot(&instruments, book.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", book.symbol);
                        continue;
                    };
                    let sequence = context.book_sequence.load(Ordering::Relaxed);

                    match l2_books.process_book(
                        book,
                        instrument,
                        sequence,
                        is_snapshot,
                        context.l2_depths,
                        ts_init,
                    ) {
                        Ok(outcome) => {
                            if let Some((deltas, next_sequence)) = outcome.deltas {
                                context
                                    .book_sequence
                                    .store(next_sequence, Ordering::Relaxed);

                                if let Err(e) = context
                                    .sender
                                    .send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
                                {
                                    log::error!("Failed to send deltas: {e}");
                                }
                            }

                            // One recovery per instrument per message, the latest: a second request
                            // for the same subscription would cost a second unsubscribe and
                            // subscribe cycle and a second snapshot, and only the latest carries
                            // the epoch the message's last accepted snapshot moved to, so an earlier
                            // one would be skipped as already served.
                            if let Some(request) = outcome.resync {
                                match resyncs.iter_mut().find(|r: &&mut L2ResyncRequest| {
                                    r.instrument_id == request.instrument_id
                                }) {
                                    Some(existing) => *existing = request,
                                    None => resyncs.push(request),
                                }
                            }
                        }
                        Err(e) => log::error!("Failed to parse book deltas: {e}"),
                    }
                }
            }
            KrakenSpotWsMessage::Ohlc(ohlc_data) => {
                let mut buffer = context.ohlc_buffer.lock();

                let instruments = context.instruments.load();

                for ohlc in &ohlc_data {
                    let Some(instrument) =
                        lookup_instrument_in_snapshot(&instruments, ohlc.symbol.as_str())
                    else {
                        log::warn!("No instrument for symbol: {}", ohlc.symbol);
                        continue;
                    };

                    match parse_ws_bar(ohlc, instrument, ts_init) {
                        Ok(new_bar) => {
                            let key: (Ustr, u32) = (ohlc.symbol, ohlc.interval);
                            let new_interval_begin = UnixNanos::from(
                                u64::try_from(ohlc.interval_begin.as_nanosecond()).unwrap_or(0),
                            );

                            if let Some((buffered_bar, buffered_begin)) = buffer.get(&key)
                                && new_interval_begin != *buffered_begin
                                && let Err(e) = context
                                    .sender
                                    .send(DataEvent::Data(Data::Bar(*buffered_bar)))
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
            KrakenSpotWsMessage::SubscriptionAck {
                req_id,
                symbol,
                success,
                error,
            } => {
                // Only a `book` subscribe is on record, and only its first answer: any other
                // answer, a second one to the same id and a failed unsubscribe the venue reports
                // under the subscribe method all match nothing and are left to their log line.
                let request = req_id.and_then(|req_id| {
                    let request = context.book_requests.lock().remove(&req_id);
                    request.map(|request| (req_id, request))
                });

                match request {
                    Some((req_id, request)) if success => {
                        Self::handle_book_confirmation(context, l2_books, req_id, request, ts_init);
                    }
                    Some((req_id, request)) => {
                        Self::handle_book_rejection(
                            context,
                            l2_books,
                            req_id,
                            request,
                            symbol,
                            error.as_deref(),
                            ts_init,
                        );
                    }
                    None => {}
                }
            }
            KrakenSpotWsMessage::Reconnected => {
                let held = Self::held_l2_subscriptions(context);

                for instrument_id in l2_books.reset_after_reconnect(ts_init, &held) {
                    Self::emit_book_clear(context, instrument_id, ts_init);
                }

                log::info!("Spot WebSocket reconnected");
            }
        }

        resyncs
    }

    /// Opens the stream of a `book` subscribe the venue confirmed and clears the consumer's book
    /// when that drops a shadow book.
    ///
    /// Only the symbol's latest request opens a stream: a confirmation of one a later request has
    /// superseded, or of one whose subscription is canceled, starts nothing, since its stream is
    /// retired before it begins.
    fn handle_book_confirmation(
        context: &SpotMessageContext,
        l2_books: &mut L2BookState,
        req_id: u64,
        request: L2BookRequest,
        now: UnixNanos,
    ) {
        let held = context.l2_depths.subscription(request.symbol.as_str());
        let Some(subscription) = held.filter(|held| held.latest_request == req_id) else {
            log::debug!(
                "Ignoring the confirmation of a superseded L2 subscribe: symbol={}, \
                 req_id={req_id}",
                request.symbol
            );
            return;
        };

        let instruments = context.instruments.load();
        let Some(instrument) = lookup_instrument_in_snapshot(&instruments, request.symbol.as_str())
        else {
            log::debug!(
                "No instrument for the confirmed L2 subscribe of {}, so its frames are dropped",
                request.symbol
            );
            return;
        };

        if l2_books.start_stream(instrument.id(), subscription, now) {
            Self::emit_book_clear(context, instrument.id(), now);
        }
    }

    /// Counts a `book` subscribe the venue rejected against its wait and clears the consumer's
    /// book when the rejection drops a shadow book.
    ///
    /// The rejection acts only when it answers the symbol's latest request, matched by request id
    /// alone: one a later request has superseded is ignored, since that request's own answer
    /// decides, and one for a canceled subscription has nothing to clear. A rejection of the latest
    /// request acts even while a book is held, since that book is a retired stream's.
    fn handle_book_rejection(
        context: &SpotMessageContext,
        l2_books: &mut L2BookState,
        req_id: u64,
        request: L2BookRequest,
        symbol: Option<Ustr>,
        error: Option<&str>,
        now: UnixNanos,
    ) {
        let symbol = symbol.unwrap_or(request.symbol);
        let reason = error.unwrap_or("no reason given");
        let held = context.l2_depths.subscription(request.symbol.as_str());

        let Some(subscription) = held.filter(|held| held.latest_request == req_id) else {
            log::debug!(
                "Ignoring a rejected L2 subscribe superseded by a later request or a canceled \
                 subscription: symbol={symbol}, req_id={req_id}, error={reason}"
            );
            return;
        };

        let instruments = context.instruments.load();
        let Some(instrument) = lookup_instrument_in_snapshot(&instruments, request.symbol.as_str())
        else {
            log::error!(
                "Kraken rejected the L2 book subscribe for {symbol}: {reason}; no instrument \
                 for the symbol, so there is no book to clear"
            );
            return;
        };

        let rejection = l2_books.reject_subscription(instrument.id(), subscription.generation, now);

        if rejection.cleared {
            Self::emit_book_clear(context, instrument.id(), now);
        }

        match rejection.next_request_due {
            Some(due) => log::error!(
                "Kraken rejected the L2 book subscribe for {symbol}: {reason}; the book stays \
                 cleared, asking again in {} s",
                due.saturating_duration_since(now).as_u64() / 1_000_000_000
            ),
            None => log::error!(
                "Kraken rejected the L2 book subscribe for {symbol}: {reason}; the snapshot \
                 request cap is reached, so the book stays cleared until the next subscription \
                 change or reconnect"
            ),
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
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Spot data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);

        self.instruments.store(ahash::AHashMap::new());
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Spot data client: {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::SeqCst)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() && self.session_tasks.is_open() && self.command_tasks.is_open() {
            return Ok(());
        }

        self.prepare_task_groups().await?;

        let instruments = self.load_instruments().await?;

        let session_result = async {
            self.ws
                .connect()
                .await
                .context("Failed to connect spot WebSocket")?;
            self.ws
                .wait_until_active(10.0)
                .await
                .context("Spot WebSocket failed to become active")?;

            self.spawn_message_handler()?;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Kraken Spot data startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

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
        self.teardown_partial_connect().await?;
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

        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        log::debug!(
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

        self.spawn_command(async move {
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

        self.spawn_command(async move {
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

        self.spawn_command(async move {
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

        self.spawn_command(async move {
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

        self.spawn_command(async move {
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

type OhlcBufferKey = (Ustr, u32);
type OhlcBuffer = Arc<Mutex<AHashMap<OhlcBufferKey, (Bar, UnixNanos)>>>;

struct DataEventSink<'a> {
    sender: &'a EventSender<DataEvent>,
}

impl L3Sink for DataEventSink<'_> {
    fn emit_deltas(&mut self, deltas: OrderBookDeltas) {
        if let Err(e) = self
            .sender
            .send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
        {
            log::error!("Failed to send L3 deltas: {e}");
        }
    }
}

struct SpotMessageContext<'a> {
    sender: &'a EventSender<DataEvent>,
    instruments: &'a Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    book_sequence: &'a Arc<AtomicU64>,
    l2_depths: &'a L2Depths,
    book_requests: &'a L2BookRequests,
    ohlc_buffer: &'a OhlcBuffer,
    clock: &'static AtomicTime,
}

#[cfg(test)]
mod tests {
    use nautilus_common::{live::runner::set_data_event_sender, messages::DataEvent};
    use nautilus_model::{
        enums::{BookAction, RecordFlag},
        identifiers::Symbol,
        instruments::{InstrumentAny, currency_pair::CurrencyPair},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::consts::KRAKEN_CLIENT_ID,
        config::KrakenDataClientConfig,
        websocket::spot_v2::{
            level_3::messages::KrakenL3Snapshot,
            messages::{KrakenWsBookData, KrakenWsBookLevel},
        },
    };

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    fn make_instrument() -> InstrumentAny {
        make_instrument_for("BTC/USD")
    }

    fn make_instrument_for(symbol: &str) -> InstrumentAny {
        let (base, quote) = symbol.split_once('/').expect("a base/quote symbol");
        InstrumentAny::CurrencyPair(
            CurrencyPair::builder()
                .instrument_id(InstrumentId::from(format!("{symbol}.KRAKEN").as_str()))
                .raw_symbol(Symbol::from(symbol))
                .base_currency(Currency::from(base))
                .quote_currency(Currency::from(quote))
                .price_precision(1)
                .size_precision(8)
                .price_increment(Price::from("0.1"))
                .size_increment(Quantity::from("0.00000001"))
                .ts_event(UnixNanos::default())
                .ts_init(UnixNanos::default())
                .build()
                .unwrap(),
        )
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
    #[tokio::test]
    async fn test_teardown_clears_l3_client_and_handler_task() {
        setup_test_env();
        let config = KrakenDataClientConfig::default();
        let mut client = KrakenSpotDataClient::new(*KRAKEN_CLIENT_ID, config.clone()).unwrap();
        let cancellation = client.session_tasks.cancellation_token();
        let task = client
            .session_tasks
            .spawn_named("kraken-spot-l3-handler", async move {
                cancellation.cancelled().await;
            })
            .unwrap();
        client.l3_handler_task = Some(task.clone());
        client.ws_l3 = Some(KrakenSpotWebSocketClient::l3(
            config,
            client.cancellation_token.clone(),
            None,
        ));

        client.teardown_partial_connect().await.unwrap();

        assert!(client.ws_l3.is_none());
        assert!(client.l3_handler_task.is_none());
        assert!(task.is_finished());
    }

    #[rstest]
    fn test_l3_snapshot_checksum_mismatch_emits_clear_and_requests_resync() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let instruments = Arc::new(AtomicMap::new());
        let instrument = make_instrument();
        instruments.insert(instrument.id(), instrument);

        let depths = Arc::new(Mutex::new(AHashMap::new()));
        depths.lock().insert("BTC/USD".to_string(), 1000);

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

        let mut sink = DataEventSink {
            sender: &sender.into(),
        };

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
        let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
            panic!("expected deltas event");
        };

        assert_eq!(deltas.deltas.len(), 1);
        assert_eq!(deltas.deltas[0].action, BookAction::Clear);
        assert!(states["BTC/USD"].awaiting_snapshot);
        assert!(states["BTC/USD"].open_orders.is_empty());
        assert!(receiver.try_recv().is_err());
    }

    const BTC: &str = "BTC/USD";

    /// A data client's L2 state with BTC/USD loaded and a settable clock, driving
    /// `handle_ws_message` and `check_l2_snapshots` as the message loop does.
    struct L2Harness {
        sender: EventSender<DataEvent>,
        receiver: tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        book_sequence: Arc<AtomicU64>,
        l2_depths: L2Depths,
        book_requests: L2BookRequests,
        ohlc_buffer: OhlcBuffer,
        clock: &'static AtomicTime,
        l2_books: L2BookState,
        start: UnixNanos,
    }

    impl L2Harness {
        fn new(validate_checksum: bool) -> Self {
            let start = UnixNanos::new(1_700_000_000_000_000_000);
            let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
            let instruments = Arc::new(AtomicMap::new());
            let btc = make_instrument();
            instruments.insert(btc.id(), btc);

            Self {
                sender: sender.into(),
                receiver,
                instruments,
                book_sequence: Arc::new(AtomicU64::new(0)),
                l2_depths: L2Depths::default(),
                book_requests: Arc::new(Mutex::new(AHashMap::new())),
                ohlc_buffer: Arc::new(Mutex::new(AHashMap::new())),
                clock: Box::leak(Box::new(AtomicTime::new(false, start))),
                l2_books: L2BookState::new(validate_checksum),
                start,
            }
        }

        fn handle(&mut self, msg: KrakenSpotWsMessage) -> Vec<L2ResyncRequest> {
            let context = SpotMessageContext {
                sender: &self.sender,
                instruments: &self.instruments,
                book_sequence: &self.book_sequence,
                l2_depths: &self.l2_depths,
                book_requests: &self.book_requests,
                ohlc_buffer: &self.ohlc_buffer,
                clock: self.clock,
            };
            KrakenSpotDataClient::handle_ws_message(msg, &context, &mut self.l2_books)
        }

        fn check(&mut self) -> Vec<L2ResyncRequest> {
            let context = SpotMessageContext {
                sender: &self.sender,
                instruments: &self.instruments,
                book_sequence: &self.book_sequence,
                l2_depths: &self.l2_depths,
                book_requests: &self.book_requests,
                ohlc_buffer: &self.ohlc_buffer,
                clock: self.clock,
            };
            KrakenSpotDataClient::check_l2_snapshots(&context, &mut self.l2_books)
        }

        /// Moves the clock to `secs` seconds after the start.
        fn at(&self, secs: u64) {
            self.clock
                .set_time(UnixNanos::new(self.start.as_u64() + secs * 1_000_000_000));
        }

        fn held(&self) -> L2Subscription {
            self.l2_depths
                .subscription(BTC)
                .expect("a held subscription")
        }

        fn record(&self, request: u64) {
            self.book_requests.lock().insert(
                request,
                L2BookRequest {
                    symbol: Ustr::from(BTC),
                },
            );
        }

        /// Sends a BTC/USD `book` subscribe at `depth` as `request`, as `subscribe_book` does.
        fn subscribe(&self, depth: u32, request: u64) -> L2Subscription {
            self.l2_depths.insert(BTC, depth, request);
            self.record(request);
            self.held()
        }

        /// Sends a recovery's resubscribe as `request`, as `resync_book` does.
        fn resubscribe(&self, request: u64) {
            let live = self.held();
            self.l2_depths
                .begin_resync(BTC, live.generation, live.snapshot_epoch, request)
                .expect("the recovery is admitted");
            self.record(request);
        }

        /// The venue's answer to `request`: a confirmation, or a rejection naming the pair.
        fn answer(&mut self, request: u64, error: Option<&str>) -> Vec<L2ResyncRequest> {
            self.handle(KrakenSpotWsMessage::SubscriptionAck {
                req_id: Some(request),
                symbol: error.map(|_| Ustr::from(BTC)),
                success: error.is_none(),
                error: error.map(str::to_string),
            })
        }

        fn snapshot(&mut self) -> Vec<L2ResyncRequest> {
            self.handle(KrakenSpotWsMessage::Book {
                data: vec![btc_book(None)],
                is_snapshot: true,
            })
        }

        fn update(&mut self, checksum: Option<u32>) -> Vec<L2ResyncRequest> {
            let mut update = btc_book(checksum);
            update.bids = Some(vec![book_level(dec!(100), dec!(2))]);
            update.asks = Some(vec![]);
            self.handle(KrakenSpotWsMessage::Book {
                data: vec![update],
                is_snapshot: false,
            })
        }

        /// The recovery the held subscription calls for: its generation and current epoch.
        fn recovery(&self) -> L2ResyncRequest {
            let live = self.held();
            L2ResyncRequest {
                instrument_id: make_instrument().id(),
                generation: live.generation,
                epoch: live.snapshot_epoch,
            }
        }

        fn has_book(&self) -> bool {
            self.l2_books.books.contains_key(&make_instrument().id())
        }

        /// The book deltas emitted since the last call.
        fn events(&mut self) -> Vec<OrderBookDeltas> {
            let mut events = Vec::new();

            while let Ok(event) = self.receiver.try_recv() {
                let DataEvent::Data(Data::BookDeltas(deltas)) = event else {
                    panic!("expected a deltas event");
                };
                events.push(*deltas);
            }
            events
        }
    }

    /// One BTC/USD level a side.
    fn btc_book(checksum: Option<u32>) -> KrakenWsBookData {
        KrakenWsBookData {
            symbol: Ustr::from(BTC),
            bids: Some(vec![book_level(dec!(100), Decimal::ONE)]),
            asks: Some(vec![book_level(dec!(101), Decimal::ONE)]),
            checksum,
            timestamp: "2024-01-01T00:00:00Z".parse().unwrap(),
        }
    }

    fn is_clear(deltas: &OrderBookDeltas) -> bool {
        deltas.deltas.len() == 1
            && deltas.deltas[0].action == BookAction::Clear
            && RecordFlag::F_LAST.matches(deltas.deltas[0].flags)
    }

    /// Every instrument in a message that mismatches gets one resync request, carrying the epoch of
    /// its last accepted snapshot.
    #[rstest]
    fn test_l2_handler_returns_one_resync_request_per_mismatching_instrument() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        let bad_snapshot = btc_book(Some(1));

        let resyncs = harness.handle(KrakenSpotWsMessage::Book {
            data: vec![bad_snapshot.clone(), bad_snapshot],
            is_snapshot: true,
        });

        assert_eq!(
            resyncs.len(),
            1,
            "one request per mismatching instrument: {resyncs:?}"
        );
        assert!(
            resyncs
                .iter()
                .all(|r| r.instrument_id == make_instrument().id())
        );
        assert_eq!(
            resyncs[0].epoch,
            harness
                .l2_depths
                .subscription("BTC/USD")
                .unwrap()
                .snapshot_epoch,
            "the surviving request carries the latest epoch, so it is not skipped as served"
        );
    }

    /// The watchdog requests the snapshot for a held book that is overdue and leaves a book whose
    /// wait has only just started alone.
    #[rstest]
    fn test_check_l2_snapshots_requests_only_the_overdue_book() {
        let mut harness = L2Harness::new(true);
        let eth = make_instrument_for("ETH/USD");
        harness.instruments.insert(eth.id(), eth);
        harness.subscribe(10, 1);

        assert!(
            harness.check().is_empty(),
            "the first check starts the wait"
        );

        harness.l2_depths.insert("ETH/USD", 25, 2);
        harness.at(10);

        assert_eq!(harness.check(), vec![harness.recovery()]);
    }

    /// A rejected `book` subscribe counts as one failed request for the subscription behind it,
    /// matched by request id whether or not the rejection names the pair: no request before the
    /// doubled base wait, one at it, while a new generation is waited for and requested as usual.
    #[rstest]
    #[case::unsupported_pair(Some(BTC), "Currency pair not supported BTC/USD")]
    #[case::rate_limit_without_a_pair(None, "Exceeded msg rate")]
    fn test_a_rejected_book_subscribe_is_requested_again_after_the_doubled_wait(
        #[case] symbol: Option<&str>,
        #[case] error: &str,
    ) {
        let mut harness = L2Harness::new(true);
        let rejected = harness.subscribe(10, 7);

        assert!(harness.check().is_empty());

        let resyncs = harness.handle(KrakenSpotWsMessage::SubscriptionAck {
            req_id: Some(7),
            symbol: symbol.map(Ustr::from),
            success: false,
            error: Some(error.to_string()),
        });
        assert!(resyncs.is_empty());
        assert!(
            harness.book_requests.lock().is_empty(),
            "the answered request is retired"
        );

        harness.at(19);
        assert!(
            harness.check().is_empty(),
            "no request before the doubled base wait"
        );

        harness.at(20);
        assert_eq!(
            harness.check(),
            vec![L2ResyncRequest {
                instrument_id: make_instrument().id(),
                generation: rejected.generation,
                epoch: 0,
            }],
            "the rejected subscribe is asked again after 20 s"
        );

        let replacement = harness.subscribe(10, 8);
        assert!(
            harness.check().is_empty(),
            "the new generation's wait starts at this tick"
        );
        harness.at(30);
        assert_eq!(
            harness.check(),
            vec![L2ResyncRequest {
                instrument_id: make_instrument().id(),
                generation: replacement.generation,
                epoch: 0,
            }]
        );
    }

    /// A rejection of a request a later one has superseded is stale: the consumer's book is kept,
    /// nothing is emitted, and the shadow book stays.
    #[rstest]
    fn test_a_superseded_requests_rejection_leaves_a_fed_book_alone() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.resubscribe(2);
        harness.answer(2, None);
        harness.snapshot();
        assert_eq!(harness.events().len(), 1, "the snapshot is emitted");
        let sequence = harness.book_sequence.load(Ordering::Relaxed);

        harness.answer(1, Some("Already subscribed"));

        assert!(
            harness.events().is_empty(),
            "a stale rejection emits nothing to the consumer"
        );
        assert!(harness.has_book(), "the fed book is kept");
        assert_eq!(harness.book_sequence.load(Ordering::Relaxed), sequence);
    }

    /// A rejection of the latest request acts even while a book is held, since the book is the
    /// older stream's: the consumer's book is cleared, the older stream's frames stay dropped, and
    /// the watchdog asks again after the doubled base wait.
    #[rstest]
    fn test_a_rejection_of_the_latest_request_clears_a_fed_book() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.events();
        // The user's replacement; the older stream still runs, as when the unsubscribe was
        // rate-limited and the venue answers the subscribe with "Already subscribed".
        let replacement = harness.subscribe(25, 2);
        harness.at(1);

        harness.answer(2, Some("Already subscribed"));

        let events = harness.events();
        assert_eq!(events.len(), 1);
        assert!(is_clear(&events[0]));
        assert!(!harness.has_book());

        harness.update(None);
        assert!(
            harness.events().is_empty(),
            "the older stream's frames stay dropped"
        );

        harness.at(20);
        assert!(harness.check().is_empty());
        harness.at(21);
        assert_eq!(
            harness.check(),
            vec![harness.recovery()],
            "the watchdog keeps asking"
        );
        assert_eq!(harness.recovery().generation, replacement.generation);
    }

    /// A reconnect drops every shadow book and clears the consumer's books: one `Clear` delta per
    /// book under the shared sequence, flagged last. The replay's answers to a request already
    /// answered are not acted on, and the replayed snapshot is accepted.
    #[rstest]
    fn test_a_reconnect_clears_the_consumers_books() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        assert_eq!(harness.events().len(), 1, "the snapshot is emitted");
        let sequence = harness.book_sequence.load(Ordering::Relaxed);

        harness.handle(KrakenSpotWsMessage::Reconnected);

        let events = harness.events();
        assert_eq!(events.len(), 1, "the consumer's book is cleared");
        assert_eq!(events[0].instrument_id, make_instrument().id());
        assert!(is_clear(&events[0]));
        assert_eq!(events[0].deltas[0].sequence, sequence);
        assert_eq!(harness.book_sequence.load(Ordering::Relaxed), sequence + 1);
        assert!(harness.l2_books.books.is_empty());

        harness.answer(1, None);
        harness.answer(1, Some("Already subscribed"));
        assert!(harness.events().is_empty());

        harness.snapshot();
        assert_eq!(
            harness.events().len(),
            1,
            "the replayed snapshot is emitted"
        );
        assert!(harness.has_book());
    }

    /// The tick that drops the book of a superseded stream clears the consumer's book, the
    /// replacement's confirmation then sends no second `Clear`, and a book dropped with its
    /// canceled subscription sends nothing.
    #[rstest]
    fn test_the_tick_that_drops_a_retired_book_clears_the_consumers_book() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.events();
        let sequence = harness.book_sequence.load(Ordering::Relaxed);

        harness.subscribe(25, 2);
        harness.at(1);
        assert!(
            harness.check().is_empty(),
            "the replacement's wait starts at this tick"
        );

        let events = harness.events();
        assert_eq!(events.len(), 1, "the retired book is cleared downstream");
        assert!(is_clear(&events[0]));
        assert_eq!(events[0].deltas[0].sequence, sequence);
        assert_eq!(
            events[0].deltas[0].ts_event,
            UnixNanos::new(harness.start.as_u64() + 1_000_000_000)
        );
        assert_eq!(harness.book_sequence.load(Ordering::Relaxed), sequence + 1);

        harness.answer(2, None);
        assert!(harness.events().is_empty(), "the book is already cleared");

        // Control: a book dropped with its canceled subscription has no consumer to clear.
        harness.snapshot();
        assert_eq!(harness.events().len(), 1, "the snapshot is emitted");
        harness.l2_depths.remove(BTC);
        assert!(harness.check().is_empty());
        assert!(
            harness.events().is_empty(),
            "no clear for an unsubscribed book"
        );
    }

    /// Answers other than the first one to the symbol's latest request change nothing: a
    /// confirmation or a rejection of a superseded request, a rejection with an unrecorded id such
    /// as a reconnect replay's, a failed unsubscribe the venue reports under the subscribe method,
    /// and a confirmation for a canceled subscription. The latest request opens no stream, and its
    /// snapshot is asked for after the base wait.
    #[rstest]
    fn test_answers_other_than_the_latest_requests_change_nothing() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 6);
        harness.subscribe(10, 7);
        let live = harness.subscribe(10, 8);
        harness.book_requests.lock().insert(
            11,
            L2BookRequest {
                symbol: Ustr::from("ETH/USD"),
            },
        );
        assert!(harness.check().is_empty());

        harness.answer(6, Some("Already subscribed"));
        harness.answer(7, None);
        harness.answer(9, Some("Currency pair not supported BTC/USD"));
        harness.handle(KrakenSpotWsMessage::SubscriptionAck {
            req_id: Some(10),
            symbol: Some(Ustr::from(BTC)),
            success: false,
            error: Some("Subscription with depth 10 not Found BTC/USD".to_string()),
        });
        harness.answer(11, None);

        assert!(harness.events().is_empty());
        assert_eq!(
            harness
                .book_requests
                .lock()
                .keys()
                .copied()
                .collect::<Vec<_>>(),
            vec![8]
        );

        harness.snapshot();
        assert!(
            harness.events().is_empty(),
            "no answer opened the latest request's stream"
        );

        harness.at(10);
        assert_eq!(harness.check(), vec![harness.recovery()]);
        assert_eq!(harness.recovery().generation, live.generation);
    }

    /// A replacement's confirmation clears a fed book exactly once, and a snapshot of the
    /// retired stream consumed before that confirmation is dropped, so the watchdog still asks for
    /// the replacement's snapshot when it does not come.
    #[rstest]
    fn test_a_replacements_confirmation_clears_the_consumers_book_once() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.events();

        harness.l2_depths.remove(BTC);
        let replacement = harness.subscribe(100, 2);
        harness.snapshot();
        assert!(
            harness.events().is_empty(),
            "the retired stream's snapshot is dropped"
        );

        harness.at(1);
        harness.answer(2, None);
        let events = harness.events();
        assert_eq!(events.len(), 1);
        assert!(is_clear(&events[0]));

        assert!(harness.check().is_empty());
        assert!(harness.events().is_empty(), "no second clear");

        harness.at(11);
        assert_eq!(harness.check(), vec![harness.recovery()]);
        assert_eq!(harness.recovery().generation, replacement.generation);
    }

    /// Of two recoveries sent back to back, only the later one opens a stream: the earlier one's
    /// confirmation and snapshot are dropped, and the later one's snapshot feeds the book.
    #[rstest]
    fn test_back_to_back_recoveries_accept_only_the_latest() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.events();
        assert_eq!(harness.update(Some(1)).len(), 1, "the mismatch recovers");
        assert!(is_clear(&harness.events()[0]));

        harness.resubscribe(2);
        harness.resubscribe(3);
        harness.answer(2, None);
        harness.snapshot();
        assert!(
            harness.events().is_empty(),
            "the superseded recovery's stream is dropped"
        );

        harness.answer(3, None);
        assert!(harness.events().is_empty(), "no book to clear");
        harness.snapshot();
        assert_eq!(harness.events().len(), 1);
        assert!(harness.has_book());
    }

    /// Each request is answered once: a second answer to a confirmed request, such as the
    /// "Already subscribed" a duplicate send draws, leaves the stream and its book alone.
    #[rstest]
    fn test_a_second_answer_to_one_request_is_ignored() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        assert_eq!(harness.events().len(), 1);

        harness.answer(1, Some("Already subscribed"));

        assert!(harness.events().is_empty());
        assert!(harness.has_book());
        harness.at(100);
        assert!(harness.check().is_empty());
    }

    /// A recovery whose subscribe is recorded after the reconnect replay began, so the venue
    /// answers its id twice, feeds the book: the reconnect makes the request live, its
    /// confirmation changes nothing, its snapshot is accepted, and the replay's "Already
    /// subscribed" is ignored.
    #[rstest]
    fn test_a_recovery_queued_across_a_reconnect_feeds_the_book() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.update(Some(1));
        harness.events();

        harness.resubscribe(2);
        harness.handle(KrakenSpotWsMessage::Reconnected);
        assert!(harness.events().is_empty(), "the mismatch dropped the book");

        harness.answer(2, None);
        assert!(harness.events().is_empty());
        harness.snapshot();
        assert_eq!(harness.events().len(), 1);

        harness.answer(2, Some("Already subscribed"));
        assert!(harness.events().is_empty());
        assert!(harness.has_book());
    }

    /// A reconnect replay that lands between a recovery's unsubscribe and subscribe costs one
    /// spurious clear: the replayed stream's snapshot is accepted under the recovery's request,
    /// the venue rejects that request as already subscribed, and the watchdog resubscribes within
    /// the doubled base wait.
    #[rstest]
    fn test_a_replay_between_the_recovery_commands_recovers_within_the_doubled_wait() {
        let mut harness = L2Harness::new(true);
        harness.subscribe(10, 1);
        harness.answer(1, None);
        harness.snapshot();
        harness.update(Some(1));
        harness.events();

        harness.resubscribe(2);
        harness.l2_depths.advance_epochs();
        harness.handle(KrakenSpotWsMessage::Reconnected);
        harness.handle(KrakenSpotWsMessage::SubscriptionAck {
            req_id: Some(500),
            symbol: Some(Ustr::from(BTC)),
            success: false,
            error: Some("Subscription with depth 10 not Found BTC/USD".to_string()),
        });
        harness.answer(1, None);
        harness.snapshot();
        assert_eq!(
            harness.events().len(),
            1,
            "the replayed snapshot is accepted"
        );

        harness.at(1);
        harness.answer(2, Some("Already subscribed"));
        let events = harness.events();
        assert_eq!(events.len(), 1);
        assert!(is_clear(&events[0]), "the spurious clear");

        harness.at(20);
        assert!(harness.check().is_empty());
        harness.at(21);
        assert_eq!(harness.check(), vec![harness.recovery()]);
    }

    #[rstest]
    fn test_l2_update_prunes_levels_beyond_subscribed_depth() {
        let mut harness = L2Harness::new(false);
        harness.subscribe(10, 1);
        harness.answer(1, None);

        let snapshot = KrakenWsBookData {
            symbol: Ustr::from("BTC/USD"),
            bids: Some(
                (0..10)
                    .map(|i| book_level(Decimal::from(100 - i), Decimal::ONE))
                    .collect(),
            ),
            asks: Some(
                (0..10)
                    .map(|i| book_level(Decimal::from(101 + i), Decimal::ONE))
                    .collect(),
            ),
            checksum: Some(0),
            timestamp: "2024-01-01T00:00:00Z".parse().unwrap(),
        };
        harness.handle(KrakenSpotWsMessage::Book {
            data: vec![snapshot],
            is_snapshot: true,
        });

        let DataEvent::Data(Data::BookDeltas(snapshot_deltas)) = harness
            .receiver
            .try_recv()
            .expect("expected snapshot deltas")
        else {
            panic!("expected snapshot deltas");
        };
        assert_eq!(snapshot_deltas.deltas.len(), 21);
        assert_eq!(snapshot_deltas.deltas[0].action, BookAction::Clear);
        assert!(RecordFlag::F_LAST.matches(snapshot_deltas.deltas.last().unwrap().flags));

        let bid_update = KrakenWsBookData {
            symbol: Ustr::from("BTC/USD"),
            bids: Some(vec![book_level(dec!(100.5), Decimal::ONE)]),
            asks: Some(vec![]),
            checksum: Some(0),
            timestamp: "2024-01-01T00:00:01Z".parse().unwrap(),
        };
        harness.handle(KrakenSpotWsMessage::Book {
            data: vec![bid_update],
            is_snapshot: false,
        });

        let DataEvent::Data(Data::BookDeltas(bid_update_deltas)) = harness
            .receiver
            .try_recv()
            .expect("expected bid update deltas")
        else {
            panic!("expected bid update deltas");
        };
        assert_eq!(bid_update_deltas.deltas.len(), 2);
        assert_eq!(bid_update_deltas.deltas[0].action, BookAction::Update);
        assert_eq!(bid_update_deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(bid_update_deltas.deltas[1].order.price, Price::from("91.0"));
        assert!(RecordFlag::F_LAST.matches(bid_update_deltas.deltas[1].flags));

        let ask_update = KrakenWsBookData {
            symbol: Ustr::from("BTC/USD"),
            bids: Some(vec![]),
            asks: Some(vec![book_level(dec!(100.6), Decimal::ONE)]),
            checksum: Some(0),
            timestamp: "2024-01-01T00:00:02Z".parse().unwrap(),
        };
        harness.handle(KrakenSpotWsMessage::Book {
            data: vec![ask_update],
            is_snapshot: false,
        });

        let DataEvent::Data(Data::BookDeltas(ask_update_deltas)) = harness
            .receiver
            .try_recv()
            .expect("expected ask update deltas")
        else {
            panic!("expected ask update deltas");
        };
        assert_eq!(ask_update_deltas.deltas.len(), 2);
        assert_eq!(ask_update_deltas.deltas[0].action, BookAction::Update);
        assert_eq!(ask_update_deltas.deltas[1].action, BookAction::Delete);
        assert_eq!(
            ask_update_deltas.deltas[1].order.price,
            Price::from("110.0")
        );
        assert!(RecordFlag::F_LAST.matches(ask_update_deltas.deltas[1].flags));

        let book = harness
            .l2_books
            .books
            .get(&make_instrument().id())
            .expect("expected shadow book");
        assert_eq!(book.bids(None).count(), 10);
        assert_eq!(book.asks(None).count(), 10);
        assert_eq!(book.best_bid_price(), Some(Price::from("100.5")));
        assert_eq!(book.best_ask_price(), Some(Price::from("100.6")));
        assert!(harness.receiver.try_recv().is_err());
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

    fn book_level(price: Decimal, qty: Decimal) -> KrakenWsBookLevel {
        KrakenWsBookLevel { price, qty }
    }
}
