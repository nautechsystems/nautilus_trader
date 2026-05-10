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

//! WebSocket client for the Bullet streaming API.
//!
//! Features:
//! - Automatic reconnect with exponential back-off (250 ms → 30 s)
//! - Active-subscription replay after reconnect
//! - Instrument cache for price/size precision lookup in parse helpers

use std::{
    collections::{HashMap, HashSet},
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
    time::Duration,
};

use futures_util::{SinkExt, StreamExt as _};
use nautilus_model::instruments::{Instrument, InstrumentAny};
use tokio::sync::{mpsc, Mutex as AsyncMutex};
use tokio_tungstenite::{connect_async, tungstenite::Message};
use ustr::Ustr;

use crate::common::error::BulletError;
use crate::websocket::{
    messages::{ServerMessage, SubscribeRequest},
    topics::Topic,
};

/// WebSocket client for Bullet streaming subscriptions.
///
/// Arc-based clone — all clones share state.  Call `connect()` once; then
/// subscribe/unsubscribe and drive the event loop with `next_event()`.
/// Reconnects automatically on drop with exponential back-off.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(
        module = "nautilus_trader.core.nautilus_pyo3.bullet",
    )
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.bullet")
)]
pub struct BulletWebSocketClient {
    url: String,
    /// Active topic subscriptions — replayed on reconnect.
    active_topics: Arc<AsyncMutex<HashSet<String>>>,
    /// Monotonically-increasing message ID counter.
    next_id: Arc<AtomicU64>,
    /// Sender half of the outbound message channel.
    write_tx: Arc<std::sync::Mutex<Option<mpsc::UnboundedSender<Message>>>>,
    /// Received parsed server messages.
    read_rx: Arc<AsyncMutex<Option<mpsc::UnboundedReceiver<ServerMessage>>>>,
    /// True while the reconnect loop is running (may be between connections).
    started: Arc<AtomicBool>,
    /// True while the WebSocket is actively connected.
    running: Arc<AtomicBool>,
    /// Set to stop the reconnect loop on next iteration.
    stop_flag: Arc<AtomicBool>,
    /// Per-symbol instrument cache for precision lookup.
    instruments: Arc<std::sync::Mutex<HashMap<Ustr, InstrumentAny>>>,
}

