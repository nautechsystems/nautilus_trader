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

//! Kraken Futures data client implementation.

use std::{
    future::Future,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clients::{DataClient, execute_bar_subscriptions, execute_instrument_subscriptions},
    live::{dst::time::Instant, get_data_event_sender, sender::EventSender},
    messages::{
        DataEvent,
        data::{
            BarsResponse, BookResponse, DataResponse, FundingRatesResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookSnapshot, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentStatus, SubscribeInstruments, SubscribeMarkPrices, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeInstrumentStatus,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    datetime::datetime_to_unix_nanos,
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    SocketControl,
    book::{BookSequenceOutcome, DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS},
    task::{TaskGroup, TaskSpawner},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas, QuoteTick},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use parking_lot::Mutex;
use rust_decimal_macros::dec;
use tokio_util::sync::CancellationToken;

use crate::{
    book::sync::{BookSyncTracker, spawn_recovery},
    common::{consts::KRAKEN_VENUE, lookup_instrument_in_snapshot},
    config::KrakenDataClientConfig,
    http::{
        KrakenFuturesHttpClient, futures::client::KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
    },
    websocket::{
        error::KrakenWsError,
        futures::{
            book::{BookCommand, BookMessage, BookStream, book_product},
            client::KrakenFuturesWebSocketClient,
            messages::KrakenFuturesWsMessage,
            parse::{
                parse_futures_ws_book_delta, parse_futures_ws_book_snapshot_deltas,
                parse_futures_ws_funding_rate, parse_futures_ws_index_price,
                parse_futures_ws_mark_price, parse_futures_ws_trade_tick,
            },
        },
    },
};

/// Kraken Futures data client.
///
/// Provides real-time market data from Kraken Futures markets.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenFuturesDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: KrakenDataClientConfig,
    http: KrakenFuturesHttpClient,
    ws: KrakenFuturesWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    quote_instruments: Arc<AtomicSet<InstrumentId>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    book_instruments: Arc<AtomicSet<InstrumentId>>,
    book_sync: Arc<Mutex<BookSyncTracker>>,
    next_book_generation: u64,
    data_sender: EventSender<DataEvent>,
}

impl KrakenFuturesDataClient {
    /// Creates a new [`KrakenFuturesDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration validation or client initialization fails.
    pub fn new(client_id: ClientId, config: KrakenDataClientConfig) -> anyhow::Result<Self> {
        config.validate()?;

        let session_tasks = TaskGroup::new();
        let cancellation_token = session_tasks.cancellation_token();
        let command_tasks = TaskGroup::new();
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let http = KrakenFuturesHttpClient::new(
            config.environment,
            config.base_url.clone(),
            config.timeout_secs,
            None,
            None,
            None,
            proxy_url.clone(),
            config
                .max_requests_per_second
                .unwrap_or(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND),
        )?;

        let ws = KrakenFuturesWebSocketClient::with_credentials(
            config.ws_public_url(),
            config.heartbeat_interval_secs,
            None,
            None,
            config.transport_backend,
            proxy_url,
        )
        .with_socket_control(SocketControl::new(
            client_id,
            Some(*KRAKEN_VENUE),
            "kraken-futures-data-streams",
        ));

        let snapshot_timeout = Duration::from_secs(config.book_snapshot_timeout_secs);

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            config,
            http,
            ws,
            is_connected: AtomicBool::new(false),
            cancellation_token,
            session_tasks,
            command_tasks,
            instruments: Arc::new(AtomicMap::new()),
            quote_instruments: Arc::new(AtomicSet::new()),
            last_quotes: Arc::new(DashMap::new()),
            book_instruments: Arc::new(AtomicSet::new()),
            book_sync: Arc::new(Mutex::new(BookSyncTracker::new(snapshot_timeout))),
            next_book_generation: 0,
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
            .request_instruments()
            .await
            .context("Failed to load futures instruments")?;

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
            log::warn!("Skipping Kraken Futures {context} after shutdown began: {e}");
        }
    }

    fn spawn_command<F>(&self, future: F)
    where
        F: Future<Output = ()> + Send + 'static,
    {
        if let Err(e) = self.command_tasks.spawn(future) {
            log::warn!("Skipping Kraken Futures data command after shutdown began: {e}");
        }
    }

    async fn finish_tasks(&self) -> anyhow::Result<()> {
        let (session_result, command_result) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
            self.command_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
        );
        session_result.context("failed to finish Kraken Futures data session tasks")?;
        command_result.context("failed to finish Kraken Futures data command tasks")?;
        Ok(())
    }

    async fn prepare_task_groups(&mut self) -> anyhow::Result<()> {
        if !self.session_tasks.is_open() || !self.command_tasks.is_open() {
            self.session_tasks.begin_shutdown();
            self.command_tasks.begin_shutdown();
            let _ = self.ws.close().await;
            self.finish_tasks().await?;
            self.session_tasks
                .start_generation()
                .context("failed to start Kraken Futures data session task generation")?;
            self.command_tasks
                .start_generation()
                .context("failed to start Kraken Futures data command task generation")?;
            self.cancellation_token = self.session_tasks.cancellation_token();
            self.ws = KrakenFuturesWebSocketClient::with_credentials(
                self.config.ws_public_url(),
                self.config.heartbeat_interval_secs,
                None,
                None,
                self.config.transport_backend,
                self.config
                    .proxy_url
                    .as_ref()
                    .map(|value| value.expose_secret().to_owned()),
            )
            .with_socket_control(SocketControl::new(
                self.client_id,
                Some(*KRAKEN_VENUE),
                "kraken-futures-data-streams",
            ));
        }
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.book_sync.lock().clear();
        let ws_result = self.ws.close().await;
        let tasks_result = self.finish_tasks().await;
        self.is_connected.store(false, Ordering::Release);
        tasks_result?;
        Ok(ws_result?)
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let mut rx = self
            .ws
            .take_book_rx()
            .context("Failed to take futures WebSocket output receiver")?;
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let quote_instruments = self.quote_instruments.clone();
        let last_quotes = self.last_quotes.clone();
        let book_instruments = self.book_instruments.clone();
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;
        let book_sync = self.book_sync.clone();
        let ws_client = self.ws.clone();
        let snapshot_timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);
        let spawner = self.session_tasks.spawner()?;

        let future = async move {
            let mut watchdog = tokio::time::interval(Duration::from_millis(100));
            watchdog.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
            let mut order_books: AHashMap<InstrumentId, OrderBook> = AHashMap::new();

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Futures message handler cancelled");
                        break;
                    }
                    _ = watchdog.tick() => {
                        let mut book_sync = book_sync.lock();
                        order_books.retain(|id, _| book_instruments.contains(id) || quote_instruments.contains(id));

                        for instrument_id in book_sync.expired(Instant::now()) {
                            Self::recover_book(&mut book_sync, instrument_id, &ws_client, &spawner, snapshot_timeout);
                        }
                    }
                    msg = rx.recv() => {
                        match msg {
                            Some(BookMessage::Data(ws_msg, stream)) => {
                                let mut book_sync = book_sync.lock();
                                if !Self::matches_book_generation(&book_sync, &ws_msg, stream.as_ref()) {
                                    continue;
                                }

                                let recover = Self::handle_ws_message(
                                    ws_msg,
                                    stream.as_ref(),
                                    &data_sender,
                                    &instruments,
                                    &quote_instruments,
                                    &book_instruments,
                                    &mut order_books,
                                    &last_quotes,
                                    &mut book_sync,
                                    clock,
                                );
                                if let Some(instrument_id) = recover {
                                    Self::recover_book(&mut book_sync, instrument_id, &ws_client, &spawner, snapshot_timeout);
                                }
                            }
                            Some(BookMessage::Rejected(symbol, generation)) => {
                                let id = InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *KRAKEN_VENUE);
                                let mut book_sync = book_sync.lock();
                                if book_sync.generation(id) == Some(generation) {
                                    Self::recover_book(&mut book_sync, id, &ws_client, &spawner, snapshot_timeout);
                                }
                            }
                            None => {
                                log::debug!("Futures WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }

            book_sync.lock().clear();
        };

        self.session_tasks
            .spawn(future)
            .context("failed to register Kraken Futures message handler")
    }

    fn matches_book_generation(
        books: &BookSyncTracker,
        msg: &KrakenFuturesWsMessage,
        stream: Option<&BookStream>,
    ) -> bool {
        let Some(symbol) = book_product(msg) else {
            return true;
        };

        let id = InstrumentId::new(Symbol::from_ustr_unchecked(symbol), *KRAKEN_VENUE);
        stream.is_some_and(|stream| books.generation(id) == Some(stream.generation))
    }

    fn start_book(&mut self, id: InstrumentId, replay: bool) -> Result<(), KrakenWsError> {
        let mut books = self.book_sync.lock();
        let generation = match books.generation(id) {
            Some(generation) if replay => generation,
            Some(_) => return Ok(()),
            None => {
                self.next_book_generation += 1;
                self.next_book_generation
            }
        };

        if replay || self.is_connected() {
            self.ws
                .send_book(BookCommand::Subscribe(id.symbol.inner(), generation))?;
        }

        books.track(id, generation, Instant::now());
        Ok(())
    }

    fn replay_held_books(&mut self) -> Result<(), KrakenWsError> {
        let deltas = self.book_instruments.load();
        let quotes = self.quote_instruments.load();
        let held: Vec<_> = deltas.union(&quotes).copied().collect();
        self.book_sync.lock().reset_on_reconnect(Instant::now());

        for id in held {
            self.start_book(id, true)?;
        }

        Ok(())
    }

    fn stop_book(&self, id: InstrumentId) {
        let mut books = self.book_sync.lock();
        if books.generation(id).is_some() {
            books.remove(id);

            if let Err(e) = self
                .ws
                .send_book(BookCommand::Unsubscribe(id.symbol.inner()))
            {
                log::debug!("Futures book unsubscribe for {id} has no active feed: {e}");
            }
        }
    }

    fn recover_book(
        books: &mut BookSyncTracker,
        instrument_id: InstrumentId,
        ws_client: &KrakenFuturesWebSocketClient,
        spawner: &TaskSpawner,
        timeout: Duration,
    ) {
        let Some(generation) = books.generation(instrument_id) else {
            return;
        };

        let Some(recovery) = books.get_mut(instrument_id).and_then(|book| book.claim()) else {
            return;
        };

        let ws_client = ws_client.clone();
        let replacement_recovery = Arc::clone(&recovery);

        // A short snapshot deadline still allows the venue to confirm the control transition.
        let write_timeout = if timeout.is_zero() {
            Duration::ZERO
        } else {
            timeout.max(Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS))
        };

        spawn_recovery(
            spawner,
            recovery,
            instrument_id,
            timeout,
            write_timeout,
            move |cancel, _| {
                let ws_client = ws_client.clone();
                let recovery = Arc::clone(&replacement_recovery);

                async move {
                    ws_client
                        .replace_book(instrument_id, generation, cancel, recovery)
                        .await
                }
            },
        );
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: KrakenFuturesWsMessage,
        stream: Option<&BookStream>,
        sender: &EventSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        quote_instruments: &Arc<AtomicSet<InstrumentId>>,
        book_instruments: &Arc<AtomicSet<InstrumentId>>,
        order_books: &mut AHashMap<InstrumentId, OrderBook>,
        last_quotes: &DashMap<InstrumentId, QuoteTick>,
        book_sync: &mut BookSyncTracker,
        clock: &'static AtomicTime,
    ) -> Option<InstrumentId> {
        let now = Instant::now();
        let ts_init = clock.get_time_ns();

        match msg {
            KrakenFuturesWsMessage::Ticker(ticker) => {
                let instruments = instruments.load();
                let Some(instrument) =
                    lookup_instrument_in_snapshot(&instruments, ticker.product_id.as_str())
                else {
                    log::warn!("No instrument for product_id: {}", ticker.product_id);
                    return None;
                };

                if let Some(mark) = parse_futures_ws_mark_price(&ticker, instrument, ts_init)
                    && let Err(e) = sender.send(DataEvent::Data(Data::MarkPrice(mark)))
                {
                    log::error!("Failed to send mark price: {e}");
                }

                if let Some(index) = parse_futures_ws_index_price(&ticker, instrument, ts_init)
                    && let Err(e) = sender.send(DataEvent::Data(Data::IndexPrice(index)))
                {
                    log::error!("Failed to send index price: {e}");
                }

                if let Some(funding) = parse_futures_ws_funding_rate(&ticker, instrument, ts_init)
                    && let Err(e) = sender.send(DataEvent::Data(Data::FundingRate(funding)))
                {
                    log::error!("Failed to send funding rate: {e}");
                }
            }
            KrakenFuturesWsMessage::Trade(trade) => {
                let instruments = instruments.load();
                let Some(instrument) =
                    lookup_instrument_in_snapshot(&instruments, trade.product_id.as_str())
                else {
                    log::warn!("No instrument for product_id: {}", trade.product_id);
                    return None;
                };

                match parse_futures_ws_trade_tick(&trade, instrument, ts_init) {
                    Ok(tick) => {
                        if let Err(e) = sender.send(DataEvent::Data(Data::Trade(tick))) {
                            log::error!("Failed to send trade: {e}");
                        }
                    }
                    Err(e) => log::error!("Failed to parse futures trade tick: {e}"),
                }
            }
            KrakenFuturesWsMessage::BookSnapshot(snapshot) => {
                let instruments = instruments.load();
                let Some(instrument) =
                    lookup_instrument_in_snapshot(&instruments, snapshot.product_id.as_str())
                else {
                    log::warn!("No instrument for product_id: {}", snapshot.product_id);
                    return None;
                };
                let instrument_id = instrument.id();
                let sync = book_sync.get_mut(instrument_id)?;
                if sync
                    .position()
                    .is_some_and(|previous| snapshot.seq < *previous)
                {
                    return None;
                }

                let sequence = u64::try_from(snapshot.seq).ok()?;
                let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);

                let built =
                    parse_futures_ws_book_snapshot_deltas(&snapshot, instrument, sequence, ts_init)
                        .and_then(|deltas| {
                            let deltas = OrderBookDeltas::new(instrument_id, deltas);
                            book.apply_deltas(&deltas)?;
                            Self::check_book_spread(&book)?;
                            Ok(deltas)
                        });

                let _active = stream?.lock_active()?;

                match built {
                    Ok(deltas) if sync.accept_snapshot(snapshot.seq, now) => {
                        Self::emit_book(
                            &book,
                            deltas,
                            sender,
                            quote_instruments,
                            book_instruments,
                            last_quotes,
                            ts_init,
                        );
                        order_books.insert(instrument_id, book);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!("Invalid Kraken Futures snapshot for {instrument_id}: {e}");
                        order_books.remove(&instrument_id);
                        sync.reject(KrakenWsError::InvalidMessage(e.to_string()));
                        return (sync.gap() == BookSequenceOutcome::Recover)
                            .then_some(instrument_id);
                    }
                }
            }
            KrakenFuturesWsMessage::BookDelta(delta) => {
                let _active = stream?.lock_active()?;
                let instruments = instruments.load();
                let instrument =
                    lookup_instrument_in_snapshot(&instruments, delta.product_id.as_str())?;
                let instrument_id = instrument.id();
                let sync = book_sync.get_mut(instrument_id)?;
                if let Some(&previous) = sync.position() {
                    if delta.seq <= previous {
                        return None;
                    }

                    if previous.checked_add(1) != Some(delta.seq) {
                        log::warn!(
                            "Kraken Futures book sequence gap for {instrument_id}: {previous} to {}",
                            delta.seq
                        );
                    } else if let Some(book) = order_books.get_mut(&instrument_id) {
                        let sequence = u64::try_from(delta.seq).ok()?;

                        let applied =
                            parse_futures_ws_book_delta(&delta, instrument, sequence, ts_init)
                                .and_then(|delta| {
                                    let deltas = OrderBookDeltas::new(instrument_id, vec![delta]);
                                    book.apply_deltas(&deltas)?;
                                    Self::check_book_spread(book)?;
                                    Ok(deltas)
                                });

                        match applied {
                            Ok(deltas) => {
                                sync.advance(delta.seq, now);
                                Self::emit_book(
                                    book,
                                    deltas,
                                    sender,
                                    quote_instruments,
                                    book_instruments,
                                    last_quotes,
                                    ts_init,
                                );
                                return None;
                            }
                            Err(e) => {
                                log::warn!("Invalid Kraken Futures delta for {instrument_id}: {e}");
                            }
                        }
                    }
                }

                order_books.remove(&instrument_id);
                last_quotes.remove(&instrument_id);
                return (sync.gap() == BookSequenceOutcome::Recover).then_some(instrument_id);
            }
            KrakenFuturesWsMessage::Reconnected => {
                order_books.clear();
                last_quotes.clear();
                book_sync.reset_on_reconnect(now);
                log::info!("Futures WebSocket reconnected");
            }
            KrakenFuturesWsMessage::OpenOrdersCancel(_)
            | KrakenFuturesWsMessage::OpenOrdersDelta(_)
            | KrakenFuturesWsMessage::FillsDelta(_)
            | KrakenFuturesWsMessage::Challenge(_) => {}
        }

        None
    }

    fn check_book_spread(book: &OrderBook) -> anyhow::Result<()> {
        // L2_MBP matches orders by price, so a level cannot contain multiple orders
        if let (Some(bid), Some(ask)) = (book.best_bid_price(), book.best_ask_price()) {
            anyhow::ensure!(bid <= ask, "Crossed book at {bid} / {ask}");
        }

        Ok(())
    }

    fn emit_book(
        book: &OrderBook,
        deltas: OrderBookDeltas,
        sender: &EventSender<DataEvent>,
        quote_instruments: &AtomicSet<InstrumentId>,
        book_instruments: &AtomicSet<InstrumentId>,
        last_quotes: &DashMap<InstrumentId, QuoteTick>,
        ts_init: UnixNanos,
    ) {
        let instrument_id = deltas.instrument_id;
        if quote_instruments.contains(&instrument_id) {
            Self::maybe_emit_quote(book, instrument_id, last_quotes, ts_init, sender);
        }

        if book_instruments.contains(&instrument_id)
            && let Err(e) = sender.send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
        {
            log::error!("Failed to send book deltas: {e}");
        }
    }

    fn maybe_emit_quote(
        book: &OrderBook,
        instrument_id: InstrumentId,
        last_quotes: &DashMap<InstrumentId, QuoteTick>,
        ts_init: UnixNanos,
        sender: &EventSender<DataEvent>,
    ) {
        let (Some(bid_price), Some(ask_price)) = (book.best_bid_price(), book.best_ask_price())
        else {
            return;
        };
        let (Some(bid_size), Some(ask_size)) = (book.best_bid_size(), book.best_ask_size()) else {
            return;
        };

        let bid = bid_price.as_decimal();
        let ask = ask_price.as_decimal();
        if bid > dec!(0) && (ask - bid) / bid > dec!(0.25) {
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

        if matches!(
            last_quotes.get(&instrument_id),
            Some(prev) if prev.has_same_bid_ask(&quote)
        ) {
            return;
        }

        last_quotes.insert(instrument_id, quote);

        if let Err(e) = sender.send(DataEvent::Data(Data::Quote(quote))) {
            log::error!("Failed to send quote: {e}");
        }
    }
}

