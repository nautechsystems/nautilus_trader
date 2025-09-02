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

use anyhow::Result;
use nautilus_network::websocket::{WebSocketClient, WebSocketConfig, channel_message_handler};
use tokio::sync::mpsc;
use tokio_tungstenite::tungstenite::Message;
use tracing::{debug, error, info, warn};

use crate::websocket::messages::{HyperliquidWsMessage, HyperliquidWsRequest};

/// Hyperliquid WebSocket client that wraps Nautilus WebSocketClient for lifecycle management.
#[derive(Debug)]
pub struct HyperliquidWebSocketClient {
    client: WebSocketClient,
    _reader_task: tokio::task::JoinHandle<()>,
}

impl HyperliquidWebSocketClient {
    /// Creates a new Hyperliquid WebSocket client with Nautilus' reconnection/backoff/heartbeat.
    /// Returns (client, rx) where `rx` yields Hyperliquid-native `HyperliquidWsMessage` events.
    pub async fn connect(url: &str) -> Result<(Self, mpsc::Receiver<HyperliquidWsMessage>)> {
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
            client,
            _reader_task: reader_task,
        };

        Ok((hl_client, rx_inbound))
    }

    /// Sends a Hyperliquid WebSocket request via the Nautilus WebSocket client.
    pub async fn send(&self, request: &HyperliquidWsRequest) -> Result<()> {
        let json = serde_json::to_string(request)?;
        debug!("Sending WS message: {}", json);
        self.client
            .send_text(json, None)
            .await
            .map_err(|e| anyhow::anyhow!(e))
    }

    /// Returns true if the WebSocket connection is active.
    pub fn is_active(&self) -> bool {
        self.client.is_active()
    }

    /// Returns true if the WebSocket is reconnecting.
    pub fn is_reconnecting(&self) -> bool {
        self.client.is_reconnecting()
    }

    /// Returns true if the WebSocket is disconnecting.
    pub fn is_disconnecting(&self) -> bool {
        self.client.is_disconnecting()
    }

    /// Returns true if the WebSocket is closed.
    pub fn is_closed(&self) -> bool {
        self.client.is_closed()
    }

    /// Disconnect the WebSocket client.
    pub async fn disconnect(&mut self) -> Result<()> {
        self.client.disconnect().await;
        Ok(())
    }
}
