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

//! Binance Futures WebSocket client for JSON market data streams.
//!
//! ## Connection Details
//!
//! - USD-M Endpoint: `wss://fstream.binance.com/ws`
//! - COIN-M Endpoint: `wss://dstream.binance.com/ws`
//! - Max streams: 200 per connection
//! - Connection validity: 24 hours
//! - Ping/pong: Every 3 minutes

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::get_runtime;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        PingHandler, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    handler::BinanceFuturesWsFeedHandler,
    messages::{BinanceFuturesHandlerCommand, NautilusFuturesWsMessage},
};
use crate::{
    common::{
        credential::Credential,
        enums::{BinanceEnvironment, BinanceProductType},
        urls::get_ws_base_url,
    },
    websocket::error::{BinanceWsError, BinanceWsResult},
};

/// Maximum streams per WebSocket connection for Futures.
pub const MAX_STREAMS_PER_CONNECTION: usize = 200;

/// Binance Futures WebSocket client for JSON market data streams.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance")
)]
pub struct BinanceFuturesWebSocketClient {
    url: String,
    product_type: BinanceProductType,
    credential: Option<Arc<Credential>>,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx:
        Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<BinanceFuturesHandlerCommand>>>,
    out_rx: Arc<
        std::sync::Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<NautilusFuturesWsMessage>>>,
    >,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions_state: SubscriptionState,
    request_id_counter: Arc<AtomicU64>,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    cancellation_token: CancellationToken,
}

