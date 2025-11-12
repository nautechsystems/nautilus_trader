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

//! Live market data client implementation for the dYdX adapter.

use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use dashmap::DashMap;
use nautilus_common::{messages::DataEvent, runner::get_data_event_sender};
use nautilus_core::time::{AtomicTime, get_atomic_clock_realtime};
use nautilus_data::client::DataClient;
use nautilus_model::{
    data::{Data as NautilusData, OrderBookDeltas_API},
    identifiers::{ClientId, Venue},
    instruments::InstrumentAny,
};
use tokio::{task::JoinHandle, time::Duration};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::consts::DYDX_VENUE, config::DydxDataClientConfig, http::client::DydxHttpClient,
    websocket::client::DydxWebSocketClient,
};

/// dYdX data client for live market data streaming and historical data requests.
///
/// This client integrates with the Nautilus DataEngine to provide:
/// - Real-time market data via WebSocket subscriptions
/// - Historical data via REST API requests
/// - Automatic instrument discovery and caching
/// - Connection lifecycle management
#[derive(Debug)]
pub struct DydxDataClient {
    /// The client ID for this data client.
    client_id: ClientId,
    /// Configuration for the data client.
    _config: DydxDataClientConfig,
    /// HTTP client for REST API requests.
    http_client: DydxHttpClient,
    /// WebSocket client for real-time data streaming (optional).
    ws_client: Option<DydxWebSocketClient>,
    /// Whether the client is currently connected.
    is_connected: AtomicBool,
    /// Cancellation token for async operations.
    cancellation_token: CancellationToken,
    /// Background task handles.
    tasks: Vec<JoinHandle<()>>,
    /// Channel sender for emitting data events to the DataEngine.
    _data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
    /// Cached instruments by symbol (shared with HTTP client via `Arc<DashMap<Ustr, InstrumentAny>>`).
    instruments: Arc<DashMap<Ustr, InstrumentAny>>,
    /// High-resolution clock for timestamps.
    _clock: &'static AtomicTime,
}

impl DydxDataClient {
    /// Creates a new [`DydxDataClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(
        client_id: ClientId,
        config: DydxDataClientConfig,
        http_client: DydxHttpClient,
        ws_client: Option<DydxWebSocketClient>,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let data_sender = get_data_event_sender();

        // Clone the instruments cache before moving http_client
        let instruments_cache = http_client.instruments().clone();

        Ok(Self {
            client_id,
            _config: config,
            http_client,
            ws_client,
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            tasks: Vec::new(),
            _data_sender: data_sender,
            instruments: instruments_cache,
            _clock: clock,
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
        tracing::info!("Bootstrapping dYdX instruments");

        // Fetch instruments from HTTP API
        // Note: maker_fee and taker_fee can be None initially - they'll be set to zero
        let instruments = self
            .http_client
            .request_instruments(None, None, None)
            .await
            .context("failed to load instruments from dYdX")?;

        if instruments.is_empty() {
            tracing::warn!("No dYdX instruments were loaded");
            return Ok(instruments);
        }

        tracing::info!("Loaded {} dYdX instruments", instruments.len());

        // Instruments are already cached in HTTP client by request_instruments()
        // No need to cache again - the cache is shared via Arc<DashMap>

        // Cache in WebSocket client if present
        if let Some(ref ws) = self.ws_client {
            ws.cache_instruments(instruments.clone());
        }

        Ok(instruments)
    }
}

// Implement DataClient trait for integration with Nautilus DataEngine
#[async_trait::async_trait]
impl DataClient for DydxDataClient {
    fn client_id(&self) -> ClientId {
        self.client_id
    }

