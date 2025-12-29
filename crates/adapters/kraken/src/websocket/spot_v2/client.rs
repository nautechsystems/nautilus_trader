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

//! WebSocket client for the Kraken v2 streaming API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_model::{
    data::BarType,
    enums::BarAggregation,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId},
    instruments::InstrumentAny,
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

/// Topic delimiter for Kraken Spot v2 WebSocket subscriptions.
///
/// Topics use colon format: `channel:symbol` (e.g., `Trade:ETH/USD`).
pub const KRAKEN_SPOT_WS_TOPIC_DELIMITER: char = ':';

use super::{
    enums::{KrakenWsChannel, KrakenWsMethod},
    handler::{SpotFeedHandler, SpotHandlerCommand},
    messages::{KrakenWsParams, KrakenWsRequest, NautilusWsMessage},
};
use crate::{
    config::KrakenDataClientConfig, http::KrakenSpotHttpClient, websocket::error::KrakenWsError,
};

const WS_PING_MSG: &str = r#"{"method":"ping"}"#;

/// WebSocket client for the Kraken Spot v2 streaming API.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken")
)]
pub struct KrakenSpotWebSocketClient {
    url: String,
    config: KrakenDataClientConfig,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    req_id_counter: Arc<tokio::sync::RwLock<u64>>,
    auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl Clone for KrakenSpotWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            config: self.config.clone(),
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: self.out_rx.clone(),
            task_handle: self.task_handle.clone(),
            subscriptions: self.subscriptions.clone(),
            auth_tracker: self.auth_tracker.clone(),
            cancellation_token: self.cancellation_token.clone(),
            req_id_counter: self.req_id_counter.clone(),
            auth_token: self.auth_token.clone(),
        }
    }
}

