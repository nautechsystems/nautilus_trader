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

//! Live market data client implementation for the dYdX adapter.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use dashmap::{DashMap, DashSet};
use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, InstrumentResponse, InstrumentsResponse, RequestBars, RequestInstrument,
            RequestInstruments, RequestTrades, SubscribeBars, SubscribeBookDeltas,
            SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeInstrument, UnsubscribeInstruments, UnsubscribeMarkPrices,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data as NautilusData, OrderBookDelta,
        OrderBookDeltas, OrderBookDeltas_API, QuoteTick,
    },
    enums::{BookAction, BookType, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::Quantity,
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::DYDX_VENUE, enums::DydxCandleResolution, instrument_cache::InstrumentCache,
        parse::extract_raw_symbol,
    },
    config::DydxDataClientConfig,
    http::client::DydxHttpClient,
    websocket::{client::DydxWebSocketClient, enums::NautilusWsMessage, handler::HandlerCommand},
};

/// Groups WebSocket message handling dependencies.
struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_cache: Arc<InstrumentCache>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    ws_client: DydxWebSocketClient,
    http_client: DydxHttpClient,
    active_quote_subs: Arc<DashSet<InstrumentId>>,
    active_delta_subs: Arc<DashSet<InstrumentId>>,
    active_trade_subs: Arc<DashMap<InstrumentId, ()>>,
    active_bar_subs: Arc<DashMap<(InstrumentId, String), BarType>>,
    incomplete_bars: Arc<DashMap<BarType, Bar>>,
    active_mark_price_subs: Arc<DashSet<InstrumentId>>,
    active_index_price_subs: Arc<DashSet<InstrumentId>>,
    active_funding_rate_subs: Arc<DashSet<InstrumentId>>,
}

/// dYdX data client for live market data streaming and historical data requests.
///
/// This client integrates with the Nautilus DataEngine to provide:
/// - Real-time market data via WebSocket subscriptions
/// - Historical data via REST API requests
/// - Automatic instrument discovery and caching
/// - Connection lifecycle management
#[derive(Debug)]
pub struct DydxDataClient {
    /// High-resolution clock for timestamps.
    clock: &'static AtomicTime,
    /// The client ID for this data client.
    client_id: ClientId,
    /// Configuration for the data client.
    config: DydxDataClientConfig,
    /// HTTP client for REST API requests.
    http_client: DydxHttpClient,
    /// WebSocket client for real-time data streaming.
    ws_client: DydxWebSocketClient,
    /// Whether the client is currently connected.
    is_connected: AtomicBool,
    /// Cancellation token for async operations.
    cancellation_token: CancellationToken,
    /// Background task handles.
    tasks: Vec<JoinHandle<()>>,
    /// Channel sender for emitting data events to the DataEngine.
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    /// Shared instrument cache (with HTTP client and execution client).
    instrument_cache: Arc<InstrumentCache>,
    /// Local order books maintained for generating quotes and resolving crosses.
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    /// Last quote tick per instrument (used for quote generation from book deltas).
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    /// Incomplete bars cache for bar aggregation.
    /// Tracks bars not yet closed (ts_event > current_time), keyed by BarType.
    /// Bars are emitted only when they close (ts_event <= current_time).
    incomplete_bars: Arc<DashMap<BarType, Bar>>,
    /// WebSocket topic to BarType mappings.
    /// Maps dYdX candle topics (e.g., "BTC-USD/1MIN") to Nautilus BarType.
    /// Used for subscription validation and reconnection recovery.
    bar_type_mappings: Arc<DashMap<String, BarType>>,
    /// Active quote subscriptions (instruments expecting `QuoteTick` events).
    active_quote_subs: Arc<DashSet<InstrumentId>>,
    /// Active orderbook delta subscriptions (instruments expecting `OrderBookDeltas` events).
    active_delta_subs: Arc<DashSet<InstrumentId>>,
    /// Active trade subscriptions for reconnection recovery.
    active_trade_subs: Arc<DashMap<InstrumentId, ()>>,
    /// Active bar/candle subscriptions for reconnection recovery (maps instrument+resolution to BarType).
    active_bar_subs: Arc<DashMap<(InstrumentId, String), BarType>>,
    /// Active mark price subscriptions (instruments expecting `MarkPriceUpdate` events).
    active_mark_price_subs: Arc<DashSet<InstrumentId>>,
    /// Active index price subscriptions (instruments expecting `IndexPriceUpdate` events).
    active_index_price_subs: Arc<DashSet<InstrumentId>>,
    /// Active funding rate subscriptions (instruments expecting `FundingRateUpdate` events).
    active_funding_rate_subs: Arc<DashSet<InstrumentId>>,
}

