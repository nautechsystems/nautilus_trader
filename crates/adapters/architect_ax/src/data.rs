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

//! Live market data client implementation for the AX Exchange adapter.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
use dashmap::DashMap;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, BookResponse, FundingRatesResponse, InstrumentResponse,
            InstrumentsResponse, RequestBars, RequestBookSnapshot, RequestFundingRates,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeFundingRates, SubscribeInstrument, SubscribeInstruments,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeFundingRates, UnsubscribeInstrument,
            UnsubscribeInstruments, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, FundingRateUpdate, OrderBookDeltas_API},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::InstrumentAny,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::AX_VENUE, credential::Credential, enums::AxMarketDataLevel,
        parse::map_bar_spec_to_candle_width,
    },
    config::AxDataClientConfig,
    http::client::AxHttpClient,
    websocket::{data::client::AxMdWebSocketClient, messages::NautilusDataWsMessage},
};

/// AX Exchange data client for live market data streaming and historical data requests.
///
/// This client integrates with the Nautilus DataEngine to provide:
/// - Real-time market data via WebSocket subscriptions
/// - Historical data via REST API requests
/// - Automatic instrument discovery and caching
/// - Connection lifecycle management
#[derive(Debug)]
pub struct AxDataClient {
    /// The client ID for this data client.
    client_id: ClientId,
    /// Configuration for the data client.
    config: AxDataClientConfig,
    /// HTTP client for REST API requests.
    http_client: AxHttpClient,
    /// WebSocket client for real-time data streaming.
    ws_client: AxMdWebSocketClient,
    /// Whether the client is currently connected.
    is_connected: Arc<AtomicBool>,
    /// Cancellation token for async operations.
    cancellation_token: CancellationToken,
    /// Background task handles.
    tasks: Vec<JoinHandle<()>>,
    /// Channel sender for emitting data events to the DataEngine.
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    /// Cached instruments by symbol (shared with HTTP client).
    instruments: Arc<DashMap<Ustr, InstrumentAny>>,
    /// High-resolution clock for timestamps.
    clock: &'static AtomicTime,
    funding_rate_tasks: AHashMap<InstrumentId, JoinHandle<()>>,
    funding_rate_cache: Arc<Mutex<AHashMap<InstrumentId, FundingRateUpdate>>>,
}

impl AxDataClient {
    /// Creates a new [`AxDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the data event sender cannot be obtained.
    pub fn new(
        client_id: ClientId,
        config: AxDataClientConfig,
        http_client: AxHttpClient,
        ws_client: AxMdWebSocketClient,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        // Share instruments cache with HTTP client
        let instruments = http_client.instruments_cache.clone();

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            is_connected: Arc::new(AtomicBool::new(false)),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments,
            clock,
            funding_rate_tasks: AHashMap::new(),
            funding_rate_cache: Arc::new(Mutex::new(AHashMap::new())),
        })
    }

    /// Returns the venue for this data client.
    #[must_use]
    pub fn venue(&self) -> Venue {
        *AX_VENUE
    }

    fn map_book_type_to_market_data_level(book_type: BookType) -> AxMarketDataLevel {
        match book_type {
            BookType::L3_MBO => AxMarketDataLevel::Level3,
            BookType::L1_MBP | BookType::L2_MBP => AxMarketDataLevel::Level2,
        }
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments
    }

    /// Spawns a message handler task to forward WebSocket data to the DataEngine.
    fn spawn_message_handler(&mut self) {
        let stream = self.ws_client.stream();
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();
        let is_connected = Arc::clone(&self.is_connected);

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Message handler cancelled");
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(ws_msg, &data_sender);
                            }
                            None => {
                                log::debug!("WebSocket stream ended");
                                is_connected.store(false, Ordering::Release);
                                break;
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    /// Handles a WebSocket message and forwards data to the DataEngine.
    fn handle_ws_message(
        msg: NautilusDataWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ) {
        match msg {
            NautilusDataWsMessage::Data(data_vec) => {
                for data in data_vec {
                    if let Err(e) = sender.send(DataEvent::Data(data)) {
                        log::error!("Failed to send data event: {e}");
                    }
                }
            }
            NautilusDataWsMessage::Deltas(deltas) => {
                let api_deltas = OrderBookDeltas_API::new(deltas);
                if let Err(e) = sender.send(DataEvent::Data(Data::Deltas(api_deltas))) {
                    log::error!("Failed to send deltas event: {e}");
                }
            }
            NautilusDataWsMessage::Bar(bar) => {
                if let Err(e) = sender.send(DataEvent::Data(Data::Bar(bar))) {
                    log::error!("Failed to send bar event: {e}");
                }
            }
            NautilusDataWsMessage::Heartbeat => {
                log::trace!("Received heartbeat");
            }
            NautilusDataWsMessage::Reconnected => {
                log::info!("WebSocket reconnected");
            }
            NautilusDataWsMessage::Error(err) => {
                // Subscription state messages are benign (e.g. duplicate subscribe/unsubscribe)
                if err.message.contains("already subscribed")
                    || err.message.contains("not subscribed")
                {
                    log::warn!("WebSocket subscription state: {err:?}");
                } else {
                    log::error!("WebSocket error: {err:?}");
                }
            }
        }
    }

    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });
    }
}