    fn venue(&self) -> Option<Venue> {
        Some(*DYDX_VENUE)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        tracing::info!(
            client_id = %self.client_id,
            is_testnet = self.http_client.is_testnet(),
            "Starting dYdX data client"
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        tracing::info!("Stopping dYdX data client {}", self.client_id);
        self.cancellation_token.cancel();
        self.is_connected.store(false, Ordering::Relaxed);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Resetting dYdX data client {}", self.client_id);
        self.is_connected.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();
        self.tasks.clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        tracing::debug!("Disposing dYdX data client {}", self.client_id);
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        tracing::info!("Connecting dYdX data client");

        // Bootstrap instruments first
        self.bootstrap_instruments().await?;

        // Connect WebSocket client and subscribe to market updates
        if let Some(ref mut ws) = self.ws_client {
            ws.connect()
                .await
                .context("failed to connect dYdX websocket")?;

            ws.subscribe_markets()
                .await
                .context("failed to subscribe to markets channel")?;

            // Start message processing task (handler already converts to NautilusWsMessage)
            if let Some(rx) = ws.take_receiver() {
                let data_tx = self._data_sender.clone();

                let task = tokio::spawn(async move {
                    let mut rx = rx;
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            crate::websocket::messages::NautilusWsMessage::Data(items) => {
                                for d in items {
                                    let _ = data_tx.send(DataEvent::Data(d));
                                }
                            }
                            crate::websocket::messages::NautilusWsMessage::Deltas(deltas) => {
                                // Wrap OrderBookDeltas in the API wrapper expected by Data::Deltas
                                let data: NautilusData = OrderBookDeltas_API::new(*deltas).into();
                                let _ = data_tx.send(DataEvent::Data(data));
                            }
                            crate::websocket::messages::NautilusWsMessage::Error(err) => {
                                tracing::error!("dYdX WS error: {err}");
                            }
                            crate::websocket::messages::NautilusWsMessage::Reconnected => {
                                tracing::info!("dYdX WS reconnected");
                            }
                            _other => {
                                // TODO: handle orders/fills/positions/account state if used by DataClient
                            }
                        }
                    }
                });
                self.tasks.push(task);
            } else {
                tracing::warn!("No inbound WS receiver available after connect");
            }
        }

        self.is_connected.store(true, Ordering::Relaxed);
        tracing::info!("Connected dYdX data client");

        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        tracing::info!("Disconnecting dYdX data client");

        // Disconnect WebSocket client if present
        if let Some(ref mut ws) = self.ws_client {
            ws.disconnect()
                .await
                .context("failed to disconnect dYdX websocket")?;
        }

        self.is_connected.store(false, Ordering::Relaxed);
        tracing::info!("Disconnected dYdX data client");

        Ok(())
    }

    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Relaxed)
    }

    fn is_disconnected(&self) -> bool {
        !self.is_connected()
    }
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
    pub fn start_instrument_refresh_task(&mut self, interval: Duration) -> anyhow::Result<()> {
        if self.tasks.iter().any(|t| !t.is_finished()) {
            tracing::warn!("Instrument refresh task already running");
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let cancellation_token = self.cancellation_token.clone();

        let task = tokio::spawn(async move {
            tracing::info!(
                "Starting instrument refresh task (interval: {:?})",
                interval
            );

            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::info!("Instrument refresh task cancelled");
                        break;
                    }
                    _ = tokio::time::sleep(interval) => {
                        tracing::debug!("Refreshing instruments");

                        match http_client.request_instruments(None, None, None).await {
                            Ok(instruments) => {
                                tracing::debug!("Refreshed {} instruments", instruments.len());
                            }
                            Err(e) => {
                                tracing::error!("Failed to refresh instruments: {}", e);
                            }
                        }
                    }
                }
            }
        });

        self.tasks.push(task);
        Ok(())
    }

    /// Get a cached instrument by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instruments.get(&Ustr::from(symbol)).map(|i| i.clone())
    }

    /// Get all cached instruments.
    #[must_use]
    pub fn get_instruments(&self) -> Vec<InstrumentAny> {
        self.instruments.iter().map(|i| i.clone()).collect()
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////
#[cfg(test)]
mod tests {
    use nautilus_common::runner::set_data_event_sender;
    use nautilus_model::identifiers::ClientId;
    use rstest::rstest;

    use super::*;

    fn setup_test_env() {
        // Initialize data event sender for tests
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        set_data_event_sender(sender);
    }

    #[rstest]
    fn test_new_data_client() {
        setup_test_env();

        let client_id = ClientId::from("DYDX-001");
        let config = DydxDataClientConfig::default();
        let http_client = DydxHttpClient::default();

        let client = DydxDataClient::new(client_id, config, http_client, None);
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

        let mut client = DydxDataClient::new(client_id, config, http_client, None).unwrap();

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
}
