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

//! WebSocket client for Gate.io real-time data.

use futures_util::{SinkExt, StreamExt};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::sync::RwLock;
use tokio_tungstenite::{connect_async, tungstenite::Message};
use tracing::{debug, error, info};

use crate::common::{
    credential::GateioCredentials,
    enums::{GateioMarketType, GateioWsChannel},
    urls::GateioUrls,
};

use super::{
    error::{GateioWsError, GateioWsResult},
    messages::{WsAuth, WsRequest},
    subscription::SubscriptionManager,
};

/// Inner state for Gate.io WebSocket client.
struct GateioWebSocketClientInner {
    urls: GateioUrls,
    credentials: Option<GateioCredentials>,
    subscriptions: RwLock<SubscriptionManager>,
}

/// WebSocket client for Gate.io real-time market data and private updates.
#[derive(Clone)]
pub struct GateioWebSocketClient {
    inner: Arc<GateioWebSocketClientInner>,
}

impl std::fmt::Debug for GateioWebSocketClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GateioWebSocketClient").finish()
    }
}

impl GateioWebSocketClient {
    /// Creates a new Gate.io WebSocket client.
    ///
    /// # Arguments
    ///
    /// * `base_http_url` - Optional base HTTP URL
    /// * `base_ws_spot_url` - Optional base WebSocket spot URL
    /// * `base_ws_futures_url` - Optional base WebSocket futures URL
    /// * `base_ws_options_url` - Optional base WebSocket options URL
    /// * `credentials` - Optional credentials for authenticated channels
    #[must_use]
    pub fn new(
        base_http_url: Option<String>,
        base_ws_spot_url: Option<String>,
        base_ws_futures_url: Option<String>,
        base_ws_options_url: Option<String>,
        credentials: Option<GateioCredentials>,
    ) -> Self {
        let urls = GateioUrls::new(
            base_http_url,
            base_ws_spot_url,
            base_ws_futures_url,
            base_ws_options_url,
        );

        Self {
            inner: Arc::new(GateioWebSocketClientInner {
                urls,
                credentials,
                subscriptions: RwLock::new(SubscriptionManager::new()),
            }),
        }
    }

    /// Subscribes to a WebSocket channel.
    pub async fn subscribe(&self, channel: GateioWsChannel) -> GateioWsResult<()> {
        let mut subscriptions = self.inner.subscriptions.write().await;

        if subscriptions.is_subscribed(&channel) {
            return Ok(());
        }

        subscriptions.subscribe(&channel);
        debug!("Subscribed to channel: {}", channel);
        Ok(())
    }

    /// Unsubscribes from a WebSocket channel.
    pub async fn unsubscribe(&self, channel: &GateioWsChannel) -> GateioWsResult<()> {
        let mut subscriptions = self.inner.subscriptions.write().await;

        if !subscriptions.is_subscribed(channel) {
            return Ok(());
        }

        subscriptions.unsubscribe(channel);
        debug!("Unsubscribed from channel: {}", channel);
        Ok(())
    }

    /// Checks if subscribed to a channel.
    pub async fn is_subscribed(&self, channel: &GateioWsChannel) -> bool {
        let subscriptions = self.inner.subscriptions.read().await;
        subscriptions.is_subscribed(channel)
    }

    /// Returns the number of active subscriptions.
    pub async fn subscription_count(&self) -> usize {
        let subscriptions = self.inner.subscriptions.read().await;
        subscriptions.count()
    }

    /// Returns all active subscriptions.
    pub async fn subscriptions(&self) -> Vec<String> {
        let subscriptions = self.inner.subscriptions.read().await;
        subscriptions.subscriptions()
    }

    /// Creates a WebSocket subscription request.
    fn create_subscription_request(
        &self,
        channel: &str,
        event: &str,
        payload: Option<Vec<String>>,
        requires_auth: bool,
    ) -> WsRequest {
        let time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Time went backwards")
            .as_secs() as i64;

        let auth = if requires_auth {
            self.inner.credentials.as_ref().map(|creds| {
                let (signature, timestamp) = creds.sign_ws_request(channel, event);
                WsAuth {
                    method: "api_key".to_string(),
                    sign: signature,
                    timestamp: timestamp.to_string(),
                    key: creds.api_key().to_string(),
                }
            })
        } else {
            None
        };

        WsRequest {
            time,
            channel: channel.to_string(),
            event: event.to_string(),
            payload,
            auth,
        }
    }

    /// Determines if a channel requires authentication.
    #[must_use]
    fn requires_auth(channel: &GateioWsChannel) -> bool {
        matches!(
            channel,
            GateioWsChannel::SpotUserTrades { .. }
                | GateioWsChannel::SpotUserOrders { .. }
                | GateioWsChannel::FuturesUserTrades { .. }
                | GateioWsChannel::FuturesUserOrders { .. }
                | GateioWsChannel::FuturesPositions { .. }
        )
    }

    /// Gets the WebSocket URL for a given channel.
    #[must_use]
    fn get_ws_url(&self, channel: &GateioWsChannel) -> String {
        match channel {
            GateioWsChannel::SpotTicker { .. }
            | GateioWsChannel::SpotOrderBook { .. }
            | GateioWsChannel::SpotTrades { .. }
            | GateioWsChannel::SpotUserTrades { .. }
            | GateioWsChannel::SpotUserOrders { .. } => self.inner.urls.base_ws_spot().to_string(),
            GateioWsChannel::FuturesTicker { .. }
            | GateioWsChannel::FuturesOrderBook { .. }
            | GateioWsChannel::FuturesTrades { .. }
            | GateioWsChannel::FuturesUserTrades { .. }
            | GateioWsChannel::FuturesUserOrders { .. }
            | GateioWsChannel::FuturesPositions { .. } => {
                self.inner.urls.base_ws_futures().to_string()
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = GateioWebSocketClient::new(None, None, None, None, None);
        let debug_str = format!("{:?}", client);
        assert!(debug_str.contains("GateioWebSocketClient"));
    }

    #[tokio::test]
    async fn test_subscription_management() {
        let client = GateioWebSocketClient::new(None, None, None, None, None);
        let channel = GateioWsChannel::SpotTicker {
            currency_pair: "BTC_USDT".to_string(),
        };

        assert_eq!(client.subscription_count().await, 0);
        assert!(!client.is_subscribed(&channel).await);

        client.subscribe(channel.clone()).await.unwrap();
        assert_eq!(client.subscription_count().await, 1);
        assert!(client.is_subscribed(&channel).await);

        client.unsubscribe(&channel).await.unwrap();
        assert_eq!(client.subscription_count().await, 0);
        assert!(!client.is_subscribed(&channel).await);
    }

    #[test]
    fn test_requires_auth() {
        let public_channel = GateioWsChannel::SpotTicker {
            currency_pair: "BTC_USDT".to_string(),
        };
        assert!(!GateioWebSocketClient::requires_auth(&public_channel));

        let private_channel = GateioWsChannel::SpotUserOrders {
            currency_pair: None,
        };
        assert!(GateioWebSocketClient::requires_auth(&private_channel));
    }
}