impl Debug for BinanceFuturesWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesWebSocketClient))
            .field("url", &self.url)
            .field("product_type", &self.product_type)
            .field(
                "credential",
                &self.credential.as_ref().map(|_| "<redacted>"),
            )
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesWebSocketClient {
    /// Creates a new [`BinanceFuturesWebSocketClient`] instance.
    ///
    /// # Arguments
    ///
    /// * `product_type` - Must be `UsdM` or `CoinM`.
    /// * `environment` - Mainnet or testnet.
    /// * `api_key` - Optional API key for user data streams.
    /// * `api_secret` - Optional API secret for signing.
    /// * `heartbeat` - Optional heartbeat interval in milliseconds.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - `product_type` is not a futures type (UsdM or CoinM).
    /// - Credential creation fails.
    pub fn new(
        product_type: BinanceProductType,
        environment: BinanceEnvironment,
        api_key: Option<String>,
        api_secret: Option<String>,
        url_override: Option<String>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        match product_type {
            BinanceProductType::UsdM | BinanceProductType::CoinM => {}
            _ => {
                anyhow::bail!(
                    "BinanceFuturesWebSocketClient requires UsdM or CoinM product type, got {product_type:?}"
                );
            }
        }

        let url =
            url_override.unwrap_or_else(|| get_ws_base_url(product_type, environment).to_string());

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Arc::new(Credential::new(key, secret))),
            _ => None,
        };

        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        Ok(Self {
            url,
            product_type,
            credential,
            heartbeat,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode: Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
                ConnectionMode::Closed as u8,
            )))),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: Arc::new(std::sync::Mutex::new(None)),
            task_handle: None,
            subscriptions_state: SubscriptionState::new('@'),
            request_id_counter: Arc::new(AtomicU64::new(1)),
            instruments_cache: Arc::new(DashMap::new()),
            cancellation_token: CancellationToken::new(),
        })
    }

    /// Returns the product type (UsdM or CoinM).
    #[must_use]
    pub const fn product_type(&self) -> BinanceProductType {
        self.product_type
    }

    /// Returns whether the client is actively connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let mode_u8 = self.connection_mode.load().load(Ordering::Relaxed);
        mode_u8 == ConnectionMode::Active as u8
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let mode_u8 = self.connection_mode.load().load(Ordering::Relaxed);
        mode_u8 == ConnectionMode::Closed as u8
    }

    /// Returns the number of confirmed subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions_state.len()
    }

    /// Connects to the WebSocket server.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub async fn connect(&mut self) -> BinanceWsResult<()> {
        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        // Build headers for HMAC authentication (if needed for user data streams)
        let headers = if let Some(ref cred) = self.credential {
            vec![("X-MBX-APIKEY".to_string(), cred.api_key().to_string())]
        } else {
            vec![]
        };

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(
            config,
            Some(raw_handler),
            Some(ping_handler),
            None,
            vec![],
            None,
        )
        .await
        .map_err(|e| BinanceWsError::NetworkError(e.to_string()))?;

        self.connection_mode.store(client.connection_mode_atomic());

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self.cmd_tx.write().await = cmd_tx;
        *self.out_rx.lock().expect("out_rx lock poisoned") = Some(out_rx);

        let (bytes_tx, bytes_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        let bytes_task = get_runtime().spawn(async move {
            let mut raw_rx = raw_rx;
            while let Some(msg) = raw_rx.recv().await {
                let data = match msg {
                    Message::Binary(data) => data.to_vec(),
                    Message::Text(text) => text.as_bytes().to_vec(),
                    Message::Close(_) => break,
                    Message::Ping(_) | Message::Pong(_) | Message::Frame(_) => continue,
                };
                if bytes_tx.send(data).is_err() {
                    break;
                }
            }
        });

        let mut handler = BinanceFuturesWsFeedHandler::new(
            self.signal.clone(),
            cmd_rx,
            bytes_rx,
            out_tx.clone(),
            self.subscriptions_state.clone(),
            self.request_id_counter.clone(),
        );

        self.cmd_tx
            .read()
            .await
            .send(BinanceFuturesHandlerCommand::SetClient(client))
            .map_err(|e| BinanceWsError::ClientError(format!("Failed to set client: {e}")))?;

        let instruments: Vec<InstrumentAny> = self
            .instruments_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        if !instruments.is_empty() {
            self.cmd_tx
                .read()
                .await
                .send(BinanceFuturesHandlerCommand::InitializeInstruments(
                    instruments,
                ))
                .map_err(|e| {
                    BinanceWsError::ClientError(format!("Failed to initialize instruments: {e}"))
                })?;
        }

        let signal = self.signal.clone();
        let cancellation_token = self.cancellation_token.clone();
        let subscriptions_state = self.subscriptions_state.clone();
        let cmd_tx = self.cmd_tx.clone();

        let task_handle = get_runtime().spawn(async move {
            loop {
                tokio::select! {
                    _ = cancellation_token.cancelled() => {
                        tracing::debug!("Handler task cancelled");
                        break;
                    }
                    result = handler.next() => {
                        match result {
                            Some(NautilusFuturesWsMessage::Reconnected) => {
                                tracing::info!("WebSocket reconnected, restoring subscriptions");
                                // Mark all confirmed subscriptions as pending
                                let all_topics = subscriptions_state.all_topics();
                                for topic in &all_topics {
                                    subscriptions_state.mark_failure(topic);
                                }

                                // Resubscribe using tracked subscription state
                                let streams = subscriptions_state.all_topics();
                                if !streams.is_empty()
                                    && let Err(e) = cmd_tx.read().await.send(BinanceFuturesHandlerCommand::Subscribe { streams }) {
                                        tracing::error!(error = %e, "Failed to resubscribe after reconnect");
                                    }

                                if out_tx.send(NautilusFuturesWsMessage::Reconnected).is_err() {
                                    tracing::debug!("Output channel closed");
                                    break;
                                }
                            }
                            Some(msg) => {
                                if out_tx.send(msg).is_err() {
                                    tracing::debug!("Output channel closed");
                                    break;
                                }
                            }
                            None => {
                                if signal.load(Ordering::Relaxed) {
                                    tracing::debug!("Handler received shutdown signal");
                                } else {
                                    tracing::warn!("Handler loop ended unexpectedly");
                                }
                                break;
                            }
                        }
                    }
                }
            }
            bytes_task.abort();
        });

        self.task_handle = Some(Arc::new(task_handle));

        tracing::info!(url = %self.url, product_type = ?self.product_type, "Connected to Binance Futures stream");
        Ok(())
    }

    /// Closes the WebSocket connection.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnect fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub async fn close(&mut self) -> BinanceWsResult<()> {
        self.signal.store(true, Ordering::Relaxed);
        self.cancellation_token.cancel();

        let _ = self
            .cmd_tx
            .read()
            .await
            .send(BinanceFuturesHandlerCommand::Disconnect);

        if let Some(handle) = self.task_handle.take()
            && let Ok(handle) = Arc::try_unwrap(handle)
        {
            let _ = handle.await;
        }

        *self.out_rx.lock().expect("out_rx lock poisoned") = None;

        tracing::info!("Disconnected from Binance Futures stream");
        Ok(())
    }

    /// Subscribes to the specified streams.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails or would exceed stream limit.
    pub async fn subscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        let current_count = self.subscriptions_state.len();
        if current_count + streams.len() > MAX_STREAMS_PER_CONNECTION {
            return Err(BinanceWsError::ClientError(format!(
                "Would exceed max streams: {} + {} > {}",
                current_count,
                streams.len(),
                MAX_STREAMS_PER_CONNECTION
            )));
        }

        self.cmd_tx
            .read()
            .await
            .send(BinanceFuturesHandlerCommand::Subscribe { streams })
            .map_err(|e| BinanceWsError::ClientError(format!("Handler not available: {e}")))?;

        Ok(())
    }

    /// Unsubscribes from the specified streams.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        self.cmd_tx
            .read()
            .await
            .send(BinanceFuturesHandlerCommand::Unsubscribe { streams })
            .map_err(|e| BinanceWsError::ClientError(format!("Handler not available: {e}")))?;

        Ok(())
    }

    /// Returns a stream of messages from the WebSocket.
    ///
    /// This method can only be called once per connection. Subsequent calls
    /// will return an empty stream. If you need to consume messages from
    /// multiple tasks, clone the client before connecting.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub fn stream(&self) -> impl Stream<Item = NautilusFuturesWsMessage> + 'static {
        let out_rx = self.out_rx.lock().expect("out_rx lock poisoned").take();
        async_stream::stream! {
            if let Some(mut rx) = out_rx {
                while let Some(msg) = rx.recv().await {
                    yield msg;
                }
            }
        }
    }

    /// Bulk initialize the instrument cache.
    ///
    /// Instruments are cached by their raw symbol (e.g., "BTCUSDT") to match
    /// the symbol format sent in WebSocket messages.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        for inst in &instruments {
            self.instruments_cache
                .insert(inst.raw_symbol().inner(), inst.clone());
        }

        if self.is_active() {
            let cmd_tx = self.cmd_tx.clone();
            let instruments_clone = instruments;
            get_runtime().spawn(async move {
                let _ =
                    cmd_tx
                        .read()
                        .await
                        .send(BinanceFuturesHandlerCommand::InitializeInstruments(
                            instruments_clone,
                        ));
            });
        }
    }

    /// Update a single instrument in the cache.
    ///
    /// Instruments are cached by their raw symbol (e.g., "BTCUSDT") to match
    /// the symbol format sent in WebSocket messages.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument.clone());

        if self.is_active() {
            let cmd_tx = self.cmd_tx.clone();
            get_runtime().spawn(async move {
                let _ = cmd_tx
                    .read()
                    .await
                    .send(BinanceFuturesHandlerCommand::UpdateInstrument(instrument));
            });
        }
    }

    /// Get an instrument from the cache.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instruments_cache
            .get(&Ustr::from(symbol))
            .map(|entry| entry.value().clone())
    }
}
