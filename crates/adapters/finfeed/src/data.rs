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

//! Data client with generation-owned requests, polling subscriptions, and bounded shutdown.

mod book;
mod history;
mod parse;
mod poll;

use std::{collections::HashMap, future::Future, num::NonZeroUsize, sync::Arc, time::Duration};

use anyhow::Context;
use async_trait::async_trait;
use jiff::Timestamp;
use nautilus_common::{
    clients::{DataClient, execute_bar_subscriptions, execute_instrument_subscriptions},
    live::{dst::time, sender::EventSender},
    messages::{DataEvent, DataResponse, data::*},
};
use nautilus_core::{
    Params,
    datetime::datetime_to_unix_nanos,
    string::secret::SecretString,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::task::{TaskGroup, TaskGroupGuard};
use nautilus_model::{
    data::depth::DEPTH10_LEN,
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{BinaryOption, InstrumentAny},
};
use parking_lot::Mutex;

use crate::{
    common::{consts::FINFEED, credential::resolve_api_key},
    config::FinFeedDataClientConfig,
    http::{client::FinFeedHttpClient, error::Error, query::PAGE_LIMIT_MAX},
    instruments::{FinFeedInstrumentProvider, selected_ids},
};

const TASK_GRACE: Duration = Duration::from_secs(1);
const TASK_FORCE: Duration = Duration::from_secs(2);

/// Prediction-market data client for requests and book, quote, trade, and bar polling subscriptions.
/// Execution is unsupported. The injected sender supports standalone clients and runtime dispatch.
///
/// # Connection Lifecycle
///
/// Startup publishes every selected definition before readiness. Disconnect closes task admission
/// and response publication before draining work. Reconnect uses fresh transport and instrument
/// state after the previous generation drains.
///
/// # Polling Subscriptions
///
/// - Quotes and trades share sampled latest-activity polls.
/// - L2 MBP deltas and depth share validated current-book snapshots.
/// - Bars publish periods closing after subscribe time, with a cutoff taken before each request.
///   Their publication watermark survives reconnects. Provider OHLCV maps to `LAST` for compatibility;
///   trade-only price provenance is unverified.
///
/// Polling guarantees neither completeness nor backfill.
///
/// # Market Status and Definition Refreshes
///
/// Fresh definition requests apply provider lifecycle metadata and publish instrument updates and
/// correlated responses.
///
/// - `Closed` and `Resolved` end all polling subscriptions and reject new ones for that outcome.
/// - `Open` and `Suspended` keep polling.
///
/// If any selected definition fails to refresh, list refreshes preserve loaded definitions and
/// outcome ownership. Historical requests remain available for loaded terminal definitions.
/// The client infers no `InstrumentStatus` or `InstrumentClose` event.
///
/// # Historical Requests
///
/// Historical series require a start and share the configured attempt budget.
///
/// - Trades and quotes retrieve whole dates, then filter and order records by exposed timestamps.
///   Saturated responses fail without inferred continuation. Completeness under provider
///   processing-time selection remains unverified.
/// - Quotes use entry time for `ts_event` and receive time for `ts_init`.
/// - Trades use their sole timestamp for both fields and infer no aggressor side.
/// - Bars use complete external OHLCV periods contained in the requested window.
///
/// # Historical Books
///
/// Historical L2 books retrieve UTC hours and replay returned records from a snapshot seed assumed
/// to be complete. Source coverage and continuity remain assumptions.
///
/// - Numeric actions `0`, `1`, and `3` mean relative ADD, absolute SET, and replacement snapshot.
/// - Missing receive clocks use exchange time.
/// - Series begin with a generated replacement at start. Point snapshots select an `end` timestamp
///   in request params.
/// - Series reject emitted receive clocks outside the window or out of order.
/// - Saturated responses, missing seeds, and unsupported actions fail.
///
/// # Validation and Failures
///
/// Current books are validated atomically. Prices and raw sizes must fit the loaded definition
/// exactly; OHLCV volumes allow bounded normalization of numeric tails at size precision.
/// Asynchronous failures log an error and publish no response. Synchronous validation failures
/// return an error.
#[derive(Debug)]
pub struct FinFeedDataClient {
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: FinFeedDataClientConfig,
    api_key: SecretString,
    http: Arc<FinFeedHttpClient>,
    provider: Arc<tokio::sync::Mutex<FinFeedInstrumentProvider>>,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    publication: Arc<Mutex<Publication>>,
    sender: EventSender<DataEvent>,
    poll_changed: Arc<tokio::sync::Notify>,
}

impl FinFeedDataClient {
    /// Constructs a disconnected client with explicit event plumbing.
    ///
    /// The bootstrap deadline must be in `[1, 86400]` seconds. Poll spacing must be in
    /// `[100, 86400000]` milliseconds and the per-list poll limit in `[1, 10000]`.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - Selections, credentials, REST controls, the bootstrap deadline, or polling bounds are invalid.
    /// - Neither an explicit key nor `FINFEED_API_KEY` is available.
    pub fn new(
        client_id: ClientId,
        config: FinFeedDataClientConfig,
        sender: EventSender<DataEvent>,
    ) -> anyhow::Result<Self> {
        selected_ids(&config.instruments)?;
        anyhow::ensure!(
            (1..=86_400).contains(&config.connect_timeout_secs),
            "invalid FinFeed connect deadline: connect_timeout_secs must be in [1, 86400], was {}",
            config.connect_timeout_secs
        );
        anyhow::ensure!(
            (100..=86_400_000).contains(&config.poll_interval_ms),
            "invalid FinFeed polling controls: poll_interval_ms must be in [100, 86400000], was {}",
            config.poll_interval_ms
        );
        anyhow::ensure!(
            (1..=PAGE_LIMIT_MAX).contains(&config.poll_limit),
            "invalid FinFeed polling controls: poll_limit must be in [1, {PAGE_LIMIT_MAX}], was {}",
            config.poll_limit
        );
        let api_key = resolve_api_key(config.api_key.clone())?;

        let http = Arc::new(FinFeedHttpClient::new(
            Some(api_key.clone()),
            config.http.clone(),
        )?);
        let provider = FinFeedInstrumentProvider::new(Arc::clone(&http));
        Ok(Self {
            clock: get_atomic_clock_realtime(),
            client_id,
            config,
            api_key,
            http,
            provider: Arc::new(tokio::sync::Mutex::new(provider)),
            session_tasks: TaskGroup::new(),
            command_tasks: TaskGroup::new(),
            publication: Arc::new(Mutex::new(Publication::default())),
            sender,
            poll_changed: Arc::new(tokio::sync::Notify::new()),
        })
    }
}

#[async_trait(?Send)]
impl DataClient for FinFeedDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        None
    }

    fn start(&mut self) -> anyhow::Result<()> {
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.begin_shutdown();
        self.session_tasks.abort();
        self.command_tasks.abort();
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        let mut state = self.publication.lock();
        state.instruments.clear();
        state.polling = poll::Polling::default();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.reset()
    }

    fn is_connected(&self) -> bool {
        self.publication.lock().connected
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        if !self.session_tasks.is_open() || !self.command_tasks.is_open() {
            self.begin_shutdown();
            self.finish_shutdown().await?;

            self.http = Arc::new(FinFeedHttpClient::new(
                Some(self.api_key.clone()),
                self.config.http.clone(),
            )?);
            self.session_tasks.start_generation()?;
            self.command_tasks.start_generation()?;
        }

        let http = Arc::clone(&self.http);
        let publication = Arc::clone(&self.publication);
        let guard = TaskGroupGuard::new(&[&self.session_tasks, &self.command_tasks], move || {
            publication.lock().connected = false;
            http.cancel();
        });

        let mut provider = FinFeedInstrumentProvider::new(Arc::clone(&self.http));
        let intended = self.publication.lock().polling.instrument_ids();

        let loaded = time::timeout(
            Duration::from_secs(self.config.connect_timeout_secs),
            Box::pin(async {
                let mut loaded = provider
                    .load(&self.config.instruments, self.clock.get_time_ns())
                    .await?;

                for id in intended {
                    if !loaded.iter().any(|instrument| instrument.id == id) {
                        loaded.push(
                            provider
                                .request_instrument(id, self.clock.get_time_ns())
                                .await?,
                        );
                    }
                }

                Ok::<_, crate::http::error::Error>(loaded)
            }),
        )
        .await
        .context("FinFeed bootstrap timed out")?;

        if let Err(Error::Http { status, .. }) = &loaded {
            log::error!("FinFeed bootstrap failed with HTTP status {status}");
        }

        let loaded = loaded?;
        {
            let mut state = self.publication.lock();

            for instrument in &loaded {
                let instrument = InstrumentAny::from(instrument.clone());
                send(&self.sender, DataEvent::Data(instrument.into()))?;
            }

            state.instruments.clear();
            for instrument in loaded {
                state.update(instrument);
            }

            self.provider = Arc::new(tokio::sync::Mutex::new(provider));
            state.connected = true;
            state.polling.restore();
        }

        let publication = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let http = Arc::clone(&self.http);
        let changed = Arc::clone(&self.poll_changed);
        let cancellation = self.session_tasks.cancellation_token();
        let interval = Duration::from_millis(self.config.poll_interval_ms);
        let limit = self.config.poll_limit;

        self.session_tasks.spawn(async move {
            poll::run(
                publication,
                sender,
                http,
                changed,
                cancellation,
                interval,
                limit,
            )
            .await;
        })?;

        guard.disarm();

        log::info!("Connected {}", self.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.begin_shutdown();
        self.finish_shutdown().await?;
        self.publication.lock().instruments.clear();

        log::info!("Disconnected {}", self.client_id);
        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        crate::common::symbol::from_instrument_id(instrument_id)?;
        let provider = Arc::clone(&self.provider);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let changed = Arc::clone(&self.poll_changed);
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed instrument request", async move {
            let mut provider = provider.lock().await;
            let instrument = provider
                .request_instrument(instrument_id, clock.get_time_ns())
                .await?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                instrument.clone().into(),
                start,
                end,
                clock.get_time_ns(),
                request.params,
            )));
            publish(&state, &sender, response, vec![instrument])?;
            changed.notify_one();
            Ok(())
        })
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let ids: Vec<_> = selected_ids(&self.config.instruments)?
            .into_iter()
            .filter(|id| request.venue.is_none_or(|venue| id.venue == venue))
            .collect();
        let provider = Arc::clone(&self.provider);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = request.venue.unwrap_or_else(|| Venue::from(FINFEED));
        let changed = Arc::clone(&self.poll_changed);
        self.spawn_request("FinFeed instruments request", async move {
            let mut provider = provider.lock().await;
            let mut refreshed = provider.clone();
            let mut loaded = Vec::with_capacity(ids.len());
            for id in ids {
                loaded.push(
                    refreshed
                        .request_instrument(id, clock.get_time_ns())
                        .await?,
                );
            }

            let data = loaded.iter().cloned().map(Into::into).collect();
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request.request_id,
                client_id,
                venue,
                data,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            publish(&state, &sender, response, loaded)?;
            *provider = refreshed;
            changed.notify_one();
            Ok(())
        })
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        let time = book_time(request.params.as_ref())?;
        let window = time
            .map(|time| history::Window::new(Some(time), Some(time), None, time))
            .transpose()?;
        let instrument = self.loaded(instrument_id)?;
        let (exchange, id) = crate::common::symbol::from_instrument_id(instrument.id)?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed book request", async move {
            let book = if let Some(window) = window {
                let batches = history::books(&http, &instrument, &window).await?;
                book::snapshot(&batches, &instrument, request.depth.map(NonZeroUsize::get))?
            } else {
                let source = http.get_book(&exchange, &id).await?;
                parse::book(&source, &instrument, request.depth.map(NonZeroUsize::get))?
            };

            let bound = datetime_to_unix_nanos(time);
            let response = DataResponse::Book(BookResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                book,
                bound,
                bound,
                clock.get_time_ns(),
                request.params,
            ));
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        let instrument = self.loaded(instrument_id)?;

        let window = history::Window::new(
            request.start,
            request.end,
            request.limit,
            self.clock.get_time_ns().to_datetime_utc(),
        )?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed quotes request", async move {
            let quotes = history::quotes(&http, &instrument, &window).await?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
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
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        let instrument = self.loaded(instrument_id)?;

        let window = history::Window::new(
            request.start,
            request.end,
            request.limit,
            self.clock.get_time_ns().to_datetime_utc(),
        )?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed trades request", async move {
            let trades = history::trades(&http, &instrument, &window).await?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
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
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.single_bar_type()?;
        let period_length = history::period_length(bar_type)?;
        let instrument = self.loaded(bar_type.instrument_id())?;

        let window = history::Window::new(
            request.start,
            request.end,
            request.limit,
            self.clock.get_time_ns().to_datetime_utc(),
        )?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed bars request", async move {
            let bars = history::bars(
                &http,
                &instrument,
                bar_type,
                &window,
                clock.get_time_ns().to_datetime_utc(),
                period_length,
            )
            .await?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
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
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        execute_bar_subscriptions(&cmd.bar_types, |bar_type| {
            history::period_length(bar_type)?;
            let mut state = self.publication.lock();
            state.check_subscription(bar_type.instrument_id())?;

            if state
                .polling
                .subscribe_bars(bar_type, self.clock.get_time_ns().to_datetime_utc())?
            {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        execute_bar_subscriptions(&cmd.bar_types, |bar_type| {
            if self.publication.lock().polling.unsubscribe_bars(bar_type) {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            self.subscribe_book(
                instrument_id,
                cmd.book_type,
                true,
                cmd.depth.map(NonZeroUsize::get),
            )
        })
    }

    fn subscribe_book_depth(&mut self, cmd: SubscribeBookDepth) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            let depth = cmd.depth.map_or(DEPTH10_LEN, NonZeroUsize::get);
            self.subscribe_book(instrument_id, cmd.book_type, false, Some(depth))
        })
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            if self
                .publication
                .lock()
                .polling
                .unsubscribe_book(instrument_id, true)
            {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn unsubscribe_book_depth(&mut self, cmd: &UnsubscribeBookDepth) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            if self
                .publication
                .lock()
                .polling
                .unsubscribe_book(instrument_id, false)
            {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            self.subscribe_activity(instrument_id, true)
        })
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            self.subscribe_activity(instrument_id, false)
        })
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            if self
                .publication
                .lock()
                .polling
                .unsubscribe(instrument_id, true)
            {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        execute_instrument_subscriptions(&cmd.instrument_ids, |instrument_id| {
            if self
                .publication
                .lock()
                .polling
                .unsubscribe(instrument_id, false)
            {
                self.poll_changed.notify_one();
            }

            Ok(())
        })
    }

    fn request_data(&self, _request: RequestCustomData) -> anyhow::Result<()> {
        anyhow::bail!("FinFeed custom data requests are unsupported")
    }

    fn request_book_depth(&self, request: RequestBookDepth) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        book_params(request.params.as_ref())?;
        let depth = request.depth.map_or(DEPTH10_LEN, NonZeroUsize::get);
        anyhow::ensure!(
            depth <= DEPTH10_LEN,
            "FinFeed book depth exceeds {DEPTH10_LEN}"
        );
        let instrument = self.loaded(instrument_id)?;

        let window = history::Window::new(
            request.start,
            request.end,
            request.limit,
            self.clock.get_time_ns().to_datetime_utc(),
        )?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed book depth request", async move {
            let batches = history::books(&http, &instrument, &window).await?;
            let depths = book::depths(&batches, &instrument, depth, request.limit)?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
            let response = DataResponse::BookDepth(BookDepthResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                depths,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn request_book_deltas(&self, request: RequestBookDeltas) -> anyhow::Result<()> {
        let instrument_id = request.single_instrument_id()?;
        book_params(request.params.as_ref())?;
        let instrument = self.loaded(instrument_id)?;

        let window = history::Window::new(
            request.start,
            request.end,
            request.limit,
            self.clock.get_time_ns().to_datetime_utc(),
        )?;
        let http = Arc::clone(&self.http);
        let state = Arc::clone(&self.publication);
        let sender = self.sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        self.spawn_request("FinFeed book deltas request", async move {
            let batches = history::books(&http, &instrument, &window).await?;
            let deltas = book::deltas(batches, request.limit)?;
            let start = datetime_to_unix_nanos(request.start);
            let end = datetime_to_unix_nanos(request.end);
            let response = DataResponse::BookDeltas(BookDeltasResponse::new(
                request.request_id,
                client_id,
                instrument_id,
                deltas,
                start,
                end,
                clock.get_time_ns(),
                request.params,
            ));
            publish(&state, &sender, response, Vec::new())
        })
    }

    fn request_funding_rates(&self, _request: RequestFundingRates) -> anyhow::Result<()> {
        anyhow::bail!("FinFeed funding rates are unsupported")
    }

    fn request_option_chain_reference_price(
        &self,
        _request: RequestOptionChainReferencePrice,
    ) -> anyhow::Result<()> {
        anyhow::bail!("FinFeed option chain requests are unsupported")
    }
}