#[async_trait(?Send)]
impl DataClient for KrakenFuturesDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*KRAKEN_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting Futures data client: client_id={}, environment={:?}",
            self.client_id,
            self.config.environment
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Futures data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.book_sync.lock().clear();
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Futures data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.book_sync.lock().clear();
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);

        self.instruments.store(ahash::AHashMap::new());

        self.last_quotes.clear();
        self.quote_instruments.store(ahash::AHashSet::new());
        self.book_instruments.store(ahash::AHashSet::new());

        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing Futures data client: {}", self.client_id);
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
        self.last_quotes.clear();

        let instruments = self.load_instruments().await?;

        let session_result = async {
            self.ws
                .connect_books()
                .await
                .context("Failed to connect futures WebSocket")?;
            self.ws
                .wait_until_active(10.0)
                .await
                .context("Futures WebSocket failed to become active")?;

            self.replay_held_books()?;
            self.spawn_message_handler()?;

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Kraken Futures data startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        for instrument in instruments {
            if let Err(e) = self.data_sender.send(DataEvent::Data(instrument.into())) {
                log::error!("Failed to send instrument: {e}");
            }
        }

        self.is_connected.store(true, Ordering::Release);
        log::info!(
            "Connected: client_id={}, product_type=Futures",
            self.client_id
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.teardown_partial_connect().await?;

        self.last_quotes.clear();
        self.quote_instruments.store(ahash::AHashSet::new());
        self.book_instruments.store(ahash::AHashSet::new());
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
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let cmd = cmd.with_instrument_ids(instrument_id);
            if cmd.book_type != BookType::L2_MBP {
                log::warn!(
                    "Book type {:?} not supported by Kraken, skipping subscription",
                    cmd.book_type
                );
                return Ok(());
            }

            let mut sync = self.book_sync.lock();
            let was_quotes_only = !self.book_instruments.contains(&instrument_id)
                && sync
                    .get_mut(instrument_id)
                    .is_some_and(|book| book.position().is_some());

            if was_quotes_only {
                let spawner = self.session_tasks.spawner()?;
                let timeout = Duration::from_secs(self.config.book_snapshot_timeout_secs);
                Self::recover_book(&mut sync, instrument_id, &self.ws, &spawner, timeout);
            }

            let held = self.book_instruments.contains(&instrument_id);
            self.book_instruments.insert(instrument_id);
            drop(sync);

            if let Err(e) = self.start_book(instrument_id, false) {
                if !held {
                    self.book_instruments.remove(&instrument_id);
                }

                return Err(e.into());
            }

            Ok(())
        })
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let held = self.quote_instruments.contains(&instrument_id);
            self.last_quotes.remove(&instrument_id);
            self.quote_instruments.insert(instrument_id);
            if let Err(e) = self.start_book(instrument_id, false) {
                if !held {
                    self.quote_instruments.remove(&instrument_id);
                }

                return Err(e.into());
            }

            Ok(())
        })
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
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
        })
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_mark_price(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "subscribe mark price",
            );

            Ok(())
        })
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_index_price(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "subscribe index price",
            );

            Ok(())
        })
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.subscribe_funding_rate(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "subscribe funding rate",
            );

            Ok(())
        })
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        execute_bar_subscriptions(&cmd.bar_types, |bar_type| {
            log::warn!(
                "Cannot subscribe to {bar_type} bars: Kraken Futures does not support EXTERNAL bar streaming",
            );
            Ok(())
        })
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            log::debug!(
                "subscribe_instrument_status: {instrument_id} (status changes detected via periodic instrument polling)",
            );
            Ok(())
        })
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            self.book_instruments.remove(&instrument_id);
            if !self.quote_instruments.contains(&instrument_id) {
                self.stop_book(instrument_id);
            }

            Ok(())
        })
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            self.quote_instruments.remove(&instrument_id);
            if !self.book_instruments.contains(&instrument_id) {
                self.stop_book(instrument_id);
            }

            self.last_quotes.remove(&instrument_id);

            Ok(())
        })
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
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
        })
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_mark_price(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "unsubscribe mark price",
            );

            Ok(())
        })
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_index_price(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "unsubscribe index price",
            );

            Ok(())
        })
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let ws = self.ws.clone();
            self.spawn_ws(
                async move {
                    ws.unsubscribe_funding_rate(instrument_id)
                        .await
                        .map_err(|e| anyhow::anyhow!("{e}"))
                },
                "unsubscribe funding rate",
            );

            Ok(())
        })
    }

    fn unsubscribe_bars(&mut self, _cmd: &UnsubscribeBars) -> anyhow::Result<()> {
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
            match http.request_instruments().await {
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
        let instrument_id = request.single_instrument_id()?;

        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_command(async move {
            match http.request_instruments().await {
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
        let instrument_id = request.single_instrument_id()?;

        let http = self.http.clone();
        let sender = self.data_sender.clone();
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
        let bar_type = request.single_bar_type()?;

        let http = self.http.clone();
        let sender = self.data_sender.clone();
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
        let instrument_id = request.single_instrument_id()?;

        let http = self.http.clone();
        let sender = self.data_sender.clone();
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

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;

        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get());
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let params = request.params;
        let clock = self.clock;

        self.spawn_command(async move {
            match http
                .request_funding_rates(instrument_id, start, end, limit)
                .await
            {
                Ok(rates) => {
                    let response = DataResponse::FundingRates(FundingRatesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        rates,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send funding rates response: {e}");
                    }
                }
                Err(e) => log::error!("Funding rates request failed: {e:?}"),
            }
        });

        Ok(())
    }
}

#[cfg(test)]
#[path = "../../benches"]
pub(crate) mod tests {
    // These routines are exercised by Criterion, not the unit-test harness
    #[allow(dead_code)]
    #[path = "quotes/mod.rs"]
    pub(crate) mod benchmarks;

    use futures_util::{SinkExt, StreamExt};
    use nautilus_common::{live::runner::set_data_event_sender, messages::DataEvent};
    use nautilus_core::UUID4;
    use nautilus_live::book::recovery::BookRecoveryOutcome;
    use nautilus_model::{
        enums::BookAction,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use tokio_tungstenite::tungstenite::Message;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            consts::KRAKEN_CLIENT_ID,
            enums::{KrakenOrderSide, KrakenProductType},
            parse::parse_futures_instrument,
        },
        config::KrakenDataClientConfig,
        http::models::FuturesInstrumentsResponse,
        websocket::futures::messages::{
            KrakenFuturesBookDelta, KrakenFuturesBookSnapshot, KrakenFuturesFeed,
        },
    };

    fn setup_test_env() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_futures_data_client_new() {
        setup_test_env();
        let config = KrakenDataClientConfig {
            product_type: KrakenProductType::Futures,
            ..Default::default()
        };
        let client = KrakenFuturesDataClient::new(*KRAKEN_CLIENT_ID, config);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.client_id(), *KRAKEN_CLIENT_ID);
        assert_eq!(client.venue(), Some(*KRAKEN_VENUE));
        assert!(!client.is_connected());
        assert!(client.is_disconnected());
        assert!(client.instruments().is_empty());
    }

    #[rstest]
    fn test_futures_data_client_start_stop() {
        setup_test_env();
        let config = KrakenDataClientConfig {
            product_type: KrakenProductType::Futures,
            ..Default::default()
        };
        let mut client = KrakenFuturesDataClient::new(*KRAKEN_CLIENT_ID, config).unwrap();

        assert!(client.start().is_ok());
        assert!(client.stop().is_ok());
        assert!(client.is_disconnected());
    }

    #[rstest]
    #[case::initial(false, Duration::ZERO)]
    #[case::replay(true, Duration::ZERO)]
    #[case::delayed_confirmation(false, Duration::from_secs(DEFAULT_BOOK_SNAPSHOT_TIMEOUT_SECS + 1))]
    #[tokio::test]
    async fn test_preconnect_futures_book_recovers_a_rejected_subscribe(
        #[case] reject_replay: bool,
        #[case] confirmation_delay: Duration,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let config = KrakenDataClientConfig {
            product_type: KrakenProductType::Futures,
            ws_public_url: Some(format!("ws://{}", listener.local_addr().unwrap())),
            book_snapshot_timeout_secs: 0,
            ..Default::default()
        };

        let mut client = KrakenFuturesDataClient::new(*KRAKEN_CLIENT_ID, config).unwrap();
        let instrument = futures_instrument();
        let id = instrument.id();
        client.instruments.insert(id, instrument);

        let subscribe = || {
            SubscribeQuotes::new(
                id,
                Some(*KRAKEN_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };

        client.subscribe_quotes(subscribe()).unwrap();
        client.subscribe_quotes(subscribe()).unwrap();

        let server = tokio::spawn(async move {
            for connection in 0..=usize::from(reject_replay) {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                for reply in ["subscribed_failed", "subscribed"] {
                    let request = socket.next().await.unwrap().unwrap();
                    let request: serde_json::Value =
                        serde_json::from_str(request.to_text().unwrap()).unwrap();
                    assert_eq!(
                        request,
                        serde_json::json!({"event": "subscribe", "feed": "book", "product_ids": ["PI_XBTUSD"]})
                    );

                    if reply == "subscribed" && !confirmation_delay.is_zero() {
                        tokio::time::sleep(confirmation_delay).await;
                    }

                    let response = serde_json::json!({"event": reply, "feed": "book", "product_ids": ["PI_XBTUSD"]});
                    socket
                        .send(Message::text(response.to_string()))
                        .await
                        .unwrap();
                }

                if reject_replay && connection == 0 {
                    drop(socket);
                    continue;
                }

                socket
                    .send(Message::text(include_str!(
                        "../../test_data/ws_futures_book_snapshot.json"
                    )))
                    .await
                    .unwrap();
                let _ = socket.next().await;
            }
        });

        client.ws.connect_books().await.unwrap();
        client.replay_held_books().unwrap();
        client.spawn_message_handler().unwrap();
        let event = tokio::time::timeout(Duration::from_secs(5) + confirmation_delay, rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(matches!(event, DataEvent::Data(Data::Quote(quote)) if quote.instrument_id == id));
        client.disconnect().await.unwrap();
        server.await.unwrap();
    }

    #[rstest]
    fn test_queued_snapshot_cannot_satisfy_the_next_generation() {
        let instrument = futures_instrument();
        let id = instrument.id();
        let mut tracker = BookSyncTracker::default();
        let timestamp = Instant::now();
        tracker.track(id, 1, timestamp);
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_futures_book_snapshot.json"
        ))
        .unwrap();
        let message = KrakenFuturesWsMessage::BookSnapshot(snapshot);

        let queued = BookStream::new(1);

        assert!(KrakenFuturesDataClient::matches_book_generation(
            &tracker,
            &message,
            Some(&queued)
        ));

        tracker.remove(id);
        tracker.track(id, 2, timestamp);
        assert!(!KrakenFuturesDataClient::matches_book_generation(
            &tracker,
            &message,
            Some(&queued)
        ));

        let current = BookStream::new(2);

        assert!(KrakenFuturesDataClient::matches_book_generation(
            &tracker,
            &message,
            Some(&current)
        ));
        assert!(!KrakenFuturesDataClient::matches_book_generation(
            &tracker, &message, None
        ));
    }

    #[rstest]
    fn test_gap_suppresses_deltas_until_replacement_snapshot() {
        let instrument = futures_instrument();
        let id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(id, instrument);
        let quote_instruments = Arc::new(AtomicSet::new());
        let book_instruments = Arc::new(AtomicSet::new());
        book_instruments.insert(id);
        let mut book_sync = BookSyncTracker::default();
        book_sync.track(id, 0, Instant::now());
        let mut order_books = AHashMap::new();
        let last_quotes = DashMap::new();
        let clock = get_atomic_clock_realtime();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        let sender = sender.into();
        let mut snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_futures_book_snapshot.json"
        ))
        .unwrap();
        let mut delta: KrakenFuturesBookDelta =
            serde_json::from_str(include_str!("../../test_data/ws_futures_book_delta.json"))
                .unwrap();

        let original = BookStream::new(0);

        {
            let mut process = |message| {
                KrakenFuturesDataClient::handle_ws_message(
                    message,
                    Some(&original),
                    &sender,
                    &instruments,
                    &quote_instruments,
                    &book_instruments,
                    &mut order_books,
                    &last_quotes,
                    &mut book_sync,
                    clock,
                )
            };

            assert_eq!(
                process(KrakenFuturesWsMessage::BookSnapshot(snapshot.clone())),
                None
            );
            delta.seq = snapshot.seq;
            assert_eq!(
                process(KrakenFuturesWsMessage::BookDelta(delta.clone())),
                None
            );
            delta.seq += 2;
            assert_eq!(
                process(KrakenFuturesWsMessage::BookDelta(delta.clone())),
                Some(id)
            );
        }

        let recovery = book_sync.get_mut(id).unwrap().claim().unwrap();
        assert!(recovery.begin_replacement());
        original.retire();
        let replacement_stream = BookStream::new(0);

        let mut process = |message, stream| {
            KrakenFuturesDataClient::handle_ws_message(
                message,
                Some(stream),
                &sender,
                &instruments,
                &quote_instruments,
                &book_instruments,
                &mut order_books,
                &last_quotes,
                &mut book_sync,
                clock,
            )
        };

        assert_eq!(
            process(KrakenFuturesWsMessage::BookDelta(delta.clone()), &original),
            None
        );
        snapshot.seq = delta.seq;
        let mut invalid = snapshot.clone();
        invalid.bids[0].price = invalid.asks[0].price + dec!(100);
        snapshot.bids.clear();
        snapshot.asks.clear();
        assert_eq!(
            process(
                KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()),
                &replacement_stream
            ),
            None
        );
        assert!(!recovery.is_accepted());

        recovery.gate.open();
        assert_eq!(
            process(
                KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()),
                &original
            ),
            None
        );
        assert_eq!(
            process(KrakenFuturesWsMessage::BookSnapshot(invalid), &original),
            None
        );
        assert!(!recovery.is_accepted());
        assert!(matches!(
            *recovery.outcome.borrow(),
            BookRecoveryOutcome::Pending
        ));
        assert_eq!(
            process(
                KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()),
                &replacement_stream
            ),
            None
        );
        assert!(recovery.is_accepted());

        let DataEvent::Data(Data::BookDeltas(initial)) = receiver.try_recv().unwrap() else {
            panic!("Expected initial book");
        };

        let DataEvent::Data(Data::BookDeltas(replacement)) = receiver.try_recv().unwrap() else {
            panic!("Expected replacement book");
        };

        assert_eq!(initial.sequence, 326_072_249);
        assert_eq!(replacement.sequence, 326_072_251);
        assert_eq!(replacement.deltas.len(), 1);
        assert_eq!(replacement.deltas[0].action, BookAction::Clear);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    #[case::bid_price(KrakenOrderSide::Buy, dec!(34893.0), dec!(6385))]
    #[case::ask_price(KrakenOrderSide::Sell, dec!(34911.0), dec!(20598))]
    #[case::bid_size(KrakenOrderSide::Buy, dec!(34892.5), dec!(7000))]
    #[case::ask_size(KrakenOrderSide::Sell, dec!(34911.5), dec!(21000))]
    fn test_handle_ws_message_emits_quote_only_on_top_of_book_change(
        #[case] side: KrakenOrderSide,
        #[case] price: Decimal,
        #[case] qty: Decimal,
    ) {
        let instrument = futures_instrument();
        let instrument_id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument_id, instrument.clone());
        let quote_instruments = Arc::new(AtomicSet::new());
        quote_instruments.insert(instrument_id);
        let book_instruments = Arc::new(AtomicSet::new());
        book_instruments.insert(instrument_id);
        let mut book_sync = BookSyncTracker::default();
        book_sync.track(instrument_id, 0, Instant::now());
        let mut order_books = AHashMap::new();
        let last_quotes = DashMap::new();
        let ts1 = UnixNanos::new(1_700_000_000_000_000_000);
        let ts2 = UnixNanos::new(1_700_000_000_100_000_000);
        let ts3 = UnixNanos::new(1_700_000_000_200_000_000);
        let ts4 = UnixNanos::new(1_700_000_000_300_000_000);
        let clock: &'static AtomicTime = Box::leak(Box::new(AtomicTime::new(false, ts1)));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<DataEvent> = tx.into();
        let json = include_str!("../../test_data/ws_futures_book_snapshot.json");
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(json).unwrap();

        let delta = |side, price, qty, seq, timestamp| {
            KrakenFuturesWsMessage::BookDelta(KrakenFuturesBookDelta {
                feed: KrakenFuturesFeed::Book,
                product_id: Ustr::from("PI_XBTUSD"),
                side,
                seq,
                price,
                qty,
                timestamp,
            })
        };

        // The first delta changes depth only; the second changes one top-of-book field
        let messages = [
            (ts1, KrakenFuturesWsMessage::BookSnapshot(snapshot)),
            (
                ts2,
                delta(
                    KrakenOrderSide::Buy,
                    dec!(34892),
                    dec!(12000),
                    326_072_250,
                    1_612_269_825_900,
                ),
            ),
            (ts3, delta(side, price, qty, 326_072_251, 1_612_269_826_000)),
            (ts4, delta(side, price, qty, 326_072_252, 1_612_269_826_100)),
        ];
        let mut expected_deltas = Vec::new();

        for (ts_init, msg) in &messages {
            let deltas = match msg {
                KrakenFuturesWsMessage::BookSnapshot(snapshot) => {
                    parse_futures_ws_book_snapshot_deltas(
                        snapshot,
                        &instrument,
                        snapshot.seq as u64,
                        *ts_init,
                    )
                    .unwrap()
                }

                KrakenFuturesWsMessage::BookDelta(delta) => {
                    let delta =
                        parse_futures_ws_book_delta(delta, &instrument, delta.seq as u64, *ts_init)
                            .unwrap();
                    vec![delta]
                }

                other => panic!("Unexpected message: {other:?}"),
            };

            expected_deltas.push(OrderBookDeltas::new(instrument_id, deltas));
        }

        for (ts_init, msg) in messages {
            clock.set_time(ts_init);
            KrakenFuturesDataClient::handle_ws_message(
                msg,
                Some(&BookStream::new(0)),
                &sender,
                &instruments,
                &quote_instruments,
                &book_instruments,
                &mut order_books,
                &last_quotes,
                &mut book_sync,
                clock,
            );
        }

        let mut quotes = Vec::new();
        let mut book_deltas = Vec::new();

        while let Ok(event) = rx.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => quotes.push(quote),
                DataEvent::Data(Data::BookDeltas(deltas)) => book_deltas.push(*deltas),
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        let initial = QuoteTick::new(
            instrument_id,
            Price::from("34892.5"),
            Price::from("34911.5"),
            Quantity::from("6385"),
            Quantity::from("20598"),
            ts1,
            ts1,
        );

        let mut changed = QuoteTick {
            ts_event: ts3,
            ts_init: ts3,
            ..initial
        };

        match side {
            KrakenOrderSide::Buy => {
                changed.bid_price = Price::from_decimal_dp(price, 1).unwrap();
                changed.bid_size = Quantity::from_decimal_dp(qty, 0).unwrap();
            }

            KrakenOrderSide::Sell => {
                changed.ask_price = Price::from_decimal_dp(price, 1).unwrap();
                changed.ask_size = Quantity::from_decimal_dp(qty, 0).unwrap();
            }
        }

        assert_eq!(quotes, vec![initial, changed]);

        // PartialEq omits batch content and nested order values
        assert_eq!(
            serde_json::to_value(&book_deltas).unwrap(),
            serde_json::to_value(&expected_deltas).unwrap(),
        );
    }

    #[rstest]
    #[case::spread_boundary(dec!(125.0), true)]
    #[case::wide_spread(dec!(125.5), false)]
    fn test_quote_spread_filter_preserves_cached_quote_and_book_delivery(
        #[case] ask_price: Decimal,
        #[case] accepted: bool,
    ) {
        let instrument = futures_instrument();
        let instrument_id = instrument.id();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(instrument_id, instrument.clone());
        let quote_instruments = Arc::new(AtomicSet::new());
        quote_instruments.insert(instrument_id);
        let book_instruments = Arc::new(AtomicSet::new());
        book_instruments.insert(instrument_id);
        let mut book_sync = BookSyncTracker::default();
        book_sync.track(instrument_id, 0, Instant::now());
        let mut order_books = AHashMap::new();
        let last_quotes = DashMap::new();
        let clock = Box::leak(Box::new(AtomicTime::new(false, UnixNanos::new(10))));
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let sender = tx.into();
        let mut initial: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_futures_book_snapshot.json"
        ))
        .unwrap();
        initial.bids[0].price = dec!(100.0);
        initial.bids[1].price = dec!(99.5);
        initial.asks[0].price = dec!(101.0);
        initial.asks[1].price = dec!(101.5);
        let mut filtered = initial.clone();
        filtered.asks[0].price = ask_price;
        filtered.asks[1].price = ask_price + dec!(0.5);
        let mut changed = initial.clone();
        changed.bids[0].qty = dec!(7000);
        let mut expected_deltas = Vec::new();

        for (snapshot, timestamp) in [(initial, 10), (filtered, 20), (changed, 30)] {
            let timestamp = UnixNanos::new(timestamp);
            let deltas = parse_futures_ws_book_snapshot_deltas(
                &snapshot,
                &instrument,
                snapshot.seq as u64,
                timestamp,
            )
            .unwrap();
            expected_deltas.push(OrderBookDeltas::new(instrument_id, deltas));
            clock.set_time(timestamp);
            KrakenFuturesDataClient::handle_ws_message(
                KrakenFuturesWsMessage::BookSnapshot(snapshot),
                Some(&BookStream::new(0)),
                &sender,
                &instruments,
                &quote_instruments,
                &book_instruments,
                &mut order_books,
                &last_quotes,
                &mut book_sync,
                clock,
            );
        }

        let mut quotes = Vec::new();
        let mut deltas = Vec::new();

        while let Ok(event) = rx.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => quotes.push(quote),
                DataEvent::Data(Data::BookDeltas(batch)) => deltas.push(*batch),
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        let initial = QuoteTick::new(
            instrument_id,
            Price::from("100.0"),
            Price::from("101.0"),
            Quantity::from("6385"),
            Quantity::from("20598"),
            UnixNanos::new(10),
            UnixNanos::new(10),
        );
        let mut expected_quotes = vec![initial];
        if accepted {
            expected_quotes.push(QuoteTick {
                ask_price: Price::from_decimal_dp(ask_price, 1).unwrap(),
                ts_event: UnixNanos::new(20),
                ts_init: UnixNanos::new(20),
                ..initial
            });
        }

        let changed = QuoteTick {
            bid_size: Quantity::from("7000"),
            ts_event: UnixNanos::new(30),
            ts_init: UnixNanos::new(30),
            ..initial
        };

        expected_quotes.push(changed);
        assert_eq!(quotes, expected_quotes);
        assert_eq!(
            last_quotes.get(&instrument_id).map(|quote| *quote),
            Some(changed)
        );
        assert_eq!(
            serde_json::to_value(&deltas).unwrap(),
            serde_json::to_value(&expected_deltas).unwrap()
        );
    }

    #[rstest]
    #[case::resubscribe(false, false)]
    #[case::resubscribe_after_late_quote(false, true)]
    #[case::reconnect(true, false)]
    #[tokio::test]
    async fn test_quote_subscription_emits_unchanged_initial_quote(
        #[case] reconnect: bool,
        #[case] late_quote: bool,
    ) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);

        let mut client = KrakenFuturesDataClient::new(
            *KRAKEN_CLIENT_ID,
            KrakenDataClientConfig {
                product_type: KrakenProductType::Futures,
                ..Default::default()
            },
        )
        .unwrap();

        let instrument = futures_instrument();
        let instrument_id = instrument.id();
        client.instruments.insert(instrument_id, instrument);
        let ts1 = UnixNanos::new(1_700_000_000_000_000_000);
        let ts2 = UnixNanos::new(1_700_000_000_100_000_000);
        let ts3 = UnixNanos::new(1_700_000_000_200_000_000);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts1)));
        let mut order_books = AHashMap::new();
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_futures_book_snapshot.json"
        ))
        .unwrap();

        let initial = QuoteTick::new(
            instrument_id,
            Price::from("34892.5"),
            Price::from("34911.5"),
            Quantity::from("6385"),
            Quantity::from("20598"),
            ts1,
            ts1,
        );

        let subscribe = || {
            SubscribeQuotes::new(
                instrument_id,
                Some(*KRAKEN_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };

        let mut handle = |client: &KrakenFuturesDataClient, msg| {
            KrakenFuturesDataClient::handle_ws_message(
                msg,
                Some(&BookStream::new(0)),
                &client.data_sender,
                &client.instruments,
                &client.quote_instruments,
                &client.book_instruments,
                &mut order_books,
                &client.last_quotes,
                &mut client.book_sync.lock(),
                clock,
            );
        };

        client.subscribe_quotes(subscribe()).unwrap();
        handle(
            &client,
            KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()),
        );

        if reconnect {
            handle(&client, KrakenFuturesWsMessage::Reconnected);
        } else {
            client
                .unsubscribe_quotes(&UnsubscribeQuotes::new(
                    instrument_id,
                    Some(*KRAKEN_CLIENT_ID),
                    None,
                    UUID4::new(),
                    UnixNanos::default(),
                    None,
                    None,
                ))
                .unwrap();
            clock.set_time(ts2);
            handle(
                &client,
                KrakenFuturesWsMessage::BookSnapshot(snapshot.clone()),
            );

            if late_quote {
                client.last_quotes.insert(instrument_id, initial);
            }

            client.subscribe_quotes(subscribe()).unwrap();
        }

        clock.set_time(ts3);
        handle(&client, KrakenFuturesWsMessage::BookSnapshot(snapshot));
        let mut quotes = Vec::new();

        while let Ok(event) = rx.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => quotes.push(quote),
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        assert_eq!(
            quotes,
            vec![
                initial,
                QuoteTick {
                    ts_event: ts3,
                    ts_init: ts3,
                    ..initial
                }
            ]
        );
    }

    #[rstest]
    #[case::joining_book_subscription(false)]
    #[case::returning_to_book_subscription(true)]
    #[tokio::test]
    async fn test_quote_subscription_uses_current_book(#[case] initial_quotes: bool) {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(tx);

        let mut client = KrakenFuturesDataClient::new(
            *KRAKEN_CLIENT_ID,
            KrakenDataClientConfig {
                product_type: KrakenProductType::Futures,
                ..Default::default()
            },
        )
        .unwrap();

        let instrument = futures_instrument();
        let instrument_id = instrument.id();
        client.instruments.insert(instrument_id, instrument.clone());
        client.book_instruments.insert(instrument_id);
        client
            .book_sync
            .lock()
            .track(instrument_id, 0, Instant::now());
        let ts1 = UnixNanos::new(1_700_000_000_000_000_000);
        let ts2 = UnixNanos::new(1_700_000_000_100_000_000);
        let ts3 = UnixNanos::new(1_700_000_000_200_000_000);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts1)));
        let mut order_books = AHashMap::new();
        let snapshot: KrakenFuturesBookSnapshot = serde_json::from_str(include_str!(
            "../../test_data/ws_futures_book_snapshot.json"
        ))
        .unwrap();

        let messages = [
            KrakenFuturesWsMessage::BookSnapshot(snapshot),
            KrakenFuturesWsMessage::BookDelta(KrakenFuturesBookDelta {
                feed: KrakenFuturesFeed::Book,
                product_id: Ustr::from("PI_XBTUSD"),
                side: KrakenOrderSide::Buy,
                seq: 326_072_250,
                price: dec!(34892.5),
                qty: dec!(7000),
                timestamp: 1_612_269_825_900,
            }),
            KrakenFuturesWsMessage::BookDelta(KrakenFuturesBookDelta {
                feed: KrakenFuturesFeed::Book,
                product_id: Ustr::from("PI_XBTUSD"),
                side: KrakenOrderSide::Buy,
                seq: 326_072_251,
                price: dec!(34892),
                qty: dec!(12000),
                timestamp: 1_612_269_826_000,
            }),
        ];

        let mut expected_deltas = Vec::new();

        for (message, ts) in messages.iter().zip([ts1, ts2, ts3]) {
            let deltas = match message {
                KrakenFuturesWsMessage::BookSnapshot(snapshot) => {
                    parse_futures_ws_book_snapshot_deltas(
                        snapshot,
                        &instrument,
                        snapshot.seq as u64,
                        ts,
                    )
                    .unwrap()
                }

                KrakenFuturesWsMessage::BookDelta(delta) => vec![
                    parse_futures_ws_book_delta(delta, &instrument, delta.seq as u64, ts).unwrap(),
                ],
                other => panic!("Unexpected message: {other:?}"),
            };

            expected_deltas.push(OrderBookDeltas::new(instrument_id, deltas));
        }

        let subscribe = || {
            SubscribeQuotes::new(
                instrument_id,
                Some(*KRAKEN_CLIENT_ID),
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            )
        };

        if initial_quotes {
            client.subscribe_quotes(subscribe()).unwrap();
        }

        for (index, message) in messages.into_iter().enumerate() {
            if index == 1 && initial_quotes {
                client
                    .unsubscribe_quotes(&UnsubscribeQuotes::new(
                        instrument_id,
                        Some(*KRAKEN_CLIENT_ID),
                        None,
                        UUID4::new(),
                        UnixNanos::default(),
                        None,
                        None,
                    ))
                    .unwrap();
            }

            if index == 2 {
                client.subscribe_quotes(subscribe()).unwrap();
            }

            clock.set_time([ts1, ts2, ts3][index]);
            KrakenFuturesDataClient::handle_ws_message(
                message,
                Some(&BookStream::new(0)),
                &client.data_sender,
                &client.instruments,
                &client.quote_instruments,
                &client.book_instruments,
                &mut order_books,
                &client.last_quotes,
                &mut client.book_sync.lock(),
                clock,
            );
        }

        let mut quotes = Vec::new();
        let mut deltas = Vec::new();

        while let Ok(event) = rx.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(quote)) => quotes.push(quote),
                DataEvent::Data(Data::BookDeltas(batch)) => deltas.push(*batch),
                other => panic!("Unexpected event: {other:?}"),
            }
        }

        let initial = QuoteTick::new(
            instrument_id,
            Price::from("34892.5"),
            Price::from("34911.5"),
            Quantity::from("6385"),
            Quantity::from("20598"),
            ts1,
            ts1,
        );

        let current = QuoteTick {
            bid_size: Quantity::from("7000"),
            ts_event: ts3,
            ts_init: ts3,
            ..initial
        };

        let expected_quotes = if initial_quotes {
            vec![initial, current]
        } else {
            vec![current]
        };

        assert_eq!(quotes, expected_quotes);
        assert_eq!(
            serde_json::to_value(&deltas).unwrap(),
            serde_json::to_value(&expected_deltas).unwrap()
        );
    }

    fn futures_instrument() -> InstrumentAny {
        let response: FuturesInstrumentsResponse = serde_json::from_str(include_str!(
            "../../test_data/http_futures_instruments.json"
        ))
        .unwrap();
        parse_futures_instrument(
            &response.instruments[0],
            UnixNanos::default(),
            UnixNanos::default(),
        )
        .unwrap()
    }
}
