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

//! Live Kalshi data client.

pub(crate) mod parse;

mod history;
mod lifecycle;

use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::{Cache, CacheView},
    clients::{DataClient, execute_instrument_subscriptions},
    live::{dst::time, sender::EventSender},
    messages::{DataEvent, data::*},
};
use nautilus_core::{
    Params, UUID4,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::task::{TaskGroup, TaskGroupGuard, TaskSpawner};
use nautilus_model::{
    data::Data,
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{BinaryOption, Instrument, InstrumentAny},
    orderbook::OrderBook,
};
use tokio_util::sync::CancellationToken;

use self::lifecycle::LifecycleTracker;
pub use self::parse::{quote, trade};
use crate::{
    book::{BookSyncTracker, Levels},
    common::{consts::KALSHI_VENUE, parse::parse_ticker},
    config::KalshiDataClientConfig,
    http::client::KalshiHttpClient,
    providers::{KalshiInstrumentProvider, request_bounds},
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        messages::{KalshiChannel, KalshiDataMessage, KalshiSubscription, KalshiWsMessage},
    },
};

const TASK_GRACE: Duration = Duration::from_secs(5);
const TASK_FORCE: Duration = Duration::from_secs(2);

/// Streams selected binary markets as exact YES-priced L2 books, quotes, trades, status, and closes.
///
/// Connection publishes fresh definitions for every selected market before streaming. Subscriptions
/// are reference counted independently for books, quotes, and trades, and survive stop or disconnect.
/// Book, quote, and trade channels each share one wire subscription across the selected markets.
/// Per-market demand controls delivery; the wire subscription ends when its last reference is released.
/// Reset clears demand; disposal permanently closes admission. Ticker quotes require both prices
/// and positive sizes in the same message. Incomplete and empty sides are not carried forward.
#[derive(Debug)]
pub struct KalshiDataClient {
    clock: &'static AtomicTime,
    cache: CacheView,
    client_id: ClientId,
    config: KalshiDataClientConfig,
    provider: Arc<KalshiInstrumentProvider>,
    ws_client: Option<KalshiWebSocketClient>,
    connected: Arc<AtomicBool>,
    disposed: bool,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    sender: EventSender<DataEvent>,
    instruments: Vec<InstrumentId>,
    context: Arc<Mutex<HashMap<InstrumentId, BinaryOption>>>,
    demand: Arc<Mutex<HashMap<InstrumentId, Demand>>>,
    lifecycle: Arc<LifecycleTracker>,
    books: BookSyncTracker,
}

#[allow(
    clippy::missing_panics_doc,
    reason = "client state mutex poisoning is not expected"
)]
impl KalshiDataClient {
    /// Creates a disconnected client for a nonempty set of distinct native markets.
    ///
    /// The sender accepts the engine dispatch sender or a standalone Tokio sender converted with
    /// `into()`. Only selected markets can stream; explicit instrument requests remain fresh and
    /// preserve their original filters and correlation IDs.
    ///
    /// # Errors
    ///
    /// Returns an error if configuration is invalid, markets are empty, repeated, or foreign,
    /// or HTTP transport setup fails.
    pub fn new(
        client_id: ClientId,
        config: KalshiDataClientConfig,
        instruments: Vec<InstrumentId>,
        sender: EventSender<DataEvent>,
    ) -> anyhow::Result<Self> {
        config.validate()?;
        anyhow::ensure!(
            !instruments.is_empty(),
            "bootstrap markets must not be empty"
        );
        let mut selected = std::collections::HashSet::new();

        for id in &instruments {
            parse_ticker(*id)?;
            anyhow::ensure!(selected.insert(*id), "duplicate bootstrap market");
        }

        let http = KalshiHttpClient::new(&config)?;
        let clock = get_atomic_clock_realtime();
        let provider = KalshiInstrumentProvider::new(http, clock, sender.clone());

        let books = BookSyncTracker::new(
            &instruments,
            Duration::from_secs(config.book_snapshot_timeout_secs),
        );

        Ok(Self {
            clock,
            cache: Rc::new(RefCell::new(Cache::default())).into(),
            client_id,
            config,
            provider: Arc::new(provider),
            ws_client: None,
            connected: Arc::new(AtomicBool::new(false)),
            disposed: false,
            session_tasks: TaskGroup::new(),
            command_tasks: TaskGroup::new(),
            sender,
            instruments,
            context: Arc::default(),
            demand: Arc::default(),
            lifecycle: Arc::default(),
            books,
        })
    }

    pub(crate) fn with_runtime(
        client_id: ClientId,
        config: KalshiDataClientConfig,
        sender: EventSender<DataEvent>,
        cache: CacheView,
    ) -> anyhow::Result<Self> {
        let instruments = config.instrument_ids.clone();
        let mut client = Self::new(client_id, config, instruments, sender)?;
        client.cache = cache;
        client.require_cached_binary_options()?;
        Ok(client)
    }

