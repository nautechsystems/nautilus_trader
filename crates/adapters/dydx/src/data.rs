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

use std::{
    str::FromStr,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use dashmap::DashMap;
use futures_util::{Stream, StreamExt, pin_mut};
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, FundingRatesResponse, InstrumentResponse, InstrumentsResponse,
            RequestBars, RequestFundingRates, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices,
            SubscribeInstrument, SubscribeInstrumentStatus, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeInstrument, UnsubscribeInstrumentStatus, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, AtomicSet,
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{
        Bar, BarSpecification, BarType, BookOrder, Data as NautilusData, FundingRateUpdate,
        IndexPriceUpdate, InstrumentStatus, MarkPriceUpdate, OrderBookDelta, OrderBookDeltas,
        OrderBookDeltas_API, QuoteTick,
    },
    enums::{BookAction, BookType, MarketStatusAction, OrderSide, RecordFlag},
    identifiers::{ClientId, InstrumentId, Symbol, Venue},
    instruments::{Instrument, InstrumentAny},
    orderbook::OrderBook,
    types::Quantity,
};
use rust_decimal::Decimal;
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::DYDX_VENUE,
        enums::DydxCandleResolution,
        instrument_cache::InstrumentCache,
        parse::{extract_raw_symbol, parse_price},
    },
    config::DydxDataClientConfig,
    http::client::DydxHttpClient,
    websocket::{client::DydxWebSocketClient, enums::DydxWsOutputMessage, parse as ws_parse},
};

struct WsMessageContext {
    clock: &'static AtomicTime,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_cache: Arc<InstrumentCache>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    ws_client: DydxWebSocketClient,
    http_client: DydxHttpClient,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    active_bar_subs: Arc<AtomicMap<(InstrumentId, String), BarType>>,
    incomplete_bars: Arc<DashMap<BarType, Bar>>,
    bar_type_mappings: Arc<AtomicMap<String, BarType>>,
    active_mark_price_subs: Arc<AtomicSet<InstrumentId>>,
    active_index_price_subs: Arc<AtomicSet<InstrumentId>>,
    active_funding_rate_subs: Arc<AtomicSet<InstrumentId>>,
    active_instrument_status_subs: Arc<AtomicSet<InstrumentId>>,
    last_instrument_statuses: Arc<DashMap<InstrumentId, InstrumentStatus>>,
    bars_timestamp_on_close: bool,
    pending_bars: Arc<DashMap<String, Bar>>,
    seen_tickers: Arc<AtomicSet<Ustr>>,
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
    clock: &'static AtomicTime,
    client_id: ClientId,
    config: DydxDataClientConfig,
    http_client: DydxHttpClient,
    ws_client: DydxWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instrument_cache: Arc<InstrumentCache>,
    order_books: Arc<DashMap<InstrumentId, OrderBook>>,
    last_quotes: Arc<DashMap<InstrumentId, QuoteTick>>,
    incomplete_bars: Arc<DashMap<BarType, Bar>>,
    bar_type_mappings: Arc<AtomicMap<String, BarType>>,
    active_quote_subs: Arc<AtomicSet<InstrumentId>>,
    active_delta_subs: Arc<AtomicSet<InstrumentId>>,
    active_trade_subs: Arc<AtomicSet<InstrumentId>>,
    active_bar_subs: Arc<AtomicMap<(InstrumentId, String), BarType>>,
    active_mark_price_subs: Arc<AtomicSet<InstrumentId>>,
    active_index_price_subs: Arc<AtomicSet<InstrumentId>>,
    active_funding_rate_subs: Arc<AtomicSet<InstrumentId>>,
    active_instrument_status_subs: Arc<AtomicSet<InstrumentId>>,
    last_instrument_statuses: Arc<DashMap<InstrumentId, InstrumentStatus>>,
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
            bar_type_mappings: Arc::new(AtomicMap::new()),
            active_quote_subs: Arc::new(AtomicSet::new()),
            active_delta_subs: Arc::new(AtomicSet::new()),
            active_trade_subs: Arc::new(AtomicSet::new()),
            active_bar_subs: Arc::new(AtomicMap::new()),
            active_mark_price_subs: Arc::new(AtomicSet::new()),
            active_index_price_subs: Arc::new(AtomicSet::new()),
            active_funding_rate_subs: Arc::new(AtomicSet::new()),
            active_instrument_status_subs: Arc::new(AtomicSet::new()),
            last_instrument_statuses: Arc::new(DashMap::new()),
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
        stream: impl Stream<Item = DydxWsOutputMessage> + Send + 'static,
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