impl FinFeedDataClient {
    fn loaded(&self, id: InstrumentId) -> anyhow::Result<BinaryOption> {
        self.publication.lock().loaded(id).cloned()
    }

    fn spawn_request<F>(&self, description: &'static str, future: F) -> anyhow::Result<()>
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let state = self.publication.lock();
        anyhow::ensure!(state.connected, "FinFeed client is disconnected");
        let cancellation = self.command_tasks.cancellation_token();

        self.command_tasks.spawn(async move {
            tokio::select! {
                biased;
                () = cancellation.cancelled() => {}
                result = future => {
                    if let Err(e) = result {
                        log::error!("{description} failed: {e:#}");
                    }
                }
            }
        })?;

        Ok(())
    }

    fn begin_shutdown(&self) {
        let mut state = self.publication.lock();
        state.connected = false;
        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.http.cancel();
    }

    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        let (session, command) = tokio::join!(
            self.session_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE),
            self.command_tasks.finish_shutdown(TASK_GRACE, TASK_FORCE),
        );
        session.context("failed to stop FinFeed polling tasks")?;
        command.context("failed to stop FinFeed request tasks")?;
        Ok(())
    }

    fn subscribe_activity(&self, id: InstrumentId, quotes: bool) -> anyhow::Result<()> {
        let mut state = self.publication.lock();
        state.check_subscription(id)?;

        if state
            .polling
            .subscribe(id, quotes, self.config.poll_limit)?
        {
            self.poll_changed.notify_one();
        }

        Ok(())
    }

    fn subscribe_book(
        &self,
        id: InstrumentId,
        book_type: BookType,
        deltas: bool,
        depth: Option<usize>,
    ) -> anyhow::Result<()> {
        let mut state = self.publication.lock();
        state.check_subscription(id)?;
        anyhow::ensure!(
            book_type == BookType::L2_MBP,
            "FinFeed book subscriptions require L2_MBP"
        );

        if state.polling.subscribe_book(id, deltas, depth)? {
            self.poll_changed.notify_one();
        }

        Ok(())
    }
}