    fn require_cached_binary_options(&self) -> anyhow::Result<()> {
        let cache = self.cache.borrow();
        for id in &self.instruments {
            if let Some(instrument) = cache.instrument(id) {
                anyhow::ensure!(
                    matches!(instrument, InstrumentAny::BinaryOption(_)) && instrument.id() == *id,
                    "cached instrument {id} is not a Kalshi binary option"
                );
            }
        }

        Ok(())
    }

    #[cfg(all(test, not(all(feature = "simulation", madsim))))]
    pub(crate) fn config(&self) -> &KalshiDataClientConfig {
        &self.config
    }

    #[cfg(all(test, not(all(feature = "simulation", madsim))))]
    pub(crate) fn instruments(&self) -> &[InstrumentId] {
        &self.instruments
    }

    #[cfg(all(test, not(all(feature = "simulation", madsim))))]
    pub(crate) fn cache(&self) -> &CacheView {
        &self.cache
    }

    fn subscribe_lifecycle(
        &self,
        id: InstrumentId,
        close: bool,
        subscribe: bool,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");
        anyhow::ensure!(!self.sender.is_closed(), "data event receiver closed");
        anyhow::ensure!(
            self.instruments.contains(&id),
            "instrument is not a selected bootstrap market"
        );
        self.lifecycle.subscribe(
            id,
            close,
            subscribe,
            &self.context,
            &self.sender,
            self.clock.get_time_ns(),
        )
    }

    fn subscribe_market(&self, id: InstrumentId, quotes: bool) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");
        anyhow::ensure!(
            self.instruments.contains(&id),
            "instrument is not a selected bootstrap market"
        );
        anyhow::ensure!(!self.sender.is_closed(), "data event receiver closed");
        let mut demand = self.demand.lock().expect("demand lock is not poisoned");
        let first = demand.values().all(|entry| entry.count(quotes) == 0);
        let entry = demand.entry(id).or_default();
        let count = entry.count_mut(quotes);
        let next = count
            .checked_add(1)
            .context("subscription reference count overflow")?;

        if first && let Some(ws_client) = &self.ws_client {
            let subscription = subscription(&self.instruments, quotes)?;
            if let Err(e) = ws_client.subscribe(subscription.clone()) {
                if let Err(e) = ws_client.unsubscribe(&subscription) {
                    log::warn!("Kalshi subscription rollback notification failed: {e}");
                }

                return Err(e);
            }
        }

        *count = next;
        Ok(())
    }

    fn unsubscribe_market(&self, id: InstrumentId, quotes: bool) -> anyhow::Result<()> {
        let mut demand = self.demand.lock().expect("demand lock is not poisoned");

        let Some(entry) = demand.get_mut(&id) else {
            return Ok(());
        };

        let count = entry.count_mut(quotes);
        if *count == 0 {
            return Ok(());
        }

        *count -= 1;
        let last = demand.values().all(|entry| entry.count(quotes) == 0);
        if last && let Some(ws_client) = &self.ws_client {
            let subscription = subscription(&self.instruments, quotes)?;
            ws_client.unsubscribe(&subscription)?;
        }

        Ok(())
    }

    fn spawn_request<F>(&self, future: F) -> anyhow::Result<()>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");
        anyhow::ensure!(!self.sender.is_closed(), "data event receiver closed");
        let cancellation = self.command_tasks.cancellation_token();

        self.command_tasks.spawn(async move {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {},
                result = future => if let Err(e) = result { log::warn!("Kalshi data request failed: {e}"); },
            }
        })?;

        Ok(())
    }

    fn begin_session_shutdown(&mut self) {
        self.connected.store(false, Ordering::Release);
        self.books.stop(false);
        self.session_tasks.begin_shutdown();
        self.ws_client = None;
    }

    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        let (session, commands) = tokio::join!(
            self.session_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE),
            self.command_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE)
        );
        session?;
        commands?;
        Ok(())
    }
}

macro_rules! unsupported {
    ($($name:ident($command:ty)),* $(,)?) => { $(
        fn $name(&mut self, _command: $command) -> anyhow::Result<()> {
            anyhow::bail!(concat!("Kalshi does not support ", stringify!($name)))
        }
    )* };
}

