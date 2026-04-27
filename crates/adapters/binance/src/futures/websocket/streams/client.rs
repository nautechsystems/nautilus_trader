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

//! Binance Futures WebSocket client for JSON market data streams.
//!
//! ## Connection Details
//!
//! - USD-M Endpoint: `wss://fstream.binance.com/market/ws`
//! - COIN-M Endpoint: `wss://dstream.binance.com/ws`
//! - Max streams: 200 per connection
//! - Max connections: 20 per pool (up to 4,000 total streams)
//! - Connection validity: 24 hours
//! - Ping/pong: Every 3 minutes

use std::{
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use futures_util::Stream;
use nautilus_common::live::get_runtime;
use nautilus_core::{AtomicMap, string::secret::REDACTED};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        PingHandler, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::{BinanceWsError, BinanceWsResult},
    handler::BinanceFuturesDataWsFeedHandler,
    messages::{BinanceFuturesWsStreamsCommand, BinanceFuturesWsStreamsMessage},
};
use crate::common::{
    consts::{
        BINANCE_API_KEY_HEADER, BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION, BINANCE_WS_CONNECTION_QUOTA,
        BINANCE_WS_SUBSCRIPTION_QUOTA,
    },
    credential::SigningCredential,
    enums::{BinanceEnvironment, BinanceProductType},
    urls::get_ws_base_url,
};

/// Maximum streams per WebSocket connection for Futures.
pub const MAX_STREAMS_PER_CONNECTION: usize = 200;

/// Maximum connections per pool.
const MAX_CONNECTIONS: usize = 20;

// State for a single WebSocket connection within the pool
struct ConnectionSlot {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsStreamsCommand>,
    streams: Vec<String>,
    subscriptions_state: SubscriptionState,
    handler_task: tokio::task::JoinHandle<()>,
    bytes_task: tokio::task::JoinHandle<()>,
    cancellation_token: CancellationToken,
    connection_mode: Arc<AtomicU8>,
}

/// Binance Futures WebSocket client for JSON market data streams.
///
/// Manages a pool of up to 20 connections, each supporting up to 200 streams.
/// New connections are created automatically when subscribing exceeds the current
/// connection's stream limit. All connections feed into a single output stream,
/// transparent to the data client.
#[derive(Clone)]
pub struct BinanceFuturesWebSocketClient {
    url: String,
    product_type: BinanceProductType,
    credential: Option<Arc<SigningCredential>>,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    slots: Arc<Mutex<Vec<ConnectionSlot>>>,
    out_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsStreamsMessage>>>>,
    out_rx:
        Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsStreamsMessage>>>>,
    request_id_counter: Arc<AtomicU64>,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    transport_backend: TransportBackend,
}