impl KrakenSpotWebSocketClient {
    /// Creates a new client with the given configuration.
    pub fn new(config: KrakenDataClientConfig, cancellation_token: CancellationToken) -> Self {
        // Prefer private URL if explicitly set (for authenticated endpoints)
        let url = if config.ws_private_url.is_some() {
            config.ws_private_url()
        } else {
            config.ws_public_url()
        };
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            config,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: SubscriptionState::new(KRAKEN_SPOT_WS_TOPIC_DELIMITER),
            auth_tracker: AuthTracker::new(),
            cancellation_token,
            req_id_counter: Arc::new(tokio::sync::RwLock::new(0)),
            auth_token: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    async fn get_next_req_id(&self) -> u64 {
        let mut counter = self.req_id_counter.write().await;
        *counter += 1;
        *counter
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Connecting to {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: self.config.heartbeat_interval_secs,
            heartbeat_msg: Some(WS_PING_MSG.to_string()),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
        };

        let ws_client = WebSocketClient::connect(
            ws_config,
            Some(raw_handler),
            None,   // ping_handler
            None,   // post_reconnection
            vec![], // keyed_quotas
            None,   // default_quota
        )
        .await
        .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        // Share connection state across clones via ArcSwap
        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(SpotHandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();
        let config_for_reconnect = self.config.clone();
        let auth_token_for_reconnect = self.auth_token.clone();
        let req_id_counter_for_reconnect = self.req_id_counter.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                SpotFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions.clone());

            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        tracing::info!("WebSocket reconnected, resubscribing");

                        // Mark all confirmed subscriptions as failed to transition to pending
                        let confirmed_topics = subscriptions.all_topics();
                        for topic in &confirmed_topics {
                            subscriptions.mark_failure(topic);
                        }

                        let topics = subscriptions.all_topics();
                        if topics.is_empty() {
                            tracing::debug!("No subscriptions to restore after reconnection");
                        } else {
                            // Check if we need to re-authenticate (had a token before)
                            let had_auth = auth_token_for_reconnect.read().await.is_some();

                            if had_auth && config_for_reconnect.has_api_credentials() {
                                tracing::debug!("Re-authenticating after reconnect");

                                match refresh_auth_token(&config_for_reconnect).await {
                                    Ok(new_token) => {
                                        *auth_token_for_reconnect.write().await = Some(new_token);
                                        tracing::debug!("Re-authentication successful");
                                    }
                                    Err(e) => {
                                        tracing::error!(
                                            error = %e,
                                            "Failed to re-authenticate after reconnect"
                                        );
                                        // Clear auth token since it's invalid
                                        *auth_token_for_reconnect.write().await = None;
                                    }
                                }
                            }

                            tracing::info!(
                                count = topics.len(),
                                "Resubscribing after reconnection"
                            );

                            // Replay subscriptions
                            for topic in &topics {
                                let auth_token = auth_token_for_reconnect.read().await.clone();

                                // Handle special "executions" topic first
                                if topic == "executions" {
                                    if let Some(ref token) = auth_token {
                                        let mut counter =
                                            req_id_counter_for_reconnect.write().await;
                                        *counter += 1;
                                        let req_id = *counter;

                                        let request = KrakenWsRequest {
                                            method: KrakenWsMethod::Subscribe,
                                            params: Some(KrakenWsParams {
                                                channel: KrakenWsChannel::Executions,
                                                symbol: None,
                                                snapshot: None,
                                                depth: None,
                                                interval: None,
                                                token: Some(token.clone()),
                                                snap_orders: Some(true),
                                                snap_trades: Some(true),
                                            }),
                                            req_id: Some(req_id),
                                        };

                                        if let Ok(payload) = serde_json::to_string(&request)
                                            && let Err(e) = cmd_tx_for_reconnect
                                                .send(SpotHandlerCommand::SendText { payload })
                                        {
                                            tracing::error!(
                                                error = %e,
                                                "Failed to send executions resubscribe"
                                            );
                                        }

                                        subscriptions.mark_subscribe(topic);
                                    } else {
                                        tracing::warn!(
                                            "Cannot resubscribe to executions: no auth token"
                                        );
                                    }
                                    continue;
                                }

                                // Parse topic format: "Channel:symbol" or "Channel:symbol:interval"
                                let parts: Vec<&str> = topic.splitn(3, ':').collect();
                                if parts.len() < 2 {
                                    tracing::warn!(topic, "Invalid topic format for resubscribe");
                                    continue;
                                }

                                let channel_str = parts[0];
                                let channel = match channel_str {
                                    "Book" => Some(KrakenWsChannel::Book),
                                    "Trade" => Some(KrakenWsChannel::Trade),
                                    "Ticker" => Some(KrakenWsChannel::Ticker),
                                    "Ohlc" => Some(KrakenWsChannel::Ohlc),
                                    "book" => Some(KrakenWsChannel::Book),
                                    "quotes" => Some(KrakenWsChannel::Book),
                                    _ => None,
                                };

                                let Some(channel) = channel else {
                                    tracing::warn!(topic, "Unknown channel for resubscribe");
                                    continue;
                                };

                                let mut counter = req_id_counter_for_reconnect.write().await;
                                *counter += 1;
                                let req_id = *counter;

                                let depth = if channel_str == "quotes" {
                                    Some(10)
                                } else {
                                    None
                                };

                                // Extract symbol and optional interval
                                let (symbol_str, interval) = if parts.len() == 3 {
                                    // Format: "Ohlc:BTC/USD:1" -> symbol="BTC/USD", interval=1
                                    (parts[1], parts[2].parse::<u32>().ok())
                                } else {
                                    // Format: "Book:BTC/USD" -> symbol="BTC/USD", interval=None
                                    (parts[1], None)
                                };

                                let request = KrakenWsRequest {
                                    method: KrakenWsMethod::Subscribe,
                                    params: Some(KrakenWsParams {
                                        channel,
                                        symbol: Some(vec![Ustr::from(symbol_str)]),
                                        snapshot: None,
                                        depth,
                                        interval,
                                        token: None,
                                        snap_orders: None,
                                        snap_trades: None,
                                    }),
                                    req_id: Some(req_id),
                                };

                                if let Ok(payload) = serde_json::to_string(&request)
                                    && let Err(e) = cmd_tx_for_reconnect
                                        .send(SpotHandlerCommand::SendText { payload })
                                {
                                    tracing::error!(
                                        error = %e,
                                        topic,
                                        "Failed to send resubscribe command"
                                    );
                                }

                                subscriptions.mark_subscribe(topic);
                            }
                        }

                        if out_tx.send(NautilusWsMessage::Reconnected).is_err() {
                            tracing::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                        continue;
                    }
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            tracing::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            tracing::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        tracing::debug!("WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Disconnecting WebSocket");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self
            .cmd_tx
            .read()
            .await
            .send(SpotHandlerCommand::Disconnect)
        {
            tracing::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    tracing::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => tracing::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => tracing::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            tracing::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                        }
                    }
                }
                Err(arc_handle) => {
                    tracing::debug!(
                        "Cannot take ownership of task handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            tracing::debug!("No task handle to await");
        }

        self.subscriptions.clear();
        self.auth_tracker.fail("Disconnected");

        Ok(())
    }

