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

//! Binance Futures WebSocket Trading API client.
//!
//! ## Connection details
//!
//! - Endpoint: `ws-fapi.binance.com/ws-fapi/v1` (USD-M only)
//! - Authentication: HMAC-SHA256 signature per request
//! - JSON request/response pattern
//! - Connection validity: 24 hours
//! - Ping/pong: every 20 seconds

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
use nautilus_core::string::secret::REDACTED;
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        PingHandler, TransportBackend, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use super::{
    error::{BinanceFuturesWsApiError, BinanceFuturesWsApiResult},
    handler::BinanceFuturesWsTradingHandler,
    messages::{BinanceFuturesWsTradingCommand, BinanceFuturesWsTradingMessage},
};
use crate::{
    common::{
        consts::{BINANCE_API_KEY_HEADER, BINANCE_FUTURES_USD_WS_API_URL},
        credential::SigningCredential,
    },
    futures::http::query::{
        BinanceCancelOrderParams, BinanceModifyOrderParams, BinanceNewOrderParams,
    },
};

/// Pre-interned rate limit key for futures order operations (place/cancel/modify).
///
/// Binance Futures WebSocket API: 1200 requests per minute per IP (20/sec).
pub static BINANCE_FUTURES_WS_RATE_LIMIT_KEY_ORDER: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("futures_order")]);

/// Returns the Binance Futures WebSocket API order rate limit quota (1200 per minute).
// Constant values are provably valid
#[expect(clippy::missing_panics_doc)]
#[must_use]
pub fn binance_futures_ws_order_quota() -> Quota {
    Quota::per_second(NonZeroU32::new(20).expect("non-zero")).expect("valid constant")
}

/// Binance Futures WebSocket Trading API client.
///
/// Provides order management via WebSocket with JSON responses,
/// complementing the HTTP client with lower-latency order submission.
/// Only available for USD-M Futures.
#[derive(Clone)]
pub struct BinanceFuturesWsTradingClient {
    url: String,
    credential: Arc<SigningCredential>,
    heartbeat: Option<u64>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<
        tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<BinanceFuturesWsTradingCommand>>,
    >,
    out_rx:
        Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<BinanceFuturesWsTradingMessage>>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    request_id_counter: Arc<AtomicU64>,
    cancellation_token: CancellationToken,
    transport_backend: TransportBackend,
}

impl Debug for BinanceFuturesWsTradingClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(BinanceFuturesWsTradingClient))
            .field("url", &self.url)
            .field("credential", &REDACTED)
            .field("heartbeat", &self.heartbeat)
            .finish_non_exhaustive()
    }
}

impl BinanceFuturesWsTradingClient {
    /// Creates a new [`BinanceFuturesWsTradingClient`] instance.
    #[must_use]
    pub fn new(
        url: Option<String>,
        api_key: String,
        api_secret: String,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
    ) -> Self {
        let url = url.unwrap_or_else(|| BINANCE_FUTURES_USD_WS_API_URL.to_string());
        let credential = Arc::new(SigningCredential::new(api_key, api_secret));

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
            transport_backend,
        }
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

    pub fn next_request_id(&self) -> String {
        let id = self.request_id_counter.fetch_add(1, Ordering::Relaxed);
        format!("req-{id}")
    }

