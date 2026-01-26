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

//! Binance Spot WebSocket API client for SBE trading.
//!
//! ## Connection Details
//!
//! - Endpoint: `ws-api.binance.com:443/ws-api/v3`
//! - Authentication: Ed25519 signature per request
//! - SBE responses: Enabled via `responseFormat=sbe` query parameter
//! - Connection validity: 24 hours
//! - Ping/pong: Every 20 seconds

use std::{
    fmt::Debug,
    num::NonZeroU32,
    sync::{
        Arc, LazyLock, Mutex,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{PingHandler, WebSocketClient, WebSocketConfig, channel_message_handler},
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::{BinanceWsApiError, BinanceWsApiResult},
    handler::BinanceSpotWsApiHandler,
    messages::{HandlerCommand, NautilusWsApiMessage},
};
use crate::{
    common::{consts::BINANCE_SPOT_SBE_WS_API_URL, credential::Credential},
    spot::http::query::{CancelOrderParams, CancelReplaceOrderParams, NewOrderParams},
};

/// Environment variable key for Binance API key.
pub const BINANCE_API_KEY: &str = "BINANCE_API_KEY";

/// Environment variable key for Binance API secret.
pub const BINANCE_API_SECRET: &str = "BINANCE_API_SECRET";

/// Pre-interned rate limit key for order operations (place/cancel/replace).
///
/// Binance WebSocket API: 1200 requests per minute per IP (20/sec).
pub static BINANCE_WS_RATE_LIMIT_KEY_ORDER: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("order")]);

/// Binance WebSocket API order rate limit: 1200 per minute (20/sec).
///
/// Based on Binance documentation for WebSocket API rate limits.
///
/// # Panics
///
/// This function will never panic as it uses a constant non-zero value.
#[must_use]
pub fn binance_ws_order_quota() -> Quota {
    Quota::per_second(NonZeroU32::new(20).expect("20 > 0"))
}

/// Binance Spot WebSocket API client for SBE trading.
///
/// This client provides order management via WebSocket with SBE-encoded responses,
/// complementing the HTTP client with lower-latency order submission.
#[derive(Clone)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.binance")
)]
pub struct BinanceSpotWsTradingClient {
    url: String,
    credential: Arc<Credential>,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsApiMessage>>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    request_id_counter: Arc<AtomicU64>,
    cancellation_token: CancellationToken,
}

