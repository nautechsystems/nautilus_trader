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

//! Binance Spot public JSON WebSocket client for market data streams.

use std::{
    fmt::Debug,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use futures_util::Stream;
use nautilus_core::AtomicMap;
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
use tokio_tungstenite::tungstenite::Message;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    handler::BinanceSpotPublicWsHandler,
    messages::{BinanceSpotPublicWsCommand, BinanceSpotPublicWsMessage},
};
use crate::common::consts::{
    BINANCE_RATE_LIMIT_KEY_SUBSCRIPTION, BINANCE_SPOT_WS_URL, BINANCE_WS_CONNECTION_QUOTA,
    BINANCE_WS_SUBSCRIPTION_QUOTA,
};

/// Maximum streams per Spot JSON WebSocket connection.
pub const MAX_STREAMS_PER_CONNECTION: usize = 1024;
/// Maximum pooled Spot JSON WebSocket connections.
pub const MAX_CONNECTIONS: usize = 20;

struct ConnectionSlot {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<BinanceSpotPublicWsCommand>,
    streams: Vec<String>,
    handler_task: TaskSlot<()>,
    bytes_task: TaskSlot<()>,
    cancellation_token: CancellationToken,
    connection_mode: Arc<AtomicU8>,
    socket_control: Option<SocketControl>,
    shutdown_errors: Vec<String>,
}

/// Binance Spot public JSON WebSocket client.
#[derive(Clone)]
pub struct BinanceSpotPublicJsonWebSocketClient {
    url: String,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    slots: Arc<ConnectionSlots>,
    connect_lock: Arc<tokio::sync::Mutex<()>>,
    out_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<BinanceSpotPublicWsMessage>>>>,
    out_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<BinanceSpotPublicWsMessage>>>>,
    request_id_counter: Arc<AtomicU64>,
    instruments_cache: Arc<AtomicMap<Ustr, InstrumentAny>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    socket_factory: Option<SocketControlFactory>,
    socket_endpoint: Option<String>,
}

impl Debug for BinanceSpotPublicJsonWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotPublicJsonWebSocketClient))
            .field("url", &self.url)
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl Default for BinanceSpotPublicJsonWebSocketClient {
    fn default() -> Self {
        Self::new(None, None, TransportBackend::default())
    }
}

impl BinanceSpotPublicJsonWebSocketClient {
    /// Creates a new Spot public JSON WebSocket client.
    #[must_use]
    pub fn new(
        url: Option<String>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
    ) -> Self {
        let url = normalize_spot_json_stream_url(
            url.unwrap_or_else(|| BINANCE_SPOT_WS_URL.to_string())
                .as_str(),
        );

        Self {
            url,
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
        }
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

    /// Connects the first WebSocket connection in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;

        if !self.slots.lock().expect("slots lock poisoned").is_empty() {
            self.close_connections().await?;
        }

        {
            let _slots = self.slots.lock().expect("slots lock poisoned");
            self.signal.store(false, Ordering::Release);
        }

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        *self.out_tx.lock().expect("out_tx lock poisoned") = Some(out_tx);
        *self.out_rx.lock().expect("out_rx lock poisoned") = Some(out_rx);

        let slot = self.create_connection(0).await?;
        let shutdown = {
            let mut slots = self.slots.lock().expect("slots lock poisoned");
            let shutdown = self.signal.load(Ordering::Acquire);
            slots.push(slot);
            shutdown
        };

        if shutdown {
            let rollback = self.close_connections().await;
            return Err(match rollback {
                Ok(()) => anyhow::anyhow!(
                    "Binance Spot public JSON stream pool shutdown began during connect"
                ),
                Err(e) => anyhow::anyhow!(
                    "Binance Spot public JSON stream pool shutdown began during connect; rollback failed: {e}"
                ),
            });
        }

        log::debug!(
            "Connected to Binance Spot public JSON stream pool: url={}",
            self.url
        );
        Ok(())
    }

    /// Closes all WebSocket connections and tasks.
    ///
    /// # Errors
    ///
    /// Returns an error if command delivery fails while shutting down.
    pub async fn close(&mut self) -> anyhow::Result<()> {
        self.begin_shutdown();
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;
        self.close_connections().await
    }

