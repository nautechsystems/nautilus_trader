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
use dashmap::DashMap;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, InstrumentResponse, InstrumentsResponse, RequestBars, RequestInstrument,
            RequestInstruments, RequestTrades, SubscribeBars, SubscribeBookDeltas,
            SubscribeInstrument, SubscribeInstruments, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeInstrument,
            UnsubscribeInstruments, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    UnixNanos,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data as NautilusData, IndexPriceUpdate,
        OrderBookDelta, OrderBookDeltas, OrderBookDeltas_API, QuoteTick, TradeTick,
    },
    enums::{
        AggregationSource, AggressorSide, BarAggregation, BookAction, BookType, OrderSide,
        PriceType, RecordFlag,
    },
    identifiers::{ClientId, InstrumentId, TradeId, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::DYDX_VENUE, enums::DydxCandleResolution, instrument_cache::InstrumentCache,
        parse::extract_raw_symbol,
    },
    config::DydxDataClientConfig,
    http::{
        client::DydxHttpClient,
        models::{Candle, OrderbookResponse},
    },
    types::DydxOraclePrice,
    websocket::{
        client::DydxWebSocketClient, enums::NautilusWsMessage, handler::HandlerCommand,
        messages::DydxOraclePriceMarket,
    },
};

/// Groups WebSocket message handling dependencies.
struct WsMessageContext {
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_cache: Arc<InstrumentCache>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    ws_client: DydxWebSocketClient,
    active_orderbook_subs: Arc<DashMap<InstrumentId, ()>>,
    active_trade_subs: Arc<DashMap<InstrumentId, ()>>,
    active_bar_subs: Arc<DashMap<(InstrumentId, String), BarType>>,
    incomplete_bars: Arc<DashMap<BarType, Bar>>,
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
    /// Active orderbook subscriptions for periodic snapshot refresh.
    active_orderbook_subs: Arc<DashMap<InstrumentId, ()>>,
    /// Active trade subscriptions for reconnection recovery.
    active_trade_subs: Arc<DashMap<InstrumentId, ()>>,
    /// Active bar/candle subscriptions for reconnection recovery (maps instrument+resolution to BarType).
    active_bar_subs: Arc<DashMap<(InstrumentId, String), BarType>>,
}