    async fn bootstrap_instruments(&self) -> anyhow::Result<Vec<InstrumentAny>> {
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

        self.ws_client.cache_instruments(instruments.clone());

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

        self.bootstrap_instruments().await?;

        self.ws_client
            .connect()
            .await
            .context("failed to connect dYdX websocket")?;

        self.ws_client
            .subscribe_markets()
            .await
            .context("failed to subscribe to markets channel")?;

        let seen_tickers: Arc<AtomicSet<Ustr>> = Arc::new(AtomicSet::new());

        for instrument in self.instrument_cache.all_instruments() {
            let id = instrument.id();
            let ticker = extract_raw_symbol(id.symbol.as_str());
            seen_tickers.insert(Ustr::from(ticker));
        }

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
            bar_type_mappings: self.bar_type_mappings.clone(),
            active_mark_price_subs: self.active_mark_price_subs.clone(),
            active_index_price_subs: self.active_index_price_subs.clone(),
            active_funding_rate_subs: self.active_funding_rate_subs.clone(),
            active_instrument_status_subs: self.active_instrument_status_subs.clone(),
            last_instrument_statuses: self.last_instrument_statuses.clone(),
            bars_timestamp_on_close: self.ws_client.bars_timestamp_on_close(),
            pending_bars: Arc::new(DashMap::new()),
            seen_tickers,
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

        self.cancellation_token.cancel();

        self.await_tasks_with_timeout(Duration::from_secs(5)).await;

        self.ws_client
            .disconnect()
            .await
            .context("failed to disconnect dYdX websocket")?;

        self.last_instrument_statuses.clear();
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

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        log::debug!(
            "subscribe_instruments: dYdX instruments discovered via global v4_markets channel"
        );
        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: SubscribeInstrument) -> anyhow::Result<()> {
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

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!(
                "dYdX only supports L2_MBP order book deltas, received {:?}",
                cmd.book_type
            );
        }

        self.ensure_order_book(cmd.instrument_id, BookType::L2_MBP);
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

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
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

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.instrument_id;