#[async_trait(?Send)]
impl DataClient for KalshiDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*KALSHI_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.begin_session_shutdown();
        self.command_tasks.begin_shutdown();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        self.books.stop(true);
        self.lifecycle.clear();
        self.demand
            .lock()
            .expect("demand lock is not poisoned")
            .clear();
        self.context
            .lock()
            .expect("context lock is not poisoned")
            .clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.reset()?;
        self.disposed = true;
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
            && !self.sender.is_closed()
            && self
                .ws_client
                .as_ref()
                .is_some_and(KalshiWebSocketClient::is_active)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");

        if self.is_connected() {
            return Ok(());
        }

        self.begin_session_shutdown();
        self.session_tasks
            .finish_shutdown(TASK_GRACE, TASK_FORCE)
            .await?;
        self.session_tasks.start_generation()?;

        if !self.command_tasks.is_open() {
            self.command_tasks
                .finish_shutdown(TASK_GRACE, TASK_FORCE)
                .await?;
            self.command_tasks.start_generation()?;
        }

        let connected = Arc::clone(&self.connected);
        let guard = TaskGroupGuard::new(&[&self.session_tasks], move || {
            connected.store(false, Ordering::Release);
        });

        self.lifecycle.begin_recovery();
        self.context
            .lock()
            .expect("context lock is not poisoned")
            .clear();
        refresh(
            &self.provider,
            &self.instruments,
            &self.context,
            &self.lifecycle,
            &self.sender,
            &self.session_tasks.cancellation_token(),
        )
        .await?;
        let (ws_client, receiver) = KalshiWebSocketClient::new(self.config.clone())?;
        ws_client.subscribe(KalshiSubscription {
            channel: KalshiChannel::MarketLifecycleV2,
            market_tickers: Vec::new(),
        })?;

        self.books.replay(&ws_client)?;
        {
            let demand = self.demand.lock().expect("demand lock is not poisoned");
            for quotes in [true, false] {
                if demand.values().any(|entry| entry.count(quotes) > 0) {
                    ws_client.subscribe(subscription(&self.instruments, quotes)?)?;
                }
            }
        }

        let session = Session {
            provider: Arc::clone(&self.provider),
            instruments: self.instruments.clone(),
            context: Arc::clone(&self.context),
            demand: Arc::clone(&self.demand),
            lifecycle: Arc::clone(&self.lifecycle),
            books: self.books.clone(),
            tasks: self.session_tasks.spawner()?,
            sender: self.sender.clone(),
            connected: Arc::clone(&self.connected),
            ws_client: ws_client.clone(),
            clock: self.clock,
            refresh_interval: Duration::from_secs(
                self.config.update_instruments_interval_mins * 60,
            ),
        };

        let (startup, initialized) = tokio::sync::oneshot::channel();

        self.session_tasks.spawn(session.run(
            receiver,
            self.session_tasks.cancellation_token(),
            startup,
        ))?;
        self.ws_client = Some(ws_client.clone());
        let result = async {
            ws_client.connect().await?;
            time::timeout(
                Duration::from_secs(self.config.ws_timeout_secs),
                initialized,
            )
            .await
            .context("initial lifecycle recovery timed out")?
            .context("initial lifecycle recovery stopped")??;
            anyhow::ensure!(
                !self.sender.is_closed(),
                "data event receiver closed during connect"
            );
            Ok(())
        }
        .await;

        if let Err(e) = result {
            drop(guard);
            self.begin_session_shutdown();
            self.session_tasks
                .finish_shutdown(TASK_GRACE, TASK_FORCE)
                .await
                .context("failed Kalshi startup cleanup")?;
            return Err(e);
        }

        self.connected.store(true, Ordering::Release);
        guard.disarm();
        log::info!("Connected {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        self.finish_shutdown().await?;
        log::info!("Disconnected {}", self.client_id);
        Ok(())
    }

    fn subscribe_quotes(&mut self, command: SubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_market(id, true)
        })
    }

    fn subscribe_trades(&mut self, command: SubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_market(id, false)
        })
    }

    fn unsubscribe_quotes(&mut self, command: &UnsubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.unsubscribe_market(id, true)
        })
    }

    fn unsubscribe_trades(&mut self, command: &UnsubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.unsubscribe_market(id, false)
        })
    }

    fn subscribe_book_deltas(&mut self, command: SubscribeBookDeltas) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi data client is disposed");
        anyhow::ensure!(!self.sender.is_closed(), "data event receiver closed");
        anyhow::ensure!(
            command.book_type == BookType::L2_MBP && command.depth.is_none(),
            "Kalshi streams full L2 market-by-price books only"
        );
        anyhow::ensure!(
            command
                .params
                .as_ref()
                .is_none_or(|params| params.is_empty()),
            "Kalshi book subscription parameters are unsupported"
        );
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            anyhow::ensure!(
                self.instruments.contains(&id),
                "instrument is not a selected bootstrap market"
            );
            self.books.subscribe(id, self.ws_client.as_ref())
        })
    }

    fn unsubscribe_book_deltas(&mut self, command: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.books.unsubscribe(id, self.ws_client.as_ref())
        })
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        let ticker = parse_ticker(instrument_id)?;
        let depth = request.depth.map_or(0, |depth| depth.get());
        anyhow::ensure!(depth <= 100, "Kalshi REST book depth exceeds 100");
        anyhow::ensure!(
            request
                .params
                .as_ref()
                .is_none_or(|params| params.is_empty()),
            "Kalshi book request parameters are unsupported"
        );
        let provider = Arc::clone(&self.provider);
        let sender = self.sender.clone();
        let client_id = self.client_id;
        self.spawn_request(async move {
            let instrument_request = RequestInstrument::new(
                instrument_id,
                None,
                None,
                None,
                UUID4::new(),
                request.ts_init,
                None,
            );

            let instrument = provider.load_instrument(&instrument_request).await?;
            let response = provider.http.get_orderbook(&ticker, depth as u32).await?;
            let now = provider.clock.get_time_ns();
            let deltas = Levels::rest(&response.orderbook_fp, &instrument, now)?;
            let mut book = OrderBook::new(instrument.id, BookType::L2_MBP);
            book.apply_deltas(&deltas)?;

            let response = BookResponse::new(
                request.request_id,
                request.client_id.unwrap_or(client_id),
                instrument_id,
                book,
                None,
                None,
                now,
                request.params,
            );

            sender
                .send(DataEvent::Response(DataResponse::Book(response)))
                .map_err(|_| anyhow::anyhow!("book response receiver closed"))?;
            Ok(())
        })
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        let ticker = parse_ticker(instrument_id)?;
        let now = self.clock.get_time_ns();

        let range = history::HistoryRange::new(
            request.start,
            request.end,
            request.limit,
            request.params.as_ref(),
            now,
        )?;

        let (start, end) = request_bounds(request.start, request.end)?;
        let provider = Arc::clone(&self.provider);
        let client_id = request.client_id.unwrap_or(self.client_id);
        let sender = self.sender.clone();
        self.spawn_request(async move {
            let metadata =
                RequestInstrument::new(instrument_id, None, None, None, UUID4::new(), now, None);
            let instrument = provider.load_instrument(&metadata).await?;
            let records = provider
                .http
                .get_trades_range(range.trades_query(ticker), range.max_pages)
                .await?;
            let mut trades = Vec::new();

            for record in records {
                let tick = history::trade(record, &instrument)?;
                if range.contains(tick.ts_event) {
                    trades.push(tick);
                }
            }

            range.truncate(&mut trades);

            let response = TradesResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                trades,
                start,
                end,
                now,
                request.params,
            );

            sender
                .send(DataEvent::Response(DataResponse::Trades(response)))
                .map_err(|_| anyhow::anyhow!("trade response receiver closed"))?;
            Ok(())
        })
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.single_bar_type()?;
        let id = bar_type.instrument_id();
        parse_ticker(id)?;
        let period = history::period(bar_type)?;
        let now = self.clock.get_time_ns();

        let range = history::HistoryRange::new(
            request.start,
            request.end,
            request.limit,
            request.params.as_ref(),
            now,
        )?;

        let (start, end) = request_bounds(request.start, request.end)?;
        let provider = Arc::clone(&self.provider);
        let client_id = request.client_id.unwrap_or(self.client_id);
        let sender = self.sender.clone();
        self.spawn_request(async move {
            let metadata = RequestInstrument::new(id, None, None, None, UUID4::new(), now, None);
            let instrument = provider.load_instrument(&metadata).await?;
            let info = instrument
                .info
                .as_ref()
                .context("missing instrument metadata")?;
            let market = serde_json::from_value(
                info.get("market")
                    .context("missing market metadata")?
                    .clone(),
            )?;
            let series = info
                .get("event")
                .and_then(|v| v.get("series_ticker"))
                .and_then(serde_json::Value::as_str)
                .context("missing series identity")?;
            let records = provider
                .http
                .get_candlesticks_range(&market, series, &range.candles_query(period))
                .await?;
            let mut bars = Vec::new();

            for record in records {
                let ts_event = history::candle_time(record.end_period_ts)?;
                if !range.contains(ts_event) || ts_event > now {
                    continue;
                }

                if let Some(bar) = history::bar(record, bar_type, &instrument)? {
                    bars.push(bar);
                }
            }

            range.truncate(&mut bars);

            let response = BarsResponse::new(
                request.request_id,
                client_id,
                bar_type,
                bars,
                start,
                end,
                now,
                request.params,
            );

            sender
                .send(DataEvent::Response(DataResponse::Bars(response)))
                .map_err(|_| anyhow::anyhow!("bar response receiver closed"))?;
            Ok(())
        })
    }

    fn request_quotes(&self, _request: RequestQuotes) -> anyhow::Result<()> {
        anyhow::bail!("Kalshi does not provide historical quotes")
    }

    fn request_instrument(&self, mut request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        parse_ticker(instrument_id)?;
        request.client_id.get_or_insert(self.client_id);
        let provider = Arc::clone(&self.provider);
        let selected = self.instruments.contains(&instrument_id);
        let context = Arc::clone(&self.context);
        let sender = self.sender.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let stamp = lifecycle.begin_read();
        self.spawn_request(async move {
            let instrument = provider.load_instrument(&request).await?;
            if selected {
                lifecycle.apply(instrument.clone(), stamp, &context, &sender)?;
            }

            let guard = if selected {
                Some(lifecycle.response_guard(&[&instrument])?)
            } else {
                None
            };

            let result = provider.respond_instrument(request, instrument);
            drop(guard);
            result
        })
    }

    fn request_instruments(&self, mut request: RequestInstruments) -> anyhow::Result<()> {
        anyhow::ensure!(
            request.venue.is_none_or(|v| v == *KALSHI_VENUE),
            "instrument request venue must be KALSHI"
        );
        request.client_id.get_or_insert(self.client_id);
        let provider = Arc::clone(&self.provider);
        let selected = self.instruments.clone();
        let context = Arc::clone(&self.context);
        let sender = self.sender.clone();
        let lifecycle = Arc::clone(&self.lifecycle);
        let stamp = lifecycle.begin_read();
        self.spawn_request(async move {
            let mut scoped = request.clone();

            if !selected.is_empty()
                && scoped.params.as_ref().is_none_or(|params| {
                    [
                        "tickers",
                        "event_ticker",
                        "series_ticker",
                        "min_updated_ts",
                        "max_updated_ts",
                    ]
                    .iter()
                    .all(|key| !params.contains_key(*key))
                })
            {
                let tickers = selected
                    .iter()
                    .map(|id| id.symbol.as_str())
                    .collect::<Vec<_>>()
                    .join(",");
                let mut params = scoped
                    .params
                    .as_ref()
                    .map(Params::to_shared_map)
                    .unwrap_or_default();
                let tickers = Arc::new(tickers.into());
                params.insert("tickers".into(), tickers);
                let params = Params::from_shared(params);
                scoped.params = Some(params);
            }

            let instruments = provider.load_instruments(&scoped).await?;
            for instrument in &instruments {
                if let InstrumentAny::BinaryOption(instrument) = instrument
                    && selected.contains(&instrument.id)
                {
                    lifecycle.apply(instrument.clone(), stamp, &context, &sender)?;
                }
            }

            let definitions: Vec<_> = instruments
                .iter()
                .filter_map(|instrument| match instrument {
                    InstrumentAny::BinaryOption(instrument)
                        if selected.contains(&instrument.id) =>
                    {
                        Some(instrument)
                    }
                    _ => None,
                })
                .collect();

            let guard = lifecycle.response_guard(&definitions)?;
            drop(definitions);
            let result = provider.respond_instruments(request, instruments);
            drop(guard);
            result
        })
    }

    fn subscribe_instrument_status(
        &mut self,
        command: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_lifecycle(id, false, true)
        })
    }

    fn unsubscribe_instrument_status(
        &mut self,
        command: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_lifecycle(id, false, false)
        })
    }

    fn subscribe_instrument_close(
        &mut self,
        command: SubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_lifecycle(id, true, true)
        })
    }

    fn unsubscribe_instrument_close(
        &mut self,
        command: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&command.instrument_ids, |id| {
            self.subscribe_lifecycle(id, true, false)
        })
    }

    unsupported! {
        subscribe(SubscribeCustomData), subscribe_instruments(SubscribeInstruments), subscribe_instrument(SubscribeInstrument),
        subscribe_book_depth(SubscribeBookDepth),
        subscribe_mark_prices(SubscribeMarkPrices), subscribe_index_prices(SubscribeIndexPrices), subscribe_funding_rates(SubscribeFundingRates),
        subscribe_bars(SubscribeBars), subscribe_option_greeks(SubscribeOptionGreeks),
        unsubscribe(&UnsubscribeCustomData), unsubscribe_instruments(&UnsubscribeInstruments), unsubscribe_instrument(&UnsubscribeInstrument),
        unsubscribe_book_depth(&UnsubscribeBookDepth),
        unsubscribe_mark_prices(&UnsubscribeMarkPrices), unsubscribe_index_prices(&UnsubscribeIndexPrices), unsubscribe_funding_rates(&UnsubscribeFundingRates),
        unsubscribe_bars(&UnsubscribeBars), unsubscribe_option_greeks(&UnsubscribeOptionGreeks),
    }
}