impl DydxDataClient {
    /// Maps Nautilus BarType spec to dYdX candle resolution string.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar aggregation or step is not supported by dYdX.
    fn map_bar_spec_to_resolution(spec: &BarSpecification) -> anyhow::Result<&'static str> {
        match spec.step.get() {
            1 => match spec.aggregation {
                BarAggregation::Minute => Ok("1MIN"),
                BarAggregation::Hour => Ok("1HOUR"),
                BarAggregation::Day => Ok("1DAY"),
                _ => anyhow::bail!("Unsupported bar aggregation: {:?}", spec.aggregation),
            },
            5 if spec.aggregation == BarAggregation::Minute => Ok("5MINS"),
            15 if spec.aggregation == BarAggregation::Minute => Ok("15MINS"),
            30 if spec.aggregation == BarAggregation::Minute => Ok("30MINS"),
            4 if spec.aggregation == BarAggregation::Hour => Ok("4HOURS"),
            step => anyhow::bail!(
                "Unsupported bar step: {step} with aggregation {:?}",
                spec.aggregation
            ),
        }
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
            active_orderbook_subs: Arc::new(DashMap::new()),
            active_trade_subs: Arc::new(DashMap::new()),
            active_bar_subs: Arc::new(DashMap::new()),
        })
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *DYDX_VENUE
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    /// Spawns an async WebSocket task with error handling.
    ///
    /// This helper ensures consistent error logging across all subscription methods.
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

    /// Bootstrap instruments from the dYdX Indexer API.
    ///
    /// This method:
    /// 1. Fetches all available instruments from the REST API
    /// 2. Caches them in the HTTP client
    /// 3. Caches them in the WebSocket client (if present)
    /// 4. Populates the local instruments cache
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The HTTP request fails.
    /// - Instrument parsing fails.
    ///
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
        self.tasks.clear();
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
        if let Some(rx) = self.ws_client.take_receiver() {
            log::debug!("Starting message processing task");
            let data_tx = self.data_sender.clone();
            let instrument_cache = self.instrument_cache.clone();
            let order_books = self.order_books.clone();
            let last_quotes = self.last_quotes.clone();
            let ws_client = self.ws_client.clone();
            let active_orderbook_subs = self.active_orderbook_subs.clone();
            let active_trade_subs = self.active_trade_subs.clone();
            let active_bar_subs = self.active_bar_subs.clone();
            let incomplete_bars = self.incomplete_bars.clone();

            let ctx = WsMessageContext {
                data_sender: data_tx,
                instrument_cache,
                order_books,
                last_quotes,
                ws_client,
                active_orderbook_subs,
                active_trade_subs,
                active_bar_subs,
                incomplete_bars,
            };

            let task = get_runtime().spawn(async move {
                log::debug!("Message processing task started");
                let mut rx = rx;

                while let Some(msg) = rx.recv().await {
                    Self::handle_ws_message(msg, &ctx);
                }
                log::debug!("Message processing task ended (channel closed)");
            });
            self.tasks.push(task);
        } else {
            log::error!("No inbound WS receiver available after connect");
        }

        // Start orderbook snapshot refresh task
        self.start_orderbook_refresh_task()?;

        // Start instrument refresh task
        self.start_instrument_refresh_task()?;

        self.is_connected.store(true, Ordering::Relaxed);
        log::info!("Connected");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting");

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
        let symbol = cmd.instrument_id.symbol.inner();

        if let Some(instrument) = self.instrument_cache.get(&symbol) {
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

        // Track active subscription for periodic refresh
        self.active_orderbook_subs.insert(cmd.instrument_id, ());

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
        // dYdX doesn't have a dedicated quotes channel
        // Quotes are synthesized from order book deltas
        log::debug!(
            "subscribe_quotes for {}: delegating to subscribe_book_deltas (no native quotes channel)",
            cmd.instrument_id
        );

        // Simply delegate to book deltas subscription
        let book_cmd = SubscribeBookDeltas {
            client_id: cmd.client_id,
            venue: cmd.venue,
            instrument_id: cmd.instrument_id,
            book_type: BookType::L2_MBP,
            depth: None,
            managed: false,
            correlation_id: None,
            params: None,
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
        };

        self.subscribe_book_deltas(&book_cmd)
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
        // Remove from active subscription tracking
        self.active_orderbook_subs.remove(&cmd.instrument_id);

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
        // dYdX doesn't have a dedicated quotes channel; quotes are derived from book deltas.
        log::debug!(
            "unsubscribe_quotes for {}: delegating to unsubscribe_book_deltas (no native quotes channel)",
            cmd.instrument_id
        );

        let book_cmd = UnsubscribeBookDeltas {
            instrument_id: cmd.instrument_id,
            client_id: cmd.client_id,
            venue: cmd.venue,
            command_id: cmd.command_id,
            ts_init: cmd.ts_init,
            correlation_id: None,
            params: cmd.params.clone(),
        };

        self.unsubscribe_book_deltas(&book_cmd)
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let spec = cmd.bar_type.spec();

        // Map BarType spec to dYdX candle resolution string
        let resolution = match spec.step.get() {
            1 => match spec.aggregation {
                BarAggregation::Minute => "1MIN",
                BarAggregation::Hour => "1HOUR",
                BarAggregation::Day => "1DAY",
                _ => {
                    anyhow::bail!("Unsupported bar aggregation: {:?}", spec.aggregation);
                }
            },
            5 => {
                if spec.aggregation == BarAggregation::Minute {
                    "5MINS"
                } else {
                    anyhow::bail!("Unsupported 5-step aggregation: {:?}", spec.aggregation);
                }
            }
            15 => {
                if spec.aggregation == BarAggregation::Minute {
                    "15MINS"
                } else {
                    anyhow::bail!("Unsupported 15-step aggregation: {:?}", spec.aggregation);
                }
            }
            30 => {
                if spec.aggregation == BarAggregation::Minute {
                    "30MINS"
                } else {
                    anyhow::bail!("Unsupported 30-step aggregation: {:?}", spec.aggregation);
                }
            }
            4 => {
                if spec.aggregation == BarAggregation::Hour {
                    "4HOURS"
                } else {
                    anyhow::bail!("Unsupported 4-step aggregation: {:?}", spec.aggregation);
                }
            }
            step => {
                anyhow::bail!("Unsupported bar step: {step}");
            }
        };

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
            let symbol = instrument_id.symbol.inner();
            let instrument = if let Some(cached) = instrument_cache.get(&symbol) {
                log::debug!("Found instrument {instrument_id} in cache");
                Some(cached)
            } else {
                // Not in cache, fetch from API
                log::debug!("Instrument {instrument_id} not in cache, fetching from API");
                match http.request_instruments(None, None, None).await {
                    Ok(instruments) => {
                        // Cache all fetched instruments
                        for inst in &instruments {
                            upsert_instrument(&instrument_cache, inst.clone());
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
                        upsert_instrument(&instrument_cache, instrument.clone());
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
        let http = self.http_client.clone();
        let instrument_cache = self.instrument_cache.clone();
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
            // dYdX Indexer trades endpoint supports `limit` but not an explicit
            // date range in this client; we approximate by using the provided
            // limit and instrument metadata for precision.
            let ticker = instrument_id
                .symbol
                .as_str()
                .trim_end_matches("-PERP")
                .to_string();

            // Look up instrument to derive price and size precision.
            let instrument = match instrument_cache.get(&instrument_id.symbol.inner()) {
                Some(inst) => inst.clone(),
                None => {
                    log::error!(
                        "request_trades: instrument {instrument_id} not found in cache; cannot convert trades"
                    );
                    let ts_now = clock.get_time_ns();
                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        Vec::new(),
                        start_nanos,
                        end_nanos,
                        ts_now,
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send empty trades response: {e}");
                    }
                    return;
                }
            };

            let price_precision = instrument.price_precision();
            let size_precision = instrument.size_precision();

            match http
                .inner
                .get_trades(&ticker, limit)
                .await
                .context("failed to request trades from dYdX")
            {
                Ok(trades_response) => {
                    let mut ticks = Vec::new();

                    for trade in trades_response.trades {
                        let aggressor_side = match trade.side {
                            OrderSide::Buy => AggressorSide::Buyer,
                            OrderSide::Sell => AggressorSide::Seller,
                            _ => continue, // Skip unsupported side
                        };

                        let price = match Price::from_decimal_dp(trade.price, price_precision) {
                            Ok(p) => p,
                            Err(e) => {
                                log::warn!(
                                    "request_trades: failed to convert price for trade {}: {e}",
                                    trade.id
                                );
                                continue;
                            }
                        };

                        let size = match Quantity::from_decimal_dp(trade.size, size_precision) {
                            Ok(q) => q,
                            Err(e) => {
                                log::warn!(
                                    "request_trades: failed to convert size for trade {}: {e}",
                                    trade.id
                                );
                                continue;
                            }
                        };

                        let ts_event = match trade.created_at.timestamp_nanos_opt() {
                            Some(ns) if ns >= 0 => UnixNanos::from(ns as u64),
                            _ => {
                                log::warn!(
                                    "request_trades: timestamp out of range for trade {}",
                                    trade.id
                                );
                                continue;
                            }
                        };

                        // Apply optional time-range filter.
                        if let Some(start_ts) = start_nanos
                            && ts_event < start_ts
                        {
                            continue;
                        }
                        if let Some(end_ts) = end_nanos
                            && ts_event > end_ts
                        {
                            continue;
                        }

                        let tick = TradeTick::new(
                            instrument_id,
                            price,
                            size,
                            aggressor_side,
                            TradeId::new(&trade.id),
                            ts_event,
                            clock.get_time_ns(),
                        );
                        ticks.push(tick);
                    }

                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        ticks,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send trades response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Trade request failed for {instrument_id}: {e:?}");

                    let response = DataResponse::Trades(TradesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        Vec::new(),
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send empty trades response: {e}");
                    }
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        const DYDX_MAX_BARS_PER_REQUEST: u32 = 1_000;

        let bar_type = request.bar_type;
        let spec = bar_type.spec();

        // Validate bar type requirements
        if bar_type.aggregation_source() != AggregationSource::External {
            anyhow::bail!(
                "dYdX only supports EXTERNAL aggregation, was {:?}",
                bar_type.aggregation_source()
            );
        }

        if spec.price_type != PriceType::Last {
            anyhow::bail!(
                "dYdX only supports LAST price type, was {:?}",
                spec.price_type
            );
        }

        // Map BarType spec to dYdX resolution
        let resolution = match spec.step.get() {
            1 => match spec.aggregation {
                BarAggregation::Minute => "1MIN",
                BarAggregation::Hour => "1HOUR",
                BarAggregation::Day => "1DAY",
                _ => {
                    anyhow::bail!("Unsupported bar aggregation: {:?}", spec.aggregation);
                }
            },
            5 if spec.aggregation == BarAggregation::Minute => "5MINS",
            15 if spec.aggregation == BarAggregation::Minute => "15MINS",
            30 if spec.aggregation == BarAggregation::Minute => "30MINS",
            4 if spec.aggregation == BarAggregation::Hour => "4HOURS",
            step => {
                anyhow::bail!("Unsupported bar step: {step}");
            }
        };

        let http = self.http_client.clone();
        let instrument_cache = self.instrument_cache.clone();
        let sender = self.data_sender.clone();
        let instrument_id = bar_type.instrument_id();
        // dYdX ticker does not include the "-PERP" suffix.
        let symbol = instrument_id
            .symbol
            .as_str()
            .trim_end_matches("-PERP")
            .to_string();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        let start = request.start;
        let end = request.end;
        let overall_limit = request.limit.map(|n| n.get() as u32);

        // Convert optional datetimes to UnixNanos for response metadata
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);

        // Parse resolution string to DydxCandleResolution enum
        let resolution_enum = match resolution {
            "1MIN" => DydxCandleResolution::OneMinute,
            "5MINS" => DydxCandleResolution::FiveMinutes,
            "15MINS" => DydxCandleResolution::FifteenMinutes,
            "30MINS" => DydxCandleResolution::ThirtyMinutes,
            "1HOUR" => DydxCandleResolution::OneHour,
            "4HOURS" => DydxCandleResolution::FourHours,
            "1DAY" => DydxCandleResolution::OneDay,
            _ => {
                anyhow::bail!("Unsupported resolution: {resolution}");
            }
        };

        get_runtime().spawn(async move {
            // Determine bar duration in seconds.
            let bar_secs: i64 = match spec.aggregation {
                BarAggregation::Minute => spec.step.get() as i64 * 60,
                BarAggregation::Hour => spec.step.get() as i64 * 3_600,
                BarAggregation::Day => spec.step.get() as i64 * 86_400,
                _ => {
                    log::error!(
                        "Unsupported aggregation for request_bars: {:?}",
                        spec.aggregation
                    );
                    return;
                }
            };

            // Look up instrument to derive price and size precision.
            let instrument = match instrument_cache.get(&instrument_id.symbol.inner()) {
                Some(inst) => inst.clone(),
                None => {
                    log::error!(
                        "request_bars: instrument {instrument_id} not found in cache; cannot convert candles"
                    );
                    let ts_now = clock.get_time_ns();
                    let response = DataResponse::Bars(BarsResponse::new(
                        request_id,
                        client_id,
                        bar_type,
                        Vec::new(),
                        start_nanos,
                        end_nanos,
                        ts_now,
                        params,
                    ));
                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send empty bars response: {e}");
                    }
                    return;
                }
            };

            let price_precision = instrument.price_precision();
            let size_precision = instrument.size_precision();

            let mut all_bars: Vec<Bar> = Vec::new();

            // If no explicit date range, fall back to a single request using only `limit`.
            let (range_start, range_end) = match (start, end) {
                (Some(s), Some(e)) if e > s => (s, e),
                _ => {
                    let limit = overall_limit.unwrap_or(DYDX_MAX_BARS_PER_REQUEST);
                    match http
                        .inner
                        .get_candles(&symbol, resolution_enum, Some(limit), None, None)
                        .await
                    {
                        Ok(candles_response) => {
                            log::debug!(
                                "request_bars fetched {} candles without explicit date range",
                                candles_response.candles.len()
                            );

                            for candle in &candles_response.candles {
                                match Self::candle_to_bar(
                                    candle,
                                    bar_type,
                                    price_precision,
                                    size_precision,
                                    bar_secs,
                                    clock,
                                ) {
                                    Ok(bar) => all_bars.push(bar),
                                    Err(e) => {
                                        log::warn!(
                                            "Failed to convert dYdX candle to bar for {instrument_id}: {e}"
                                        );
                                    }
                                }
                            }

                            let current_time_ns = clock.get_time_ns();
                            all_bars.retain(|bar| bar.ts_event < current_time_ns);

                            let response = DataResponse::Bars(BarsResponse::new(
                                request_id,
                                client_id,
                                bar_type,
                                all_bars,
                                start_nanos,
                                end_nanos,
                                current_time_ns,
                                params,
                            ));

                            if let Err(e) = sender.send(DataEvent::Response(response)) {
                                log::error!("Failed to send bars response: {e}");
                            }
                        }
                        Err(e) => {
                            log::error!(
                                "Failed to request candles for {symbol} without date range: {e:?}"
                            );
                        }
                    }
                    return;
                }
            };

            // Calculate expected bars for the range.
            let total_secs = (range_end - range_start).num_seconds().max(0);
            let expected_bars = (total_secs / bar_secs).max(1) as u64;

            log::debug!(
                "request_bars range {range_start:?} -> {range_end:?}, expected_bars ~= {expected_bars}"
            );

            let mut remaining = overall_limit.unwrap_or(u32::MAX);

            // Determine chunk duration using max bars per request.
            let bars_per_call = DYDX_MAX_BARS_PER_REQUEST.min(remaining);
            let chunk_duration = chrono::Duration::seconds(bar_secs * bars_per_call as i64);

            let mut chunk_start = range_start;

            while chunk_start < range_end && remaining > 0 {
                let mut chunk_end = chunk_start + chunk_duration;
                if chunk_end > range_end {
                    chunk_end = range_end;
                }

                let per_call_limit = remaining.min(DYDX_MAX_BARS_PER_REQUEST);

                log::debug!(
                    "request_bars chunk: {chunk_start} -> {chunk_end}, limit={per_call_limit}"
                );

                match http
                    .inner
                    .get_candles(
                        &symbol,
                        resolution_enum,
                        Some(per_call_limit),
                        Some(chunk_start),
                        Some(chunk_end),
                    )
                    .await
                {
                    Ok(candles_response) => {
                        let count = candles_response.candles.len() as u32;

                        if count == 0 {
                            // No more data available; stop early.
                            break;
                        }

                        // Convert candles to bars and accumulate.
                        for candle in &candles_response.candles {
                            match Self::candle_to_bar(
                                candle,
                                bar_type,
                                price_precision,
                                size_precision,
                                bar_secs,
                                clock,
                            ) {
                                Ok(bar) => all_bars.push(bar),
                                Err(e) => {
                                    log::warn!(
                                        "Failed to convert dYdX candle to bar for {instrument_id}: {e}"
                                    );
                                }
                            }
                        }

                        if remaining <= count {
                            break;
                        } else {
                            remaining -= count;
                        }
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to request candles for {symbol} in chunk {chunk_start:?} -> {chunk_end:?}: {e:?}"
                        );
                        break;
                    }
                }

                chunk_start += chunk_duration;
            }

            log::debug!("request_bars completed partitioned fetch for {bar_type}");

            // Filter incomplete bars: only return bars where ts_event < current_time_ns
            let current_time_ns = clock.get_time_ns();
            all_bars.retain(|bar| bar.ts_event < current_time_ns);

            log::debug!(
                "request_bars filtered to {} completed bars (current_time_ns={})",
                all_bars.len(),
                current_time_ns
            );

            let response = DataResponse::Bars(BarsResponse::new(
                request_id,
                client_id,
                bar_type,
                all_bars,
                start_nanos,
                end_nanos,
                current_time_ns,
                params,
            ));

            if let Err(e) = sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send bars response: {e}");
            }
        });

        Ok(())
    }
}