        self.active_trade_subs.insert(instrument_id);

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

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_mark_price_subs.insert(instrument_id);
        log::info!("Subscribed to mark prices for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_index_price_subs.insert(instrument_id);
        log::info!("Subscribed to index prices for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let spec = cmd.bar_type.spec();

        let resolution = Self::map_bar_spec_to_resolution(&spec)?;
        let bar_type = cmd.bar_type;
        self.active_bar_subs
            .insert((instrument_id, resolution.to_string()), bar_type);

        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");
        self.bar_type_mappings.insert(topic, bar_type);

        self.spawn_ws(
            async move {
                ws.subscribe_candles(instrument_id, resolution)
                    .await
                    .context("candles subscription")
            },
            "dYdX candles subscription",
        );

        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_funding_rate_subs.insert(instrument_id);
        log::info!("Subscribed to funding rates for {instrument_id} (via v4_markets channel)");
        Ok(())
    }

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;
        self.active_instrument_status_subs.insert(instrument_id);
        log::info!("Subscribed to instrument status for {instrument_id} (via v4_markets channel)");

        // Replay last known status (initial snapshot arrives before subscription)
        if let Some(status) = self.last_instrument_statuses.get(&instrument_id)
            && let Err(e) = self.data_sender.send(DataEvent::InstrumentStatus(*status))
        {
            log::error!("Failed to replay instrument status for {instrument_id}: {e}");
        }

        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        log::debug!("unsubscribe_instruments: dYdX markets channel is global; no-op");
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        log::debug!("unsubscribe_instrument: dYdX markets channel is global; no-op");
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
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

        self.active_quote_subs.remove(&cmd.instrument_id);

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

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
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

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self.ws_client.clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let spec = cmd.bar_type.spec();

        let resolution = Self::map_bar_spec_to_resolution(&spec)?;

        self.active_bar_subs
            .remove(&(instrument_id, resolution.to_string()));

        let ticker = extract_raw_symbol(instrument_id.symbol.as_str());
        let topic = format!("{ticker}/{resolution}");
        self.bar_type_mappings.remove(&topic);

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

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        self.active_funding_rate_subs.remove(&cmd.instrument_id);
        log::info!("Unsubscribed from funding rates for {}", cmd.instrument_id);
        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        self.active_instrument_status_subs
            .remove(&cmd.instrument_id);
        log::info!(
            "Unsubscribed from instrument status for {}",
            cmd.instrument_id
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
            let instrument = match http.request_instruments(None, None, None).await {
                Ok(instruments) => {
                    for inst in &instruments {
                        instrument_cache.insert_instrument_only(inst.clone());
                    }
                    instruments.into_iter().find(|i| i.id() == instrument_id)
                }
                Err(e) => {
                    log::error!("Failed to fetch instruments from dYdX: {e:?}");
                    None
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

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
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
                .request_funding_rates(instrument_id, start, end, limit)
                .await
                .context("failed to request funding rates from dYdX")
            {
                Ok(funding_rates) => {
                    let response = DataResponse::FundingRates(FundingRatesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        funding_rates,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send funding rates response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Funding rates request failed for {instrument_id}: {e:?}");

                    let response = DataResponse::FundingRates(FundingRatesResponse::new(
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
                        log::error!("Failed to send empty funding rates response: {e}");
                    }
                }
            }
        });

        Ok(())
    }
}

impl DydxDataClient {
    /// Returns a cached instrument by InstrumentId.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instrument_cache.get(instrument_id)
    }

    /// Returns all cached instruments.
    #[must_use]
    pub fn get_instruments(&self) -> Vec<InstrumentAny> {
        self.instrument_cache.all_instruments()
    }

    /// Caches a single instrument.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instrument_cache.insert_instrument_only(instrument);
    }

    /// Caches multiple instruments.
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

    /// Returns the BarType for a given WebSocket candle topic.
    #[must_use]
    pub fn get_bar_type_for_topic(&self, topic: &str) -> Option<BarType> {
        self.bar_type_mappings.load().get(topic).copied()
    }

    /// Returns all registered bar topics.
    #[must_use]
    pub fn get_bar_topics(&self) -> Vec<String> {
        self.bar_type_mappings.load().keys().cloned().collect()
    }

    fn handle_ws_message(message: DydxWsOutputMessage, ctx: &WsMessageContext) {
        let ts_init = ctx.clock.get_time_ns();

        match message {
            DydxWsOutputMessage::Trades { id, contents } => {
                let Some(instrument) = ctx.instrument_cache.get_by_market(&id) else {
                    log::warn!("No instrument cached for market {id}");
                    return;
                };
                let instrument_id = instrument.id();

                match ws_parse::parse_trade_ticks(instrument_id, &instrument, &contents, ts_init) {
                    Ok(data) => {
                        Self::handle_data_message(
                            data,
                            &ctx.data_sender,
                            &ctx.incomplete_bars,
                            ctx.clock,
                        );
                    }
                    Err(e) => log::error!("Failed to parse trade ticks for {id}: {e}"),
                }
            }
            DydxWsOutputMessage::OrderbookSnapshot { id, contents } => {
                let Some(instrument) = ctx.instrument_cache.get_by_market(&id) else {
                    log::warn!("No instrument cached for market {id}");
                    return;
                };
                let instrument_id = instrument.id();

                match ws_parse::parse_orderbook_snapshot(
                    &instrument_id,
                    &contents,
                    instrument.price_precision(),
                    instrument.size_precision(),
                    ts_init,
                ) {
                    Ok(deltas) => {
                        Self::handle_deltas_message(
                            deltas,
                            &ctx.data_sender,
                            &ctx.order_books,
                            &ctx.last_quotes,
                            &ctx.instrument_cache,
                            &ctx.active_quote_subs,
                            &ctx.active_delta_subs,
                        );
                    }
                    Err(e) => log::error!("Failed to parse orderbook snapshot for {id}: {e}"),
                }
            }
            DydxWsOutputMessage::OrderbookUpdate { id, contents } => {
                let Some(instrument) = ctx.instrument_cache.get_by_market(&id) else {
                    log::warn!("No instrument cached for market {id}");
                    return;
                };
                let instrument_id = instrument.id();

                match ws_parse::parse_orderbook_deltas(
                    &instrument_id,
                    &contents,
                    instrument.price_precision(),
                    instrument.size_precision(),
                    ts_init,
                ) {
                    Ok(deltas) => {
                        Self::handle_deltas_message(
                            deltas,
                            &ctx.data_sender,
                            &ctx.order_books,
                            &ctx.last_quotes,
                            &ctx.instrument_cache,
                            &ctx.active_quote_subs,
                            &ctx.active_delta_subs,
                        );
                    }
                    Err(e) => log::error!("Failed to parse orderbook deltas for {id}: {e}"),
                }
            }
            DydxWsOutputMessage::OrderbookBatch { id, updates } => {
                let Some(instrument) = ctx.instrument_cache.get_by_market(&id) else {
                    log::warn!("No instrument cached for market {id}");
                    return;
                };
                let instrument_id = instrument.id();
                let price_precision = instrument.price_precision();
                let size_precision = instrument.size_precision();

                let mut all_deltas = Vec::new();
                let last_idx = updates.len().saturating_sub(1);

                for (i, update) in updates.iter().enumerate() {
                    let is_last = i == last_idx;
                    let result = if is_last {
                        ws_parse::parse_orderbook_deltas(
                            &instrument_id,
                            update,
                            price_precision,
                            size_precision,
                            ts_init,
                        )
                        .map(|d| d.deltas)
                    } else {
                        ws_parse::parse_orderbook_deltas_with_flag(
                            &instrument_id,
                            update,
                            price_precision,
                            size_precision,
                            ts_init,
                            false,
                        )
                    };

                    match result {
                        Ok(deltas) => all_deltas.extend(deltas),
                        Err(e) => {
                            log::error!("Failed to parse orderbook batch delta {i} for {id}: {e}");
                            return;
                        }
                    }
                }

                if all_deltas.is_empty() {
                    return;
                }
                let deltas = OrderBookDeltas::new(instrument_id, all_deltas);
                Self::handle_deltas_message(
                    deltas,
                    &ctx.data_sender,
                    &ctx.order_books,
                    &ctx.last_quotes,
                    &ctx.instrument_cache,
                    &ctx.active_quote_subs,
                    &ctx.active_delta_subs,
                );
            }
            DydxWsOutputMessage::Candles { id, contents } => {
                let parts: Vec<&str> = id.splitn(2, '/').collect();
                if parts.len() != 2 {
                    log::warn!("Unexpected candle topic format: {id}");
                    return;
                }
                let ticker = parts[0];

                let Some(bar_type) = ctx.bar_type_mappings.load().get(&id).copied() else {
                    log::debug!("No bar type mapping for candle topic {id}");
                    return;
                };

                let Some(instrument) = ctx.instrument_cache.get_by_market(ticker) else {
                    log::warn!("No instrument cached for market {ticker}");
                    return;
                };

                match ws_parse::parse_candle_bar(
                    bar_type,
                    &instrument,
                    &contents,
                    ctx.bars_timestamp_on_close,
                    ts_init,
                ) {
                    Ok(bar) => {
                        let prev = ctx.pending_bars.get(&id).map(|r| *r);
                        if let Some(prev_bar) = prev
                            && bar.ts_event != prev_bar.ts_event
                        {
                            Self::emit_bar_guarded(prev_bar, ctx);
                        }
                        ctx.pending_bars.insert(id, bar);
                    }
                    Err(e) => log::error!("Failed to parse candle bar for {id}: {e}"),
                }
            }
            DydxWsOutputMessage::Markets(contents) => {
                Self::handle_markets_message(&contents, ctx, ts_init);
            }
            DydxWsOutputMessage::SubaccountSubscribed(_) => {
                log::debug!("Ignoring subaccount subscribed on data client");
            }
            DydxWsOutputMessage::SubaccountsChannelData(_) => {
                log::debug!("Ignoring subaccounts channel data on data client");
            }
            DydxWsOutputMessage::BlockHeight { .. } => {
                log::debug!("Ignoring block height on data client");
            }
            DydxWsOutputMessage::Error(err) => {
                log::error!("dYdX WS error: {err}");
            }
            DydxWsOutputMessage::Reconnected => {
                log::info!("dYdX WS reconnected, re-subscribing to active subscriptions");
                ctx.pending_bars.clear();

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

                for instrument_id in ctx.active_quote_subs.load().iter().copied() {
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

                for instrument_id in ctx.active_delta_subs.load().iter().copied() {
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

                for instrument_id in ctx.active_trade_subs.load().iter().copied() {
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

                for ((instrument_id, resolution), _) in ctx.active_bar_subs.load().iter() {
                    let instrument_id = *instrument_id;
                    let resolution = resolution.clone();
                    let ws_clone = ctx.ws_client.clone();

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
        }
    }

    fn instrument_id_from_ticker(ticker: &str) -> InstrumentId {
        let symbol = format!("{ticker}-PERP");
        InstrumentId::new(Symbol::new(&symbol), *DYDX_VENUE)
    }

    fn handle_markets_message(
        contents: &crate::websocket::messages::DydxMarketsContents,
        ctx: &WsMessageContext,
        ts_init: nautilus_core::UnixNanos,
    ) {
        if let Some(ref oracle_prices) = contents.oracle_prices {
            for (ticker, oracle_data) in oracle_prices {
                let instrument_id = Self::instrument_id_from_ticker(ticker);

                let Ok(price) = parse_price(&oracle_data.oracle_price, "oracle_price") else {
                    log::warn!("Failed to parse oracle price for {ticker}");
                    continue;
                };

                if ctx.active_mark_price_subs.contains(&instrument_id) {
                    let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_init, ts_init);
                    let data = NautilusData::MarkPriceUpdate(mark_price);
                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit mark price for {instrument_id}: {e}");
                    }
                }

                if ctx.active_index_price_subs.contains(&instrument_id) {
                    let index_price = IndexPriceUpdate::new(instrument_id, price, ts_init, ts_init);
                    let data = NautilusData::IndexPriceUpdate(index_price);
                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit index price for {instrument_id}: {e}");
                    }
                }
            }
        }

        Self::handle_markets_trading_data(contents.trading.as_ref(), ctx, ts_init, false);
        Self::handle_markets_trading_data(contents.markets.as_ref(), ctx, ts_init, true);
    }

    fn handle_markets_trading_data(
        trading: Option<
            &std::collections::HashMap<String, crate::websocket::messages::DydxMarketTradingUpdate>,
        >,
        ctx: &WsMessageContext,
        ts_init: nautilus_core::UnixNanos,
        is_snapshot: bool,
    ) {
        let Some(trading_map) = trading else {
            return;
        };

        for (ticker, update) in trading_map {
            let instrument_id = Self::instrument_id_from_ticker(ticker);

            if let Some(status) = &update.status {
                let action = MarketStatusAction::from(*status);
                let is_trading = matches!(status, crate::common::enums::DydxMarketStatus::Active);

                let instrument_status = InstrumentStatus::new(
                    instrument_id,
                    action,
                    ts_init,
                    ts_init,
                    None,
                    None,
                    Some(is_trading),
                    None,
                    None,
                );

                ctx.last_instrument_statuses
                    .insert(instrument_id, instrument_status);

                if ctx.active_instrument_status_subs.contains(&instrument_id)
                    && let Err(e) = ctx
                        .data_sender
                        .send(DataEvent::InstrumentStatus(instrument_status))
                {
                    log::error!("Failed to emit instrument status for {instrument_id}: {e}");
                }
            }

            let ticker_ustr = Ustr::from(ticker.as_str());
            if !ctx.seen_tickers.contains(&ticker_ustr) {
                let is_active = update
                    .status
                    .as_ref()
                    .is_none_or(|s| matches!(s, crate::common::enums::DydxMarketStatus::Active));
                if ctx.instrument_cache.get_by_market(ticker).is_some() {
                    ctx.seen_tickers.insert(ticker_ustr);
                } else if is_active {
                    ctx.seen_tickers.insert(ticker_ustr);
                    Self::handle_new_instrument_discovered(ticker, ctx);
                }
            }

            if let Some(ref rate_str) = update.next_funding_rate {
                if let Ok(rate) = Decimal::from_str(rate_str) {
                    if ctx.active_funding_rate_subs.contains(&instrument_id) {
                        let funding_rate = FundingRateUpdate {
                            instrument_id,
                            rate,
                            interval: Some(60),
                            next_funding_ns: None,
                            ts_event: ts_init,
                            ts_init,
                        };

                        if let Err(e) = ctx.data_sender.send(DataEvent::FundingRate(funding_rate)) {
                            log::error!("Failed to emit funding rate for {instrument_id}: {e}");
                        }
                    }
                } else {
                    log::warn!("Failed to parse next_funding_rate for {ticker}: {rate_str}");
                }
            }

            if is_snapshot
                && let Some(ref oracle_price_str) = update.oracle_price
                && let Ok(price) = parse_price(oracle_price_str, "oracle_price")
            {
                if ctx.active_mark_price_subs.contains(&instrument_id) {
                    let mark_price = MarkPriceUpdate::new(instrument_id, price, ts_init, ts_init);
                    let data = NautilusData::MarkPriceUpdate(mark_price);

                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit mark price for {instrument_id}: {e}");
                    }
                }

                if ctx.active_index_price_subs.contains(&instrument_id) {
                    let index_price = IndexPriceUpdate::new(instrument_id, price, ts_init, ts_init);
                    let data = NautilusData::IndexPriceUpdate(index_price);

                    if let Err(e) = ctx.data_sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to emit index price for {instrument_id}: {e}");
                    }
                }
            }
        }
    }

    fn emit_bar_guarded(bar: Bar, ctx: &WsMessageContext) {
        let current_time_ns = ctx.clock.get_time_ns();
        if bar.ts_event <= current_time_ns {
            ctx.incomplete_bars.remove(&bar.bar_type);
            if let Err(e) = ctx
                .data_sender
                .send(DataEvent::Data(NautilusData::Bar(bar)))
            {
                log::error!("Failed to emit completed bar: {e}");
            }
        } else {
            ctx.incomplete_bars.insert(bar.bar_type, bar);
        }
    }

    fn handle_new_instrument_discovered(ticker: &str, ctx: &WsMessageContext) {
        log::info!("New instrument discovered via WebSocket: {ticker}");

        let http_client = ctx.http_client.clone();
        let ws_client = ctx.ws_client.clone();
        let data_sender = ctx.data_sender.clone();
        let ticker = ticker.to_string();

        get_runtime().spawn(async move {
            match http_client.fetch_and_cache_single_instrument(&ticker).await {
                Ok(Some(instrument)) => {
                    ws_client.cache_instrument(instrument.clone());
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
        venue_deltas: &OrderBookDeltas,
        instrument: &InstrumentAny,
    ) -> anyhow::Result<OrderBookDeltas> {
        let instrument_id = venue_deltas.instrument_id;
        let ts_init = venue_deltas.ts_init;
        let mut all_deltas = venue_deltas.deltas.clone();

        // If the input batch is a snapshot, every synthetic and terminator delta must
        // carry F_SNAPSHOT as well so consumers apply the whole batch as one
        // replacement image rather than a snapshot followed by standalone updates.
        let snapshot_flag = RecordFlag::F_SNAPSHOT as u8;
        let is_snapshot_batch = venue_deltas
            .deltas
            .iter()
            .any(|d| d.flags & snapshot_flag != 0);
        let synthetic_flags = if is_snapshot_batch { snapshot_flag } else { 0 };

        // Apply the original venue deltas first
        book.apply_deltas(venue_deltas)?;

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
                let new_bid_size = Quantity::from_decimal_dp(
                    bid_size.as_decimal() - ask_size.as_decimal(),
                    instrument.size_precision(),
                )?;
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update,
                    BookOrder::new(OrderSide::Buy, bid_price, new_bid_size, 0),
                    synthetic_flags,
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
                        Quantity::zero(instrument.size_precision()),
                        0,
                    ),
                    synthetic_flags,
                    0,
                    ts_init,
                    ts_init,
                ));
            } else if bid_size < ask_size {
                // Remove bid level, reduce ask level
                let new_ask_size = Quantity::from_decimal_dp(
                    ask_size.as_decimal() - bid_size.as_decimal(),
                    instrument.size_precision(),
                )?;
                temp_deltas.push(OrderBookDelta::new(
                    instrument_id,
                    BookAction::Update,
                    BookOrder::new(OrderSide::Sell, ask_price, new_ask_size, 0),
                    synthetic_flags,
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
                        Quantity::zero(instrument.size_precision()),
                        0,
                    ),
                    synthetic_flags,
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
                        Quantity::zero(instrument.size_precision()),
                        0,
                    ),
                    synthetic_flags,
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
                        Quantity::zero(instrument.size_precision()),
                        0,
                    ),
                    synthetic_flags,
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

        // Set F_LAST on the final delta, preserving F_SNAPSHOT when the batch is a
        // snapshot so consumers close the replacement image correctly.
        if let Some(last_delta) = all_deltas.last_mut() {
            last_delta.flags = synthetic_flags | RecordFlag::F_LAST as u8;
        }

        Ok(OrderBookDeltas::new(instrument_id, all_deltas))
    }

    fn handle_deltas_message(
        deltas: OrderBookDeltas,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        order_books: &Arc<DashMap<InstrumentId, OrderBook>>,
        last_quotes: &Arc<DashMap<InstrumentId, QuoteTick>>,
        instrument_cache: &Arc<InstrumentCache>,
        active_quote_subs: &Arc<AtomicSet<InstrumentId>>,
        active_delta_subs: &Arc<AtomicSet<InstrumentId>>,
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

        // Always maintain local orderbook -- both subscription types need book state
        let mut book = order_books
            .entry(instrument_id)
            .or_insert_with(|| OrderBook::new(instrument_id, BookType::L2_MBP));

        // Resolve crossed orderbook (applies deltas internally)
        let resolved_deltas =
            match Self::resolve_crossed_order_book(&mut book, &deltas, &instrument) {
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

#[cfg(test)]
mod tests {
    use nautilus_core::UnixNanos;
    use nautilus_model::{
        data::{BookOrder, OrderBookDelta, OrderBookDeltas},
        enums::{BookAction, BookType, OrderSide, RecordFlag},
        identifiers::{InstrumentId, Symbol, Venue},
        instruments::{CryptoPerpetual, InstrumentAny},
        orderbook::OrderBook,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;

    fn test_instrument() -> InstrumentAny {
        let instrument_id = InstrumentId::new(Symbol::new("BTC-USD-PERP"), Venue::new("DYDX"));
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            instrument_id,
            instrument_id.symbol,
            Currency::BTC(),
            Currency::USD(),
            Currency::USD(),
            false,
            2,                   // price_precision
            8,                   // size_precision (wide enough to reveal f64 rounding)
            Price::new(0.01, 2), // price_increment
            Quantity::new(0.00000001, 8),
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
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn seed_book_with_levels(
        instrument_id: InstrumentId,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> OrderBook {
        let mut book = OrderBook::new(instrument_id, BookType::L2_MBP);
        let ts = UnixNanos::default();

        let mut deltas: Vec<OrderBookDelta> = Vec::new();
        deltas.push(OrderBookDelta::clear(instrument_id, 0, ts, ts));
        for (price, size) in bids {
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Buy,
                    Price::new(*price, 2),
                    Quantity::new(*size, 8),
                    0,
                ),
                0,
                0,
                ts,
                ts,
            ));
        }

        for (price, size) in asks {
            deltas.push(OrderBookDelta::new(
                instrument_id,
                BookAction::Add,
                BookOrder::new(
                    OrderSide::Sell,
                    Price::new(*price, 2),
                    Quantity::new(*size, 8),
                    0,
                ),
                0,
                0,
                ts,
                ts,
            ));
        }

        if let Some(last) = deltas.last_mut() {
            last.flags = RecordFlag::F_LAST as u8;
        }

        book.apply_deltas(&OrderBookDeltas::new(instrument_id, deltas))
            .expect("failed to apply seed deltas");
        book
    }

    fn crossing_bid_deltas(
        instrument_id: InstrumentId,
        bid_price: f64,
        bid_size: f64,
    ) -> OrderBookDeltas {
        let ts = UnixNanos::default();
        let delta = OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::new(bid_price, 2),
                Quantity::new(bid_size, 8),
                0,
            ),
            RecordFlag::F_LAST as u8,
            0,
            ts,
            ts,
        );
        OrderBookDeltas::new(instrument_id, vec![delta])
    }

    #[rstest]
    fn test_resolve_crossed_order_book_preserves_decimal_precision() {
        // Book seeded uncrossed: bid at 99.00 / ask at 100.05 size=0.50000000.
        // Venue delta adds a crossing bid at 100.10 size=1.00000001.
        // The reducing side (Buy) must end up at size = 0.50000001 exactly --
        // f64 subtraction of 1.00000001 - 0.5 would round this to 0.50000000 at 8 dp.
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let mut book = seed_book_with_levels(
            instrument_id,
            &[(99.00, 1.00000000)],
            &[(100.05, 0.50000000)],
        );

        let venue_deltas = crossing_bid_deltas(instrument_id, 100.10, 1.00000001);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, &venue_deltas, &instrument)
                .expect("resolution should succeed");

        // An Update on the Buy side at the crossing price must carry the exact
        // Decimal-subtracted remainder (0.50000001), not the f64-rounded 0.50000000.
        let update = resolved
            .deltas
            .iter()
            .find(|d| {
                d.action == BookAction::Update
                    && d.order.side == OrderSide::Buy
                    && d.order.price.as_decimal() == dec!(100.10)
            })
            .expect("expected a Buy Update delta from crossed-book resolution");
        assert_eq!(update.order.size.as_decimal(), dec!(0.50000001));

        // The terminal delta must carry F_LAST so downstream buffering flushes.
        assert_eq!(
            resolved.deltas.last().unwrap().flags,
            RecordFlag::F_LAST as u8,
        );

        // Book is no longer crossed.
        if let (Some(bid), Some(ask)) = (book.best_bid_price(), book.best_ask_price()) {
            assert!(bid < ask, "book still crossed: bid={bid:?} ask={ask:?}");
        }
    }

    fn crossing_snapshot_batch(
        instrument_id: InstrumentId,
        bid_price: f64,
        bid_size: f64,
    ) -> OrderBookDeltas {
        let ts = UnixNanos::default();
        let snapshot = RecordFlag::F_SNAPSHOT as u8;
        let last = RecordFlag::F_LAST as u8;
        // Mimic an inbound snapshot: every delta carries F_SNAPSHOT; terminator also
        // carries F_LAST.
        let deltas = vec![OrderBookDelta::new(
            instrument_id,
            BookAction::Add,
            BookOrder::new(
                OrderSide::Buy,
                Price::new(bid_price, 2),
                Quantity::new(bid_size, 8),
                0,
            ),
            snapshot | last,
            0,
            ts,
            ts,
        )];
        OrderBookDeltas::new(instrument_id, deltas)
    }

    /// A crossed snapshot must exit `resolve_crossed_order_book` still flagged as a
    /// snapshot: every delta keeps `F_SNAPSHOT` and the terminator carries
    /// `F_SNAPSHOT | F_LAST`, so downstream consumers treat the emitted batch as a
    /// complete replacement image.
    #[rstest]
    fn test_resolve_crossed_order_book_preserves_snapshot_flags() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let mut book = seed_book_with_levels(
            instrument_id,
            &[(99.00, 1.00000000)],
            &[(100.05, 0.50000000)],
        );

        let venue_deltas = crossing_snapshot_batch(instrument_id, 100.10, 1.00000001);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, &venue_deltas, &instrument)
                .expect("resolution should succeed");

