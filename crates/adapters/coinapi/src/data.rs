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

//! Live CoinAPI data client for instruments, order books, quotes, trades, OHLCV bars, and
//! historical requests.

use std::{
    collections::HashMap,
    future::Future,
    num::NonZeroUsize,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use arc_swap::ArcSwap;
use async_trait::async_trait;
use jiff::Timestamp;
use nautilus_common::{
    clients::DataClient,
    live::{dst::time, runner::get_data_event_sender, sender::EventSender},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, BookResponse, InstrumentResponse, InstrumentsResponse, QuotesResponse,
            RequestBars, RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestQuotes,
            RequestTrades, SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth, SubscribeQuotes,
            SubscribeTrades, TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas,
            UnsubscribeBookDepth, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    UnixNanos,
    datetime::datetime_to_unix_nanos,
    string::secret::SecretString,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    book::{
        BookSequenceOutcome,
        snapshot::{SnapshotGate, snapshot_expired},
    },
    task::{TaskGroup, TaskSpawner},
};
use nautilus_model::{
    data::{DEPTH10_LEN, Data},
    enums::{AggregationSource, BookType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::fixed::check_fixed_precision,
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    book::{
        recovery::{spawn_recovery_monitor, spawn_recovery_task, start_recovery},
        sync::{BookFrame, BookSubscription, BookSubscriptions},
    },
    common::{consts::COINAPI, credential::resolve_api_key, symbol::VenueSymbol},
    config::{CoinApiDataClientConfig, CoinApiInstrumentSelection},
    http::{
        client::{CoinApiHttpClient, HistoryPages},
        parse::{self as http_parse, CoinApiInstrument},
        query::{BookQuery, HistoryRange},
    },
    websocket::{
        client::CoinApiWebSocketClient,
        messages::{Channel, CoinApiWsMessage, DataType, Ohlcv, ScopeChange, Subscription},
        parse::{
            bar_spec_to_period_id, parse_bar, parse_book_depth, parse_quote_tick, parse_trade_tick,
        },
    },
};

const TASK_GRACE: Duration = Duration::from_secs(1);
const TASK_FORCE: Duration = Duration::from_secs(2);
const SUBSCRIPTION_RETRY_DELAY_INITIAL: Duration = Duration::from_secs(1);
const SUBSCRIPTION_RETRY_DELAY_MAX: Duration = Duration::from_secs(30);

// Fixed-depth channels in cost order, with the levels each lists per side
const BOOK_DEPTH_CHANNELS: [(DataType, usize); 3] = [
    (DataType::Book5, 5),
    (DataType::Book20, 20),
    (DataType::Book50, 50),
];

/// CoinAPI data client streaming order books, quotes, trades, and OHLCV bars for configured
/// native instruments.
///
/// Connecting loads the configured instruments, emits them, and retains the CoinAPI `symbol_id`
/// of each one for subscriptions and historical requests. Every subscription shares one WebSocket
/// connection, which replays its scope after a transport reconnect. Instrument refreshes update
/// precision and REST symbol IDs; subscriptions retain their connect-time symbol IDs until the
/// data client disconnects and connects again.
///
/// Book deltas stream from the full `book` channel for L2 books and from `book_l3` for L3 books,
/// with sequence gap detection and recovery. Depth snapshots stream from the cheapest of `book5`,
/// `book20`, and `book50` that lists the requested depth, ten levels by default. CoinAPI allows one
/// book channel per connection, so every book subscription must use the same channel.
#[derive(Debug)]
pub struct CoinApiDataClient {
    client_id: ClientId,
    config: CoinApiDataClientConfig,
    api_key: SecretString,
    http: Arc<CoinApiHttpClient>,
    ws: Option<Arc<CoinApiWebSocketClient>>,
    subscription_tx: Option<SubscriptionSender>,
    is_connected: bool,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    data_sender: EventSender<DataEvent>,
    instruments: Arc<InstrumentIndex>,
    books: Arc<BookSubscriptions>,
    ohlcv: Arc<Mutex<OhlcvCache>>,
    clock: &'static AtomicTime,
}

impl CoinApiDataClient {
    // panics-doc-ok (transitive via get_data_event_sender)
    /// Creates a client without connecting.
    ///
    /// # Errors
    ///
    /// Returns an error if no valid API key resolves from the config or `COINAPI_API_KEY`, when an
    /// instrument selection is empty, repeats an instrument, or names an instrument that does not
    /// map to its exchange, or when the REST or WebSocket configuration is invalid.
    ///
    /// # Panics
    ///
    /// Panics if the data event sender is not initialized on this thread.
    pub fn new(client_id: ClientId, config: CoinApiDataClientConfig) -> anyhow::Result<Self> {
        validate_selections(&config.instruments)?;
        validate_price_precisions(&config.instruments, &config.price_precisions)?;
        let api_key = resolve_api_key(config.api_key.clone())?;
        let http = CoinApiHttpClient::new(api_key.clone(), config.http_config())?;

        // Validates the WebSocket endpoint now; connect creates the client that owns the stream
        CoinApiWebSocketClient::new(api_key.clone(), config.websocket_config())?;

        Ok(Self {
            client_id,
            config,
            api_key,
            http: Arc::new(http),
            ws: None,
            subscription_tx: None,
            is_connected: false,
            session_tasks: TaskGroup::new(),
            command_tasks: TaskGroup::new(),
            data_sender: get_data_event_sender(),
            instruments: Arc::new(InstrumentIndex::default()),
            books: Arc::new(BookSubscriptions::default()),
            ohlcv: Arc::new(Mutex::new(OhlcvCache::new())),
            clock: get_atomic_clock_realtime(),
        })
    }

    // A `depth` subscribes fixed-depth snapshots; otherwise the book streams synchronized deltas
    fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        data_type: DataType,
        depth: Option<usize>,
    ) -> anyhow::Result<()> {
        let _write = self.books.writes.lock();

        if let Some(active) = self.books.entries.get_cloned(&instrument_id) {
            let active_depth = active.depth;
            anyhow::ensure!(
                active.data_type == data_type && active_depth == depth,
                "Conflicting CoinAPI book subscription for {instrument_id}: active {} with depth \
                 {active_depth:?}, requested {} with depth {depth:?}",
                active.data_type.as_ref(),
                data_type.as_ref()
            );
            return Ok(());
        }

        if let Some(active) = self
            .books
            .entries
            .load()
            .values()
            .find(|active| active.data_type != data_type)
        {
            anyhow::bail!(
                "CoinAPI streams one book channel per connection: {} is active, requested {}",
                active.data_type.as_ref(),
                data_type.as_ref()
            );
        }

        let (commands, subscription) =
            self.subscription(instrument_id, channel(data_type, None))?;

        let snapshot = if depth.is_some() {
            None
        } else {
            Some(self.expect_book_snapshot(instrument_id, subscription)?)
        };

        let registration = BookSubscription {
            data_type,
            depth,
            active: Arc::new(AtomicBool::new(false)),
        };

        if let Err(e) = send_subscription(
            &commands,
            ScopeChange::Subscribe(subscription),
            snapshot,
            Some(Arc::clone(&registration.active)),
        ) {
            self.books.sync.remove(instrument_id);
            return Err(e);
        }

        self.books.entries.insert(instrument_id, registration);
        Ok(())
    }

    // Holds snapshots behind a closed gate until the subscribing hello is written, then recovers
    // the book when its first snapshot misses the deadline
    fn expect_book_snapshot(
        &self,
        instrument_id: InstrumentId,
        subscription: Subscription,
    ) -> anyhow::Result<SnapshotWait> {
        let ws = self
            .ws
            .clone()
            .with_context(|| format!("{} is not connected", self.client_id))?;
        let spawner = self.session_tasks.spawner()?;
        let shutdown = spawner.cancellation_token();
        let tracker = self.books.sync.clone();
        let ohlcv = Arc::clone(&self.ohlcv);
        let timeout = self.book_snapshot_timeout();

        let gate = SnapshotGate::default();
        gate.lock().close();
        let cancel = tracker.record_subscription(
            instrument_id,
            subscription,
            time::Instant::now(),
            gate.clone(),
        );
        let guard = cancel.clone().drop_guard();
        let (written_tx, written_rx) = tokio::sync::oneshot::channel();

        let task = {
            let spawner = spawner.clone();

            async move {
                let _guard = guard;

                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => return,
                    () = cancel.cancelled() => return,
                    written = written_rx => {
                        if written.is_err() {
                            return;
                        }
                    }
                }

                tokio::select! {
                    biased;
                    () = shutdown.cancelled() => return,
                    expired = snapshot_expired(&cancel, timeout) => {
                        if !expired {
                            return;
                        }
                    }
                }

                log::warn!(
                    "Initial book snapshot missing for {instrument_id}; requesting a fresh snapshot"
                );

                if let Some((recovery, subscription)) =
                    tracker.claim_subscription_recovery(instrument_id, &cancel)
                {
                    spawn_recovery_task(
                        instrument_id,
                        subscription,
                        recovery,
                        ws,
                        ohlcv,
                        timeout,
                        &spawner,
                    );
                }
            }
        };

        if let Err(e) = spawner.spawn(task) {
            self.books.sync.remove(instrument_id);
            return Err(e.into());
        }

        Ok(SnapshotWait {
            gate,
            written: written_tx,
        })
    }

    // Removes the instrument's depth snapshots when `depth` is set, otherwise its deltas
    fn unsubscribe_book(&self, instrument_id: InstrumentId, depth: bool) -> anyhow::Result<()> {
        let _write = self.books.writes.lock();

        let Some(registration) = self
            .books
            .entries
            .get_cloned(&instrument_id)
            .filter(|registration| registration.depth.is_some() == depth)
        else {
            return Ok(());
        };

        let (commands, subscription) =
            self.subscription(instrument_id, channel(registration.data_type, None))?;

        // Removing the book first cancels its recovery before the scope change is queued
        self.books.sync.remove(instrument_id);
        self.books.entries.remove(&instrument_id);

        send_subscription(
            &commands,
            ScopeChange::Unsubscribe(subscription),
            None,
            None,
        )
    }

    fn book_snapshot_timeout(&self) -> Duration {
        Duration::from_secs(self.config.book_snapshot_timeout_secs)
    }

    fn subscribe_channel(
        &self,
        instrument_id: InstrumentId,
        channel: Channel,
    ) -> anyhow::Result<()> {
        let (commands, subscription) = self.subscription(instrument_id, channel)?;

        // A period retained from an earlier subscription must not close against this one
        if let Some(period_id) = channel.period_id {
            self.ohlcv
                .lock()
                .remove(&(subscription.symbol_id, period_id));
        }

        send_subscription(&commands, ScopeChange::Subscribe(subscription), None, None)
    }

    fn unsubscribe_channel(
        &self,
        instrument_id: InstrumentId,
        channel: Channel,
    ) -> anyhow::Result<()> {
        let (commands, subscription) = self.subscription(instrument_id, channel)?;
        send_subscription(
            &commands,
            ScopeChange::Unsubscribe(subscription),
            None,
            None,
        )
    }

    fn subscription(
        &self,
        instrument_id: InstrumentId,
        channel: Channel,
    ) -> anyhow::Result<(SubscriptionSender, Subscription)> {
        let commands = self
            .subscription_tx
            .clone()
            .with_context(|| format!("{} is not connected", self.client_id))?;
        let symbol_id = self
            .instruments
            .stream_symbol_id(instrument_id)
            .with_context(|| format!("{instrument_id} is not a loaded CoinAPI instrument"))?;

        Ok((commands, Subscription { channel, symbol_id }))
    }

    // Resolves a loaded instrument and a window ending at `end`, or now when absent
    fn history(
        &self,
        instrument_id: InstrumentId,
        start: Option<Timestamp>,
        end: Option<Timestamp>,
        limit: Option<NonZeroUsize>,
    ) -> anyhow::Result<(Ustr, InstrumentAny, HistoryRange)> {
        let (symbol_id, instrument) = self.loaded(instrument_id)?;
        let time_start = start.context("CoinAPI historical requests require a start time")?;
        let time_end = end.unwrap_or_else(|| self.clock.get_time_ns().to_datetime_utc());

        let range = HistoryRange {
            time_start,
            time_end,
            limit,
        };

        Ok((symbol_id, instrument, range))
    }

    fn loaded(&self, instrument_id: InstrumentId) -> anyhow::Result<(Ustr, InstrumentAny)> {
        self.instruments
            .loaded(instrument_id)
            .with_context(|| format!("{instrument_id} is not a loaded CoinAPI instrument"))
    }

    fn spawn_command<F>(&self, description: &'static str, future: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let cancellation = self.command_tasks.cancellation_token();

        let future = async move {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {}
                result = future => {
                    if let Err(e) = result {
                        log::error!("{description} failed: {e:#}");
                    }
                }
            }
        };

        if let Err(e) = self.command_tasks.spawn(future) {
            log::warn!("Skipping {description} after shutdown began: {e}");
        }
    }

    fn begin_shutdown(&mut self) {
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.is_connected = false;
    }

    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        let (session, command) = tokio::join!(
            self.session_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE),
            self.command_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE),
        );
        session.map_err(|e| anyhow::anyhow!("Failed to stop CoinAPI stream task: {e}"))?;
        command.map_err(|e| anyhow::anyhow!("Failed to stop CoinAPI command tasks: {e}"))?;
        Ok(())
    }
}