/// Upserts an instrument into the shared cache.
fn upsert_instrument(cache: &Arc<InstrumentCache>, instrument: InstrumentAny) {
    cache.insert_instrument_only(instrument);
}

impl DydxDataClient {
    /// Start a task to periodically refresh instruments.
    ///
    /// This task runs in the background and updates the instrument cache
    /// at the configured interval.
    ///
    /// # Errors
    ///
    /// Returns an error if a refresh task is already running.
    pub fn start_instrument_refresh_task(&mut self) -> anyhow::Result<()> {
        let interval_secs = match self.config.instrument_refresh_interval_secs {
            Some(secs) if secs > 0 => secs,
            _ => {
                log::info!("Instrument refresh disabled (interval not configured)");
                return Ok(());
            }
        };

        let interval = Duration::from_secs(interval_secs);
        let http_client = self.http_client.clone();
        let ws_client = self.ws_client.clone();
        let cancellation_token = self.cancellation_token.clone();

        log::info!("Starting instrument refresh task (interval: {interval_secs}s)");

        let task = get_runtime().spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.tick().await; // Skip first immediate tick

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::info!("Instrument refresh task cancelled");
                        break;
                    }
                    _ = interval_timer.tick() => {
                        log::debug!("Refreshing instruments");

                        // Populates shared InstrumentCache via HTTP client
                        match http_client.fetch_and_cache_instruments().await {
                            Ok(()) => {
                                let instruments = http_client.all_instruments();
                                log::debug!("Refreshed {} instruments in shared cache", instruments.len());

                                // Propagate to WS handler for message parsing
                                ws_client.cache_instruments(instruments);
                            }
                            Err(e) => {
                                log::error!("Failed to refresh instruments: {e}");
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(task);
        Ok(())
    }

    /// Start a background task to periodically refresh orderbook snapshots.
    ///
    /// This prevents stale orderbooks from missed WebSocket messages due to:
    /// - Network issues or message drops
    /// - dYdX validator delays
    /// - WebSocket reconnection gaps
    ///
    /// The task fetches fresh snapshots via HTTP at the configured interval
    /// and applies them to the local orderbooks.
    fn start_orderbook_refresh_task(&mut self) -> anyhow::Result<()> {
        let interval_secs = match self.config.orderbook_refresh_interval_secs {
            Some(secs) if secs > 0 => secs,
            _ => {
                log::info!("Orderbook snapshot refresh disabled (interval not configured)");
                return Ok(());
            }
        };

        let interval = Duration::from_secs(interval_secs);
        let http_client = self.http_client.clone();
        let instrument_cache = self.instrument_cache.clone();
        let order_books = self.order_books.clone();
        let active_subs = self.active_orderbook_subs.clone();
        let cancellation_token = self.cancellation_token.clone();
        let data_sender = self.data_sender.clone();

        log::info!("Starting orderbook snapshot refresh task (interval: {interval_secs}s)");

        let task = get_runtime().spawn(async move {
            let mut interval_timer = tokio::time::interval(interval);
            interval_timer.tick().await; // Skip first immediate tick

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::info!("Orderbook refresh task cancelled");
                        break;
                    }
                    _ = interval_timer.tick() => {
                        let active_instruments: Vec<InstrumentId> = active_subs
                            .iter()
                            .map(|entry| *entry.key())
                            .collect();

                        if active_instruments.is_empty() {
                            log::debug!("No active orderbook subscriptions to refresh");
                            continue;
                        }

                        log::debug!(
                            "Refreshing {} orderbook snapshots",
                            active_instruments.len()
                        );

                        for instrument_id in active_instruments {
                            // Get instrument for parsing
                            let instrument = match instrument_cache.get(&instrument_id.symbol.inner()) {
                                Some(inst) => inst.clone(),
                                None => {
                                    log::warn!(
                                        "Cannot refresh orderbook: no instrument for {instrument_id}"
                                    );
                                    continue;
                                }
                            };

                            // Fetch snapshot via HTTP (strip -PERP suffix for dYdX API)
                            let symbol = instrument_id.symbol.as_str().trim_end_matches("-PERP");
                            let snapshot_result = http_client.inner.get_orderbook(symbol).await;

                            let snapshot = match snapshot_result {
                                Ok(s) => s,
                                Err(e) => {
                                    log::error!(
                                        "Failed to fetch orderbook snapshot for {instrument_id}: {e}"
                                    );
                                    continue;
                                }
                            };

                            // Convert HTTP snapshot to OrderBookDeltas
                            let deltas_result = Self::parse_orderbook_snapshot(
                                instrument_id,
                                &snapshot,
                                &instrument,
                            );

                            let deltas = match deltas_result {
                                Ok(d) => d,
                                Err(e) => {
                                    log::error!(
                                        "Failed to parse orderbook snapshot for {instrument_id}: {e}"
                                    );
                                    continue;
                                }
                            };

                            // Apply snapshot to local orderbook
                            if let Some(mut book) = order_books.get_mut(&instrument_id) {
                                if let Err(e) = book.apply_deltas(&deltas) {
                                    log::error!(
                                        "Failed to apply orderbook snapshot for {instrument_id}: {e}"
                                    );
                                    continue;
                                }

                                log::debug!(
                                    "Refreshed orderbook snapshot for {} (bid={:?}, ask={:?})",
                                    instrument_id,
                                    book.best_bid_price(),
                                    book.best_ask_price()
                                );
                            }

                            // Emit the snapshot deltas
                            let data = NautilusData::from(OrderBookDeltas_API::new(deltas));
                            if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                                log::error!("Failed to emit orderbook snapshot: {e}");
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(task);
        Ok(())
    }

    /// Parse HTTP orderbook snapshot into OrderBookDeltas.
    ///
    /// Converts the REST API orderbook format into Nautilus deltas with CLEAR + ADD actions.
    fn parse_orderbook_snapshot(
        instrument_id: InstrumentId,
        snapshot: &OrderbookResponse,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<OrderBookDeltas> {
        let ts_init = get_atomic_clock_realtime().get_time_ns();
        let mut deltas = Vec::new();

        // Add clear delta first
        deltas.push(OrderBookDelta::clear(instrument_id, 0, ts_init, ts_init));

        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        let bids_len = snapshot.bids.len();
        let asks_len = snapshot.asks.len();

        // Add bid levels
        for (idx, bid) in snapshot.bids.iter().enumerate() {
            let is_last = idx == bids_len - 1 && asks_len == 0;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Price::from_decimal_dp(bid.price, price_precision)
                .context("failed to parse bid price")?;
            let size = Quantity::from_decimal_dp(bid.size, size_precision)
                .context("failed to parse bid size")?;

            let order = BookOrder::new(OrderSide::Buy, price, size, 0);
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        // Add ask levels
        for (idx, ask) in snapshot.asks.iter().enumerate() {
            let is_last = idx == asks_len - 1;
            let flags = if is_last { RecordFlag::F_LAST as u8 } else { 0 };

            let price = Price::from_decimal_dp(ask.price, price_precision)
                .context("failed to parse ask price")?;
            let size = Quantity::from_decimal_dp(ask.size, size_precision)
                .context("failed to parse ask size")?;

            let order = BookOrder::new(OrderSide::Sell, price, size, 0);
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                order,
                flags,
                0,
                ts_init,
                ts_init,
            ));
        }

        Ok(OrderBookDeltas::new(instrument_id, deltas))
    }

    /// Get a cached instrument by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instrument_cache.get(&Ustr::from(symbol))
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

    /// Convert a dYdX HTTP candle into a Nautilus [`Bar`].
    ///
    /// This mirrors the conversion logic used in other adapters (for example
    /// Hyperliquid), using the instrument price/size precision and mapping the
    /// candle start time to `ts_init` with `ts_event` at the end of the bar
    /// interval.
    fn candle_to_bar(
        candle: &Candle,
        bar_type: BarType,
        price_precision: u8,
        size_precision: u8,
        bar_secs: i64,
        clock: &AtomicTime,
    ) -> anyhow::Result<Bar> {
        // Convert candle start time to UnixNanos (ts_init).
        let ts_init =
            datetime_to_unix_nanos(Some(candle.started_at)).unwrap_or_else(|| clock.get_time_ns());

        // Treat ts_event as the end of the bar interval.
        let ts_event_ns = ts_init
            .as_u64()
            .saturating_add((bar_secs as u64).saturating_mul(1_000_000_000));
        let ts_event = UnixNanos::from(ts_event_ns);

        let open = Price::from_decimal_dp(candle.open, price_precision)
            .context("failed to parse candle open price")?;
        let high = Price::from_decimal_dp(candle.high, price_precision)
            .context("failed to parse candle high price")?;
        let low = Price::from_decimal_dp(candle.low, price_precision)
            .context("failed to parse candle low price")?;
        let close = Price::from_decimal_dp(candle.close, price_precision)
            .context("failed to parse candle close price")?;

        // Use base token volume as bar volume.
        let volume = Quantity::from_decimal_dp(candle.base_token_volume, size_precision)
            .context("failed to parse candle base_token_volume")?;

        Ok(Bar::new(
            bar_type, open, high, low, close, volume, ts_event, ts_init,
        ))
    }

    fn handle_ws_message(message: NautilusWsMessage, ctx: &WsMessageContext) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                Self::handle_data_message(payloads, &ctx.data_sender, &ctx.incomplete_bars);
            }
            NautilusWsMessage::Deltas(deltas) => {
                Self::handle_deltas_message(
                    *deltas,
                    &ctx.data_sender,
                    &ctx.order_books,
                    &ctx.last_quotes,
                    &ctx.instrument_cache,
                );
            }
            NautilusWsMessage::OraclePrices(oracle_prices) => {
                Self::handle_oracle_prices(oracle_prices, &ctx.instrument_cache, &ctx.data_sender);
            }
            NautilusWsMessage::Error(err) => {
                log::error!("dYdX WS error: {err}");
            }
            NautilusWsMessage::Reconnected => {
                log::info!("dYdX WS reconnected - re-subscribing to active subscriptions");

                let total_subs = ctx.active_orderbook_subs.len()
                    + ctx.active_trade_subs.len()
                    + ctx.active_bar_subs.len();

                if total_subs == 0 {
                    log::debug!("No active subscriptions to restore");
                    return;
                }

                log::info!(
                    "Restoring {} subscriptions (orderbook={}, trades={}, bars={})",
                    total_subs,
                    ctx.active_orderbook_subs.len(),
                    ctx.active_trade_subs.len(),
                    ctx.active_bar_subs.len()
                );

                // Re-subscribe to orderbook channels
                for entry in ctx.active_orderbook_subs.iter() {
                    let instrument_id = *entry.key();
                    let ws_clone = ctx.ws_client.clone();
                    get_runtime().spawn(async move {
                        if let Err(e) = ws_clone.subscribe_orderbook(instrument_id).await {
                            log::error!(
                                "Failed to re-subscribe to orderbook for {instrument_id}: {e:?}"
                            );
                        } else {
                            log::debug!("Re-subscribed to orderbook for {instrument_id}");
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
        }
    }

    fn handle_data_message(
        payloads: Vec<NautilusData>,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        incomplete_bars: &Arc<DashMap<BarType, Bar>>,
    ) {
        for data in payloads {
            // Filter bars through incomplete bars cache
            if let NautilusData::Bar(bar) = data {
                Self::handle_bar_message(bar, data_sender, incomplete_bars);
            } else if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit data event: {e}");
            }
        }
    }

    /// Handles bar messages by tracking incomplete bars and only emitting completed ones.
    ///
    /// WebSocket candle updates arrive continuously. This method:
    /// - Caches bars where ts_event > current_time (incomplete)
    /// - Emits bars where ts_event <= current_time (complete)
    /// - Updates cached incomplete bars with latest data
    fn handle_bar_message(
        bar: Bar,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        incomplete_bars: &Arc<DashMap<BarType, Bar>>,
    ) {
        let current_time_ns = get_atomic_clock_realtime().get_time_ns();
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

    /// Resolves a crossed order book by generating synthetic deltas to uncross it.
    ///
    /// dYdX order books can become crossed due to:
    /// - Validator delays in order acknowledgment across the network
    /// - Missed or delayed WebSocket messages from the venue
    ///
    /// This function detects when bid_price >= ask_price and iteratively removes
    /// the smaller side while adjusting the larger side until the book is uncrossed.
    ///
    /// # Algorithm
    ///
    /// For each crossed level:
    /// - If bid_size > ask_size: DELETE ask, UPDATE bid (reduce by ask_size)
    /// - If bid_size < ask_size: DELETE bid, UPDATE ask (reduce by bid_size)
    /// - If bid_size == ask_size: DELETE both bid and ask
    ///
    /// The algorithm continues until no more crosses exist or the book is empty.
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
    ) {
        let instrument_id = deltas.instrument_id;

        // Get instrument for crossed orderbook resolution
        let instrument = match instrument_cache.get(&instrument_id.symbol.inner()) {
            Some(inst) => inst,
            None => {
                log::error!("Cannot resolve crossed order book: no instrument for {instrument_id}");
                // Still emit the raw deltas even without instrument
                if let Err(e) = data_sender.send(DataEvent::Data(NautilusData::from(
                    OrderBookDeltas_API::new(deltas),
                ))) {
                    log::error!("Failed to emit order book deltas: {e}");
                }
                return;
            }
        };

        // Get or create order book
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

        // Generate QuoteTick from updated top-of-book
        // Edge case: If orderbook is empty after deltas, fall back to last quote
        let quote_opt = if let (Some(bid_price), Some(ask_price)) =
            (book.best_bid_price(), book.best_ask_price())
            && let (Some(bid_size), Some(ask_size)) = (book.best_bid_size(), book.best_ask_size())
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
            let emit_quote =
                !matches!(last_quotes.get(&instrument_id), Some(existing) if *existing == quote);

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

        // Emit the resolved order book deltas
        let data: NautilusData = OrderBookDeltas_API::new(resolved_deltas).into();
        if let Err(e) = data_sender.send(DataEvent::Data(data)) {
            log::error!("Failed to emit order book deltas event: {e}");
        }
    }

    fn handle_oracle_prices(
        oracle_prices: std::collections::HashMap<String, DydxOraclePriceMarket>,
        instrument_cache: &Arc<InstrumentCache>,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) {
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        for (symbol_str, oracle_market) in oracle_prices {
            // Oracle prices use market format (e.g., "BTC-USD"), but instruments are keyed
            // by perpetual symbol (e.g., "BTC-USD-PERP")
            let perp_symbol = format!("{symbol_str}-PERP");
            let symbol = Ustr::from(&perp_symbol);

            // Get instrument to access instrument_id
            let Some(instrument) = instrument_cache.get(&symbol) else {
                log::debug!(
                    "Received oracle price for unknown instrument (not cached yet): symbol={symbol}"
                );
                continue;
            };

            let instrument_id = instrument.id();

            // Parse oracle price string to Price
            let oracle_price_str = &oracle_market.oracle_price;
            let Ok(oracle_price_dec) = oracle_price_str.parse::<Decimal>() else {
                log::error!(
                    "Failed to parse oracle price: symbol={symbol}, price_str={oracle_price_str}"
                );
                continue;
            };

            let price_precision = instrument.price_precision();
            let Ok(oracle_price) = Price::from_decimal_dp(oracle_price_dec, price_precision) else {
                log::error!(
                    "Failed to create oracle Price: symbol={symbol}, price={oracle_price_dec}"
                );
                continue;
            };

            let oracle_price_event = DydxOraclePrice::new(
                instrument_id,
                oracle_price,
                ts_init, // Use ts_init as ts_event since dYdX doesn't provide event timestamp
                ts_init,
            );

            log::debug!(
                "Received dYdX oracle price: instrument_id={instrument_id}, oracle_price={oracle_price}, {oracle_price_event:?}"
            );

            let data = NautilusData::IndexPriceUpdate(IndexPriceUpdate::new(
                instrument_id,
                oracle_price,
                ts_init, // Use ts_init as ts_event since dYdX doesn't provide event timestamp
                ts_init,
            ));

            if let Err(e) = data_sender.send(DataEvent::Data(data)) {
                log::error!("Failed to emit oracle price: {e}");
            }
        }
    }
}
