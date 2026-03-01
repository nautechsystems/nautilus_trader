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

//! Kalshi WebSocket client for real-time market data.
//!
//! Requires authentication for all channels (orderbook, trades).
//! Authentication headers are sent during the HTTP upgrade handshake.

use std::sync::{
    Arc,
    atomic::{AtomicU32, Ordering},
};

use log::{error, info, warn};
use tokio::sync::Mutex;

use crate::{
    common::credential::{HEADER_ACCESS_KEY, HEADER_SIGNATURE, HEADER_TIMESTAMP, KalshiCredential},
    websocket::{
        error::KalshiWsError,
        handler::KalshiWsHandler,
        messages::{KalshiSubscribeCmd, KalshiWsMessage},
    },
};

/// WebSocket client for Kalshi real-time market data.
///
/// Maintains a single authenticated connection and handles multiple subscriptions.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KalshiWebSocketClient {
    ws_url: String,
    credential: Arc<KalshiCredential>,
    handler: Arc<Mutex<KalshiWsHandler>>,
    next_cmd_id: AtomicU32,
}

impl KalshiWebSocketClient {
    /// Create a new WebSocket client.
    ///
    /// `credential` is required — all Kalshi WebSocket channels require authentication.
    #[must_use]
    pub fn new(ws_url: String, credential: Arc<KalshiCredential>) -> Self {
        Self {
            ws_url,
            credential,
            handler: Arc::new(Mutex::new(KalshiWsHandler::default())),
            next_cmd_id: AtomicU32::new(1),
        }
    }

    fn next_id(&self) -> u32 {
        self.next_cmd_id.fetch_add(1, Ordering::Relaxed)
    }

    /// Returns the WebSocket URL this client connects to.
    #[must_use]
    pub fn ws_url(&self) -> &str {
        &self.ws_url
    }

    /// Build the authentication headers for the WebSocket upgrade request.
    ///
    /// Signs `GET /trade-api/ws/v2` — the path is always fixed for the WS endpoint.
    pub fn auth_headers(&self) -> Vec<(String, String)> {
        let ws_path = "/trade-api/ws/v2";
        let (ts, sig) = self.credential.sign("GET", ws_path);
        vec![
            (
                HEADER_ACCESS_KEY.to_string(),
                self.credential.api_key_id().to_string(),
            ),
            (HEADER_TIMESTAMP.to_string(), ts),
            (HEADER_SIGNATURE.to_string(), sig),
        ]
    }

    /// Subscribe to real-time orderbook deltas for the given market tickers.
    ///
    /// The first message received for each market will be an `orderbook_snapshot`,
    /// followed by incremental `orderbook_delta` messages.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established or the
    /// subscription command cannot be sent.
    pub async fn subscribe_orderbook(
        &self,
        market_tickers: Vec<String>,
    ) -> Result<(), KalshiWsError> {
        info!("Kalshi WS: subscribing orderbook for {market_tickers:?}");
        let cmd = KalshiSubscribeCmd::orderbook(self.next_id(), market_tickers);
        let cmd_json =
            serde_json::to_string(&cmd).map_err(|e| KalshiWsError::Connection(e.to_string()))?;
        self.send_command(cmd_json).await
    }

    /// Subscribe to real-time public trade events for the given market tickers.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established or the
    /// subscription command cannot be sent.
    pub async fn subscribe_trades(
        &self,
        market_tickers: Vec<String>,
    ) -> Result<(), KalshiWsError> {
        info!("Kalshi WS: subscribing trades for {market_tickers:?}");
        let cmd = KalshiSubscribeCmd::trades(self.next_id(), market_tickers);
        let cmd_json =
            serde_json::to_string(&cmd).map_err(|e| KalshiWsError::Connection(e.to_string()))?;
        self.send_command(cmd_json).await
    }

    /// Internal: send a serialized command JSON over the WebSocket.
    ///
    /// NOTE: The actual WebSocket connection management (connect, reconnect,
    /// message receive loop) should follow the pattern in
    /// `crates/adapters/polymarket/src/websocket/client.rs`, using
    /// `nautilus-network`'s `WebSocketClient`. This stub documents the
    /// interface; the full implementation must adapt to the `nautilus-network` API.
    async fn send_command(&self, _cmd_json: String) -> Result<(), KalshiWsError> {
        // TODO: obtain or reuse active WebSocket connection and send the command.
        // Reference: PolymarketWebSocketClient::subscribe_market / subscribe_user
        // in crates/adapters/polymarket/src/websocket/client.rs
        Ok(())
    }

    /// Process a raw WebSocket text message.
    ///
    /// On sequence gap, logs a warning and returns the gap error so the
    /// caller can re-subscribe.
    ///
    /// # Errors
    ///
    /// Returns `KalshiWsError::SequenceGap` if a sequence gap is detected,
    /// or a parse error if the message could not be deserialized.
    pub async fn handle_message(&self, raw: &str) -> Result<KalshiWsMessage, KalshiWsError> {
        let mut handler = self.handler.lock().await;
        match handler.handle(raw) {
            Ok(msg) => Ok(msg),
            Err(KalshiWsError::SequenceGap { sid, expected, got }) => {
                warn!("Kalshi WS: sequence gap sid={sid} (expected {expected}, got {got}) — re-subscribe needed");
                Err(KalshiWsError::SequenceGap { sid, expected, got })
            }
            Err(e) => {
                error!("Kalshi WS: message error: {e}");
                Err(e)
            }
        }
    }
}