    pub(crate) fn begin_shutdown(&self) {
        let slots = self.slots.lock().expect("slots lock poisoned");
        self.signal.store(true, Ordering::Release);

        for slot in slots.iter() {
            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
            slot.cancellation_token.cancel();
            let _ = slot.cmd_tx.send(BinanceSpotPublicWsCommand::Disconnect);
        }
    }

    async fn close_connections(&self) -> anyhow::Result<()> {
        self.begin_shutdown();

        let mut batch = ConnectionSlotBatch::take(&self.slots);
        let mut index = batch.slots.len();
        while index > 0 {
            index -= 1;
            let slot = &mut batch.slots[index];
            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
            let _ = slot.cmd_tx.send(BinanceSpotPublicWsCommand::Disconnect);
            slot.cancellation_token.cancel();
            if let Some(error) =
                finish_slot_task(&mut slot.handler_task, "Spot public stream handler").await
            {
                slot.shutdown_errors.push(error);
            }

            if let Some(error) =
                finish_slot_task(&mut slot.bytes_task, "Spot public byte stream").await
            {
                slot.shutdown_errors.push(error);
            }

            if slot.handler_task.is_none()
                && slot.bytes_task.is_none()
                && slot.shutdown_errors.is_empty()
            {
                batch.slots.remove(index);
            }
        }

        *self.out_tx.lock().expect("out_tx lock poisoned") = None;
        *self.out_rx.lock().expect("out_rx lock poisoned") = None;

        let errors = batch
            .slots
            .iter_mut()
            .flat_map(|slot| std::mem::take(&mut slot.shutdown_errors))
            .collect::<Vec<_>>();
        batch
            .slots
            .retain(|slot| slot.handler_task.is_some() || slot.bytes_task.is_some());

        if !errors.is_empty() {
            anyhow::bail!(errors.join("; "));
        }
        log::debug!("Disconnected from Binance Spot public JSON stream pool");
        Ok(())
    }

    /// Subscribes to stream names.
    ///
    /// # Errors
    ///
    /// Returns an error if command delivery fails or if the connection pool is exhausted.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn subscribe(&self, streams: Vec<String>) -> anyhow::Result<()> {
        let _connect_guard = self.connect_lock.lock().await;

        // Phase 1: filter already-subscribed streams (brief lock)
        let new_streams: Vec<String> = {
            let slots = self.slots.lock().expect("slots lock poisoned");

            if self.signal.load(Ordering::Acquire) {
                anyhow::bail!("Binance Spot public JSON stream pool is shutting down");
            }
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
                    .map(|s| MAX_STREAMS_PER_CONNECTION.saturating_sub(s.streams.len()))
                    .sum();
                (cap, slots.len())
            };

            if remaining_capacity >= new_streams.len() || slot_count >= MAX_CONNECTIONS {
                break;
            }

            let new_slot = self.create_connection(slot_count).await?;
            let (slot_count, shutdown) = {
                let mut slots = self.slots.lock().expect("slots lock poisoned");
                let shutdown = self.signal.load(Ordering::Acquire);
                slots.push(new_slot);
                (slots.len(), shutdown)
            };

