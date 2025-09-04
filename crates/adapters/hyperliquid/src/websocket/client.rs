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

use std::collections::HashSet;

use anyhow::Result;
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::websocket::messages::{HyperliquidWsMessage, HyperliquidWsRequest, SubscriptionRequest};

/// Errors that can occur during Hyperliquid WebSocket operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum HyperliquidError {
    #[error("URL parsing failed: {0}")]
    UrlParsing(String),

    #[error("Message serialization failed: {0}")]
    MessageSerialization(String),

    #[error("Message deserialization failed: {0}")]
    MessageDeserialization(String),

    #[error("WebSocket connection failed: {0}")]
    Connection(String),

    #[error("Channel send failed: {0}")]
    ChannelSend(String),
}

/// Codec for encoding and decoding Hyperliquid WebSocket messages.
///
/// This struct provides methods to validate URLs and serialize/deserialize messages
/// according to the Hyperliquid WebSocket protocol.
#[derive(Debug, Default)]
pub struct HyperliquidCodec;

impl HyperliquidCodec {
    /// Creates a new Hyperliquid codec instance.
    pub fn new() -> Self {
        Self
    }

    /// Validates that a URL is a proper WebSocket URL.
    pub fn validate_url(url: &str) -> Result<(), HyperliquidError> {
        if url.starts_with("ws://") || url.starts_with("wss://") {
            Ok(())
        } else {
            Err(HyperliquidError::UrlParsing(format!(
                "URL must start with ws:// or wss://, got: {}",
                url
            )))
        }
    }

    /// Encodes a WebSocket request to JSON bytes.
    pub fn encode(&self, request: &HyperliquidWsRequest) -> Result<Vec<u8>, HyperliquidError> {
        serde_json::to_vec(request).map_err(|e| {
            HyperliquidError::MessageSerialization(format!("Failed to serialize request: {}", e))
        })
    }

    /// Decodes JSON bytes to a WebSocket message.
    pub fn decode(&self, data: &[u8]) -> Result<HyperliquidWsMessage, HyperliquidError> {
        serde_json::from_slice(data).map_err(|e| {
            HyperliquidError::MessageDeserialization(format!(
                "Failed to deserialize message: {}",
                e
            ))
        })
    }
}

/// Low-level Hyperliquid WebSocket client that wraps Nautilus WebSocketClient.
///
/// This is the inner client that handles the transport layer and provides low-level
/// WebSocket methods with `ws_*` prefixes.
#[derive(Debug)]
pub struct HyperliquidWebSocketInnerClient {
    inner: WebSocketClient,
    rx_inbound: mpsc::Receiver<HyperliquidWsMessage>,
    sent_subscriptions: HashSet<String>,
    _reader_task: tokio::task::JoinHandle<()>,
}

impl HyperliquidWebSocketInnerClient {
    /// Creates a new Hyperliquid WebSocket inner client with Nautilus' reconnection/backoff/heartbeat.
    /// Returns a client that owns the inbound message receiver.
    pub async fn connect(url: &str) -> Result<Self> {
        // Create message handler for receiving raw WebSocket messages
        let (message_handler, mut raw_rx) = channel_message_handler();

        let cfg = WebSocketConfig {
            url: url.to_string(),
            headers: vec![],
            message_handler: Some(message_handler),
            heartbeat: Some(20), // seconds; set lower than server idle timeout
            heartbeat_msg: None, // use WS Ping frames by default
            ping_handler: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
        };

        let client = WebSocketClient::connect(cfg, None, vec![], None).await?;
        info!("Hyperliquid WebSocket connected: {}", url);

        // Decode task – turns tungstenite Messages into HyperliquidWsMessage
        let (tx_inbound, rx_inbound) = mpsc::channel::<HyperliquidWsMessage>(1024);
        let reader_task = tokio::spawn(async move {
            while let Some(msg) = raw_rx.recv().await {
                match msg {
                    Message::Text(txt) => {
                        debug!("Received WS text: {}", txt);
                        match serde_json::from_str::<HyperliquidWsMessage>(&txt) {
                            Ok(hl_msg) => {
                                if let Err(e) = tx_inbound.send(hl_msg).await {
                                    error!("Failed to send decoded message: {}", e);
                                    break;
                                }
                            }
                            Err(err) => {
                                error!(
                                    "Failed to decode Hyperliquid message: {} | text: {}",
                                    err, txt
                                );
                                // Continue processing other messages instead of breaking
                            }
                        }
                    }
                    Message::Binary(data) => {
                        debug!("Received binary message ({} bytes), ignoring", data.len());
                    }
                    Message::Ping(data) => {
                        debug!("Received ping frame ({} bytes)", data.len());
                        // Nautilus handles pong automatically
                    }
                    Message::Pong(data) => {
                        debug!("Received pong frame ({} bytes)", data.len());
                        // Nautilus updates heartbeat internally
                    }
                    Message::Close(close_frame) => {
                        info!("Received close frame: {:?}", close_frame);
                        break;
                    }
                    Message::Frame(_) => {
                        warn!("Received raw frame (unexpected)");
                    }
                }
            }
            info!("Hyperliquid WebSocket reader finished");
        });

        let hl_client = Self {
            inner: client,
            rx_inbound,
            sent_subscriptions: HashSet::new(),
            _reader_task: reader_task,
        };

        Ok(hl_client)
    }