#[async_trait(?Send)]
impl DataClient for AxDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*AX_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::debug!("Starting {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::debug!("Stopping {}", self.client_id);
        self.cancellation_token.cancel();
        for task in self.tasks.drain(..) {
            task.abort();
        }
        for (_, task) in self.funding_rate_tasks.drain() {
            task.abort();
        }
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {}", self.client_id);
        self.cancellation_token.cancel();
        for task in self.tasks.drain(..) {
            task.abort();
        }
        for (_, task) in self.funding_rate_tasks.drain() {
            task.abort();
        }
        self.funding_rate_cache.lock().unwrap().clear();
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {}", self.client_id);
        self.cancellation_token.cancel();
        for task in self.tasks.drain(..) {
            task.abort();
        }
        for (_, task) in self.funding_rate_tasks.drain() {
            task.abort();
        }
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            log::debug!("Already connected {}", self.client_id);
            return Ok(());
        }

        log::info!("Connecting {}", self.client_id);

        // Recreate token so a previous disconnect/stop doesn't block new operations
        self.cancellation_token = CancellationToken::new();

        if self.config.has_api_credentials() {
            let credential =
                Credential::resolve(self.config.api_key.clone(), self.config.api_secret.clone())
                    .context("API credentials not configured")?;

            let token = self
                .http_client
                .authenticate(credential.api_key(), credential.api_secret(), 86400)
                .await
                .context("Failed to authenticate with Ax")?;
            log::info!("Authenticated with Ax");
            self.ws_client.set_auth_token(token);
        }

        let instruments = self
            .http_client
            .request_instruments(None, None)
            .await
            .context("Failed to fetch instruments")?;

        for instrument in &instruments {
            self.ws_client.cache_instrument(instrument.clone());

            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to send instrument: {e}");
            }
        }
        self.http_client.cache_instruments(instruments);
        log::info!(
            "Cached {} instruments",
            self.http_client.get_cached_symbols().len()
        );

        self.ws_client
            .connect()
            .await
            .context("Failed to connect WebSocket")?;
        log::info!("WebSocket connected");
        self.spawn_message_handler();

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected {}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting {}", self.client_id);
        self.cancellation_token.cancel();
        self.ws_client.close().await;

        for task in self.tasks.drain(..) {
            task.abort();
        }
        for (_, task) in self.funding_rate_tasks.drain() {
            task.abort();
        }
        self.funding_rate_cache.lock().unwrap().clear();

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected {}", self.client_id);

        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        // AX does not have a real-time instruments channel; instruments are fetched via HTTP
        log::debug!("Instruments subscription not applicable for AX (use request_instruments)");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        // AX does not have a real-time instrument channel; instruments are fetched via HTTP
        log::debug!("Instrument subscription not applicable for AX (use request_instrument)");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        let level = Self::map_book_type_to_market_data_level(cmd.book_type);
        if cmd.book_type == BookType::L1_MBP {
            log::warn!(
                "Book type L1_MBP not supported by AX for deltas, downgrading {symbol} to LEVEL_2"
            );
        }
        log::debug!("Subscribing to book deltas for {symbol} at {level:?}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_book_deltas(&symbol, level)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe book deltas",
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Subscribing to quotes for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_quotes(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe quotes",
        );

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Subscribing to trades for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_trades(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe trades",
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = map_bar_spec_to_candle_width(&bar_type.spec())?;
        log::debug!("Subscribing to bars for {bar_type} (width: {width:?})");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.subscribe_candles(&symbol, width)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "subscribe bars",
        );

        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let poll_interval_mins = self
            .config
            .funding_rate_poll_interval_mins
            .unwrap_or(15)
            .max(1);

        // Use 7-day lookback to capture latest rate across weekends/holidays
        let lookback = ChronoDuration::days(7);

        let instrument_id = cmd.instrument_id;

        if self.funding_rate_tasks.contains_key(&instrument_id) {
            log::debug!("Already subscribed to funding rates for {instrument_id}");
            return Ok(());
        }

        log::debug!("Subscribing to funding rates for {instrument_id} (HTTP polling)");

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let symbol = instrument_id.symbol.inner();
        let cancel = self.cancellation_token.clone();
        let cache = Arc::clone(&self.funding_rate_cache);
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            // First tick fires immediately for initial emission
            let mut interval = tokio::time::interval(Duration::from_mins(poll_interval_mins));

            loop {
                tokio::select! {
                    () = cancel.cancelled() => {
                        log::debug!("Funding rate polling cancelled for {symbol}");
                        break;
                    }
                    _ = interval.tick() => {
                        let now: DateTime<Utc> = clock.get_time_ns().into();
                        let start = now - lookback;

                        match http.request_funding_rates(instrument_id, Some(start), Some(now)).await {
                            Ok(funding_rates) => {
                                if funding_rates.is_empty() {
                                    log::warn!(
                                        "No funding rates returned for {symbol}"
                                    );
                                } else if let Some(update) = funding_rates.last() {
                                    // Only emit if rate changed
                                    let should_emit = cache.lock().unwrap()
                                        .get(&instrument_id) != Some(update);

                                    if should_emit {
                                        log::info!(
                                            "Funding rate for {symbol}: {}",
                                            update.rate,
                                        );
                                        let update = *update;
                                        cache.lock().unwrap()
                                            .insert(instrument_id, update);

                                        if let Err(e) = sender.send(
                                            DataEvent::FundingRate(update),
                                        ) {
                                            log::error!(
                                                "Failed to send funding rate for {symbol}: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                            Err(e) => {
                                log::error!(
                                    "Failed to poll funding rates for {symbol}: {e}"
                                );
                            }
                        }
                    }
                }
            }
        });

        self.funding_rate_tasks.insert(instrument_id, handle);
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from book deltas for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_book_deltas(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe book deltas",
        );

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from quotes for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_quotes(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe quotes",
        );

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let symbol = cmd.instrument_id.symbol.to_string();
        log::debug!("Unsubscribing from trades for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_trades(&symbol)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe trades",
        );

        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let bar_type = cmd.bar_type;
        let symbol = bar_type.instrument_id().symbol.to_string();
        let width = map_bar_spec_to_candle_width(&bar_type.spec())?;
        log::debug!("Unsubscribing from bars for {bar_type}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move {
                ws.unsubscribe_candles(&symbol, width)
                    .await
                    .map_err(|e| anyhow::anyhow!(e))
            },
            "unsubscribe bars",
        );

        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if let Some(task) = self.funding_rate_tasks.remove(&instrument_id) {
            log::debug!("Unsubscribing from funding rates for {instrument_id}");
            task.abort();
            self.funding_rate_cache
                .lock()
                .unwrap()
                .remove(&instrument_id);
        } else {
            log::debug!("Not subscribed to funding rates for {instrument_id}");
        }

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let ws = self.ws_client.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let venue = *AX_VENUE;
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instruments(None, None).await {
                Ok(instruments) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::info!("Fetched {} instruments from Ax", instruments.len());
                    for inst in &instruments {
                        ws.cache_instrument(inst.clone());
                    }
                    http.cache_instruments(instruments.clone());

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
                    log::error!("Failed to request instruments: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let ws = self.ws_client.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let symbol = instrument_id.symbol.inner();
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_instrument(symbol, None, None).await {
                Ok(instrument) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::debug!("Fetched instrument {symbol} from Ax");
                    ws.cache_instrument(instrument.clone());
                    http.cache_instrument(instrument.clone());

                    let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        instrument,
                        start_nanos,
                        end_nanos,
                        clock.get_time_ns(),
                        params,
                    )));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send instrument response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request instrument {symbol}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let symbol = instrument_id.symbol.inner();
        let depth = request.depth.map(|n| n.get());
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_book_snapshot(symbol, depth).await {
                Ok(book) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::debug!(
                        "Fetched book snapshot for {symbol} ({} bids, {} asks)",
                        book.bids(None).count(),
                        book.asks(None).count(),
                    );

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
                Err(e) => {
                    log::error!("Failed to request book snapshot for {symbol}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: RequestTrades) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let symbol = instrument_id.symbol.inner();
        let limit = request.limit.map(|n| n.get() as i32);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http
                .request_trade_ticks(symbol, limit, start_nanos, end_nanos)
                .await
            {
                Ok(ticks) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::debug!("Fetched {} trades for {symbol}", ticks.len());

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
                    log::error!("Failed to request trades for {symbol}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_bars(&self, request: RequestBars) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let bar_type = request.bar_type;
        let symbol = bar_type.instrument_id().symbol.inner();
        let start = request.start;
        let end = request.end;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let params = request.params;
        let clock = self.clock;
        let width = match map_bar_spec_to_candle_width(&bar_type.spec()) {
            Ok(w) => w,
            Err(e) => {
                log::error!("Failed to map bar type {bar_type}: {e}");
                return Err(e);
            }
        };

        let cancel = self.cancellation_token.clone();

        get_runtime().spawn(async move {
            match http.request_bars(symbol, start, end, width).await {
                Ok(bars) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::debug!("Fetched {} bars for {symbol}", bars.len());

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
                Err(e) => {
                    log::error!("Failed to request bars for {symbol}: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_funding_rates(&self, request: RequestFundingRates) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let cancel = self.cancellation_token.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let instrument_id = request.instrument_id;
        let symbol = instrument_id.symbol.inner();
        let start = request.start;
        let end = request.end;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http.request_funding_rates(instrument_id, start, end).await {
                Ok(funding_rates) => {
                    if cancel.is_cancelled() {
                        return;
                    }
                    log::debug!("Fetched {} funding rates for {symbol}", funding_rates.len());

                    let ts_init = clock.get_time_ns();
                    let response = DataResponse::FundingRates(FundingRatesResponse::new(
                        request_id,
                        client_id,
                        instrument_id,
                        funding_rates,
                        start_nanos,
                        end_nanos,
                        ts_init,
                        params,
                    ));

                    if let Err(e) = sender.send(DataEvent::Response(response)) {
                        log::error!("Failed to send funding rates response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request funding rates for {symbol}: {e}");
                }
            }
        });

        Ok(())
    }
}
