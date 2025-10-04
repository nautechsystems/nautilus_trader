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

//! WebSocket client implementation for Coinbase Advanced Trade API.

use std::{
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use aws_lc_rs::signature::{EcdsaKeyPair, ECDSA_P256_SHA256_ASN1_SIGNING};
use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use futures::{SinkExt, StreamExt};
use serde_json::json;
use tokio::sync::Mutex;
use tokio_tungstenite::{connect_async, tungstenite::Message, MaybeTlsStream, WebSocketStream};
use tracing::{debug, error, info, warn};

use crate::{
    http::client::der_to_raw_signature,
    websocket::types::*,
};

type WsStream = WebSocketStream<MaybeTlsStream<tokio::net::TcpStream>>;

/// WebSocket URLs
pub const WS_URL_MARKET_DATA: &str = "wss://advanced-trade-ws.coinbase.com";
pub const WS_URL_USER_DATA: &str = "wss://advanced-trade-ws-user.coinbase.com";

/// WebSocket client for Coinbase Advanced Trade API
#[derive(Debug)]
pub struct CoinbaseWebSocketClient {
    ws_url: String,
    api_key: String,
    api_secret: String,
    stream: Arc<Mutex<Option<WsStream>>>,
}

impl CoinbaseWebSocketClient {
    /// Create a new WebSocket client for market data
    #[must_use]
    pub fn new_market_data(api_key: String, api_secret: String) -> Self {
        Self {
            ws_url: WS_URL_MARKET_DATA.to_string(),
            api_key,
            api_secret,
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new WebSocket client for user data
    #[must_use]
    pub fn new_user_data(api_key: String, api_secret: String) -> Self {
        Self {
            ws_url: WS_URL_USER_DATA.to_string(),
            api_key,
            api_secret,
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// Create a new WebSocket client with custom URL
    #[must_use]
    pub fn new_with_url(ws_url: String, api_key: String, api_secret: String) -> Self {
        Self {
            ws_url,
            api_key,
            api_secret,
            stream: Arc::new(Mutex::new(None)),
        }
    }

    /// Connect to WebSocket
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails
    pub async fn connect(&self) -> Result<()> {
        info!("Connecting to Coinbase WebSocket: {}", self.ws_url);

        let (ws_stream, response) = connect_async(&self.ws_url)
            .await
            .context("Failed to connect to WebSocket")?;

        debug!("WebSocket handshake response: {:?}", response);

        let mut stream = self.stream.lock().await;
        *stream = Some(ws_stream);

        info!("Connected to Coinbase WebSocket");
        Ok(())
    }

    /// Disconnect from WebSocket
    ///
    /// # Errors
    ///
    /// Returns an error if disconnection fails
    pub async fn disconnect(&self) -> Result<()> {
        let mut stream = self.stream.lock().await;
        if let Some(mut ws) = stream.take() {
            ws.close(None)
                .await
                .context("Failed to close WebSocket")?;
            info!("Disconnected from Coinbase WebSocket");
        }
        Ok(())
    }

    /// Generate JWT for WebSocket authentication
    ///
    /// WebSocket authentication uses the same JWT format as HTTP API
    fn generate_jwt(&self) -> Result<String> {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .context("Failed to get system time")?
            .as_secs();

        // Generate a random nonce (required by Coinbase)
        let mut nonce_bytes = [0u8; 32];
        rand::RngCore::fill_bytes(&mut rand::thread_rng(), &mut nonce_bytes);
        let nonce = nonce_bytes.iter()
            .map(|b| format!("{:02x}", b))
            .collect::<String>();

        // Create JWT header with nonce
        let header = json!({
            "alg": "ES256",
            "kid": self.api_key,
            "nonce": nonce,
            "typ": "JWT"
        });

        // Create JWT claims for WebSocket
        // Note: WebSocket JWT doesn't include the "uri" field
        let claims = json!({
            "sub": self.api_key,
            "iss": "cdp",
            "nbf": now,
            "exp": now + 120,
        });

        // Encode header and claims to base64url
        let header_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&header)?.as_bytes());
        let claims_b64 = URL_SAFE_NO_PAD.encode(serde_json::to_string(&claims)?.as_bytes());

        // Create the message to sign
        let message = format!("{}.{}", header_b64, claims_b64);

        // Parse the EC private key from PEM format
        let key_with_newlines = self.api_secret.replace("\\n", "\n");

        // Parse the EC private key - handle both SEC1 and PKCS#8 formats
        let pkcs8_der = if key_with_newlines.contains("BEGIN EC PRIVATE KEY") {
            // SEC1 format - need to convert to PKCS#8
            use pkcs8::{PrivateKeyInfo, AlgorithmIdentifierRef, ObjectIdentifier};
            use pkcs8::der::Encode;

            // Parse PEM to get SEC1 DER bytes
            let pem_data = pem::parse(key_with_newlines.as_bytes())
                .context("Failed to parse SEC1 PEM")?;

            // OID for EC public key (1.2.840.10045.2.1)
            const EC_PUBLIC_KEY_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.2.1");
            // OID for secp256r1/prime256v1 curve (1.2.840.10045.3.1.7)
            const SECP256R1_OID: ObjectIdentifier = ObjectIdentifier::new_unwrap("1.2.840.10045.3.1.7");

            // Create PKCS#8 PrivateKeyInfo structure
            let algorithm = AlgorithmIdentifierRef {
                oid: EC_PUBLIC_KEY_OID,
                parameters: Some((&SECP256R1_OID).into()),
            };

            let private_key_info = PrivateKeyInfo {
                algorithm,
                private_key: pem_data.contents(),
                public_key: None,
            };

            // Encode to DER
            private_key_info.to_der()
                .context("Failed to encode PKCS#8 DER")?
        } else {
            // Already in PKCS#8 format
            let pem_data = pem::parse(key_with_newlines.as_bytes())
                .context("Failed to parse PEM format")?;
            pem_data.contents().to_vec()
        };

        // Create ECDSA key pair from PKCS#8 DER
        let key_pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &pkcs8_der)
            .context("Failed to create ECDSA key pair from PKCS#8")?;

        // Sign the message
        let rng = aws_lc_rs::rand::SystemRandom::new();
        let signature = key_pair.sign(&rng, message.as_bytes())
            .context("Failed to sign JWT")?;

        // Convert DER signature to raw R||S format (64 bytes for P-256)
        let signature_bytes = signature.as_ref();
        let raw_signature = der_to_raw_signature(signature_bytes)
            .context("Failed to convert DER signature to raw format")?;

        // Encode signature to base64url
        let signature_b64 = URL_SAFE_NO_PAD.encode(&raw_signature);

        // Combine to create final JWT
        let jwt = format!("{}.{}", message, signature_b64);

        debug!("Successfully generated WebSocket JWT (length: {} bytes)", jwt.len());

        Ok(jwt)
    }

    /// Subscribe to a channel
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails
    pub async fn subscribe(
        &self,
        product_ids: Vec<String>,
        channel: Channel,
    ) -> Result<()> {
        let request = if channel == Channel::Heartbeats {
            // Heartbeats channel doesn't need product_ids
            SubscribeRequest::new_heartbeats()
        } else {
            SubscribeRequest::new(product_ids.clone(), channel.clone())
        };

        // Add JWT if channel requires authentication or if we have credentials
        let request = if channel.requires_auth() || !self.api_key.is_empty() {
            let jwt = self.generate_jwt()?;
            request.with_jwt(jwt)
        } else {
            request
        };

        let message = serde_json::to_string(&request).context("Failed to serialize subscribe request")?;
        self.send_message(&message).await?;

        info!("Subscribed to {:?} for products: {:?}", channel, product_ids);
        Ok(())
    }

    /// Subscribe to heartbeats channel (no product_ids needed)
    ///
    /// # Errors
    ///
    /// Returns an error if subscription fails
    pub async fn subscribe_heartbeats(&self) -> Result<()> {
        let request = SubscribeRequest::new_heartbeats();

        // Add JWT if we have credentials (recommended for reliability)
        let request = if !self.api_key.is_empty() {
            let jwt = self.generate_jwt()?;
            request.with_jwt(jwt)
        } else {
            request
        };

        let message = serde_json::to_string(&request).context("Failed to serialize subscribe request")?;
        self.send_message(&message).await?;

        info!("Subscribed to heartbeats channel");
        Ok(())
    }

    /// Unsubscribe from a channel
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails
    pub async fn unsubscribe(&self, product_ids: Vec<String>, channel: Channel) -> Result<()> {
        let request = if channel == Channel::Heartbeats {
            UnsubscribeRequest::new_heartbeats()
        } else {
            UnsubscribeRequest::new(product_ids.clone(), channel.clone())
        };

        let message = serde_json::to_string(&request).context("Failed to serialize unsubscribe request")?;
        self.send_message(&message).await?;

        info!("Unsubscribed from {:?} for products: {:?}", channel, product_ids);
        Ok(())
    }

    /// Unsubscribe from heartbeats channel
    ///
    /// # Errors
    ///
    /// Returns an error if unsubscription fails
    pub async fn unsubscribe_heartbeats(&self) -> Result<()> {
        let request = UnsubscribeRequest::new_heartbeats();
        let message = serde_json::to_string(&request).context("Failed to serialize unsubscribe request")?;
        self.send_message(&message).await?;

        info!("Unsubscribed from heartbeats channel");
        Ok(())
    }

    /// Send a message to the WebSocket
    async fn send_message(&self, message: &str) -> Result<()> {
        let mut stream = self.stream.lock().await;
        if let Some(ws) = stream.as_mut() {
            ws.send(Message::Text(message.to_string().into()))
                .await
                .context("Failed to send message")?;
            debug!("Sent message: {}", message);
            Ok(())
        } else {
            anyhow::bail!("WebSocket not connected")
        }
    }

    /// Receive next message from WebSocket
    ///
    /// # Errors
    ///
    /// Returns an error if receiving message fails
    pub async fn receive_message(&self) -> Result<Option<String>> {
        let mut stream = self.stream.lock().await;
        if let Some(ws) = stream.as_mut() {
            match ws.next().await {
                Some(Ok(Message::Text(text))) => {
                    debug!("Received message: {}", text);
                    Ok(Some(text.to_string()))
                }
                Some(Ok(Message::Close(frame))) => {
                    warn!("WebSocket closed: {:?}", frame);
                    Ok(None)
                }
                Some(Ok(Message::Ping(data))) => {
                    debug!("Received ping, sending pong");
                    ws.send(Message::Pong(data))
                        .await
                        .context("Failed to send pong")?;
                    Ok(None)
                }
                Some(Ok(msg)) => {
                    debug!("Received non-text message: {:?}", msg);
                    Ok(None)
                }
                Some(Err(e)) => {
                    error!("WebSocket error: {}", e);
                    Err(e.into())
                }
                None => {
                    warn!("WebSocket stream ended");
                    Ok(None)
                }
            }
        } else {
            anyhow::bail!("WebSocket not connected")
        }
    }

    /// Check if connected
    #[must_use]
    pub async fn is_connected(&self) -> bool {
        let stream = self.stream.lock().await;
        stream.is_some()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let _client = CoinbaseWebSocketClient::new_market_data(
            "test_key".to_string(),
            "test_secret".to_string(),
        );
    }

    #[test]
    fn test_subscribe_request() {
        let request = SubscribeRequest::new(
            vec!["BTC-USD".to_string()],
            Channel::Ticker,
        );
        assert_eq!(request.msg_type, "subscribe");
        assert_eq!(request.channel, "ticker");
    }

    #[test]
    fn test_channel_requires_auth() {
        assert!(!Channel::Heartbeats.requires_auth());
        assert!(!Channel::Ticker.requires_auth());
        assert!(!Channel::Level2.requires_auth());
        assert!(Channel::User.requires_auth());
        assert!(Channel::FuturesBalanceSummary.requires_auth());
    }

    #[test]
    fn test_channel_as_str() {
        assert_eq!(Channel::Heartbeats.as_str(), "heartbeats");
        assert_eq!(Channel::Candles.as_str(), "candles");
        assert_eq!(Channel::MarketTrades.as_str(), "market_trades");
        assert_eq!(Channel::Ticker.as_str(), "ticker");
        assert_eq!(Channel::TickerBatch.as_str(), "ticker_batch");
        assert_eq!(Channel::Level2.as_str(), "level2");
        assert_eq!(Channel::User.as_str(), "user");
    }
}

