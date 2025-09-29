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
use anyhow::{Context, Result, anyhow};
use chrono::{DateTime, Utc};
use nautilus_common::{
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
    runner::get_data_event_sender,
};
use nautilus_core::{
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::Data,
    identifiers::{ClientId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::consts::{HYPERLIQUID_TESTNET_WS_URL, HYPERLIQUID_VENUE, HYPERLIQUID_WS_URL},
    config::HyperliquidDataClientConfig,
    http::client::HyperliquidHttpClient,
    websocket::client::HyperliquidWebSocketClient,
};

#[derive(Debug)]
pub struct HyperliquidDataClient {
    client_id: ClientId,
    #[allow(dead_code)]
    config: HyperliquidDataClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: Option<HyperliquidWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
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
    pub fn new(client_id: ClientId, config: HyperliquidDataClientConfig) -> Result<Self> {
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
            HyperliquidHttpClient::with_credentials(&secrets, config.http_timeout_secs)
        } else {
            HyperliquidHttpClient::new(config.is_testnet, config.http_timeout_secs)
        };

        Ok(Self {
            client_id,
            config,
            http_client,
            ws_client: None, // Will be initialized on connect
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            data_sender,
            instruments: Arc::new(RwLock::new(AHashMap::new())),
            clock,
            instrument_refresh_active: false,
        })
    }

    fn venue(&self) -> Venue {
        *HYPERLIQUID_VENUE
    }

    async fn bootstrap_instruments(&mut self) -> Result<Vec<InstrumentAny>> {
        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("Failed to fetch instruments during bootstrap")?;

        let mut instruments_map = self.instruments.write().unwrap();
        for instrument in &instruments {
            instruments_map.insert(instrument.id(), instrument.clone());
        }

        tracing::info!("Bootstrapped {} instruments", instruments_map.len());
        Ok(instruments)
    }

    async fn spawn_ws(&mut self) -> Result<()> {
        let ws_url = if self.config.is_testnet {
            HYPERLIQUID_TESTNET_WS_URL
        } else {
            HYPERLIQUID_WS_URL
        };

        tracing::info!("Connecting to Hyperliquid WebSocket at {}", ws_url);

        let ws_client = HyperliquidWebSocketClient::connect(ws_url)
            .await
            .context("Failed to connect to Hyperliquid WebSocket")?;

        self.ws_client = Some(ws_client);
        tracing::info!("Hyperliquid WebSocket client connected successfully");

        Ok(())
    }

    fn get_instrument(&self, instrument_id: &InstrumentId) -> Result<InstrumentAny> {
        let instruments = self.instruments.read().unwrap();
        instruments
            .get(instrument_id)
            .cloned()
            .ok_or_else(|| anyhow!("Instrument {instrument_id} not found"))
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
        if let Err(err) = sender.send(DataEvent::Data(data)) {
            tracing::error!("Failed to emit data event: {err}");
        }
    }
}