    /// Connects to the WebSocket Trading API server.
    ///
    /// # Errors
    ///
    /// Returns an error if connection fails.
    // Mutex poisoning is not documented individually
    #[expect(clippy::missing_panics_doc)]
    pub async fn connect(&mut self) -> BinanceFuturesWsApiResult<()> {
        self.signal.store(false, Ordering::Relaxed);
        self.cancellation_token = CancellationToken::new();

        let (raw_handler, raw_rx) = channel_message_handler();
        let ping_handler: PingHandler = Arc::new(move |_| {});

        let headers = vec![(
            BINANCE_API_KEY_HEADER.to_string(),
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
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: None,
        };

        let keyed_quotas = vec![(
            BINANCE_FUTURES_WS_RATE_LIMIT_KEY_ORDER[0]
                .as_str()
                .to_string(),
            binance_futures_ws_order_quota(),
        )];

        let client = WebSocketClient::connect(
            config,
            Some(raw_handler),
            Some(ping_handler),
            None,
            keyed_quotas,
            Some(binance_futures_ws_order_quota()),
        )
        .await
        .map_err(|e| BinanceFuturesWsApiError::ConnectionError(e.to_string()))?;

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
        let mut handler =
            BinanceFuturesWsTradingHandler::new(signal, cmd_rx, raw_rx, out_tx, credential);

        self.cmd_tx
            .read()
            .await
            .send(BinanceFuturesWsTradingCommand::SetClient(client))
            .map_err(|e| BinanceFuturesWsApiError::HandlerUnavailable(e.to_string()))?;

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

    /// Disconnects from the WebSocket Trading API server.
    pub async fn disconnect(&mut self) {
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self
            .cmd_tx
            .read()
            .await
            .send(BinanceFuturesWsTradingCommand::Disconnect)
        {
            log::warn!("Failed to send disconnect command: {e}");
        }

        self.cancellation_token.cancel();

        if let Some(handle) = self.task_handle.take()
            && let Ok(handle) = Arc::try_unwrap(handle)
        {
            let _ = handle.await;
        }
    }

    /// Places a new order via the WebSocket Trading API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn place_order(
        &self,
        params: BinanceNewOrderParams,
    ) -> BinanceFuturesWsApiResult<String> {
        let id = self.next_request_id();
        self.place_order_with_id(id.clone(), params).await?;
        Ok(id)
    }

    /// Places a new order via the WebSocket Trading API using a pre-generated request ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn place_order_with_id(
        &self,
        id: String,
        params: BinanceNewOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let cmd = BinanceFuturesWsTradingCommand::PlaceOrder { id, params };
        self.send_cmd(cmd).await
    }

    /// Cancels an order via the WebSocket Trading API.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn cancel_order(
        &self,
        params: BinanceCancelOrderParams,
    ) -> BinanceFuturesWsApiResult<String> {
        let id = self.next_request_id();
        self.cancel_order_with_id(id.clone(), params).await?;
        Ok(id)
    }

    /// Cancels an order via the WebSocket Trading API using a pre-generated request ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn cancel_order_with_id(
        &self,
        id: String,
        params: BinanceCancelOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let cmd = BinanceFuturesWsTradingCommand::CancelOrder { id, params };
        self.send_cmd(cmd).await
    }

    /// Modifies an order via the WebSocket Trading API (in-place amendment).
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn modify_order(
        &self,
        params: BinanceModifyOrderParams,
    ) -> BinanceFuturesWsApiResult<String> {
        let id = self.next_request_id();
        self.modify_order_with_id(id.clone(), params).await?;
        Ok(id)
    }

    /// Modifies an order via the WebSocket Trading API using a pre-generated request ID.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler is unavailable.
    pub async fn modify_order_with_id(
        &self,
        id: String,
        params: BinanceModifyOrderParams,
    ) -> BinanceFuturesWsApiResult<()> {
        let cmd = BinanceFuturesWsTradingCommand::ModifyOrder { id, params };
        self.send_cmd(cmd).await
    }

    /// Receives the next message from the handler.
    ///
    /// Returns `None` if the receiver is closed or not initialized.
    ///
    /// # Panics
    ///
    /// Panics if the internal output receiver mutex is poisoned.
    pub async fn recv(&self) -> Option<BinanceFuturesWsTradingMessage> {
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

    async fn send_cmd(&self, cmd: BinanceFuturesWsTradingCommand) -> BinanceFuturesWsApiResult<()> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| BinanceFuturesWsApiError::HandlerUnavailable(e.to_string()))
    }
}