    /// Closes the WebSocket connection.
    pub async fn close(&mut self) -> Result<(), KrakenWsError> {
        self.disconnect().await
    }

    /// Waits until the connection is active or timeout.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), KrakenWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            KrakenWsError::ConnectionError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    /// Authenticates with the Kraken API to enable private subscriptions.
    pub async fn authenticate(&self) -> Result<(), KrakenWsError> {
        if !self.config.has_api_credentials() {
            return Err(KrakenWsError::AuthenticationError(
                "API credentials required for authentication".to_string(),
            ));
        }

        let api_key = self
            .config
            .api_key
            .clone()
            .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API key".to_string()))?;
        let api_secret =
            self.config.api_secret.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError("Missing API secret".to_string())
            })?;

        let http_client = KrakenSpotHttpClient::with_credentials(
            api_key,
            api_secret,
            self.config.environment,
            Some(self.config.http_base_url()),
            self.config.timeout_secs,
            None,
            None,
            None,
            self.config.http_proxy.clone(),
            self.config.max_requests_per_second,
        )
        .map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to create HTTP client: {e}"))
        })?;

        let ws_token = http_client.get_websockets_token().await.map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to get WebSocket token: {e}"))
        })?;

        tracing::debug!(
            token_length = ws_token.token.len(),
            expires = ws_token.expires,
            "WebSocket authentication token received"
        );

        let mut auth_token = self.auth_token.write().await;
        *auth_token = Some(ws_token.token);

        Ok(())
    }

    /// Caches multiple instruments for symbol lookup.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        // Before connect() the handler isn't running; this send will fail and that's expected
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::InitializeInstruments(instruments))
        {
            tracing::debug!("Failed to send instruments to handler: {e}");
        }
    }

    /// Caches a single instrument for symbol lookup.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        // Before connect() the handler isn't running; this send will fail and that's expected
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::UpdateInstrument(instrument))
        {
            tracing::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    /// Sets the account ID for execution reports.
    ///
    /// Must be called before subscribing to executions to properly generate
    /// OrderStatusReport and FillReport objects.
    pub fn set_account_id(&self, account_id: AccountId) {
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::SetAccountId(account_id))
        {
            tracing::debug!("Failed to send account ID to handler: {e}");
        }
    }

    /// Caches order info for order tracking.
    ///
    /// This should be called BEFORE submitting an order via HTTP to handle the
    /// race condition where WebSocket execution messages arrive before the
    /// HTTP response (which contains the venue_order_id).
    pub fn cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) {
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(SpotHandlerCommand::CacheClientOrder {
                client_order_id,
                instrument_id,
                trader_id,
                strategy_id,
            })
        {
            tracing::debug!("Failed to send cache client order command to handler: {e}");
        }
    }

    /// Cancels all pending requests.
    pub fn cancel_all_requests(&self) {
        self.cancellation_token.cancel();
    }

    /// Returns the cancellation token for this client.
    pub fn cancellation_token(&self) -> &CancellationToken {
        &self.cancellation_token
    }

    /// Subscribes to a channel for the given symbols.
    pub async fn subscribe(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        let mut symbols_to_subscribe = Vec::new();
        for symbol in &symbols {
            let key = format!("{:?}:{}", channel, symbol);
            if self.subscriptions.add_reference(&key) {
                self.subscriptions.mark_subscribe(&key);
                symbols_to_subscribe.push(*symbol);
            }
        }

        if symbols_to_subscribe.is_empty() {
            return Ok(());
        }

        let is_private = matches!(
            channel,
            KrakenWsChannel::Executions | KrakenWsChannel::Balances
        );
        let token = if is_private {
            Some(self.auth_token.read().await.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError(
                    "Authentication token required for private channels. Call authenticate() first"
                        .to_string(),
                )
            })?)
        } else {
            None
        };

        let req_id = self.get_next_req_id().await;
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols_to_subscribe.clone()),
                snapshot: None,
                depth,
                interval: None,
                token,
                snap_orders: None,
                snap_trades: None,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in &symbols_to_subscribe {
            let key = format!("{:?}:{}", channel, symbol);
            self.subscriptions.confirm_subscribe(&key);
        }

        Ok(())
    }

    /// Subscribes to a channel with a specific interval (for OHLC).
    async fn subscribe_with_interval(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
        interval: u32,
    ) -> Result<(), KrakenWsError> {
        let mut symbols_to_subscribe = Vec::new();
        for symbol in &symbols {
            let key = format!("{channel:?}:{symbol}:{interval}");
            if self.subscriptions.add_reference(&key) {
                self.subscriptions.mark_subscribe(&key);
                symbols_to_subscribe.push(*symbol);
            }
        }

        if symbols_to_subscribe.is_empty() {
            return Ok(());
        }

        let req_id = self.get_next_req_id().await;
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols_to_subscribe.clone()),
                snapshot: None,
                depth: None,
                interval: Some(interval),
                token: None,
                snap_orders: None,
                snap_trades: None,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in &symbols_to_subscribe {
            let key = format!("{channel:?}:{symbol}:{interval}");
            self.subscriptions.confirm_subscribe(&key);
        }

        Ok(())
    }

    /// Unsubscribes from a channel with a specific interval (for OHLC).
    async fn unsubscribe_with_interval(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
        interval: u32,
    ) -> Result<(), KrakenWsError> {
        let mut symbols_to_unsubscribe = Vec::new();
        for symbol in &symbols {
            let key = format!("{channel:?}:{symbol}:{interval}");
            if self.subscriptions.remove_reference(&key) {
                self.subscriptions.mark_unsubscribe(&key);
                symbols_to_unsubscribe.push(*symbol);
            }
        }

        if symbols_to_unsubscribe.is_empty() {
            return Ok(());
        }

        let req_id = self.get_next_req_id().await;
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols_to_unsubscribe.clone()),
                snapshot: None,
                depth: None,
                interval: Some(interval),
                token: None,
                snap_orders: None,
                snap_trades: None,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in &symbols_to_unsubscribe {
            let key = format!("{channel:?}:{symbol}:{interval}");
            self.subscriptions.confirm_unsubscribe(&key);
        }

        Ok(())
    }

    /// Unsubscribes from a channel for the given symbols.
    pub async fn unsubscribe(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
    ) -> Result<(), KrakenWsError> {
        let mut symbols_to_unsubscribe = Vec::new();
        for symbol in &symbols {
            let key = format!("{:?}:{}", channel, symbol);
            if self.subscriptions.remove_reference(&key) {
                self.subscriptions.mark_unsubscribe(&key);
                symbols_to_unsubscribe.push(*symbol);
            } else {
                tracing::debug!(
                    "Channel {:?} symbol {} still has active subscriptions, not unsubscribing",
                    channel,
                    symbol
                );
            }
        }

        if symbols_to_unsubscribe.is_empty() {
            return Ok(());
        }

        let is_private = matches!(
            channel,
            KrakenWsChannel::Executions | KrakenWsChannel::Balances
        );
        let token = if is_private {
            Some(self.auth_token.read().await.clone().ok_or_else(|| {
                KrakenWsError::AuthenticationError(
                    "Authentication token required for private channels. Call authenticate() first"
                        .to_string(),
                )
            })?)
        } else {
            None
        };

        let req_id = self.get_next_req_id().await;
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams {
                channel,
                symbol: Some(symbols_to_unsubscribe.clone()),
                snapshot: None,
                depth: None,
                interval: None,
                token,
                snap_orders: None,
                snap_trades: None,
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        for symbol in &symbols_to_unsubscribe {
            let key = format!("{:?}:{}", channel, symbol);
            self.subscriptions.confirm_unsubscribe(&key);
        }

        Ok(())
    }

    /// Sends a ping message to keep the connection alive.
    pub async fn send_ping(&self) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id().await;

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Ping,
            params: None,
            req_id: Some(req_id),
        };

        self.send_request(&request).await
    }

    async fn send_request(&self, request: &KrakenWsRequest) -> Result<(), KrakenWsError> {
        let payload =
            serde_json::to_string(request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

        tracing::trace!("Sending message: {payload}");

        self.cmd_tx
            .read()
            .await
            .send(SpotHandlerCommand::SendText { payload })
            .map_err(|e| KrakenWsError::ConnectionError(format!("Failed to send request: {e}")))?;

        Ok(())
    }

    /// Returns true if connected (not closed).
    pub fn is_connected(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        !ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
    }

    /// Returns true if the connection is active.
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Relaxed)
    }

    /// Returns true if the connection is closed.
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Relaxed)
    }

    /// Returns the WebSocket URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns all active subscriptions.
    pub fn get_subscriptions(&self) -> Vec<String> {
        self.subscriptions.all_topics()
    }

    /// Returns a stream of WebSocket messages.
    pub fn stream(&mut self) -> impl futures_util::Stream<Item = NautilusWsMessage> + use<> {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or client not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Subscribes to order book updates for the given instrument.
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        // Kraken v2 WebSocket expects ISO 4217-A3 format (e.g., "ETH/USD")
        let symbol = instrument_id.symbol.inner();
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.add_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&book_key);
        self.subscriptions.confirm_subscribe(&book_key);

        self.subscribe(KrakenWsChannel::Book, vec![symbol], depth)
            .await
    }

    /// Subscribes to quote updates for the given instrument.
    ///
    /// Uses the order book channel with depth 10 for low-latency top-of-book quotes
    /// instead of the throttled ticker feed.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        let quotes_key = format!("quotes:{symbol}");

        if !self.subscriptions.add_reference(&quotes_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&quotes_key);
        self.subscriptions.confirm_subscribe(&quotes_key);
        self.ensure_book_subscribed(symbol).await
    }

    /// Subscribes to trade updates for the given instrument.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.subscribe(KrakenWsChannel::Trade, vec![symbol], None)
            .await
    }

    /// Subscribes to bar/OHLC updates for the given bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar aggregation is not supported by Kraken.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = bar_type.instrument_id().symbol.inner();
        let interval = bar_type_to_ws_interval(bar_type)?;
        self.subscribe_with_interval(KrakenWsChannel::Ohlc, vec![symbol], interval)
            .await
    }

    /// Subscribes to execution updates (order and fill events).
    ///
    /// Requires authentication - call `authenticate()` first.
    pub async fn subscribe_executions(
        &self,
        snap_orders: bool,
        snap_trades: bool,
    ) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id().await;

        let token = self.auth_token.read().await.clone().ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "Authentication token required for executions channel. Call authenticate() first"
                    .to_string(),
            )
        })?;

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams {
                channel: KrakenWsChannel::Executions,
                symbol: None,
                snapshot: None,
                depth: None,
                interval: None,
                token: Some(token),
                snap_orders: Some(snap_orders),
                snap_trades: Some(snap_trades),
            }),
            req_id: Some(req_id),
        };

        self.send_request(&request).await?;

        let key = "executions";
        if self.subscriptions.add_reference(key) {
            self.subscriptions.mark_subscribe(key);
            self.subscriptions.confirm_subscribe(key);
        }

        Ok(())
    }

    /// Unsubscribes from order book updates for the given instrument.
    ///
    /// Note: Will only actually unsubscribe if quotes are not also subscribed.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.remove_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&book_key);
        self.subscriptions.confirm_unsubscribe(&book_key);
        self.maybe_unsubscribe_book(symbol).await
    }

    /// Unsubscribes from quote updates for the given instrument.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        let quotes_key = format!("quotes:{symbol}");

        if !self.subscriptions.remove_reference(&quotes_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&quotes_key);
        self.subscriptions.confirm_unsubscribe(&quotes_key);
        self.maybe_unsubscribe_book(symbol).await
    }

    /// Unsubscribes from trade updates for the given instrument.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(KrakenWsChannel::Trade, vec![symbol]).await
    }

    /// Unsubscribes from bar/OHLC updates for the given bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar aggregation is not supported by Kraken.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = bar_type.instrument_id().symbol.inner();
        let interval = bar_type_to_ws_interval(bar_type)?;
        self.unsubscribe_with_interval(KrakenWsChannel::Ohlc, vec![symbol], interval)
            .await
    }

    /// Ensures book channel is subscribed for the given symbol (used internally by quotes).
    ///
    /// Reference counting is handled by `subscribe` method.
    async fn ensure_book_subscribed(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        self.subscribe(KrakenWsChannel::Book, vec![symbol], Some(10))
            .await
    }

    /// Unsubscribes from book channel if no more dependent subscriptions.
    ///
    /// Reference counting is handled by `unsubscribe` method.
    async fn maybe_unsubscribe_book(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        self.unsubscribe(KrakenWsChannel::Book, vec![symbol]).await
    }
}