impl Drop for FinFeedDataClient {
    fn drop(&mut self) {
        self.begin_shutdown();
    }
}

#[derive(Debug, Default)]
struct Publication {
    connected: bool,
    instruments: HashMap<InstrumentId, BinaryOption>,
    polling: poll::Polling,
}

impl Publication {
    fn loaded(&self, id: InstrumentId) -> anyhow::Result<&BinaryOption> {
        anyhow::ensure!(self.connected, "FinFeed client is disconnected");
        self.instruments
            .get(&id)
            .with_context(|| format!("{id} is not a loaded FinFeed instrument"))
    }

    fn check_subscription(&self, id: InstrumentId) -> anyhow::Result<()> {
        let instrument = self.loaded(id)?;
        anyhow::ensure!(
            !terminal(instrument),
            "FinFeed instrument {id} is inactive or resolved"
        );
        Ok(())
    }

    fn update(&mut self, instrument: BinaryOption) {
        if terminal(&instrument) {
            self.polling.deactivate(instrument.id);
        } else {
            self.polling.refresh(instrument.id);
        }

        self.instruments.insert(instrument.id, instrument);
    }
}

fn terminal(instrument: &BinaryOption) -> bool {
    instrument
        .info
        .as_ref()
        .and_then(|info| info.get_str("status"))
        .is_some_and(|status| matches!(status, "Closed" | "Resolved"))
}