#[derive(Debug, Default)]
struct Demand {
    quotes: usize,
    trades: usize,
}

impl Demand {
    fn count_mut(&mut self, quotes: bool) -> &mut usize {
        if quotes {
            &mut self.quotes
        } else {
            &mut self.trades
        }
    }

    fn count(&self, quotes: bool) -> usize {
        if quotes { self.quotes } else { self.trades }
    }
}

struct Session {
    provider: Arc<KalshiInstrumentProvider>,
    instruments: Vec<InstrumentId>,
    context: Arc<Mutex<HashMap<InstrumentId, BinaryOption>>>,
    demand: Arc<Mutex<HashMap<InstrumentId, Demand>>>,
    lifecycle: Arc<LifecycleTracker>,
    books: BookSyncTracker,
    tasks: TaskSpawner,
    sender: EventSender<DataEvent>,
    connected: Arc<AtomicBool>,
    ws_client: KalshiWebSocketClient,
    clock: &'static AtomicTime,
    refresh_interval: Duration,
}

impl Session {
    async fn run(
        self,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
        cancellation: CancellationToken,
        startup: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    ) {
        let mut startup = Some(startup);
        let result = self
            .receive(&mut receiver, &cancellation, &mut startup)
            .await;

        if let Some(startup) = startup {
            let _ = startup.send(Err(anyhow::anyhow!("initial lifecycle recovery failed")));
        }

        self.connected.store(false, Ordering::Release);
        self.books.stop(false);

        if let Err(e) = result {
            log::warn!("Kalshi data stream stopped: {e}");
        }

        if let Err(e) = self.ws_client.stop().await {
            log::warn!("Kalshi WebSocket shutdown failed: {e}");
        }
    }

