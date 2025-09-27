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

use std::{
    future::Future,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashMap;
use anyhow::{Context, Result};
use nautilus_common::{
    messages::{
        DataEvent,
        data::{
            BarsResponse, DataResponse, InstrumentResponse, InstrumentsResponse, RequestBars,
            RequestInstrument, RequestInstruments, RequestTrades, SubscribeBars,
            SubscribeBookDeltas, SubscribeBookSnapshots, SubscribeFundingRates,
            SubscribeIndexPrices, SubscribeMarkPrices, SubscribeQuotes, SubscribeTrades,
            TradesResponse, UnsubscribeBars, UnsubscribeBookDeltas, UnsubscribeBookSnapshots,
            UnsubscribeFundingRates, UnsubscribeIndexPrices, UnsubscribeMarkPrices,
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
    common::{
        consts::HYPERLIQUID_VENUE,
        credential::{EvmPrivateKey, Secrets},
    },
    config::HyperliquidDataClientConfig,
    http::client::HyperliquidHttpClient,
    websocket::client::HyperliquidWebSocketClient,
};

#[derive(Debug)]
pub struct HyperliquidDataClient {
    client_id: ClientId,
    config: HyperliquidDataClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: Option<HyperliquidWebSocketClient>,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    tasks: Vec<JoinHandle<()>>,
    data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    instruments: Arc<RwLock<AHashMap<InstrumentId, InstrumentAny>>>,
    clock: &'static AtomicTime,
}

impl HyperliquidDataClient {
    /// Creates a new [`HyperliquidDataClient`] instance.
    pub fn new(client_id: ClientId, config: HyperliquidDataClientConfig) -> Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        let http_client = if let Some(private_key_str) = &config.private_key {
            let secrets = Secrets {
                private_key: EvmPrivateKey::new(private_key_str.clone())?,
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
        })
    }

    fn venue(&self) -> Venue {
        *HYPERLIQUID_VENUE
    }

    #[allow(dead_code)]
    fn send_data(sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>, data: Data) {
        if let Err(err) = sender.send(DataEvent::Data(data)) {
            tracing::error!("Failed to emit data event: {err}");
        }
    }

    #[allow(dead_code)]
    fn spawn_ws<F>(&self, fut: F, context: &'static str)
    where
        F: Future<Output = Result<()>> + Send + 'static,
    {
        tokio::spawn(async move {
            if let Err(err) = fut.await {
                tracing::error!("{context}: {err:?}");
            }
        });
    }

    async fn bootstrap_instruments(&mut self) -> Result<Vec<InstrumentAny>> {
        let mut instruments = self.http_client.request_instruments().await?;

        tracing::debug!(
            count = instruments.len(),
            "Received Hyperliquid instruments"
        );
        instruments.sort_by_key(|instrument| instrument.id());

        tracing::info!(count = instruments.len(), "Loaded Hyperliquid instruments");

        // Update cache
        {
            let mut guard = self
                .instruments
                .write()
                .expect("instrument cache lock poisoned");
            guard.clear();
            for instrument in &instruments {
                guard.insert(instrument.id(), instrument.clone());
            }
        }

        Ok(instruments)
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
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
        tracing::info!("Starting Hyperliquid data client {id}", id = self.client_id);
        Ok(())
    }

    fn stop(&mut self) -> Result<()> {
        tracing::info!("Stopping Hyperliquid data client {id}", id = self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> Result<()> {
        tracing::debug!(
            "Resetting Hyperliquid data client {id}",
            id = self.client_id
        );
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> Result<()> {
        tracing::debug!(
            "Disposing Hyperliquid data client {id}",
            id = self.client_id
        );
        self.stop()
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }

    async fn connect(&mut self) -> Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        self.bootstrap_instruments().await?;

        // Initialize WebSocket client on connect
        self.ws_client = Some(
            HyperliquidWebSocketClient::connect(&self.config.ws_url())
                .await
                .context("failed to connect Hyperliquid websocket")?,
        );

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!(
            "Connected Hyperliquid data client {id}",
            id = self.client_id
        );

        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        // WebSocket client will be dropped when set to None
        self.ws_client = None;

        self.is_connected.store(false, Ordering::Relaxed);
        tracing::info!(
            "Disconnected Hyperliquid data client {id}",
            id = self.client_id
        );

        Ok(())
    }

    fn subscribe_trades(&mut self, cmd: &SubscribeTrades) -> Result<()> {
        tracing::debug!("Subscribing to trades for {}", cmd.instrument_id);
        // WebSocket trade subscription implementation pending
        Ok(())
    }

    fn subscribe_quotes(&mut self, cmd: &SubscribeQuotes) -> Result<()> {
        tracing::debug!("Subscribing to quotes for {}", cmd.instrument_id);
        // WebSocket quote subscription implementation pending
        Ok(())
    }

    fn subscribe_book_deltas(&mut self, cmd: &SubscribeBookDeltas) -> Result<()> {
        tracing::debug!("Subscribing to book deltas for {}", cmd.instrument_id);
        // WebSocket book delta subscription implementation pending
        Ok(())
    }

    fn subscribe_book_snapshots(&mut self, cmd: &SubscribeBookSnapshots) -> Result<()> {
        tracing::debug!("Subscribing to book snapshots for {}", cmd.instrument_id);
        // WebSocket book snapshot subscription implementation pending
        Ok(())
    }

    fn subscribe_bars(&mut self, cmd: &SubscribeBars) -> Result<()> {
        tracing::debug!("Subscribing to bars for {}", cmd.bar_type);
        // WebSocket bar subscription implementation pending
        Ok(())
    }

    fn subscribe_funding_rates(&mut self, cmd: &SubscribeFundingRates) -> Result<()> {
        tracing::debug!("Subscribing to funding rates for {}", cmd.instrument_id);
        // WebSocket funding rate subscription implementation pending
        Ok(())
    }

    fn subscribe_mark_prices(&mut self, cmd: &SubscribeMarkPrices) -> Result<()> {
        tracing::debug!("Subscribing to mark prices for {}", cmd.instrument_id);
        // WebSocket mark price subscription implementation pending
        Ok(())
    }

    fn subscribe_index_prices(&mut self, cmd: &SubscribeIndexPrices) -> Result<()> {
        tracing::debug!("Subscribing to index prices for {}", cmd.instrument_id);
        // WebSocket index price subscription implementation pending
        Ok(())
    }

    fn unsubscribe_trades(&mut self, cmd: &UnsubscribeTrades) -> Result<()> {
        tracing::debug!("Unsubscribing from trades for {}", cmd.instrument_id);
        // WebSocket trade unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_quotes(&mut self, cmd: &UnsubscribeQuotes) -> Result<()> {
        tracing::debug!("Unsubscribing from quotes for {}", cmd.instrument_id);
        // WebSocket quote unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_book_deltas(&mut self, cmd: &UnsubscribeBookDeltas) -> Result<()> {
        tracing::debug!("Unsubscribing from book deltas for {}", cmd.instrument_id);
        // WebSocket book delta unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_book_snapshots(&mut self, cmd: &UnsubscribeBookSnapshots) -> Result<()> {
        tracing::debug!(
            "Unsubscribing from book snapshots for {}",
            cmd.instrument_id
        );
        // WebSocket book snapshot unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_bars(&mut self, cmd: &UnsubscribeBars) -> Result<()> {
        tracing::debug!("Unsubscribing from bars for {}", cmd.bar_type);
        // WebSocket bar unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_funding_rates(&mut self, cmd: &UnsubscribeFundingRates) -> Result<()> {
        tracing::debug!("Unsubscribing from funding rates for {}", cmd.instrument_id);
        // WebSocket funding rate unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_mark_prices(&mut self, cmd: &UnsubscribeMarkPrices) -> Result<()> {
        tracing::debug!("Unsubscribing from mark prices for {}", cmd.instrument_id);
        // WebSocket mark price unsubscription implementation pending
        Ok(())
    }

    fn unsubscribe_index_prices(&mut self, cmd: &UnsubscribeIndexPrices) -> Result<()> {
        tracing::debug!("Unsubscribing from index prices for {}", cmd.instrument_id);
        // WebSocket index price unsubscription implementation pending
        Ok(())
    }

    fn request_instruments(&self, request: &RequestInstruments) -> Result<()> {
        tracing::debug!("Requesting instruments");
        let instruments = {
            let guard = self
                .instruments
                .read()
                .expect("instrument cache lock poisoned");
            guard.values().cloned().collect::<Vec<_>>()
        };

        let response = DataResponse::Instruments(InstrumentsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            self.venue(),
            instruments,
            request
                .start
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            request
                .end
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send instruments response: {err}");
        }

        Ok(())
    }

    fn request_instrument(&self, request: &RequestInstrument) -> Result<()> {
        tracing::debug!("Requesting instrument for {}", request.instrument_id);
        let guard = self
            .instruments
            .read()
            .expect("instrument cache lock poisoned");

        let instrument = guard.get(&request.instrument_id).cloned();

        match instrument {
            Some(instr) => {
                let response = DataResponse::Instrument(Box::new(InstrumentResponse::new(
                    request.request_id,
                    request.client_id.unwrap_or(self.client_id),
                    request.instrument_id,
                    instr,
                    request
                        .start
                        .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
                    request
                        .end
                        .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
                    self.clock.get_time_ns(),
                    request.params.clone(),
                )));

                if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
                    tracing::error!("Failed to send instrument response: {err}");
                }
            }
            None => {
                tracing::warn!("Instrument {} not found", request.instrument_id);
                // For now, we don't send a response for missing instruments
                // Consider sending an error response in future enhancement
            }
        }

        Ok(())
    }

    fn request_trades(&self, request: &RequestTrades) -> Result<()> {
        tracing::debug!("Requesting trades for {}", request.instrument_id);
        // Historical trade request implementation pending
        let response = DataResponse::Trades(TradesResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            request.instrument_id,
            Vec::new(),
            request
                .start
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            request
                .end
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send trades response: {err}");
        }

        Ok(())
    }

    fn request_bars(&self, request: &RequestBars) -> Result<()> {
        tracing::debug!("Requesting bars for {}", request.bar_type);
        // Historical bar request implementation pending
        let response = DataResponse::Bars(BarsResponse::new(
            request.request_id,
            request.client_id.unwrap_or(self.client_id),
            request.bar_type,
            Vec::new(),
            request
                .start
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            request
                .end
                .map(|dt| UnixNanos::from(dt.timestamp_nanos_opt().unwrap_or(0) as u64)),
            self.clock.get_time_ns(),
            request.params.clone(),
        ));

        if let Err(err) = self.data_sender.send(DataEvent::Response(response)) {
            tracing::error!("Failed to send bars response: {err}");
        }

        Ok(())
    }
}