fn publish(
    state: &Mutex<Publication>,
    sender: &EventSender<DataEvent>,
    response: DataResponse,
    instruments: Vec<BinaryOption>,
) -> anyhow::Result<()> {
    let mut state = state.lock();
    anyhow::ensure!(state.connected, "FinFeed request generation is closed");

    for instrument in &instruments {
        let instrument = InstrumentAny::from(instrument.clone());
        send(sender, DataEvent::Data(instrument.into()))?;
    }

    send(sender, DataEvent::Response(response))?;

    for instrument in instruments {
        state.update(instrument);
    }

    Ok(())
}

fn send(sender: &EventSender<DataEvent>, event: DataEvent) -> anyhow::Result<()> {
    sender
        .send(event)
        .map_err(|_| anyhow::anyhow!("FinFeed data receiver is closed"))
}

fn book_time(params: Option<&Params>) -> anyhow::Result<Option<Timestamp>> {
    book_params(params)?;

    let Some(params) = params else {
        return Ok(None);
    };

    anyhow::ensure!(
        !["date", "start", "time_start", "time_end"]
            .iter()
            .any(|key| params.contains_key(*key)),
        "FinFeed historical point snapshots require an end timestamp"
    );
    params
        .get("end")
        .map(|_| {
            params
                .get_str("end")
                .context("FinFeed book end must be a timestamp string")?
                .parse()
                .context("invalid FinFeed book end timestamp")
        })
        .transpose()
}

