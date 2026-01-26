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

//! Live market data client implementation for the Deribit adapter.

use std::sync::{
    Arc, RwLock,
    atomic::{AtomicBool, Ordering},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::StreamExt;
use nautilus_common::{
    clients::DataClient,
    live::{runner::get_data_event_sender, runtime::get_runtime},
    log_info,
    messages::{
        DataEvent, DataResponse,
        data::{
            BarsResponse, BookResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestBookSnapshot, RequestInstrument, RequestInstruments, RequestTrades,
            SubscribeBars, SubscribeBookDeltas, SubscribeBookDepth10, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeInstrument, SubscribeInstruments, SubscribeMarkPrices,
            SubscribeQuotes, SubscribeTrades, TradesResponse, UnsubscribeBars,
            UnsubscribeBookDeltas, UnsubscribeBookDepth10, UnsubscribeFundingRates,
            UnsubscribeIndexPrices, UnsubscribeInstrument, UnsubscribeInstruments,
            UnsubscribeMarkPrices, UnsubscribeQuotes, UnsubscribeTrades,
        },
    },
};
use nautilus_core::{
    datetime::datetime_to_unix_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_model::{
    data::{Data, OrderBookDeltas_API},
    enums::BookType,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{
            DERIBIT_BOOK_DEFAULT_DEPTH, DERIBIT_BOOK_DEFAULT_GROUP, DERIBIT_BOOK_VALID_DEPTHS,
            DERIBIT_VENUE,
        },
        parse::{bar_spec_to_resolution, parse_instrument_kind_currency},
    },
    config::DeribitDataClientConfig,
    http::{
        client::DeribitHttpClient,
        models::{DeribitCurrency, DeribitInstrumentKind},
    },
    websocket::{
        auth::DERIBIT_DATA_SESSION_NAME, client::DeribitWebSocketClient,
        enums::DeribitUpdateInterval, messages::NautilusWsMessage,
    },
};

/// Deribit live data client.
#[derive(Debug)]
pub struct DeribitDataClient {
    client_id: ClientId,
    config: DeribitDataClientConfig,
    http_client: DeribitHttpClient,
    ws_client: Option<DeribitWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    clock: &'static AtomicTime,
}

impl DeribitDataClient {
    /// Creates a new [`DeribitDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(client_id: ClientId, config: DeribitDataClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        };

        let ws_client = DeribitWebSocketClient::new(
            Some(config.ws_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.heartbeat_interval_secs,
            config.use_testnet,
        )?;

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client: Some(ws_client),
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            clock,
        })
    }

    /// Returns a mutable reference to the WebSocket client.
    fn ws_client_mut(&mut self) -> anyhow::Result<&mut DeribitWebSocketClient> {
        self.ws_client
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))
    }

    /// Spawns a task to process WebSocket messages.
    fn spawn_stream_task(
        &mut self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) -> anyhow::Result<()> {
        let data_sender = self.data_sender.clone();
        let instruments = Arc::clone(&self.instruments);
        let cancellation = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    maybe_msg = stream.next() => {
                        match maybe_msg {
                            Some(msg) => Self::handle_ws_message(msg, &data_sender, &instruments),
                            None => {
                                log::debug!("WebSocket stream ended");
                                break;
                            }
                        }
                    }
                    () = cancellation.cancelled() => {
                        log::debug!("WebSocket stream task cancelled");
                        break;
                    }
                }
            }
        });

        self.tasks.push(handle);
        Ok(())
    }

    /// Handles incoming WebSocket messages.
    fn handle_ws_message(
        message: NautilusWsMessage,
        sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        instruments: &Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    ) {
        match message {
            NautilusWsMessage::Data(payloads) => {
                for data in payloads {
                    Self::send_data(sender, data);
                }
            }
            NautilusWsMessage::Deltas(deltas) => {
                Self::send_data(sender, Data::Deltas(OrderBookDeltas_API::new(deltas)));
            }
            NautilusWsMessage::Instrument(instrument) => {
                let instrument_any = *instrument;
                if let Ok(mut guard) = instruments.write() {
                    let instrument_id = instrument_any.id();
                    guard.insert(instrument_id, instrument_any.clone());
                    drop(guard);

                    if let Err(e) = sender.send(DataEvent::Instrument(instrument_any)) {
                        log::warn!("Failed to send instrument update: {e}");
                    }
                } else {
                    log::error!("Instrument cache lock poisoned, skipping instrument update");
                }
            }
            NautilusWsMessage::Error(e) => {
                log::error!("WebSocket error: {e:?}");
            }
            NautilusWsMessage::Raw(value) => {
                log::debug!("Unhandled raw message: {value}");
            }
            NautilusWsMessage::Reconnected => {
                log::info!("WebSocket reconnected");
            }
            NautilusWsMessage::Authenticated(auth) => {
                log::debug!("WebSocket authenticated: expires_in={}s", auth.expires_in);
            }
            NautilusWsMessage::FundingRates(funding_rates) => {
                log::info!(
                    "Received {} funding rate update(s) from WebSocket",
                    funding_rates.len()
                );
                for funding_rate in funding_rates {
                    log::debug!("Sending funding rate: {funding_rate:?}");
                    if let Err(e) = sender.send(DataEvent::FundingRate(funding_rate)) {
                        log::error!("Failed to send funding rate: {e}");
                    }
                }
            }
            NautilusWsMessage::OrderStatusReports(reports) => {
                log::warn!(
                    "Data client received OrderStatusReports message (should be handled by execution client): {} reports",
                    reports.len()
                );
            }
            NautilusWsMessage::FillReports(reports) => {
                log::warn!(
                    "Data client received FillReports message (should be handled by execution client): {} reports",
                    reports.len()
                );
            }
            NautilusWsMessage::OrderRejected(order) => {
                log::warn!(
                    "Data client received OrderRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderAccepted(order) => {
                log::warn!(
                    "Data client received OrderAccepted message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderCanceled(order) => {
                log::warn!(
                    "Data client received OrderCanceled message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderExpired(order) => {
                log::warn!(
                    "Data client received OrderExpired message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderUpdated(order) => {
                log::warn!(
                    "Data client received OrderUpdated message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderCancelRejected(order) => {
                log::warn!(
                    "Data client received OrderCancelRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::OrderModifyRejected(order) => {
                log::warn!(
                    "Data client received OrderModifyRejected message (should be handled by execution client): {order:?}"
                );
            }
            NautilusWsMessage::AccountState(state) => {
                log::warn!(
                    "Data client received AccountState message (should be handled by execution client): {state:?}"
                );
            }
        }
    }

    /// Sends data to the data channel.
    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(e) = sender.send(DataEvent::Data(data)) {
            log::error!("Failed to send data: {e}");
        }
    }
}

#[async_trait(?Send)]
impl DataClient for DeribitDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DERIBIT_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        log::info!(
            "Starting data client: client_id={}, use_testnet={}",
            self.client_id,
            self.config.use_testnet
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        log::info!("Stopping data client: {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        log::info!("Resetting data client: {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        if let Ok(mut instruments) = self.instruments.write() {
            instruments.clear();
        }
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        log::info!("Disposing data client: {}", self.client_id);
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

        // Fetch instruments for each configured instrument kind
        let instrument_kinds = if self.config.instrument_kinds.is_empty() {
            vec![DeribitInstrumentKind::Future]
        } else {
            self.config.instrument_kinds.clone()
        };

        let mut all_instruments = Vec::new();
        for kind in &instrument_kinds {
            let fetched = self
                .http_client
                .request_instruments(DeribitCurrency::ANY, Some(*kind))
                .await
                .with_context(|| format!("failed to request instruments for {kind:?}"))?;

            // Cache in http client
            self.http_client.cache_instruments(fetched.clone());

            // Cache locally
            let mut guard = self
                .instruments
                .write()
                .map_err(|e| anyhow::anyhow!("{e}"))?;
            for instrument in &fetched {
                guard.insert(instrument.id(), instrument.clone());
            }
            drop(guard);

            all_instruments.extend(fetched);
        }

        log::info!(
            "Cached instruments: client_id={}, total={}",
            self.client_id,
            all_instruments.len()
        );

        for instrument in &all_instruments {
            if let Err(e) = self
                .data_sender
                .send(DataEvent::Instrument(instrument.clone()))
            {
                log::warn!("Failed to send instrument: {e}");
            }
        }

        // Cache instruments in WebSocket client before connecting
        let ws = self.ws_client_mut()?;
        ws.cache_instruments(all_instruments);

        // Connect WebSocket and wait until active
        ws.connect().await.context("failed to connect WebSocket")?;
        ws.wait_until_active(10.0)
            .await
            .context("WebSocket failed to become active")?;

        // Authenticate if credentials are configured (required for raw streams)
        if ws.has_credentials() {
            ws.authenticate_session(DERIBIT_DATA_SESSION_NAME)
                .await
                .context("failed to authenticate WebSocket")?;
            log_info!("WebSocket authenticated");
        }

        // Get the stream and spawn processing task
        let stream = self.ws_client_mut()?.stream();
        self.spawn_stream_task(stream)?;

        self.is_connected.store(true, Ordering::Release);
        let network = if self.config.use_testnet {
            "testnet"
        } else {
            "mainnet"
        };
        log_info!("Connected ({})", network);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.is_disconnected() {
            return Ok(());
        }

        // Cancel all tasks
        self.cancellation_token.cancel();

        // Close WebSocket connection
        if let Some(ws) = self.ws_client.as_ref()
            && let Err(e) = ws.close().await
        {
            log::warn!("Error while closing WebSocket: {e:?}");
        }

        // Wait for all tasks to complete
        for handle in self.tasks.drain(..) {
            if let Err(e) = handle.await {
                log::error!("Error joining WebSocket task: {e:?}");
            }
        }

        // Reset cancellation token for potential reconnection
        self.cancellation_token = CancellationToken::new();
        self.is_connected.store(false, Ordering::Relaxed);

        log_info!("Disconnected");
        Ok(())
    }

    fn subscribe_instruments(&mut self, cmd: &SubscribeInstruments) -> anyhow::Result<()> {
        // Extract kind and currency from params, defaulting to "any.any" (all instruments)
        let kind = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("kind"))
            .map_or("any", |s| s.as_str())
            .to_string();
        let currency = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("currency"))
            .map_or("any", |s| s.as_str())
            .to_string();

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::info!("Subscribing to instrument state changes for {kind}.{currency}");

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_instrument_state(&kind, &currency).await {
                log::error!("Failed to subscribe to instrument state for {kind}.{currency}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_instrument(&mut self, cmd: &SubscribeInstrument) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Check if instrument is in cache (should be from connect())
        let guard = self
            .instruments
            .read()
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        if !guard.contains_key(&instrument_id) {
            log::warn!(
                "Instrument {instrument_id} not in cache - it may have been created after connect()"
            );
        }
        drop(guard);

        // Determine kind and currency from instrument_id
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::info!(
            "Subscribing to instrument state for {instrument_id} (channel: {kind}.{currency})"
        );

        // Subscribe to broader kind/currency channel (filter in handler)
        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_instrument_state(&kind, &currency).await {
                log::error!("Failed to subscribe to instrument state for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Deribit only supports L2_MBP order book deltas");
        }

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        // Get interval from params, default to 100ms (public)
        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        let depth = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("depth"))
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DERIBIT_BOOK_DEFAULT_DEPTH);

        if !DERIBIT_BOOK_VALID_DEPTHS.contains(&depth) {
            anyhow::bail!("invalid depth {depth}; supported depths: {DERIBIT_BOOK_VALID_DEPTHS:?}");
        }

        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("group"))
            .map_or(DERIBIT_BOOK_DEFAULT_GROUP, String::as_str)
            .to_string();

        log::info!(
            "Subscribing to book deltas for {} (group: {}, depth: {}, interval: {}, book_type: {:?})",
            instrument_id,
            group,
            depth,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string()),
            cmd.book_type
        );

        get_runtime().spawn(async move {
            let result = if interval == Some(DeribitUpdateInterval::Raw) {
                ws.subscribe_book(instrument_id, interval).await
            } else {
                ws.subscribe_book_grouped(instrument_id, &group, depth, interval)
                    .await
            };

            if let Err(e) = result {
                log::error!("Failed to subscribe to book deltas for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_book_depth10(&mut self, cmd: &SubscribeBookDepth10) -> anyhow::Result<()> {
        if cmd.book_type != BookType::L2_MBP {
            anyhow::bail!("Deribit only supports L2_MBP order book depth");
        }

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        // Get interval from params, default to 100ms (public)
        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        // Get price grouping from params, default to "none" (no grouping)
        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("group"))
            .map_or(DERIBIT_BOOK_DEFAULT_GROUP, String::as_str)
            .to_string();

        log::info!(
            "Subscribing to book depth10 for {} (group: {}, interval: {}, book_type: {:?})",
            instrument_id,
            group,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string()),
            cmd.book_type
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .subscribe_book_grouped(instrument_id, &group, 10, interval)
                .await
            {
                log::error!("Failed to subscribe to book depth10 for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        log::info!("Subscribing to quotes for {instrument_id}");

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_quotes(instrument_id).await {
                log::error!("Failed to subscribe to quotes for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Subscribing to trades for {} (interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_trades(instrument_id, interval).await {
                log::error!("Failed to subscribe to trades for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Subscribing to mark prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to subscribe to mark prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Subscribing to index prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to subscribe to index prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Validate instrument is a perpetual - funding rates only apply to perpetual contracts
        let is_perpetual = self
            .instruments
            .read()
            .map_err(|e| anyhow::anyhow!("Instrument cache lock poisoned: {e}"))?
            .get(&instrument_id)
            .is_some_and(|inst| matches!(inst, InstrumentAny::CryptoPerpetual(_)));

        if !is_perpetual {
            log::warn!(
                "Funding rates subscription rejected for {instrument_id}: only available for perpetual instruments."
            );
            return Ok(());
        }

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        // Funding rates use the dedicated perpetual channel
        log::info!(
            "Subscribing to funding rates for {} (perpetual channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .subscribe_perpetual_interests_rates_updates(instrument_id, interval)
                .await
            {
                log::error!("Failed to subscribe to funding rates for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.bar_type.instrument_id();
        // Convert bar spec to Deribit resolution
        let resolution = bar_spec_to_resolution(&cmd.bar_type);

        get_runtime().spawn(async move {
            if let Err(e) = ws.subscribe_chart(instrument_id, &resolution).await {
                log::error!("Failed to subscribe to bars for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_instruments(&mut self, cmd: &UnsubscribeInstruments) -> anyhow::Result<()> {
        let kind = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("kind"))
            .map_or("any", |s| s.as_str())
            .to_string();
        let currency = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("currency"))
            .map_or("any", |s| s.as_str())
            .to_string();

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::info!("Unsubscribing from instrument state changes for {kind}.{currency}");

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_instrument_state(&kind, &currency).await {
                log::error!(
                    "Failed to unsubscribe from instrument state for {kind}.{currency}: {e}"
                );
            }
        });

        Ok(())
    }

    fn unsubscribe_instrument(&mut self, cmd: &UnsubscribeInstrument) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Determine kind and currency from instrument_id
        let (kind, currency) = parse_instrument_kind_currency(&instrument_id);

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        log::info!(
            "Unsubscribing from instrument state for {instrument_id} (channel: {kind}.{currency})"
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_instrument_state(&kind, &currency).await {
                log::error!("Failed to unsubscribe from instrument state for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        // Get interval from params to match the subscribed channel
        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        let depth = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("depth"))
            .and_then(|v| v.parse::<u32>().ok())
            .unwrap_or(DERIBIT_BOOK_DEFAULT_DEPTH);

        if !DERIBIT_BOOK_VALID_DEPTHS.contains(&depth) {
            anyhow::bail!("invalid depth {depth}; supported depths: {DERIBIT_BOOK_VALID_DEPTHS:?}");
        }

        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("group"))
            .map_or(DERIBIT_BOOK_DEFAULT_GROUP, String::as_str)
            .to_string();

        log::info!(
            "Unsubscribing from book deltas for {} (group: {}, depth: {}, interval: {})",
            instrument_id,
            group,
            depth,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            let result = if interval == Some(DeribitUpdateInterval::Raw) {
                ws.unsubscribe_book(instrument_id, interval).await
            } else {
                ws.unsubscribe_book_grouped(instrument_id, &group, depth, interval)
                    .await
            };

            if let Err(e) = result {
                log::error!("Failed to unsubscribe from book deltas for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_book_depth10(&mut self, cmd: &UnsubscribeBookDepth10) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        // Get interval from params to match the subscribed channel
        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        // Get price grouping from params to match the subscribed channel
        let group = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("group"))
            .map_or(DERIBIT_BOOK_DEFAULT_GROUP, String::as_str)
            .to_string();

        log::info!(
            "Unsubscribing from book depth10 for {} (group: {}, interval: {})",
            instrument_id,
            group,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .unsubscribe_book_grouped(instrument_id, &group, 10, interval)
                .await
            {
                log::error!("Failed to unsubscribe from book depth10 for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        log::info!("Unsubscribing from quotes for {instrument_id}");

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_quotes(instrument_id).await {
                log::error!("Failed to unsubscribe from quotes for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Unsubscribing from trades for {} (interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_trades(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from trades for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Unsubscribing from mark prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from mark prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.instrument_id;

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Unsubscribing from index prices for {} (via ticker channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_ticker(instrument_id, interval).await {
                log::error!("Failed to unsubscribe from index prices for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // Validate instrument is a perpetual - funding rates only apply to perpetual contracts
        let is_perpetual = self
            .instruments
            .read()
            .map_err(|e| anyhow::anyhow!("Instrument cache lock poisoned: {e}"))?
            .get(&instrument_id)
            .is_some_and(|inst| matches!(inst, InstrumentAny::CryptoPerpetual(_)));

        if !is_perpetual {
            log::warn!(
                "Funding rates unsubscription rejected for {instrument_id}: only available for perpetual instruments."
            );
            return Ok(());
        }

        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();

        let interval = cmd
            .params
            .as_ref()
            .and_then(|p| p.get("interval"))
            .and_then(|v| v.parse::<DeribitUpdateInterval>().ok());

        log::info!(
            "Unsubscribing from funding rates for {} (perpetual channel, interval: {})",
            instrument_id,
            interval.map_or("100ms (default)".to_string(), |i| i.to_string())
        );

        get_runtime().spawn(async move {
            if let Err(e) = ws
                .unsubscribe_perpetual_interest_rates_updates(instrument_id, interval)
                .await
            {
                log::error!("Failed to unsubscribe from funding rates for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> anyhow::Result<()> {
        let ws = self
            .ws_client
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("WebSocket client not initialized"))?
            .clone();
        let instrument_id = cmd.bar_type.instrument_id();
        let resolution = bar_spec_to_resolution(&cmd.bar_type);

        get_runtime().spawn(async move {
            if let Err(e) = ws.unsubscribe_chart(instrument_id, &resolution).await {
                log::error!("Failed to unsubscribe from bars for {instrument_id}: {e}");
            }
        });

        Ok(())
    }

    fn request_instruments(&self, request: RequestInstruments) -> anyhow::Result<()> {
        if request.start.is_some() {
            log::warn!(
                "Requesting instruments for {:?} with specified `start` which has no effect",
                request.venue
            );
        }
        if request.end.is_some() {
            log::warn!(
                "Requesting instruments for {:?} with specified `end` which has no effect",
                request.venue
            );
        }

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;
        let venue = *DERIBIT_VENUE;

        // Get instrument kinds from config, default to Future if empty
        let instrument_kinds = if self.config.instrument_kinds.is_empty() {
            vec![crate::http::models::DeribitInstrumentKind::Future]
        } else {
            self.config.instrument_kinds.clone()
        };

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            for kind in &instrument_kinds {
                log::debug!("Requesting instruments for currency=ANY, kind={kind:?}");

                match http_client
                    .request_instruments(DeribitCurrency::ANY, Some(*kind))
                    .await
                {
                    Ok(instruments) => {
                        log::info!(
                            "Fetched {} instruments for ANY/{:?}",
                            instruments.len(),
                            kind
                        );

                        for instrument in instruments {
                            // Cache the instrument
                            {
                                match instruments_cache.write() {
                                    Ok(mut guard) => {
                                        guard.insert(instrument.id(), instrument.clone());
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Instrument cache lock poisoned: {e}, skipping cache update"
                                        );
                                    }
                                }
                            }

                            all_instruments.push(instrument);
                        }
                    }
                    Err(e) => {
                        log::error!("Failed to fetch instruments for ANY/{kind:?}: {e:?}");
                    }
                }
            }

            // Send response with all collected instruments
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
                log::error!("Failed to send instruments response: {e}");
            }
        });

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

        // First, check if instrument exists in cache
        if let Some(instrument) = self
            .instruments
            .read()
            .map_err(|e| anyhow::anyhow!("Instrument cache lock poisoned: {e}"))?
            .get(&request.instrument_id)
            .cloned()
        {
            let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                request.request_id,
                request.client_id.unwrap_or(self.client_id),
                instrument.id(),
                instrument,
                datetime_to_unix_nanos(request.start),
                datetime_to_unix_nanos(request.end),
                self.clock.get_time_ns(),
                request.params,
            )));

            if let Err(e) = self.data_sender.send(DataEvent::Response(response)) {
                log::error!("Failed to send instrument response: {e}");
            }
            return Ok(());
        }

        log::debug!(
            "Instrument {} not in cache, fetching from API",
            request.instrument_id
        );

        let http_client = self.http_client.clone();
        let instruments_cache = Arc::clone(&self.instruments);
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let start_nanos = datetime_to_unix_nanos(request.start);
        let end_nanos = datetime_to_unix_nanos(request.end);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http_client
                .request_instrument(instrument_id)
                .await
                .context("failed to request instrument from Deribit")
            {
                Ok(instrument) => {
                    log::info!("Successfully fetched instrument: {instrument_id}");

                    // Cache the instrument
                    {
                        let mut guard = instruments_cache
                            .write()
                            .expect("instrument cache lock poisoned");
                        guard.insert(instrument.id(), instrument.clone());
                    }

                    // Send response
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
                        log::error!("Failed to send instrument response: {e}");
                    }
                }
                Err(e) => {
                    log::error!("Instrument request failed for {instrument_id}: {e:?}");
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
                .request_trades(instrument_id, start, end, limit)
                .await
                .context("failed to request trades from Deribit")
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
                Err(e) => log::error!("Trades request failed for {instrument_id}: {e:?}"),
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
                .request_bars(bar_type, start, end, limit)
                .await
                .context("failed to request bars from Deribit")
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
                Err(e) => log::error!("Bars request failed for {bar_type}: {e:?}"),
            }
        });

        Ok(())
    }

    fn request_book_snapshot(&self, request: RequestBookSnapshot) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let sender = self.data_sender.clone();
        let instrument_id = request.instrument_id;
        let depth = request.depth.map(|n| n.get() as u32);
        let request_id = request.request_id;
        let client_id = request.client_id.unwrap_or(self.client_id);
        let params = request.params;
        let clock = self.clock;

        get_runtime().spawn(async move {
            match http_client
                .request_book_snapshot(instrument_id, depth)
                .await
                .context("failed to request book snapshot from Deribit")
            {
                Ok(book) => {
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
                    log::error!("Book snapshot request failed for {instrument_id}: {e:?}");
                }
            }
        });

        Ok(())
    }
}