#[async_trait::async_trait]
impl DataClient for HyperliquidDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(self.venue())
    }

    fn start(&mut self) -> Result<()> {
        tracing::info!("Starting Hyperliquid data client {}", self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        tracing::info!("Stopping Hyperliquid data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        tracing::debug!("Resetting Hyperliquid data client {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> Result<()> {
        tracing::debug!("Disposing Hyperliquid data client {}", self.client_id);
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        tracing::info!("Connecting HyperliquidDataClient...");

        // Bootstrap instruments from HTTP API
        let _instruments = self
            .bootstrap_instruments()
            .await
            .context("Failed to bootstrap instruments")?;

        // Connect WebSocket client
        self.spawn_ws()
            .await
            .context("Failed to spawn WebSocket client")?;

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!("HyperliquidDataClient connected");

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        tracing::info!("Disconnecting HyperliquidDataClient...");

        // Cancel all tasks
        self.cancellation_token.cancel();

        // Wait for all tasks to complete
        for task in self.tasks.drain(..) {
            if let Err(e) = task.await {
                tracing::error!("Error waiting for task to complete: {e}");
            }
        }

        // Disconnect WebSocket client
        if let Some(mut ws_client) = self.ws_client.take()
            && let Err(e) = ws_client.disconnect().await
        {
            tracing::error!("Error disconnecting WebSocket client: {e}");
        }

        // Clear state
        {
            let mut instruments = self.instruments.write().unwrap();
            instruments.clear();
        }

        self.is_connected.store(false, Ordering::Relaxed);
        tracing::info!("HyperliquidDataClient disconnected");

        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> Result<()> {
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

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instruments response: {err}");
        }

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> Result<()> {
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

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instrument response: {err}");
        }

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> Result<()> {
        tracing::debug!("Requesting bars for {}", request.bar_type);

        // TODO: Implement actual bar data fetching from HTTP API
        let bars = Vec::new(); // Placeholder

        let response = DataResponse::Bars(BarsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            request.bar_type,
            bars,
            datetime_to_unix_nanos(request.start),
            datetime_to_unix_nanos(request.end),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send bars response: {err}");
        }

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> Result<()> {
        tracing::debug!("Requesting trades for {}", request.instrument_id);

        // TODO: Implement actual trade data fetching from HTTP API
        let trades = Vec::new(); // Placeholder

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

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send trades response: {err}");
        }

        Ok(())
    }

    fn subscribe_trades(&mut self, subscription: &SubscribeTrades) -> Result<()> {
        tracing::debug!("Subscribing to trades: {}", subscription.instrument_id);

        // Validate instrument exists
        let instruments = self.instruments.read().unwrap();
        if !instruments.contains_key(&subscription.instrument_id) {
            return Err(anyhow!(
                "Instrument {} not found",
                subscription.instrument_id
            ));
        }

        // TODO: Add WebSocket subscription logic for trades
        tracing::info!("Subscribed to trades for {}", subscription.instrument_id);

        Ok(())
    }

    fn unsubscribe_trades(&mut self, unsubscription: &UnsubscribeTrades) -> Result<()> {
        tracing::debug!(
            "Unsubscribing from trades: {}",
            unsubscription.instrument_id
        );

        // TODO: Add WebSocket unsubscription logic for trades
        tracing::info!(
            "Unsubscribed from trades for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_book_deltas(&mut self, subscription: &SubscribeBookDeltas) -> Result<()> {
        tracing::debug!("Subscribing to book deltas: {}", subscription.instrument_id);

        // Validate instrument exists
        let instruments = self.instruments.read().unwrap();
        if !instruments.contains_key(&subscription.instrument_id) {
            return Err(anyhow!(
                "Instrument {} not found",
                subscription.instrument_id
            ));
        }

        // TODO: Add WebSocket subscription logic for book deltas
        tracing::info!(
            "Subscribed to book deltas for {}",
            subscription.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, unsubscription: &UnsubscribeBookDeltas) -> Result<()> {
        tracing::debug!(
            "Unsubscribing from book deltas: {}",
            unsubscription.instrument_id
        );

        // TODO: Add WebSocket unsubscription logic for book deltas
        tracing::info!(
            "Unsubscribed from book deltas for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, subscription: &SubscribeBookSnapshots) -> Result<()> {
        tracing::debug!(
            "Subscribing to book snapshots: {}",
            subscription.instrument_id
        );

        // Validate instrument exists
        let instruments = self.instruments.read().unwrap();
        if !instruments.contains_key(&subscription.instrument_id) {
            return Err(anyhow!(
                "Instrument {} not found",
                subscription.instrument_id
            ));
        }

        // TODO: Add WebSocket subscription logic for book snapshots
        tracing::info!(
            "Subscribed to book snapshots for {}",
            subscription.instrument_id
        );

        Ok(())
    }

    fn unsubscribe_book_snapshots(
        &mut self,
        unsubscription: &UnsubscribeBookSnapshots,
    ) -> Result<()> {
        tracing::debug!(
            "Unsubscribing from book snapshots: {}",
            unsubscription.instrument_id
        );

        // TODO: Add WebSocket unsubscription logic for book snapshots
        tracing::info!(
            "Unsubscribed from book snapshots for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_quotes(&mut self, subscription: &SubscribeQuotes) -> Result<()> {
        tracing::debug!("Subscribing to quotes: {}", subscription.instrument_id);

        // Validate instrument exists
        let instruments = self.instruments.read().unwrap();
        if !instruments.contains_key(&subscription.instrument_id) {
            return Err(anyhow!(
                "Instrument {} not found",
                subscription.instrument_id
            ));
        }

        // TODO: Add WebSocket subscription logic for quotes
        tracing::info!("Subscribed to quotes for {}", subscription.instrument_id);

        Ok(())
    }

    fn unsubscribe_quotes(&mut self, unsubscription: &UnsubscribeQuotes) -> Result<()> {
        tracing::debug!(
            "Unsubscribing from quotes: {}",
            unsubscription.instrument_id
        );

        // TODO: Add WebSocket unsubscription logic for quotes
        tracing::info!(
            "Unsubscribed from quotes for {}",
            unsubscription.instrument_id
        );

        Ok(())
    }

    fn subscribe_bars(&mut self, subscription: &SubscribeBars) -> Result<()> {
        tracing::debug!("Subscribing to bars: {}", subscription.bar_type);

        // Validate instrument exists
        let instruments = self.instruments.read().unwrap();
        let instrument_id = subscription.bar_type.instrument_id();
        if !instruments.contains_key(&instrument_id) {
            return Err(anyhow!("Instrument {} not found", instrument_id));
        }

        // TODO: Add WebSocket subscription logic for bars
        tracing::info!("Subscribed to bars for {}", subscription.bar_type);

        Ok(())
    }

    fn unsubscribe_bars(&mut self, unsubscription: &UnsubscribeBars) -> Result<()> {
        tracing::debug!("Unsubscribing from bars: {}", unsubscription.bar_type);

        // TODO: Add WebSocket unsubscription logic for bars
        tracing::info!("Unsubscribed from bars for {}", unsubscription.bar_type);

        Ok(())
    }
}
