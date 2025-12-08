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

//! WebSocket client for Lighter public market data.

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, RwLock, atomic::AtomicU8},
};

use anyhow::{Context, Result, anyhow};
use nautilus_core::time::get_atomic_clock_realtime;
use nautilus_model::instruments::{Instrument, InstrumentAny};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{WebSocketClient, WebSocketConfig, channel_message_handler},
};
use serde_json::json;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

use crate::{
    common::LighterNetwork,
    http::client::LighterHttpClient,
    urls::get_ws_url,
    websocket::{
        messages::{NautilusWsMessage, WsMessage},
        parse::parse_ws_message,
    },
};

/// WebSocket client focused on public data streams (order books, trades, market stats).
#[derive(Debug, Clone)]
pub struct LighterWebSocketClient {
    url: String,
    client: Option<Arc<WebSocketClient>>,
    connection_mode: Arc<AtomicU8>,
    instruments: Arc<RwLock<HashMap<u32, InstrumentAny>>>,
    subscriptions: Arc<RwLock<HashSet<String>>>,
    out_rx: Arc<RwLock<Option<mpsc::UnboundedReceiver<NautilusWsMessage>>>>,
    ts_init: nautilus_core::nanos::UnixNanos,
    meta_client: Option<LighterHttpClient>,
}