    async fn receive(
        &self,
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
        cancellation: &CancellationToken,
        startup: &mut Option<tokio::sync::oneshot::Sender<anyhow::Result<()>>>,
    ) -> anyhow::Result<()> {
        let mut refresh_tick = time::interval(self.refresh_interval);
        refresh_tick.tick().await;
        let mut health_tick = time::interval(Duration::from_millis(100));
        health_tick.set_missed_tick_behavior(time::MissedTickBehavior::Skip);
        let mut generation = None;
        let mut lifecycle_sequence = None;
        let mut lifecycle_recovered = false;

        loop {
            let event = tokio::select! {
                biased;
                () = cancellation.cancelled() => return Ok(()),
                _ = health_tick.tick() => { anyhow::ensure!(!self.sender.is_closed(), "data event receiver closed"); self.books.monitor(&self.ws_client, &self.tasks); self.lifecycle.tick(&self.context, &self.sender, self.clock.get_time_ns())?; continue; },
                _ = refresh_tick.tick() => { self.refresh(&mut refresh_tick, cancellation).await?; continue; },
                event = receiver.recv() => event.context("WebSocket event receiver closed")?,
            };

            let current = event.generation();

            if generation.is_some_and(|generation| current < generation) {
                continue;
            }

            let new_generation = generation != Some(current);
            if new_generation {
                self.lifecycle.begin_recovery();
                self.refresh(&mut refresh_tick, cancellation).await?;
                generation = Some(current);
                lifecycle_sequence = None;
                lifecycle_recovered = false;
            }

            if matches!(&event, KalshiWsEvent::Message { message: KalshiWsMessage::Subscribed { msg, .. }, .. } if msg.channel == KalshiChannel::MarketLifecycleV2)
            {
                if !new_generation {
                    self.refresh(&mut refresh_tick, cancellation).await?;
                }

                lifecycle_recovered = true;
            }

            if matches!(&event, KalshiWsEvent::Ready { .. }) {
                if !lifecycle_recovered {
                    self.refresh(&mut refresh_tick, cancellation).await?;
                }

                lifecycle_recovered = true;

                if let Some(startup) = startup.take() {
                    let _ = startup.send(Ok(()));
                }
            }

            if !self
                .sequence(
                    &event,
                    &mut lifecycle_sequence,
                    &mut refresh_tick,
                    cancellation,
                )
                .await?
            {
                continue;
            }

            self.books.event(
                &event,
                &self.context.lock().expect("context lock is not poisoned"),
                &self.sender,
                self.clock.get_time_ns(),
            )?;

            match event {
                KalshiWsEvent::Message {
                    message: KalshiWsMessage::MarketLifecycleV2(data),
                    ..
                } => {
                    self.receive_lifecycle(&data, &mut refresh_tick, cancellation)
                        .await?;
                }
                KalshiWsEvent::Message { message, .. } => self.dispatch(message)?,
                KalshiWsEvent::Error { message, .. } => log::warn!("Kalshi WebSocket: {message}"),
                KalshiWsEvent::Ready { .. }
                | KalshiWsEvent::BookSequenceGap { .. }
                | KalshiWsEvent::BookSnapshotRequested { .. } => {}
            }
        }
    }

