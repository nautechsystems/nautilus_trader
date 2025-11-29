// -------------------------------------------------------------------------------------------------
//  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use chrono::{DateTime, Utc};
use nautilus_common::{
    live::runner::get_data_event_sender,
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots,
            UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::{Bar, BarType, Data, OrderBookDeltas_API},
    enums::BarAggregation,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    types::{Price, Quantity},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{HyperliquidProductType, consts::HYPERLIQUID_VENUE, parse::bar_type_to_interval},
    config::HyperliquidDataClientConfig,
    http::{client::HyperliquidHttpClient, models::HyperliquidCandle},
    websocket::{
        client::HyperliquidWebSocketClient,
        messages::{HyperliquidWsMessage, NautilusWsMessage},
        parse::{
            parse_ws_candle, parse_ws_order_book_deltas, parse_ws_quote_tick, parse_ws_trade_tick,
        },
    },
};

#[derive(Debug)]
pub struct HyperliquidDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: HyperliquidDataClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: HyperliquidWebSocketClient,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    /// Maps coin symbols (e.g., "BTC") to instrument IDs (e.g., "BTC-PERP")
    /// for efficient O(1) lookup in WebSocket message handlers
    coin_to_instrument_id: Arc<RwLock<AHashMap<Ustr, InstrumentId>>>,
    clock: &'static AtomicTime,
    #[allow(dead_code)]
    instrument_refresh_active: bool,
}

