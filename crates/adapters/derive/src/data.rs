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

//! Live market data client implementation for the Derive adapter.

use std::{
    num::NonZeroUsize,
    str::FromStr,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    cache::quote::QuoteCache,
    clients::DataClient,
    live::{get_runtime, runner::get_data_event_sender},
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, ForwardPricesResponse, FundingRatesResponse,
            InstrumentResponse, InstrumentsResponse, QuotesResponse, RequestBars,
            RequestForwardPrices, RequestFundingRates, RequestInstrument, RequestInstruments,
            RequestQuotes, RequestTrades, SubscribeBookDeltas, SubscribeBookDepth10,
            SubscribeFundingRates, SubscribeIndexPrices, SubscribeMarkPrices,
            SubscribeOptionGreeks, SubscribeQuotes, SubscribeTrades, TradesResponse,
            UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeMarkPrices, UnsubscribeOptionGreeks,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
    providers::InstrumentProvider,
};
use nautilus_core::{
    AtomicMap, AtomicSet, MUTEX_POISONED, Params, UnixNanos,
    datetime::{NANOSECONDS_IN_SECOND, datetime_to_unix_nanos},
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Bar, Data, ForwardPrice, OrderBookDeltas_API, QuoteTick},
    enums::{AggregationSource, BookType, PriceType},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{
            DERIVE_CANDLES_DEFAULT_LIMIT, DERIVE_CANDLES_MAX_PAGES, DERIVE_TRADES_PAGE_SIZE,
            DERIVE_VENUE,
        },
        enums::{
            DeriveInstrumentType, DeriveOrderbookDepth, DeriveOrderbookGroup, DeriveTickerInterval,
        },
        parse::{format_instrument_id, format_venue_symbol, parse_derive_instrument_any},
    },
    config::DeriveDataClientConfig,
    http::DeriveHttpClient,
    providers::{
        DeriveInstrumentProvider, fetch_instrument_definitions, parse_instrument_definitions,
    },
    websocket::{
        DEFAULT_ORDERBOOK_DEPTH, DEFAULT_ORDERBOOK_GROUP, DEFAULT_TICKER_INTERVAL,
        DerivePublicWsData, DeriveTickerMsg, DeriveWebSocketClient,
        DeriveWebSocketSubscriptionHandle, DeriveWsMessage, WsMessageContext,
        bar_spec_to_derive_period, orderbook_channel, parse_candle_record, parse_funding_rate,
        parse_funding_rate_history_record, parse_index_price, parse_mark_price,
        parse_option_greeks, parse_orderbook_deltas, parse_public_ws_data, parse_ticker_quote,
        parse_ticker_quote_from_rest, parse_trade_tick, ticker_channel, trades_channel,
    },
};

/// Derive live data client.
#[derive(Debug)]
pub struct DeriveDataClient {
    client_id: ClientId,
    config: DeriveDataClientConfig,
    http_client: DeriveHttpClient,
    provider: DeriveInstrumentProvider,
    ws_client: DeriveWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    active_book_delta_channels: Arc<AtomicMap<InstrumentId, String>>,
    active_book_depth10_channels: Arc<AtomicMap<InstrumentId, String>>,
    active_ticker_channels: Arc<AtomicMap<InstrumentId, String>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_channels: Arc<DashMap<String, ()>>,
    active_mark_subs: Arc<AtomicSet<InstrumentId>>,
    active_index_subs: Arc<AtomicSet<InstrumentId>>,
    active_funding_subs: Arc<AtomicSet<InstrumentId>>,
    active_greeks_subs: Arc<AtomicSet<InstrumentId>>,
    clock: &'static AtomicTime,
}

