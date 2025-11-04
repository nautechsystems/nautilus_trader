use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use futures_util::{SinkExt, StreamExt};
use tokio::net::TcpStream;
use tokio::sync::RwLock;
use tokio_tungstenite::{
    connect_async, tungstenite::protocol::Message, MaybeTlsStream, WebSocketStream,
};
use tracing::{debug, info};

use crate::common::enums::AsterdexWsChannel;
use crate::common::AsterdexUrls;
use crate::websocket::error::AsterdexWebSocketError;
use crate::websocket::messages::{AsterdexWsSubscribe, AsterdexWsUnsubscribe};
use crate::websocket::subscription::SubscriptionManager;

pub struct AsterdexWebSocketClientInner {
    pub urls: AsterdexUrls,
    pub subscription_manager: SubscriptionManager,
    pub message_id: AtomicU64,
    pub ws_stream: RwLock<Option<WebSocketStream<MaybeTlsStream<TcpStream>>>>,
}

#[derive(Clone)]
pub struct AsterdexWebSocketClient {
    inner: Arc<AsterdexWebSocketClientInner>,
}

impl AsterdexWebSocketClient {
    pub fn new(
        base_url_ws_spot: Option<String>,
        base_url_ws_futures: Option<String>,
    ) -> Self {
        let urls = AsterdexUrls::new(None, None, base_url_ws_spot, base_url_ws_futures);

        Self {
            inner: Arc::new(AsterdexWebSocketClientInner {
                urls,
                subscription_manager: SubscriptionManager::new(),
                message_id: AtomicU64::new(1),
                ws_stream: RwLock::new(None),
            }),
        }
    }

    fn next_message_id(&self) -> u64 {
        self.inner.message_id.fetch_add(1, Ordering::Relaxed)
    }

    pub async fn connect(&self, is_spot: bool) -> Result<(), AsterdexWebSocketError> {
        let url = if is_spot {
            format!("{}/ws", self.inner.urls.base_ws_spot())
        } else {
            format!("{}/ws", self.inner.urls.base_ws_futures())
        };

        debug!("Connecting to WebSocket: {}", url);

        let (ws_stream, _) = connect_async(&url)
            .await
            .map_err(|e| AsterdexWebSocketError::Connection(e.to_string()))?;

        info!("WebSocket connected: {}", url);

        let mut stream_lock = self.inner.ws_stream.write().await;
        *stream_lock = Some(ws_stream);

        Ok(())
    }

    pub async fn disconnect(&self) -> Result<(), AsterdexWebSocketError> {
        let mut stream_lock = self.inner.ws_stream.write().await;
        if let Some(mut ws) = stream_lock.take() {
            ws.close(None)
                .await
                .map_err(|e| AsterdexWebSocketError::Connection(e.to_string()))?;
            info!("WebSocket disconnected");
        }
        Ok(())
    }

    pub async fn subscribe(&self, channel: AsterdexWsChannel) -> Result<(), AsterdexWebSocketError> {
        let stream_name = channel.to_stream_name();
        let id = self.next_message_id();

        let subscribe_msg = AsterdexWsSubscribe::new(vec![stream_name.clone()], id);
        let msg_json = serde_json::to_string(&subscribe_msg)
            .map_err(|e| AsterdexWebSocketError::Subscription(e.to_string()))?;

        self.inner
            .subscription_manager
            .add_pending(channel.clone())
            .await;

        let mut stream_lock = self.inner.ws_stream.write().await;
        if let Some(ws) = stream_lock.as_mut() {
            ws.send(Message::Text(msg_json.into()))
                .await
                .map_err(|e| AsterdexWebSocketError::Subscription(e.to_string()))?;

            debug!("Subscribed to channel: {}", stream_name);

            // Mark as subscribed (in production, wait for confirmation)
            drop(stream_lock);
            self.inner.subscription_manager.mark_subscribed(&channel).await;

            Ok(())
        } else {
            Err(AsterdexWebSocketError::Connection(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    pub async fn unsubscribe(&self, channel: AsterdexWsChannel) -> Result<(), AsterdexWebSocketError> {
        let stream_name = channel.to_stream_name();
        let id = self.next_message_id();

        let unsubscribe_msg = AsterdexWsUnsubscribe::new(vec![stream_name.clone()], id);
        let msg_json = serde_json::to_string(&unsubscribe_msg)
            .map_err(|e| AsterdexWebSocketError::Subscription(e.to_string()))?;

        let mut stream_lock = self.inner.ws_stream.write().await;
        if let Some(ws) = stream_lock.as_mut() {
            ws.send(Message::Text(msg_json.into()))
                .await
                .map_err(|e| AsterdexWebSocketError::Subscription(e.to_string()))?;

            debug!("Unsubscribed from channel: {}", stream_name);

            drop(stream_lock);
            self.inner
                .subscription_manager
                .mark_unsubscribed(&channel)
                .await;

            Ok(())
        } else {
            Err(AsterdexWebSocketError::Connection(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    pub async fn receive(&self) -> Result<Option<String>, AsterdexWebSocketError> {
        let mut stream_lock = self.inner.ws_stream.write().await;
        if let Some(ws) = stream_lock.as_mut() {
            match ws.next().await {
                Some(Ok(msg)) => match msg {
                    Message::Text(text) => {
                        let text_str = text.to_string();
                        debug!("Received message: {}", text_str);
                        Ok(Some(text_str))
                    }
                    Message::Ping(data) => {
                        // Respond to ping with pong
                        ws.send(Message::Pong(data))
                            .await
                            .map_err(|e| AsterdexWebSocketError::Connection(e.to_string()))?;
                        Ok(None)
                    }
                    Message::Pong(_) => Ok(None),
                    Message::Close(_) => {
                        info!("WebSocket closed by server");
                        Ok(None)
                    }
                    _ => Ok(None),
                },
                Some(Err(e)) => Err(AsterdexWebSocketError::Connection(e.to_string())),
                None => Ok(None),
            }
        } else {
            Err(AsterdexWebSocketError::Connection(
                "WebSocket not connected".to_string(),
            ))
        }
    }

    pub async fn is_connected(&self) -> bool {
        let stream_lock = self.inner.ws_stream.read().await;
        stream_lock.is_some()
    }

    pub async fn get_subscriptions(&self) -> Vec<AsterdexWsChannel> {
        self.inner.subscription_manager.get_all().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_websocket_client_creation() {
        let client = AsterdexWebSocketClient::new(None, None);
        assert!(!client.is_connected().await);
    }

    #[test]
    fn test_message_id_increment() {
        let client = AsterdexWebSocketClient::new(None, None);
        let id1 = client.next_message_id();
        let id2 = client.next_message_id();
        assert_eq!(id2, id1 + 1);
    }
}