impl Debug for BinanceFuturesWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesWebSocketClient))
            .field("url", &self.url)
            .field("product_type", &self.product_type)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesWebSocketClient {
    /// Creates a new [`BinanceFuturesWebSocketClient`] instance.
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
        transport_backend: TransportBackend,
    ) -> anyhow::Result<Self> {
        match product_type {
            BinanceProductType::UsdM | BinanceProductType::CoinM => {}
            _ => {
                anyhow::bail!(
                    "BinanceFuturesWebSocketClient requires UsdM or CoinM product type, was {product_type:?}"
                );
            }
        }

        let url =
            url_override.unwrap_or_else(|| get_ws_base_url(product_type, environment).to_string());

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Arc::new(SigningCredential::new(key, secret))),
            _ => None,
        };

        Ok(Self {
            url,
            product_type,
            credential,
            heartbeat,
            signal: Arc::new(AtomicBool::new(false)),
            slots: Arc::new(Mutex::new(Vec::new())),
            out_tx: Arc::new(Mutex::new(None)),
            out_rx: Arc::new(Mutex::new(None)),
            request_id_counter: Arc::new(AtomicU64::new(1)),
            instruments_cache: Arc::new(AtomicMap::new()),
            transport_backend,
        })
    }

    /// Returns the product type (UsdM or CoinM).
    #[must_use]
    pub const fn product_type(&self) -> BinanceProductType {
        self.product_type
    }

    /// Returns whether any connection in the pool is active.
    #[must_use]
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn is_active(&self) -> bool {
        let slots = self.slots.lock().expect("slots lock poisoned");
        slots
            .iter()
            .any(|s| s.connection_mode.load(Ordering::Relaxed) == ConnectionMode::Active as u8)
    }

    /// Returns whether all connections in the pool are closed.
    #[must_use]
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn is_closed(&self) -> bool {
        let slots = self.slots.lock().expect("slots lock poisoned");
        slots.is_empty()
            || slots
                .iter()
                .all(|s| s.connection_mode.load(Ordering::Relaxed) == ConnectionMode::Closed as u8)
    }

    /// Returns the total number of confirmed subscriptions across all connections.
    #[must_use]
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn subscription_count(&self) -> usize {
        let slots = self.slots.lock().expect("slots lock poisoned");
        slots.iter().map(|s| s.subscriptions_state.len()).sum()
    }

    /// Connects the first WebSocket connection in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn connect(&mut self) -> BinanceWsResult<()> {
        self.signal.store(false, Ordering::Relaxed);

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self.out_tx.lock().expect("out_tx lock poisoned") = Some(out_tx);
        *self.out_rx.lock().expect("out_rx lock poisoned") = Some(out_rx);

        let slot = self.create_connection().await?;
        self.slots.lock().expect("slots lock poisoned").push(slot);

        log::info!(
            "Connected to Binance Futures stream pool: url={}, product_type={:?}",
            self.url,
            self.product_type
        );
        Ok(())
    }

    /// Closes all WebSocket connections in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnect fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn close(&mut self) -> BinanceWsResult<()> {
        self.signal.store(true, Ordering::Relaxed);

        let slots: Vec<ConnectionSlot> = {
            let mut guard = self.slots.lock().expect("slots lock poisoned");
            guard.drain(..).collect()
        };

        for slot in slots {
            slot.cancellation_token.cancel();
            let _ = slot.cmd_tx.send(BinanceFuturesWsStreamsCommand::Disconnect);
            let _ = slot.handler_task.await;
            slot.bytes_task.abort();
        }

        *self.out_tx.lock().expect("out_tx lock poisoned") = None;
        *self.out_rx.lock().expect("out_rx lock poisoned") = None;

        log::info!("Disconnected from Binance Futures stream pool");
        Ok(())
    }

    /// Subscribes to the specified streams.
    ///
    /// Streams are distributed across pool connections. New connections are created
    /// automatically when existing ones reach the 200-stream limit, up to a maximum
    /// of 20 connections.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool is exhausted or command delivery fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn subscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        // Phase 1: filter already-subscribed streams (brief lock)
        let new_streams: Vec<String> = {
            let slots = self.slots.lock().expect("slots lock poisoned");
            streams
                .into_iter()
                .filter(|s| !slots.iter().any(|slot| slot.streams.contains(s)))
                .collect()
        };

        if new_streams.is_empty() {
            return Ok(());
        }

        // Phase 2: create connections if needed (no lock held during async connect)
        loop {
            let (remaining_capacity, slot_count) = {
                let slots = self.slots.lock().expect("slots lock poisoned");
                let cap: usize = slots
                    .iter()
                    .map(|s| MAX_STREAMS_PER_CONNECTION - s.streams.len())
                    .sum();
                (cap, slots.len())
            };

            if remaining_capacity >= new_streams.len() || slot_count >= MAX_CONNECTIONS {
                break;
            }

            let new_slot = self.create_connection().await?;
            let slot_count = {
                let mut slots = self.slots.lock().expect("slots lock poisoned");
                slots.push(new_slot);
                slots.len()
            };
            log::info!(
                "Pool slot {} connected: url={}, product_type={:?}",
                slot_count - 1,
                self.url,
                self.product_type
            );
        }

        // Phase 3: assign streams to slots and send commands (brief lock).
        // Stage assignments first so a capacity error leaves slots unchanged.
        let mut slots = self.slots.lock().expect("slots lock poisoned");
        let mut slot_batches: Vec<(usize, Vec<String>)> = Vec::new();
        let mut slot_counts: Vec<usize> = slots.iter().map(|s| s.streams.len()).collect();

        for stream in &new_streams {
            let slot_idx = slot_counts
                .iter()
                .position(|&count| count < MAX_STREAMS_PER_CONNECTION)
                .ok_or_else(|| {
                    let max_total = MAX_CONNECTIONS * MAX_STREAMS_PER_CONNECTION;
                    BinanceWsError::ClientError(format!(
                        "Pool exhausted: {max_total} total subscriptions \
                         ({MAX_CONNECTIONS} connections x {MAX_STREAMS_PER_CONNECTION} streams)"
                    ))
                })?;

            slot_counts[slot_idx] += 1;

            if let Some(batch) = slot_batches.iter_mut().find(|(i, _)| *i == slot_idx) {
                batch.1.push(stream.clone());
            } else {
                slot_batches.push((slot_idx, vec![stream.clone()]));
            }
        }

        // Send commands first; only update slot state on success
        for (slot_idx, batch) in &slot_batches {
            slots[*slot_idx]
                .cmd_tx
                .send(BinanceFuturesWsStreamsCommand::Subscribe {
                    streams: batch.clone(),
                })
                .map_err(|e| {
                    BinanceWsError::ClientError(format!(
                        "Handler not available for pool slot {slot_idx}: {e}"
                    ))
                })?;
            slots[*slot_idx].streams.extend(batch.iter().cloned());
        }

        Ok(())
    }

    /// Unsubscribes from the specified streams.
    ///
    /// # Errors
    ///
    /// Returns an error if command delivery fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn unsubscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        let mut slots = self.slots.lock().expect("slots lock poisoned");
        let mut slot_batches: Vec<(usize, Vec<String>)> = Vec::new();

        for stream in &streams {
            if let Some(slot_idx) = slots.iter().position(|s| s.streams.contains(stream)) {
                if let Some(batch) = slot_batches.iter_mut().find(|(i, _)| *i == slot_idx) {
                    batch.1.push(stream.clone());
                } else {
                    slot_batches.push((slot_idx, vec![stream.clone()]));
                }
            }
        }

        // Send commands first; only update slot state on success
        for (slot_idx, batch) in &slot_batches {
            slots[*slot_idx]
                .cmd_tx
                .send(BinanceFuturesWsStreamsCommand::Unsubscribe {
                    streams: batch.clone(),
                })
                .map_err(|e| {
                    BinanceWsError::ClientError(format!(
                        "Handler not available for pool slot {slot_idx}: {e}"
                    ))
                })?;

            for stream in batch {
                slots[*slot_idx].streams.retain(|s| s != stream);
            }
        }

        Ok(())
    }

    /// Returns a stream of messages from all WebSocket connections.
    ///
    /// This method can only be called once per connection lifecycle. Subsequent calls
    /// return an empty stream.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub fn stream(&self) -> impl Stream<Item = BinanceFuturesWsStreamsMessage> + 'static {
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
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments_cache.rcu(|m| {
            for inst in instruments {
                m.insert(inst.raw_symbol().inner(), inst.clone());
            }
        });
    }

    /// Update a single instrument in the cache.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.raw_symbol().inner(), instrument);
    }

    /// Returns a shared reference to the instruments cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<AtomicMap<Ustr, InstrumentAny>> {
        self.instruments_cache.clone()
    }

    /// Returns an instrument from the cache by raw symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(&Ustr::from(symbol))
    }

    async fn create_connection(&self) -> BinanceWsResult<ConnectionSlot> {
        let out_tx = self
            .out_tx
            .lock()
            .expect("out_tx lock poisoned")
            .clone()
            .ok_or_else(|| {
                BinanceWsError::ClientError("Output channel not initialized".to_string())
            })?;

        let (raw_handler, raw_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        let headers = if let Some(ref cred) = self.credential {
            vec![(
                BINANCE_API_KEY_HEADER.to_string(),
                cred.api_key().to_string(),
            )]
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
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: None,
        };

        let keyed_quotas = vec![(
            BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION[0].as_str().to_string(),
            *BINANCE_WS_SUBSCRIPTION_QUOTA,
        )];

        let client = WebSocketClient::connect(
            config,
            Some(raw_handler),
            Some(ping_handler),
            None,
            keyed_quotas,
            Some(*BINANCE_WS_CONNECTION_QUOTA),
        )
        .await
        .map_err(|e| BinanceWsError::NetworkError(e.to_string()))?;

        let connection_mode = client.connection_mode_atomic();
        let subscriptions_state = SubscriptionState::new('@');
        let cancellation_token = CancellationToken::new();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        // Convert raw Message frames to Vec<u8> for the JSON handler
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

        let mut handler = BinanceFuturesDataWsFeedHandler::new(
            self.signal.clone(),
            cmd_rx,
            bytes_rx,
            out_tx.clone(),
            subscriptions_state.clone(),
            self.request_id_counter.clone(),
        );

        cmd_tx
            .send(BinanceFuturesWsStreamsCommand::SetClient(client))
            .map_err(|e| BinanceWsError::ClientError(format!("Failed to set client: {e}")))?;

        let signal = self.signal.clone();
        let token = cancellation_token.clone();
        let subs = subscriptions_state.clone();
        let resubscribe_tx = cmd_tx.clone();

        let handler_task = get_runtime().spawn(async move {
            loop {
                tokio::select! {
                    () = token.cancelled() => {
                        log::debug!("Handler task cancelled");
                        break;
                    }
                    result = handler.next() => {
                        match result {
                            Some(BinanceFuturesWsStreamsMessage::Reconnected) => {
                                log::info!("WebSocket reconnected, restoring subscriptions");
                                let all_topics = subs.all_topics();
                                for topic in &all_topics {
                                    subs.mark_failure(topic);
                                }

                                let streams = subs.all_topics();
                                if !streams.is_empty()
                                    && let Err(e) = resubscribe_tx.send(BinanceFuturesWsStreamsCommand::Subscribe { streams }) {
                                        log::error!("Failed to resubscribe after reconnect: {e}");
                                    }

                                if out_tx.send(BinanceFuturesWsStreamsMessage::Reconnected).is_err() {
                                    log::debug!("Output channel closed");
                                    break;
                                }
                            }
                            Some(msg) => {
                                if out_tx.send(msg).is_err() {
                                    log::debug!("Output channel closed");
                                    break;
                                }
                            }
                            None => {
                                if signal.load(Ordering::Relaxed) {
                                    log::debug!("Handler received shutdown signal");
                                } else {
                                    log::warn!("Handler loop ended unexpectedly");
                                }
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(ConnectionSlot {
            cmd_tx,
            streams: Vec::new(),
            subscriptions_state,
            handler_task,
            bytes_task,
            cancellation_token,
            connection_mode,
        })
    }
}