impl HyperliquidDataClient {
    /// Creates a new [`HyperliquidDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize.
    pub fn new(client_id: ClientId, config: HyperliquidDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if let Some(private_key_str) = &config.private_key {
            let secrets = crate::common::credential::Secrets {
                private_key: crate::common::credential::EvmPrivateKey::new(
                    private_key_str.clone(),
                )?,
                is_testnet: config.is_testnet,
                vault_address: None,
            };
            HyperliquidHttpClient::with_credentials(
                &secrets,
                config.http_timeout_secs,
                config.http_proxy_url.clone(),
            )?
        } else {
            HyperliquidHttpClient::new(
                config.is_testnet,
                config.http_timeout_secs,
                config.http_proxy_url.clone(),
            )?
        };

        // Note: Rust data client is not the primary interface; Python adapter is used instead.
        // Defaulting to Perp for basic functionality.
        let ws_client = HyperliquidWebSocketClient::new(
            None,
            config.is_testnet,
            HyperliquidProductType::Perp,
            None,
        );

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            coin_to_instrument_id: Arc::new(RwLock::new(AHashMap::new())),
            clock,
            instrument_refresh_active: false,
        })
    }

    fn venue(&self) -> Venue {
        *HYPERLIQUID_VENUE
    }

    async fn bootstrap_instruments(&mut self) -> anyhow::Result<Vec<InstrumentAny>> {
        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to fetch instruments during bootstrap")?;

        let mut instruments_map = self.instruments.write().unwrap();
        let mut coin_map = self.coin_to_instrument_id.write().unwrap();

        for instrument in &instruments {
            let instrument_id = instrument.id();
            instruments_map.insert(instrument_id, instrument.clone());

            // Build coin-to-instrument-id index for efficient WebSocket message lookup
            // Use raw_symbol which contains Hyperliquid's coin ticker (e.g., "BTC")
            let coin = instrument.raw_symbol().inner();
            if instrument_id.symbol.as_str().starts_with("BTCUSD") {
                log::warn!(
                    "DEBUG bootstrap BTCUSD: instrument_id={}, raw_symbol={}, coin={}",
                    instrument_id,
                    instrument.raw_symbol(),
                    coin
                );
            }
            coin_map.insert(coin, instrument_id);

            self.ws_client.cache_instrument(instrument.clone());
        }

        tracing::info!(
            "Bootstrapped {} instruments with {} coin mappings",
            instruments_map.len(),
            coin_map.len()
        );
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> anyhow::Result<()> {
        // Clone client before connecting so the clone can have out_rx set
        let mut ws_client = self.ws_client.clone();

        ws_client
            .connect()
            .await
            .context("failed to connect to Hyperliquid WebSocket")?;

        let _data_sender = self.data_sender.clone();
        let _instruments = Arc::clone(&self.instruments);
        let _coin_to_instrument_id = Arc::clone(&self.coin_to_instrument_id);
        let _venue = self.venue();
        let _clock = self.clock;
        let cancellation_token = self.cancellation_token.clone();

        let task = tokio::spawn(async move {
            tracing::info!("Hyperliquid WebSocket consumption loop started");

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("WebSocket consumption loop cancelled");
                        break;
                    }
                    msg_opt = ws_client.next_event() => {
                        if let Some(msg) = msg_opt {
                            match msg {
                                // Handled by python/websocket.rs
                                NautilusWsMessage::Trades(_)
                                | NautilusWsMessage::Quote(_)
                                | NautilusWsMessage::Deltas(_)
                                | NautilusWsMessage::Candle(_)
                                | NautilusWsMessage::MarkPrice(_)
                                | NautilusWsMessage::IndexPrice(_)
                                | NautilusWsMessage::FundingRate(_) => {}
                                NautilusWsMessage::Reconnected => {
                                    tracing::info!("WebSocket reconnected");
                                }
                                NautilusWsMessage::Error(e) => {
                                    tracing::error!("WebSocket error: {e}");
                                }
                                NautilusWsMessage::ExecutionReports(_) => {
                                    // Handled by execution client
                                }
                            }
                        } else {
                            // Connection closed or error
                            tracing::warn!("WebSocket next_event returned None, connection may be closed");
                            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
                        }
                    }
                }
            }

            tracing::info!("Hyperliquid WebSocket consumption loop finished");
        });

        self.tasks.push(task);
        tracing::info!("WebSocket consumption task spawned");

        Ok(())
    }

    #[allow(dead_code)]
    fn handle_ws_message(
        msg: HyperliquidWsMessage,
        ws_client: &HyperliquidWebSocketClient,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
        coin_to_instrument_id: &Arc<RwLock<AHashMap<Ustr, InstrumentId>>>,
        _venue: Venue,
        clock: &'static AtomicTime,
    ) {
        match msg {
            HyperliquidWsMessage::Bbo { data } => {
                let coin = data.coin;
                tracing::debug!("Received BBO message for coin: {coin}");

                // Use efficient O(1) lookup instead of iterating through all instruments
                // Hyperliquid WebSocket sends coin="BTC", lookup returns "BTC-PERP" instrument ID
                let coin_map = coin_to_instrument_id.read().unwrap();
                let instrument_id = coin_map.get(&data.coin);

                if let Some(&instrument_id) = instrument_id {
                    let instruments_map = instruments.read().unwrap();
                    if let Some(instrument) = instruments_map.get(&instrument_id) {
                        let ts_init = clock.get_time_ns();

                        match parse_ws_quote_tick(&data, instrument, ts_init) {
                            Ok(quote_tick) => {
                                tracing::debug!(
                                    "Parsed quote tick for {}: bid={}, ask={}",
                                    data.coin,
                                    quote_tick.bid_price,
                                    quote_tick.ask_price
                                );
                                if let Err(e) =
                                    data_sender.send(DataEvent::Data(Data::Quote(quote_tick)))
                                {
                                    tracing::error!("Failed to send quote tick: {e}");
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to parse quote tick for {}: {e}",
                                    data.coin
                                );
                            }
                        }
                    }
                } else {
                    tracing::warn!(
                        "Received BBO for unknown coin: {} (no matching instrument found)",
                        data.coin
                    );
                }
            }
            HyperliquidWsMessage::Trades { data } => {
                let count = data.len();
                tracing::debug!("Received {count} trade(s)");

                // Process each trade in the batch
                for trade_data in data {
                    let coin = trade_data.coin;
                    let coin_map = coin_to_instrument_id.read().unwrap();

                    if let Some(&instrument_id) = coin_map.get(&coin) {
                        let instruments_map = instruments.read().unwrap();
                        if let Some(instrument) = instruments_map.get(&instrument_id) {
                            let ts_init = clock.get_time_ns();

                            match parse_ws_trade_tick(&trade_data, instrument, ts_init) {
                                Ok(trade_tick) => {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Trade(trade_tick)))
                                    {
                                        tracing::error!("Failed to send trade tick: {e}");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse trade tick for {coin}: {e}");
                                }
                            }
                        }
                    } else {
                        tracing::warn!("Received trade for unknown coin: {coin}");
                    }
                }
            }
            HyperliquidWsMessage::L2Book { data } => {
                let coin = data.coin;
                tracing::debug!("Received L2 book update for coin: {coin}");

                let coin_map = coin_to_instrument_id.read().unwrap();
                if let Some(&instrument_id) = coin_map.get(&data.coin) {
                    let instruments_map = instruments.read().unwrap();
                    if let Some(instrument) = instruments_map.get(&instrument_id) {
                        let ts_init = clock.get_time_ns();

                        match parse_ws_order_book_deltas(&data, instrument, ts_init) {
                            Ok(deltas) => {
                                if let Err(e) = data_sender.send(DataEvent::Data(Data::Deltas(
                                    OrderBookDeltas_API::new(deltas),
                                ))) {
                                    tracing::error!("Failed to send order book deltas: {e}");
                                }
                            }
                            Err(e) => {
                                tracing::error!(
                                    "Failed to parse order book deltas for {}: {e}",
                                    data.coin
                                );
                            }
                        }
                    }
                } else {
                    tracing::warn!("Received L2 book for unknown coin: {coin}");
                }
            }
            HyperliquidWsMessage::Candle { data } => {
                let coin = &data.s;
                let interval = &data.i;
                tracing::debug!("Received candle for {coin}:{interval}");

                if let Some(bar_type) = ws_client.get_bar_type(&data.s, &data.i) {
                    let coin = Ustr::from(&data.s);
                    let coin_map = coin_to_instrument_id.read().unwrap();

                    if let Some(&instrument_id) = coin_map.get(&coin) {
                        let instruments_map = instruments.read().unwrap();
                        if let Some(instrument) = instruments_map.get(&instrument_id) {
                            let ts_init = clock.get_time_ns();

                            match parse_ws_candle(&data, instrument, &bar_type, ts_init) {
                                Ok(bar) => {
                                    if let Err(e) =
                                        data_sender.send(DataEvent::Data(Data::Bar(bar)))
                                    {
                                        tracing::error!("Failed to send bar data: {e}");
                                    }
                                }
                                Err(e) => {
                                    tracing::error!("Failed to parse candle for {coin}: {e}");
                                }
                            }
                        }
                    } else {
                        tracing::warn!("Received candle for unknown coin: {coin}");
                    }
                } else {
                    tracing::debug!("Received candle for {coin}:{interval} but no BarType tracked");
                }
            }
            _ => {
                // Log other message types for debugging
                tracing::trace!("Received unhandled WebSocket message: {:?}", msg);
            }
        }
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> anyhow::Result<InstrumentAny> {
        let instruments = self.instruments.read().unwrap();
        instruments
            .get(instrument_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Instrument {instrument_id} not found"))
    }
}