impl Debug for BinanceSpotWsTradingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceSpotWsTradingClient))
            .field("url", &self.url)
            .field("credential", &"<redacted>")
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl BinanceSpotWsTradingClient {
    /// Creates a new [`BinanceSpotWsTradingClient`] instance.
    #[must_use]
    pub fn new(
        url: Option<String>,
        api_key: String,
        api_secret: String,
        heartbeat: Option<u64>,
    ) -> Self {
        let url = url.unwrap_or_else(|| BINANCE_SPOT_SBE_WS_API_URL.to_string());
        let credential = Arc::new(Credential::new(api_key, api_secret));

        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();

        Self {
            url,
            credential,
            heartbeat,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode: Arc::new(ArcSwap::new(Arc::new(AtomicU8::new(
                ConnectionMode::Closed as u8,
            )))),
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: Arc::new(Mutex::new(None)),
            task_handle: None,
            request_id_counter: Arc::new(AtomicU64::new(1)),
            cancellation_token: CancellationToken::new(),
        }
    }

    /// Creates a new client with credentials sourced from environment variables.
    ///
    /// Falls back to env vars if `api_key` or `api_secret` are `None`:
    /// - `BINANCE_API_KEY` for the API key
    /// - `BINANCE_API_SECRET` for the API secret
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are missing from environment.
    pub fn with_env(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let api_key = nautilus_core::env::get_or_env_var(api_key, BINANCE_API_KEY)?;
        let api_secret = nautilus_core::env::get_or_env_var(api_secret, BINANCE_API_SECRET)?;
        Ok(Self::new(url, api_key, api_secret, heartbeat))
    }

    /// Creates a new client with credentials loaded entirely from environment variables.
    ///
    /// Reads:
    /// - `BINANCE_API_KEY` for the API key
    /// - `BINANCE_API_SECRET` for the API secret
    ///
    /// # Errors
    ///
    /// Returns an error if environment variables are missing.
    pub fn from_env(url: Option<String>, heartbeat: Option<u64>) -> anyhow::Result<Self> {
        Self::with_env(url, None, None, heartbeat)
    }

    /// Returns whether the client is actively connected.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let mode_u8 = self.connection_mode.load().load(Ordering::Relaxed);
        mode_u8 == ConnectionMode::Active as u8
    }

    /// Returns whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let mode_u8 = self.connection_mode.load().load(Ordering::Relaxed);
        mode_u8 == ConnectionMode::Closed as u8
    }

    /// Generates the next request ID.
    fn next_request_id(&self) -> String {
        let id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        format!("req-{id}")
    }

    /// Connects to the WebSocket API server.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub async fn connect(&mut self) -> BinanceWsApiResult<()> {
        self.signal.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();

        let (raw_handler, raw_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        let headers = vec![(
            "X-MBX-APIKEY".to_string(),
            self.credential.api_key().to_string(),
        )];

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers,
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
        };

        // Configure rate limits for order operations
        let keyed_quotas = vec![(
            BINANCE_WS_RATE_LIMIT_KEY_ORDER[0].as_str().to_string(),
            binance_ws_order_quota(),
        )];

        let client = WebSocketClient::connect(
            config,
            Some(raw_handler),
            Some(ping_handler),
            None,
            keyed_quotas,
            Some(binance_ws_order_quota()), // Default quota for all operations
        )
        .await
        .map_err(|e| BinanceWsApiError::ConnectionError(e.to_string()))?;

        self.connection_mode.store(client.connection_mode_atomic());

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();

        {
            let mut rx_guard = self.out_rx.lock().expect("Mutex poisoned");
            *rx_guard = Some(out_rx);
        }

        {
            let mut tx_guard = self.cmd_tx.write().await;
            *tx_guard = cmd_tx;
        }

        let signal = self.signal.clone();
        let credential = self.credential.clone();
        let mut handler = BinanceSpotWsApiHandler::new(signal, cmd_rx, raw_rx, out_tx, credential);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SetClient(client))
            .map_err(|e| BinanceWsApiError::HandlerUnavailable(e.to_string()))?;

        let cancellation_token = self.cancellation_token.clone();
        let handle = get_runtime().spawn(async move {
            tokio::select! {
                () = cancellation_token.cancelled() => {
                    log::debug!("Handler task cancelled");
                }
                _ = handler.run() => {
                    log::debug!("Handler run completed");
                }
            }
        });

        self.task_handle = Some(Arc::new(handle));

        Ok(())
    }

    /// Disconnects from the WebSocket API server.
    pub async fn disconnect(&mut self) {
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::warn!("Failed to send disconnect command: {e}");
        }

        self.cancellation_token.cancel();

        if let Some(handle) = self.task_handle.take()
            && let Ok(handle) = Arc::try_unwrap(handle)
        {
            let _ = handle.await;
        }
    }

    /// Places a new order via WebSocket API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn place_order(&self, params: NewOrderParams) -> BinanceWsApiResult<String> {
        let id = self.next_request_id();
        let cmd = HandlerCommand::PlaceOrder {
            id: id.clone(),
            params,
        };
        self.send_cmd(cmd).await?;
        Ok(id)
    }

    /// Cancels an order via WebSocket API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn cancel_order(&self, params: CancelOrderParams) -> BinanceWsApiResult<String> {
        let id = self.next_request_id();
        let cmd = HandlerCommand::CancelOrder {
            id: id.clone(),
            params,
        };
        self.send_cmd(cmd).await?;
        Ok(id)
    }

    /// Cancel and replace an order atomically via WebSocket API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn cancel_replace_order(
        &self,
        params: CancelReplaceOrderParams,
    ) -> BinanceWsApiResult<String> {
        let id = self.next_request_id();
        let cmd = HandlerCommand::CancelReplaceOrder {
            id: id.clone(),
            params,
        };
        self.send_cmd(cmd).await?;
        Ok(id)
    }

    /// Cancels all open orders for a symbol via WebSocket API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn cancel_all_orders(&self, symbol: impl Into<String>) -> BinanceWsApiResult<String> {
        let id = self.next_request_id();
        let cmd = HandlerCommand::CancelAllOrders {
            id: id.clone(),
            symbol: symbol.into(),
        };
        self.send_cmd(cmd).await?;
        Ok(id)
    }

    /// Receives the next message from the handler.
    ///
    /// Returns `None` if the receiver is closed or not initialized.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub async fn recv(&self) -> Option<NautilusWsApiMessage> {
        // Take the receiver out of the mutex to avoid holding it across await
        let rx_opt = {
            let mut rx_guard = self.out_rx.lock().expect("Mutex poisoned");
            rx_guard.take()
        };

        if let Some(mut rx) = rx_opt {
            let result = rx.recv().await;

            let mut rx_guard = self.out_rx.lock().expect("Mutex poisoned");
            *rx_guard = Some(rx);
            result
        } else {
            None
        }
    }

    async fn send_cmd(&self, cmd: HandlerCommand) -> BinanceWsApiResult<()> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| BinanceWsApiError::HandlerUnavailable(e.to_string()))
    }
}