    /// Low-level method to send a Hyperliquid WebSocket request.
    pub async fn ws_send(&self, request: &HyperliquidWsRequest) -> Result<()> {
        let json = serde_json::to_string(request)?;
        debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Low-level method to send a request only once (dedup by JSON serialization).
    pub async fn ws_send_once(&mut self, request: &HyperliquidWsRequest) -> Result<()> {
        let json = serde_json::to_string(request)?;
        if self.sent_subscriptions.contains(&json) {
            debug!("Skipping duplicate request: {}", json);
            return Ok(());
        }

        debug!("Sending WS message: {}", json);
        self.inner
            .send_text(json.clone(), None)
            .await
            .map_err(|e| anyhow::anyhow!(e))?;

        self.sent_subscriptions.insert(json);
        Ok(())
    }

    /// Low-level method to subscribe to a specific channel.
    pub async fn ws_subscribe(&mut self, subscription: SubscriptionRequest) -> Result<()> {
        let request = HyperliquidWsRequest::Subscribe { subscription };
        self.ws_send_once(&request).await
    }

    /// Get the next event from the WebSocket stream.
    /// Returns None when the connection is closed or the receiver is exhausted.
    pub async fn ws_next_event(&mut self) -> Option<HyperliquidWsMessage> {
        self.rx_inbound.recv().await
    }

    /// Returns true if the WebSocket connection is active.
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Returns true if the WebSocket is reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        self.inner.is_reconnecting()
    }

    /// Returns true if the WebSocket is disconnecting.
    pub fn is_disconnecting(&self) -> bool {
        self.inner.is_disconnecting()
    }

    /// Returns true if the WebSocket is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Disconnect the WebSocket client.
    pub async fn ws_disconnect(&mut self) -> Result<()> {
        self.inner.disconnect().await;
        Ok(())
    }
}

/// High-level Hyperliquid WebSocket client that provides standardized domain methods.
///
/// This is the outer client that wraps the inner client and provides Nautilus-specific
/// functionality for WebSocket operations using standard domain methods.
#[derive(Debug)]
pub struct HyperliquidWebSocketClient {
    inner: HyperliquidWebSocketInnerClient,
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client.
    pub async fn connect(url: &str) -> Result<Self> {
        let inner = HyperliquidWebSocketInnerClient::connect(url).await?;
        Ok(Self { inner })
    }

    /// Subscribe to order updates for a specific user address.
    pub async fn subscribe_order_updates(&mut self, user: &str) -> Result<()> {
        let subscription = SubscriptionRequest::OrderUpdates {
            user: user.to_string(),
        };
        self.inner.ws_subscribe(subscription).await
    }

    /// Subscribe to user events (fills, funding, liquidations) for a specific user address.
    pub async fn subscribe_user_events(&mut self, user: &str) -> Result<()> {
        let subscription = SubscriptionRequest::UserEvents {
            user: user.to_string(),
        };
        self.inner.ws_subscribe(subscription).await
    }

    /// Subscribe to all user channels (order updates + user events) for convenience.
    pub async fn subscribe_all_user_channels(&mut self, user: &str) -> Result<()> {
        self.subscribe_order_updates(user).await?;
        self.subscribe_user_events(user).await?;
        Ok(())
    }

    /// Get the next event from the WebSocket stream.
    /// Returns None when the connection is closed or the receiver is exhausted.
    pub async fn next_event(&mut self) -> Option<HyperliquidWsMessage> {
        self.inner.ws_next_event().await
    }

    /// Returns true if the WebSocket connection is active.
    pub fn is_active(&self) -> bool {
        self.inner.is_active()
    }

    /// Returns true if the WebSocket is reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        self.inner.is_reconnecting()
    }

    /// Returns true if the WebSocket is disconnecting.
    pub fn is_disconnecting(&self) -> bool {
        self.inner.is_disconnecting()
    }

    /// Returns true if the WebSocket is closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Disconnect the WebSocket client.
    pub async fn disconnect(&mut self) -> Result<()> {
        self.inner.ws_disconnect().await
    }

    /// Escape hatch: send raw requests for tests/power users.
    pub async fn send_raw(&mut self, request: &HyperliquidWsRequest) -> Result<()> {
        self.inner.ws_send(request).await
    }
}
