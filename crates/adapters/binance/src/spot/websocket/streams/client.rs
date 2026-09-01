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

//! Binance Spot WebSocket client for SBE market data streams.
//!
//! ## Connection Details
//!
//! - Endpoint: `stream-sbe.binance.com` or `stream-sbe.binance.com:9443`
//! - Authentication: Ed25519 API key in `X-MBX-APIKEY` header
//! - Max streams: 1024 per connection
//! - Max connections: 20 per pool (up to 20,480 total streams)
//! - Connection validity: 24 hours
//! - Ping/pong: Every 20 seconds

use std::{
    fmt::Debug,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use futures_util::Stream;
use nautilus_core::{AtomicMap, string::secret::REDACTED};
use nautilus_live::{
    SocketControl, SocketControlFactory,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        PingHandler, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    super::error::{BinanceWsError, BinanceWsResult},
    handler::BinanceSpotWsFeedHandler,
    messages::{BinanceSpotWsMessage, BinanceSpotWsStreamsCommand},
    subscription::{MAX_CONNECTIONS, MAX_STREAMS_PER_CONNECTION},
};
use crate::common::{
    consts::{
        BINANCE_API_KEY_HEADER, BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION, BINANCE_SPOT_SBE_WS_URL,
        BINANCE_WS_CONNECTION_QUOTA, BINANCE_WS_SUBSCRIPTION_QUOTA,
    },
    credential::Ed25519Credential,
};

// State for a single WebSocket connection within the pool
struct ConnectionSlot {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotWsStreamsCommand>,
    streams: Vec<String>,
    subscriptions_state: SubscriptionState,
    task_handle: TaskSlot<()>,
    cancellation_token: CancellationToken,
    connection_mode: Arc<AtomicU8>,
    socket_control: Option<SocketControl>,
    shutdown_errors: Vec<String>,
}

/// Binance Spot WebSocket client for SBE market data streams.
///
/// Manages a pool of up to 20 connections, each supporting up to 1024 streams.
/// New connections are created automatically when subscribing exceeds the current
/// connection's stream limit. All connections feed into a single output stream,
/// transparent to the data client.
#[derive(Clone)]
pub struct BinanceSpotWebSocketClient {
    url: String,
    credential: Option<Arc<Ed25519Credential>>,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    slots: Arc<ConnectionSlots>,
    connect_lock: Arc<tokio::sync::Mutex<()>>,
    out_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<BinanceSpotWsMessage>>>>,
    out_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<BinanceSpotWsMessage>>>>,
    request_id_counter: Arc<AtomicU64>,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    socket_factory: Option<SocketControlFactory>,
    socket_endpoint: Option<String>,
}

impl Debug for BinanceSpotWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotWebSocketClient))
            .field("url", &self.url)
            .field("credential", &self.credential.as_ref().map(|_| REDACTED))
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl Default for BinanceSpotWebSocketClient {
    fn default() -> Self {
        Self::new(None, None, None, None, TransportBackend::default()).unwrap()
    }
}