fn datetime_to_unix_nanos(value: Option<DateTime<Utc>>) -> Option<UnixNanos> {
    value
        .and_then(|dt| dt.timestamp_nanos_opt())
        .and_then(|nanos| u64::try_from(nanos).ok())
        .map(UnixNanos::from)
}

impl HyperliquidDataClient {
    #[allow(dead_code)]
    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            tracing::error!("Failed to emit data event: {e}");
        }
    }
}

#[async_trait::async_trait(?Send)]
impl DataClient for HyperliquidDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            client_id = %self.client_id,
            is_testnet = self.config.is_testnet,
            http_proxy_url = ?self.config.http_proxy_url,
            ws_proxy_url = ?self.config.ws_proxy_url,
            "Starting Hyperliquid data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping Hyperliquid data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Resetting Hyperliquid data client {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Disposing Hyperliquid data client {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        // Bootstrap instruments from HTTP API
        let _instruments = self
            .bootstrap_instruments()
            .await
            .context("failed to bootstrap instruments")?;

        // Connect WebSocket client
        self.spawn_ws()
            .await
            .context("failed to spawn WebSocket client")?;

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!(client_id = %self.client_id, "Connected");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        // Cancel all tasks
        self.cancellation_token.cancel();

        // Wait for all tasks to complete
        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                tracing::error!("Error waiting for task to complete: {e}");
            }
        }

        // Disconnect WebSocket client
        if let Err(e) = self.ws_client.disconnect().await {
            tracing::error!("Error disconnecting WebSocket client: {e}");
        }

        // Clear state
        {
            let mut instruments = self.instruments.write().unwrap();
            instruments.clear();
        }

        self.is_connected.store(false, Ordering::Relaxed);
        tracing::info!(client_id = %self.client_id, "Disconnected");

        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> anyhow::Result<()> {
        tracing::debug!("Requesting all instruments");

        let instruments = {
            let instruments_map = self.instruments.read().unwrap();
            instruments_map.values().cloned().collect()
        };

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            self.venue(),
            instruments,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instruments response: {e}");
        }

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> anyhow::Result<()> {
        tracing::debug!("Requesting instrument: {}", request.instrument_id);

        let instrument = self.get_instrument(&request.instrument_id)?;

        let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            instrument.id(),
            instrument,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        )));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instrument response: {e}");
        }

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> anyhow::Result<()> {
        tracing::debug!("Requesting bars for {}", request.bar_type);

        let http = self.http_client.clone();
        let sender = self.data_sender.clone();
        let bar_type = request.bar_type;
        let start = request.start;
        let end = request.end;
        let limit = request.limit.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params.clone();
        let clock = self.clock;
        let start_nanos = datetime_to_unix_nanos(start);
        let end_nanos = datetime_to_unix_nanos(end);
        let instruments = Arc::clone(&self.instruments);

        tokio::spawn(async move {
            match request_bars_from_http(http, bar_type, start, end, limit, instruments).await {
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
                        tracing::error!("Failed to send bars response: {e}");
                    }
                }
                Err(e) => tracing::error!("Bar request failed: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> anyhow::Result<()> {
        tracing::debug!("Requesting trades for {}", request.instrument_id);

        // NOTE: Hyperliquid does not provide public historical trade data via REST API
        // - Real-time trades are available via WebSocket (subscribe_trades)
        // - User fills (authenticated) are available via generate_fill_reports
        // For now, return empty response similar to exchanges without public trade history
        tracing::warn!(
            "Historical trade data not available via REST on Hyperliquid for {}",
            request.instrument_id
        );

        let trades = Vec::new();

        let response = DataResponse::Trades(TradesResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            request.instrument_id,
            trades,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send trades response: {e}");
        }

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: &SubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to trades: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.subscribe_trades(instrument_id).await {
                tracing::error!("Failed to subscribe to trades: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_trades(&mut self, unsubscription: &UnsubscribeTrades) -> anyhow::Result<()> {
        tracing::debug!(
            "Unsubscribing from trades: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let instrument_id = unsubscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.unsubscribe_trades(instrument_id).await {
                tracing::error!("Failed to unsubscribe from trades: {e:?}");
            }
        });

        tracing::info!(
            "Unsubscribed from trades for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, subscription: &SubscribeBookDeltas) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to book deltas: {}", subscription.instrument_id);

        if subscription.book_type != nautilus_model::enums::BookType::L2_MBP {
            anyhow::bail!("Hyperliquid only supports L2_MBP order book deltas");
        }

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.subscribe_book(instrument_id).await {
                tracing::error!("Failed to subscribe to book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_deltas(
        &mut self,
        unsubscription: &UnsubscribeBookDeltas,
    ) -> anyhow::Result<()> {
        tracing::debug!(
            "Unsubscribing from book deltas: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let instrument_id = unsubscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.unsubscribe_book(instrument_id).await {
                tracing::error!("Failed to unsubscribe from book deltas: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_book_snapshots(
        &mut self,
        subscription: &SubscribeBookSnapshots,
    ) -> anyhow::Result<()> {
        tracing::debug!(
            "Subscribing to book snapshots: {}",
            subscription.instrument_id
        );

        if subscription.book_type != nautilus_model::enums::BookType::L2_MBP {
            anyhow::bail!("Hyperliquid only supports L2_MBP order book snapshots");
        }

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                tracing::error!("Failed to subscribe to book snapshots: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_snapshots(
        &mut self,
        unsubscription: &UnsubscribeBookSnapshots,
    ) -> anyhow::Result<()> {
        tracing::debug!(
            "Unsubscribing from book snapshots: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let instrument_id = unsubscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
                tracing::error!("Failed to unsubscribe from book snapshots: {e:?}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, subscription: &SubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to quotes: {}", subscription.instrument_id);

        let ws = self.ws_client.clone();
        let instrument_id = subscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                tracing::error!("Failed to subscribe to quotes: {e:?}");
            }
        });

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, unsubscription: &UnsubscribeQuotes) -> anyhow::Result<()> {
        tracing::debug!(
            "Unsubscribing from quotes: {}",
            unsubscription.instrument_id
        );

        let ws = self.ws_client.clone();
        let instrument_id = unsubscription.instrument_id;

        tokio::spawn(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
                tracing::error!("Failed to unsubscribe from quotes: {e:?}");
            }
        });

        tracing::info!(
            "Unsubscribed from quotes for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: &SubscribeBars) -> anyhow::Result<()> {
        tracing::debug!("Subscribing to bars: {}", subscription.bar_type);

        let instruments = self.instruments.read().unwrap();
        let instrument_id = subscription.bar_type.instrument_id();
        if !instruments.contains_key(&instrument_id) {
            anyhow::bail!("Instrument {instrument_id} not found");
        }

        drop(instruments);

        let bar_type = subscription.bar_type;
        let ws = self.ws_client.clone();

        tokio::spawn(async move {
            if let Err(e) = ws.subscribe_bars(bar_type).await {
                tracing::error!("Failed to subscribe to bars: {e:?}");
            }
        });

        tracing::info!("Subscribed to bars for {}", subscription.bar_type);

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> anyhow::Result<()> {
        tracing::debug!("Unsubscribing from bars: {}", unsubscription.bar_type);

        let bar_type = unsubscription.bar_type;
        let ws = self.ws_client.clone();

        tokio::spawn(async move {
            if let Err(e) = ws.unsubscribe_bars(bar_type).await {
                tracing::error!("Failed to unsubscribe from bars: {e:?}");
            }
        });

        tracing::info!("Unsubscribed from bars for {}", unsubscription.bar_type);

        Ok(())
    }
}

pub(crate) fn candle_to_bar(
    candle: &HyperliquidCandle,
    bar_type: BarType,
    price_precision: u8,
    size_precision: u8,
) -> anyhow::Result<Bar> {
    let ts_init = UnixNanos::from(candle.timestamp * 1_000_000);
    let ts_event = ts_init;

    let open = candle.open.parse::<f64>().context("parse open price")?;
    let high = candle.high.parse::<f64>().context("parse high price")?;
    let low = candle.low.parse::<f64>().context("parse low price")?;
    let close = candle.close.parse::<f64>().context("parse close price")?;
    let volume = candle.volume.parse::<f64>().context("parse volume")?;

    Ok(Bar::new(
        bar_type,
        Price::new(open, price_precision),
        Price::new(high, price_precision),
        Price::new(low, price_precision),
        Price::new(close, price_precision),
        Quantity::new(volume, size_precision),
        ts_event,
        ts_init,
    ))
}

/// Request bars from HTTP API.
async fn request_bars_from_http(
    http_client: HyperliquidHttpClient,
    bar_type: BarType,
    start: Option<DateTime<Utc>>,
    end: Option<DateTime<Utc>>,
    limit: Option<u32>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
) -> anyhow::Result<Vec<Bar>> {
    // Get instrument details for precision
    let instrument_id = bar_type.instrument_id();
    let instrument = {
        let guard = instruments.read().unwrap();
        guard
            .get(&instrument_id)
            .cloned()
            .context("instrument not found in cache")?
    };

    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    // Extract coin symbol from instrument ID (e.g., "BTC-PERP.HYPERLIQUID" -> "BTC")
    let coin = instrument_id
        .symbol
        .as_str()
        .split('-')
        .next()
        .context("invalid instrument symbol")?;

    let interval = bar_type_to_interval(&bar_type)?;

    // Hyperliquid uses millisecond timestamps
    let now = Utc::now();
    let end_time = end.unwrap_or(now).timestamp_millis() as u64;
    let start_time = if let Some(start) = start {
        start.timestamp_millis() as u64
    } else {
        // Default to 1000 bars before end_time
        let spec = bar_type.spec();
        let step_ms = match spec.aggregation {
            BarAggregation::Minute => spec.step.get() as u64 * 60_000,
            BarAggregation::Hour => spec.step.get() as u64 * 3_600_000,
            BarAggregation::Day => spec.step.get() as u64 * 86_400_000,
            _ => 60_000,
        };
        end_time.saturating_sub(1000 * step_ms)
    };

    let candles = http_client
        .info_candle_snapshot(coin, interval, start_time, end_time)
        .await
        .context("failed to fetch candle snapshot from Hyperliquid")?;

    let mut bars: Vec<Bar> = candles
        .iter()
        .filter_map(|candle| {
            candle_to_bar(candle, bar_type, price_precision, size_precision)
                .map_err(|e| {
                    tracing::warn!("Failed to convert candle to bar: {e}");
                    e
                })
                .ok()
        })
        .collect();

    if let Some(limit) = limit
        && bars.len() > limit as usize
    {
        bars = bars.into_iter().take(limit as usize).collect();
    }

    tracing::debug!("Fetched {} bars for {}", bars.len(), bar_type);
    Ok(bars)
}