    async fn receive_lifecycle(
        &self,
        data: &KalshiDataMessage,
        refresh_tick: &mut time::Interval,
        cancellation: &CancellationToken,
    ) -> anyhow::Result<()> {
        if let Some(id) = self
            .instruments
            .iter()
            .find(|id| Some(id.symbol.as_str()) == data.msg["market_ticker"].as_str())
        {
            self.lifecycle.observe(
                *id,
                data,
                &self.context,
                &self.sender,
                self.clock.get_time_ns(),
            )?;
            refresh(
                &self.provider,
                &[*id],
                &self.context,
                &self.lifecycle,
                &self.sender,
                cancellation,
            )
            .await?;
            refresh_tick.reset();

            if data.msg["event_type"] == "settled" {
                self.recheck_settlement(*id)?;
            }
        }

        Ok(())
    }

    async fn sequence(
        &self,
        event: &KalshiWsEvent,
        lifecycle_sequence: &mut Option<(u64, u64)>,
        refresh_tick: &mut time::Interval,
        cancellation: &CancellationToken,
    ) -> anyhow::Result<bool> {
        if let KalshiWsEvent::Message { message, .. } = event
            && let Some((KalshiChannel::MarketLifecycleV2, data)) = message.data()
            && let Some(seq) = data.seq
        {
            if let Some((sid, previous)) = *lifecycle_sequence
                && sid == data.sid
            {
                if seq <= previous {
                    return Ok(false);
                }

                if seq != previous + 1 {
                    self.refresh(refresh_tick, cancellation).await?;
                }
            }

            *lifecycle_sequence = Some((data.sid, seq));
        }

        Ok(true)
    }