            if shutdown {
                let client = self.clone();
                let rollback = client.close_connections().await;
                return Err(match rollback {
                    Ok(()) => anyhow::anyhow!(
                        "Binance Spot public JSON stream pool shutdown began during subscribe"
                    ),
                    Err(e) => anyhow::anyhow!(
                        "Binance Spot public JSON stream pool shutdown began during subscribe; rollback failed: {e}"
                    ),
                });
            }
            log::debug!(
                "Spot JSON pool slot {} connected: url={}",
                slot_count - 1,
                self.url
            );
        }

        // Phase 3: stage assignments, send commands, then commit slot state.
        let mut slots = self.slots.lock().expect("slots lock poisoned");

        if self.signal.load(Ordering::Acquire) {
            anyhow::bail!("Binance Spot public JSON stream pool is shutting down");
        }
        let mut slot_batches: Vec<(usize, Vec<String>)> = Vec::new();
        let mut slot_counts: Vec<usize> = slots.iter().map(|s| s.streams.len()).collect();

        for stream in &new_streams {
            let slot_idx = slot_counts
                .iter()
                .position(|&count| count < MAX_STREAMS_PER_CONNECTION)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "Spot public JSON stream pool exhausted ({MAX_CONNECTIONS} connections x {MAX_STREAMS_PER_CONNECTION} streams)",
                    )
                })?;

            slot_counts[slot_idx] += 1;

            if let Some(batch) = slot_batches.iter_mut().find(|(i, _)| *i == slot_idx) {
                batch.1.push(stream.clone());
            } else {
                slot_batches.push((slot_idx, vec![stream.clone()]));
            }
        }

        for (slot_idx, batch) in &slot_batches {
            slots[*slot_idx]
                .cmd_tx
                .send(BinanceSpotPublicWsCommand::Subscribe {
                    streams: batch.clone(),
                })
                .map_err(|e| {
                    anyhow::anyhow!("Handler not available for Spot JSON pool slot {slot_idx}: {e}")
                })?;
            slots[*slot_idx].streams.extend(batch.iter().cloned());
        }

        Ok(())
    }

    /// Unsubscribes from stream names.
    ///
    /// # Errors
    ///
    /// Returns an error if command delivery fails.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub async fn unsubscribe(&self, streams: Vec<String>) -> anyhow::Result<()> {
        if streams.is_empty() {
            return Ok(());
        }

        let _connect_guard = self.connect_lock.lock().await;
        let mut slots = self.slots.lock().expect("slots lock poisoned");

        if self.signal.load(Ordering::Acquire) {
            anyhow::bail!("Binance Spot public JSON stream pool is shutting down");
        }
        let mut slot_batches: Vec<(usize, Vec<String>)> = Vec::new();

        for stream in &streams {
            if let Some(slot_idx) = slots
                .iter()
                .position(|s| s.streams.iter().any(|x| x == stream))
            {
                if let Some(batch) = slot_batches.iter_mut().find(|(i, _)| *i == slot_idx) {
                    batch.1.push(stream.clone());
                } else {
                    slot_batches.push((slot_idx, vec![stream.clone()]));
                }
            }
        }

        for (slot_idx, batch) in &slot_batches {
            slots[*slot_idx]
                .cmd_tx
                .send(BinanceSpotPublicWsCommand::Unsubscribe {
                    streams: batch.clone(),
                })
                .map_err(|e| {
                    anyhow::anyhow!("Handler not available for Spot JSON pool slot {slot_idx}: {e}")
                })?;

            for stream in batch {
                slots[*slot_idx].streams.retain(|s| s != stream);
            }
        }

        Ok(())
    }

    /// Returns a stream of output messages.
    #[expect(clippy::missing_panics_doc, reason = "mutex poisoning is not expected")]
    pub fn stream(&self) -> impl Stream<Item = BinanceSpotPublicWsMessage> + 'static {
        let mut guard = self.out_rx.lock().expect("out_rx lock poisoned");
        let out_rx = guard.take();
        drop(guard);

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

    /// Replaces the complete instrument cache.
    pub fn replace_instruments(&self, instruments: &[InstrumentAny]) {
        let cache = instruments
            .iter()
            .map(|instrument| (instrument.raw_symbol().inner(), instrument.clone()))
            .collect();
        self.instruments_cache.store(cache);
    }

    /// Returns a shared reference to the instruments cache.
    #[must_use]
    pub fn instruments_cache(&self) -> Arc<AtomicMap<Ustr, InstrumentAny>> {
        self.instruments_cache.clone()
    }

    async fn create_connection(&self, slot_index: usize) -> anyhow::Result<ConnectionSlot> {
        let out_tx = self
            .out_tx
            .lock()
            .expect("out_tx lock poisoned")
            .clone()
            .ok_or_else(|| anyhow::anyhow!("Output channel not initialized"))?;

        let (raw_handler, raw_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
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
            .map_err(|e| anyhow::anyhow!("Failed to connect Spot public JSON WS: {e}"))?;

        let connection_mode = client.connection_mode_atomic();
        let reconnect_handle = client.reconnect_handle();
        let subscriptions_state = SubscriptionState::new('@');
        let cancellation_token = CancellationToken::new();

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let (bytes_tx, bytes_rx) = tokio::sync::mpsc::unbounded_channel::<Vec<u8>>();

        let mut bytes_task = TaskSlot::new();
        if let Err(e) = bytes_task.spawn(async move {
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
        }) {
            let shutdown_error = finish_slot_task(&mut bytes_task, "Binance Spot WS bytes").await;
            anyhow::bail!(match shutdown_error {
                Some(shutdown_error) => format!(
                    "Failed to start Spot public JSON WS bytes task: {e}; startup rollback failed: \
                     {shutdown_error}"
                ),
                None => format!("Failed to start Spot public JSON WS bytes task: {e}"),
            });
        }

        let mut handler = BinanceSpotPublicWsHandler::new(
            self.signal.clone(),
            cmd_rx,
            bytes_rx,
            subscriptions_state.clone(),
            self.request_id_counter.clone(),
        );

        cmd_tx
            .send(BinanceSpotPublicWsCommand::SetClient(client))
            .map_err(|e| anyhow::anyhow!("Failed to set Spot public JSON WS client: {e}"))?;

        let signal = self.signal.clone();
        let token = cancellation_token.clone();
        let resubscribe_tx = cmd_tx.clone();

        let mut handler_task = TaskSlot::new();
        if let Err(e) = handler_task.spawn(async move {
            loop {
                tokio::select! {
                    () = token.cancelled() => {
                        log::debug!("Spot public JSON handler task cancelled");
                        break;
                    }
                    result = handler.next() => {
                        match result {
                            Some(BinanceSpotPublicWsMessage::Reconnected) => {
                                log::info!("Spot public JSON WebSocket reconnected, restoring subscriptions");
                                let topics = subscriptions_state.all_topics();
                                for topic in &topics {
                                    subscriptions_state.mark_failure(topic);
                                }

                                let streams = subscriptions_state.all_topics();
                                if !streams.is_empty()
                                    && let Err(e) = resubscribe_tx.send(BinanceSpotPublicWsCommand::Subscribe { streams }) {
                                        log::error!("Failed to resubscribe after reconnect: {e}");
                                    }

                                if out_tx.send(BinanceSpotPublicWsMessage::Reconnected).is_err() {
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
                                    break;
                                }
                                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
                            }
                        }
                    }
                }
            }
        }) {
            cancellation_token.cancel();
            bytes_task.abort();
            let mut shutdown_errors = Vec::new();

            if let Some(error) =
                finish_slot_task(&mut handler_task, "Binance Spot public JSON handler").await
            {
                shutdown_errors.push(error);
            }

            if let Some(error) =
                finish_slot_task(&mut bytes_task, "Binance Spot public JSON bytes").await
            {
                shutdown_errors.push(error);
            }
            anyhow::bail!(if shutdown_errors.is_empty() {
                format!("Failed to start Spot public JSON WS handler task: {e}")
            } else {
                format!(
                    "Failed to start Spot public JSON WS handler task: {e}; startup rollback failed: \
                     {}",
                    shutdown_errors.join("; ")
                )
            });
        }

        if let Some(control) = &socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        Ok(ConnectionSlot {
            cmd_tx,
            streams: Vec::new(),
            handler_task,
            bytes_task,
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
        for slot in self.0.get_mut().expect("slots lock poisoned").iter() {
            slot.cancellation_token.cancel();
            if let Some(handle) = slot.handler_task.as_ref() {
                handle.abort();
            }

            if let Some(handle) = slot.bytes_task.as_ref() {
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
        let slots = std::mem::take(&mut *owner.lock().expect("slots lock poisoned"));
        Self { owner, slots }
    }
}

impl Drop for ConnectionSlotBatch<'_> {
    fn drop(&mut self) {
        self.owner
            .lock()
            .expect("slots lock poisoned")
            .extend(self.slots.drain(..));
    }
}

async fn finish_slot_task(slot: &mut TaskSlot<()>, owner: &str) -> Option<String> {
    let outcome = finish_task(
        slot,
        std::time::Duration::from_secs(2),
        std::time::Duration::from_secs(2),
    )
    .await?;

    match outcome {
        TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted => None,
        TaskJoinOutcome::Failed(e) => Some(format!("{owner} task failed: {e}")),
        TaskJoinOutcome::Incomplete => Some(format!("{owner} task did not stop after abort")),
    }
}

fn normalize_spot_json_stream_url(base_url: &str) -> String {
    let trimmed = base_url.trim_end_matches('/');

    if trimmed.ends_with("/stream") {
        return trimmed.to_string();
    }

    if let Some(prefix) = trimmed.strip_suffix("/ws") {
        return format!("{prefix}/stream");
    }

    format!("{trimmed}/stream")
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::AtomicU8;

    use nautilus_network::mode::ConnectionMode;
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_with_proxy_preserves_proxy_url() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default())
                .with_proxy(Some("http://proxy.example:8080".to_string()));

        assert_eq!(
            client.proxy_url.as_deref(),
            Some("http://proxy.example:8080")
        );
    }

    fn make_slot_with_streams(
        streams: Vec<String>,
    ) -> (
        ConnectionSlot,
        tokio::sync::mpsc::UnboundedReceiver<BinanceSpotPublicWsCommand>,
    ) {
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        let handler_task = tokio::spawn(async {});

        let bytes_task = tokio::spawn(async {});

        let slot = ConnectionSlot {
            cmd_tx,
            streams,
            handler_task: TaskSlot::from_handle(handler_task),
            bytes_task: TaskSlot::from_handle(bytes_task),
            cancellation_token: CancellationToken::new(),
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Active as u8)),
            socket_control: None,
            shutdown_errors: Vec::new(),
        };

        (slot, cmd_rx)
    }

    #[tokio::test]
    async fn test_cancelled_close_retains_connection_slot() {
        let mut client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (mut slot, mut cmd_rx) = make_slot_with_streams(Vec::new());
        slot.handler_task = TaskSlot::from_handle(tokio::spawn(std::future::pending()));
        client.slots.lock().expect("slots lock poisoned").push(slot);

        {
            let close = client.close();
            tokio::pin!(close);
            tokio::select! {
                result = &mut close => panic!("close completed unexpectedly: {result:?}"),
                command = cmd_rx.recv() => assert!(command.is_some()),
            }
        }

        let slots = client.slots.lock().expect("slots lock poisoned");
        assert_eq!(slots.len(), 1);
        assert!(slots[0].handler_task.is_some());
    }

    #[tokio::test]
    async fn test_subscribe_reuses_existing_stream_and_only_subscribes_new_one() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (slot, mut cmd_rx) = make_slot_with_streams(vec!["btcusdt@trade".to_string()]);
        client.slots.lock().expect("slots lock poisoned").push(slot);

        client
            .subscribe(vec![
                "btcusdt@trade".to_string(),
                "ethusdt@trade".to_string(),
            ])
            .await
            .expect("subscribe should succeed");

        match cmd_rx
            .try_recv()
            .expect("one subscribe command should be sent")
        {
            BinanceSpotPublicWsCommand::Subscribe { streams } => {
                assert_eq!(streams, vec!["ethusdt@trade".to_string()]);
            }
            _ => panic!("unexpected command type"),
        }
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let slots = client.slots.lock().expect("slots lock poisoned");
        assert_eq!(slots.len(), 1);
        assert_eq!(
            slots[0].streams,
            vec!["btcusdt@trade".to_string(), "ethusdt@trade".to_string()]
        );
    }

    #[tokio::test]
    async fn test_unsubscribe_removes_only_target_stream_when_sibling_still_subscribed() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (slot, mut cmd_rx) = make_slot_with_streams(vec![
            "btcusdt@trade".to_string(),
            "btcusdt@bookTicker".to_string(),
        ]);
        client.slots.lock().expect("slots lock poisoned").push(slot);

        client
            .unsubscribe(vec!["btcusdt@bookTicker".to_string()])
            .await
            .expect("unsubscribe should succeed");

        match cmd_rx
            .try_recv()
            .expect("one unsubscribe command should be sent")
        {
            BinanceSpotPublicWsCommand::Unsubscribe { streams } => {
                assert_eq!(streams, vec!["btcusdt@bookTicker".to_string()]);
            }
            _ => panic!("unexpected command type"),
        }
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let slots = client.slots.lock().expect("slots lock poisoned");
        assert_eq!(slots.len(), 1);
        assert_eq!(slots[0].streams, vec!["btcusdt@trade".to_string()]);
    }

    #[tokio::test]
    async fn test_unsubscribe_all_streams_clears_slot_state() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (slot, mut cmd_rx) = make_slot_with_streams(vec![
            "btcusdt@trade".to_string(),
            "ethusdt@trade".to_string(),
        ]);
        client.slots.lock().expect("slots lock poisoned").push(slot);

        client
            .unsubscribe(vec![
                "btcusdt@trade".to_string(),
                "ethusdt@trade".to_string(),
            ])
            .await
            .expect("unsubscribe should succeed");

        let mut sent = match cmd_rx
            .try_recv()
            .expect("one unsubscribe command should be sent")
        {
            BinanceSpotPublicWsCommand::Unsubscribe { streams } => streams,
            _ => panic!("unexpected command type"),
        };

        sent.sort();
        assert_eq!(
            sent,
            vec!["btcusdt@trade".to_string(), "ethusdt@trade".to_string()]
        );

        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let slots = client.slots.lock().expect("slots lock poisoned");
        assert_eq!(slots.len(), 1);
        assert!(slots[0].streams.is_empty());
    }

    #[tokio::test]
    async fn test_subscribe_batches_same_slot_streams_in_single_command() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (slot, mut cmd_rx) = make_slot_with_streams(vec![]);
        client.slots.lock().expect("slots lock poisoned").push(slot);

        client
            .subscribe(vec![
                "btcusdt@trade".to_string(),
                "ethusdt@trade".to_string(),
            ])
            .await
            .expect("subscribe should succeed");

        let mut sent = match cmd_rx
            .try_recv()
            .expect("one subscribe command should be sent")
        {
            BinanceSpotPublicWsCommand::Subscribe { streams } => streams,
            _ => panic!("unexpected command type"),
        };

        sent.sort();
        assert_eq!(
            sent,
            vec!["btcusdt@trade".to_string(), "ethusdt@trade".to_string()]
        );
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let slots = client.slots.lock().expect("slots lock poisoned");
        let mut stored = slots[0].streams.clone();
        stored.sort();
        assert_eq!(
            stored,
            vec!["btcusdt@trade".to_string(), "ethusdt@trade".to_string()]
        );
    }

    #[tokio::test]
    async fn test_unsubscribe_batches_same_slot_streams_in_single_command() {
        let client =
            BinanceSpotPublicJsonWebSocketClient::new(None, None, TransportBackend::default());
        let (slot, mut cmd_rx) = make_slot_with_streams(vec![
            "btcusdt@trade".to_string(),
            "ethusdt@trade".to_string(),
        ]);
        client.slots.lock().expect("slots lock poisoned").push(slot);

        client
            .unsubscribe(vec![
                "btcusdt@trade".to_string(),
                "ethusdt@trade".to_string(),
            ])
            .await
            .expect("unsubscribe should succeed");

        let mut sent = match cmd_rx
            .try_recv()
            .expect("one unsubscribe command should be sent")
        {
            BinanceSpotPublicWsCommand::Unsubscribe { streams } => streams,
            _ => panic!("unexpected command type"),
        };

        sent.sort();
        assert_eq!(
            sent,
            vec!["btcusdt@trade".to_string(), "ethusdt@trade".to_string()]
        );
        assert!(matches!(
            cmd_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));

        let slots = client.slots.lock().expect("slots lock poisoned");
        assert_eq!(slots.len(), 1);
        assert!(slots[0].streams.is_empty());
    }

    #[rstest]
    #[case("wss://stream.binance.com/ws", "wss://stream.binance.com/stream")]
    #[case("wss://stream.binance.com/stream", "wss://stream.binance.com/stream")]
    #[case("wss://stream.binance.com/stream/", "wss://stream.binance.com/stream")]
    fn test_normalize_spot_json_stream_url(#[case] input: &str, #[case] expected: &str) {
        assert_eq!(normalize_spot_json_stream_url(input), expected);
    }
}
