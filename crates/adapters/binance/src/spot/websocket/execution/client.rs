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

//! Binance Spot User Data Stream WebSocket client.
//!
//! Manages the listen key lifecycle and WebSocket connection for the Spot
//! user data stream. Handles keepalive pings and reconnection on listen key
//! expiry.

use std::sync::{Arc, Mutex};

use futures_util::StreamExt;
use nautilus_common::live::get_runtime;
use nautilus_core::MUTEX_POISONED;
use tokio::task::JoinHandle;

use super::{handler::parse_raw_message, messages::BinanceSpotUdsMessage};
use crate::spot::http::client::BinanceSpotHttpClient;

/// Listen key keepalive interval (30 minutes).
const LISTEN_KEY_KEEPALIVE_SECS: u64 = 30 * 60;

/// Binance Spot User Data Stream WebSocket client.
///
/// Manages the full lifecycle of a user data stream connection:
/// 1. Creates a listen key via HTTP API
/// 2. Connects a WebSocket to `{base_ws_url}/{listen_key}`
/// 3. Periodically extends the listen key (every 30 minutes)
/// 4. Parses raw frames into typed venue messages
/// 5. Handles reconnection on listen key expiry
#[derive(Debug)]
pub struct BinanceSpotUdsClient {
    base_ws_url: String,
    http_client: BinanceSpotHttpClient,
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<BinanceSpotUdsMessage>>,
    keepalive_handle: Arc<Mutex<Option<JoinHandle<()>>>>,
    ws_read_handle: Mutex<Option<JoinHandle<()>>>,
}

impl BinanceSpotUdsClient {
    /// Creates a new [`BinanceSpotUdsClient`].
    pub fn new(base_ws_url: String, http_client: BinanceSpotHttpClient) -> Self {
        Self {
            base_ws_url,
            http_client,
            out_rx: None,
            keepalive_handle: Arc::new(Mutex::new(None)),
            ws_read_handle: Mutex::new(None),
        }
    }

    /// Connects to the user data stream.
    ///
    /// Creates a listen key, opens a WebSocket connection, and starts the
    /// keepalive task.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    ///
    /// # Errors
    ///
    /// Returns an error if the listen key cannot be created or the WebSocket
    /// connection fails.
    pub async fn connect(&mut self) -> anyhow::Result<()> {
        let listen_key_response = self
            .http_client
            .inner()
            .create_listen_key()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to create listen key: {e}"))?;

        let listen_key = listen_key_response.listen_key;
        let ws_url = format!("{}/{listen_key}", self.base_ws_url);

        log::info!("Connecting to Spot user data stream: {}", self.base_ws_url);

        let (ws_stream, _response) = tokio_tungstenite::connect_async(&ws_url)
            .await
            .map_err(|e| anyhow::anyhow!("WebSocket connection failed: {e}"))?;

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();

        let keepalive_handle = Arc::clone(&self.keepalive_handle);
        let initial_keepalive = spawn_keepalive(self.http_client.clone(), listen_key);
        *keepalive_handle.lock().expect(MUTEX_POISONED) = Some(initial_keepalive);

        let (_, read_stream) = ws_stream.split();
        let reconnect_http = self.http_client.clone();
        let reconnect_base_url = self.base_ws_url.clone();
        let ws_read_handle = get_runtime().spawn(async move {
            let mut read_stream = read_stream;

            loop {
                while let Some(result) = read_stream.next().await {
                    match result {
                        Ok(msg) => {
                            if let Some(parsed) = parse_raw_message(msg)
                                && out_tx.send(parsed).is_err()
                            {
                                return;
                            }
                        }
                        Err(e) => {
                            log::error!("User data stream read error: {e}");
                            break;
                        }
                    }
                }

                log::warn!("User data stream disconnected, reconnecting");

                let mut delay = std::time::Duration::from_secs(1);
                let max_delay = std::time::Duration::from_secs(30);

                loop {
                    tokio::time::sleep(delay).await;

                    let new_key = match reconnect_http.inner().create_listen_key().await {
                        Ok(resp) => resp.listen_key,
                        Err(e) => {
                            log::error!("Failed to create listen key for reconnection: {e}");
                            delay = (delay * 2).min(max_delay);
                            continue;
                        }
                    };

                    let ws_url = format!("{reconnect_base_url}/{new_key}");

                    match tokio_tungstenite::connect_async(&ws_url).await {
                        Ok((ws_stream, _)) => {
                            let (_, new_read) = ws_stream.split();
                            read_stream = new_read;

                            if let Some(old) = keepalive_handle.lock().expect(MUTEX_POISONED).take()
                            {
                                old.abort();
                            }
                            let new_keepalive = spawn_keepalive(reconnect_http.clone(), new_key);
                            *keepalive_handle.lock().expect(MUTEX_POISONED) = Some(new_keepalive);

                            if out_tx.send(BinanceSpotUdsMessage::Reconnected).is_err() {
                                return;
                            }

                            log::info!("User data stream reconnected");
                            break;
                        }
                        Err(e) => {
                            log::error!("WebSocket reconnection failed: {e}");
                            delay = (delay * 2).min(max_delay);
                        }
                    }
                }
            }
        });

        self.out_rx = Some(out_rx);
        *self.ws_read_handle.lock().expect(MUTEX_POISONED) = Some(ws_read_handle);

        log::info!("Connected to Spot user data stream");
        Ok(())
    }

    /// Disconnects from the user data stream.
    ///
    /// Cancels the keepalive task, closes the WebSocket, and closes the listen key.
    ///
    /// # Panics
    ///
    /// Panics if a mutex is poisoned.
    pub async fn disconnect(&mut self) {
        if let Some(handle) = self.keepalive_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_read_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.out_rx = None;

        log::info!("Disconnected from Spot user data stream");
    }

    /// Takes the message receiver for consuming UDS messages.
    ///
    /// Can only be called once after connect. Returns `None` if the receiver
    /// has already been taken or the client is not connected.
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<BinanceSpotUdsMessage>> {
        self.out_rx.take()
    }
}

fn spawn_keepalive(http_client: BinanceSpotHttpClient, listen_key: String) -> JoinHandle<()> {
    get_runtime().spawn(async move {
        let interval = std::time::Duration::from_secs(LISTEN_KEY_KEEPALIVE_SECS);

        loop {
            tokio::time::sleep(interval).await;

            match http_client.inner().extend_listen_key(&listen_key).await {
                Ok(()) => {
                    log::debug!("Listen key extended");
                }
                Err(e) => {
                    log::error!("Failed to extend listen key: {e}");
                    break;
                }
            }
        }
    })
}
