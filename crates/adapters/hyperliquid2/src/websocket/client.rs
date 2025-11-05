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

//! Hyperliquid WebSocket client implementation.

use crate::common::{
    HyperliquidUrls, HyperliquidWebSocketError, HyperliquidWsChannel, SubscriptionStatus,
};
use futures_util::{stream::{SplitSink, SplitStream}, SinkExt, StreamExt};
use serde_json::{json, Value};
use std::{collections::HashMap, sync::Arc};
use tokio::{
    net::TcpStream,
    sync::{mpsc, RwLock},
    time::{interval, Duration},
};
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, error, trace, warn};

type WsStream = WebSocketStream<MaybeTlsStream<TcpStream>>;
type WsSink = SplitSink<WsStream, Message>;
type WsReceiver = SplitStream<WsStream>;

/// Manages WebSocket subscriptions
struct SubscriptionManager {
    subscriptions: RwLock<HashMap<String, SubscriptionStatus>>,
}

impl SubscriptionManager {
    fn new() -> Self {
        Self {
            subscriptions: RwLock::new(HashMap::new()),
        }
    }

    async fn add_subscription(&self, channel: &str, status: SubscriptionStatus) {
        let mut subs = self.subscriptions.write().await;
        subs.insert(channel.to_string(), status);
    }

    async fn update_status(&self, channel: &str, status: SubscriptionStatus) {
        let mut subs = self.subscriptions.write().await;
        if let Some(sub_status) = subs.get_mut(channel) {
            *sub_status = status;
        }
    }

    async fn get_status(&self, channel: &str) -> Option<SubscriptionStatus> {
        let subs = self.subscriptions.read().await;
        subs.get(channel).copied()
    }

    async fn remove_subscription(&self, channel: &str) {
        let mut subs = self.subscriptions.write().await;
        subs.remove(channel);
    }
}

/// Inner state for Hyperliquid WebSocket client
pub struct Hyperliquid2WebSocketClientInner {
    urls: HyperliquidUrls,
    subscription_manager: SubscriptionManager,
    write_tx: Option<mpsc::UnboundedSender<Message>>,
    read_rx: Option<mpsc::UnboundedReceiver<Message>>,
}

/// Hyperliquid WebSocket client
#[derive(Clone)]
pub struct Hyperliquid2WebSocketClient {
    inner: Arc<RwLock<Hyperliquid2WebSocketClientInner>>,
}

impl Hyperliquid2WebSocketClient {
    /// Creates a new [`Hyperliquid2WebSocketClient`] instance
    ///
    /// # Parameters
    /// - `ws_base`: Optional custom WebSocket base URL
    /// - `testnet`: Whether to use testnet (default: false)
    pub fn new(ws_base: Option<String>, testnet: bool) -> anyhow::Result<Self> {
        let urls = HyperliquidUrls::new(None, ws_base, testnet)?;

        let inner = Hyperliquid2WebSocketClientInner {
            urls,
            subscription_manager: SubscriptionManager::new(),
            write_tx: None,
            read_rx: None,
        };

        Ok(Self {
            inner: Arc::new(RwLock::new(inner)),
        })
    }

    /// Connects to the WebSocket
    pub async fn connect(&self) -> Result<(), HyperliquidWebSocketError> {
        let mut inner = self.inner.write().await;
        let url = &inner.urls.ws_base;

        debug!("Connecting to Hyperliquid WebSocket: {}", url);

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(|e| HyperliquidWebSocketError::Connection(e.to_string()))?;

        debug!("WebSocket connected successfully");

        let (write, read) = ws_stream.split();

        // Create channels for write and read
        let (write_tx, write_rx) = mpsc::unbounded_channel::<Message>();
        let (read_tx, read_rx) = mpsc::unbounded_channel::<Message>();

        inner.write_tx = Some(write_tx);
        inner.read_rx = Some(read_rx);

        // Spawn write task
        tokio::spawn(write_task(write, write_rx));

        // Spawn read task
        tokio::spawn(read_task(read, read_tx));

        // Spawn ping task
        let write_tx_clone = inner.write_tx.as_ref().unwrap().clone();
        tokio::spawn(ping_task(write_tx_clone));

        Ok(())
    }

    /// Subscribes to a channel
    pub async fn subscribe(
        &self,
        channel: HyperliquidWsChannel,
    ) -> Result<(), HyperliquidWebSocketError> {
        let inner = self.inner.read().await;
        let channel_name = channel.channel_name();

        debug!("Subscribing to channel: {}", channel_name);

        // Add subscription with Pending status
        inner
            .subscription_manager
            .add_subscription(&channel_name, SubscriptionStatus::Pending)
            .await;

        // Create subscription message
        let sub_msg = json!({
            "method": "subscribe",
            "subscription": channel_to_subscription(&channel)
        });

        // Send subscription message
        if let Some(tx) = &inner.write_tx {
            let msg = Message::Text(sub_msg.to_string().into());
            tx.send(msg)
                .map_err(|e| HyperliquidWebSocketError::Send(e.to_string()))?;
        } else {
            return Err(HyperliquidWebSocketError::Connection(
                "Not connected".to_string(),
            ));
        }

        // Update status to Subscribed
        inner
            .subscription_manager
            .update_status(&channel_name, SubscriptionStatus::Subscribed)
            .await;

        debug!("Subscribed to channel: {}", channel_name);
        Ok(())
    }