impl LighterWebSocketClient {
    /// Create a new client for the given network.
    pub fn new(
        network: LighterNetwork,
        base_url_override: Option<&str>,
        meta: Option<LighterHttpClient>,
    ) -> Self {
        Self {
            url: get_ws_url(network, base_url_override),
            client: None,
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)),
            instruments: Arc::new(RwLock::new(HashMap::new())),
            subscriptions: Arc::new(RwLock::new(HashSet::new())),
            out_rx: Arc::new(RwLock::new(None)),
            ts_init: get_atomic_clock_realtime().get_time_ns(),
            meta_client: meta,
        }
    }

    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Cache an instrument with its Lighter market index for downstream parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny, market_index: Option<u32>) {
        let resolved_index = market_index.or_else(|| {
            self.meta_client
                .as_ref()
                .and_then(|client| client.get_market_index(&instrument.id()))
        });

        if let Some(index) = resolved_index {
            if let Ok(mut map) = self.instruments.write() {
                map.insert(index, instrument);
            }
        } else {
            warn!(
                instrument_id = %instrument.id(),
                "Unable to cache instrument without market index",
            );
        }
    }

    /// Establish the WebSocket connection and spawn the reader loop.
    pub async fn connect(&mut self) -> Result<()> {
        if self.is_active() {
            return Ok(());
        }

        let (handler, mut raw_rx) = channel_message_handler();
        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            message_handler: Some(handler),
            heartbeat: None,
            heartbeat_msg: None,
            ping_handler: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(10_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(500),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(cfg, None, vec![], None)
            .await
            .context("failed to connect Lighter WebSocket")?;
        self.connection_mode = client.connection_mode_atomic();
        let client = Arc::new(client);
        self.client = Some(Arc::clone(&client));

        let (out_tx, out_rx) = mpsc::unbounded_channel::<NautilusWsMessage>();
        if let Ok(mut guard) = self.out_rx.write() {
            *guard = Some(out_rx);
        }

        let instruments = Arc::clone(&self.instruments);
        let subscriptions = Arc::clone(&self.subscriptions);
        let ts_init = self.ts_init;
        tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                match msg.into_text() {
                    Ok(text) => {
                        if let Err(err) = handle_text_message(
                            &text,
                            &out_tx,
                            &client,
                            &instruments,
                            &subscriptions,
                            ts_init,
                        )
                        .await
                        {
                            warn!(%err, "Failed to handle Lighter WebSocket message");
                        }
                    }
                    Err(err) => {
                        warn!(%err, "Ignoring non-text WebSocket message");
                    }
                }
            }
        });

        info!("Connected to Lighter WebSocket {}", self.url);
        Ok(())
    }

    /// Gracefully disconnect the client.
    pub async fn close(&self) {
        if let Some(client) = self.client.as_ref() {
            client.disconnect().await;
        }
        if let Ok(mut guard) = self.out_rx.write() {
            *guard = None;
        }
    }

    /// Wait for the connection to reach `ACTIVE` state (or timeout).
    pub async fn wait_until_active(&self, timeout_ms: u64) -> Result<()> {
        let deadline = tokio::time::Instant::now() + std::time::Duration::from_millis(timeout_ms);
        loop {
            if self.is_active() {
                return Ok(());
            }
            if tokio::time::Instant::now() > deadline {
                anyhow::bail!("Timed out waiting for Lighter WebSocket to become active");
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
    }

    #[must_use]
    pub fn is_active(&self) -> bool {
        ConnectionMode::from_atomic(&self.connection_mode).is_active()
    }

    /// Subscribe to order book updates for the given market index.
    pub async fn subscribe_order_book(&self, market_index: u32) -> Result<()> {
        let channel = format!("order_book/{market_index}");
        self.send_subscribe(&channel).await
    }

    /// Subscribe to trades for the given market index.
    pub async fn subscribe_trades(&self, market_index: u32) -> Result<()> {
        let channel = format!("trade/{market_index}");
        self.send_subscribe(&channel).await
    }

    /// Subscribe to market stats for the given market index.
    pub async fn subscribe_market_stats(&self, market_index: u32) -> Result<()> {
        let channel = format!("market_stats/{market_index}");
        self.send_subscribe(&channel).await
    }

    pub async fn unsubscribe_order_book(&self, market_index: u32) -> Result<()> {
        let channel = format!("order_book/{market_index}");
        self.send_unsubscribe(&channel).await
    }

    pub async fn unsubscribe_trades(&self, market_index: u32) -> Result<()> {
        let channel = format!("trade/{market_index}");
        self.send_unsubscribe(&channel).await
    }

    pub async fn unsubscribe_market_stats(&self, market_index: u32) -> Result<()> {
        let channel = format!("market_stats/{market_index}");
        self.send_unsubscribe(&channel).await
    }

    /// Receive the next parsed WebSocket event (if connected).
    pub async fn next_event(&self) -> Option<NautilusWsMessage> {
        let mut guard = self.out_rx.write().ok()?;
        let rx = guard.as_mut()?;
        rx.recv().await
    }

    async fn send_subscribe(&self, channel: &str) -> Result<()> {
        self.send_message(channel, "subscribe").await?;
        if let Ok(mut guard) = self.subscriptions.write() {
            guard.insert(channel.to_string());
        }
        Ok(())
    }

    async fn send_unsubscribe(&self, channel: &str) -> Result<()> {
        self.send_message(channel, "unsubscribe").await?;
        if let Ok(mut guard) = self.subscriptions.write() {
            guard.remove(channel);
        }
        Ok(())
    }

    async fn send_message(&self, channel: &str, msg_type: &str) -> Result<()> {
        let payload = json!({
            "type": msg_type,
            "channel": channel,
        });
        let Some(client) = self.client.as_ref() else {
            return Err(anyhow!("WebSocket client is not connected"));
        };
        client
            .send_text(payload.to_string(), None)
            .await
            .context("failed to send WebSocket message")
    }

    async fn resubscribe_all(
        client: &WebSocketClient,
        subscriptions: &Arc<RwLock<HashSet<String>>>,
    ) {
        let channels = subscriptions
            .read()
            .map(|set| set.clone())
            .unwrap_or_default();

        for channel in channels {
            let payload = json!({
                "type": "subscribe",
                "channel": channel,
            });
            if let Err(err) = client.send_text(payload.to_string(), None).await {
                warn!(%err, "Failed to resubscribe to {channel}");
            } else {
                debug!(%channel, "Resubscribed to Lighter channel");
            }
        }
    }
}

async fn handle_text_message(
    text: &str,
    out_tx: &mpsc::UnboundedSender<NautilusWsMessage>,
    client: &Arc<WebSocketClient>,
    instruments: &Arc<RwLock<HashMap<u32, InstrumentAny>>>,
    subscriptions: &Arc<RwLock<HashSet<String>>>,
    ts_init: nautilus_core::nanos::UnixNanos,
) -> Result<()> {
    let message: WsMessage =
        serde_json::from_str(text).context("failed to deserialize WS message")?;

    if matches!(message, WsMessage::Connected { .. }) {
        info!("Lighter WebSocket connected, resubscribing active channels");
        LighterWebSocketClient::resubscribe_all(client, subscriptions).await;
        return Ok(());
    }

    let instruments_guard = instruments
        .read()
        .map_err(|_| anyhow!("instrument cache poisoned"))?;
    let events = parse_ws_message(message, &*instruments_guard, ts_init)?;

    for event in events {
        if let Err(err) = out_tx.send(event) {
            error!(%err, "Failed to enqueue WebSocket event");
        }
    }

    Ok(())
}