    async fn refresh(
        &self,
        refresh_tick: &mut time::Interval,
        cancellation: &CancellationToken,
    ) -> anyhow::Result<()> {
        refresh(
            &self.provider,
            &self.instruments,
            &self.context,
            &self.lifecycle,
            &self.sender,
            cancellation,
        )
        .await?;
        refresh_tick.reset();
        Ok(())
    }

    fn recheck_settlement(&self, id: InstrumentId) -> anyhow::Result<()> {
        let provider = Arc::clone(&self.provider);
        let context = Arc::clone(&self.context);
        let lifecycle = Arc::clone(&self.lifecycle);
        let sender = self.sender.clone();
        let cancellation = self.tasks.cancellation_token();

        self.tasks.spawn(async move {
            for millis in [250, 500, 1_000, 2_000] {
                let finalized = context
                    .lock()
                    .expect("context lock is not poisoned")
                    .get(&id)
                    .and_then(|instrument| instrument.info.as_ref())
                    .is_some_and(|info| info["market"]["status"] == "finalized");

                if finalized {
                    return;
                }

                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => return,
                    () = time::sleep(Duration::from_millis(millis)) => {}
                }

                if let Err(e) = refresh(
                    &provider,
                    &[id],
                    &context,
                    &lifecycle,
                    &sender,
                    &cancellation,
                )
                .await
                {
                    if !cancellation.is_cancelled() {
                        log::warn!("Kalshi final settlement recheck failed: {e}");
                    }

                    return;
                }
            }
        })?;

        Ok(())
    }

    fn dispatch(&self, message: KalshiWsMessage) -> anyhow::Result<()> {
        let (data, quotes) = match message {
            KalshiWsMessage::Ticker(data) => (data, true),
            KalshiWsMessage::Trade(data) => (data, false),
            _ => return Ok(()),
        };

        let Some(ticker) = data
            .msg
            .get("market_ticker")
            .and_then(serde_json::Value::as_str)
        else {
            log::warn!("Kalshi data frame has no market ticker");
            return Ok(());
        };

        let Some(id) = self
            .instruments
            .iter()
            .find(|id| id.symbol.as_str() == ticker)
            .copied()
        else {
            return Ok(());
        };

        let demanded = self
            .demand
            .lock()
            .expect("demand lock is not poisoned")
            .get(&id)
            .is_some_and(|demand| {
                if quotes {
                    demand.quotes > 0
                } else {
                    demand.trades > 0
                }
            });

        if !demanded {
            return Ok(());
        }

        let ts_init = self.clock.get_time_ns();

        let result = {
            let context = self.context.lock().expect("context lock is not poisoned");

            let Some(instrument) = context.get(&id) else {
                return Ok(());
            };

            if quotes {
                parse::quote(data.msg, instrument, ts_init).map(|tick| tick.map(Data::Quote))
            } else {
                parse::trade(data.msg, instrument, ts_init).map(|tick| tick.map(Data::Trade))
            }
        };

        match result {
            Ok(Some(data)) => self
                .sender
                .send(DataEvent::Data(data))
                .map_err(|_| anyhow::anyhow!("data event receiver closed"))?,
            Ok(None) => {}
            Err(e) => log::warn!("Invalid Kalshi market data: {e}"),
        }

        Ok(())
    }
}

fn subscription(ids: &[InstrumentId], quotes: bool) -> anyhow::Result<KalshiSubscription> {
    Ok(KalshiSubscription {
        channel: if quotes {
            KalshiChannel::Ticker
        } else {
            KalshiChannel::Trade
        },
        market_tickers: ids
            .iter()
            .map(|id| parse_ticker(*id))
            .collect::<anyhow::Result<_>>()?,
    })
}

async fn refresh(
    provider: &KalshiInstrumentProvider,
    ids: &[InstrumentId],
    context: &Mutex<HashMap<InstrumentId, BinaryOption>>,
    lifecycle: &LifecycleTracker,
    sender: &EventSender<DataEvent>,
    cancellation: &CancellationToken,
) -> anyhow::Result<()> {
    let stamp = lifecycle.begin_read();

    for id in ids {
        let request = RequestInstrument::new(
            *id,
            None,
            None,
            None,
            UUID4::new(),
            provider.clock.get_time_ns(),
            None,
        );

        let instrument = tokio::select! {
            biased;
            () = cancellation.cancelled() => anyhow::bail!("metadata refresh canceled"),
            result = provider.load_instrument(&request) => result?,
        };
        lifecycle.apply(instrument, stamp, context, sender)?;
    }

    Ok(())
}

#[cfg(test)]
#[cfg(not(all(feature = "simulation", madsim)))]
mod tests {
    use std::{
        process::Command,
        sync::{Barrier, atomic::AtomicUsize},
    };

    use nautilus_core::UnixNanos;
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::{
        common::{consts::KALSHI_CLIENT_ID, parse::parse_instrument_id},
        http::{
            models::{Event, Market},
            parse::parse_instrument,
        },
        websocket::messages::KalshiDataMessage,
    };