impl BinanceSpotWebSocketClient {
    /// Creates a new [`BinanceSpotWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if credential creation fails.
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
    ) -> anyhow::Result<Self> {
        let url = url.unwrap_or(BINANCE_SPOT_SBE_WS_URL.to_string());

        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => {
                let credential = Ed25519Credential::new(key, &secret).map_err(|e| {
                    anyhow::anyhow!(
                        "Binance Spot SBE market-data streams require an Ed25519 API key \
                         (HMAC keys are not supported): {e}"
                    )
                })?;
                Some(Arc::new(credential))
            }
            _ => None,
        };

        Ok(Self {
            url,
            credential,
            heartbeat,
            signal: Arc::new(AtomicBool::new(false)),
            slots: Arc::new(ConnectionSlots(Mutex::new(Vec::new()))),
            connect_lock: Arc::new(tokio::sync::Mutex::new(())),
            out_tx: Arc::new(Mutex::new(None)),
            out_rx: Arc::new(Mutex::new(None)),
            request_id_counter: Arc::new(AtomicU64::new(1)),
            instruments_cache: Arc::new(AtomicMap::new()),
            transport_backend,
            proxy_url: None,
            socket_factory: None,
            socket_endpoint: None,
        })
    }

    /// Configures the proxy used by every connection in the stream pool.
    #[must_use]
    pub fn with_proxy(mut self, proxy_url: Option<String>) -> Self {
        self.proxy_url = proxy_url;
        self
    }

    /// Configures socket state reporting and reconnect control for the stream pool.
    #[must_use]
    pub fn with_socket_control(
        mut self,
        factory: SocketControlFactory,
        endpoint: impl Into<String>,
    ) -> Self {
        self.socket_factory = Some(factory);
        self.socket_endpoint = Some(endpoint.into());
        self
    }

    /// Returns whether API credentials are configured.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Returns whether any connection in the pool is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let slots = self.slots.lock();
        slots
            .iter()
            .any(|s| s.connection_mode.load(Ordering::Relaxed) == ConnectionMode::Active as u8)
    }

    /// Returns whether all connections in the pool are closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let slots = self.slots.lock();
        slots.is_empty()
            || slots
                .iter()
                .all(|s| s.connection_mode.load(Ordering::Relaxed) == ConnectionMode::Closed as u8)
    }

    /// Returns the total number of confirmed subscriptions across all connections.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        let slots = self.slots.lock();
        slots.iter().map(|s| s.subscriptions_state.len()).sum()
    }

    /// Connects the first WebSocket connection in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    pub async fn connect(&mut self) -> BinanceWsResult<()> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;

        if !self.slots.lock().is_empty() {
            self.close_connections().await?;
        }

        {
            let _slots = self.slots.lock();
            self.signal.store(false, Ordering::Release);
        }

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self.out_tx.lock() = Some(out_tx);
        *self.out_rx.lock() = Some(out_rx);

        let slot = self.create_connection(0).await?;
        let shutdown = {
            let mut slots = self.slots.lock();
            let shutdown = self.signal.load(Ordering::Acquire);
            slots.push(slot);
            shutdown
        };

        if shutdown {
            let rollback = self.close_connections().await;
            return Err(BinanceWsError::ClientError(match rollback {
                Ok(()) => "Binance Spot SBE stream pool shutdown began during connect".to_string(),
                Err(e) => format!(
                    "Binance Spot SBE stream pool shutdown began during connect; rollback failed: {e}"
                ),
            }));
        }

        log::debug!(
            "Connected to Binance Spot SBE stream pool: url={}",
            self.url
        );
        Ok(())
    }

    /// Closes all WebSocket connections in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if disconnect fails.
    pub async fn close(&mut self) -> BinanceWsResult<()> {
        self.begin_shutdown();
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;
        self.close_connections().await
    }

    pub(crate) fn begin_shutdown(&self) {
        let slots = self.slots.lock();
        self.signal.store(true, Ordering::Release);

        for slot in slots.iter() {
            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
            slot.cancellation_token.cancel();
            let _ = slot.cmd_tx.send(BinanceSpotWsStreamsCommand::Disconnect);
        }
    }

    async fn close_connections(&self) -> BinanceWsResult<()> {
        self.begin_shutdown();

        let mut batch = ConnectionSlotBatch::take(&self.slots);
        let mut index = batch.slots.len();
        while index > 0 {
            index -= 1;
            let slot = &mut batch.slots[index];
            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
            slot.cancellation_token.cancel();
            let _result = slot.cmd_tx.send(BinanceSpotWsStreamsCommand::Disconnect);
            let Some(outcome) = finish_task(
                &mut slot.task_handle,
                std::time::Duration::from_secs(2),
                std::time::Duration::from_secs(2),
            )
            .await
            else {
                if slot.shutdown_errors.is_empty() {
                    batch.slots.remove(index);
                }
                continue;
            };

            match outcome {
                TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => {}
                TaskJoinOutcome::Failed(e) => slot
                    .shutdown_errors
                    .push(format!("Spot SBE handler failed: {e}")),
                TaskJoinOutcome::Incomplete => {
                    slot.shutdown_errors
                        .push("Spot SBE handler did not stop after abort".to_string());
                }
            }

            if slot.task_handle.is_none() && slot.shutdown_errors.is_empty() {
                batch.slots.remove(index);
            }
        }

        *self.out_tx.lock() = None;
        *self.out_rx.lock() = None;

        let errors = batch
            .slots
            .iter_mut()
            .flat_map(|slot| std::mem::take(&mut slot.shutdown_errors))
            .collect::<Vec<_>>();
        batch.slots.retain(|slot| slot.task_handle.is_some());
        if !errors.is_empty() {
            return Err(BinanceWsError::ClientError(errors.join("; ")));
        }
        log::debug!("Disconnected from Binance Spot SBE stream pool");
        Ok(())
    }

    /// Subscribes to the specified streams.
    ///
    /// Streams are distributed across pool connections. New connections are created
    /// automatically when existing ones reach the 1024-stream limit, up to a maximum
    /// of 20 connections.
    ///
    /// # Errors
    ///
    /// Returns an error if the pool is exhausted or command delivery fails.
    pub async fn subscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        // Serialize all phases so concurrent subscribers see a consistent
        // pool state and can't trigger spurious `Pool exhausted`.
        let _connect_guard = self.connect_lock.lock().await;

        // Phase 1: filter already-subscribed streams (brief lock).
        let new_streams: Vec<String> = {
            let slots = self.slots.lock();

            if self.signal.load(Ordering::Acquire) {
                return Err(BinanceWsError::ClientError(
                    "Binance Spot SBE stream pool is shutting down".to_string(),
                ));
            }
            streams
                .into_iter()
                .filter(|s| !slots.iter().any(|slot| slot.streams.contains(s)))
                .collect()
        };

        if new_streams.is_empty() {
            return Ok(());
        }

        // Phase 2: create connections if needed.
        loop {
            let (remaining_capacity, slot_count) = {
                let slots = self.slots.lock();
                let cap: usize = slots
                    .iter()
                    .map(|s| MAX_STREAMS_PER_CONNECTION - s.streams.len())
                    .sum();
                (cap, slots.len())
            };

            if remaining_capacity >= new_streams.len() || slot_count >= MAX_CONNECTIONS {
                break;
            }

            let new_slot = self.create_connection(slot_count).await?;
            let (slot_count, shutdown) = {
                let mut slots = self.slots.lock();
                let shutdown = self.signal.load(Ordering::Acquire);
                slots.push(new_slot);
                (slots.len(), shutdown)
            };

            if shutdown {
                let client = self.clone();
                let rollback = client.close_connections().await;
                return Err(BinanceWsError::ClientError(match rollback {
                    Ok(()) => {
                        "Binance Spot SBE stream pool shutdown began during subscribe".to_string()
                    }
                    Err(e) => format!(
                        "Binance Spot SBE stream pool shutdown began during subscribe; rollback failed: {e}"
                    ),
                }));
            }
            log::debug!("Pool slot {} connected: url={}", slot_count - 1, self.url);
        }

        // Phase 3: assign streams to slots and send commands (brief lock).
        // Stage assignments first so a capacity error leaves slots unchanged.
        let mut slots = self.slots.lock();

        if self.signal.load(Ordering::Acquire) {
            return Err(BinanceWsError::ClientError(
                "Binance Spot SBE stream pool is shutting down".to_string(),
            ));
        }
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
                .send(BinanceSpotWsStreamsCommand::Subscribe {
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
    pub async fn unsubscribe(&self, streams: Vec<String>) -> BinanceWsResult<()> {
        let _connect_guard = self.connect_lock.lock().await;
        let mut slots = self.slots.lock();

        if self.signal.load(Ordering::Acquire) {
            return Err(BinanceWsError::ClientError(
                "Binance Spot SBE stream pool is shutting down".to_string(),
            ));
        }
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
                .send(BinanceSpotWsStreamsCommand::Unsubscribe {
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
    pub fn stream(&self) -> impl Stream<Item = BinanceSpotWsMessage> + 'static {
        let out_rx = self.out_rx.lock().take();
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
                m.insert(inst.symbol().inner(), inst.clone());
            }
        });
    }

    /// Replaces the complete instrument cache.
    pub fn replace_instruments(&self, instruments: &[InstrumentAny]) {
        let cache = instruments
            .iter()
            .map(|instrument| (instrument.symbol().inner(), instrument.clone()))
            .collect();
        self.instruments_cache.store(cache);
    }

    /// Update a single instrument in the cache.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument);
    }

    /// Returns a shared reference to the instruments cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<AtomicMap<Ustr, InstrumentAny>> {
        self.instruments_cache.clone()
    }

    /// Returns an instrument from the cache by symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instruments_cache.get_cloned(&Ustr::from(symbol))
    }

    async fn create_connection(&self, slot_index: usize) -> BinanceWsResult<ConnectionSlot> {
        let out_tx = self.out_tx.lock().clone().ok_or_else(|| {
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
            heartbeat_interval_secs: self.heartbeat,
            heartbeat_payload: None,
            connect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let keyed_quotas = vec![(
            BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION[0].as_str().to_string(),
            *BINANCE_WS_SUBSCRIPTION_QUOTA,
        )];

        let socket_control = self
            .socket_factory
            .as_ref()
            .zip(self.socket_endpoint.as_ref())
            .map(|(factory, endpoint)| {
                let endpoint = if slot_index == 0 {
                    endpoint.clone()
                } else {
                    format!("{endpoint}-{slot_index}")
                };
                factory.control(endpoint)
            });
        let client = WebSocketClient::builder()
            .config(config)
            .message_handler(raw_handler)
            .ping_handler(ping_handler)
            .keyed_quotas(keyed_quotas)
            .default_quota(*BINANCE_WS_CONNECTION_QUOTA)
            .maybe_state_sink(socket_control.as_ref().map(SocketControl::sink))
            .connect()
            .await
            .map_err(|e| {
                log::error!("WebSocket connection failed: {e}");
                BinanceWsError::NetworkError(e.to_string())
            })?;

        let connection_mode = client.connection_mode_atomic();
        let reconnect_handle = client.reconnect_handle();
        let subscriptions_state = SubscriptionState::new('@');
        let cancellation_token = CancellationToken::new();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let mut handler = BinanceSpotWsFeedHandler::new(
            self.signal.clone(),
            cmd_rx,
            raw_rx,
            out_tx.clone(),
            subscriptions_state.clone(),
            self.request_id_counter.clone(),
        );

        cmd_tx
            .send(BinanceSpotWsStreamsCommand::SetClient(client))
            .map_err(|e| BinanceWsError::ClientError(format!("Failed to set client: {e}")))?;

        let signal = self.signal.clone();
        let token = cancellation_token.clone();
        let subs = subscriptions_state.clone();
        let resubscribe_tx = cmd_tx.clone();

        let mut task_handle = TaskSlot::new();
        if let Err(e) = task_handle.spawn(async move {
            loop {
                tokio::select! {
                    () = token.cancelled() => {
                        log::debug!("Handler task cancelled");
                        break;
                    }
                    result = handler.next() => {
                        match result {
                            Some(BinanceSpotWsMessage::Reconnected) => {
                                log::info!("WebSocket reconnected, restoring subscriptions");
                                let all_topics = subs.all_topics();
                                for topic in &all_topics {
                                    subs.mark_failure(topic);
                                }

                                let streams = subs.all_topics();
                                if !streams.is_empty()
                                    && let Err(e) = resubscribe_tx.send(BinanceSpotWsStreamsCommand::Subscribe { streams }) {
                                        log::error!("Failed to resubscribe after reconnect: {e}");
                                    }

                                if out_tx.send(BinanceSpotWsMessage::Reconnected).is_err() {
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
        }) {
            cancellation_token.cancel();
            let shutdown_error = match finish_task(
                &mut task_handle,
                std::time::Duration::ZERO,
                std::time::Duration::from_secs(2),
            )
            .await
            {
                Some(TaskJoinOutcome::Failed(error)) => {
                    Some(format!("Binance Spot WS handler task failed: {error}"))
                }
                Some(TaskJoinOutcome::Incomplete) => {
                    Some("Binance Spot WS handler task did not stop after abort".to_string())
                }
                None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => None,
            };
            return Err(BinanceWsError::ClientError(match shutdown_error {
                Some(shutdown_error) => format!(
                    "Failed to start Spot WS handler task: {e}; startup rollback failed: \
                     {shutdown_error}"
                ),
                None => format!("Failed to start Spot WS handler task: {e}"),
            }));
        }

        if let Some(control) = &socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        Ok(ConnectionSlot {
            cmd_tx,
            streams: Vec::new(),
            subscriptions_state,
            task_handle,
            cancellation_token,
            connection_mode,
            socket_control,
            shutdown_errors: Vec::new(),
        })
    }
}

struct ConnectionSlots(Mutex<Vec<ConnectionSlot>>);

impl std::ops::Deref for ConnectionSlots {
    type Target = Mutex<Vec<ConnectionSlot>>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl Drop for ConnectionSlots {
    fn drop(&mut self) {
        for slot in self.0.get_mut().iter() {
            slot.cancellation_token.cancel();
            if let Some(handle) = slot.task_handle.as_ref() {
                handle.abort();
            }

            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
        }
    }
}

struct ConnectionSlotBatch<'a> {
    owner: &'a Mutex<Vec<ConnectionSlot>>,
    slots: Vec<ConnectionSlot>,
}

impl<'a> ConnectionSlotBatch<'a> {
    fn take(owner: &'a Mutex<Vec<ConnectionSlot>>) -> Self {
        let slots = std::mem::take(&mut *owner.lock());
        Self { owner, slots }
    }
}

impl Drop for ConnectionSlotBatch<'_> {
    fn drop(&mut self) {
        self.owner.lock().extend(self.slots.drain(..));
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[tokio::test]
    async fn test_cancelled_close_retains_connection_slot() {
        let mut client =
            BinanceSpotWebSocketClient::new(None, None, None, None, TransportBackend::default())
                .unwrap();
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        client.slots.lock().push(ConnectionSlot {
            cmd_tx,
            streams: Vec::new(),
            subscriptions_state: SubscriptionState::new('@'),
            task_handle: TaskSlot::from_handle(tokio::spawn(std::future::pending())),
            cancellation_token: CancellationToken::new(),
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Active as u8)),
            socket_control: None,
            shutdown_errors: Vec::new(),
        });

        {
            let close = client.close();
            tokio::pin!(close);
            tokio::select! {
                result = &mut close => panic!("close completed unexpectedly: {result:?}"),
                command = cmd_rx.recv() => assert!(command.is_some()),
            }
        }

        let slots = client.slots.lock();
        assert_eq!(slots.len(), 1);
        assert!(slots[0].task_handle.is_some());
    }

    #[rstest]
    fn test_new_rejects_hmac_secret_with_actionable_error() {
        // An all-zero 48-byte buffer base64-encodes to a non-Ed25519 secret
        // (no PKCS#8 OID), standing in for an HMAC key. SBE market-data streams
        // require Ed25519, so construction must fail with guidance that names
        // HMAC, not the raw OID error.
        let secret = base64::Engine::encode(&base64::engine::general_purpose::STANDARD, [0u8; 48]);

        let result = BinanceSpotWebSocketClient::new(
            None,
            Some("test_key".to_string()),
            Some(secret),
            None,
            TransportBackend::default(),
        );

        let err = result.expect_err("HMAC secret must be rejected");
        let msg = err.to_string();
        assert!(
            msg.contains("Ed25519"),
            "error should mention Ed25519, was: {msg}"
        );
        assert!(
            msg.contains("HMAC"),
            "error should mention HMAC, was: {msg}"
        );
    }

    #[rstest]
    fn test_with_proxy_preserves_proxy_url() {
        let client =
            BinanceSpotWebSocketClient::new(None, None, None, None, TransportBackend::default())
                .unwrap()
                .with_proxy(Some("socks5://proxy.example:1080".to_string()));

        assert_eq!(
            client.proxy_url.as_deref(),
            Some("socks5://proxy.example:1080")
        );
    }
}