        let snapshot = RecordFlag::F_SNAPSHOT as u8;
        let last = RecordFlag::F_LAST as u8;

        // Every delta must still carry F_SNAPSHOT; the terminator carries both.
        for (idx, delta) in resolved.deltas.iter().enumerate() {
            assert!(
                delta.flags & snapshot != 0,
                "delta at index {idx} lost F_SNAPSHOT: flags={:#010b}",
                delta.flags,
            );
        }
        assert_eq!(
            resolved.deltas.last().unwrap().flags,
            snapshot | last,
            "snapshot terminator must be F_SNAPSHOT | F_LAST",
        );
    }

    #[rstest]
    fn test_resolve_crossed_order_book_equal_sizes_removes_both_levels() {
        // Seed bid at 99.00 / ask at 100.05 size=1.0, then add a crossing bid at
        // 100.10 size=1.0 -- both top-of-book sides match in size and must be deleted.
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let mut book = seed_book_with_levels(
            instrument_id,
            &[(99.00, 1.00000000)],
            &[(100.05, 1.00000000)],
        );

        let venue_deltas = crossing_bid_deltas(instrument_id, 100.10, 1.00000000);

        let resolved =
            DydxDataClient::resolve_crossed_order_book(&mut book, &venue_deltas, &instrument)
                .expect("resolution should succeed");

        // Equal-size branch must emit two Deletes (one per side) at top-of-book.
        let deletes_count = resolved
            .deltas
            .iter()
            .filter(|d| {
                d.action == BookAction::Delete
                    && (d.order.price.as_decimal() == dec!(100.10)
                        || d.order.price.as_decimal() == dec!(100.05))
            })
            .count();
        assert_eq!(deletes_count, 2);
    }
}