    /// Unsubscribes from a channel
    pub async fn unsubscribe(
        &self,
        channel: HyperliquidWsChannel,
    ) -> Result<(), HyperliquidWebSocketError> {
        let inner = self.inner.read().await;
        let channel_name = channel.channel_name();

        debug!("Unsubscribing from channel: {}", channel_name);

        // Update status to Unsubscribing
        inner
            .subscription_manager
            .update_status(&channel_name, SubscriptionStatus::Unsubscribing)
            .await;

        // Create unsubscription message
        let unsub_msg = json!({
            "method": "unsubscribe",
            "subscription": channel_to_subscription(&channel)
        });

        // Send unsubscription message
        if let Some(tx) = &inner.write_tx {
            let msg = Message::Text(unsub_msg.to_string().into());
            tx.send(msg)
                .map_err(|e| HyperliquidWebSocketError::Send(e.to_string()))?;
        } else {
            return Err(HyperliquidWebSocketError::Connection(
                "Not connected".to_string(),
            ));
        }

        // Remove subscription
        inner.subscription_manager.remove_subscription(&channel_name).await;

        debug!("Unsubscribed from channel: {}", channel_name);
        Ok(())
    }

    /// Receives a message from the WebSocket
    pub async fn receive(&self) -> Result<Option<String>, HyperliquidWebSocketError> {
        let mut inner = self.inner.write().await;

        if let Some(rx) = &mut inner.read_rx {
            match rx.recv().await {
                Some(Message::Text(text)) => {
                    let text_str = text.to_string();
                    trace!("Received message: {}", text_str);
                    Ok(Some(text_str))
                }
                Some(Message::Binary(data)) => {
                    trace!("Received binary message: {} bytes", data.len());
                    let text = String::from_utf8_lossy(&data).to_string();
                    Ok(Some(text))
                }
                Some(Message::Ping(_)) => {
                    trace!("Received ping");
                    Ok(None)
                }
                Some(Message::Pong(_)) => {
                    trace!("Received pong");
                    Ok(None)
                }
                Some(Message::Close(_)) => {
                    warn!("Received close frame");
                    Err(HyperliquidWebSocketError::Connection(
                        "Connection closed".to_string(),
                    ))
                }
                None => Ok(None),
                _ => Ok(None),
            }
        } else {
            Err(HyperliquidWebSocketError::Connection(
                "Not connected".to_string(),
            ))
        }
    }

    /// Checks subscription status for a channel
    pub async fn is_subscribed(&self, channel: &HyperliquidWsChannel) -> bool {
        let inner = self.inner.read().await;
        let channel_name = channel.channel_name();

        matches!(
            inner.subscription_manager.get_status(&channel_name).await,
            Some(SubscriptionStatus::Subscribed)
        )
    }
}

/// Converts channel to subscription object
fn channel_to_subscription(channel: &HyperliquidWsChannel) -> Value {
    match channel {
        HyperliquidWsChannel::AllMids => json!({ "type": "allMids" }),
        HyperliquidWsChannel::Trades { coin } => json!({ "type": "trades", "coin": coin }),
        HyperliquidWsChannel::L2Book { coin } => json!({ "type": "l2Book", "coin": coin }),
        HyperliquidWsChannel::Candle { coin, interval } => {
            json!({ "type": "candle", "coin": coin, "interval": interval })
        }
        HyperliquidWsChannel::User { user } => json!({ "type": "user", "user": user }),
        HyperliquidWsChannel::UserFills { user } => json!({ "type": "userFills", "user": user }),
    }
}

/// Write task for sending messages
async fn write_task(
    mut write: WsSink,
    mut write_rx: mpsc::UnboundedReceiver<Message>,
) {
    while let Some(msg) = write_rx.recv().await {
        if let Err(e) = write.send(msg).await {
            error!("WebSocket write error: {}", e);
            break;
        }
    }
    debug!("Write task ended");
}

/// Read task for receiving messages
async fn read_task(
    mut read: WsReceiver,
    read_tx: mpsc::UnboundedSender<Message>,
) {
    while let Some(result) = read.next().await {
        match result {
            Ok(msg) => {
                if read_tx.send(msg).is_err() {
                    error!("Failed to send message to read channel");
                    break;
                }
            }
            Err(e) => {
                error!("WebSocket read error: {}", e);
                break;
            }
        }
    }
    debug!("Read task ended");
}

/// Ping task for keeping connection alive
async fn ping_task(write_tx: mpsc::UnboundedSender<Message>) {
    let mut ping_interval = interval(Duration::from_secs(30));

    loop {
        ping_interval.tick().await;
        if write_tx.send(Message::Ping(vec![].into())).is_err() {
            error!("Failed to send ping");
            break;
        }
        trace!("Sent ping");
    }

    debug!("Ping task ended");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = Hyperliquid2WebSocketClient::new(None, false);
        assert!(client.is_ok());
    }

    #[test]
    fn test_channel_to_subscription() {
        let channel = HyperliquidWsChannel::Trades {
            coin: "BTC".to_string(),
        };
        let sub = channel_to_subscription(&channel);
        assert_eq!(sub["type"], "trades");
        assert_eq!(sub["coin"], "BTC");
    }

    #[test]
    fn test_channel_name() {
        let channel = HyperliquidWsChannel::L2Book {
            coin: "ETH".to_string(),
        };
        assert_eq!(channel.channel_name(), "l2Book@ETH");
    }
}