    #[allow(
        dead_code,
        reason = "shared key fixtures serve both session and integration peers"
    )]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }
    mod http_request {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/http_request.rs"
        ));
    }
    #[allow(
        dead_code,
        reason = "shared peer also serves the complete data-client integration suite"
    )]
    mod peer {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/common/data_peer.rs"
        ));
    }

    #[tokio::test]
    async fn slow_periodic_refresh_yields_to_queued_market_data() {
        let peer = peer::Peer::new().await;
        peer.delay_millis.store(10, Ordering::SeqCst);
        let fixtures: Value =
            serde_json::from_str(include_str!("../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let id = instrument.id;
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<DataEvent> = sender.into();
        let clock = get_atomic_clock_realtime();

        let provider = Arc::new(KalshiInstrumentProvider::new(
            KalshiHttpClient::new(&peer.config).unwrap(),
            clock,
            sender.clone(),
        ));

        let (ws_client, _events) = KalshiWebSocketClient::new(peer.config.clone()).unwrap();

        let tasks = TaskGroup::new();

        let session = Session {
            provider,
            lifecycle: Arc::default(),
            books: BookSyncTracker::new(&[id], Duration::from_secs(10)),
            tasks: tasks.spawner().unwrap(),
            instruments: vec![id],
            context: Arc::new(Mutex::new(HashMap::from([(id, instrument)]))),
            demand: Arc::new(Mutex::new(HashMap::from([(
                id,
                Demand {
                    quotes: 0,
                    trades: 1,
                },
            )]))),
            sender,
            connected: Arc::new(AtomicBool::new(true)),
            ws_client,
            clock,
            refresh_interval: Duration::from_millis(1),
        };

        let (input, receiver) = tokio::sync::mpsc::unbounded_channel();
        let cancellation = CancellationToken::new();

        // Keep the session on the test's local runtime
        let session = session.run(
            receiver,
            cancellation.clone(),
            tokio::sync::oneshot::channel().0,
        );

        let run = tokio::spawn(session); // tokio-import-ok

        let result = time::timeout(Duration::from_secs(1), async {
            while peer.requests.load(Ordering::SeqCst) == 0 {
                tokio::task::yield_now().await;
            }

            let payloads: Value =
                serde_json::from_str(include_str!("../test_data/market_data.json")).unwrap();
            input
                .send(KalshiWsEvent::Message {
                    generation: 0,
                    message: KalshiWsMessage::Trade(KalshiDataMessage {
                        id: None,
                        sid: 1,
                        seq: Some(1),
                        sending_ts_ms: None,
                        msg: payloads["trade"].clone(),
                    }),
                })
                .unwrap();

            loop {
                if let DataEvent::Data(Data::Trade(tick)) = output.recv().await.unwrap() {
                    return tick;
                }
            }
        })
        .await;

        cancellation.cancel();
        run.await.unwrap();
        let tick = result.expect("queued trade must progress after a slow metadata refresh");
        assert_eq!(tick.instrument_id, id);
        assert_eq!(tick.size.as_decimal(), "7.25".parse().unwrap());
        assert_eq!(tick.price.as_decimal(), "0.123456".parse().unwrap());
    }

    #[rstest]
    fn canceled_ready_requests_do_not_poll_their_work() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "data::tests::canceled_ready_requests_child",
                "--test-threads=1",
            ])
            .env("NAUTILUS_KALSHI_CANCEL_TEST", "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("1 passed; 0 failed;"));
    }

    #[rstest]
    fn canceled_ready_requests_child() {
        if std::env::var_os("NAUTILUS_KALSHI_CANCEL_TEST").is_none() {
            return;
        }

        let runtime = tokio::runtime::Builder::new_multi_thread()
            .worker_threads(1)
            .enable_all()
            .build()
            .unwrap();
        nautilus_common::live::set_runtime(runtime).unwrap();
        let (blocked, observed) = std::sync::mpsc::channel();
        let gate = Arc::new(Barrier::new(2));
        let blocker_gate = Arc::clone(&gate);

        let blocker = nautilus_common::live::get_runtime().spawn(async move {
            blocked.send(()).unwrap();
            blocker_gate.wait();
        });

        observed.recv_timeout(Duration::from_secs(1)).unwrap();
        let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
        let id = parse_instrument_id("MARKET-A").unwrap();
        let mut client = KalshiDataClient::new(
            *KALSHI_CLIENT_ID,
            KalshiDataClientConfig::default(),
            vec![id],
            sender.into(),
        )
        .unwrap();
        let polled = Arc::new(AtomicUsize::new(0));
        for _ in 0..64 {
            let polled = Arc::clone(&polled);
            client
                .spawn_request(async move {
                    polled.fetch_add(1, Ordering::SeqCst);
                    Ok(())
                })
                .unwrap();
        }

        client.stop().unwrap();
        gate.wait();
        nautilus_common::live::get_runtime().block_on(async {
            client.disconnect().await.unwrap();
            blocker.await.unwrap();
        });

        assert_eq!(polled.load(Ordering::SeqCst), 0);
    }
}