fn book_params(params: Option<&Params>) -> anyhow::Result<()> {
    if let Some(params) = params
        && params.contains_key("book_type")
    {
        anyhow::ensure!(
            params.get_str("book_type") == Some("L2_MBP"),
            "FinFeed books require L2_MBP"
        );
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::data::{BarType, DataType};
    use rstest::rstest;

    use super::*;
    use crate::{
        http::{models::Market, parse::parse_instrument},
        instruments::FinFeedInstrumentSelection,
    };

    #[rstest]
    #[case("instrument")]
    #[case("snapshot")]
    #[case("quotes")]
    #[case("trades")]
    #[case("bars")]
    #[case("depth")]
    #[case("deltas")]
    fn requests_reject_multiple_members_before_dispatch(#[case] family: &str) {
        let client = subscription_client();
        let ids = [
            InstrumentId::from("first_NO.FINFEED_POLYMARKET"),
            InstrumentId::from("last_NO.FINFEED_POLYMARKET"),
        ];
        let client_id = Some(client.client_id());
        let request_id = UUID4::new();
        let ts_init = UnixNanos::from(19);

        let result = match family {
            "instrument" => client.request_instrument(RequestInstrument::new(
                ids, None, None, client_id, request_id, ts_init, None,
            )),
            "snapshot" => client.request_book_snapshot(RequestBookSnapshot::new(
                ids, None, client_id, request_id, ts_init, None,
            )),
            "quotes" => client.request_quotes(RequestQuotes::new(
                ids, None, None, None, client_id, request_id, ts_init, None,
            )),
            "trades" => client.request_trades(RequestTrades::new(
                ids, None, None, None, client_id, request_id, ts_init, None,
            )),
            "bars" => {
                let bar_types =
                    ids.map(|id| BarType::from(format!("{id}-1-MINUTE-LAST-EXTERNAL").as_str()));
                client.request_bars(RequestBars::new(
                    bar_types, None, None, None, client_id, request_id, ts_init, None,
                ))
            }
            "depth" => client.request_book_depth(RequestBookDepth::new(
                ids, None, None, None, None, client_id, request_id, ts_init, None,
            )),
            "deltas" => client.request_book_deltas(RequestBookDeltas::new(
                ids, None, None, None, client_id, request_id, ts_init, None,
            )),
            _ => unreachable!(),
        };

        let error = result.unwrap_err();

        let kind = if family == "bars" {
            "bar type"
        } else {
            "instrument ID"
        };

        assert!(error.downcast_ref::<IdentifierCountError>().is_some());
        assert_eq!(
            error.to_string(),
            format!("request requires one {kind} on this adapter path, received 2")
        );
    }

    #[rstest]
    #[case("data", "FinFeed custom data requests are unsupported")]
    #[case("funding", "FinFeed funding rates are unsupported")]
    #[case("option_chain", "FinFeed option chain requests are unsupported")]
    fn unsupported_requests_fail_explicitly(#[case] family: &str, #[case] expected: &str) {
        let client = subscription_client();
        let id = InstrumentId::from("first_NO.FINFEED_POLYMARKET");
        let client_id = client.client_id();
        let request_id = UUID4::new();
        let ts_init = UnixNanos::from(29);

        let result = match family {
            "data" => client.request_data(RequestCustomData::new(
                client_id,
                DataType::new("FinFeedCustom", None, None),
                None,
                None,
                None,
                request_id,
                ts_init,
                None,
            )),
            "funding" => client.request_funding_rates(RequestFundingRates::new(
                id,
                None,
                None,
                None,
                Some(client_id),
                request_id,
                ts_init,
                None,
            )),
            "option_chain" => {
                client.request_option_chain_reference_price(RequestOptionChainReferencePrice::new(
                    "DERIBIT:BTC:BTC:1700000000000000000".parse().unwrap(),
                    id,
                    Some(client_id),
                    request_id,
                    ts_init,
                    None,
                ))
            }
            _ => unreachable!(),
        };

        assert_eq!(result.unwrap_err().to_string(), expected);
    }

    #[rstest]
    #[case("quotes")]
    #[case("trades")]
    #[case("deltas")]
    #[case("depth")]
    fn instrument_batches_continue_after_invalid_member_and_release_every_scope(
        #[case] feed: &str,
    ) {
        let mut client = subscription_client();
        let first = InstrumentId::from("first_NO.FINFEED_POLYMARKET");
        let missing = InstrumentId::from("missing_NO.FINFEED_POLYMARKET");
        let last = InstrumentId::from("last_NO.FINFEED_POLYMARKET");
        let ids = [first, missing, last];
        let client_id = Some(client.client_id());
        let command_id = UUID4::new();
        let ts_init = UnixNanos::from(17);
        let depth = NonZeroUsize::new(5);

        let result = match feed {
            "quotes" => client.subscribe_quotes(SubscribeQuotes::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            "trades" => client.subscribe_trades(SubscribeTrades::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            "deltas" => client.subscribe_book_deltas(SubscribeBookDeltas::new(
                ids,
                BookType::L2_MBP,
                client_id,
                None,
                command_id,
                ts_init,
                depth,
                false,
                None,
                None,
            )),
            "depth" => client.subscribe_book_depth(SubscribeBookDepth::new(
                ids,
                BookType::L2_MBP,
                client_id,
                None,
                command_id,
                ts_init,
                depth,
                false,
                None,
                None,
            )),
            _ => unreachable!(),
        };

        let error = result.unwrap_err();
        let partial = error.downcast_ref::<SubscriptionError>().unwrap();
        assert_eq!(partial.instrument_ids_accepted, vec![first, last]);
        assert_eq!(partial.bar_types_accepted, Vec::<BarType>::new());
        assert_eq!(
            partial.source.to_string(),
            format!("{missing} is not a loaded FinFeed instrument")
        );

        {
            let mut state = client.publication.lock();
            assert_eq!(state.polling.instrument_ids(), vec![first, last]);

            for id in [first, last] {
                let changed = match feed {
                    "quotes" | "trades" => state
                        .polling
                        .subscribe(id, feed == "quotes", client.config.poll_limit)
                        .unwrap(),
                    "deltas" | "depth" => state
                        .polling
                        .subscribe_book(id, feed == "deltas", Some(5))
                        .unwrap(),
                    _ => unreachable!(),
                };

                assert!(!changed);
            }
        }

        match feed {
            "quotes" => client.unsubscribe_quotes(&UnsubscribeQuotes::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            "trades" => client.unsubscribe_trades(&UnsubscribeTrades::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            "deltas" => client.unsubscribe_book_deltas(&UnsubscribeBookDeltas::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            "depth" => client.unsubscribe_book_depth(&UnsubscribeBookDepth::new(
                ids, client_id, None, command_id, ts_init, None, None,
            )),
            _ => unreachable!(),
        }
        .unwrap();

        assert_eq!(
            client.publication.lock().polling.instrument_ids(),
            Vec::<InstrumentId>::new()
        );
    }

    #[rstest]
    fn bar_batches_preserve_full_types_and_release_every_scope_after_invalid_member() {
        let mut client = subscription_client();
        let first = BarType::from("first_NO.FINFEED_POLYMARKET-1-MINUTE-LAST-EXTERNAL");
        let invalid = BarType::from("first_NO.FINFEED_POLYMARKET-1-MINUTE-LAST-INTERNAL");
        let second = BarType::from("first_NO.FINFEED_POLYMARKET-1-HOUR-LAST-EXTERNAL");
        let last = BarType::from("last_NO.FINFEED_POLYMARKET-1-DAY-LAST-EXTERNAL");
        let bar_types = [first, invalid, second, last];
        let client_id = Some(client.client_id());
        let command_id = UUID4::new();
        let ts_init = UnixNanos::from(23);
        let error = client
            .subscribe_bars(SubscribeBars::new(
                bar_types, client_id, None, command_id, ts_init, None, None,
            ))
            .unwrap_err();
        let partial = error.downcast_ref::<SubscriptionError>().unwrap();
        assert_eq!(partial.instrument_ids_accepted, Vec::<InstrumentId>::new());
        assert_eq!(partial.bar_types_accepted, vec![first, second, last]);
        assert_eq!(
            partial.source.to_string(),
            "FinFeed provides external OHLCV bars only"
        );

        {
            let mut state = client.publication.lock();
            assert_eq!(
                state.polling.instrument_ids(),
                vec![first.instrument_id(), last.instrument_id()]
            );

            for bar_type in [first, second, last] {
                assert!(
                    !state
                        .polling
                        .subscribe_bars(bar_type, Timestamp::UNIX_EPOCH)
                        .unwrap()
                );
            }
        }

        client
            .unsubscribe_bars(&UnsubscribeBars::new(
                bar_types, client_id, None, command_id, ts_init, None, None,
            ))
            .unwrap();
        assert_eq!(
            client.publication.lock().polling.instrument_ids(),
            Vec::<InstrumentId>::new()
        );
    }

    pub(super) fn subscription_client() -> FinFeedDataClient {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();

        let config = FinFeedDataClientConfig {
            api_key: Some(SecretString::from("test-api-key")),
            instruments: vec![FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec!["first_NO".into(), "last_NO".into()],
            }],
            ..Default::default()
        };

        let client =
            FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
        let source: Market =
            serde_json::from_str(include_str!("../test_data/polymarket-market.json")).unwrap();
        let instrument = parse_instrument(&source, UnixNanos::from(0)).unwrap();
        {
            let mut state = client.publication.lock();
            state.connected = true;

            for id in ["first_NO.FINFEED_POLYMARKET", "last_NO.FINFEED_POLYMARKET"] {
                let mut instrument = instrument.clone();
                instrument.id = InstrumentId::from(id);
                state.instruments.insert(instrument.id, instrument);
            }
        }

        client
    }

    #[rstest]
    fn drop_closes_publication_for_captured_request_state() {
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        let config = FinFeedDataClientConfig {
            api_key: Some(SecretString::from("test-api-key")),
            instruments: vec![FinFeedInstrumentSelection {
                exchange_id: "POLYMARKET".into(),
                market_ids: vec!["contract_NO".into()],
            }],
            ..Default::default()
        };

        let client =
            FinFeedDataClient::new(ClientId::from("FINFEED"), config, sender.into()).unwrap();
        let state = Arc::clone(&client.publication);
        let sender = client.sender.clone();
        state.lock().connected = true;
        let response = DataResponse::Instruments(InstrumentsResponse::new(
            UUID4::new(),
            ClientId::from("FINFEED"),
            Venue::from(FINFEED),
            Vec::new(),
            None,
            None,
            UnixNanos::from(1),
            None,
        ));
        publish(&state, &sender, response.clone(), Vec::new()).unwrap();
        assert!(matches!(
            receiver.try_recv().unwrap(),
            DataEvent::Response(_)
        ));
        drop(client);
        assert!(!state.lock().connected);
        let error = publish(&state, &sender, response, Vec::new()).unwrap_err();
        assert_eq!(error.to_string(), "FinFeed request generation is closed");
        assert!(receiver.try_recv().is_err());
    }
}
