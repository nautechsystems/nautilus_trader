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

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Duration as ChronoDuration, Utc};
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
            SubscribeBookDeltas, SubscribeFundingRates, SubscribeIndexPrices, SubscribeInstrument,
            SubscribeInstrumentClose, SubscribeInstrumentStatus, SubscribeInstruments,
            SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeFundingRates, UnsubscribeIndexPrices,
            UnsubscribeInstrument, UnsubscribeInstrumentClose, UnsubscribeInstrumentStatus,
            UnsubscribeInstruments, UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    AtomicMap, MUTEX_POISONED,
    datetime::datetime_to_unix_nanos,
    nanos::UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, FundingRateUpdate, InstrumentStatus, MarkPriceUpdate, OrderBookDeltas_API},
    enums::{BookType, MarketStatusAction},
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::Price,
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{AX_AUTH_TOKEN_TTL_DATA_SECS, AX_FUNDING_RATE_LOOKBACK_DAYS, AX_VENUE},
        credential::Credential,
        enums::{AxCandleWidth, AxInstrumentState, AxMarketDataLevel},
        parse::{ax_timestamp_stn_to_unix_nanos, map_bar_spec_to_candle_width},
    },
    config::AxDataClientConfig,
    http::client::AxHttpClient,
    websocket::{
        data::{
            client::{AxMdWebSocketClient, AxWsClientError, SymbolDataTypes},
            parse::{
                parse_book_l1_quote, parse_book_l2_deltas, parse_book_l3_deltas, parse_candle_bar,
                parse_trade_tick,
            },
        },
        messages::{AxDataWsMessage, AxMdCandle, AxMdMessage},
    },
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
    instruments: Arc<AtomicMap<Ustr, InstrumentAny>>,
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
    pub fn instruments(&self) -> &Arc<AtomicMap<Ustr, InstrumentAny>> {
        &self.instruments
    }

    /// Spawns a message handler task to forward WebSocket data to the DataEngine.
    fn spawn_message_handler(&mut self) {
        let stream = self.ws_client.stream();
        let data_sender = self.data_sender.clone();
        let cancellation_token = self.cancellation_token.clone();
        let is_connected = Arc::clone(&self.is_connected);
        let instruments = Arc::clone(&self.instruments);
        let symbol_data_types = self.ws_client.symbol_data_types();
        let status_invalidations = self.ws_client.status_invalidations();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            let mut book_sequences: AHashMap<Ustr, u64> = AHashMap::new();
            let mut candle_cache: AHashMap<(Ustr, AxCandleWidth), AxMdCandle> = AHashMap::new();
            let mut instrument_states: AHashMap<Ustr, AxInstrumentState> = AHashMap::new();

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Message handler cancelled");
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                drain_status_invalidations(
                                    &status_invalidations,
                                    &mut instrument_states,
                                );

                                handle_ws_message(
                                    ws_msg,
                                    &data_sender,
                                    &instruments,
                                    &symbol_data_types,
                                    &mut book_sequences,
                                    &mut candle_cache,
                                    &mut instrument_states,
                                    clock,
                                );
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

    fn spawn_instrument_refresh(&mut self) {
        let minutes = self.config.update_instruments_interval_mins;
        if minutes == 0 {
            return;
        }

        let interval = Duration::from_secs(minutes.saturating_mul(60));
        let cancellation = self.cancellation_token.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let http_client = self.http_client.clone();
        let data_sender = self.data_sender.clone();
        let client_id = self.client_id;

        let handle = get_runtime().spawn(async move {
            loop {
                let sleep = tokio::time::sleep(interval);
                tokio::pin!(sleep);
                tokio::select! {
                    () = cancellation.cancelled() => {
                        log::debug!("Instrument refresh task cancelled");
                        break;
                    }
                    () = &mut sleep => {
                        match http_client.request_instruments(None, None).await {
                            Ok(instruments) => {
                                for inst in &instruments {
                                    instruments_cache.insert(inst.symbol().inner(), inst.clone());

                                    if let Err(e) = data_sender
                                        .send(DataEvent::Instrument(inst.clone()))
                                    {
                                        log::warn!("Failed to send refreshed instrument: {e}");
                                    }
                                }
                                http_client.cache_instruments(&instruments);
                                log::debug!(
                                    "Instruments refreshed: client_id={client_id}, count={}",
                                    instruments.len(),
                                );
                            }
                            Err(e) => {
                                log::warn!("Failed to refresh instruments: client_id={client_id}, error={e:?}");
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(handle);
    }

    #[expect(
        clippy::unnecessary_wraps,
        reason = "callers forward Result to trait methods"
    )]
    fn ws_symbol_op<F, Fut>(
        &mut self,
        instrument_id: InstrumentId,
        op: F,
        context: &'static str,
    ) -> anyhow::Result<()>
    where
        F: FnOnce(AxMdWebSocketClient, String) -> Fut + Send + 'static,
        Fut: Future<Output = Result<(), AxWsClientError>> + Send,
    {
        let symbol = instrument_id.symbol.to_string();
        log::debug!("{context} for {symbol}");

        let ws = self.ws_client.clone();
        self.spawn_ws(
            async move { op(ws, symbol).await.map_err(|e| anyhow::anyhow!(e)) },
            context,
        );

        Ok(())
    }

    fn spawn_ws<F>(&mut self, fut: F, context: &'static str)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{context}: {e:?}");
            }
        });

        self.tasks.retain(|h| !h.is_finished());
        self.tasks.push(handle);
    }

    fn abort_all_tasks(&mut self) {
        self.cancellation_token.cancel();

        for task in self.tasks.drain(..) {
            task.abort();
        }

        for (_, task) in self.funding_rate_tasks.drain() {
            task.abort();
        }
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
        self.abort_all_tasks();
        self.is_connected.store(false, Ordering::Release);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::debug!("Resetting {}", self.client_id);
        self.abort_all_tasks();
        self.funding_rate_cache
            .lock()
            .expect(MUTEX_POISONED)
            .clear();
        self.cancellation_token = CancellationToken::new();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::debug!("Disposing {}", self.client_id);
        self.abort_all_tasks();
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
                .authenticate(
                    credential.api_key(),
                    credential.api_secret(),
                    AX_AUTH_TOKEN_TTL_DATA_SECS,
                )
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
            self.instruments
                .insert(instrument.symbol().inner(), instrument.clone());

            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to send instrument: {e}");
            }
        }
        self.http_client.cache_instruments(&instruments);
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
        self.spawn_instrument_refresh();

        self.is_connected.store(true, Ordering::Release);
        log::info!("Connected {}", self.client_id);

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting {}", self.client_id);
        self.ws_client.close().await;
        self.abort_all_tasks();
        self.funding_rate_cache
            .lock()
            .expect(MUTEX_POISONED)
            .clear();

        self.is_connected.store(false, Ordering::Release);
        log::info!("Disconnected {}", self.client_id);

        Ok(())
    }

    fn subscribe_instruments(&mut self, _cmd: SubscribeInstruments) -> anyhow::Result<()> {
        // AX does not have a real-time instruments channel; instruments are fetched via HTTP
        log::debug!("Instruments subscription not applicable for AX (use request_instruments)");
        Ok(())
    }

    fn subscribe_instrument(&mut self, _cmd: SubscribeInstrument) -> anyhow::Result<()> {
        // AX does not have a real-time instrument channel; instruments are fetched via HTTP
        log::debug!("Instrument subscription not applicable for AX (use request_instrument)");
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: SubscribeBookDeltas) -> anyhow::Result<()> {
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

    fn subscribe_quotes(&mut self, cmd: SubscribeQuotes) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.subscribe_quotes(&s).await },
            "Subscribing to quotes",
        )
    }

    fn subscribe_trades(&mut self, cmd: SubscribeTrades) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.subscribe_trades(&s).await },
            "Subscribing to trades",
        )
    }

    fn subscribe_mark_prices(&mut self, cmd: SubscribeMarkPrices) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.subscribe_mark_prices(&s).await },
            "Subscribing to mark prices",
        )
    }

    fn subscribe_index_prices(&mut self, _cmd: SubscribeIndexPrices) -> anyhow::Result<()> {
        log::warn!("Index prices not supported by AX Exchange");
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: SubscribeBars) -> anyhow::Result<()> {
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

    fn subscribe_funding_rates(&mut self, cmd: SubscribeFundingRates) -> anyhow::Result<()> {
        let poll_interval_mins = self.config.funding_rate_poll_interval_mins.max(1);

        // Use 7-day lookback to capture latest rate across weekends/holidays
        let lookback = ChronoDuration::days(AX_FUNDING_RATE_LOOKBACK_DAYS);

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
                                    let should_emit = cache.lock().expect(MUTEX_POISONED)
                                        .get(&instrument_id) != Some(update);

                                    if should_emit {
                                        log::info!(
                                            "Funding rate for {symbol}: {}",
                                            update.rate,
                                        );
                                        let update = *update;
                                        cache.lock().expect(MUTEX_POISONED)
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

    fn subscribe_instrument_status(
        &mut self,
        cmd: SubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.subscribe_instrument_status(&s).await },
            "Subscribing to instrument status",
        )
    }

    fn subscribe_instrument_close(&mut self, _cmd: SubscribeInstrumentClose) -> anyhow::Result<()> {
        log::warn!("Instrument close not supported by AX Exchange");
        Ok(())
    }

    fn unsubscribe_instruments(&mut self, _cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_instrument(&mut self, _cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.unsubscribe_book_deltas(&s).await },
            "Unsubscribing from book deltas",
        )
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.unsubscribe_quotes(&s).await },
            "Unsubscribing from quotes",
        )
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.unsubscribe_trades(&s).await },
            "Unsubscribing from trades",
        )
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.unsubscribe_mark_prices(&s).await },
            "Unsubscribing from mark prices",
        )
    }

    fn unsubscribe_index_prices(&mut self, _cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
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
                .expect(MUTEX_POISONED)
                .remove(&instrument_id);
        } else {
            log::debug!("Not subscribed to funding rates for {instrument_id}");
        }

        Ok(())
    }

    fn unsubscribe_instrument_status(
        &mut self,
        cmd: &UnsubscribeInstrumentStatus,
    ) -> anyhow::Result<()> {
        self.ws_symbol_op(
            cmd.instrument_id,
            |ws, s| async move { ws.unsubscribe_instrument_status(&s).await },
            "Unsubscribing from instrument status",
        )
    }

    fn unsubscribe_instrument_close(
        &mut self,
        _cmd: &UnsubscribeInstrumentClose,
    ) -> anyhow::Result<()> {
        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
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
                        instruments_cache.insert(inst.symbol().inner(), inst.clone());
                    }
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
                Err(e) => {
                    log::error!("Failed to request instruments: {e}");
                }
            }
        });

        Ok(())
    }

    fn request_instrument(&self, request: RequestInstrument) -> anyhow::Result<()> {
        let http = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
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
                    instruments_cache.insert(symbol, instrument.clone());
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

fn drain_status_invalidations(
    invalidations: &Arc<Mutex<AHashSet<Ustr>>>,
    instrument_states: &mut AHashMap<Ustr, AxInstrumentState>,
) {
    if let Ok(mut set) = invalidations.lock() {
        for symbol in set.drain() {
            instrument_states.remove(&symbol);
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_ws_message(
    msg: AxDataWsMessage,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: &Arc<AtomicMap<Ustr, InstrumentAny>>,
    symbol_data_types: &Arc<AtomicMap<String, SymbolDataTypes>>,
    book_sequences: &mut AHashMap<Ustr, u64>,
    candle_cache: &mut AHashMap<(Ustr, AxCandleWidth), AxMdCandle>,
    instrument_states: &mut AHashMap<Ustr, AxInstrumentState>,
    clock: &'static AtomicTime,
) {
    match msg {
        AxDataWsMessage::Reconnected => {
            candle_cache.clear();
            instrument_states.clear();
            log::info!("WebSocket reconnected");
        }
        AxDataWsMessage::CandleUnsubscribed { symbol, width } => {
            candle_cache.remove(&(symbol, width));
        }
        AxDataWsMessage::MdMessage(md_msg) => {
            handle_md_message(
                md_msg,
                sender,
                instruments,
                symbol_data_types,
                book_sequences,
                candle_cache,
                instrument_states,
                clock,
            );
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn handle_md_message(
    message: AxMdMessage,
    sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: &Arc<AtomicMap<Ustr, InstrumentAny>>,
    symbol_data_types: &Arc<AtomicMap<String, SymbolDataTypes>>,
    book_sequences: &mut AHashMap<Ustr, u64>,
    candle_cache: &mut AHashMap<(Ustr, AxCandleWidth), AxMdCandle>,
    instrument_states: &mut AHashMap<Ustr, AxInstrumentState>,
    clock: &'static AtomicTime,
) {
    let ts_init = || -> UnixNanos { clock.get_time_ns() };

    let instruments_snap = instruments.load();
    let sdt_snap = symbol_data_types.load();

    match message {
        AxMdMessage::BookL1(book) => {
            let l1_subscribed = sdt_snap
                .get(book.s.as_str())
                .is_some_and(|e| e.quotes || e.book_level == Some(AxMarketDataLevel::Level1));

            if !l1_subscribed {
                return;
            }

            let Some(instrument) = instruments_snap.get(&book.s) else {
                log::error!(
                    "No instrument cached for symbol '{}' - cannot parse L1 book",
                    book.s
                );
                return;
            };

            match parse_book_l1_quote(&book, instrument, ts_init()) {
                Ok(quote) => {
                    let _ = sender.send(DataEvent::Data(Data::Quote(quote)));
                }
                Err(e) => log::error!("Failed to parse L1 to QuoteTick: {e}"),
            }
        }
        AxMdMessage::BookL2(book) => {
            let symbol = book.s;
            let seq = book_sequences.entry(symbol).or_insert(0);
            *seq += 1;
            let sequence = *seq;

            let Some(instrument) = instruments_snap.get(&symbol) else {
                log::error!("No instrument cached for symbol '{symbol}' - cannot parse L2 book");
                return;
            };

            match parse_book_l2_deltas(&book, instrument, sequence, ts_init()) {
                Ok(deltas) => {
                    let api_deltas = OrderBookDeltas_API::new(deltas);
                    let _ = sender.send(DataEvent::Data(Data::Deltas(api_deltas)));
                }
                Err(e) => log::error!("Failed to parse L2 to OrderBookDeltas: {e}"),
            }
        }
        AxMdMessage::BookL3(book) => {
            let symbol = book.s;
            let seq = book_sequences.entry(symbol).or_insert(0);
            *seq += 1;
            let sequence = *seq;

            let Some(instrument) = instruments_snap.get(&symbol) else {
                log::error!("No instrument cached for symbol '{symbol}' - cannot parse L3 book");
                return;
            };

            match parse_book_l3_deltas(&book, instrument, sequence, ts_init()) {
                Ok(deltas) => {
                    let api_deltas = OrderBookDeltas_API::new(deltas);
                    let _ = sender.send(DataEvent::Data(Data::Deltas(api_deltas)));
                }
                Err(e) => log::error!("Failed to parse L3 to OrderBookDeltas: {e}"),
            }
        }
        AxMdMessage::Ticker(ticker) => {
            let Some(instrument) = instruments_snap.get(&ticker.s) else {
                log::debug!("No instrument cached for ticker symbol '{}'", ticker.s);
                return;
            };

            let instrument_id = instrument.id();
            let price_precision = instrument.price_precision();
            let ts_event =
                ax_timestamp_stn_to_unix_nanos(ticker.ts, ticker.tn).unwrap_or_else(|_| ts_init());
            let ts_init = ts_init();

            let mark_prices_subscribed = sdt_snap
                .get(ticker.s.as_str())
                .is_some_and(|e| e.mark_prices);
            if mark_prices_subscribed && let Some(mark_price) = ticker.m {
                match Price::from_decimal_dp(mark_price, price_precision) {
                    Ok(price) => {
                        let update = MarkPriceUpdate::new(instrument_id, price, ts_event, ts_init);
                        let _ = sender.send(DataEvent::Data(Data::MarkPriceUpdate(update)));
                    }
                    Err(e) => {
                        log::error!("Failed to parse mark price for {}: {e}", ticker.s);
                    }
                }
            }

            if let Some(state) = ticker.i {
                let status_subscribed = sdt_snap
                    .get(ticker.s.as_str())
                    .is_some_and(|e| e.instrument_status);
                if status_subscribed {
                    let prev = instrument_states.insert(ticker.s, state);
                    if prev != Some(state) {
                        let action = MarketStatusAction::from(state);
                        let status = InstrumentStatus::new(
                            instrument_id,
                            action,
                            ts_event,
                            ts_init,
                            None,
                            None,
                            Some(state == AxInstrumentState::Open),
                            None,
                            None,
                        );
                        let _ = sender.send(DataEvent::InstrumentStatus(status));
                    }
                }
            }
        }
        AxMdMessage::Trade(trade) => {
            let trades_subscribed = sdt_snap.get(trade.s.as_str()).is_some_and(|e| e.trades);

            if !trades_subscribed {
                return;
            }

            let Some(instrument) = instruments_snap.get(&trade.s) else {
                log::error!(
                    "No instrument cached for symbol '{}' - cannot parse trade",
                    trade.s
                );
                return;
            };

            match parse_trade_tick(&trade, instrument, ts_init()) {
                Ok(tick) => {
                    let _ = sender.send(DataEvent::Data(Data::Trade(tick)));
                }
                Err(e) => log::error!("Failed to parse trade to TradeTick: {e}"),
            }
        }
        AxMdMessage::Candle(candle) => {
            let cache_key = (candle.symbol, candle.width);

            let closed_candle = if let Some(cached) = candle_cache.get(&cache_key) {
                if cached.ts == candle.ts {
                    None
                } else {
                    Some(cached.clone())
                }
            } else {
                None
            };

            candle_cache.insert(cache_key, candle);

            if let Some(closed) = closed_candle {
                let Some(instrument) = instruments_snap.get(&closed.symbol) else {
                    log::error!(
                        "No instrument cached for symbol '{}' - cannot parse candle",
                        closed.symbol
                    );
                    return;
                };

                match parse_candle_bar(&closed, instrument, ts_init()) {
                    Ok(bar) => {
                        let _ = sender.send(DataEvent::Data(Data::Bar(bar)));
                    }
                    Err(e) => log::error!("Failed to parse candle to Bar: {e}"),
                }
            }
        }
        AxMdMessage::Heartbeat(_) => {
            log::trace!("Received heartbeat");
        }
        AxMdMessage::SubscriptionResponse(_) => {}
        AxMdMessage::Error(error) => {
            log::error!("WebSocket error: {}", error.message);
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, Mutex};

    use ahash::{AHashMap, AHashSet};
    use nautilus_model::{
        data::InstrumentStatus,
        enums::AssetClass,
        identifiers::{InstrumentId, Symbol},
        instruments::PerpetualContract,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;
    use crate::websocket::{
        data::client::SymbolDataTypes,
        messages::{AxMdMessage, AxMdTicker},
    };

    #[rstest]
    fn test_drain_status_invalidations_removes_cached_state() {
        let invalidations = Arc::new(Mutex::new(AHashSet::new()));
        let mut states = AHashMap::new();
        let sym = Ustr::from("EURUSD-PERP");

        states.insert(sym, AxInstrumentState::Open);
        invalidations.lock().unwrap().insert(sym);

        drain_status_invalidations(&invalidations, &mut states);

        assert!(!states.contains_key(&sym));
        assert!(invalidations.lock().unwrap().is_empty());
    }

    #[rstest]
    fn test_drain_status_invalidations_no_op_when_empty() {
        let invalidations = Arc::new(Mutex::new(AHashSet::new()));
        let mut states = AHashMap::new();
        let sym = Ustr::from("EURUSD-PERP");
        states.insert(sym, AxInstrumentState::Open);

        drain_status_invalidations(&invalidations, &mut states);

        assert!(states.contains_key(&sym));
    }

    fn ticker_test_instrument() -> InstrumentAny {
        let symbol = Symbol::new("EURUSD-PERP");
        let instrument = PerpetualContract::new(
            InstrumentId::new(symbol, *crate::common::consts::AX_VENUE),
            symbol,
            Ustr::from("EURUSD"),
            AssetClass::FX,
            None,
            Currency::USD(),
            Currency::USD(),
            false,
            4,
            0,
            Price::from("0.0001"),
            Quantity::from("1"),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some(Decimal::new(1, 2)),
            Some(Decimal::new(5, 3)),
            Some(Decimal::new(2, 4)),
            Some(Decimal::new(5, 4)),
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        );
        InstrumentAny::PerpetualContract(instrument)
    }

    fn ticker_message(state: AxInstrumentState) -> AxMdTicker {
        AxMdTicker {
            ts: 1_700_000_000,
            tn: 0,
            s: Ustr::from("EURUSD-PERP"),
            p: rust_decimal::Decimal::ZERO,
            q: 0,
            o: rust_decimal::Decimal::ZERO,
            l: rust_decimal::Decimal::ZERO,
            h: rust_decimal::Decimal::ZERO,
            v: 0,
            oi: None,
            m: None,
            i: Some(state),
            pl: None,
            pu: None,
            lsp: None,
        }
    }

    fn collect_instrument_statuses(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    ) -> Vec<InstrumentStatus> {
        let mut statuses = Vec::new();

        while let Ok(event) = rx.try_recv() {
            if let DataEvent::InstrumentStatus(status) = event {
                statuses.push(status);
            }
        }
        statuses
    }

    #[rstest]
    fn test_ticker_instrument_status_emitted_once_when_state_unchanged() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(Ustr::from("EURUSD-PERP"), ticker_test_instrument());

        let sdt = Arc::new(AtomicMap::new());
        sdt.insert(
            "EURUSD-PERP".to_string(),
            SymbolDataTypes {
                quotes: false,
                trades: false,
                mark_prices: false,
                instrument_status: true,
                book_level: None,
            },
        );

        let mut book_sequences = AHashMap::new();
        let mut candle_cache = AHashMap::new();
        let mut instrument_states = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        let msg = AxMdMessage::Ticker(ticker_message(AxInstrumentState::Open));
        handle_md_message(
            msg.clone(),
            &tx,
            &instruments,
            &sdt,
            &mut book_sequences,
            &mut candle_cache,
            &mut instrument_states,
            clock,
        );

        // Same state repeated: second call should not emit a second InstrumentStatus
        handle_md_message(
            msg,
            &tx,
            &instruments,
            &sdt,
            &mut book_sequences,
            &mut candle_cache,
            &mut instrument_states,
            clock,
        );

        let statuses = collect_instrument_statuses(&mut rx);
        assert_eq!(
            statuses.len(),
            1,
            "expected a single emission, found {statuses:?}"
        );
        assert_eq!(statuses[0].is_trading, Some(true));
    }

    #[rstest]
    fn test_ticker_instrument_status_emitted_on_transition() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(Ustr::from("EURUSD-PERP"), ticker_test_instrument());

        let sdt = Arc::new(AtomicMap::new());
        sdt.insert(
            "EURUSD-PERP".to_string(),
            SymbolDataTypes {
                quotes: false,
                trades: false,
                mark_prices: false,
                instrument_status: true,
                book_level: None,
            },
        );

        let mut book_sequences = AHashMap::new();
        let mut candle_cache = AHashMap::new();
        let mut instrument_states = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        handle_md_message(
            AxMdMessage::Ticker(ticker_message(AxInstrumentState::Open)),
            &tx,
            &instruments,
            &sdt,
            &mut book_sequences,
            &mut candle_cache,
            &mut instrument_states,
            clock,
        );
        handle_md_message(
            AxMdMessage::Ticker(ticker_message(AxInstrumentState::Closed)),
            &tx,
            &instruments,
            &sdt,
            &mut book_sequences,
            &mut candle_cache,
            &mut instrument_states,
            clock,
        );

        let statuses = collect_instrument_statuses(&mut rx);
        assert_eq!(statuses.len(), 2, "expected one emission per transition");
        assert_eq!(statuses[0].is_trading, Some(true));
        assert_eq!(statuses[1].is_trading, Some(false));
    }

    #[rstest]
    fn test_ticker_instrument_status_skipped_when_not_subscribed() {
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let instruments = Arc::new(AtomicMap::new());
        instruments.insert(Ustr::from("EURUSD-PERP"), ticker_test_instrument());

        let sdt = Arc::new(AtomicMap::new());
        sdt.insert(
            "EURUSD-PERP".to_string(),
            SymbolDataTypes {
                quotes: false,
                trades: false,
                mark_prices: false,
                instrument_status: false,
                book_level: None,
            },
        );

        let mut book_sequences = AHashMap::new();
        let mut candle_cache = AHashMap::new();
        let mut instrument_states = AHashMap::new();
        let clock = get_atomic_clock_realtime();

        handle_md_message(
            AxMdMessage::Ticker(ticker_message(AxInstrumentState::Open)),
            &tx,
            &instruments,
            &sdt,
            &mut book_sequences,
            &mut candle_cache,
            &mut instrument_states,
            clock,
        );

        let statuses = collect_instrument_statuses(&mut rx);
        assert!(statuses.is_empty());
    }
}
