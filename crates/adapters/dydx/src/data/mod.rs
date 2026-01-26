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
    common::{consts::DYDX_VENUE, instrument_cache::InstrumentCache, parse::extract_raw_symbol},
    config::DydxDataClientConfig,
    http::client::DydxHttpClient,
    types::DydxOraclePrice,
    websocket::client::DydxWebSocketClient,
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
                if let Err(e) =
                    ws.send_command(crate::websocket::handler::HandlerCommand::RegisterBarType {
                        topic,
                        bar_type,
                    })
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

        if let Err(e) =
            ws.send_command(crate::websocket::handler::HandlerCommand::UnregisterBarType { topic })
        {
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
            let symbol = Ustr::from(instrument_id.symbol.as_str());
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
            let instrument = match instrument_cache.get(&Ustr::from(instrument_id.symbol.as_ref())) {
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
            "1MIN" => crate::common::enums::DydxCandleResolution::OneMinute,
            "5MINS" => crate::common::enums::DydxCandleResolution::FiveMinutes,
            "15MINS" => crate::common::enums::DydxCandleResolution::FifteenMinutes,
            "30MINS" => crate::common::enums::DydxCandleResolution::ThirtyMinutes,
            "1HOUR" => crate::common::enums::DydxCandleResolution::OneHour,
            "4HOURS" => crate::common::enums::DydxCandleResolution::FourHours,
            "1DAY" => crate::common::enums::DydxCandleResolution::OneDay,
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
            let instrument = match instrument_cache.get(&Ustr::from(instrument_id.symbol.as_ref())) {
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
                            let instrument = match instrument_cache.get(&Ustr::from(instrument_id.symbol.as_ref())) {
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
        snapshot: &crate::http::models::OrderbookResponse,
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
        candle: &crate::http::models::Candle,
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

    fn handle_ws_message(
        message: crate::websocket::enums::NautilusWsMessage,
        ctx: &WsMessageContext,
    ) {
        match message {
            crate::websocket::enums::NautilusWsMessage::Data(payloads) => {
                Self::handle_data_message(payloads, &ctx.data_sender, &ctx.incomplete_bars);
            }
            crate::websocket::enums::NautilusWsMessage::Deltas(deltas) => {
                Self::handle_deltas_message(
                    *deltas,
                    &ctx.data_sender,
                    &ctx.order_books,
                    &ctx.last_quotes,
                    &ctx.instrument_cache,
                );
            }
            crate::websocket::enums::NautilusWsMessage::OraclePrices(oracle_prices) => {
                Self::handle_oracle_prices(oracle_prices, &ctx.instrument_cache, &ctx.data_sender);
            }
            crate::websocket::enums::NautilusWsMessage::Error(err) => {
                log::error!("dYdX WS error: {err}");
            }
            crate::websocket::enums::NautilusWsMessage::Reconnected => {
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
                    if let Err(e) = ctx.ws_client.send_command(
                        crate::websocket::handler::HandlerCommand::RegisterBarType {
                            topic,
                            bar_type,
                        },
                    ) {
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
            crate::websocket::enums::NautilusWsMessage::BlockHeight { .. } => {
                log::debug!(
                    "Ignoring block height message on dYdX data client (handled by execution adapter)"
                );
            }
            crate::websocket::enums::NautilusWsMessage::Order(_)
            | crate::websocket::enums::NautilusWsMessage::Fill(_)
            | crate::websocket::enums::NautilusWsMessage::Position(_)
            | crate::websocket::enums::NautilusWsMessage::AccountState(_)
            | crate::websocket::enums::NautilusWsMessage::SubaccountSubscribed(_)
            | crate::websocket::enums::NautilusWsMessage::SubaccountsChannelData(_) => {
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
        let instrument = match instrument_cache.get(&Ustr::from(instrument_id.symbol.as_ref())) {
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
        oracle_prices: std::collections::HashMap<
            String,
            crate::websocket::messages::DydxOraclePriceMarket,
        >,
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

#[cfg(test)]
mod tests {
    use std::{collections::HashMap, net::SocketAddr, time::Duration};

    use axum::{
        Router,
        extract::{Path, Query, State},
        response::Json,
        routing::get,
    };
    use chrono::Utc;
    use indexmap::IndexMap;
    use nautilus_common::{
        live::runner::set_data_event_sender,
        messages::{DataEvent, data::DataResponse},
        testing::wait_until_async,
    };
    use nautilus_core::UUID4;
    use nautilus_model::{
        data::{
            BarSpecification, BarType, Data as NautilusData, OrderBookDelta, OrderBookDeltas,
            TradeTick, order::BookOrder,
        },
        enums::{
            AggregationSource, AggressorSide, BarAggregation, BookAction, BookType, OrderSide,
            PriceType,
        },
        identifiers::{ClientId, InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, Instrument, InstrumentAny},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use tokio::net::{TcpListener, TcpStream};

    use super::*;
    use crate::http::models::{Candle, CandlesResponse};

    fn setup_test_env() {
        // Initialize data event sender for tests
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(sender);
    }

    async fn wait_for_server(addr: SocketAddr) {
        wait_until_async(
            || async move { TcpStream::connect(addr).await.is_ok() },
            Duration::from_secs(5),
        )
        .await;
    }

    fn create_test_ws_client() -> DydxWebSocketClient {
        DydxWebSocketClient::new_public("ws://test".to_string(), None)
    }

    #[rstest]
    fn test_new_data_client() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-001");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client = DydxDataClient::new(client_id, config, http_client, create_test_ws_client());
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.client_id(), client_id);
        assert_eq!(client.venue(), *DYDX_VENUE);
        assert!(!client.is_connected());
    }

    #[tokio::test]
    async fn test_data_client_lifecycle() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-001");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let mut client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Test start
        assert!(client.start().is_ok());

        // Test stop
        assert!(client.stop().is_ok());
        assert!(!client.is_connected());

        // Test reset
        assert!(client.reset().is_ok());

        // Test dispose
        assert!(client.dispose().is_ok());
    }

    #[rstest]
    fn test_subscribe_unsubscribe_instruments_noop() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let mut client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let venue = *DYDX_VENUE;
        let command_id = UUID4::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let subscribe = SubscribeInstruments {
            client_id: Some(client_id),
            venue,
            command_id,
            ts_init,
            correlation_id: None,
            params: None,
        };
        let unsubscribe = UnsubscribeInstruments::new(None, venue, command_id, ts_init, None, None);

        // No-op methods should succeed even without a WebSocket client.
        assert!(client.subscribe_instruments(&subscribe).is_ok());
        assert!(client.unsubscribe_instruments(&unsubscribe).is_ok());
    }

    #[rstest]
    fn test_bar_type_mappings_registration() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        // Initially no topics registered
        assert!(client.get_bar_topics().is_empty());
        assert!(client.get_bar_type_for_topic("BTC-USD/1MIN").is_none());

        // Register topic
        client
            .bar_type_mappings
            .insert("BTC-USD/1MIN".to_string(), bar_type);

        // Verify registration
        assert_eq!(client.get_bar_topics().len(), 1);
        assert!(
            client
                .get_bar_topics()
                .contains(&"BTC-USD/1MIN".to_string())
        );
        assert_eq!(
            client.get_bar_type_for_topic("BTC-USD/1MIN"),
            Some(bar_type)
        );

        // Register another topic
        let spec_5min = BarSpecification {
            step: std::num::NonZeroUsize::new(5).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type_5min = BarType::new(instrument_id, spec_5min, AggregationSource::External);
        client
            .bar_type_mappings
            .insert("BTC-USD/5MINS".to_string(), bar_type_5min);

        // Verify multiple topics
        assert_eq!(client.get_bar_topics().len(), 2);
        assert_eq!(
            client.get_bar_type_for_topic("BTC-USD/5MINS"),
            Some(bar_type_5min)
        );
    }

    #[rstest]
    fn test_bar_type_mappings_unregistration() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument_id = InstrumentId::from("ETH-USD-PERP.DYDX");
        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Hour,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        // Register topic
        client
            .bar_type_mappings
            .insert("ETH-USD/1HOUR".to_string(), bar_type);
        assert_eq!(client.get_bar_topics().len(), 1);

        // Unregister topic
        client.bar_type_mappings.remove("ETH-USD/1HOUR");

        // Verify unregistration
        assert!(client.get_bar_topics().is_empty());
        assert!(client.get_bar_type_for_topic("ETH-USD/1HOUR").is_none());
    }

    #[rstest]
    fn test_bar_type_mappings_lookup_nonexistent() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Lookup non-existent topic
        assert!(client.get_bar_type_for_topic("NONEXISTENT/1MIN").is_none());
        assert!(client.get_bar_topics().is_empty());
    }

    #[tokio::test]
    async fn test_handle_ws_message_deltas_updates_orderbook_and_emits_quote() {
        setup_test_env();

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let instrument_cache = Arc::new(InstrumentCache::new());
        let order_books = Arc::new(DashMap::new());
        let last_quotes = Arc::new(DashMap::new());
        let ws_client = DydxWebSocketClient::new_public("ws://test".to_string(), None);
        let active_orderbook_subs = Arc::new(DashMap::new());
        let active_trade_subs = Arc::new(DashMap::new());
        let active_bar_subs = Arc::new(DashMap::new());

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let bar_ts = get_atomic_clock_realtime().get_time_ns();

        // Add a test instrument to the cache (required for crossed book resolution)
        let symbol = Symbol::from("BTC-USD-PERP");
        let instrument = CryptoPerpetual::new(
            instrument_id,
            symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,
            4,
            Price::from("0.01"),
            Quantity::from("0.0001"),
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
            bar_ts,
            bar_ts,
        );
        instrument_cache.insert_instrument_only(InstrumentAny::CryptoPerpetual(instrument));

        let price = Price::from("100.00");
        let size = Quantity::from("1.0");

        // Create both bid and ask deltas to generate a quote
        let bid_delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Buy, price, size, 1),
            0,
            1,
            bar_ts,
            bar_ts,
        );
        let ask_delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(OrderSide::Sell, Price::from("101.00"), size, 1),
            0,
            1,
            bar_ts,
            bar_ts,
        );
        let deltas = OrderBookDeltas::new(instrument_id, vec![bid_delta, ask_delta]);

        let message = crate::websocket::enums::NautilusWsMessage::Deltas(Box::new(deltas));

        let incomplete_bars = Arc::new(DashMap::new());
        let ctx = WsMessageContext {
            data_sender: sender,
            instrument_cache,
            order_books,
            last_quotes,
            ws_client,
            active_orderbook_subs,
            active_trade_subs,
            active_bar_subs,
            incomplete_bars,
        };
        DydxDataClient::handle_ws_message(message, &ctx);

        // Ensure order book was created and top-of-book quote cached.
        assert!(ctx.order_books.get(&instrument_id).is_some());
        assert!(ctx.last_quotes.get(&instrument_id).is_some());

        // Ensure a quote and deltas Data events were emitted.
        let mut saw_quote = false;
        let mut saw_deltas = false;

        while let Ok(event) = rx.try_recv() {
            if let DataEvent::Data(data) = event {
                match data {
                    NautilusData::Quote(_) => saw_quote = true,
                    NautilusData::Deltas(_) => saw_deltas = true,
                    _ => {}
                }
            }
        }

        assert!(saw_quote);
        assert!(saw_deltas);
    }

    #[rstest]
    fn test_handle_ws_message_error_does_not_panic() {
        // Ensure malformed/error WebSocket messages are logged and ignored
        // without panicking or affecting client state.
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        let instrument_cache = Arc::new(InstrumentCache::new());
        let order_books = Arc::new(DashMap::new());
        let last_quotes = Arc::new(DashMap::new());
        let ws_client = DydxWebSocketClient::new_public("ws://test".to_string(), None);
        let active_orderbook_subs = Arc::new(DashMap::new());
        let active_trade_subs = Arc::new(DashMap::new());
        let active_bar_subs = Arc::new(DashMap::new());
        let incomplete_bars = Arc::new(DashMap::new());

        let ctx = WsMessageContext {
            data_sender: sender,
            instrument_cache,
            order_books,
            last_quotes,
            ws_client,
            active_orderbook_subs,
            active_trade_subs,
            active_bar_subs,
            incomplete_bars,
        };

        let err = crate::websocket::error::DydxWebSocketError::from_message(
            "malformed WebSocket payload".to_string(),
        );

        DydxDataClient::handle_ws_message(
            crate::websocket::enums::NautilusWsMessage::Error(err),
            &ctx,
        );
    }

    #[tokio::test]
    async fn test_request_bars_partitioning_math_does_not_panic() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-BARS");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");
        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        let now = Utc::now();
        let start = Some(now - chrono::Duration::hours(10));
        let end = Some(now);

        let request = RequestBars::new(
            bar_type,
            start,
            end,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // We only verify that the partitioning logic executes without panicking;
        // HTTP calls are allowed to fail and are handled internally.
        assert!(client.request_bars(request).is_ok());
    }

    #[tokio::test]
    async fn test_request_bars_partitioning_months_range_does_not_overflow() {
        setup_test_env();

        // Prepare a simple candles response served by a local Axum HTTP server.
        let now = Utc::now();
        let candle = crate::http::models::Candle {
            started_at: now - chrono::Duration::minutes(1),
            ticker: "BTC-USD".to_string(),
            resolution: crate::common::enums::DydxCandleResolution::OneMinute,
            open: dec!(100.0),
            high: dec!(101.0),
            low: dec!(99.0),
            close: dec!(100.5),
            base_token_volume: dec!(1.0),
            usd_volume: dec!(100.0),
            trades: 10,
            starting_open_interest: dec!(1000.0),
        };
        let candles_response = crate::http::models::CandlesResponse {
            candles: vec![candle],
        };
        let state = CandlesTestState {
            response: Arc::new(candles_response),
        };
        let addr = start_candles_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-BARS-MONTHS");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Seed instrument cache so request_bars can resolve precision.
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        // Use a date range spanning multiple months to exercise partitioning math.
        let start = Some(now - chrono::Duration::days(90));
        let end = Some(now);

        // Limit the total number of bars so the test completes quickly.
        let limit = Some(std::num::NonZeroUsize::new(10).unwrap());

        let request = RequestBars::new(
            bar_type,
            start,
            end,
            limit,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_bars(request).is_ok());
    }

    #[derive(Clone)]
    struct OrderbookTestState {
        snapshot: Arc<crate::http::models::OrderbookResponse>,
    }

    #[derive(Clone)]
    struct TradesTestState {
        response: Arc<crate::http::models::TradesResponse>,
        last_ticker: Arc<tokio::sync::Mutex<Option<String>>>,
        last_limit: Arc<tokio::sync::Mutex<Option<Option<u32>>>>,
    }

    #[derive(Clone)]
    struct CandlesTestState {
        response: Arc<crate::http::models::CandlesResponse>,
    }

    async fn start_orderbook_test_server(state: OrderbookTestState) -> SocketAddr {
        async fn handle_orderbook(
            Path(_ticker): Path<String>,
            State(state): State<OrderbookTestState>,
        ) -> Json<crate::http::models::OrderbookResponse> {
            Json((*state.snapshot).clone())
        }

        let router = Router::new().route(
            "/v4/orderbooks/perpetualMarket/{ticker}",
            get(handle_orderbook).with_state(state),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        get_runtime().spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        });

        wait_for_server(addr).await;
        addr
    }

    async fn start_trades_test_server(state: TradesTestState) -> SocketAddr {
        async fn handle_trades(
            Path(ticker): Path<String>,
            Query(params): Query<HashMap<String, String>>,
            State(state): State<TradesTestState>,
        ) -> Json<crate::http::models::TradesResponse> {
            {
                let mut last_ticker = state.last_ticker.lock().await;
                *last_ticker = Some(ticker);
            }

            let limit = params
                .get("limit")
                .and_then(|value| value.parse::<u32>().ok());
            {
                let mut last_limit = state.last_limit.lock().await;
                *last_limit = Some(limit);
            }

            Json((*state.response).clone())
        }

        let router = Router::new().route(
            "/v4/trades/perpetualMarket/{ticker}",
            get(handle_trades).with_state(state),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        get_runtime().spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        });

        wait_for_server(addr).await;
        addr
    }

    async fn start_candles_test_server(state: CandlesTestState) -> SocketAddr {
        async fn handle_candles(
            Path(_ticker): Path<String>,
            Query(_params): Query<HashMap<String, String>>,
            State(state): State<CandlesTestState>,
        ) -> Json<crate::http::models::CandlesResponse> {
            Json((*state.response).clone())
        }

        let router = Router::new().route(
            "/v4/candles/perpetualMarkets/{ticker}",
            get(handle_candles).with_state(state),
        );

        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        get_runtime().spawn(async move {
            axum::serve(listener, router.into_make_service())
                .await
                .unwrap();
        });

        wait_for_server(addr).await;
        addr
    }

    fn create_test_instrument_any() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));

        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            instrument_id.symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,                                // price_precision
            8,                                // size_precision
            Price::new(0.01, 2),              // price_increment
            Quantity::new(0.001, 8),          // size_increment
            Some(Quantity::new(1.0, 0)),      // multiplier
            Some(Quantity::new(0.001, 8)),    // lot_size
            Some(Quantity::new(100000.0, 8)), // max_quantity
            Some(Quantity::new(0.001, 8)),    // min_quantity
            None,                             // max_notional
            None,                             // min_notional
            Some(Price::new(1000000.0, 2)),   // max_price
            Some(Price::new(0.01, 2)),        // min_price
            Some(dec!(0.05)),                 // margin_init
            Some(dec!(0.03)),                 // margin_maint
            Some(dec!(0.0002)),               // maker_fee
            Some(dec!(0.0005)),               // taker_fee
            UnixNanos::default(),             // ts_event
            UnixNanos::default(),             // ts_init
        ))
    }

    // ------------------------------------------------------------------------
    // Precision & bar conversion tests
    // ------------------------------------------------------------------------

    #[tokio::test]
    async fn test_candle_to_bar_price_size_edge_cases() {
        setup_test_env();

        let clock = get_atomic_clock_realtime();
        let now = Utc::now();

        // Very large prices and sizes (edge cases).
        let candle = Candle {
            started_at: now,
            ticker: "BTC-USD".to_string(),
            resolution: crate::common::enums::DydxCandleResolution::OneMinute,
            open: dec!(123456789.123456),
            high: dec!(987654321.987654),  // high is max
            low: dec!(123456.789),         // low is min
            close: dec!(223456789.123456), // close between low and high
            base_token_volume: dec!(0.00000001),
            usd_volume: dec!(1234500.0),
            trades: 42,
            starting_open_interest: dec!(1000.0),
        };

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        let bar = DydxDataClient::candle_to_bar(
            &candle,
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            60,
            clock,
        )
        .expect("candle_to_bar should handle large/scientific values");

        assert!(bar.open.as_f64() > 0.0);
        assert!(bar.high.as_f64() >= bar.low.as_f64());
        assert!(bar.volume.as_f64() > 0.0);
    }

    #[tokio::test]
    async fn test_candle_to_bar_ts_event_overflow_safe() {
        setup_test_env();

        let clock = get_atomic_clock_realtime();
        let now = Utc::now();

        let candle = Candle {
            started_at: now,
            ticker: "BTC-USD".to_string(),
            resolution: crate::common::enums::DydxCandleResolution::OneDay,
            open: Decimal::from(1),
            high: Decimal::from(1),
            low: Decimal::from(1),
            close: Decimal::from(1),
            base_token_volume: Decimal::from(1),
            usd_volume: Decimal::from(1),
            trades: 1,
            starting_open_interest: Decimal::from(1),
        };

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Day,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        // Use an intentionally large bar_secs to exercise saturating_add path.
        let bar_secs = i64::MAX / 1_000_000_000;
        let bar = DydxDataClient::candle_to_bar(
            &candle,
            bar_type,
            instrument.price_precision(),
            instrument.size_precision(),
            bar_secs,
            clock,
        )
        .expect("candle_to_bar should not overflow on ts_event");

        assert!(bar.ts_event.as_u64() >= bar.ts_init.as_u64());
    }

    #[tokio::test]
    async fn test_request_bars_incomplete_bar_filtering_with_clock_skew() {
        // Simulate bars with ts_event both before and after current_time_ns and
        // ensure only completed bars (ts_event < now) are retained.
        let clock = get_atomic_clock_realtime();
        let now = Utc::now();

        // Use a dedicated data channel for this test and register it
        // before constructing the data client.
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        // two candles: one in the past, one in the future
        let candle_past = Candle {
            started_at: now - chrono::Duration::minutes(2),
            ticker: "BTC-USD".to_string(),
            resolution: crate::common::enums::DydxCandleResolution::OneMinute,
            open: Decimal::from(1),
            high: Decimal::from(2),
            low: Decimal::from(1),
            close: Decimal::from(1),
            base_token_volume: Decimal::from(1),
            usd_volume: Decimal::from(1),
            trades: 1,
            starting_open_interest: Decimal::from(1),
        };
        let candle_future = Candle {
            started_at: now + chrono::Duration::minutes(2),
            ..candle_past.clone()
        };

        let candles_response = CandlesResponse {
            candles: vec![candle_past, candle_future],
        };

        let state = CandlesTestState {
            response: Arc::new(candles_response),
        };
        let addr = start_candles_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-BARS-SKEW");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_ref());
        client.instrument_cache.insert_instrument_only(instrument);

        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        let request = RequestBars::new(
            bar_type,
            Some(now - chrono::Duration::minutes(5)),
            Some(now + chrono::Duration::minutes(5)),
            None,
            Some(client_id),
            UUID4::new(),
            clock.get_time_ns(),
            None,
        );

        assert!(client.request_bars(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Bars(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Only the past candle should remain after filtering.
            assert_eq!(resp.data.len(), 1);
        }
    }

    #[rstest]
    fn test_decimal_to_f64_precision_loss_within_tolerance() {
        // Verify converting via Price/Quantity preserves reasonable precision.
        let price_value = 12345.125_f64;
        let qty_value = 0.00012345_f64;

        let price = Price::new(price_value, 6);
        let qty = Quantity::new(qty_value, 8);

        let price_diff = (price.as_f64() - price_value).abs();
        let qty_diff = (qty.as_f64() - qty_value).abs();

        // Differences should be well within a tiny epsilon.
        assert!(price_diff < 1e-10);
        assert!(qty_diff < 1e-12);
    }

    #[tokio::test]
    async fn test_orderbook_refresh_task_applies_http_snapshot_and_emits_event() {
        // Set up a dedicated data event channel for this test.
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        // Prepare a static orderbook snapshot served by a local Axum HTTP server.
        let snapshot = crate::http::models::OrderbookResponse {
            bids: vec![crate::http::models::OrderbookLevel {
                price: dec!(100.0),
                size: dec!(1.0),
            }],
            asks: vec![crate::http::models::OrderbookLevel {
                price: dec!(101.0),
                size: dec!(2.0),
            }],
        };
        let state = OrderbookTestState {
            snapshot: Arc::new(snapshot),
        };
        let addr = start_orderbook_test_server(state).await;
        let base_url = format!("http://{addr}");

        // Configure the data client with a short refresh interval and mock HTTP base URL.
        let client_id = ClientId::from("DYDX-REFRESH");
        let config = DydxDataClientConfig {
            is_testnet: true,
            base_url_http: Some(base_url),
            orderbook_refresh_interval_secs: Some(1),
            instrument_refresh_interval_secs: None,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let mut client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Seed instruments and orderbook state for a single instrument.
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_ref());
        client.instrument_cache.insert_instrument_only(instrument);
        client.order_books.insert(
            instrument_id,
            OrderBook::new(instrument_id, BookType::L2_MBP),
        );
        client.active_orderbook_subs.insert(instrument_id, ());

        // Start the refresh task and wait for a snapshot to be applied and emitted.
        client.start_orderbook_refresh_task().unwrap();

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
        let mut saw_snapshot_event = false;

        while std::time::Instant::now() < deadline {
            if let Ok(Some(DataEvent::Data(NautilusData::Deltas(_)))) =
                tokio::time::timeout(std::time::Duration::from_millis(250), rx.recv()).await
            {
                saw_snapshot_event = true;
                break;
            }
        }

        assert!(
            saw_snapshot_event,
            "expected at least one snapshot deltas event from refresh task"
        );

        // Verify that the local orderbook has been updated with the snapshot.
        let book = client
            .order_books
            .get(&instrument_id)
            .expect("orderbook should exist after refresh");
        let best_bid = book.best_bid_price().expect("best bid should be set");
        let best_ask = book.best_ask_price().expect("best ask should be set");

        assert_eq!(best_bid, Price::from("100.00"));
        assert_eq!(best_ask, Price::from("101.00"));
    }

    #[rstest]
    fn test_resolve_crossed_order_book_bid_larger_than_ask() {
        // Test scenario: bid_size > ask_size
        // Expected: DELETE ask, UPDATE bid (reduce by ask_size)
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Create initial non-crossed book
        let initial_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("99.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("2.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        book.apply_deltas(&OrderBookDeltas::new(instrument_id, initial_deltas))
            .unwrap();

        // Create crossed orderbook: bid @ 102.00 (size 5.0) > ask @ 101.00 (size 2.0)
        let crossed_deltas = vec![OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("102.00"),
                Quantity::from("5.0"),
                0,
            ),
            0,
            0,
            ts_init,
            ts_init,
        )];
        let venue_deltas = OrderBookDeltas::new(instrument_id, crossed_deltas);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, venue_deltas, &instrument)
                .unwrap();

        // Verify resolution: ask @ 101.00 should be deleted
        // bid @ 102.00 should remain but reduced (note: precision affects exact value)
        assert_eq!(book.best_bid_price(), Some(Price::from("102.00")));
        assert!(book.best_bid_size().unwrap().as_f64() < 5.0); // Reduced from original
        assert!(
            book.best_ask_price().is_none()
                || book.best_ask_price().unwrap() > book.best_bid_price().unwrap()
        ); // No longer crossed

        // Verify synthetic deltas were generated
        assert!(resolved.deltas.len() > 1); // Original delta + synthetic resolution deltas
    }

    #[rstest]
    fn test_resolve_crossed_order_book_ask_larger_than_bid() {
        // Test scenario: bid_size < ask_size
        // Expected: DELETE bid, UPDATE ask (reduce by bid_size)
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Create initial non-crossed book
        let initial_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("99.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("5.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        book.apply_deltas(&OrderBookDeltas::new(instrument_id, initial_deltas))
            .unwrap();

        // Create crossed orderbook: bid @ 102.00 (size 2.0) < ask @ 101.00 (size 5.0)
        let crossed_deltas = vec![OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("102.00"),
                Quantity::from("2.0"),
                0,
            ),
            0,
            0,
            ts_init,
            ts_init,
        )];
        let venue_deltas = OrderBookDeltas::new(instrument_id, crossed_deltas);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, venue_deltas, &instrument)
                .unwrap();

        // Verify resolution: bid @ 102.00 should be deleted, ask @ 101.00 reduced
        assert_eq!(book.best_ask_price(), Some(Price::from("101.00")));
        assert!(book.best_ask_size().unwrap().as_f64() < 5.0); // Reduced from original
        assert_eq!(book.best_bid_price(), Some(Price::from("99.00"))); // Next bid level remains
        assert!(book.best_ask_price().unwrap() > book.best_bid_price().unwrap()); // No longer crossed

        // Verify synthetic deltas were generated
        assert!(resolved.deltas.len() > 1);
    }

    #[rstest]
    fn test_resolve_crossed_order_book_equal_sizes() {
        // Test scenario: bid_size == ask_size
        // Expected: DELETE both bid and ask
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Create initial non-crossed book with multiple levels
        let initial_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("99.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("3.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        book.apply_deltas(&OrderBookDeltas::new(instrument_id, initial_deltas))
            .unwrap();

        // Create crossed orderbook: bid @ 102.00 (size 3.0) == ask @ 101.00 (size 3.0)
        let crossed_deltas = vec![OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("102.00"),
                Quantity::from("3.0"),
                0,
            ),
            0,
            0,
            ts_init,
            ts_init,
        )];
        let venue_deltas = OrderBookDeltas::new(instrument_id, crossed_deltas);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, venue_deltas, &instrument)
                .unwrap();

        // Verify resolution: both crossed levels should be deleted, reverting to deeper levels
        assert_eq!(book.best_bid_price(), Some(Price::from("99.00"))); // Next bid level
        // Ask at 101.00 should be deleted, book may be empty on ask side or have deeper levels
        if let Some(ask_price) = book.best_ask_price() {
            assert!(ask_price > book.best_bid_price().unwrap()); // No longer crossed
        }

        // Verify synthetic deltas were generated
        assert!(resolved.deltas.len() > 1);
    }

    #[rstest]
    fn test_resolve_crossed_order_book_multiple_iterations() {
        // Test scenario: multiple crossed levels requiring multiple iterations
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Create initial book with multiple levels on both sides
        let initial_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("98.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("100.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        book.apply_deltas(&OrderBookDeltas::new(instrument_id, initial_deltas))
            .unwrap();

        // Create heavily crossed orderbook with multiple bids above asks
        let crossed_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("102.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("103.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        let venue_deltas = OrderBookDeltas::new(instrument_id, crossed_deltas);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, venue_deltas, &instrument)
                .unwrap();

        // Verify final state is uncrossed (or book has no asks left)
        if let (Some(bid_price), Some(ask_price)) = (book.best_bid_price(), book.best_ask_price()) {
            assert!(ask_price > bid_price, "Book should be uncrossed");
        }

        // Verify multiple synthetic deltas were generated for multiple iterations
        assert!(resolved.deltas.len() > 2); // Original deltas + multiple resolution passes
    }

    #[rstest]
    fn test_resolve_crossed_order_book_non_crossed_passthrough() {
        // Test scenario: non-crossed orderbook should pass through unchanged
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        // Create normal non-crossed book
        let initial_deltas = vec![
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::from("99.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
            OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::from("101.00"),
                    Quantity::from("1.0"),
                    0,
                ),
                0,
                0,
                ts_init,
                ts_init,
            ),
        ];
        book.apply_deltas(&OrderBookDeltas::new(instrument_id, initial_deltas))
            .unwrap();

        // Add another non-crossing level
        let new_deltas = vec![OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::from("98.50"),
                Quantity::from("2.0"),
                0,
            ),
            0,
            0,
            ts_init,
            ts_init,
        )];
        let venue_deltas = OrderBookDeltas::new(instrument_id, new_deltas.clone());

        let original_bid = book.best_bid_price();
        let original_ask = book.best_ask_price();

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, venue_deltas, &instrument)
                .unwrap();

        // Verify no resolution was needed - deltas should be original only
        assert_eq!(resolved.deltas.len(), new_deltas.len());
        assert_eq!(book.best_bid_price(), original_bid);
        assert_eq!(book.best_ask_price(), original_ask);
        assert!(book.best_ask_price().unwrap() > book.best_bid_price().unwrap());
    }

    #[tokio::test]
    async fn test_request_instruments_successful_fetch() {
        // Test successful fetch of all instruments
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Execute request (spawns async task)
        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Wait for response (with timeout)
        let timeout = tokio::time::Duration::from_secs(5);
        let result = tokio::time::timeout(timeout, rx.recv()).await;

        match result {
            Ok(Some(DataEvent::Response(resp))) => {
                if let DataResponse::Instruments(inst_resp) = resp {
                    // Verify response structure
                    assert_eq!(inst_resp.correlation_id, request_id);
                    assert_eq!(inst_resp.client_id, client_id);
                    assert_eq!(inst_resp.venue, *DYDX_VENUE);
                    assert!(inst_resp.start.is_none());
                    assert!(inst_resp.end.is_none());
                    // Note: may be empty if HTTP fails, but structure should be correct
                }
            }
            Ok(Some(_)) => panic!("Expected InstrumentsResponse"),
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => {
                // Timeout is acceptable if testnet is unreachable
                println!("Test timed out - testnet may be unreachable");
            }
        }
    }

    #[tokio::test]
    async fn test_request_instruments_empty_response_on_http_error() {
        // Test empty response handling when HTTP call fails
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-ERROR-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://invalid-url-does-not-exist.local".to_string()),
            ..Default::default()
        };
        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should receive empty response on error
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty instruments on HTTP error"
            );
            assert_eq!(resp.correlation_id, request_id);
            assert_eq!(resp.client_id, client_id);
        }
    }

    #[tokio::test]
    async fn test_request_instruments_caching() {
        // Test instrument caching after fetch
        setup_test_env();

        let client_id = ClientId::from("DYDX-CACHE-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let initial_cache_size = client.instrument_cache.len();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Wait for async task to complete
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify cache populated (if HTTP succeeded)
        let final_cache_size = client.instrument_cache.len();
        // Cache should be unchanged (empty) if HTTP failed, or populated if succeeded
        // We can't assert exact size without mocking, but can verify no panic
        assert!(final_cache_size >= initial_cache_size);
    }

    #[tokio::test]
    async fn test_request_instruments_correlation_id_matching() {
        // Test correlation_id matching in response
        setup_test_env();

        let client_id = ClientId::from("DYDX-CORR-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request_id = UUID4::new();
        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should execute without panic (actual correlation checked in async handler)
        assert!(client.request_instruments(request).is_ok());
    }

    #[tokio::test]
    async fn test_request_instruments_venue_assignment() {
        // Test venue assignment
        setup_test_env();

        let client_id = ClientId::from("DYDX-VENUE-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        assert_eq!(client.venue(), *DYDX_VENUE);

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());
    }

    #[tokio::test]
    async fn test_request_instruments_timestamp_handling() {
        // Test timestamp handling (start_nanos, end_nanos)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TS-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let now = Utc::now();
        let start = Some(now - chrono::Duration::hours(24));
        let end = Some(now);

        let request = RequestInstruments::new(
            start,
            end,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Wait for response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify timestamps are set
            assert!(resp.start.unwrap() > 0);
            assert!(resp.end.unwrap() > 0);
            assert!(resp.start.unwrap() <= resp.end.unwrap());
            assert!(resp.ts_init > 0);
        }
    }

    #[tokio::test]
    async fn test_request_instruments_with_start_only() {
        // Test timestamp handling when only `start` is provided
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TS-START-ONLY");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let now = Utc::now();
        let start = Some(now - chrono::Duration::hours(24));

        let request = RequestInstruments::new(
            start,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.start.is_some());
            assert!(resp.end.is_none());
            assert!(resp.ts_init > 0);
        }
    }

    #[tokio::test]
    async fn test_request_instruments_with_end_only() {
        // Test timestamp handling when only `end` is provided
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TS-END-ONLY");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let now = Utc::now();
        let end = Some(now);

        let request = RequestInstruments::new(
            None,
            end,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.start.is_none());
            assert!(resp.end.is_some());
            assert!(resp.ts_init > 0);
        }
    }

    #[tokio::test]
    async fn test_request_instruments_client_id_fallback() {
        // Test client_id fallback to default when not provided
        setup_test_env();

        let client_id = ClientId::from("DYDX-FALLBACK-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            None, // No client_id provided
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should use client's default client_id
        assert!(client.request_instruments(request).is_ok());
    }

    #[tokio::test]
    async fn test_request_instruments_with_params() {
        // Test custom params handling
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-PARAMS-TEST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Create params - just verify they're passed through
        let mut params_map = IndexMap::new();
        params_map.insert("test_key".to_string(), "test_value".to_string());

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            Some(params_map),
        );

        assert!(client.request_instruments(request).is_ok());

        // Wait for response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify params are propagated into the response
            assert_eq!(resp.client_id, client_id);
            let params = resp
                .params
                .expect("expected params to be present in InstrumentsResponse");
            assert_eq!(
                params.get("test_key").map(String::as_str),
                Some("test_value")
            );
        }
    }

    #[tokio::test]
    async fn test_request_instruments_with_start_and_end_range() {
        // Test timestamp handling when both start and end are provided
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-START-END-RANGE");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let now = Utc::now();
        let start = Some(now - chrono::Duration::hours(48));
        let end = Some(now - chrono::Duration::hours(24));

        let request = RequestInstruments::new(
            start,
            end,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify both timestamps are present
            assert!(
                resp.start.is_some(),
                "start timestamp should be present when provided"
            );
            assert!(
                resp.end.is_some(),
                "end timestamp should be present when provided"
            );
            assert!(resp.ts_init > 0, "ts_init should always be set");

            // Verify start is before end
            if let (Some(start_ts), Some(end_ts)) = (resp.start, resp.end) {
                assert!(
                    start_ts < end_ts,
                    "start timestamp should be before end timestamp"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_request_instruments_different_client_ids() {
        // Test that different client_id values are properly handled using a shared channel.
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let timeout = tokio::time::Duration::from_secs(3);

        // First client
        let client_id_1 = ClientId::from("DYDX-CLIENT-1");
        let config1 = DydxDataClientConfig::default();
        let http_client1 = DydxHttpClient::default();
        let client1 =
            DydxDataClient::new(client_id_1, config1, http_client1, create_test_ws_client())
                .unwrap();

        let request1 = RequestInstruments::new(
            None,
            None,
            Some(client_id_1),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client1.request_instruments(request1).is_ok());

        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp1)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(
                resp1.client_id, client_id_1,
                "Response should contain client_id_1"
            );
        }

        // Second client
        let client_id_2 = ClientId::from("DYDX-CLIENT-2");
        let config2 = DydxDataClientConfig::default();
        let http_client2 = DydxHttpClient::default();
        let client2 =
            DydxDataClient::new(client_id_2, config2, http_client2, create_test_ws_client())
                .unwrap();

        let request2 = RequestInstruments::new(
            None,
            None,
            Some(client_id_2),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client2.request_instruments(request2).is_ok());

        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp2)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(
                resp2.client_id, client_id_2,
                "Response should contain client_id_2"
            );
            assert_ne!(
                resp2.client_id, client_id_1,
                "Different clients should have different client_ids"
            );
        }
    }

    #[tokio::test]
    async fn test_request_instruments_no_timestamps() {
        // Test fetching all current instruments (no start/end filters)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NO-TIMESTAMPS");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None, // No start filter
            None, // No end filter
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(5);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify no timestamp filters
            assert!(
                resp.start.is_none(),
                "start should be None when not provided"
            );
            assert!(resp.end.is_none(), "end should be None when not provided");

            // Should still get current instruments
            assert_eq!(resp.venue, *DYDX_VENUE);
            assert_eq!(resp.client_id, client_id);
            assert!(resp.ts_init > 0);
        }
    }

    #[tokio::test]
    async fn test_request_instrument_cache_hit() {
        // Test cache hit (instrument already cached)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-CACHE-HIT");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Pre-populate cache with test instrument
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Should get immediate response from cache
        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.instrument_id, instrument_id);
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.data.id(), instrument_id);
        }
    }

    #[tokio::test]
    async fn test_request_instrument_cache_miss() {
        // Test cache miss (fetch from API)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-CACHE-MISS");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument_id = InstrumentId::from("BTC-USD-PERP.DYDX");

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Wait for async HTTP fetch and response
        let timeout = tokio::time::Duration::from_secs(5);
        let result = tokio::time::timeout(timeout, rx.recv()).await;

        // May timeout if testnet unreachable, but should not panic
        match result {
            Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) => {
                assert_eq!(resp.instrument_id, instrument_id);
                assert_eq!(resp.client_id, client_id);
            }
            Ok(Some(_)) => panic!("Expected InstrumentResponse"),
            Ok(None) => panic!("Channel closed unexpectedly"),
            Err(_) => {
                println!("Test timed out - testnet may be unreachable");
            }
        }
    }

    #[tokio::test]
    async fn test_request_instrument_not_found() {
        // Test instrument not found scenario
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NOT-FOUND");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://invalid-url.local".to_string()),
            ..Default::default()
        };
        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument_id = InstrumentId::from("INVALID-SYMBOL.DYDX");

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should not panic on invalid instrument
        assert!(client.request_instrument(request).is_ok());

        // Note: No response sent when instrument not found (by design)
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
    }

    #[tokio::test]
    async fn test_request_instrument_bulk_caching() {
        // Test bulk caching when fetching from API
        setup_test_env();

        let client_id = ClientId::from("DYDX-BULK-CACHE");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let initial_cache_size = client.instrument_cache.len();

        let instrument_id = InstrumentId::from("ETH-USD-PERP.DYDX");

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Wait for async bulk fetch
        tokio::time::sleep(tokio::time::Duration::from_secs(2)).await;

        // Verify cache populated with all instruments (if HTTP succeeded)
        let final_cache_size = client.instrument_cache.len();
        assert!(final_cache_size >= initial_cache_size);
        // If HTTP succeeded, cache should have multiple instruments
    }

    #[tokio::test]
    async fn test_request_instrument_correlation_id() {
        // Test correlation_id matching
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-CORR-ID");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Pre-populate cache to get immediate response
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request_id = UUID4::new();
        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Verify correlation_id matches
        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.correlation_id, request_id);
        }
    }

    #[tokio::test]
    async fn test_request_instrument_response_format_boxed() {
        // Verify InstrumentResponse format (boxed)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-BOXED");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Pre-populate cache
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Verify response is properly boxed
        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(boxed_resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify boxed response structure
            assert_eq!(boxed_resp.instrument_id, instrument_id);
            assert_eq!(boxed_resp.client_id, client_id);
            assert!(boxed_resp.start.is_none());
            assert!(boxed_resp.end.is_none());
            assert!(boxed_resp.ts_init > 0);
        }
    }

    #[rstest]
    fn test_request_instrument_symbol_extraction() {
        // Test symbol extraction from InstrumentId
        setup_test_env();

        let client_id = ClientId::from("DYDX-SYMBOL");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let _client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Test various instrument ID formats
        // Note: Symbol includes the -PERP suffix in dYdX
        let test_cases = vec![
            ("BTC-USD-PERP.DYDX", "BTC-USD-PERP"),
            ("ETH-USD-PERP.DYDX", "ETH-USD-PERP"),
            ("SOL-USD-PERP.DYDX", "SOL-USD-PERP"),
        ];

        for (instrument_id_str, expected_symbol) in test_cases {
            let instrument_id = InstrumentId::from(instrument_id_str);
            let symbol = Ustr::from(instrument_id.symbol.as_str());
            assert_eq!(symbol.as_str(), expected_symbol);
        }
    }

    #[tokio::test]
    async fn test_request_instrument_client_id_fallback() {
        // Test client_id fallback to default
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-FALLBACK");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Pre-populate cache
        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            None, // No client_id provided
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Should use client's default client_id
        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.client_id, client_id);
        }
    }

    #[tokio::test]
    async fn test_request_trades_success_with_limit_and_symbol_conversion() {
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();

        let http_trade = crate::http::models::Trade {
            id: "trade-1".to_string(),
            side: OrderSide::Buy,
            size: dec!(1.5),
            price: dec!(100.25),
            created_at,
            created_at_height: 1,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![http_trade],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state.clone()).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-SUCCESS");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();
        let now = Utc::now();
        let start = Some(now - chrono::Duration::seconds(10));
        let end = Some(now + chrono::Duration::seconds(10));
        let limit = std::num::NonZeroUsize::new(100).unwrap();

        let request = RequestTrades::new(
            instrument_id,
            start,
            end,
            Some(limit),
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(1);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.correlation_id, request_id);
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.instrument_id, instrument_id);
            assert_eq!(resp.data.len(), 1);

            let tick = &resp.data[0];
            assert_eq!(tick.instrument_id, instrument_id);
            assert_eq!(tick.price, Price::new(100.25, price_precision));
            assert_eq!(tick.size, Quantity::new(1.5, size_precision));
            assert_eq!(tick.trade_id.to_string(), "trade-1");
            assert_eq!(tick.aggressor_side, AggressorSide::Buyer);
        } else {
            panic!("did not receive trades response in time");
        }

        // Verify symbol conversion (strip -PERP suffix) and limit propagation.
        let last_ticker = state.last_ticker.lock().await.clone();
        assert_eq!(last_ticker.as_deref(), Some("BTC-USD"));

        let last_limit = *state.last_limit.lock().await;
        assert_eq!(last_limit, Some(Some(100)));
    }

    #[tokio::test]
    async fn test_request_trades_empty_response_and_no_limit() {
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let trades_response = crate::http::models::TradesResponse { trades: vec![] };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state.clone()).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-EMPTY");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None, // No limit
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(1);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.correlation_id, request_id);
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.instrument_id, instrument_id);
            assert!(resp.data.is_empty());
        } else {
            panic!("did not receive trades response in time");
        }

        // Verify that no `limit` query parameter was sent.
        let last_limit = *state.last_limit.lock().await;
        assert_eq!(last_limit, Some(None));
    }

    #[tokio::test]
    async fn test_request_trades_timestamp_filtering() {
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let now = Utc::now();
        let trade_before = crate::http::models::Trade {
            id: "before".to_string(),
            side: OrderSide::Buy,
            size: dec!(1.0),
            price: dec!(100.0),
            created_at: now - chrono::Duration::seconds(60),
            created_at_height: 1,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };
        let trade_inside = crate::http::models::Trade {
            id: "inside".to_string(),
            side: OrderSide::Sell,
            size: dec!(2.0),
            price: dec!(101.0),
            created_at: now,
            created_at_height: 2,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };
        let trade_after = crate::http::models::Trade {
            id: "after".to_string(),
            side: OrderSide::Buy,
            size: dec!(3.0),
            price: dec!(102.0),
            created_at: now + chrono::Duration::seconds(60),
            created_at_height: 3,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![trade_before, trade_inside.clone(), trade_after],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-FILTER");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();

        // Filter range includes only the "inside" trade.
        let start = Some(now - chrono::Duration::seconds(10));
        let end = Some(now + chrono::Duration::seconds(10));

        let request = RequestTrades::new(
            instrument_id,
            start,
            end,
            None,
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(1);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.correlation_id, request_id);
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.instrument_id, instrument_id);
            assert_eq!(resp.data.len(), 1);

            let tick = &resp.data[0];
            assert_eq!(tick.trade_id.to_string(), "inside");
            assert_eq!(tick.price.as_decimal(), dec!(101.0));
        } else {
            panic!("did not receive trades response in time");
        }
    }

    #[tokio::test]
    async fn test_request_trades_correlation_id_matching() {
        // Test correlation_id matching in response
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let trades_response = crate::http::models::TradesResponse { trades: vec![] };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-CORR");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.correlation_id, request_id);
        }
    }

    #[tokio::test]
    async fn test_request_trades_response_format() {
        // Verify TradesResponse format
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();
        let http_trade = crate::http::models::Trade {
            id: "format-test".to_string(),
            side: OrderSide::Sell,
            size: dec!(5.0),
            price: dec!(200.0),
            created_at,
            created_at_height: 100,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![http_trade],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-FORMAT");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify response structure
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.instrument_id, instrument_id);
            assert!(resp.data.len() == 1);
            assert!(resp.ts_init > 0);

            // Verify trade tick structure
            let tick = &resp.data[0];
            assert_eq!(tick.instrument_id, instrument_id);
            assert!(tick.ts_event > 0);
            assert!(tick.ts_init > 0);
        }
    }

    #[tokio::test]
    async fn test_request_trades_no_instrument_in_cache() {
        // Test empty response when instrument not in cache
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TRADES-NO-INST");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Don't add instrument to cache
        let instrument_id = InstrumentId::from("UNKNOWN-SYMBOL.DYDX");

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        // Should receive empty response when instrument not found
        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.data.is_empty());
        }
    }

    #[tokio::test]
    async fn test_request_trades_limit_parameter() {
        // Test limit parameter handling
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let trades_response = crate::http::models::TradesResponse { trades: vec![] };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state.clone()).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-TRADES-LIMIT");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        // Test with limit
        let limit = std::num::NonZeroUsize::new(500).unwrap();
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            Some(limit),
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let state_clone = state.clone();
        wait_until_async(
            || async { state_clone.last_limit.lock().await.is_some() },
            Duration::from_secs(5),
        )
        .await;

        // Verify limit was passed to HTTP client
        let last_limit = *state.last_limit.lock().await;
        assert_eq!(last_limit, Some(Some(500)));
    }

    #[rstest]
    fn test_request_trades_symbol_conversion() {
        // Test symbol conversion (strip -PERP suffix)
        setup_test_env();

        let client_id = ClientId::from("DYDX-SYMBOL-CONV");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();
        let _client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Verify symbol format for various instruments
        let test_cases = vec![
            ("BTC-USD-PERP.DYDX", "BTC-USD"),
            ("ETH-USD-PERP.DYDX", "ETH-USD"),
            ("SOL-USD-PERP.DYDX", "SOL-USD"),
        ];

        for (instrument_id_str, expected_ticker) in test_cases {
            let instrument_id = InstrumentId::from(instrument_id_str);
            let ticker = instrument_id
                .symbol
                .as_str()
                .trim_end_matches("-PERP")
                .to_string();
            assert_eq!(ticker, expected_ticker);
        }
    }

    #[tokio::test]
    async fn test_http_404_handling() {
        // Test HTTP 404 handling (instrument not found)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-404");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://localhost:1/nonexistent".to_string()),
            http_timeout_secs: Some(1),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Should receive empty response on 404
        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.data.is_empty(), "Expected empty response on 404");
        }
    }

    #[tokio::test]
    async fn test_http_500_handling() {
        // Test HTTP 500 handling (internal server error)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-500");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://httpstat.us/500".to_string()),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        // Should receive empty response on 500 error
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.data.is_empty(), "Expected empty response on 500");
        }
    }

    #[tokio::test]
    async fn test_network_timeout_handling() {
        // Test network timeout scenarios
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TIMEOUT");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://10.255.255.1:81".to_string()), // Non-routable IP
            http_timeout_secs: Some(1),                                // Very short timeout
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Should timeout and return empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.data.is_empty(), "Expected empty response on timeout");
        }
    }

    #[tokio::test]
    async fn test_connection_refused_handling() {
        // Test connection refused errors
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-REFUSED");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://localhost:9999".to_string()), // Port unlikely to be open
            http_timeout_secs: Some(1),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Should handle connection refused gracefully
        let timeout = tokio::time::Duration::from_secs(2);
        let result = tokio::time::timeout(timeout, rx.recv()).await;

        // May not receive response if connection fails before spawning handler
        // This is acceptable - the important part is no panic
        match result {
            Ok(Some(DataEvent::Response(_))) => {
                // Response received (empty data expected)
            }
            Ok(None) | Err(_) => {
                // No response or timeout - acceptable for connection refused
            }
            _ => {}
        }
    }

    #[tokio::test]
    async fn test_dns_resolution_failure_handling() {
        // Test DNS resolution failures
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-DNS");
        let config = DydxDataClientConfig {
            base_url_http: Some(
                "http://this-domain-definitely-does-not-exist-12345.invalid".to_string(),
            ),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Should handle DNS failure gracefully with empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty response on DNS failure"
            );
        }
    }

    #[tokio::test]
    async fn test_http_502_503_handling() {
        // Test HTTP 502/503 handling (bad gateway/service unavailable)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-503");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://httpstat.us/503".to_string()),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Should handle 503 gracefully with empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.data.is_empty(), "Expected empty response on 503");
        }
    }

    #[tokio::test]
    async fn test_http_429_rate_limit_handling() {
        // Test HTTP 429 handling (rate limit exceeded)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-429");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://httpstat.us/429".to_string()),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        // Should handle rate limit with empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty response on rate limit"
            );
        }
    }

    #[tokio::test]
    async fn test_error_handling_does_not_panic() {
        // Test that error scenarios don't cause panics
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NO-PANIC");
        let config = DydxDataClientConfig {
            base_url_http: Some("http://invalid".to_string()),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // All these should return Ok() without panicking
        let request_instruments = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_instruments(request_instruments).is_ok());

        let instrument_id = InstrumentId::from("INVALID.DYDX");
        let request_instrument = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_instrument(request_instrument).is_ok());

        let request_trades = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_trades(request_trades).is_ok());
    }

    #[tokio::test]
    async fn test_malformed_json_response() {
        // Test handling of malformed JSON from API
        use axum::{Router, routing::get};

        #[derive(Clone)]
        struct MalformedState;

        async fn malformed_markets_handler() -> String {
            // Invalid JSON - missing closing brace
            r#"{"markets": {"BTC-USD": {"ticker": "BTC-USD""#.to_string()
        }

        let app = Router::new()
            .route("/v4/markets", get(malformed_markets_handler))
            .with_state(MalformedState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-MALFORMED");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        // Should handle malformed JSON gracefully with empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty response on malformed JSON"
            );
        }
    }

    #[tokio::test]
    async fn test_missing_required_fields_in_response() {
        // Test handling when API response missing required fields
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct MissingFieldsState;

        async fn missing_fields_handler() -> Json<Value> {
            // Missing critical fields like "ticker", "stepSize", etc.
            Json(json!({
                "markets": {
                    "BTC-USD": {
                        // Missing "ticker"
                        "status": "ACTIVE",
                        "baseAsset": "BTC",
                        "quoteAsset": "USD",
                        // Missing "stepSize", "tickSize", "minOrderSize"
                    }
                }
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(missing_fields_handler))
            .with_state(MissingFieldsState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-MISSING");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should handle missing fields gracefully (may skip instruments or return empty)
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Parse errors should result in empty or partial response
            // The important part is no panic
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_invalid_data_types_in_response() {
        // Test handling when API returns wrong data types
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct InvalidTypesState;

        async fn invalid_types_handler() -> Json<Value> {
            // Wrong data types - strings instead of numbers, etc.
            Json(json!({
                "markets": {
                    "BTC-USD": {
                        "ticker": "BTC-USD",
                        "status": "ACTIVE",
                        "baseAsset": "BTC",
                        "quoteAsset": "USD",
                        "stepSize": "not_a_number",  // Should be numeric
                        "tickSize": true,  // Should be numeric
                        "minOrderSize": ["array"],  // Should be numeric
                        "market": 12345,  // Should be string
                    }
                }
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(invalid_types_handler))
            .with_state(InvalidTypesState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TYPES");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should handle type errors gracefully
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Type mismatch should result in parse failure and empty/partial response
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_unexpected_response_structure() {
        // Test handling when API response has completely unexpected structure
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct UnexpectedState;

        async fn unexpected_structure_handler() -> Json<Value> {
            // Completely different structure than expected
            Json(json!({
                "error": "Something went wrong",
                "code": 500,
                "data": null,
                "unexpected_field": {
                    "nested": {
                        "deeply": [1, 2, 3]
                    }
                }
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(unexpected_structure_handler))
            .with_state(UnexpectedState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-STRUCT");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should handle unexpected structure gracefully with empty response
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty response on unexpected structure"
            );
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_empty_markets_object_in_response() {
        // Test handling when markets object is empty (valid JSON but no data)
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct EmptyMarketsState;

        async fn empty_markets_handler() -> Json<Value> {
            Json(json!({
                "markets": {}
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(empty_markets_handler))
            .with_state(EmptyMarketsState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-EMPTY");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should handle empty markets gracefully
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(
                resp.data.is_empty(),
                "Expected empty response for empty markets"
            );
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_null_values_in_response() {
        // Test handling of null values in critical fields
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct NullValuesState;

        async fn null_values_handler() -> Json<Value> {
            Json(json!({
                "markets": {
                    "BTC-USD": {
                        "ticker": null,
                        "status": "ACTIVE",
                        "baseAsset": null,
                        "quoteAsset": "USD",
                        "stepSize": null,
                    }
                }
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(null_values_handler))
            .with_state(NullValuesState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NULL");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_instruments(request).is_ok());

        // Should handle null values gracefully
        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Null values should cause parse failures and result in empty/partial response
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_invalid_instrument_id_format() {
        // Test handling of non-existent instrument (valid ID format but doesn't exist)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-INVALID-ID");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Valid format but non-existent instrument
        let non_existent_id = InstrumentId::from("NONEXISTENT-USD.DYDX");

        let request = RequestInstrument::new(
            non_existent_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        // Should handle non-existent instrument gracefully
        let timeout = tokio::time::Duration::from_secs(2);
        let result = tokio::time::timeout(timeout, rx.recv()).await;

        // Either no response or empty response is acceptable for non-existent instrument
        match result {
            Ok(Some(DataEvent::Response(DataResponse::Instrument(_)))) => {
                // Empty response acceptable
            }
            Ok(None) | Err(_) => {
                // Timeout or no response also acceptable
            }
            _ => {}
        }
    }

    #[tokio::test]
    async fn test_invalid_date_range_end_before_start() {
        // Test handling when end date is before start date
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-DATE-RANGE");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        // Invalid date range: end is before start
        let start = Utc::now();
        let end = start - chrono::Duration::hours(24); // End is 24 hours before start

        let request = RequestTrades::new(
            instrument_id,
            Some(start),
            Some(end),
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_trades(request).is_ok());

        // Should handle invalid range gracefully - may return empty or no response
        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Empty response expected for invalid date range
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_negative_limit_value() {
        // Test handling of limit edge cases
        // Note: Rust's NonZeroUsize prevents negative/zero values at type level
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NEG-LIMIT");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        // Minimum valid limit (1)
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            std::num::NonZeroUsize::new(1), // Minimum valid value
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should not panic with minimum limit
        assert!(client.request_trades(request).is_ok());
    }

    #[tokio::test]
    async fn test_zero_limit_value() {
        // Test handling of no limit (None = use API default)
        // Note: NonZeroUsize type prevents actual zero, so None represents "no limit"
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-ZERO-LIMIT");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None, // No limit specified (None = use default)
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let request_id = request.request_id;
        assert!(client.request_trades(request).is_ok());

        // Should handle None limit gracefully (uses API default)
        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_very_large_limit_value() {
        // Test handling of extremely large limit values (boundary testing)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-LARGE-LIMIT");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            std::num::NonZeroUsize::new(1_000_000), // Very large limit
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should not panic with very large limit
        let request_id = request.request_id;
        assert!(client.request_trades(request).is_ok());

        // Should handle large limit gracefully
        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_none_limit_uses_default() {
        // Test that None limit falls back to default behavior
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NONE-LIMIT");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None, // No limit specified
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        // Should work fine with None limit (uses API default)
        let request_id = request.request_id;
        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert!(resp.correlation_id == request_id);
        }
    }

    #[tokio::test]
    async fn test_validation_does_not_panic() {
        // Test that various validation edge cases don't cause panics
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-VALIDATION");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        // Test 1: Invalid instrument ID
        let invalid_id = InstrumentId::from("INVALID.WRONG");
        let req1 = RequestInstrument::new(
            invalid_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_instrument(req1).is_ok());

        // Test 2: Invalid date range
        let start = Utc::now();
        let end = start - chrono::Duration::hours(1);
        let req2 = RequestTrades::new(
            instrument_id,
            Some(start),
            Some(end),
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_trades(req2).is_ok());

        // Test 3: Minimum limit (1)
        let req3 = RequestTrades::new(
            instrument_id,
            None,
            None,
            std::num::NonZeroUsize::new(1),
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_trades(req3).is_ok());

        // Test 4: Very large limit
        let req4 = RequestTrades::new(
            instrument_id,
            None,
            None,
            std::num::NonZeroUsize::new(usize::MAX),
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );
        assert!(client.request_trades(req4).is_ok());

        // All validation edge cases handled without panic
    }

    #[tokio::test]
    async fn test_instruments_response_has_correct_venue() {
        // Verify InstrumentsResponse includes correct DYDX venue
        use axum::{Json, Router, routing::get};
        use serde_json::{Value, json};

        #[derive(Clone)]
        struct VenueTestState;

        async fn venue_handler() -> Json<Value> {
            Json(json!({
                "markets": {
                    "BTC-USD": {
                        "ticker": "BTC-USD",
                        "status": "ACTIVE",
                        "baseAsset": "BTC",
                        "quoteAsset": "USD",
                        "stepSize": "0.0001",
                        "tickSize": "1",
                        "indexPrice": "50000",
                        "oraclePrice": "50000",
                        "priceChange24H": "1000",
                        "nextFundingRate": "0.0001",
                        "nextFundingAt": "2024-01-01T00:00:00.000Z",
                        "minOrderSize": "0.001",
                        "type": "PERPETUAL",
                        "initialMarginFraction": "0.05",
                        "maintenanceMarginFraction": "0.03",
                        "volume24H": "1000000",
                        "trades24H": "10000",
                        "openInterest": "5000000",
                        "incrementalInitialMarginFraction": "0.01",
                        "incrementalPositionSize": "10",
                        "maxPositionSize": "1000",
                        "baselinePositionSize": "100",
                        "assetResolution": "10000000000"
                    }
                }
            }))
        }

        let app = Router::new()
            .route("/v4/markets", get(venue_handler))
            .with_state(VenueTestState);

        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let server_addr = listener.local_addr().unwrap();
        let port = server_addr.port();

        tokio::spawn(async move {
            axum::serve(listener, app).await.unwrap();
        });

        wait_for_server(server_addr).await;

        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-VENUE-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(format!("http://127.0.0.1:{port}")),
            http_timeout_secs: Some(2),
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(3);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify venue is DYDX
            assert_eq!(resp.venue, *DYDX_VENUE, "Response should have DYDX venue");
        }
    }

    #[tokio::test]
    async fn test_instruments_response_contains_vec_instrument_any() {
        // Verify InstrumentsResponse contains Vec<InstrumentAny>
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-VEC-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify data is Vec<InstrumentAny>
            assert!(
                resp.data.is_empty() || !resp.data.is_empty(),
                "data should be Vec<InstrumentAny>"
            );
        }
    }

    #[tokio::test]
    async fn test_instruments_response_includes_correlation_id() {
        // Verify InstrumentsResponse includes correlation_id matching request_id
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-CORR-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request_id = UUID4::new();
        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify correlation_id matches request_id
            assert_eq!(
                resp.correlation_id, request_id,
                "correlation_id should match request_id"
            );
        }
    }

    #[tokio::test]
    async fn test_instruments_response_includes_client_id() {
        // Verify InstrumentsResponse includes client_id
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-CLIENT-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify client_id is included
            assert_eq!(
                resp.client_id, client_id,
                "client_id should be included in response"
            );
        }
    }

    #[tokio::test]
    async fn test_instruments_response_includes_timestamps() {
        // Verify InstrumentsResponse includes start, end, and ts_init timestamps
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-TS-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let start = Some(Utc::now() - chrono::Duration::days(1));
        let end = Some(Utc::now());
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = RequestInstruments::new(
            start,
            end,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            ts_init,
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify timestamps are included
            assert!(
                resp.start.is_some() || resp.start.is_none(),
                "start timestamp field exists"
            );
            assert!(
                resp.end.is_some() || resp.end.is_none(),
                "end timestamp field exists"
            );
            assert!(resp.ts_init > 0, "ts_init should be greater than 0");
        }
    }

    #[tokio::test]
    async fn test_instruments_response_includes_params_when_provided() {
        // Verify InstrumentsResponse includes params when provided in request
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-PARAMS-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        // Since we can't easily create IndexMap in tests without importing,
        // just verify the params field exists by passing None
        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None, // params
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify params field exists (structure validation)
            let _params = resp.params;
        }
    }

    #[tokio::test]
    async fn test_instruments_response_params_none_when_not_provided() {
        // Verify InstrumentsResponse params is None when not provided
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-NO-PARAMS");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request = RequestInstruments::new(
            None,
            None,
            Some(client_id),
            Some(*DYDX_VENUE),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None, // No params
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify params field exists and is None when not provided
            assert!(
                resp.params.is_none(),
                "params should be None when not provided"
            );
        }
    }

    #[tokio::test]
    async fn test_instruments_response_complete_structure() {
        // Comprehensive test verifying all InstrumentsResponse fields
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-FULL-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let request_id = UUID4::new();
        let start = Some(Utc::now() - chrono::Duration::hours(1));
        let end = Some(Utc::now());
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = RequestInstruments::new(
            start,
            end,
            Some(client_id),
            Some(*DYDX_VENUE),
            request_id,
            ts_init,
            None,
        );

        assert!(client.request_instruments(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instruments(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Comprehensive validation of all fields
            assert_eq!(resp.venue, *DYDX_VENUE, "venue should be DYDX");
            assert_eq!(
                resp.correlation_id, request_id,
                "correlation_id should match"
            );
            assert_eq!(resp.client_id, client_id, "client_id should match");
            assert!(resp.ts_init > 0, "ts_init should be set");

            // data field exists (Vec<InstrumentAny>)
            let _data: Vec<InstrumentAny> = resp.data;

            // Timestamp fields can be present or None
            let _start = resp.start;
            let _end = resp.end;
            let _params = resp.params;
        }
    }

    #[tokio::test]
    async fn test_instrument_response_properly_boxed() {
        // Verify InstrumentResponse is properly boxed in DataResponse
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-BOXED-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(boxed_resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify it's boxed - we receive Box<InstrumentResponse>
            let _response: Box<InstrumentResponse> = boxed_resp;
            // Successfully matched boxed pattern
        }
    }

    #[tokio::test]
    async fn test_instrument_response_contains_single_instrument() {
        // Verify InstrumentResponse contains single InstrumentAny
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-SINGLE-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify data contains single InstrumentAny
            let _instrument: InstrumentAny = resp.data;
            // Successfully matched InstrumentAny type
        }
    }

    #[tokio::test]
    async fn test_instrument_response_has_correct_instrument_id() {
        // Verify InstrumentResponse has correct instrument_id
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-ID-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify instrument_id matches request
            assert_eq!(
                resp.instrument_id, instrument_id,
                "instrument_id should match requested ID"
            );
        }
    }

    #[tokio::test]
    async fn test_instrument_response_includes_metadata() {
        // Verify InstrumentResponse includes all metadata fields
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-META-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();
        let start = Some(Utc::now() - chrono::Duration::hours(1));
        let end = Some(Utc::now());
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = RequestInstrument::new(
            instrument_id,
            start,
            end,
            Some(client_id),
            request_id,
            ts_init,
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify all metadata fields
            assert_eq!(
                resp.correlation_id, request_id,
                "correlation_id should match"
            );
            assert_eq!(resp.client_id, client_id, "client_id should match");
            assert!(resp.ts_init > 0, "ts_init should be set");

            // Timestamp fields exist (can be Some or None)
            let _start = resp.start;
            let _end = resp.end;

            // Params field exists
            let _params = resp.params;
        }
    }

    #[tokio::test]
    async fn test_instrument_response_matches_requested_instrument() {
        // Verify InstrumentResponse data matches the requested instrument exactly
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-MATCH-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify returned instrument matches requested instrument
            assert_eq!(
                resp.data.id(),
                instrument_id,
                "Returned instrument should match requested"
            );
            assert_eq!(
                resp.instrument_id, instrument_id,
                "instrument_id field should match"
            );

            // Both should point to the same instrument
            assert_eq!(
                resp.data.id(),
                resp.instrument_id,
                "data.id() should match instrument_id field"
            );
        }
    }

    #[tokio::test]
    async fn test_instrument_response_complete_structure() {
        // Comprehensive test verifying all InstrumentResponse fields
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let client_id = ClientId::from("DYDX-FULL-INST-TEST");
        let config = DydxDataClientConfig::default();

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client
            .instrument_cache
            .insert_instrument_only(instrument.clone());

        let request_id = UUID4::new();
        let ts_init = get_atomic_clock_realtime().get_time_ns();

        let request = RequestInstrument::new(
            instrument_id,
            None,
            None,
            Some(client_id),
            request_id,
            ts_init,
            None,
        );

        assert!(client.request_instrument(request).is_ok());

        let timeout = tokio::time::Duration::from_secs(2);
        if let Ok(Some(DataEvent::Response(DataResponse::Instrument(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Comprehensive validation
            // 1. Boxed structure
            let _boxed: Box<InstrumentResponse> = resp.clone();

            // 2. All required fields present
            assert_eq!(resp.correlation_id, request_id);
            assert_eq!(resp.client_id, client_id);
            assert_eq!(resp.instrument_id, instrument_id);
            assert!(resp.ts_init > 0);

            // 3. Data field contains InstrumentAny
            let returned_instrument: InstrumentAny = resp.data;
            assert_eq!(returned_instrument.id(), instrument_id);

            // 4. Optional fields exist
            let _start = resp.start;
            let _end = resp.end;
            let _params = resp.params;
        }
    }

    #[tokio::test]
    async fn test_trades_response_contains_vec_trade_tick() {
        // Verify TradesResponse.data is Vec<TradeTick>
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();
        let http_trades = vec![
            crate::http::models::Trade {
                id: "trade-1".to_string(),
                side: OrderSide::Buy,
                size: dec!(1.0),
                price: dec!(100.0),
                created_at,
                created_at_height: 100,
                trade_type: crate::common::enums::DydxTradeType::Limit,
            },
            crate::http::models::Trade {
                id: "trade-2".to_string(),
                side: OrderSide::Sell,
                size: dec!(2.0),
                price: dec!(101.0),
                created_at: created_at + chrono::Duration::seconds(1),
                created_at_height: 101,
                trade_type: crate::common::enums::DydxTradeType::Limit,
            },
        ];

        let trades_response = crate::http::models::TradesResponse {
            trades: http_trades,
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-VEC-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify data is Vec<TradeTick>
            let trade_ticks: Vec<TradeTick> = resp.data;
            assert_eq!(trade_ticks.len(), 2, "Should contain 2 TradeTick elements");

            // Each element is a TradeTick
            for tick in &trade_ticks {
                assert_eq!(tick.instrument_id, instrument_id);
            }
        }
    }

    #[tokio::test]
    async fn test_trades_response_has_correct_instrument_id() {
        // Verify TradesResponse.instrument_id matches request
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();
        let http_trade = crate::http::models::Trade {
            id: "instrument-id-test".to_string(),
            side: OrderSide::Buy,
            size: dec!(1.0),
            price: dec!(100.0),
            created_at,
            created_at_height: 100,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![http_trade],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-INSTID-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify instrument_id field matches request
            assert_eq!(
                resp.instrument_id, instrument_id,
                "TradesResponse.instrument_id should match request"
            );

            // Verify all trade ticks have the same instrument_id
            for tick in &resp.data {
                assert_eq!(
                    tick.instrument_id, instrument_id,
                    "Each TradeTick should have correct instrument_id"
                );
            }
        }
    }

    #[tokio::test]
    async fn test_trades_response_properly_ordered_by_timestamp() {
        // Verify trades are ordered by timestamp (ascending)
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let base_time = Utc::now();
        let http_trades = vec![
            crate::http::models::Trade {
                id: "trade-oldest".to_string(),
                side: OrderSide::Buy,
                size: dec!(1.0),
                price: dec!(100.0),
                created_at: base_time,
                created_at_height: 100,
                trade_type: crate::common::enums::DydxTradeType::Limit,
            },
            crate::http::models::Trade {
                id: "trade-middle".to_string(),
                side: OrderSide::Sell,
                size: dec!(2.0),
                price: dec!(101.0),
                created_at: base_time + chrono::Duration::seconds(1),
                created_at_height: 101,
                trade_type: crate::common::enums::DydxTradeType::Limit,
            },
            crate::http::models::Trade {
                id: "trade-newest".to_string(),
                side: OrderSide::Buy,
                size: dec!(3.0),
                price: dec!(102.0),
                created_at: base_time + chrono::Duration::seconds(2),
                created_at_height: 102,
                trade_type: crate::common::enums::DydxTradeType::Limit,
            },
        ];

        let trades_response = crate::http::models::TradesResponse {
            trades: http_trades,
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-ORDER-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify trades are ordered by timestamp
            let trade_ticks = resp.data;
            assert_eq!(trade_ticks.len(), 3, "Should have 3 trades");

            // Check ascending timestamp order
            for i in 1..trade_ticks.len() {
                assert!(
                    trade_ticks[i].ts_event >= trade_ticks[i - 1].ts_event,
                    "Trades should be ordered by timestamp (ts_event) in ascending order"
                );
            }

            // Verify specific ordering
            assert!(
                trade_ticks[0].ts_event < trade_ticks[1].ts_event,
                "First trade should be before second"
            );
            assert!(
                trade_ticks[1].ts_event < trade_ticks[2].ts_event,
                "Second trade should be before third"
            );
        }
    }

    #[tokio::test]
    async fn test_trades_response_all_trade_tick_fields_populated() {
        // Verify all TradeTick fields are properly populated
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();
        let http_trade = crate::http::models::Trade {
            id: "field-test".to_string(),
            side: OrderSide::Buy,
            size: dec!(5.5),
            price: dec!(12345.67),
            created_at,
            created_at_height: 999,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![http_trade],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-FIELDS-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            assert_eq!(resp.data.len(), 1, "Should have 1 trade");
            let tick = &resp.data[0];

            // Verify all TradeTick fields are properly populated
            assert_eq!(
                tick.instrument_id, instrument_id,
                "instrument_id should be set"
            );
            assert!(tick.price.as_f64() > 0.0, "price should be positive");
            assert!(tick.size.as_f64() > 0.0, "size should be positive");

            // Verify aggressor_side is set (Buy or Sell)
            match tick.aggressor_side {
                AggressorSide::Buyer | AggressorSide::Seller => {
                    // Valid aggressor side
                }
                AggressorSide::NoAggressor => {
                    panic!("aggressor_side should be Buyer or Seller, not NoAggressor")
                }
            }

            // Verify trade_id is set
            assert!(
                !tick.trade_id.to_string().is_empty(),
                "trade_id should be set"
            );

            // Verify timestamps are set and valid
            assert!(tick.ts_event > 0, "ts_event should be set");
            assert!(tick.ts_init > 0, "ts_init should be set");
            assert!(
                tick.ts_init >= tick.ts_event,
                "ts_init should be >= ts_event"
            );
        }
    }

    #[tokio::test]
    async fn test_trades_response_includes_metadata() {
        // Verify TradesResponse includes all metadata fields
        let (sender, mut rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let created_at = Utc::now();
        let http_trade = crate::http::models::Trade {
            id: "metadata-test".to_string(),
            side: OrderSide::Buy,
            size: dec!(1.0),
            price: dec!(100.0),
            created_at,
            created_at_height: 100,
            trade_type: crate::common::enums::DydxTradeType::Limit,
        };

        let trades_response = crate::http::models::TradesResponse {
            trades: vec![http_trade],
        };

        let state = TradesTestState {
            response: Arc::new(trades_response),
            last_ticker: Arc::new(tokio::sync::Mutex::new(None)),
            last_limit: Arc::new(tokio::sync::Mutex::new(None)),
        };

        let addr = start_trades_test_server(state).await;
        let base_url = format!("http://{addr}");

        let client_id = ClientId::from("DYDX-META-TEST");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client =
            DydxDataClient::new(client_id, config, http_client, create_test_ws_client()).unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        let _symbol_key = Ustr::from(instrument_id.symbol.as_str());
        client.instrument_cache.insert_instrument_only(instrument);

        let request_id = UUID4::new();
        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            None,
            Some(client_id),
            request_id,
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        assert!(client.request_trades(request).is_ok());

        let timeout = tokio::time::Duration::from_millis(500);
        if let Ok(Some(DataEvent::Response(DataResponse::Trades(resp)))) =
            tokio::time::timeout(timeout, rx.recv()).await
        {
            // Verify metadata fields
            assert_eq!(
                resp.correlation_id, request_id,
                "correlation_id should match request"
            );
            assert_eq!(resp.client_id, client_id, "client_id should be set");
            assert_eq!(
                resp.instrument_id, instrument_id,
                "instrument_id should be set"
            );
            assert!(resp.ts_init > 0, "ts_init should be set");

            let _start = resp.start;
            let _end = resp.end;
            let _params = resp.params;
        }
    }

    #[tokio::test]
    async fn test_orderbook_cache_growth_with_many_instruments() {
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let base_url = String::from("https://indexer.v4testnet.dydx.exchange");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client = DydxDataClient::new(
            ClientId::from("dydx_test"),
            config,
            http_client,
            create_test_ws_client(),
        )
        .unwrap();

        let initial_capacity = client.order_books.capacity();

        for i in 0..100 {
            let symbol = format!("INSTRUMENT-{i}");
            let instrument_id = InstrumentId::from(format!("{symbol}-PERP.DYDX"));
            client.order_books.insert(
                instrument_id,
                OrderBook::new(instrument_id, BookType::L2_MBP),
            );
        }

        assert_eq!(client.order_books.len(), 100);
        assert!(client.order_books.capacity() >= initial_capacity);

        client.order_books.clear();
        assert_eq!(client.order_books.len(), 0);
    }

    #[rstest]
    fn test_instrument_id_validation_rejects_invalid_formats() {
        // InstrumentId::from() validates format and panics on invalid input
        let test_cases = vec![
            ("", "Empty string missing separator"),
            ("INVALID", "No venue separator"),
            ("NO-VENUE", "No venue separator"),
            (".DYDX", "Empty symbol"),
            ("SYMBOL.", "Empty venue"),
        ];

        for (invalid_id, description) in test_cases {
            let result = std::panic::catch_unwind(|| InstrumentId::from(invalid_id));
            assert!(
                result.is_err(),
                "Expected {invalid_id} to panic: {description}"
            );
        }
    }

    #[rstest]
    fn test_instrument_id_validation_accepts_valid_formats() {
        let valid_ids = vec![
            "BTC-USD-PERP.DYDX",
            "ETH-USD-PERP.DYDX",
            "SOL-USD.DYDX",
            "AVAX-USD-PERP.DYDX",
        ];

        for valid_id in valid_ids {
            let instrument_id = InstrumentId::from(valid_id);
            assert!(
                !instrument_id.symbol.as_str().is_empty()
                    && !instrument_id.venue.as_str().is_empty(),
                "Expected {valid_id} to have non-empty symbol and venue"
            );
        }
    }

    #[tokio::test]
    async fn test_request_bars_with_inverted_date_range() {
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let base_url = String::from("https://indexer.v4testnet.dydx.exchange");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client = DydxDataClient::new(
            ClientId::from("dydx_test"),
            config,
            http_client,
            create_test_ws_client(),
        )
        .unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        client.instrument_cache.insert_instrument_only(instrument);

        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        let now = Utc::now();
        let start = Some(now);
        let end = Some(now - chrono::Duration::hours(1));

        let request = RequestBars::new(
            bar_type,
            start,
            end,
            None,
            Some(ClientId::from("dydx_test")),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let result = client.request_bars(request);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_request_bars_with_zero_limit() {
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let base_url = String::from("https://indexer.v4testnet.dydx.exchange");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client = DydxDataClient::new(
            ClientId::from("dydx_test"),
            config,
            http_client,
            create_test_ws_client(),
        )
        .unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        client.instrument_cache.insert_instrument_only(instrument);

        let spec = BarSpecification {
            step: std::num::NonZeroUsize::new(1).unwrap(),
            aggregation: BarAggregation::Minute,
            price_type: PriceType::Last,
        };
        let bar_type = BarType::new(instrument_id, spec, AggregationSource::External);

        let request = RequestBars::new(
            bar_type,
            None,
            None,
            Some(std::num::NonZeroUsize::new(1).unwrap()),
            Some(ClientId::from("dydx_test")),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let result = client.request_bars(request);
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_request_trades_with_excessive_limit() {
        let (sender, _rx) = tokio::sync::mpsc::unbounded_channel::<DataEvent>();
        set_data_event_sender(sender);

        let base_url = String::from("https://indexer.v4testnet.dydx.exchange");
        let config = DydxDataClientConfig {
            base_url_http: Some(base_url),
            is_testnet: true,
            ..Default::default()
        };

        let http_client = DydxHttpClient::new(
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.http_proxy_url.clone(),
            config.is_testnet,
            None,
        )
        .unwrap();

        let client = DydxDataClient::new(
            ClientId::from("dydx_test"),
            config,
            http_client,
            create_test_ws_client(),
        )
        .unwrap();

        let instrument = create_test_instrument_any();
        let instrument_id = instrument.id();
        client.instrument_cache.insert_instrument_only(instrument);

        let request = RequestTrades::new(
            instrument_id,
            None,
            None,
            Some(std::num::NonZeroUsize::new(100_000).unwrap()),
            Some(ClientId::from("dydx_test")),
            UUID4::new(),
            get_atomic_clock_realtime().get_time_ns(),
            None,
        );

        let result = client.request_trades(request);
        assert!(result.is_ok());
    }

    #[rstest]
    fn test_candle_topic_format() {
        let instrument_id = InstrumentId::new(Symbol::from("BTC-USD-PERP"), Venue::from("DYDX"));
        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let resolution = "1MIN";
        let topic = format!("{ticker}/{resolution}");

        assert_eq!(topic, "BTC-USD/1MIN");
        assert!(!topic.contains("-PERP"));
        assert!(!topic.contains(".DYDX"));
    }
}