/// Helper function to refresh authentication token via HTTP API.
async fn refresh_auth_token(config: &KrakenDataClientConfig) -> Result<String, KrakenWsError> {
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API key".to_string()))?;
    let api_secret = config
        .api_secret
        .clone()
        .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API secret".to_string()))?;

    let http_client = KrakenSpotHttpClient::with_credentials(
        api_key,
        api_secret,
        config.environment,
        Some(config.http_base_url()),
        config.timeout_secs,
        None,
        None,
        None,
        config.http_proxy.clone(),
        config.max_requests_per_second,
    )
    .map_err(|e| {
        KrakenWsError::AuthenticationError(format!("Failed to create HTTP client: {e}"))
    })?;

    let ws_token = http_client.get_websockets_token().await.map_err(|e| {
        KrakenWsError::AuthenticationError(format!("Failed to get WebSocket token: {e}"))
    })?;

    tracing::debug!(
        token_length = ws_token.token.len(),
        expires = ws_token.expires,
        "WebSocket authentication token refreshed"
    );

    Ok(ws_token.token)
}

/// Converts a Nautilus BarType to Kraken WebSocket OHLC interval (in minutes).
///
/// Supported intervals: 1, 5, 15, 30, 60, 240, 1440, 10080, 21600
/// (1m, 5m, 15m, 30m, 1h, 4h, 1d, 1w, 2w).
fn bar_type_to_ws_interval(bar_type: BarType) -> Result<u32, KrakenWsError> {
    let spec = bar_type.spec();
    let step = spec.step.get() as u32;

    let base_minutes = match spec.aggregation {
        BarAggregation::Minute => 1,
        BarAggregation::Hour => 60,
        BarAggregation::Day => 1440,
        BarAggregation::Week => 10080,
        other => {
            return Err(KrakenWsError::SubscriptionError(format!(
                "Unsupported bar aggregation for Kraken OHLC streaming: {other:?}"
            )));
        }
    };

    let interval = base_minutes * step;

    const VALID_INTERVALS: [u32; 9] = [1, 5, 15, 30, 60, 240, 1440, 10080, 21600];
    if !VALID_INTERVALS.contains(&interval) {
        return Err(KrakenWsError::SubscriptionError(format!(
            "Invalid bar interval {interval} minutes for Kraken OHLC streaming. \
             Supported intervals: 1, 5, 15, 30, 60, 240, 1440, 10080, 21600"
        )));
    }

    Ok(interval)
}