impl BulletWebSocketClient {
    /// Create a new disconnected [`BulletWebSocketClient`].
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            active_topics: Arc::new(AsyncMutex::new(HashSet::new())),
            next_id: Arc::new(AtomicU64::new(1)),
            write_tx: Arc::new(std::sync::Mutex::new(None)),
            read_rx: Arc::new(AsyncMutex::new(None)),
            started: Arc::new(AtomicBool::new(false)),
            running: Arc::new(AtomicBool::new(false)),
            stop_flag: Arc::new(AtomicBool::new(false)),
            instruments: Arc::new(std::sync::Mutex::new(HashMap::new())),
        }
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns `true` if the reconnect loop is running (may be between connections).
    #[must_use]
    pub fn is_started(&self) -> bool {
        self.started.load(Ordering::Relaxed)
    }

    /// Returns `true` if there is an active WebSocket connection right now.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        self.running.load(Ordering::Relaxed)
    }

    /// Cache a single instrument keyed by its raw Bullet symbol (e.g. `"SOL-USD"`).
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let key = Ustr::from(instrument.raw_symbol().as_str());
        if let Ok(mut guard) = self.instruments.lock() {
            guard.insert(key, instrument);
        }
    }

    /// Cache multiple instruments.
    pub fn cache_instruments(&self, instruments: impl IntoIterator<Item = InstrumentAny>) {
        if let Ok(mut guard) = self.instruments.lock() {
            for inst in instruments {
                let key = Ustr::from(inst.raw_symbol().as_str());
                guard.insert(key, inst);
            }
        }
    }

    /// Look up a cached instrument by raw Bullet symbol.
    #[must_use]
    pub fn get_instrument(&self, symbol: &str) -> Option<InstrumentAny> {
        self.instruments
            .lock()
            .ok()
            .and_then(|g| g.get(&Ustr::from(symbol)).cloned())
    }

    /// Connect to the Bullet WS endpoint and start the reconnect loop.
    ///
    /// Returns immediately after the loop is spawned; the first connection
    /// attempt happens asynchronously.  Use `wait_until_active()` if you need
    /// to block until the initial handshake completes.
    ///
    /// Calling `connect()` on an already-started client is a no-op.
    ///
    /// # Errors
    ///
    /// Returns an error only if the internal channel is in an inconsistent state.
    pub async fn connect(&self) -> Result<(), BulletError> {
        if self.started.load(Ordering::Relaxed) {
            return Ok(());
        }

        self.stop_flag.store(false, Ordering::Relaxed);
        self.started.store(true, Ordering::Relaxed);

        // Create the long-lived output channel.
        let (read_tx, read_rx) = mpsc::unbounded_channel::<ServerMessage>();
        *self.read_rx.lock().await = Some(read_rx);

        // Clone everything the reconnect loop needs.
        let url = self.url.clone();
        let write_tx = Arc::clone(&self.write_tx);
        let active_topics = Arc::clone(&self.active_topics);
        let next_id = Arc::clone(&self.next_id);
        let running = Arc::clone(&self.running);
        let started = Arc::clone(&self.started);
        let stop_flag = Arc::clone(&self.stop_flag);

        tokio::spawn(async move {
            // Ensure a TLS crypto provider is registered when both aws-lc-rs and ring
            // are in the dependency tree (rustls panics without an explicit default).
            let _ = rustls::crypto::aws_lc_rs::default_provider().install_default();

            let mut delay = Duration::from_millis(250);

            'outer: loop {
                if stop_flag.load(Ordering::Relaxed) {
                    break;
                }

                match connect_async(url.as_str()).await {
                    Ok((ws_stream, _)) => {
                        delay = Duration::from_millis(250); // reset back-off

                        let (mut sink, mut stream) = ws_stream.split();

                        // Per-connection write channel.
                        let (conn_tx, mut conn_rx) = mpsc::unbounded_channel::<Message>();
                        {
                            let mut guard = write_tx.lock().expect("write_tx poisoned");
                            *guard = Some(conn_tx.clone());
                        }

                        // Replay active subscriptions.
                        {
                            let active = active_topics.lock().await;
                            if !active.is_empty() {
                                let params: Vec<String> = active.iter().cloned().collect();
                                let id = next_id.fetch_add(1, Ordering::Relaxed);
                                if let Ok(json) =
                                    serde_json::to_string(&SubscribeRequest::subscribe(params, id))
                                {
                                    let _ = conn_tx.send(Message::Text(json.into()));
                                }
                            }
                        }

                        running.store(true, Ordering::Relaxed);
                        tracing::info!(url = %url, "Bullet WS connected");

                        // Write task: pump outbound messages to the WS sink.
                        tokio::spawn(async move {
                            while let Some(msg) = conn_rx.recv().await {
                                if sink.send(msg).await.is_err() {
                                    break;
                                }
                            }
                        });

                        // Read loop.
                        while let Some(frame) = stream.next().await {
                            if stop_flag.load(Ordering::Relaxed) {
                                break;
                            }
                            match frame {
                                Ok(Message::Text(text)) => {
                                    tracing::debug!(raw = %text, "WS frame");
                                    match serde_json::from_str::<ServerMessage>(&text) {
                                        Ok(msg) => {
                                            if read_tx.send(msg).is_err() {
                                                // Receiver dropped — stop everything.
                                                break 'outer;
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                error = %e,
                                                raw = %text,
                                                "failed to parse WS message"
                                            );
                                        }
                                    }
                                }
                                Ok(Message::Close(_)) | Err(_) => break,
                                _ => {}
                            }
                        }

                        // Connection dropped — clear write sender.
                        {
                            let mut guard = write_tx.lock().expect("write_tx poisoned");
                            *guard = None;
                        }
                        running.store(false, Ordering::Relaxed);

                        if stop_flag.load(Ordering::Relaxed) {
                            break;
                        }

                        tracing::info!(retry_in = ?delay, "Bullet WS disconnected, will reconnect");
                    }
                    Err(e) => {
                        tracing::warn!(error = %e, retry_in = ?delay, "Bullet WS connect failed");
                    }
                }

                tokio::time::sleep(delay).await;
                delay = (delay * 2).min(Duration::from_secs(30));
            }

            running.store(false, Ordering::Relaxed);
            started.store(false, Ordering::Relaxed);
            tracing::debug!("Bullet WS reconnect loop exited");
        });

        Ok(())
    }

    /// Signal the reconnect loop to stop and close the current connection.
    pub fn disconnect(&self) {
        self.stop_flag.store(true, Ordering::Relaxed);
        if let Ok(mut guard) = self.write_tx.lock() {
            *guard = None;
        }
        self.running.store(false, Ordering::Relaxed);
    }

    /// Wait until the WebSocket is actively connected, or until `timeout_secs` elapse.
    ///
    /// # Errors
    ///
    /// Returns a timeout error if the connection does not become active in time.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), BulletError> {
        let start = std::time::Instant::now();
        loop {
            if self.running.load(Ordering::Relaxed) {
                return Ok(());
            }
            if start.elapsed().as_secs_f64() >= timeout_secs {
                return Err(BulletError::WebSocket(format!(
                    "WS did not become active within {timeout_secs}s"
                )));
            }
            tokio::time::sleep(Duration::from_millis(50)).await;
        }
    }

    /// Receive the next parsed server message.
    ///
    /// Returns `None` if the connection is closed and all messages have been drained.
    pub async fn next_event(&self) -> Option<ServerMessage> {
        let mut guard = self.read_rx.lock().await;
        match guard.as_mut() {
            Some(rx) => rx.recv().await,
            None => None,
        }
    }

    // ── Subscription helpers ──────────────────────────────────────────────────

    /// Subscribe to one or more topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected.
    pub async fn subscribe(&self, topics: impl IntoIterator<Item = Topic>) -> Result<(), BulletError> {
        let params: Vec<String> = topics.into_iter().map(|t| t.to_string()).collect();
        if params.is_empty() {
            return Ok(());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send_json(&SubscribeRequest::subscribe(params.clone(), id))?;
        let mut active = self.active_topics.lock().await;
        for p in params {
            active.insert(p);
        }
        Ok(())
    }

    /// Unsubscribe from one or more topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the client is not connected.
    pub async fn unsubscribe(
        &self,
        topics: impl IntoIterator<Item = Topic>,
    ) -> Result<(), BulletError> {
        let params: Vec<String> = topics.into_iter().map(|t| t.to_string()).collect();
        if params.is_empty() {
            return Ok(());
        }
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        self.send_json(&SubscribeRequest::unsubscribe(params.clone(), id))?;
        let mut active = self.active_topics.lock().await;
        for p in &params {
            active.remove(p);
        }
        Ok(())
    }

    // ── Convenience subscribe wrappers ────────────────────────────────────────

    /// Subscribe to best bid/ask for a symbol.
    pub async fn subscribe_quotes_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.subscribe([Topic::book_ticker(symbol)]).await
    }

    /// Unsubscribe from best bid/ask for a symbol.
    pub async fn unsubscribe_quotes_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.unsubscribe([Topic::book_ticker(symbol)]).await
    }

    /// Subscribe to aggregated trades for a symbol.
    pub async fn subscribe_trades_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.subscribe([Topic::agg_trade(symbol)]).await
    }

    /// Unsubscribe from aggregated trades for a symbol.
    pub async fn unsubscribe_trades_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.unsubscribe([Topic::agg_trade(symbol)]).await
    }

    /// Subscribe to L2 depth (top 20) for a symbol.
    pub async fn subscribe_book_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.subscribe([Topic::depth(symbol, crate::websocket::topics::DepthLevel::D20)]).await
    }

    /// Unsubscribe from L2 depth for a symbol.
    pub async fn unsubscribe_book_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.unsubscribe([Topic::depth(symbol, crate::websocket::topics::DepthLevel::D20)]).await
    }

    /// Subscribe to mark price / funding rate for a symbol.
    pub async fn subscribe_mark_price_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.subscribe([Topic::mark_price(symbol)]).await
    }

    /// Unsubscribe from mark price for a symbol.
    pub async fn unsubscribe_mark_price_for_symbol(&self, symbol: &str) -> Result<(), BulletError> {
        self.unsubscribe([Topic::mark_price(symbol)]).await
    }

    /// Subscribe to authenticated order updates for an address.
    pub async fn subscribe_order_updates_for_address(
        &self,
        address: &str,
    ) -> Result<(), BulletError> {
        self.subscribe([Topic::user_orders(address)]).await
    }

    /// Unsubscribe from order updates for an address.
    pub async fn unsubscribe_order_updates_for_address(
        &self,
        address: &str,
    ) -> Result<(), BulletError> {
        self.unsubscribe([Topic::user_orders(address)]).await
    }

    // ── Internal ──────────────────────────────────────────────────────────────

    fn send_json<T: serde::Serialize>(&self, msg: &T) -> Result<(), BulletError> {
        let guard = self
            .write_tx
            .lock()
            .map_err(|_| BulletError::WebSocket("write_tx mutex poisoned".to_string()))?;
        let tx = guard.as_ref().ok_or_else(|| {
            BulletError::WebSocket("not connected — call connect() first".to_string())
        })?;
        let json = serde_json::to_string(msg)
            .map_err(|e| BulletError::WebSocket(e.to_string()))?;
        tx.send(Message::Text(json.into()))
            .map_err(|e| BulletError::WebSocket(e.to_string()))
    }
}
