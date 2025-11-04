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

//! WebSocket client implementation for Lighter.

use std::sync::Arc;

use async_stream::stream;
use futures_util::{SinkExt, StreamExt};
use tokio::sync::RwLock;
use tokio_tungstenite::{
    connect_async,
    tungstenite::{Message as WsMessage, protocol::CloseFrame},
};
use tracing::{debug, error, info, trace, warn};

use crate::common::{
    credential::LighterCredentials,
    enums::LighterWsChannel,
    urls::LighterUrls,
};

use super::{
    error::{LighterWsError, LighterWsResult},
    messages::{WsMessage as LighterWsMessage, WsSubscribe, WsUnsubscribe},
    subscription::SubscriptionManager,
};

/// Inner WebSocket client implementation.
struct LighterWebSocketClientInner {
    /// URL manager.
    urls: LighterUrls,
    /// API credentials (optional).
    credentials: Option<LighterCredentials>,
    /// Subscription manager.
    subscriptions: Arc<RwLock<SubscriptionManager>>,
}

/// Lighter WebSocket client for real-time data streams.
#[derive(Clone)]
pub struct LighterWebSocketClient {
    inner: Arc<LighterWebSocketClientInner>,
}

impl LighterWebSocketClient {
    /// Creates a new Lighter WebSocket client.
    ///
    /// # Arguments
    ///
    /// * `base_http_url` - Base HTTP URL (None for default mainnet)
    /// * `base_ws_url` - Base WebSocket URL (None for default mainnet)
    /// * `is_testnet` - Whether to use testnet
    /// * `credentials` - API credentials (optional for public channels)
    #[must_use]
    pub fn new(
        base_http_url: Option<String>,
        base_ws_url: Option<String>,
        is_testnet: bool,
        credentials: Option<LighterCredentials>,
    ) -> Self {
        let urls = LighterUrls::new(base_http_url, base_ws_url, is_testnet);

        Self {
            inner: Arc::new(LighterWebSocketClientInner {
                urls,
                credentials,
                subscriptions: Arc::new(RwLock::new(SubscriptionManager::new())),
            }),
        }
    }

    /// Connects to the WebSocket and returns a stream of messages.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    pub async fn connect(&self) -> LighterWsResult<impl futures_util::Stream<Item = LighterWsResult<LighterWsMessage>>> {
        let url = self.inner.urls.base_ws();
        info!("Connecting to Lighter WebSocket: {}", url);

        let (ws_stream, _) = connect_async(url)
            .await
            .map_err(LighterWsError::Connection)?;

        let (mut write, mut read) = ws_stream.split();

        debug!("Connected to Lighter WebSocket");

        let stream = stream! {
            while let Some(msg) = read.next().await {
                match msg {
                    Ok(WsMessage::Text(text)) => {
                        trace!("Received WebSocket message: {}", text);
                        match serde_json::from_str::<LighterWsMessage>(&text) {
                            Ok(lighter_msg) => yield Ok(lighter_msg),
                            Err(e) => {
                                error!("Failed to parse WebSocket message: {}", e);
                                yield Err(LighterWsError::Json(e));
                            }
                        }
                    }
                    Ok(WsMessage::Ping(data)) => {
                        trace!("Received ping, sending pong");
                        if let Err(e) = write.send(WsMessage::Pong(data)).await {
                            error!("Failed to send pong: {}", e);
                            yield Err(LighterWsError::Connection(e));
                            break;
                        }
                    }
                    Ok(WsMessage::Pong(_)) => {
                        trace!("Received pong");
                    }
                    Ok(WsMessage::Close(frame)) => {
                        info!("WebSocket connection closed: {:?}", frame);
                        break;
                    }
                    Ok(_) => {
                        debug!("Received other WebSocket message type");
                    }
                    Err(e) => {
                        error!("WebSocket error: {}", e);
                        yield Err(LighterWsError::Connection(e));
                        break;
                    }
                }
            }
        };

        Ok(stream)
    }

    /// Subscribes to a channel.
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails.
    pub async fn subscribe(&self, channel: LighterWsChannel) -> LighterWsResult<()> {
        let mut subs = self.inner.subscriptions.write().await;
        subs.add(channel.clone());
        debug!("Subscribed to channel: {}", channel);
        Ok(())
    }

    /// Unsubscribes from a channel.
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails.
    pub async fn unsubscribe(&self, channel: &LighterWsChannel) -> LighterWsResult<()> {
        let mut subs = self.inner.subscriptions.write().await;
        subs.remove(channel);
        debug!("Unsubscribed from channel: {}", channel);
        Ok(())
    }

    /// Returns the number of active subscriptions.
    pub async fn subscription_count(&self) -> usize {
        let subs = self.inner.subscriptions.read().await;
        subs.count()
    }

    /// Checks if subscribed to a channel.
    pub async fn is_subscribed(&self, channel: &LighterWsChannel) -> bool {
        let subs = self.inner.subscriptions.read().await;
        subs.is_subscribed(channel)
    }
}

impl std::fmt::Debug for LighterWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LighterWebSocketClient")
            .field("urls", &self.inner.urls)
            .field("has_credentials", &self.inner.credentials.is_some())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = LighterWebSocketClient::new(None, None, false, None);
        assert!(format!("{:?}", client).contains("LighterWebSocketClient"));
    }

    #[tokio::test]
    async fn test_subscription_management() {
        let client = LighterWebSocketClient::new(None, None, false, None);
        let channel = LighterWsChannel::OrderBook { market_id: 0 };

        assert_eq!(client.subscription_count().await, 0);

        client.subscribe(channel.clone()).await.unwrap();
        assert_eq!(client.subscription_count().await, 1);
        assert!(client.is_subscribed(&channel).await);

        client.unsubscribe(&channel).await.unwrap();
        assert_eq!(client.subscription_count().await, 0);
        assert!(!client.is_subscribed(&channel).await);
    }
}
