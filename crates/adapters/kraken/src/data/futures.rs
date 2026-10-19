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
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{get_data_event_sender, sender::EventSender},
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
use nautilus_live::{SocketControl, task::TaskGroup};
use nautilus_model::{
    data::{Data, OrderBookDeltas, QuoteTick},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use rust_decimal_macros::dec;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{consts::KRAKEN_VENUE, lookup_instrument_in_snapshot},
    config::KrakenDataClientConfig,
    http::{
        KrakenFuturesHttpClient, futures::client::KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
    },
    websocket::futures::{
        client::KrakenFuturesWebSocketClient,
        messages::KrakenFuturesWsMessage,
        parse::{
            parse_futures_ws_book_delta, parse_futures_ws_book_snapshot_deltas,
            parse_futures_ws_funding_rate, parse_futures_ws_index_price,
            parse_futures_ws_mark_price, parse_futures_ws_trade_tick,
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
    data_sender: EventSender<DataEvent>,
}

impl KrakenFuturesDataClient {
    /// Creates a new [`KrakenFuturesDataClient`] instance.
    pub fn new(client_id: ClientId, config: KrakenDataClientConfig) -> anyhow::Result<Self> {
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
        let ws_result = self.ws.close().await;
        let tasks_result = self.finish_tasks().await;
        self.is_connected.store(false, Ordering::Release);
        tasks_result?;
        Ok(ws_result?)
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let mut rx = self
            .ws
            .take_output_rx()
            .context("Failed to take futures WebSocket output receiver")?;
        let data_sender = self.data_sender.clone();
        let instruments = self.instruments.clone();
        let quote_instruments = self.quote_instruments.clone();
        let last_quotes = self.last_quotes.clone();
        let book_instruments = self.book_instruments.clone();
        let book_sequence = Arc::new(AtomicU64::new(0));
        let cancellation_token = self.cancellation_token.clone();
        let clock = self.clock;

        let future = async move {
            let mut order_books: AHashMap<InstrumentId, OrderBook> = AHashMap::new();

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Futures message handler cancelled");
                        break;
                    }
                    msg = rx.recv() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(
                                    ws_msg,
                                    &data_sender,
                                    &instruments,
                                    &quote_instruments,
                                    &book_instruments,
                                    &mut order_books,
                                    &last_quotes,
                                    &book_sequence,
                                    clock,
                                );
                            }
                            None => {
                                log::debug!("Futures WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        };

        self.session_tasks
            .spawn(future)
            .context("failed to register Kraken Futures message handler")
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: KrakenFuturesWsMessage,
        sender: &EventSender<DataEvent>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        quote_instruments: &Arc<AtomicSet<InstrumentId>>,
        book_instruments: &Arc<AtomicSet<InstrumentId>>,
        order_books: &mut AHashMap<InstrumentId, OrderBook>,
        last_quotes: &DashMap<InstrumentId, QuoteTick>,
        book_sequence: &Arc<AtomicU64>,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();

        match msg {
            KrakenFuturesWsMessage::Ticker(ticker) => {
                let instruments = instruments.load();
                let Some(instrument) =
                    lookup_instrument_in_snapshot(&instruments, ticker.product_id.as_str())
                else {
                    log::warn!("No instrument for product_id: {}", ticker.product_id);
                    return;
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
                    && let Err(e) = sender.send(DataEvent::FundingRate(funding))
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
                    return;
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
                    return;
                };
                let instrument_id = instrument.id();
                let sequence = book_sequence.load(Ordering::Relaxed);

                match parse_futures_ws_book_snapshot_deltas(
                    &snapshot, instrument, sequence, ts_init,
                ) {
                    Ok(delta_vec) => {
                        if delta_vec.is_empty() {
                            return;
                        }
                        book_sequence.fetch_add(delta_vec.len() as u64, Ordering::Relaxed);
                        let deltas = OrderBookDeltas::new(instrument_id, delta_vec);

                        let has_quote_sub = quote_instruments.contains(&instrument_id);
                        let book = order_books
                            .entry(instrument_id)
                            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

                        if let Err(e) = book.apply_deltas(&deltas) {
                            log::error!("Failed to apply snapshot deltas to order book: {e}");
                        } else if has_quote_sub {
                            Self::maybe_emit_quote(
                                book,
                                instrument_id,
                                last_quotes,
                                ts_init,
                                sender,
                            );
                        }

                        let has_book_sub = book_instruments.contains(&instrument_id);

                        if has_book_sub
                            && let Err(e) =
                                sender.send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
                        {
                            log::error!("Failed to send book snapshot deltas: {e}");
                        }
                    }
                    Err(e) => log::error!("Failed to parse book snapshot: {e}"),
                }
            }
            KrakenFuturesWsMessage::BookDelta(delta) => {
                let instruments = instruments.load();
                let Some(instrument) =
                    lookup_instrument_in_snapshot(&instruments, delta.product_id.as_str())
                else {
                    log::warn!("No instrument for product_id: {}", delta.product_id);
                    return;
                };
                let instrument_id = instrument.id();
                let sequence = book_sequence.fetch_add(1, Ordering::Relaxed);
                match parse_futures_ws_book_delta(&delta, instrument, sequence, ts_init) {
                    Ok(book_delta) => {
                        let deltas = OrderBookDeltas::new(instrument_id, vec![book_delta]);

                        let has_quote_sub = quote_instruments.contains(&instrument_id);

                        if let Some(book) = order_books.get_mut(&instrument_id) {
                            if let Err(e) = book.apply_deltas(&deltas) {
                                log::error!("Failed to apply delta to order book: {e}");
                            } else if has_quote_sub {
                                Self::maybe_emit_quote(
                                    book,
                                    instrument_id,
                                    last_quotes,
                                    ts_init,
                                    sender,
                                );
                            }
                        }

                        let has_book_sub = book_instruments.contains(&instrument_id);

                        if has_book_sub
                            && let Err(e) =
                                sender.send(DataEvent::Data(Data::BookDeltas(Box::new(deltas))))
                        {
                            log::error!("Failed to send book delta: {e}");
                        }
                    }
                    Err(e) => log::error!("Failed to parse book delta: {e}"),
                }
            }
            KrakenFuturesWsMessage::Reconnected => {
                last_quotes.clear();
                log::info!("Futures WebSocket reconnected");
            }
            KrakenFuturesWsMessage::OpenOrdersCancel(_)
            | KrakenFuturesWsMessage::OpenOrdersDelta(_)
            | KrakenFuturesWsMessage::FillsDelta(_)
            | KrakenFuturesWsMessage::Challenge(_) => {}
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
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Futures data client: {}", self.client_id);
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.ws.begin_shutdown();
        self.is_connected.store(false, Ordering::Relaxed);

        self.instruments.store(ahash::AHashMap::new());

        self.last_quotes.clear();
        self.quote_instruments.store(ahash::AHashSet::new());

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
                .connect()
                .await
                .context("Failed to connect futures WebSocket")?;
            self.ws
                .wait_until_active(10.0)
                .await
                .context("Futures WebSocket failed to become active")?;

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
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
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

        if cmd.book_type != BookType::L2_MBP {
            log::warn!(
                "Book type {:?} not supported by Kraken, skipping subscription",
                cmd.book_type
            );
            return Ok(());
        }

        self.book_instruments.insert(instrument_id);

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

        self.last_quotes.remove(&instrument_id);
        self.quote_instruments.insert(instrument_id);

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
        let instrument_id = cmd.instrument_id;
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
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
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
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
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
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        log::warn!(
            "Cannot subscribe to {} bars: Kraken Futures does not support EXTERNAL bar streaming",
            cmd.bar_type
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

        self.book_instruments.remove(&instrument_id);

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

        self.quote_instruments.remove(&instrument_id);
        self.last_quotes.remove(&instrument_id);
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

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
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
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
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
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
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

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let http = self.http.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
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

    use nautilus_common::{live::runner::set_data_event_sender, messages::DataEvent};
    use nautilus_core::UUID4;
    use nautilus_model::types::{Price, Quantity};
    use rstest::rstest;
    use rust_decimal::Decimal;
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
        let mut order_books = AHashMap::new();
        let last_quotes = DashMap::new();
        let book_sequence = Arc::new(AtomicU64::new(0));
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
        let mut sequence = 0;

        for (ts_init, msg) in &messages {
            let deltas = match msg {
                KrakenFuturesWsMessage::BookSnapshot(snapshot) => {
                    parse_futures_ws_book_snapshot_deltas(snapshot, &instrument, sequence, *ts_init)
                        .unwrap()
                }

                KrakenFuturesWsMessage::BookDelta(delta) => {
                    let delta = parse_futures_ws_book_delta(delta, &instrument, sequence, *ts_init)
                        .unwrap();
                    vec![delta]
                }

                other => panic!("Unexpected message: {other:?}"),
            };

            sequence += deltas.len() as u64;
            expected_deltas.push(OrderBookDeltas::new(instrument_id, deltas));
        }

        for (ts_init, msg) in messages {
            clock.set_time(ts_init);
            KrakenFuturesDataClient::handle_ws_message(
                msg,
                &sender,
                &instruments,
                &quote_instruments,
                &book_instruments,
                &mut order_books,
                &last_quotes,
                &book_sequence,
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
        let mut order_books = AHashMap::new();
        let last_quotes = DashMap::new();
        let sequence = Arc::new(AtomicU64::new(0));
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
        let mut next_sequence = 0;

        for (snapshot, timestamp) in [(initial, 10), (filtered, 20), (changed, 30)] {
            let timestamp = UnixNanos::new(timestamp);
            let deltas = parse_futures_ws_book_snapshot_deltas(
                &snapshot,
                &instrument,
                next_sequence,
                timestamp,
            )
            .unwrap();
            next_sequence += deltas.len() as u64;
            expected_deltas.push(OrderBookDeltas::new(instrument_id, deltas));
            clock.set_time(timestamp);
            KrakenFuturesDataClient::handle_ws_message(
                KrakenFuturesWsMessage::BookSnapshot(snapshot),
                &sender,
                &instruments,
                &quote_instruments,
                &book_instruments,
                &mut order_books,
                &last_quotes,
                &sequence,
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
        let sequence = Arc::new(AtomicU64::new(0));
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
                &client.data_sender,
                &client.instruments,
                &client.quote_instruments,
                &client.book_instruments,
                &mut order_books,
                &client.last_quotes,
                &sequence,
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
        let ts1 = UnixNanos::new(1_700_000_000_000_000_000);
        let ts2 = UnixNanos::new(1_700_000_000_100_000_000);
        let ts3 = UnixNanos::new(1_700_000_000_200_000_000);
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts1)));
        let mut order_books = AHashMap::new();
        let sequence = Arc::new(AtomicU64::new(0));
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
        let mut next_sequence = 0;

        for (message, ts) in messages.iter().zip([ts1, ts2, ts3]) {
            let deltas = match message {
                KrakenFuturesWsMessage::BookSnapshot(snapshot) => {
                    parse_futures_ws_book_snapshot_deltas(snapshot, &instrument, next_sequence, ts)
                        .unwrap()
                }

                KrakenFuturesWsMessage::BookDelta(delta) => vec![
                    parse_futures_ws_book_delta(delta, &instrument, next_sequence, ts).unwrap(),
                ],
                other => panic!("Unexpected message: {other:?}"),
            };

            next_sequence += deltas.len() as u64;
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
                &client.data_sender,
                &client.instruments,
                &client.quote_instruments,
                &client.book_instruments,
                &mut order_books,
                &client.last_quotes,
                &sequence,
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