#[async_trait(?Send)]
impl DataClient for CoinApiDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::debug!("Starting {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Stopping {}", self.client_id);
        self.begin_shutdown();

        // Dropping the last handle closes the connection
        self.ws = None;
        self.subscription_tx = None;
        self.books.clear();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {}", self.client_id);
        self.begin_shutdown();
        self.ws = None;
        self.subscription_tx = None;
        self.books.clear();
        self.ohlcv.lock().clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {}", self.client_id);
        self.begin_shutdown();
        self.ws = None;
        self.subscription_tx = None;
        self.books.clear();
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected {
            return Ok(());
        }

        log::info!("Connecting {}", self.client_id);

        if !self.session_tasks.is_open() || !self.command_tasks.is_open() {
            self.begin_shutdown();
            self.finish_shutdown().await?;
            self.session_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to reopen CoinAPI stream tasks: {e}"))?;
            self.command_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to reopen CoinAPI command tasks: {e}"))?;
        }

        let loaded = load_instruments(
            &self.http,
            &self.config.instruments,
            &self.config.price_precisions,
            self.clock.get_time_ns(),
        )
        .await
        .context("Failed to load CoinAPI instruments")?;

        self.instruments.begin_stream(&loaded);

        for instrument in loaded {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.instrument))
            {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        let (mut ws, rx) =
            CoinApiWebSocketClient::new(self.api_key.clone(), self.config.websocket_config())?;

        self.books = Arc::new(BookSubscriptions {
            sync: self.books.sync.clone(),
            ..BookSubscriptions::default()
        });

        ws.bind_books(Arc::clone(&self.books));
        let ws = Arc::new(ws);

        let books = BookStreams {
            subscriptions: Arc::clone(&self.books),
            ws: Arc::clone(&ws),
            tasks: self.session_tasks.spawner()?,
            snapshot_timeout: self.book_snapshot_timeout(),
        };

        self.session_tasks.spawn(run_stream(
            rx,
            self.session_tasks.cancellation_token(),
            self.data_sender.clone(),
            Arc::clone(&self.instruments),
            books,
            Arc::clone(&self.ohlcv),
            self.clock,
        ))?;

        let (subscription_tx, subscription_rx) = tokio::sync::mpsc::unbounded_channel();

        self.session_tasks.spawn(run_subscriptions(
            Arc::clone(&ws),
            subscription_rx,
            self.session_tasks.cancellation_token(),
        ))?;

        self.ws = Some(ws);
        self.subscription_tx = Some(subscription_tx);
        self.is_connected = true;
        log::info!("Connected {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting {}", self.client_id);
        self.begin_shutdown();

        // Draining tasks first prevents a queued subscription from reopening the connection
        let result = self.finish_shutdown().await;

        self.subscription_tx = None;

        if let Some(ws) = self.ws.take() {
            ws.close().await;
        }

        self.books.clear();
        self.ohlcv.lock().clear();

        log::info!("Disconnected {}", self.client_id);
        result
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        let data_type = match cmd.book_type {
            BookType::L2_MBP => DataType::Book,
            BookType::L3_MBO => DataType::BookL3,
            BookType::L1_MBP => anyhow::bail!("CoinAPI streams L2_MBP and L3_MBO book deltas"),
        };

        // Every channel sends the full book, so a depth limit cannot reduce the stream
        self.subscribe_book(cmd.instrument_id, data_type, None)
    }

    fn subscribe_book_depth(&mut self, cmd: SubscribeBookDepth) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.book_type == BookType::L2_MBP,
            "CoinAPI depth snapshots are L2_MBP, was {}",
            cmd.book_type
        );

        let depth = cmd.depth.map_or(DEPTH10_LEN, NonZeroUsize::get);
        let (data_type, _) = BOOK_DEPTH_CHANNELS
            .into_iter()
            .find(|(_, levels)| depth <= *levels)
            .context(format!(
                "CoinAPI depth snapshots list at most 50 levels, was {depth}"
            ))?;

        self.subscribe_book(cmd.instrument_id, data_type, Some(depth))
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.subscribe_channel(cmd.instrument_id, channel(DataType::Quote, None))
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.subscribe_channel(cmd.instrument_id, channel(DataType::Trade, None))
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "CoinAPI streams only external bars, was {bar_type}"
        );

        let period_id = bar_spec_to_period_id(bar_type.spec())?;
        self.subscribe_channel(
            bar_type.instrument_id(),
            channel(DataType::Ohlcv, Some(period_id)),
        )
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        self.unsubscribe_book(cmd.instrument_id, false)
    }

    fn unsubscribe_book_depth(&mut self, cmd: &UnsubscribeBookDepth) -> anyhow::Result<()> {
        self.unsubscribe_book(cmd.instrument_id, true)
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.unsubscribe_channel(cmd.instrument_id, channel(DataType::Quote, None))
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        self.unsubscribe_channel(cmd.instrument_id, channel(DataType::Trade, None))
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let period_id = bar_spec_to_period_id(cmd.bar_type.spec())?;
        self.unsubscribe_channel(
            cmd.bar_type.instrument_id(),
            channel(DataType::Ohlcv, Some(period_id)),
        )
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let selections = select_instruments(&self.config.instruments, |instrument_id| {
            request
                .venue
                .is_none_or(|venue| instrument_id.venue == venue)
        });

        let http = Arc::clone(&self.http);
        let price_precisions = self.config.price_precisions.clone();
        let index = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);

        // Names the provider when the request names no venue
        let venue = request.venue.unwrap_or_else(|| Venue::new(COINAPI));
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let clock = self.clock;

        self.spawn_command("CoinAPI instruments request", async move {
            let loaded =
                load_instruments(&http, &selections, &price_precisions, clock.get_time_ns())
                    .await?;

            let instruments = loaded
                .iter()
                .map(|instrument| {
                    index.insert(instrument);
                    instrument.instrument.clone()
                })
                .collect();

            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request.request_id,
                client_id,
                venue,
                instruments,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let selections = select_instruments(&self.config.instruments, |candidate| {
            *candidate == instrument_id
        });

        anyhow::ensure!(
            !selections.is_empty(),
            "{instrument_id} is not a configured CoinAPI instrument"
        );

        let http = Arc::clone(&self.http);
        let price_precisions = self.config.price_precisions.clone();
        let index = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let clock = self.clock;

        self.spawn_command("CoinAPI instrument request", async move {
            let loaded =
                load_instruments(&http, &selections, &price_precisions, clock.get_time_ns())
                    .await?;
            let instrument = loaded
                .into_iter()
                .next()
                .context("CoinAPI instrument load returned nothing")?;
            index.insert(&instrument);

            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                instrument.instrument,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            )));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let (symbol_id, instrument) = self.loaded(instrument_id)?;

        let query = BookQuery {
            limit_levels: request
                .depth
                .map(|depth| u32::try_from(depth.get()).unwrap_or(u32::MAX)),
        };

        let http = Arc::clone(&self.http);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let clock = self.clock;

        self.spawn_command("CoinAPI book snapshot request", async move {
            let book = http.get_book(&symbol_id, &query).await?;
            let book = http_parse::parse_order_book(&book, &instrument)
                .with_context(|| format!("Invalid CoinAPI book for {symbol_id}"))?;

            let response = DataResponse::Book(BookResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                book,
                None,
                None,
                clock.get_time_ns(),
                request.params,
            ));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let (symbol_id, instrument, range) =
            self.history(instrument_id, request.start, request.end, request.limit)?;

        let http = Arc::clone(&self.http);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let clock = self.clock;

        self.spawn_command("CoinAPI quotes request", async move {
            let pages = http.get_quotes_range(&symbol_id, &range).await?;

            let quotes = parse_history(&pages, "quote", symbol_id, |quote| {
                http_parse::parse_quote_tick(quote, &instrument)
            });

            let response = DataResponse::Quotes(QuotesResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                quotes,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let (symbol_id, instrument, range) =
            self.history(instrument_id, request.start, request.end, request.limit)?;

        let http = Arc::clone(&self.http);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let clock = self.clock;

        self.spawn_command("CoinAPI trades request", async move {
            let pages = http.get_trades_range(&symbol_id, &range).await?;

            let trades = parse_history(&pages, "trade", symbol_id, |trade| {
                http_parse::parse_trade_tick(trade, &instrument)
            });

            let response = DataResponse::Trades(TradesResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                trades,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.bar_type;
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "CoinAPI provides only external bars, was {bar_type}"
        );

        let period_id = bar_spec_to_period_id(bar_type.spec())?;
        let (symbol_id, instrument, range) = self.history(
            bar_type.instrument_id(),
            request.start,
            request.end,
            request.limit,
        )?;

        let http = Arc::clone(&self.http);
        let sender = self.data_sender.clone();
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = datetime_to_unix_nanos(request.start);
        let end = datetime_to_unix_nanos(request.end);
        let clock = self.clock;

        self.spawn_command("CoinAPI bars request", async move {
            let mut pages = http.get_ohlcv_range(&symbol_id, &period_id, &range).await?;

            // A period still in progress, or closing after the window, is not a complete bar
            let closed_by = range.time_end.min(clock.get_time_ns().to_datetime_utc());
            pages
                .records
                .retain(|ohlcv| ohlcv.time_period_end <= closed_by);

            let bars = parse_history(&pages, "OHLCV period", symbol_id, |ohlcv| {
                http_parse::parse_bar(ohlcv, bar_type, &instrument)
            });

            let response = DataResponse::Bars(BarsResponse::new(
                request.request_id,
                client_id,
                bar_type,
                bars,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            send_response(&sender, response);
            Ok(())
        });

        Ok(())
    }
}

// Scope changes apply in call order through one task, so a quick unsubscribe cannot overtake its
// subscribe
type SubscriptionSender = tokio::sync::mpsc::UnboundedSender<SubscriptionCommand>;

#[derive(Debug)]
struct SubscriptionCommand {
    change: ScopeChange,
    snapshot: Option<SnapshotWait>,
    book_active: Option<Arc<AtomicBool>>,
}

// Opens a book's snapshot gate once its subscribing hello is written, then starts its deadline
#[derive(Debug)]
struct SnapshotWait {
    gate: SnapshotGate,
    written: tokio::sync::oneshot::Sender<()>,
}

fn send_subscription(
    commands: &SubscriptionSender,
    change: ScopeChange,
    snapshot: Option<SnapshotWait>,
    book_active: Option<Arc<AtomicBool>>,
) -> anyhow::Result<()> {
    commands
        .send(SubscriptionCommand {
            change,
            snapshot,
            book_active,
        })
        .map_err(|_| anyhow::anyhow!("CoinAPI subscription task has stopped"))
}

// Applies every change queued by the time it wakes as one batch, so a burst of subscriptions shares
// one hello
async fn run_subscriptions(
    ws: Arc<CoinApiWebSocketClient>,
    mut commands: tokio::sync::mpsc::UnboundedReceiver<SubscriptionCommand>,
    cancellation: CancellationToken,
) {
    loop {
        let first = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            command = commands.recv() => match command {
                Some(command) => command,
                None => break,
            },
        };

        let mut batch = vec![first];
        while let Ok(command) = commands.try_recv() {
            batch.push(command);
        }

        let changes: Vec<ScopeChange> = batch.iter().map(|command| command.change).collect();
        let registrations: Vec<_> = batch
            .iter()
            .map(|command| command.book_active.clone())
            .collect();
        let mut delay = SUBSCRIPTION_RETRY_DELAY_INITIAL;

        // The engine records the change once queued, so a transport failure must not drop it
        let rejected = loop {
            let result = tokio::select! {
                biased;
                () = cancellation.cancelled() => return,
                result = ws.apply_registered(&changes, &registrations) => result,
            };

            match result {
                Ok(rejected) => break rejected,
                Err(e) => {
                    log::warn!("CoinAPI subscription change failed, retrying in {delay:?}: {e}");
                    tokio::select! {
                        biased;
                        () = cancellation.cancelled() => return,
                        () = time::sleep(delay) => {}
                    }
                    delay = (delay * 2).min(SUBSCRIPTION_RETRY_DELAY_MAX);
                }
            }
        };

        for (change, e) in &rejected {
            log::error!("CoinAPI rejected {change:?}: {e}");
        }

        for command in batch {
            let Some(snapshot) = command.snapshot else {
                continue;
            };

            if rejected
                .iter()
                .all(|(rejected, _)| *rejected != command.change)
            {
                snapshot.gate.open();
                let _ = snapshot.written.send(());
            }
        }
    }
}

// Book state and recovery handles the stream task shares with subscription commands
#[derive(Debug)]
struct BookStreams {
    subscriptions: Arc<BookSubscriptions>,
    ws: Arc<CoinApiWebSocketClient>,
    tasks: TaskSpawner,
    snapshot_timeout: Duration,
}

// Latest update for each (symbol_id, period_id) OHLCV stream
pub(crate) type OhlcvCache = AHashMap<(Ustr, Ustr), Ohlcv>;

#[derive(Debug, Default)]
struct InstrumentIndex {
    state: ArcSwap<InstrumentIndexState>,
}

#[derive(Debug, Clone, Default)]
struct InstrumentIndexState {
    by_symbol_id: AHashMap<Ustr, InstrumentAny>,
    symbol_ids: AHashMap<InstrumentId, Ustr>,
    stream_symbol_ids: AHashMap<InstrumentId, Ustr>,
}

impl InstrumentIndex {
    fn insert(&self, loaded: &CoinApiInstrument) {
        self.state.rcu(|state| {
            let mut next = (**state).clone();
            next.insert(loaded);
            Arc::new(next)
        });
    }

    fn begin_stream(&self, loaded: &[CoinApiInstrument]) {
        let mut state = InstrumentIndexState::default();
        for instrument in loaded {
            state
                .stream_symbol_ids
                .insert(instrument.instrument.id(), instrument.symbol_id);
            state.insert(instrument);
        }

        self.state.store(Arc::new(state));
    }

    fn stream_symbol_id(&self, instrument_id: InstrumentId) -> Option<Ustr> {
        self.state
            .load()
            .stream_symbol_ids
            .get(&instrument_id)
            .copied()
    }

    fn loaded(&self, instrument_id: InstrumentId) -> Option<(Ustr, InstrumentAny)> {
        let state = self.state.load();
        let symbol_id = *state.symbol_ids.get(&instrument_id)?;
        Some((symbol_id, state.by_symbol_id.get(&symbol_id)?.clone()))
    }

    fn instrument(&self, symbol_id: Ustr) -> Option<InstrumentAny> {
        self.state.load().by_symbol_id.get(&symbol_id).cloned()
    }
}

impl InstrumentIndexState {
    fn insert(&mut self, loaded: &CoinApiInstrument) {
        let instrument_id = loaded.instrument.id();
        let stream_symbol_id = self.stream_symbol_ids.get(&instrument_id).copied();

        if let Some(previous) = self.symbol_ids.get(&instrument_id)
            && *previous != loaded.symbol_id
            && Some(*previous) != stream_symbol_id
        {
            self.by_symbol_id.remove(previous);
        }

        self.by_symbol_id
            .insert(loaded.symbol_id, loaded.instrument.clone());
        self.symbol_ids.insert(instrument_id, loaded.symbol_id);

        if let Some(symbol_id) = stream_symbol_id
            && symbol_id != loaded.symbol_id
        {
            self.by_symbol_id
                .insert(symbol_id, loaded.instrument.clone());
        }
    }
}

fn validate_selections(selections: &[CoinApiInstrumentSelection]) -> anyhow::Result<()> {
    let mut seen = AHashSet::new();

    for selection in selections {
        anyhow::ensure!(
            !selection.instrument_ids.is_empty(),
            "CoinAPI selection for {} lists no instruments",
            selection.exchange_id
        );

        for instrument_id in &selection.instrument_ids {
            let exchange_id = VenueSymbol::from_native(*instrument_id)?.exchange_id;
            anyhow::ensure!(
                exchange_id == selection.exchange_id,
                "{instrument_id} maps to CoinAPI exchange {exchange_id}, not {}",
                selection.exchange_id
            );
            anyhow::ensure!(
                seen.insert(*instrument_id),
                "{instrument_id} is selected more than once"
            );
        }
    }

    Ok(())
}

fn validate_price_precisions(
    selections: &[CoinApiInstrumentSelection],
    price_precisions: &HashMap<InstrumentId, u8>,
) -> anyhow::Result<()> {
    for (instrument_id, precision) in price_precisions {
        anyhow::ensure!(
            selections
                .iter()
                .any(|selection| selection.instrument_ids.contains(instrument_id)),
            "price precision for {instrument_id}, which is not a selected instrument"
        );
        check_fixed_precision(*precision)
            .with_context(|| format!("invalid price precision for {instrument_id}"))?;
    }

    Ok(())
}

// Narrows each selection to the matching instruments and drops selections left empty
fn select_instruments(
    selections: &[CoinApiInstrumentSelection],
    predicate: impl Fn(&InstrumentId) -> bool,
) -> Vec<CoinApiInstrumentSelection> {
    selections
        .iter()
        .filter_map(|selection| {
            let instrument_ids: Vec<InstrumentId> = selection
                .instrument_ids
                .iter()
                .copied()
                .filter(&predicate)
                .collect();

            (!instrument_ids.is_empty()).then(|| CoinApiInstrumentSelection {
                instrument_ids,
                ..selection.clone()
            })
        })
        .collect()
}

// Requests each selection's exchange once and requires every selected instrument
async fn load_instruments(
    http: &CoinApiHttpClient,
    selections: &[CoinApiInstrumentSelection],
    price_precisions: &HashMap<InstrumentId, u8>,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<CoinApiInstrument>> {
    let mut selected = Vec::new();

    for selection in selections {
        let loaded = http
            .request_instruments(
                &selection.exchange_id,
                &selection.query,
                price_precisions,
                ts_init,
            )
            .await?;

        for instrument_id in &selection.instrument_ids {
            let instrument = loaded
                .iter()
                .find(|instrument| instrument.instrument.id() == *instrument_id)
                .with_context(|| {
                    format!(
                        "CoinAPI {} returned no instrument {instrument_id}",
                        selection.exchange_id
                    )
                })?;

            selected.push(instrument.clone());
        }
    }

    Ok(selected)
}

fn channel(data_type: DataType, period_id: Option<Ustr>) -> Channel {
    Channel {
        data_type,
        period_id,
    }
}

fn send_response(sender: &EventSender<DataEvent>, response: DataResponse) {
    if let Err(e) = sender.send(DataEvent::Response(response)) {
        log::error!("Failed to send CoinAPI response: {e}");
    }
}

// Converts each record, dropping and counting those that fail rather than failing the request
fn parse_history<T, D>(
    pages: &HistoryPages<T>,
    kind: &str,
    symbol_id: Ustr,
    parse: impl Fn(&T) -> anyhow::Result<D>,
) -> Vec<D> {
    let total = pages.records.len();

    if pages.is_truncated {
        log::warn!(
            "CoinAPI credit budget truncated the {kind} history for {symbol_id} after {total} records"
        );
    }

    let mut first_error = None;

    let parsed: Vec<D> = pages
        .records
        .iter()
        .filter_map(|record| {
            parse(record)
                .map_err(|e| {
                    first_error.get_or_insert(e);
                })
                .ok()
        })
        .collect();

    if let Some(e) = first_error {
        let dropped = total - parsed.len();
        log::warn!(
            "Dropped {dropped} of {total} CoinAPI {kind} records for {symbol_id}, first failure: {e:#}"
        );
    }

    parsed
}

async fn run_stream(
    mut rx: tokio::sync::mpsc::UnboundedReceiver<CoinApiWsMessage>,
    cancellation: CancellationToken,
    sender: EventSender<DataEvent>,
    instruments: Arc<InstrumentIndex>,
    books: BookStreams,
    ohlcv: Arc<Mutex<OhlcvCache>>,
    clock: &'static AtomicTime,
) {
    loop {
        let message = tokio::select! {
            biased;
            () = cancellation.cancelled() => break,
            message = rx.recv() => match message {
                Some(message) => message,
                None => break,
            },
        };

        handle_message(message, &sender, &instruments, &books, &ohlcv, clock);
    }
}

fn handle_message(
    message: CoinApiWsMessage,
    sender: &EventSender<DataEvent>,
    instruments: &InstrumentIndex,
    books: &BookStreams,
    ohlcv: &Arc<Mutex<OhlcvCache>>,
    clock: &AtomicTime,
) {
    let (symbol_id, kind, parsed) = match message {
        CoinApiWsMessage::Book(book) => {
            let Some(instrument) = instruments.instrument(book.symbol_id) else {
                return log_unknown_symbol(book.symbol_id);
            };

            if book.data_type == DataType::Book {
                return handle_book_frame(
                    BookFrame::Levels(&book),
                    &instrument,
                    books,
                    ohlcv,
                    sender,
                );
            }

            let Some(depth) = books
                .subscriptions
                .entries
                .load()
                .get(&instrument.id())
                .and_then(|registration| registration.depth)
            else {
                return;
            };

            let parsed = parse_book_depth(&book, depth, &instrument).map(|mut depth| {
                depth.sequence = books
                    .subscriptions
                    .sync
                    .depth_sequence(instrument.id(), book.sequence);
                Data::BookDepth(Box::new(depth))
            });

            (book.symbol_id, "book depth", parsed)
        }
        CoinApiWsMessage::BookL3(book) => {
            let Some(instrument) = instruments.instrument(book.symbol_id) else {
                return log_unknown_symbol(book.symbol_id);
            };

            return handle_book_frame(BookFrame::Orders(&book), &instrument, books, ohlcv, sender);
        }
        CoinApiWsMessage::Quote(quote) => {
            let Some(instrument) = instruments.instrument(quote.symbol_id) else {
                return log_unknown_symbol(quote.symbol_id);
            };

            let parsed = parse_quote_tick(&quote, &instrument).map(Data::Quote);
            (quote.symbol_id, "quote", parsed)
        }
        CoinApiWsMessage::Trade(trade) => {
            let Some(instrument) = instruments.instrument(trade.symbol_id) else {
                return log_unknown_symbol(trade.symbol_id);
            };

            let parsed = parse_trade_tick(&trade, &instrument).map(Data::Trade);
            (trade.symbol_id, "trade", parsed)
        }
        CoinApiWsMessage::Ohlcv(update) => {
            let Some(closed) = close_period(&mut ohlcv.lock(), update) else {
                return;
            };

            let Some(instrument) = instruments.instrument(closed.symbol_id) else {
                return log_unknown_symbol(closed.symbol_id);
            };

            let parsed = parse_bar(&closed, &instrument, clock.get_time_ns()).map(Data::Bar);
            (closed.symbol_id, "OHLCV period", parsed)
        }
        CoinApiWsMessage::Error(message) => {
            log::error!("CoinAPI retired the stream and its subscriptions: {message}");
            return;
        }
        CoinApiWsMessage::Reconnected => {
            // Updates missed during the outage could leave a retained period incomplete
            ohlcv.lock().clear();
            reset_books(books, ohlcv);
            log::info!("CoinAPI stream reconnected and replayed its subscriptions");
            return;
        }
    };

    match parsed {
        Ok(data) => {
            if let Err(e) = sender.send(DataEvent::Data(data)) {
                log::error!("Failed to send CoinAPI data: {e}");
            }
        }
        Err(e) => log::warn!("Dropping CoinAPI {kind} for {symbol_id}: {e:#}"),
    }
}

fn handle_book_frame(
    frame: BookFrame<'_>,
    instrument: &InstrumentAny,
    books: &BookStreams,
    ohlcv: &Arc<Mutex<OhlcvCache>>,
    sender: &EventSender<DataEvent>,
) {
    let instrument_id = instrument.id();

    match books
        .subscriptions
        .sync
        .apply(instrument_id, frame, instrument, time::Instant::now())
    {
        (BookSequenceOutcome::Accept, Some(deltas)) => {
            if let Err(e) = sender.send(DataEvent::Data(Data::BookDeltas(Box::new(deltas)))) {
                log::error!("Failed to send CoinAPI data: {e}");
            }
        }
        (BookSequenceOutcome::Recover, _) => start_recovery(
            instrument_id,
            &books.subscriptions.sync,
            &books.ws,
            ohlcv,
            books.snapshot_timeout,
            &books.tasks,
        ),
        _ => {}
    }
}

// Sequences restart per connection, so each book waits for the snapshot its replayed pair brings
fn reset_books(books: &BookStreams, ohlcv: &Arc<Mutex<OhlcvCache>>) {
    let deadline =
        (!books.snapshot_timeout.is_zero()).then(|| time::Instant::now() + books.snapshot_timeout);

    if books.subscriptions.sync.reset_on_reconnect(deadline) > 0 {
        spawn_recovery_monitor(
            books.subscriptions.sync.clone(),
            Arc::clone(&books.ws),
            Arc::clone(ohlcv),
            books.snapshot_timeout,
            &books.tasks,
        );
    }
}

fn log_unknown_symbol(symbol_id: Ustr) {
    log::warn!("Dropping CoinAPI message for unloaded symbol {symbol_id}");
}

// CoinAPI repeats a period's running totals and sends no close flag; an update for a later
// period closes the retained one
fn close_period(cache: &mut OhlcvCache, update: Ohlcv) -> Option<Ohlcv> {
    let key = (update.symbol_id, update.period_id);
    match cache.get(&key) {
        Some(retained) if update.time_period_start < retained.time_period_start => None,
        Some(retained) if update.time_period_start == retained.time_period_start => {
            cache.insert(key, update);
            None
        }
        _ => cache.insert(key, update),
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::types::Price;
    use rstest::rstest;

    use super::*;

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[case::stop(DataClient::stop, false)]
    #[case::reset(DataClient::reset, true)]
    #[case::dispose(DataClient::dispose, false)]
    #[tokio::test]
    async fn shutdown_cancels_tasks_and_clears_book_state(
        #[case] shutdown: fn(&mut CoinApiDataClient) -> anyhow::Result<()>,
        #[case] clears_bars: bool,
    ) {
        let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::live::runner::replace_data_event_sender(sender);

        let mut client = CoinApiDataClient::new(
            ClientId::from("COINAPI"),
            CoinApiDataClientConfig {
                api_key: Some("test-key".into()),
                ..CoinApiDataClientConfig::default()
            },
        )
        .unwrap();

        let instrument_id = InstrumentId::from("BTC-USD.COINBASE");

        let subscription = Subscription {
            channel: channel(DataType::Book, None),
            symbol_id: "COINBASE_SPOT_BTC_USD".into(),
        };

        let symbols: Vec<crate::http::models::Symbol> =
            serde_json::from_str(include_str!("../test_data/symbols.json")).unwrap();
        let (loaded, _) = http_parse::parse_instruments(&symbols, UnixNanos::default());
        let instrument = loaded
            .into_iter()
            .find(|loaded| loaded.instrument.id() == instrument_id)
            .unwrap()
            .instrument;
        let mut frame: crate::websocket::messages::Book =
            serde_json::from_str(include_str!("../test_data/ws_book.json")).unwrap();
        frame.sequence = 100;
        frame.is_snapshot = Some(true);
        let gate = SnapshotGate::default();
        gate.open();
        let book_cancel = client.books.sync.record_subscription(
            instrument_id,
            subscription,
            time::Instant::now(),
            gate.clone(),
        );
        client.books.entries.insert(
            instrument_id,
            BookSubscription {
                data_type: DataType::Book,
                depth: None,
                active: Arc::new(AtomicBool::new(false)),
            },
        );

        let (outcome, deltas) = client.books.sync.apply(
            instrument_id,
            BookFrame::Levels(&frame),
            &instrument,
            time::Instant::now(),
        );
        assert_eq!(outcome, BookSequenceOutcome::Accept);
        assert_eq!(deltas.unwrap().sequence, 100);
        assert_eq!(client.books.sync.depth_sequence(instrument_id, 17), 17);
        let retained = update(1, "84523.69");
        let key = (retained.symbol_id, retained.period_id);
        client.ohlcv.lock().insert(key, retained.clone());
        client.is_connected = true;
        let (ws, _messages) =
            CoinApiWebSocketClient::new(client.api_key.clone(), client.config.websocket_config())
                .unwrap();
        client.ws = Some(Arc::new(ws));
        let (commands, mut command_receiver) = tokio::sync::mpsc::unbounded_channel();
        client.subscription_tx = Some(commands);
        let mut completions = Vec::new();

        for group in [&client.session_tasks, &client.command_tasks] {
            let cancel = group.cancellation_token();
            let (done, completion) = tokio::sync::oneshot::channel();

            group
                .spawn(async move {
                    cancel.cancelled().await;
                    done.send(()).unwrap();
                })
                .unwrap();

            completions.push(completion);
        }

        let retired = Arc::clone(&client.books);
        shutdown(&mut client).unwrap();

        for completion in completions {
            tokio::time::timeout(Duration::from_secs(1), completion)
                .await
                .unwrap()
                .unwrap();
        }

        assert!(client.is_disconnected());
        assert!(client.ws.is_none());
        assert!(client.subscription_tx.is_none());
        assert!(command_receiver.recv().await.is_none());
        assert!(client.books.entries.is_empty());
        assert!(book_cancel.is_cancelled());
        assert_eq!(client.books.sync.reset_on_reconnect(None), 0);
        assert!(!client.session_tasks.is_open());
        assert!(!client.command_tasks.is_open());

        {
            let bars = client.ohlcv.lock();

            if clears_bars {
                assert!(bars.is_empty());
            } else {
                assert_eq!(bars.len(), 1);
                assert_eq!(bars[&key], retained);
            }
        }

        client.connect().await.unwrap();
        let active = Arc::new(AtomicBool::new(true));
        let replacement_cancel = client.books.sync.record_subscription(
            instrument_id,
            subscription,
            time::Instant::now(),
            gate,
        );
        client.books.entries.insert(
            instrument_id,
            BookSubscription {
                data_type: DataType::Book,
                depth: None,
                active: Arc::clone(&active),
            },
        );

        retired.retire();

        let replacement = client.books.entries.get_cloned(&instrument_id).unwrap();
        assert_eq!(replacement.data_type, DataType::Book);
        assert_eq!(replacement.depth, None);
        assert!(Arc::ptr_eq(&replacement.active, &active));
        assert!(
            replacement
                .active
                .load(std::sync::atomic::Ordering::Relaxed)
        );
        assert!(!replacement_cancel.is_cancelled());
        assert!(!Arc::ptr_eq(&retired, &client.books));
        frame.sequence = 1;
        let (outcome, deltas) = client.books.sync.apply(
            instrument_id,
            BookFrame::Levels(&frame),
            &instrument,
            time::Instant::now(),
        );
        assert_eq!(outcome, BookSequenceOutcome::Accept);
        assert_eq!(deltas.unwrap().sequence, 101);
        frame.sequence = 2;
        frame.is_snapshot = Some(false);
        let (outcome, deltas) = client.books.sync.apply(
            instrument_id,
            BookFrame::Levels(&frame),
            &instrument,
            time::Instant::now(),
        );
        assert_eq!(outcome, BookSequenceOutcome::Accept);
        assert_eq!(deltas.unwrap().sequence, 102);
        assert_eq!(client.books.sync.depth_sequence(instrument_id, 1), 18);
        client.disconnect().await.unwrap();
    }

    fn update(start_second: i64, close: &str) -> Ohlcv {
        let mut update: Ohlcv =
            serde_json::from_str(include_str!("../test_data/ws_ohlcv.json")).unwrap();
        update.time_period_start = jiff::Timestamp::from_second(start_second).unwrap();
        update.time_period_end = jiff::Timestamp::from_second(start_second + 1).unwrap();
        update.price_close = close.parse().unwrap();
        update
    }

    fn selection(exchange_id: &str, instrument_ids: &[&str]) -> CoinApiInstrumentSelection {
        CoinApiInstrumentSelection {
            exchange_id: exchange_id.into(),
            query: crate::http::query::SymbolsQuery::default(),
            instrument_ids: instrument_ids
                .iter()
                .map(|id| InstrumentId::from(*id))
                .collect(),
        }
    }

    #[rstest]
    #[case(vec![], None)]
    #[case(
        vec![selection("COINBASE", &["BTC-USD.COINBASE"]), selection("BINANCEFTS", &["BTCUSDT-PERP.BINANCE"])],
        None
    )]
    #[case(
        vec![selection("COINBASE", &[])],
        Some("CoinAPI selection for COINBASE lists no instruments")
    )]
    #[case(
        vec![selection("BINANCE", &["BTCUSDT-PERP.BINANCE"])],
        Some("BTCUSDT-PERP.BINANCE maps to CoinAPI exchange BINANCEFTS, not BINANCE")
    )]
    #[case(
        vec![selection("COINBASE", &["BTC-USD.COINBASE"]), selection("COINBASE", &["BTC-USD.COINBASE"])],
        Some("BTC-USD.COINBASE is selected more than once")
    )]
    #[case(
        vec![selection("DERIBIT", &["BTC-PERPETUAL.DERIBIT"])],
        Some("instrument ID BTC-PERPETUAL.DERIBIT has no CoinAPI mapping")
    )]
    fn selections_validate_mapping_and_uniqueness(
        #[case] selections: Vec<CoinApiInstrumentSelection>,
        #[case] expected: Option<&str>,
    ) {
        let result = validate_selections(&selections);

        assert_eq!(result.err().map(|e| e.to_string()).as_deref(), expected);
    }

    #[rstest]
    #[case(&[("BTC-USD.COINBASE", 3)], None)]
    #[case(
        &[("ETH-USD.COINBASE", 3)],
        Some("price precision for ETH-USD.COINBASE, which is not a selected instrument")
    )]
    #[case(&[("BTC-USD.COINBASE", 19)], Some("invalid price precision for BTC-USD.COINBASE"))]
    fn price_precisions_require_selected_instruments_and_valid_precision(
        #[case] precisions: &[(&str, u8)],
        #[case] expected: Option<&str>,
    ) {
        let selections = vec![selection("COINBASE", &["BTC-USD.COINBASE"])];
        let precisions = precisions
            .iter()
            .map(|(id, precision)| (InstrumentId::from(*id), *precision))
            .collect();

        let result = validate_price_precisions(&selections, &precisions);

        assert_eq!(result.err().map(|e| e.to_string()).as_deref(), expected);
    }

    #[rstest]
    fn select_instruments_narrows_and_drops_empty_selections() {
        let selections = vec![
            selection("COINBASE", &["BTC-USD.COINBASE", "ETH-USD.COINBASE"]),
            selection("BINANCE", &["BTCUSDT.BINANCE"]),
        ];

        let selected = select_instruments(&selections, |id| {
            id.symbol.as_str() != "BTCUSDT" && id.symbol.as_str() != "ETH-USD"
        });

        assert_eq!(selected.len(), 1);
        assert_eq!(selected[0].exchange_id, "COINBASE");
        assert_eq!(
            selected[0].instrument_ids,
            vec![InstrumentId::from("BTC-USD.COINBASE")]
        );
    }

    #[rstest]
    fn instrument_index_replaces_changed_symbol_id() {
        let symbols: Vec<crate::http::models::Symbol> =
            serde_json::from_str(include_str!("../test_data/symbols.json")).unwrap();
        let (instruments, _) =
            crate::http::parse::parse_instruments(&symbols, UnixNanos::default());
        let original = instruments
            .into_iter()
            .find(|loaded| loaded.symbol_id == "COINBASE_SPOT_BTC_USD")
            .unwrap();

        let suffixed = CoinApiInstrument {
            symbol_id: "COINBASE_SPOT_BTC_USD_22F0062".into(),
            ..original.clone()
        };

        let index = InstrumentIndex::default();

        index.insert(&original);
        index.insert(&suffixed);

        let instrument_id = original.instrument.id();
        assert_eq!(
            index.loaded(instrument_id).map(|(symbol_id, _)| symbol_id),
            Some(suffixed.symbol_id)
        );
        assert_eq!(
            index.instrument(suffixed.symbol_id),
            Some(original.instrument)
        );
        assert_eq!(index.instrument(original.symbol_id), None);
    }

    #[rstest]
    fn stream_symbol_survives_refresh_until_new_session() {
        let symbols: Vec<crate::http::models::Symbol> =
            serde_json::from_str(include_str!("../test_data/symbols.json")).unwrap();
        let (loaded, _) = crate::http::parse::parse_instruments(&symbols, UnixNanos::default());
        let original = loaded
            .into_iter()
            .find(|loaded| loaded.symbol_id == "COINBASE_SPOT_BTC_USD")
            .unwrap();

        let refreshed = CoinApiInstrument {
            symbol_id: "COINBASE_SPOT_BTC_USD_22F0062".into(),
            ..original.clone()
        };

        let latest = CoinApiInstrument {
            symbol_id: "COINBASE_SPOT_BTC_USD_22F0063".into(),
            ..original.clone()
        };

        let index = InstrumentIndex::default();
        let instrument_id = original.instrument.id();
        index.begin_stream(std::slice::from_ref(&original));
        index.insert(&refreshed);
        index.insert(&latest);

        assert_eq!(
            index.loaded(instrument_id).map(|(symbol_id, _)| symbol_id),
            Some(latest.symbol_id)
        );
        assert_eq!(
            index.stream_symbol_id(instrument_id),
            Some(original.symbol_id)
        );
        assert_eq!(
            index.instrument(original.symbol_id),
            Some(latest.instrument.clone())
        );
        assert_eq!(index.instrument(refreshed.symbol_id), None);
        assert_eq!(
            index.instrument(latest.symbol_id),
            Some(latest.instrument.clone())
        );

        index.begin_stream(std::slice::from_ref(&latest));

        assert_eq!(
            index.loaded(instrument_id).map(|(symbol_id, _)| symbol_id),
            Some(latest.symbol_id)
        );
        assert_eq!(
            index.stream_symbol_id(instrument_id),
            Some(latest.symbol_id)
        );
        assert_eq!(index.instrument(original.symbol_id), None);
        assert_eq!(index.instrument(latest.symbol_id), Some(latest.instrument));
    }

    #[rstest]
    fn concurrent_refresh_publishes_coherent_aliases() {
        let symbols: Vec<crate::http::models::Symbol> =
            serde_json::from_str(include_str!("../test_data/symbols.json")).unwrap();
        let (loaded, _) = crate::http::parse::parse_instruments(&symbols, UnixNanos::default());
        let original = loaded
            .into_iter()
            .find(|loaded| loaded.symbol_id == "COINBASE_SPOT_BTC_USD")
            .unwrap();
        let coarse: Vec<crate::http::models::Symbol> = serde_json::from_str(
            &include_str!("../test_data/symbols.json").replace("0.01000000000000000000", "0.1"),
        )
        .unwrap();
        let (coarse, _) = crate::http::parse::parse_instruments(&coarse, UnixNanos::default());
        let coarse = coarse
            .into_iter()
            .find(|loaded| loaded.symbol_id == original.symbol_id)
            .unwrap();

        let updates = [
            CoinApiInstrument {
                symbol_id: "COINBASE_SPOT_BTC_USD_22F0062".into(),
                ..coarse
            },
            CoinApiInstrument {
                symbol_id: "COINBASE_SPOT_BTC_USD_22F0063".into(),
                ..original.clone()
            },
        ];

        assert_eq!(updates[0].instrument.price_increment(), Price::from("0.1"));
        assert_eq!(updates[1].instrument.price_increment(), Price::from("0.01"));
        let index = InstrumentIndex::default();
        index.begin_stream(std::slice::from_ref(&original));
        let barrier = std::sync::Barrier::new(updates.len());
        std::thread::scope(|scope| {
            for update in &updates {
                scope.spawn(|| {
                    barrier.wait();

                    for _ in 0..1000 {
                        index.insert(update);
                        let state = index.state.load();
                        let symbol_id = state.symbol_ids[&original.instrument.id()];
                        assert_eq!(state.by_symbol_id.len(), 2);
                        assert_eq!(
                            serde_json::to_value(&state.by_symbol_id[&original.symbol_id]).unwrap(),
                            serde_json::to_value(&state.by_symbol_id[&symbol_id]).unwrap()
                        );
                        assert_eq!(
                            state.stream_symbol_ids[&original.instrument.id()],
                            original.symbol_id
                        );
                        let expected = updates
                            .iter()
                            .find(|update| update.symbol_id == symbol_id)
                            .unwrap();
                        assert_eq!(
                            serde_json::to_value(&state.by_symbol_id[&symbol_id]).unwrap(),
                            serde_json::to_value(&expected.instrument).unwrap()
                        );
                    }
                });
            }
        });
    }

    #[rstest]
    fn close_period_emits_latest_update_of_prior_period_once() {
        let mut cache = OhlcvCache::new();

        let first = close_period(&mut cache, update(10, "1"));
        let repeat = close_period(&mut cache, update(10, "2"));
        let next = close_period(&mut cache, update(11, "3"));
        let late = close_period(&mut cache, update(10, "4"));
        let skipped = close_period(&mut cache, update(13, "5"));

        assert_eq!(first, None);
        assert_eq!(repeat, None);
        assert_eq!(next, Some(update(10, "2")));
        assert_eq!(late, None);
        assert_eq!(skipped, Some(update(11, "3")));
        assert_eq!(cache.len(), 1);
        assert_eq!(cache.values().next(), Some(&update(13, "5")));
    }
}