impl DeriveDataClient {
    /// Creates a new [`DeriveDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error when the HTTP client cannot be initialized.
    pub fn new(client_id: ClientId, config: DeriveDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();
        let http_client = DeriveHttpClient::new(
            config.rest_url(),
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
            None,
        )?;
        let provider = DeriveInstrumentProvider::with_expired(
            http_client.clone(),
            config.currencies.clone(),
            config.include_expired,
        );
        let ws_client = DeriveWebSocketClient::new(
            Some(config.ws_url()),
            config.environment,
            config.transport_backend,
            config.proxy_url.clone(),
        );

        Ok(Self {
            client_id,
            config,
            http_client,
            provider,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            ws_stream_handle: Mutex::new(None),
            pending_tasks: Mutex::new(Vec::new()),
            data_sender,
            instruments: Arc::new(AtomicMap::new()),
            active_book_delta_channels: Arc::new(AtomicMap::new()),
            active_book_depth10_channels: Arc::new(AtomicMap::new()),
            active_ticker_channels: Arc::new(AtomicMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            active_trade_channels: Arc::new(DashMap::new()),
            active_mark_subs: Arc::new(AtomicSet::new()),
            active_index_subs: Arc::new(AtomicSet::new()),
            active_funding_subs: Arc::new(AtomicSet::new()),
            active_greeks_subs: Arc::new(AtomicSet::new()),
            clock,
        })
    }

    /// Spawns a fire-and-forget task, tracks its handle in `pending_tasks`
    /// for teardown abort, and logs failures with the task description.
    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        // Prune finished handles before pushing so the Vec doesn't grow
        // unboundedly across long-running sessions.
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    /// Aborts every tracked pending task; used by `disconnect` and `reset`.
    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    /// Clears every local subscription map. Called from `disconnect` and
    /// `reset` because the venue drops all subscriptions on socket close,
    /// and aborted in-flight subscribe tasks can leak entries staged before
    /// spawn (they never reach their on-error rollback branch).
    fn clear_subscription_state(&self) {
        self.active_book_delta_channels.store(AHashMap::new());
        self.active_book_depth10_channels.store(AHashMap::new());
        self.active_ticker_channels.store(AHashMap::new());
        self.active_quote_subs.store(AHashSet::new());
        self.active_trade_subs.store(AHashSet::new());
        self.active_trade_channels.clear();
        self.active_mark_subs.store(AHashSet::new());
        self.active_index_subs.store(AHashSet::new());
        self.active_funding_subs.store(AHashSet::new());
        self.active_greeks_subs.store(AHashSet::new());
    }

    fn spawn_stream_task(&self, mut rx: tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>) {
        let mut ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            instruments: Arc::clone(&self.instruments),
            active_book_delta_channels: Arc::clone(&self.active_book_delta_channels),
            active_book_depth10_channels: Arc::clone(&self.active_book_depth10_channels),
            active_ticker_channels: Arc::clone(&self.active_ticker_channels),
            active_quote_subs: Arc::clone(&self.active_quote_subs),
            active_trade_subs: Arc::clone(&self.active_trade_subs),
            active_mark_subs: Arc::clone(&self.active_mark_subs),
            active_index_subs: Arc::clone(&self.active_index_subs),
            active_funding_subs: Arc::clone(&self.active_funding_subs),
            active_greeks_subs: Arc::clone(&self.active_greeks_subs),
            quote_cache: QuoteCache::new(),
        };
        let cancellation = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            loop {
                tokio::select! {
                    maybe_msg = rx.recv() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &mut ctx),
                            None => {
                                log::debug!("Derive WebSocket data stream ended");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("Derive WebSocket data stream task cancelled");
                        break;
                    }
                }
            }
        });

        let mut slot = self.ws_stream_handle.lock().expect(MUTEX_POISONED);
        *slot = Some(handle);
    }

    fn handle_ws_message(message: DeriveWsMessage, ctx: &mut WsMessageContext) {
        match message {
            DeriveWsMessage::Subscription(payload) => match parse_public_ws_data(&payload) {
                Ok(data) => Self::handle_public_ws_data(data, ctx),
                Err(e) => {
                    // Include channel and a truncated payload snippet so the
                    // wire shape that broke the parser is actionable from the
                    // log alone.
                    let snippet = truncated_payload_snippet(payload.data.get());
                    log::warn!(
                        "Failed to parse Derive public WS data on channel `{}`: {e}; payload: {snippet}",
                        payload.channel,
                    );
                }
            },
            DeriveWsMessage::Reconnected => {
                ctx.quote_cache.clear();
                log::info!("Derive WebSocket reconnected");
            }
            DeriveWsMessage::Authenticated => log::debug!("Derive WebSocket authenticated"),
        }
    }

    fn handle_public_ws_data(data: DerivePublicWsData, ctx: &mut WsMessageContext) {
        match data {
            DerivePublicWsData::Orderbook(msg) => {
                let instrument_id = msg.data.instrument_id();
                if !book_channel_is_active(ctx, instrument_id, msg.channel.as_str()) {
                    return;
                }

                let Some(instrument) = ctx.instruments.get_cloned(&instrument_id) else {
                    log::warn!("Orderbook message received for unknown instrument {instrument_id}");
                    return;
                };

                let ts_init = ctx.clock.get_time_ns();

                match parse_orderbook_deltas(
                    &msg,
                    instrument.price_precision(),
                    instrument.size_precision(),
                    ts_init,
                ) {
                    Ok(deltas) => {
                        Self::send_data(ctx, Data::Deltas(OrderBookDeltas_API::new(deltas)));
                    }
                    Err(e) => log::warn!("Failed to parse Derive orderbook deltas: {e}"),
                }
            }
            DerivePublicWsData::Trades(msg) => {
                let ts_init = ctx.clock.get_time_ns();

                for trade in &msg.trades {
                    let instrument_id = format_instrument_id(trade.instrument_name.as_str());

                    if !ctx.active_trade_subs.contains(&instrument_id) {
                        continue;
                    }

                    let Some(instrument) = ctx.instruments.get_cloned(&instrument_id) else {
                        log::warn!("Trade message received for unknown instrument {instrument_id}");
                        continue;
                    };

                    match parse_trade_tick(
                        trade,
                        instrument.price_precision(),
                        instrument.size_precision(),
                        ts_init,
                    ) {
                        Ok(tick) => Self::send_data(ctx, Data::Trade(tick)),
                        Err(e) => log::warn!("Failed to parse Derive trade tick: {e}"),
                    }
                }
            }
            DerivePublicWsData::Ticker(msg) => {
                let instrument_id = msg.data.instrument_id();

                if !channel_is_active(
                    &ctx.active_ticker_channels,
                    instrument_id,
                    msg.channel.as_str(),
                ) {
                    return;
                }

                let Some(instrument) = ctx.instruments.get_cloned(&instrument_id) else {
                    log::warn!("Ticker message received for unknown instrument {instrument_id}");
                    return;
                };

                let ts_init = ctx.clock.get_time_ns();
                let price_precision = instrument.price_precision();

                if ctx.active_quote_subs.contains(&instrument_id) {
                    match process_ticker_quote(
                        &msg,
                        price_precision,
                        instrument.size_precision(),
                        ts_init,
                        &mut ctx.quote_cache,
                    ) {
                        Ok(Some(quote)) => Self::send_data(ctx, Data::Quote(quote)),
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Derive ticker quote: {e}"),
                    }
                }

                if ctx.active_mark_subs.contains(&instrument_id) {
                    match parse_mark_price(&msg, price_precision, ts_init) {
                        Ok(Some(update)) => Self::send_data(ctx, Data::MarkPriceUpdate(update)),
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Derive mark price: {e}"),
                    }
                }

                if ctx.active_index_subs.contains(&instrument_id) {
                    match parse_index_price(&msg, price_precision, ts_init) {
                        Ok(Some(update)) => Self::send_data(ctx, Data::IndexPriceUpdate(update)),
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Derive index price: {e}"),
                    }
                }

                if ctx.active_funding_subs.contains(&instrument_id) {
                    match parse_funding_rate(&msg, ts_init) {
                        Ok(Some(update)) => {
                            if let Err(e) = ctx.data_sender.send(DataEvent::FundingRate(update)) {
                                log::error!("Failed to send Derive funding rate: {e}");
                            }
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Derive funding rate: {e}"),
                    }
                }

                if ctx.active_greeks_subs.contains(&instrument_id) {
                    match parse_option_greeks(&msg, ts_init) {
                        Ok(Some(greeks)) => {
                            if let Err(e) = ctx.data_sender.send(DataEvent::OptionGreeks(greeks)) {
                                log::error!("Failed to send Derive option greeks: {e}");
                            }
                        }
                        Ok(None) => {}
                        Err(e) => log::warn!("Failed to parse Derive option greeks: {e}"),
                    }
                }
            }
        }
    }

    fn send_data(ctx: &WsMessageContext, data: Data) {
        if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
            log::error!("Failed to send Derive data event: {e}");
        }
    }

    fn cache_provider_instruments(&self) {
        let instruments = self
            .provider
            .store()
            .get_all()
            .values()
            .cloned()
            .collect::<Vec<_>>();

        for instrument in instruments {
            self.cache_instrument(&instrument);
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send Derive instrument: {e}");
            }
        }
    }

    fn cache_instrument(&self, instrument: &InstrumentAny) {
        cache_instrument(&self.instruments, instrument);
    }

    fn prepare_subscribe(&self, instrument_id: InstrumentId) -> anyhow::Result<bool> {
        if self.instruments.contains_key(&instrument_id) {
            return Ok(false);
        }

        if !self.config.auto_load_missing_instruments {
            anyhow::bail!(
                "Instrument {instrument_id} not found and `auto_load_missing_instruments` is disabled"
            );
        }
        Ok(true)
    }

    async fn lazy_load_instrument(
        http_client: DeriveHttpClient,
        instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        instrument_id: InstrumentId,
        include_expired: bool,
    ) -> anyhow::Result<()> {
        let currency = currency_from_instrument_id(&instrument_id)?;
        let definitions = fetch_instrument_definitions(&http_client, currency, include_expired)
            .await
            .with_context(|| format!("failed to lazy-load Derive instruments for {currency}"))?;
        let mut found = false;

        for instrument in parse_instrument_definitions(definitions)? {
            if instrument.id() == instrument_id {
                found = true;
            }
            cache_instrument(&instruments, &instrument);
        }

        if !found {
            anyhow::bail!("Derive instrument {instrument_id} not found");
        }

        Ok(())
    }

    fn ws_handle(&self) -> DeriveWebSocketSubscriptionHandle {
        self.ws_client.subscription_handle()
    }

    fn feed_subs(&self, feed: TickerFeed) -> Arc<AtomicSet<InstrumentId>> {
        match feed {
            TickerFeed::Quote => Arc::clone(&self.active_quote_subs),
            TickerFeed::Mark => Arc::clone(&self.active_mark_subs),
            TickerFeed::Index => Arc::clone(&self.active_index_subs),
            TickerFeed::Funding => Arc::clone(&self.active_funding_subs),
            TickerFeed::Greeks => Arc::clone(&self.active_greeks_subs),
        }
    }

    fn has_any_ticker_feed(&self, instrument_id: InstrumentId) -> bool {
        self.active_quote_subs.contains(&instrument_id)
            || self.active_mark_subs.contains(&instrument_id)
            || self.active_index_subs.contains(&instrument_id)
            || self.active_funding_subs.contains(&instrument_id)
            || self.active_greeks_subs.contains(&instrument_id)
    }

    fn subscribe_ticker_feed(
        &self,
        instrument_id: InstrumentId,
        params: &Option<Params>,
        feed: TickerFeed,
        label: &'static str,
    ) -> anyhow::Result<()> {
        let feed_subs = self.feed_subs(feed);
        if feed_subs.contains(&instrument_id) {
            return Ok(());
        }

        if self.active_ticker_channels.contains_key(&instrument_id) {
            feed_subs.insert(instrument_id);
            return Ok(());
        }

        let instrument_name = format_venue_symbol(&instrument_id)?.to_string();
        let interval = ticker_interval(params)?;
        let channel = ticker_channel(&instrument_name, &interval);
        let needs_load = self.prepare_subscribe(instrument_id)?;
        feed_subs.insert(instrument_id);
        let ws = self.ws_handle();
        let http_client = self.http_client.clone();
        let include_expired = self.config.include_expired;
        let instruments = Arc::clone(&self.instruments);
        let active_ticker_channels = Arc::clone(&self.active_ticker_channels);
        let active_quote_subs = Arc::clone(&self.active_quote_subs);
        let active_mark_subs = Arc::clone(&self.active_mark_subs);
        let active_index_subs = Arc::clone(&self.active_index_subs);
        let active_funding_subs = Arc::clone(&self.active_funding_subs);
        let active_greeks_subs = Arc::clone(&self.active_greeks_subs);
        active_ticker_channels.insert(instrument_id, channel.clone());

        self.spawn_task("subscribe_ticker_feed", async move {
            if needs_load
                && let Err(e) = Self::lazy_load_instrument(
                    http_client,
                    instruments,
                    instrument_id,
                    include_expired,
                )
                .await
            {
                rollback_ticker_subscription(
                    &active_ticker_channels,
                    &active_quote_subs,
                    &active_mark_subs,
                    &active_index_subs,
                    &active_funding_subs,
                    &active_greeks_subs,
                    instrument_id,
                    &channel,
                );
                log::error!("Lazy-load failed for {instrument_id} ({label}): {e}");
                return Ok(());
            }

            if !channel_is_active(&active_ticker_channels, instrument_id, &channel) {
                return Ok(());
            }

            if let Err(e) = ws.subscribe_ticker(&instrument_name, &interval).await {
                rollback_ticker_subscription(
                    &active_ticker_channels,
                    &active_quote_subs,
                    &active_mark_subs,
                    &active_index_subs,
                    &active_funding_subs,
                    &active_greeks_subs,
                    instrument_id,
                    &channel,
                );
                log::error!("Failed to subscribe to Derive {label} for {instrument_id}: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn unsubscribe_ticker_feed(&self, instrument_id: InstrumentId, feed: TickerFeed) {
        let feed_subs = self.feed_subs(feed);
        if !feed_subs.contains(&instrument_id) {
            return;
        }
        feed_subs.remove(&instrument_id);

        if self.has_any_ticker_feed(instrument_id) {
            return;
        }

        let Some(channel) = self.active_ticker_channels.get_cloned(&instrument_id) else {
            return;
        };
        self.active_ticker_channels.remove(&instrument_id);

        let (instrument_name, interval) = match ticker_channel_parts(&channel) {
            Ok(parts) => parts,
            Err(e) => {
                log::error!("Invalid Derive ticker channel `{channel}`: {e}");
                return;
            }
        };
        let ws = self.ws_handle();

        self.spawn_task("unsubscribe_ticker_feed", async move {
            if let Err(e) = ws.unsubscribe_ticker(&instrument_name, &interval).await {
                log::error!("Failed to unsubscribe from Derive ticker for {instrument_id}: {e}");
            }
            Ok(())
        });
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TickerFeed {
    Quote,
    Mark,
    Index,
    Funding,
    Greeks,
}

#[async_trait(?Send)]
impl DataClient for DeriveDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DERIVE_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!("Starting Derive data client: {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping Derive data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting Derive data client: {}", self.client_id);
        self.cancellation_token.cancel();

        self.abort_pending_tasks();

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        // Leave the cancellation token cancelled; connect() refreshes it
        // (and tears down the inner WS client) on the next lifecycle start.
        self.instruments.store(AHashMap::new());
        self.clear_subscription_state();
        self.provider.store_mut().clear();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing Derive data client: {}", self.client_id);
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

        // Completes the async teardown deferred by sync reset()/stop().
        if self.cancellation_token.is_cancelled() {
            if let Err(e) = self.ws_client.disconnect().await {
                log::debug!("Error tearing down WebSocket on reconnect: {e}");
            }
            self.abort_pending_tasks();
            self.clear_subscription_state();
            self.cancellation_token = CancellationToken::new();
        }

        if !self.config.currencies.is_empty() {
            self.provider
                .load_all(None)
                .await
                .context("failed to load Derive instruments")?;
            self.cache_provider_instruments();
        }

        self.ws_client
            .connect()
            .await
            .context("failed to connect Derive WebSocket")?;
        let rx = self
            .ws_client
            .take_event_receiver()
            .ok_or_else(|| anyhow::anyhow!("Derive WebSocket event receiver not initialized"))?;
        self.spawn_stream_task(rx);

        self.is_connected.store(true, Ordering::Release);
        log::info!(
            "Connected Derive data client ({:?})",
            self.config.environment
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Err(e) = self.ws_client.disconnect().await {
            log::warn!("Error while disconnecting Derive WebSocket: {e}");
        }

        // Await the WS consumption loop so its sender is dropped before we
        // return; abort the request-handler tasks since they don't observe
        // the cancellation token and would otherwise outlive the client.
        let ws_handle = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take();
        if let Some(handle) = ws_handle
            && let Err(e) = handle.await
        {
            log::error!("Error joining Derive WebSocket data task: {e:?}");
        }
        self.abort_pending_tasks();

        // Aborting in-flight subscribe tasks skips their on-error rollback,
        // so any `active_*` entries staged before spawn would leak across
        // a reconnect and silently suppress the next subscribe. Clear the
        // local subscription state to match the venue-side reality that
        // disconnect drops all subscriptions.
        self.clear_subscription_state();

        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected Derive data client");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Derive only supports L2_MBP order book deltas");
        }

        let instrument_id = cmd.instrument_id;
        if self.active_book_delta_channels.contains_key(&instrument_id) {
            return Ok(());
        }

        let instrument_name = format_venue_symbol(&instrument_id)?.to_string();
        let group = orderbook_group(&cmd.params)?;
        let depth = orderbook_depth(cmd.depth.map(|d| d.get()), &cmd.params)?;
        let channel = orderbook_channel(&instrument_name, &group, &depth);
        let needs_load = self.prepare_subscribe(instrument_id)?;
        let ws = self.ws_handle();
        let http_client = self.http_client.clone();
        let include_expired = self.config.include_expired;
        let instruments = Arc::clone(&self.instruments);
        let active_book_delta_channels = Arc::clone(&self.active_book_delta_channels);
        active_book_delta_channels.insert(instrument_id, channel.clone());

        self.spawn_task("subscribe_book_deltas", async move {
            if needs_load
                && let Err(e) = Self::lazy_load_instrument(
                    http_client,
                    instruments,
                    instrument_id,
                    include_expired,
                )
                .await
            {
                remove_channel_if_matches(&active_book_delta_channels, instrument_id, &channel);
                log::error!("Lazy-load failed for {instrument_id} (book deltas): {e}");
                return Ok(());
            }

            if !channel_is_active(&active_book_delta_channels, instrument_id, &channel) {
                return Ok(());
            }

            if let Err(e) = ws
                .subscribe_orderbook(&instrument_name, &group, &depth)
                .await
            {
                remove_channel_if_matches(&active_book_delta_channels, instrument_id, &channel);
                log::error!("Failed to subscribe to Derive book deltas for {instrument_id}: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Derive only supports L2_MBP order book depth");
        }

        let instrument_id = cmd.instrument_id;

        if self
            .active_book_depth10_channels
            .contains_key(&instrument_id)
        {
            return Ok(());
        }

        let instrument_name = format_venue_symbol(&instrument_id)?.to_string();
        let group = orderbook_group(&cmd.params)?;
        let depth = DeriveOrderbookDepth::D10.to_string();
        let channel = orderbook_channel(&instrument_name, &group, &depth);
        let needs_load = self.prepare_subscribe(instrument_id)?;
        let ws = self.ws_handle();
        let http_client = self.http_client.clone();
        let include_expired = self.config.include_expired;
        let instruments = Arc::clone(&self.instruments);
        let active_book_depth10_channels = Arc::clone(&self.active_book_depth10_channels);
        active_book_depth10_channels.insert(instrument_id, channel.clone());

        self.spawn_task("subscribe_book_depth10", async move {
            if needs_load
                && let Err(e) = Self::lazy_load_instrument(
                    http_client,
                    instruments,
                    instrument_id,
                    include_expired,
                )
                .await
            {
                remove_channel_if_matches(&active_book_depth10_channels, instrument_id, &channel);
                log::error!("Lazy-load failed for {instrument_id} (book depth10): {e}");
                return Ok(());
            }

            if !channel_is_active(&active_book_depth10_channels, instrument_id, &channel) {
                return Ok(());
            }

            if let Err(e) = ws
                .subscribe_orderbook(&instrument_name, &group, &depth)
                .await
            {
                remove_channel_if_matches(&active_book_depth10_channels, instrument_id, &channel);
                log::error!("Failed to subscribe to Derive book depth10 for {instrument_id}: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.subscribe_ticker_feed(cmd.instrument_id, &cmd.params, TickerFeed::Quote, "quotes")
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        if self.active_trade_subs.contains(&instrument_id) {
            return Ok(());
        }

        let needs_load = self.prepare_subscribe(instrument_id)?;
        let ws = self.ws_handle();
        let http_client = self.http_client.clone();
        let include_expired = self.config.include_expired;
        let instruments = Arc::clone(&self.instruments);
        let active_trade_subs = Arc::clone(&self.active_trade_subs);
        let active_trade_channels = Arc::clone(&self.active_trade_channels);
        active_trade_subs.insert(instrument_id);

        self.spawn_task("subscribe_trades", async move {
            if needs_load
                && let Err(e) = Self::lazy_load_instrument(
                    http_client,
                    Arc::clone(&instruments),
                    instrument_id,
                    include_expired,
                )
                .await
            {
                active_trade_subs.remove(&instrument_id);
                log::error!("Lazy-load failed for {instrument_id} (trades): {e}");
                return Ok(());
            }

            if !active_trade_subs.contains(&instrument_id) {
                return Ok(());
            }

            let Some(instrument) = instruments.get_cloned(&instrument_id) else {
                active_trade_subs.remove(&instrument_id);
                log::error!("Instrument {instrument_id} not found for Derive trades");
                return Ok(());
            };
            let channel = match trade_channel(&instrument) {
                Ok(channel) => channel,
                Err(e) => {
                    active_trade_subs.remove(&instrument_id);
                    log::error!("Failed to resolve Derive trades channel: {e}");
                    return Ok(());
                }
            };

            if active_trade_channels.insert(channel.clone(), ()).is_some() {
                return Ok(());
            }

            let Some((instrument_type, currency)) = channel
                .strip_prefix("trades.")
                .and_then(|s| s.split_once('.'))
            else {
                active_trade_subs.remove(&instrument_id);
                active_trade_channels.remove(&channel);
                log::error!("Invalid Derive trades channel `{channel}`");
                return Ok(());
            };

            if let Err(e) = ws.subscribe_trades(instrument_type, currency).await {
                active_trade_subs.remove(&instrument_id);
                active_trade_channels.remove(&channel);
                log::error!("Failed to subscribe to Derive trades for {instrument_id}: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        self.subscribe_ticker_feed(
            cmd.instrument_id,
            &cmd.params,
            TickerFeed::Mark,
            "mark prices",
        )
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        self.subscribe_ticker_feed(
            cmd.instrument_id,
            &cmd.params,
            TickerFeed::Index,
            "index prices",
        )
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        self.subscribe_ticker_feed(
            cmd.instrument_id,
            &cmd.params,
            TickerFeed::Funding,
            "funding rates",
        )
    }

    fn subscribe_option_greeks(&mut self, cmd: SubscribeOptionGreeks) -> anyhow::Result<()> {
        self.subscribe_ticker_feed(
            cmd.instrument_id,
            &cmd.params,
            TickerFeed::Greeks,
            "option greeks",
        )
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let Some(channel) = self.active_book_delta_channels.get_cloned(&instrument_id) else {
            return Ok(());
        };
        self.active_book_delta_channels.remove(&instrument_id);

        let (instrument_name, group, depth) = orderbook_channel_parts(&channel)?;
        let ws = self.ws_handle();

        self.spawn_task("unsubscribe_book_deltas", async move {
            if let Err(e) = ws
                .unsubscribe_orderbook(&instrument_name, &group, &depth)
                .await
            {
                log::error!(
                    "Failed to unsubscribe from Derive book deltas for {instrument_id}: {e}"
                );
            }
            Ok(())
        });

        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let Some(channel) = self.active_book_depth10_channels.get_cloned(&instrument_id) else {
            return Ok(());
        };
        self.active_book_depth10_channels.remove(&instrument_id);

        let (instrument_name, group, depth) = orderbook_channel_parts(&channel)?;
        let ws = self.ws_handle();

        self.spawn_task("unsubscribe_book_depth10", async move {
            if let Err(e) = ws
                .unsubscribe_orderbook(&instrument_name, &group, &depth)
                .await
            {
                log::error!(
                    "Failed to unsubscribe from Derive book depth10 for {instrument_id}: {e}"
                );
            }
            Ok(())
        });

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.unsubscribe_ticker_feed(cmd.instrument_id, TickerFeed::Quote);
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        let Some(instrument) = self.instruments.get_cloned(&instrument_id) else {
            self.active_trade_subs.remove(&instrument_id);
            return Ok(());
        };
        let channel = trade_channel(&instrument)?;

        self.active_trade_subs.remove(&instrument_id);
        if active_trade_channel_count(&self.instruments, &self.active_trade_subs, &channel) > 0 {
            return Ok(());
        }

        if self.active_trade_channels.remove(&channel).is_none() {
            return Ok(());
        }

        let (instrument_type, currency) = channel
            .strip_prefix("trades.")
            .and_then(|s| s.split_once('.'))
            .ok_or_else(|| anyhow::anyhow!("invalid Derive trades channel `{channel}`"))?;
        let instrument_type = instrument_type.to_string();
        let currency = currency.to_string();
        let ws = self.ws_handle();

        self.spawn_task("unsubscribe_trades", async move {
            if let Err(e) = ws.unsubscribe_trades(&instrument_type, &currency).await {
                log::error!("Failed to unsubscribe from Derive trades for {instrument_id}: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        self.unsubscribe_ticker_feed(cmd.instrument_id, TickerFeed::Mark);
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        self.unsubscribe_ticker_feed(cmd.instrument_id, TickerFeed::Index);
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        self.unsubscribe_ticker_feed(cmd.instrument_id, TickerFeed::Funding);
        Ok(())
    }

    fn unsubscribe_option_greeks(&mut self, cmd: &UnsubscribeOptionGreeks) -> anyhow::Result<()> {
        self.unsubscribe_ticker_feed(cmd.instrument_id, TickerFeed::Greeks);
        Ok(())
    }

    fn request_quotes(&self, request: RequestQuotes) -> anyhow::Result<()> {
        // No historical quote endpoint; `public/get_tickers` is a current snapshot.
        let instrument_id = request.instrument_id;
        let instrument = self.instruments.get_cloned(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Derive instrument {instrument_id} not found in cache")
        })?;
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);

        self.spawn_task("request_quotes", async move {
            let ticker = match http_client.get_ticker(&venue_symbol).await {
                Ok(ticker) => ticker,
                Err(e) => {
                    log::error!("Failed to fetch Derive ticker for {instrument_id}: {e:?}");
                    return Ok(());
                }
            };

            let ts_init = clock.get_time_ns();
            let quotes = match parse_ticker_quote_from_rest(
                &ticker,
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(quote) => {
                    // Drop the snapshot when bounded callers ask for a past window.
                    let within_start = start_nanos.is_none_or(|nanos| quote.ts_event >= nanos);
                    let within_end = end_nanos.is_none_or(|nanos| quote.ts_event <= nanos);
                    if within_start && within_end {
                        vec![quote]
                    } else {
                        Vec::new()
                    }
                }
                Err(e) => {
                    log::warn!("Failed to parse Derive ticker for {instrument_id}: {e}");
                    Vec::new()
                }
            };

            let response = DataResponse::Quotes(QuotesResponse::new(
                request_id,
                client_id,
                instrument_id,
                quotes,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send Derive quotes response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instrument = self.instruments.get_cloned(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Derive instrument {instrument_id} not found in cache")
        })?;
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(NonZeroUsize::get);
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let from_timestamp = start.map(|dt| dt.timestamp_millis());
        let to_timestamp = end.map(|dt| dt.timestamp_millis());

        self.spawn_task("request_trades", async move {
            // Hold page_size constant across requests: the venue paginates by
            // offset = (page - 1) * page_size, so shrinking page_size mid-walk
            // would skip and duplicate trades.
            let page_size = limit.map_or(DERIVE_TRADES_PAGE_SIZE, |cap| {
                cap.min(DERIVE_TRADES_PAGE_SIZE as usize) as u32
            });
            let mut trades = Vec::new();
            let mut page = 1u32;

            loop {
                let result = match http_client
                    .get_trade_history(&venue_symbol, from_timestamp, to_timestamp, page, page_size)
                    .await
                {
                    Ok(result) => result,
                    Err(e) => {
                        log::error!("Failed to fetch Derive trades for {instrument_id}: {e:?}");
                        return Ok(());
                    }
                };

                if result.trades.is_empty() {
                    break;
                }

                let num_pages = result.pagination.num_pages;
                let ts_init = clock.get_time_ns();

                for trade in &result.trades {
                    if let Some(cap) = limit
                        && trades.len() >= cap
                    {
                        break;
                    }

                    match parse_trade_tick(trade, price_precision, size_precision, ts_init) {
                        Ok(tick) => trades.push(tick),
                        Err(e) => log::warn!(
                            "Failed to parse Derive trade {} for {instrument_id}: {e}",
                            trade.trade_id,
                        ),
                    }
                }

                if let Some(cap) = limit
                    && trades.len() >= cap
                {
                    break;
                }

                if (page as i64) >= num_pages {
                    break;
                }
                page += 1;
            }

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
                log::error!("Failed to send Derive trades response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let instrument = self.instruments.get_cloned(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Derive instrument {instrument_id} not found in cache")
        })?;
        anyhow::ensure!(
            matches!(instrument, InstrumentAny::CryptoPerpetual(_)),
            "Funding rates are only available for Derive perpetual instruments (got {instrument_id})",
        );
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();

        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(NonZeroUsize::get);
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let start_ms = start.map(|dt| dt.timestamp_millis());
        let end_ms = end.map(|dt| dt.timestamp_millis());

        self.spawn_task("request_funding_rates", async move {
            let result = match http_client
                .get_funding_rate_history(&venue_symbol, start_ms, end_ms, None)
                .await
            {
                Ok(result) => result,
                Err(e) => {
                    log::error!(
                        "Failed to fetch Derive funding rate history for {instrument_id}: {e:?}",
                    );
                    return Ok(());
                }
            };

            let ts_init = clock.get_time_ns();
            let mut updates = Vec::with_capacity(result.funding_rate_history.len());

            for record in &result.funding_rate_history {
                if let Some(cap) = limit
                    && updates.len() >= cap
                {
                    break;
                }

                match parse_funding_rate_history_record(record, instrument_id, None, ts_init) {
                    Ok(update) => updates.push(update),
                    Err(e) => log::warn!(
                        "Failed to parse Derive funding rate record for {instrument_id} at {}: {e}",
                        record.timestamp,
                    ),
                }
            }

            let response = DataResponse::FundingRates(FundingRatesResponse::new(
                request_id,
                client_id,
                instrument_id,
                updates,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send Derive funding rates response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let bar_type = request.bar_type;
        anyhow::ensure!(
            bar_type.aggregation_source() == AggregationSource::External,
            "Derive only supports EXTERNAL aggregation source (got {bar_type})",
        );
        let spec = bar_type.spec();
        anyhow::ensure!(
            spec.price_type == PriceType::Last,
            "Derive candles are trade-based; only PriceType::Last is supported (got {bar_type})",
        );

        let instrument_id = bar_type.instrument_id();
        let instrument = self.instruments.get_cloned(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Derive instrument {instrument_id} not found in cache")
        })?;
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let period = bar_spec_to_derive_period(spec.aggregation, spec.step.get() as u64)
            .with_context(|| format!("unsupported Derive bar spec for {bar_type}"))?;

        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let params = request.params;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(NonZeroUsize::get);
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        // The venue requires both bounds in UNIX seconds. Default end to now
        // and start to one window of `limit` buckets (or 1000) before end.
        let now_secs = (clock.get_time_ns().as_u64() / NANOSECONDS_IN_SECOND) as i64;
        let end_ts = end.map_or(now_secs, |dt| dt.timestamp());
        let default_span = i64::from(period) * limit.unwrap_or(DERIVE_CANDLES_DEFAULT_LIMIT) as i64;
        let start_ts = start.map_or(end_ts - default_span, |dt| dt.timestamp());

        self.spawn_task("request_bars", async move {
            // Venue caps each call at 5000 candles; walk backwards by shrinking
            // `current_end` to one second before each page's earliest bucket.
            let mut seen_timestamps: AHashSet<i64> = AHashSet::new();
            let mut pages: Vec<Vec<Bar>> = Vec::new();
            let mut total_bars = 0usize;
            let mut current_end = end_ts;
            let mut page_count = 0;

            loop {
                page_count += 1;

                let mut records = match http_client
                    .get_candles(&venue_symbol, start_ts, current_end, period)
                    .await
                {
                    Ok(records) => records,
                    Err(e) => {
                        log::error!("Failed to fetch Derive candles for {bar_type}: {e:?}");
                        return Ok(());
                    }
                };

                if records.is_empty() {
                    break;
                }

                records.sort_by_key(|r| r.timestamp_bucket);

                let has_new = records
                    .iter()
                    .any(|r| !seen_timestamps.contains(&r.timestamp_bucket));

                if !has_new {
                    break;
                }

                let ts_init = clock.get_time_ns();
                let mut page_bars = Vec::with_capacity(records.len());
                let mut earliest_ts: Option<i64> = None;

                for record in &records {
                    let bucket = record.timestamp_bucket;
                    if earliest_ts.is_none_or(|ts| bucket < ts) {
                        earliest_ts = Some(bucket);
                    }

                    if seen_timestamps.contains(&bucket) {
                        continue;
                    }

                    match parse_candle_record(
                        record,
                        bar_type,
                        price_precision,
                        size_precision,
                        ts_init,
                    ) {
                        Ok(bar) => {
                            page_bars.push(bar);
                            seen_timestamps.insert(bucket);
                        }
                        Err(e) => log::warn!(
                            "Failed to parse Derive candle for {bar_type} at {bucket}: {e}",
                        ),
                    }
                }

                total_bars += page_bars.len();
                pages.push(page_bars);

                if let Some(cap) = limit
                    && total_bars >= cap
                {
                    break;
                }

                let Some(earliest) = earliest_ts else {
                    break;
                };

                if earliest <= start_ts {
                    break;
                }

                current_end = earliest - 1;

                if page_count >= DERIVE_CANDLES_MAX_PAGES {
                    log::warn!(
                        "Derive bars pagination hit safety cap of {DERIVE_CANDLES_MAX_PAGES} pages for {bar_type}",
                    );
                    break;
                }
            }

            let mut bars: Vec<Bar> = Vec::with_capacity(total_bars);
            for page in pages.into_iter().rev() {
                bars.extend(page);
            }

            if let Some(cap) = limit
                && bars.len() > cap
            {
                let drop_count = bars.len() - cap;
                bars.drain(..drop_count);
            }

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
                log::error!("Failed to send Derive bars response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_forward_prices(&self, request: RequestForwardPrices) -> anyhow::Result<()> {
        // The DataEngine drives this from `subscribe_option_chain` to bootstrap
        // the ATM price for the option series. It passes one option instrument
        // from the target series; that instrument's ticker carries the forward
        // price for every option at the same expiry. Bulk mode is unsupported
        // because Derive has no per-currency ticker endpoint.
        let Some(instrument_id) = request.instrument_id else {
            anyhow::bail!(
                "Derive request_forward_prices requires an `instrument_id`; bulk fetch is not supported",
            );
        };
        let instrument = self.instruments.get_cloned(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Derive instrument {instrument_id} not found in cache")
        })?;
        anyhow::ensure!(
            matches!(instrument, InstrumentAny::CryptoOption(_)),
            "Derive forward prices are only meaningful for options (got {instrument_id})",
        );
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();

        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let venue = request.venue;
        let underlying = request.underlying;
        let params = request.params;

        self.spawn_task("request_forward_prices", async move {
            // The engine inserts this request into `pending_option_chain_requests`
            // and blocks `OptionChainManager` creation until a response arrives.
            // Always emit a response so the engine can fall back to live-tick
            // bootstrap when the REST ticker is unavailable or non-option.
            let forwards: Vec<ForwardPrice> = match http_client.get_ticker(&venue_symbol).await {
                Ok(ticker) => match ticker.option_pricing.as_ref() {
                    Some(pricing) => {
                        let ts_event = clock.get_time_ns();
                        vec![ForwardPrice::new(
                            instrument_id,
                            pricing.forward_price,
                            Some(underlying.to_string()),
                            ts_event,
                            ts_event,
                        )]
                    }
                    None => {
                        log::warn!(
                            "Derive ticker for {instrument_id} has no option_pricing; emitting empty forward prices",
                        );
                        Vec::new()
                    }
                },
                Err(e) => {
                    log::error!(
                        "Failed to fetch Derive ticker for {instrument_id}: {e:?}; emitting empty forward prices",
                    );
                    Vec::new()
                }
            };

            let response = DataResponse::ForwardPrices(ForwardPricesResponse::new(
                request_id,
                client_id,
                venue,
                forwards,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send Derive forward prices response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let currencies = self.config.currencies.clone();
        if currencies.is_empty() {
            anyhow::bail!(
                "Derive request_instruments requires at least one configured currency \
                 (DeriveDataClientConfig::currencies)"
            );
        }

        let http_client = self.http_client.clone();
        let include_expired = self.config.include_expired;
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let venue = self.venue().unwrap_or(*DERIVE_VENUE);
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;

        self.spawn_task("request_instruments", async move {
            let mut all_instruments = Vec::new();

            for currency in currencies {
                match fetch_instrument_definitions(&http_client, &currency, include_expired).await {
                    Ok(definitions) => match parse_instrument_definitions(definitions) {
                        Ok(instruments) => {
                            for instrument in instruments {
                                cache_instrument(&instruments_cache, &instrument);
                                all_instruments.push(instrument);
                            }
                        }
                        Err(e) => {
                            log::error!("Failed to parse Derive instruments for {currency}: {e}");
                        }
                    },
                    Err(e) => {
                        log::error!("Failed to fetch Derive instruments for {currency}: {e:?}");
                    }
                }
            }

            let response = DataResponse::Instruments(InstrumentsResponse::new(
                request_id,
                client_id,
                venue,
                all_instruments,
                start_nanos,
                end_nanos,
                clock.get_time_ns(),
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send Derive instruments response: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let instrument_id = request.instrument_id;
        let venue_symbol = format_venue_symbol(&instrument_id)?.to_string();

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let clock = self.clock;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let request_id = request.request_id;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;

        self.spawn_task("request_instrument", async move {
            let definition = match http_client.get_instrument(&venue_symbol).await {
                Ok(definition) => definition,
                Err(e) => {
                    log::error!("Failed to fetch Derive instrument {instrument_id}: {e:?}");
                    return Ok(());
                }
            };

            let ts_init = clock.get_time_ns();
            let instrument = match parse_derive_instrument_any(&definition, ts_init) {
                Ok(Some(instrument)) => instrument,
                Ok(None) => {
                    log::warn!(
                        "Derive instrument {instrument_id} resolved to an unsupported type ({:?})",
                        definition.instrument_type,
                    );
                    return Ok(());
                }
                Err(e) => {
                    log::error!("Failed to parse Derive instrument {instrument_id}: {e}");
                    return Ok(());
                }
            };

            cache_instrument(&instruments_cache, &instrument);

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
                log::error!("Failed to send Derive instrument response: {e}");
            }
            Ok(())
        });

        Ok(())
    }
}

fn cache_instrument(
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    instrument: &InstrumentAny,
) {
    instruments.insert(instrument.id(), instrument.clone());
}

fn process_ticker_quote(
    msg: &DeriveTickerMsg,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
    quote_cache: &mut QuoteCache,
) -> anyhow::Result<Option<QuoteTick>> {
    let quote = parse_ticker_quote(msg, price_precision, size_precision, ts_init)?;
    let (bid_price, bid_size) = quote_side(quote.bid_price, quote.bid_size);
    let (ask_price, ask_size) = quote_side(quote.ask_price, quote.ask_size);

    match quote_cache.process(
        quote.instrument_id,
        bid_price,
        ask_price,
        bid_size,
        ask_size,
        quote.ts_event,
        quote.ts_init,
    ) {
        Ok(quote) => Ok(Some(quote)),
        Err(e) => {
            log::debug!(
                "Skipping partial Derive ticker quote for {}: {e}",
                msg.data.instrument_name(),
            );
            Ok(None)
        }
    }
}

fn quote_side(price: Price, size: Quantity) -> (Option<Price>, Option<Quantity>) {
    if price.is_zero() || size.is_zero() {
        (None, None)
    } else {
        (Some(price), Some(size))
    }
}

fn book_channel_is_active(
    ctx: &WsMessageContext,
    instrument_id: InstrumentId,
    channel: &str,
) -> bool {
    channel_is_active(&ctx.active_book_delta_channels, instrument_id, channel)
        || channel_is_active(&ctx.active_book_depth10_channels, instrument_id, channel)
}

fn channel_is_active(
    channels: &AtomicMap<InstrumentId, String>,
    instrument_id: InstrumentId,
    channel: &str,
) -> bool {
    channels
        .get_cloned(&instrument_id)
        .is_some_and(|active_channel| active_channel == channel)
}

fn remove_channel_if_matches(
    channels: &AtomicMap<InstrumentId, String>,
    instrument_id: InstrumentId,
    channel: &str,
) {
    if channel_is_active(channels, instrument_id, channel) {
        channels.remove(&instrument_id);
    }
}

#[allow(clippy::too_many_arguments)]
fn rollback_ticker_subscription(
    channels: &AtomicMap<InstrumentId, String>,
    quote_subs: &AtomicSet<InstrumentId>,
    mark_subs: &AtomicSet<InstrumentId>,
    index_subs: &AtomicSet<InstrumentId>,
    funding_subs: &AtomicSet<InstrumentId>,
    greeks_subs: &AtomicSet<InstrumentId>,
    instrument_id: InstrumentId,
    channel: &str,
) {
    remove_channel_if_matches(channels, instrument_id, channel);
    quote_subs.remove(&instrument_id);
    mark_subs.remove(&instrument_id);
    index_subs.remove(&instrument_id);
    funding_subs.remove(&instrument_id);
    greeks_subs.remove(&instrument_id);
}

fn orderbook_channel_parts(channel: &str) -> anyhow::Result<(String, String, String)> {
    let rest = channel
        .strip_prefix("orderbook.")
        .ok_or_else(|| anyhow::anyhow!("invalid Derive orderbook channel `{channel}`"))?;
    let mut parts = rest.rsplitn(3, '.');
    let depth = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid Derive orderbook channel `{channel}`"))?;
    let group = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid Derive orderbook channel `{channel}`"))?;
    let instrument_name = parts
        .next()
        .filter(|value| !value.is_empty())
        .ok_or_else(|| anyhow::anyhow!("invalid Derive orderbook channel `{channel}`"))?;

    Ok((
        instrument_name.to_string(),
        group.to_string(),
        depth.to_string(),
    ))
}

fn ticker_channel_parts(channel: &str) -> anyhow::Result<(String, String)> {
    let rest = channel
        .strip_prefix("ticker_slim.")
        .or_else(|| channel.strip_prefix("ticker."))
        .ok_or_else(|| anyhow::anyhow!("invalid Derive ticker channel `{channel}`"))?;
    let (instrument_name, interval) = rest
        .rsplit_once('.')
        .ok_or_else(|| anyhow::anyhow!("invalid Derive ticker channel `{channel}`"))?;
    anyhow::ensure!(
        !instrument_name.is_empty() && !interval.is_empty(),
        "invalid Derive ticker channel `{channel}`"
    );

    Ok((instrument_name.to_string(), interval.to_string()))
}

fn orderbook_group(params: &Option<Params>) -> anyhow::Result<String> {
    let group = params
        .as_ref()
        .and_then(|p| {
            p.get_str("group")
                .map(ToOwned::to_owned)
                .or_else(|| p.get_u64("group").map(|value| value.to_string()))
        })
        .unwrap_or_else(|| DEFAULT_ORDERBOOK_GROUP.to_string());

    DeriveOrderbookGroup::from_str(&group)
        .with_context(|| format!("invalid Derive orderbook group `{group}`"))?;
    Ok(group)
}

fn orderbook_depth(depth: Option<usize>, params: &Option<Params>) -> anyhow::Result<String> {
    let depth = depth
        .map(|value| value.to_string())
        .or_else(|| {
            params.as_ref().and_then(|p| {
                p.get_str("depth")
                    .map(ToOwned::to_owned)
                    .or_else(|| p.get_u64("depth").map(|value| value.to_string()))
            })
        })
        .unwrap_or_else(|| DEFAULT_ORDERBOOK_DEPTH.to_string());

    DeriveOrderbookDepth::from_str(&depth)
        .with_context(|| format!("invalid Derive orderbook depth `{depth}`"))?;
    Ok(depth)
}

fn ticker_interval(params: &Option<Params>) -> anyhow::Result<String> {
    let interval = params
        .as_ref()
        .and_then(|p| {
            p.get_str("interval")
                .map(ToOwned::to_owned)
                .or_else(|| p.get_u64("interval").map(|value| value.to_string()))
        })
        .unwrap_or_else(|| DEFAULT_TICKER_INTERVAL.to_string());

    DeriveTickerInterval::from_str(&interval)
        .with_context(|| format!("invalid Derive ticker interval `{interval}`"))?;
    Ok(interval)
}

fn trade_channel(instrument: &InstrumentAny) -> anyhow::Result<String> {
    let instrument_type = derive_instrument_type(instrument)?.to_string();
    let instrument_id = instrument.id();
    let currency = currency_from_instrument_id(&instrument_id)?;
    Ok(trades_channel(&instrument_type, currency))
}

fn derive_instrument_type(instrument: &InstrumentAny) -> anyhow::Result<DeriveInstrumentType> {
    match instrument {
        InstrumentAny::CryptoPerpetual(_) => Ok(DeriveInstrumentType::Perp),
        InstrumentAny::CryptoOption(_) => Ok(DeriveInstrumentType::Option),
        InstrumentAny::CurrencyPair(_) => Ok(DeriveInstrumentType::Erc20),
        other => anyhow::bail!("unsupported Derive instrument type for trades: {other:?}"),
    }
}

fn currency_from_instrument_id(instrument_id: &InstrumentId) -> anyhow::Result<&str> {
    anyhow::ensure!(
        instrument_id.venue == *DERIVE_VENUE,
        "instrument ID `{instrument_id}` is not for venue {}",
        DERIVE_VENUE.as_str(),
    );

    instrument_id
        .symbol
        .as_str()
        .split_once('-')
        .and_then(|(currency, _)| (!currency.is_empty()).then_some(currency))
        .ok_or_else(|| anyhow::anyhow!("cannot derive currency from {instrument_id}"))
}

fn active_trade_channel_count(
    instruments: &AtomicMap<InstrumentId, InstrumentAny>,
    active_trade_subs: &AtomicSet<InstrumentId>,
    channel: &str,
) -> usize {
    active_trade_subs
        .load()
        .iter()
        .filter(|instrument_id| {
            instruments
                .get_cloned(instrument_id)
                .and_then(|instrument| trade_channel(&instrument).ok())
                .is_some_and(|active_channel| active_channel == channel)
        })
        .count()
}

// Caps the rendered JSON at ~512 bytes for log grep-ability and backs the
// slice off to a UTF-8 char boundary so a multi-byte codepoint near the cap
// can never produce a panicking slice.
fn truncated_payload_snippet(raw: &str) -> String {
    const MAX_LEN: usize = 512;
    if raw.len() <= MAX_LEN {
        return raw.to_string();
    }
    let mut end = MAX_LEN;
    while end > 0 && !raw.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...(truncated)", &raw[..end])
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, time::Duration};

    use nautilus_common::{live::runner::replace_data_event_sender, testing::wait_until_async};
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        identifiers::InstrumentId,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        common::{
            consts::DERIVE_CLIENT_ID, enums::DeriveEnvironment, parse::parse_derive_instrument_any,
        },
        http::models::DeriveInstrument,
        websocket::{DeriveWsFrame, WsSubscriptionPayload},
    };

    fn data_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("test_data")
    }

    fn load_json(filename: &str) -> Value {
        let content = std::fs::read_to_string(data_path().join(filename))
            .unwrap_or_else(|_| panic!("failed to read {filename}"));
        serde_json::from_str(&content).expect("invalid json")
    }

    #[rstest]
    fn test_truncated_payload_snippet_short_payload_is_unchanged() {
        let short = json!({"ok": true}).to_string();
        assert_eq!(truncated_payload_snippet(&short), r#"{"ok":true}"#);
    }

    #[rstest]
    fn test_truncated_payload_snippet_truncates_long_ascii_payload() {
        let big = json!({"msg": "x".repeat(1024)}).to_string();
        let snippet = truncated_payload_snippet(&big);
        assert!(snippet.ends_with("...(truncated)"));
        // Body before the suffix capped near the 512-byte target.
        assert!(snippet.len() <= 512 + "...(truncated)".len());
    }

    #[rstest]
    fn test_truncated_payload_snippet_handles_multibyte_at_boundary() {
        // Construct a payload where MAX_LEN (512) lands *inside* a UTF-8
        // codepoint. `{"a":"x` is 7 bytes (odd-aligned), each 'e' with acute
        // accent (U+00E9) is 2 bytes, and 7 + 252*2 = 511, so byte 512 falls
        // inside the 253rd accented char. Under the old fixed-byte slice
        // `&raw[..512]` would panic with "byte index 512 is not a char
        // boundary"; with the backoff it must return a valid `String`
        // strictly shorter than 512 bytes for the body.
        let value: String = format!("x{}", "\u{00E9}".repeat(1024));
        let big = json!({"a": value});
        let raw = big.to_string();
        assert!(
            !raw.is_char_boundary(512),
            "test premise: 512 must be mid-codepoint",
        );

        let snippet = truncated_payload_snippet(&raw);
        assert!(snippet.ends_with("...(truncated)"));
        let body_len = snippet.len() - "...(truncated)".len();
        assert!(body_len <= 512);
    }

    fn subscription_payload(channel: &str, data: &Value) -> WsSubscriptionPayload {
        let frame = json!({
            "jsonrpc": "2.0",
            "method": "subscription",
            "params": {
                "channel": channel,
                "data": data
            }
        });

        match DeriveWsFrame::parse(&frame.to_string()).unwrap() {
            DeriveWsFrame::Subscription(payload) => payload,
            other => panic!("expected subscription frame, was {other:?}"),
        }
    }

    fn make_ctx(
        instrument: Option<InstrumentAny>,
    ) -> (
        WsMessageContext,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) {
        let (data_sender, data_rx) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());
        if let Some(instrument) = instrument {
            cache_instrument(&instruments, &instrument);
        }

        (
            WsMessageContext {
                clock: get_atomic_clock_realtime(),
                data_sender,
                instruments,
                active_book_delta_channels: Arc::new(AtomicMap::new()),
                active_book_depth10_channels: Arc::new(AtomicMap::new()),
                active_ticker_channels: Arc::new(AtomicMap::new()),
                active_quote_subs: Arc::new(AtomicSet::new()),
                active_trade_subs: Arc::new(AtomicSet::new()),
                active_mark_subs: Arc::new(AtomicSet::new()),
                active_index_subs: Arc::new(AtomicSet::new()),
                active_funding_subs: Arc::new(AtomicSet::new()),
                active_greeks_subs: Arc::new(AtomicSet::new()),
                quote_cache: QuoteCache::new(),
            },
            data_rx,
        )
    }

    fn perp_instrument() -> InstrumentAny {
        parse_derive_instrument_any(&perp_definition("ETH-PERP", "ETH"), UnixNanos::from(1))
            .unwrap()
            .unwrap()
    }

    fn btc_perp_instrument() -> InstrumentAny {
        parse_derive_instrument_any(&perp_definition("BTC-PERP", "BTC"), UnixNanos::from(1))
            .unwrap()
            .unwrap()
    }

    fn perp_definition(name: &str, currency: &str) -> DeriveInstrument {
        let mut value = load_json("perps/instrument_eth.json");
        value["base_currency"] = json!(currency);
        value["instrument_name"] = json!(name);
        value["perp_details"]["index"] = json!(format!("{currency}-USD"));

        serde_json::from_value(value).unwrap()
    }

    fn option_instrument() -> InstrumentAny {
        let definition: DeriveInstrument =
            serde_json::from_value(load_json("options/instrument_eth.json")).unwrap();
        parse_derive_instrument_any(&definition, UnixNanos::from(1))
            .unwrap()
            .unwrap()
    }

    fn spot_instrument() -> InstrumentAny {
        let definition: DeriveInstrument =
            serde_json::from_value(load_json("spot/instrument_eth.json")).unwrap();
        parse_derive_instrument_any(&definition, UnixNanos::from(1))
            .unwrap()
            .unwrap()
    }

    fn ticker_json(timestamp: i64) -> Value {
        let mut value = load_json("perps/ws_ticker_eth.json");
        value["timestamp"] = json!(timestamp);
        value
    }

    fn option_ticker_json(timestamp: i64) -> Value {
        let mut value = load_json("options/http_ticker_eth_snapshot.json");
        value["timestamp"] = json!(timestamp);
        value
    }

    fn spot_ticker_slim_json() -> Value {
        load_json("spot/ws_ticker_slim_eth.json")
    }

    fn orderbook_json() -> Value {
        load_json("perps/ws_orderbook_eth.json")
    }

    fn trade_json(instrument_name: &str, trade_id: &str) -> Value {
        let mut value = load_json("perps/ws_trade_eth.json");
        value["instrument_name"] = json!(instrument_name);
        value["trade_id"] = json!(trade_id);
        value
    }

    #[rstest]
    fn test_handle_ticker_subscription_emits_quote_with_instrument_precision() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        ctx.active_ticker_channels
            .insert(instrument_id, "ticker_slim.ETH-PERP.1000".to_string());
        ctx.active_quote_subs.insert(instrument_id);
        let payload = subscription_payload(
            "ticker_slim.ETH-PERP.1000",
            &json!({
                "timestamp": 1_700_000_000_010_i64,
                "instrument_ticker": ticker_json(1_700_000_000_000)
            }),
        );

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.instrument_id, instrument_id);
                assert_eq!(quote.bid_price, Price::from("3500.00"));
                assert_eq!(quote.ask_price, Price::from("3501.00"));
                assert_eq!(quote.bid_size, Quantity::from("1.000"));
                assert_eq!(quote.ask_size, Quantity::from("2.000"));
                assert_eq!(quote.bid_price.precision, 2);
                assert_eq!(quote.bid_size.precision, 3);
            }
            other => panic!("expected quote data event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_ticker_partial_quote_without_cache_emits_no_quote() {
        let instrument = spot_instrument();
        let instrument_id = instrument.id();
        let channel = "ticker_slim.ETH-USDC.1000";
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, channel);
        ctx.active_quote_subs.insert(instrument_id);
        let payload = subscription_payload(channel, &spot_ticker_slim_json());

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_ticker_partial_quote_uses_cached_side() {
        let instrument = spot_instrument();
        let instrument_id = instrument.id();
        let channel = "ticker_slim.ETH-USDC.1000";
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        let cached_quote = QuoteTick::new(
            instrument_id,
            Price::from("0.1"),
            Price::from("0.3"),
            Quantity::from("10.00"),
            Quantity::from("20.00"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        ctx.quote_cache.insert(instrument_id, cached_quote);
        install_ticker(&ctx, instrument_id, channel);
        ctx.active_quote_subs.insert(instrument_id);
        let payload = subscription_payload(channel, &spot_ticker_slim_json());

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::Quote(quote)) => {
                assert_eq!(quote.instrument_id, instrument_id);
                assert_eq!(quote.bid_price, Price::from("0.2"));
                assert_eq!(quote.ask_price, Price::from("0.3"));
                assert_eq!(quote.bid_size, Quantity::from("45.00"));
                assert_eq!(quote.ask_size, Quantity::from("20.00"));
            }
            other => panic!("expected quote data event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_reconnected_clears_quote_cache() {
        let instrument = spot_instrument();
        let instrument_id = instrument.id();
        let channel = "ticker_slim.ETH-USDC.1000";
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        let cached_quote = QuoteTick::new(
            instrument_id,
            Price::from("0.1"),
            Price::from("0.3"),
            Quantity::from("10.00"),
            Quantity::from("20.00"),
            UnixNanos::from(1),
            UnixNanos::from(1),
        );
        ctx.quote_cache.insert(instrument_id, cached_quote);
        install_ticker(&ctx, instrument_id, channel);
        ctx.active_quote_subs.insert(instrument_id);

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Reconnected, &mut ctx);
        let payload = subscription_payload(channel, &spot_ticker_slim_json());
        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_orderbook_subscription_emits_snapshot_deltas() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        ctx.active_book_delta_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        let payload = subscription_payload("orderbook.ETH-PERP.1.10", &orderbook_json());

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::Deltas(deltas)) => {
                assert_eq!(deltas.instrument_id, instrument_id);
                assert_eq!(deltas.deltas.len(), 3);
                assert_eq!(deltas.deltas[1].order.price, Price::from("3500.00"));
                assert_eq!(deltas.deltas[1].order.size, Quantity::from("1.000"));
                assert_eq!(deltas.deltas[2].order.price, Price::from("3501.00"));
                assert_eq!(deltas.deltas[2].order.size, Quantity::from("2.000"));
            }
            other => panic!("expected deltas data event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_handle_orderbook_subscription_emits_for_depth10_subscription() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        ctx.active_book_depth10_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        let payload = subscription_payload("orderbook.ETH-PERP.1.10", &orderbook_json());

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::Deltas(deltas)) => {
                assert_eq!(deltas.instrument_id, instrument_id);
                assert_eq!(deltas.deltas.len(), 3);
            }
            other => panic!("expected deltas data event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_orderbook_frame_ignored_for_inactive_channel() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        ctx.active_book_delta_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.20".to_string());
        let payload = subscription_payload("orderbook.ETH-PERP.1.10", &orderbook_json());

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_trades_subscription_filters_and_emits_active_instrument() {
        let instrument = perp_instrument();
        let other = btc_perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        cache_instrument(&ctx.instruments, &other);
        ctx.active_trade_subs.insert(instrument_id);
        let payload = subscription_payload(
            "trades.perp.ETH",
            &json!([
                trade_json("ETH-PERP", "trade-1"),
                trade_json("BTC-PERP", "trade-2")
            ]),
        );

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::Trade(trade)) => {
                assert_eq!(trade.instrument_id, instrument_id);
                assert_eq!(trade.trade_id.to_string(), "trade-1");
                assert_eq!(trade.price, Price::from("3500.00"));
                assert_eq!(trade.size, Quantity::from("1.000"));
            }
            other => panic!("expected trade data event, was {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_handle_subscription_without_cached_instrument_emits_no_event() {
        let (mut ctx, mut rx) = make_ctx(None);
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        ctx.active_ticker_channels
            .insert(instrument_id, "ticker_slim.ETH-PERP.1000".to_string());
        ctx.active_quote_subs.insert(instrument_id);
        let payload =
            subscription_payload("ticker_slim.ETH-PERP.1000", &ticker_json(1_700_000_000_000));

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_frame_ignored_without_quote_subscription() {
        let instrument = perp_instrument();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        let payload =
            subscription_payload("ticker_slim.ETH-PERP.1000", &ticker_json(1_700_000_000_000));

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_frame_ignored_for_inactive_channel() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        ctx.active_ticker_channels
            .insert(instrument_id, "ticker_slim.ETH-PERP.100".to_string());
        ctx.active_quote_subs.insert(instrument_id);
        let payload =
            subscription_payload("ticker_slim.ETH-PERP.1000", &ticker_json(1_700_000_000_000));

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_trade_channel_uses_instrument_type_and_currency() {
        let instrument = perp_instrument();

        assert_eq!(trade_channel(&instrument).unwrap(), "trades.perp.ETH");
    }

    #[rstest]
    fn test_trade_channel_uses_erc20_for_spot() {
        let instrument = spot_instrument();

        assert_eq!(
            derive_instrument_type(&instrument).unwrap(),
            DeriveInstrumentType::Erc20
        );
        assert_eq!(trade_channel(&instrument).unwrap(), "trades.erc20.ETH");
    }

    #[rstest]
    fn test_param_defaults_match_derive_public_channels() {
        assert_eq!(orderbook_group(&None).unwrap(), DEFAULT_ORDERBOOK_GROUP);
        assert_eq!(
            orderbook_depth(None, &None).unwrap(),
            DEFAULT_ORDERBOOK_DEPTH
        );
        assert_eq!(ticker_interval(&None).unwrap(), DEFAULT_TICKER_INTERVAL);
    }

    #[rstest]
    fn test_orderbook_channel_parts_splits_from_right() {
        assert_eq!(
            orderbook_channel_parts("orderbook.ETH.TEST-PERP.10.100").unwrap(),
            (
                "ETH.TEST-PERP".to_string(),
                "10".to_string(),
                "100".to_string()
            )
        );
    }

    #[rstest]
    fn test_ticker_channel_parts_splits_from_right() {
        assert_eq!(
            ticker_channel_parts("ticker_slim.ETH.TEST-PERP.1000").unwrap(),
            ("ETH.TEST-PERP".to_string(), "1000".to_string())
        );
    }

    #[rstest]
    fn test_ticker_channel_parts_accepts_legacy_ticker_channel() {
        assert_eq!(
            ticker_channel_parts("ticker.ETH.TEST-PERP.1000").unwrap(),
            ("ETH.TEST-PERP".to_string(), "1000".to_string())
        );
    }

    fn perp_ticker_payload(instrument_id: InstrumentId) -> WsSubscriptionPayload {
        let channel = "ticker_slim.ETH-PERP.1000";
        let payload = subscription_payload(
            channel,
            &json!({
                "timestamp": 1_700_000_000_010_i64,
                "instrument_ticker": ticker_json(1_700_000_000_000)
            }),
        );
        assert_eq!(payload.channel.as_str(), channel);
        let _ = instrument_id;
        payload
    }

    fn install_ticker(ctx: &WsMessageContext, instrument_id: InstrumentId, channel: &str) {
        ctx.active_ticker_channels
            .insert(instrument_id, channel.to_string());
    }

    #[rstest]
    fn test_ticker_emits_mark_price_when_mark_subscribed() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, "ticker_slim.ETH-PERP.1000");
        ctx.active_mark_subs.insert(instrument_id);

        DeriveDataClient::handle_ws_message(
            DeriveWsMessage::Subscription(perp_ticker_payload(instrument_id)),
            &mut ctx,
        );

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::MarkPriceUpdate(mark)) => {
                assert_eq!(mark.instrument_id, instrument_id);
                assert_eq!(mark.value, Price::from("3500.50"));
            }
            other => panic!("expected MarkPriceUpdate, was {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_emits_index_price_when_index_subscribed() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, "ticker_slim.ETH-PERP.1000");
        ctx.active_index_subs.insert(instrument_id);

        DeriveDataClient::handle_ws_message(
            DeriveWsMessage::Subscription(perp_ticker_payload(instrument_id)),
            &mut ctx,
        );

        match rx.try_recv().unwrap() {
            DataEvent::Data(Data::IndexPriceUpdate(index)) => {
                assert_eq!(index.instrument_id, instrument_id);
                assert_eq!(index.value, Price::from("3500.00"));
            }
            other => panic!("expected IndexPriceUpdate, was {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_emits_funding_rate_for_perp_when_subscribed() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, "ticker_slim.ETH-PERP.1000");
        ctx.active_funding_subs.insert(instrument_id);

        DeriveDataClient::handle_ws_message(
            DeriveWsMessage::Subscription(perp_ticker_payload(instrument_id)),
            &mut ctx,
        );

        match rx.try_recv().unwrap() {
            DataEvent::FundingRate(update) => {
                assert_eq!(update.instrument_id, instrument_id);
                assert_eq!(update.rate, "0.0002".parse().unwrap());
            }
            other => panic!("expected FundingRateUpdate, was {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_skips_funding_rate_when_not_perp() {
        let instrument = option_instrument();
        let instrument_id = instrument.id();
        let channel = format!("ticker_slim.{}.1000", instrument_id.symbol.as_str());
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, &channel);
        ctx.active_funding_subs.insert(instrument_id);

        let mut option_data = option_ticker_json(1_700_000_000_000);
        option_data["instrument_name"] = json!(instrument_id.symbol.as_str());
        let payload = subscription_payload(
            &channel,
            &json!({
                "timestamp": 1_700_000_000_010_i64,
                "instrument_ticker": option_data
            }),
        );

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_emits_option_greeks_when_subscribed() {
        let instrument = option_instrument();
        let instrument_id = instrument.id();
        let channel = format!("ticker_slim.{}.1000", instrument_id.symbol.as_str());
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, &channel);
        ctx.active_greeks_subs.insert(instrument_id);

        let mut option_data = option_ticker_json(1_700_000_000_000);
        option_data["instrument_name"] = json!(instrument_id.symbol.as_str());
        let payload = subscription_payload(
            &channel,
            &json!({
                "timestamp": 1_700_000_000_010_i64,
                "instrument_ticker": option_data
            }),
        );

        DeriveDataClient::handle_ws_message(DeriveWsMessage::Subscription(payload), &mut ctx);

        match rx.try_recv().unwrap() {
            DataEvent::OptionGreeks(greeks) => {
                assert_eq!(greeks.instrument_id, instrument_id);
                assert!((greeks.greeks.delta - 0.55).abs() < 1e-9);
                assert!((greeks.greeks.gamma - 0.0008).abs() < 1e-9);
                assert!((greeks.greeks.vega - 4.5).abs() < 1e-9);
                assert!((greeks.greeks.theta + 2.1).abs() < 1e-9);
                assert!((greeks.greeks.rho - 1.2).abs() < 1e-9);
                assert_eq!(greeks.mark_iv, Some(0.60));
                assert_eq!(greeks.bid_iv, Some(0.58));
                assert_eq!(greeks.ask_iv, Some(0.62));
                assert_eq!(greeks.underlying_price, Some(3505.0));
                assert_eq!(greeks.open_interest, Some(1000.0));
            }
            other => panic!("expected OptionGreeks, was {other:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ticker_emits_all_subscribed_feeds_in_one_frame() {
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        let (mut ctx, mut rx) = make_ctx(Some(instrument));
        install_ticker(&ctx, instrument_id, "ticker_slim.ETH-PERP.1000");
        ctx.active_quote_subs.insert(instrument_id);
        ctx.active_mark_subs.insert(instrument_id);
        ctx.active_index_subs.insert(instrument_id);
        ctx.active_funding_subs.insert(instrument_id);

        DeriveDataClient::handle_ws_message(
            DeriveWsMessage::Subscription(perp_ticker_payload(instrument_id)),
            &mut ctx,
        );

        let mut quote = None;
        let mut mark = None;
        let mut index = None;
        let mut funding = None;

        while let Ok(event) = rx.try_recv() {
            match event {
                DataEvent::Data(Data::Quote(q)) => {
                    assert!(quote.replace(q).is_none(), "duplicate Quote emission");
                }
                DataEvent::Data(Data::MarkPriceUpdate(m)) => {
                    assert!(
                        mark.replace(m).is_none(),
                        "duplicate MarkPriceUpdate emission"
                    );
                }
                DataEvent::Data(Data::IndexPriceUpdate(i)) => {
                    assert!(
                        index.replace(i).is_none(),
                        "duplicate IndexPriceUpdate emission"
                    );
                }
                DataEvent::FundingRate(f) => {
                    assert!(
                        funding.replace(f).is_none(),
                        "duplicate FundingRate emission"
                    );
                }
                other => panic!("unexpected event: {other:?}"),
            }
        }

        let quote = quote.expect("Quote event missing");
        let mark = mark.expect("MarkPriceUpdate missing");
        let index = index.expect("IndexPriceUpdate missing");
        let funding = funding.expect("FundingRateUpdate missing");

        assert_eq!(quote.instrument_id, instrument_id);
        assert_eq!(quote.bid_price, Price::from("3500.00"));
        assert_eq!(quote.ask_price, Price::from("3501.00"));
        assert_eq!(mark.instrument_id, instrument_id);
        assert_eq!(mark.value, Price::from("3500.50"));
        assert_eq!(index.instrument_id, instrument_id);
        assert_eq!(index.value, Price::from("3500.00"));
        assert_eq!(funding.instrument_id, instrument_id);
        assert_eq!(funding.rate, "0.0002".parse().unwrap());
    }

    #[rstest]
    fn test_reset_clears_all_subscription_state() {
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let config = DeriveDataClientConfig {
            environment: DeriveEnvironment::Mainnet,
            ..Default::default()
        };
        let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config).unwrap();
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        cache_instrument(&client.instruments, &instrument);

        client
            .active_book_delta_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        client
            .active_book_depth10_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        client
            .active_ticker_channels
            .insert(instrument_id, "ticker_slim.ETH-PERP.1000".to_string());
        client.active_quote_subs.insert(instrument_id);
        client.active_trade_subs.insert(instrument_id);
        client
            .active_trade_channels
            .insert("trades.perp.ETH".to_string(), ());
        client.active_mark_subs.insert(instrument_id);
        client.active_index_subs.insert(instrument_id);
        client.active_funding_subs.insert(instrument_id);
        client.active_greeks_subs.insert(instrument_id);

        client.reset().unwrap();

        assert!(!client.instruments.contains_key(&instrument_id));
        assert!(
            !client
                .active_book_delta_channels
                .contains_key(&instrument_id)
        );
        assert!(
            !client
                .active_book_depth10_channels
                .contains_key(&instrument_id)
        );
        assert!(!client.active_ticker_channels.contains_key(&instrument_id));
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.active_trade_subs.contains(&instrument_id));
        assert!(client.active_trade_channels.is_empty());
        assert!(!client.active_mark_subs.contains(&instrument_id));
        assert!(!client.active_index_subs.contains(&instrument_id));
        assert!(!client.active_funding_subs.contains(&instrument_id));
        assert!(!client.active_greeks_subs.contains(&instrument_id));
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_disconnect_clears_subscription_state() {
        // Regression: aborting in-flight subscribe tasks during disconnect()
        // skips their on-error rollback, so any `active_*` entries staged
        // before spawn would leak across reconnect and silently suppress
        // the next subscribe. disconnect() must clear the same local
        // subscription maps that reset() does.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let config = DeriveDataClientConfig {
            environment: DeriveEnvironment::Mainnet,
            ..Default::default()
        };
        let mut client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config).unwrap();
        let instrument = perp_instrument();
        let instrument_id = instrument.id();
        cache_instrument(&client.instruments, &instrument);

        client
            .active_book_delta_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        client
            .active_book_depth10_channels
            .insert(instrument_id, "orderbook.ETH-PERP.1.10".to_string());
        client
            .active_ticker_channels
            .insert(instrument_id, "ticker_slim.ETH-PERP.1000".to_string());
        client.active_quote_subs.insert(instrument_id);
        client.active_trade_subs.insert(instrument_id);
        client
            .active_trade_channels
            .insert("trades.perp.ETH".to_string(), ());
        client.active_mark_subs.insert(instrument_id);
        client.active_index_subs.insert(instrument_id);
        client.active_funding_subs.insert(instrument_id);
        client.active_greeks_subs.insert(instrument_id);
        client.is_connected.store(true, Ordering::Relaxed);

        client.disconnect().await.unwrap();

        // Instrument cache must survive disconnect (reset() is the path
        // that wipes it); only the subscription maps are cleared.
        assert!(client.instruments.contains_key(&instrument_id));
        assert!(
            !client
                .active_book_delta_channels
                .contains_key(&instrument_id)
        );
        assert!(
            !client
                .active_book_depth10_channels
                .contains_key(&instrument_id)
        );
        assert!(!client.active_ticker_channels.contains_key(&instrument_id));
        assert!(!client.active_quote_subs.contains(&instrument_id));
        assert!(!client.active_trade_subs.contains(&instrument_id));
        assert!(client.active_trade_channels.is_empty());
        assert!(!client.active_mark_subs.contains(&instrument_id));
        assert!(!client.active_index_subs.contains(&instrument_id));
        assert!(!client.active_funding_subs.contains(&instrument_id));
        assert!(!client.active_greeks_subs.contains(&instrument_id));
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_spawn_task_prunes_finished_handles() {
        // Regression: every spawn_task call must prune finished handles
        // before pushing the new one, otherwise `pending_tasks` grows
        // unboundedly across long-running sessions.
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        replace_data_event_sender(tx);

        let config = DeriveDataClientConfig {
            environment: DeriveEnvironment::Mainnet,
            ..Default::default()
        };
        let client = DeriveDataClient::new(*DERIVE_CLIENT_ID, config).unwrap();

        // Spawn many no-op tasks, then wait until their handles are
        // observably finished. `spawn_task` uses the global Nautilus runtime,
        // so a single test-runtime yield is not a reliable completion fence
        // under a busy full-suite run.
        for _ in 0..100 {
            client.spawn_task("test_noop", async { Ok(()) });
        }

        wait_until_async(
            || async {
                {
                    let tasks = client.pending_tasks.lock().expect(MUTEX_POISONED);
                    tasks.iter().all(JoinHandle::is_finished)
                }
            },
            Duration::from_secs(2),
        )
        .await;

        // The next spawn should prune the finished handles before pushing the
        // new one, leaving exactly the new tracked task.
        client.spawn_task("test_prune", async { Ok(()) });
        let len = client.pending_tasks.lock().expect(MUTEX_POISONED).len();
        assert_eq!(len, 1, "pending_tasks should retain only the new task");
    }
}