impl DydxDataClient {
    fn map_bar_spec_to_resolution(spec: &BarSpecification) -> anyhow::Result<&'static str> {
        let resolution: &'static str = DydxCandleResolution::from_bar_spec(spec)?.into();
        Ok(resolution)
    }

    /// Creates a new [`DydxDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(
        client_id: ClientId,
        config: DydxDataClientConfig,
        http_client: DydxHttpClient,
        ws_client: DydxWebSocketClient,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        // Share the instrument cache from HTTP client
        let instrument_cache = Arc::clone(http_client.instrument_cache());

        Ok(Self {
            clock,
            client_id,
            config,
            http_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instrument_cache,
            order_books: Arc::new(DashMap::new()),
            last_quotes: Arc::new(DashMap::new()),
            incomplete_bars: Arc::new(DashMap::new()),
            bar_type_mappings: Arc::new(DashMap::new()),
            active_quote_subs: Arc::new(DashSet::new()),
            active_delta_subs: Arc::new(DashSet::new()),
            active_trade_subs: Arc::new(DashMap::new()),
            active_bar_subs: Arc::new(DashMap::new()),
            active_mark_price_subs: Arc::new(DashSet::new()),
            active_index_price_subs: Arc::new(DashSet::new()),
            active_funding_rate_subs: Arc::new(DashSet::new()),
        })
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *DYDX_VENUE
    }

    /// Returns a reference to the client configuration.
    #[must_use]
    pub fn config(&self) -> &DydxDataClientConfig {
        &self.config
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }

    fn spawn_ws_stream_handler(
        &mut self,
        stream: impl Stream<Item = NautilusWsMessage> + Send + 'static,
        ctx: WsMessageContext,
    ) {
        let cancellation = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            log::debug!("Message processing task started");
            pin_mut!(stream);

            loop {
                tokio::select! {
                    maybe_msg = stream.next() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &ctx),
                            None => {
                                log::debug!("WebSocket message channel closed");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("WebSocket message task cancelled");
                        break;
                    }
                }
            }
            log::debug!("WebSocket stream handler ended");
        });

        self.tasks.push(handle);
    }

    async fn await_tasks_with_timeout(&mut self, timeout: Duration) {
        for handle in self.tasks.drain(..) {
            let _ = tokio::time::timeout(timeout, handle).await;
        }
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<Vec<InstrumentAny>> {
        // Fetch instruments via HTTP - this populates the shared InstrumentCache
        self.http_client
            .fetch_and_cache_instruments()
            .await
            .context("failed to load instruments from dYdX")?;

        let instruments: Vec<InstrumentAny> = self.http_client.all_instruments();

        if instruments.is_empty() {
            log::warn!("No instruments were loaded");
            return Ok(instruments);
        }

        log::info!("Loaded {} instruments into shared cache", instruments.len());

        // Cache in WebSocket client for handler lookups
        self.ws_client.cache_instruments(instruments.clone());

        // Publish all instruments to the data engine so they're available in the shared Cache
        for instrument in &instruments {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to publish instrument {}: {e}", instrument.id());
            }
        }
        log::debug!("Published {} instruments to data engine", instruments.len());

        Ok(instruments)
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for DydxDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DYDX_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting: client_id={}, is_testnet={}",
            self.client_id,
            self.http_client.is_testnet()
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        // Abort remaining tasks instead of just dropping handles to prevent resource leaks
        for handle in self.tasks.drain(..) {
            handle.abort();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {}", self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        log::info!("Connecting");

        // Bootstrap instruments first
        self.bootstrap_instruments().await?;

        // Connect WebSocket client and subscribe to market updates
        self.ws_client
            .connect()
            .await
            .context("failed to connect dYdX websocket")?;

        self.ws_client
            .subscribe_markets()
            .await
            .context("failed to subscribe to markets channel")?;

        // Start message processing task (handler already converts to NautilusWsMessage)
        let ctx = WsMessageContext {
            clock: self.clock,
            data_sender: self.data_sender.clone(),
            instrument_cache: self.instrument_cache.clone(),
            order_books: self.order_books.clone(),
            last_quotes: self.last_quotes.clone(),
            ws_client: self.ws_client.clone(),
            http_client: self.http_client.clone(),
            active_quote_subs: self.active_quote_subs.clone(),
            active_delta_subs: self.active_delta_subs.clone(),
            active_trade_subs: self.active_trade_subs.clone(),
            active_bar_subs: self.active_bar_subs.clone(),
            incomplete_bars: self.incomplete_bars.clone(),
            active_mark_price_subs: self.active_mark_price_subs.clone(),
            active_index_price_subs: self.active_index_price_subs.clone(),
            active_funding_rate_subs: self.active_funding_rate_subs.clone(),
        };

        let stream = self.ws_client.stream();
        self.spawn_ws_stream_handler(stream, ctx);

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting");

        // Cancel all tasks
        self.cancellation_token.cancel();

        // Await tasks with timeout for graceful shutdown
        self.await_tasks_with_timeout(Duration::from_secs(5)).await;

        self.ws_client
            .disconnect()
            .await
            .context("failed to disconnect dYdX websocket")?;

        self.is_connected.store(false, Ordering::Relaxed);
        log::info!("Disconnected dYdX data client");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        // dYdX uses a global markets channel which streams instruments implicitly.
        // There is no dedicated instruments subscription, so this is a no-op to
        // mirror the behaviour of `subscribe_instruments`.
        log::debug!("unsubscribe_instruments: dYdX markets channel is global; no-op");
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        // dYdX does not support per-instrument instrument feed subscriptions.
        // The markets channel always streams all instruments, so this is a no-op.
        log::debug!("unsubscribe_instrument: dYdX markets channel is global; no-op");
        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        // dYdX markets channel auto-subscribes to all instruments
        // No explicit subscription needed - already handled in connect()
        log::debug!("subscribe_instruments: dYdX auto-subscribes via markets channel");
        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        // dYdX instruments are already cached from HTTP during connect()
        // Look up and send the requested instrument to the data engine
        if let Some(instrument) = self.instrument_cache.get(&cmd.instrument_id) {
            log::debug!("Sending cached instrument for {}", cmd.instrument_id);
            if let Err(e) = self.data_sender.send(DataEvent::Instrument(instrument)) {
                log::warn!("Failed to send instrument {}: {e}", cmd.instrument_id);
            }
        } else {
            log::warn!(
                "Instrument {} not found in cache (available: {})",
                cmd.instrument_id,
                self.instrument_cache.len()
            );
        }
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_mark_price_subs.insert(instrument_id);
        log::info!("Subscribed to mark prices for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_index_price_subs.insert(instrument_id);
        log::info!("Subscribed to index prices for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_funding_rate_subs.insert(instrument_id);
        log::info!("Subscribed to funding rates for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        self.active_mark_price_subs.remove(&cmd.instrument_id);
        log::info!("Unsubscribed from mark prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        self.active_index_price_subs.remove(&cmd.instrument_id);
        log::info!("Unsubscribed from index prices for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        self.active_funding_rate_subs.remove(&cmd.instrument_id);
        log::info!("Unsubscribed from funding rates for {}", cmd.instrument_id);
        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        // Track active subscription for reconnection recovery
        self.active_trade_subs.insert(instrument_id, ());

        self.spawn_ws(
            async move {
                ws.subscribe_trades(instrument_id)
                    .await
                    .context("trade subscription")
            },
            "dYdX trade subscription",
        );

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!(
                "dYdX only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        // Ensure local order book exists for this instrument.
        self.ensure_order_book(cmd.instrument_id, BookType::L2_MBP);

        // Track active delta subscription
        self.active_delta_subs.insert(cmd.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.subscribe_orderbook(instrument_id)
                    .await
                    .context("orderbook subscription")
            },
            "dYdX orderbook subscription",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        // dYdX doesn't have a dedicated quotes channel —
        // quotes are synthesized from order book deltas (top-of-book).
        log::debug!(
            "Subscribe_quotes for {}: subscribing to orderbook WS channel for quote synthesis",
            cmd.instrument_id
        );

        self.ensure_order_book(cmd.instrument_id, BookType::L2_MBP);
        self.active_quote_subs.insert(cmd.instrument_id);
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.subscribe_orderbook(instrument_id)
                    .await
                    .context("orderbook subscription (for quotes)")
            },
            "dYdX orderbook subscription (quotes)",
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let spec = cmd.bar_type.spec();

        // Use centralized bar spec mapping
        let resolution = Self::map_bar_spec_to_resolution(&spec)?;

        // Track active subscription for reconnection recovery
        let bar_type = cmd.bar_type;
        self.active_bar_subs
            .insert((instrument_id, resolution.to_string()), bar_type);

        // Register topic → BarType mapping for validation and lookup
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");
        self.bar_type_mappings.insert(topic.clone(), bar_type);

        self.spawn_ws(
            async move {
                // Register bar type in handler BEFORE subscribing to avoid race condition
                if let Err(e) = ws.send_command(HandlerCommand::RegisterBarType { topic, bar_type })
                {
                    anyhow::bail!("Failed to register bar type: {e}");
                }

                // Delay to ensure handler processes registration before candle messages arrive
                tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;

                ws.subscribe_candles(instrument_id, resolution)
                    .await
                    .context("candles subscription")
            },
            "dYdX candles subscription",
        );

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        // Remove from active subscription tracking
        self.active_trade_subs.remove(&cmd.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(instrument_id)
                    .await
                    .context("trade unsubscription")
            },
            "dYdX trade unsubscription",
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        // Remove from active delta subscription tracking
        self.active_delta_subs.remove(&cmd.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.unsubscribe_orderbook(instrument_id)
                    .await
                    .context("orderbook unsubscription")
            },
            "dYdX orderbook unsubscription",
        );

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        log::debug!(
            "unsubscribe_quotes for {}: removing quote subscription",
            cmd.instrument_id
        );

        // Remove from active quote subscription tracking
        self.active_quote_subs.remove(&cmd.instrument_id);

        // Unsubscribe from WS orderbook channel (refcount handles dedup —
        // only sends WS unsubscribe when no delta sub remains either)
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.spawn_ws(
            async move {
                ws.unsubscribe_orderbook(instrument_id)
                    .await
                    .context("orderbook unsubscription (for quotes)")
            },
            "dYdX orderbook unsubscription (quotes)",
        );

        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let spec = cmd.bar_type.spec();

        let resolution = Self::map_bar_spec_to_resolution(&spec)?;

        // Remove from active subscription tracking
        self.active_bar_subs
            .remove(&(instrument_id, resolution.to_string()));

        // Unregister bar type from handler and local mappings
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");
        self.bar_type_mappings.remove(&topic);

        if let Err(e) = ws.send_command(HandlerCommand::UnregisterBarType { topic }) {
            log::warn!("Failed to unregister bar type: {e}");
        }

        self.spawn_ws(
            async move {
                ws.unsubscribe_candles(instrument_id, resolution)
                    .await
                    .context("candles unsubscription")
            },
            "dYdX candles unsubscription",
        );

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        if request.start.is_some() {
            log::warn!(
                "Requesting instrument {} with specified `start` which has no effect",
                request.instrument_id
            );
        }

        if request.end.is_some() {
            log::warn!(
                "Requesting instrument {} with specified `end` which has no effect",
                request.instrument_id
            );
        }

        let instrument_cache = self.instrument_cache.clone();
        let sender = self.data_sender.clone();
        let http = self.http_client.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            // First try to get from cache
            let instrument = if let Some(cached) = instrument_cache.get(&instrument_id) {
                log::debug!("Found instrument {instrument_id} in cache");
                Some(cached)
            } else {
                // Not in cache, fetch from API
                log::debug!("Instrument {instrument_id} not in cache, fetching from API");
                match http.request_instruments(None, None, None).await {
                    Ok(instruments) => {
                        // Cache all fetched instruments
                        for inst in &instruments {
                            instrument_cache.insert_instrument_only(inst.clone());
                        }
                        // Find the requested instrument
                        instruments.into_iter().find(|i| i.id() == instrument_id)
                    }
                    Err(e) => {
                        log::error!("Failed to fetch instruments from dYdX: {e:?}");
                        None
                    }
                }
            };

            if let Some(inst) = instrument {
                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request_id,
                    client_id,
                    instrument_id,
                    inst,
                    start_nanos,
                    end_nanos,
                    clock.get_time_ns(),
                    params,
                )));

                if let Err(e) = sender.send(DataEvent::Response(response)) {
                    log::error!("Failed to send instrument response: {e}");
                }
            } else {
                log::error!("Instrument {instrument_id} not found");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_cache = self.instrument_cache.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = self.venue();
        let start = request.start;
        let end = request.end;
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http.request_instruments(None, None, None).await {
                Ok(instruments) => {
                    log::info!("Fetched {} instruments from dYdX", instruments.len());

                    // Cache all instruments
                    for instrument in &instruments {
                        instrument_cache.insert_instrument_only(instrument.clone());
                    }

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
                Err(e) => {
                    log::error!("Failed to fetch instruments from dYdX: {e:?}");

                    // Send empty response on error
                    let response = DataResponse::Instruments(InstrumentsResponse::new(
                        request_id,
                        client_id,
                        venue,
                        Vec::new(),
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send empty instruments response: {e}");
                    }
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http_client
                .request_trade_ticks(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from dYdX")
            {
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
                Err(e) => log::error!("Trade request failed for {instrument_id}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        get_runtime().spawn(async move {
            match http_client
                .request_bars(bar_type, start, end, limit, true)
                .await
                .context("failed to request bars from dYdX")
            {
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
                Err(e) => log::error!("Bar request failed for {bar_type}: {e:?}"),
            }
        });

        Ok(())
    }
}

impl DydxDataClient {
    /// Get a cached instrument by InstrumentId.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instrument_cache.get(instrument_id)
    }

    /// Get all cached instruments.
    #[must_use]
    pub fn get_instruments(&self) -> Vec<InstrumentAny> {
        self.instrument_cache.all_instruments()
    }

    /// Cache a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instrument_cache.insert_instrument_only(instrument);
    }

    /// Cache multiple instruments.
    ///
    /// Clears the existing cache first, then adds all provided instruments.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        self.instrument_cache.clear();
        self.instrument_cache.insert_instruments_only(instruments);
    }

    fn ensure_order_book(&self, instrument_id: InstrumentId, book_type: BookType) {
        self.order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, book_type));
    }

    /// Get BarType for a given WebSocket candle topic.
    #[must_use]
    pub fn get_bar_type_for_topic(&self, topic: &str) -> Option<BarType> {
        self.bar_type_mappings
            .get(topic)
            .map(|entry| *entry.value())
    }

    /// Get all registered bar topics.
    #[must_use]
    pub fn get_bar_topics(&self) -> Vec<String> {
        self.bar_type_mappings
            .iter()
            .map(|entry| entry.key().clone())
            .collect()
    }

    fn handle_ws_message(message: NautilusWsMessage, ctx: &WsMessageContext) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                Self::handle_data_message(
                    payloads,
                    &ctx.data_sender,
                    &ctx.incomplete_bars,
                    ctx.clock,
                );
            }
            NautilusWsMessage::Deltas(deltas) => {
                Self::handle_deltas_message(
                    *deltas,
                    &ctx.data_sender,
                    &ctx.order_books,
                    &ctx.last_quotes,
                    &ctx.instrument_cache,
                    &ctx.active_quote_subs,
                    &ctx.active_delta_subs,
                );
            }
            NautilusWsMessage::MarkPrice(mark_price) => {
                if ctx
                    .active_mark_price_subs
                    .contains(&mark_price.instrument_id)
                {
                    let data = NautilusData::MarkPriceUpdate(mark_price);
                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit mark price: {e}");
                    }
                }
            }
            NautilusWsMessage::IndexPrice(index_price) => {
                if ctx
                    .active_index_price_subs
                    .contains(&index_price.instrument_id)
                {
                    let data = NautilusData::IndexPriceUpdate(index_price);
                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit index price: {e}");
                    }
                }
            }
            NautilusWsMessage::FundingRate(funding_rate) => {
                if ctx
                    .active_funding_rate_subs
                    .contains(&funding_rate.instrument_id)
                    && let Err(e) = ctx.data_sender.send(DataEvent::FundingRate(funding_rate))
                {
                    log::error!("Failed to emit funding rate: {e}");
                }
            }
            NautilusWsMessage::Error(err) => {
                log::error!("dYdX WS error: {err}");
            }
            NautilusWsMessage::Reconnected => {
                log::info!("dYdX WS reconnected - re-subscribing to active subscriptions");

                let total_subs = ctx.active_quote_subs.len()
                    + ctx.active_delta_subs.len()
                    + ctx.active_trade_subs.len()
                    + ctx.active_bar_subs.len();

                if total_subs == 0 {
                    log::debug!("No active subscriptions to restore");
                    return;
                }

                log::info!(
                    "Restoring {} subscriptions (quotes={}, deltas={}, trades={}, bars={})",
                    total_subs,
                    ctx.active_quote_subs.len(),
                    ctx.active_delta_subs.len(),
                    ctx.active_trade_subs.len(),
                    ctx.active_bar_subs.len()
                );

                // Re-subscribe for quote subscriptions (bumps WS refcount)
                for instrument_id in ctx.active_quote_subs.iter() {
                    let instrument_id = *instrument_id;
                    let ws_clone = ctx.ws_client.clone();
                    get_runtime().spawn(async move {
                        if let Err(e) = ws_clone.subscribe_orderbook(instrument_id).await {
                            log::error!(
                                "Failed to re-subscribe to orderbook (quotes) for {instrument_id}: {e:?}"
                            );
                        } else {
                            log::debug!("Re-subscribed to orderbook (quotes) for {instrument_id}");
                        }
                    });
                }

                // Re-subscribe for delta subscriptions (bumps WS refcount)
                for instrument_id in ctx.active_delta_subs.iter() {
                    let instrument_id = *instrument_id;
                    let ws_clone = ctx.ws_client.clone();
                    get_runtime().spawn(async move {
                        if let Err(e) = ws_clone.subscribe_orderbook(instrument_id).await {
                            log::error!(
                                "Failed to re-subscribe to orderbook (deltas) for {instrument_id}: {e:?}"
                            );
                        } else {
                            log::debug!("Re-subscribed to orderbook (deltas) for {instrument_id}");
                        }
                    });
                }

                // Re-subscribe to trade channels
                for entry in ctx.active_trade_subs.iter() {
                    let instrument_id = *entry.key();
                    let ws_clone = ctx.ws_client.clone();
                    get_runtime().spawn(async move {
                        if let Err(e) = ws_clone.subscribe_trades(instrument_id).await {
                            log::error!(
                                "Failed to re-subscribe to trades for {instrument_id}: {e:?}"
                            );
                        } else {
                            log::debug!("Re-subscribed to trades for {instrument_id}");
                        }
                    });
                }

                // Re-subscribe to candle/bar channels
                for entry in ctx.active_bar_subs.iter() {
                    let (instrument_id, resolution) = entry.key();
                    let instrument_id = *instrument_id;
                    let resolution = resolution.clone();
                    let bar_type = *entry.value();
                    let ws_clone = ctx.ws_client.clone();

                    // Re-register bar type with handler
                    let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
                    let topic = format!("{ticker}/{resolution}");

                    if let Err(e) = ctx
                        .ws_client
                        .send_command(HandlerCommand::RegisterBarType { topic, bar_type })
                    {
                        log::warn!(
                            "Failed to re-register bar type for {instrument_id} ({resolution}): {e}"
                        );
                    }

                    get_runtime().spawn(async move {
                        if let Err(e) =
                            ws_clone.subscribe_candles(instrument_id, &resolution).await
                        {
                            log::error!(
                                "Failed to re-subscribe to candles for {instrument_id} ({resolution}): {e:?}"
                            );
                        } else {
                            log::debug!(
                                "Re-subscribed to candles for {instrument_id} ({resolution})"
                            );
                        }
                    });
                }

                log::info!("Completed re-subscription requests after reconnection");
            }
            NautilusWsMessage::BlockHeight { .. } => {
                log::debug!(
                    "Ignoring block height message on dYdX data client (handled by execution adapter)"
                );
            }
            NautilusWsMessage::Order(_)
            | NautilusWsMessage::Fill(_)
            | NautilusWsMessage::Position(_)
            | NautilusWsMessage::AccountState(_)
            | NautilusWsMessage::SubaccountSubscribed(_)
            | NautilusWsMessage::SubaccountsChannelData(_) => {
                log::debug!(
                    "Ignoring execution/subaccount message on dYdX data client (handled by execution adapter)"
                );
            }
            NautilusWsMessage::NewInstrumentDiscovered { ticker } => {
                // New instrument discovered via WebSocket - fetch via HTTP and cache
                log::info!("New instrument discovered via WebSocket: {ticker}");

                let http_client = ctx.http_client.clone();
                let ws_client = ctx.ws_client.clone();
                let data_sender = ctx.data_sender.clone();

                get_runtime().spawn(async move {
                    match http_client.fetch_and_cache_single_instrument(&ticker).await {
                        Ok(Some(instrument)) => {
                            // Cache in WebSocket client for future data parsing
                            ws_client.cache_instrument(instrument.clone());
                            // The InstrumentCache is already updated by fetch_and_cache_single_instrument

                            // Send to data engine
                            if let Err(e) = data_sender.send(DataEvent::Instrument(instrument)) {
                                log::error!("Failed to emit new instrument: {e}");
                            }
                            log::info!("Fetched and cached new instrument: {ticker}");
                        }
                        Ok(None) => {
                            log::warn!("New instrument {ticker} not found or inactive");
                        }
                        Err(e) => {
                            log::error!("Failed to fetch new instrument {ticker}: {e}");
                        }
                    }
                });
            }
        }
    }

    fn handle_data_message(
        payloads: Vec<NautilusData>,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        incomplete_bars: &Arc<DashMap<BarType, Bar>>,
        clock: &'static AtomicTime,
    ) {
        for data in payloads {
            // Filter bars through incomplete bars cache
            if let NautilusData::Bar(bar) = data {
                Self::handle_bar_message(bar, data_sender, incomplete_bars, clock);
            } else if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit data event: {e}");
            }
        }
    }

    fn handle_bar_message(
        bar: Bar,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        incomplete_bars: &Arc<DashMap<BarType, Bar>>,
        clock: &'static AtomicTime,
    ) {
        let current_time_ns = clock.get_time_ns();
        let bar_type = bar.bar_type;

        if bar.ts_event <= current_time_ns {
            // Bar is complete - emit it and remove from incomplete cache
            incomplete_bars.remove(&bar_type);

            if let Err(e) = data_sender.send(DataEvent::Data(NautilusData::Bar(bar))) {
                log::error!("Failed to emit completed bar: {e}");
            }
        } else {
            // Bar is incomplete - cache it (updates existing entry)
            log::trace!(
                "Caching incomplete bar for {} (ts_event={}, current={})",
                bar_type,
                bar.ts_event,
                current_time_ns
            );
            incomplete_bars.insert(bar_type, bar);
        }
    }

    fn resolve_crossed_order_book(
        book: &mut OrderBook,
        venue_deltas: OrderBookDeltas,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<OrderBookDeltas> {
        let instrument_id = venue_deltas.instrument_id;
        let ts_init = venue_deltas.ts_init;
        let mut all_deltas = venue_deltas.deltas.clone();

        // Apply the original venue deltas first
        book.apply_deltas(&venue_deltas)?;

        // Check if orderbook is crossed
        let mut is_crossed = if let (Some(bid_price), Some(ask_price)) =
            (book.best_bid_price(), book.best_ask_price())
        {
            bid_price >= ask_price
        } else {
            false
        };

        // Iteratively uncross the orderbook
        while is_crossed {
            log::debug!(
                "Resolving crossed order book for {}: bid={:?} >= ask={:?}",
                instrument_id,
                book.best_bid_price(),
                book.best_ask_price()
            );

            let bid_price = match book.best_bid_price() {
                Some(p) => p,
                None => break,
            };
            let ask_price = match book.best_ask_price() {
                Some(p) => p,
                None => break,
            };
            let bid_size = match book.best_bid_size() {
                Some(s) => s,
                None => break,
            };
            let ask_size = match book.best_ask_size() {
                Some(s) => s,
                None => break,
            };

            let mut temp_deltas = Vec::new();

            if bid_size > ask_size {
                // Remove ask level, reduce bid level
                let new_bid_size = Quantity::new(
                    bid_size.as_f64() - ask_size.as_f64(),
                    instrument.size_precision(),
                );
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update,
                    BookOrder::new(OrderSide::Buy, bid_price, new_bid_size, 0),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        ask_price,
                        Quantity::new(0.0, instrument.size_precision()),
                        0,
                    ),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
            } else if bid_size < ask_size {
                // Remove bid level, reduce ask level
                let new_ask_size = Quantity::new(
                    ask_size.as_f64() - bid_size.as_f64(),
                    instrument.size_precision(),
                );
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update,
                    BookOrder::new(OrderSide::Sell, ask_price, new_ask_size, 0),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Buy,
                        bid_price,
                        Quantity::new(0.0, instrument.size_precision()),
                        0,
                    ),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
            } else {
                // Equal sizes: remove both levels
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Buy,
                        bid_price,
                        Quantity::new(0.0, instrument.size_precision()),
                        0,
                    ),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Delete,
                    BookOrder::new(
                        OrderSide::Sell,
                        ask_price,
                        Quantity::new(0.0, instrument.size_precision()),
                        0,
                    ),
                    0,
                    0,
                    ts_init,
                    ts_init,
                ));
            }

            // Apply temporary deltas to the book
            let temp_deltas_obj = OrderBookDeltas::new(instrument_id, temp_deltas.clone());
            book.apply_deltas(&temp_deltas_obj)?;
            all_deltas.extend(temp_deltas);

            // Check if still crossed
            is_crossed = if let (Some(bid_price), Some(ask_price)) =
                (book.best_bid_price(), book.best_ask_price())
            {
                bid_price >= ask_price
            } else {
                false
            };
        }

        // Set F_LAST flag on the final delta
        if let Some(last_delta) = all_deltas.last_mut() {
            last_delta.flags = RecordFlag::F_LAST as u8;
        }

        Ok(OrderBookDeltas::new(instrument_id, all_deltas))
    }

    fn handle_deltas_message(
        deltas: OrderBookDeltas,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
        last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
        instrument_cache: &Arc<InstrumentCache>,
        active_quote_subs: &Arc<DashSet<InstrumentId>>,
        active_delta_subs: &Arc<DashSet<InstrumentId>>,
    ) {
        let instrument_id = deltas.instrument_id;

        // Get instrument for crossed orderbook resolution
        let instrument = match instrument_cache.get(&instrument_id) {
            Some(inst) => inst,
            None => {
                log::error!("Cannot resolve crossed order book: no instrument for {instrument_id}");
                // Still emit the raw deltas if delta subscription is active
                if active_delta_subs.contains(&instrument_id)
                    && let Err(e) = data_sender.send(DataEvent::Data(NautilusData::from(
                        OrderBookDeltas_API::new(deltas),
                    )))
                {
                    log::error!("Failed to emit order book deltas: {e}");
                }
                return;
            }
        };

        // Always maintain local orderbook — both subscription types need book state
        let mut book = order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        // Resolve crossed orderbook (applies deltas internally)
        let resolved_deltas = match Self::resolve_crossed_order_book(&mut book, deltas, &instrument)
        {
            Ok(d) => d,
            Err(e) => {
                log::error!("Failed to resolve crossed order book for {instrument_id}: {e}");
                return;
            }
        };

        // Conditionally emit QuoteTick if instrument has quote subscription
        if active_quote_subs.contains(&instrument_id) {
            // Generate QuoteTick from updated top-of-book
            // Edge case: If orderbook is empty after deltas, fall back to last quote
            let quote_opt = if let (Some(bid_price), Some(ask_price)) =
                (book.best_bid_price(), book.best_ask_price())
                && let (Some(bid_size), Some(ask_size)) =
                    (book.best_bid_size(), book.best_ask_size())
            {
                Some(QuoteTick::new(
                    instrument_id,
                    bid_price,
                    ask_price,
                    bid_size,
                    ask_size,
                    resolved_deltas.ts_event,
                    resolved_deltas.ts_init,
                ))
            } else {
                // Edge case: Empty orderbook levels - use last quote as fallback
                if book.best_bid_price().is_none() && book.best_ask_price().is_none() {
                    log::debug!(
                        "Empty orderbook for {instrument_id} after applying deltas, using last quote"
                    );
                    last_quotes.get(&instrument_id).map(|q| *q)
                } else {
                    None
                }
            };

            if let Some(quote) = quote_opt {
                // Only emit when top-of-book changes
                let emit_quote = !matches!(
                    last_quotes.get(&instrument_id),
                    Some(existing) if *existing == quote
                );

                if emit_quote {
                    last_quotes.insert(instrument_id, quote);
                    if let Err(e) = data_sender.send(DataEvent::Data(NautilusData::Quote(quote))) {
                        log::error!("Failed to emit quote tick: {e}");
                    }
                }
            } else if book.best_bid_price().is_some() || book.best_ask_price().is_some() {
                // Partial orderbook (only one side) - log but don't emit
                log::debug!(
                    "Incomplete top-of-book for {instrument_id} (bid={:?}, ask={:?})",
                    book.best_bid_price(),
                    book.best_ask_price()
                );
            }
        }

        // Conditionally emit OrderBookDeltas if instrument has delta subscription
        if active_delta_subs.contains(&instrument_id) {
            let data: NautilusData = OrderBookDeltas_API::new(resolved_deltas).into();
            if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit order book deltas event: {e}");
            }
        }
    }
}
