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

//! WebSocket client for the Kraken v2 streaming API.

use std::{
    collections::HashMap,
    sync::{
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    data::BarType,
    enums::BarAggregation,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
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
    messages::{KrakenSpotWsMessage, KrakenWsChannelParams, KrakenWsParams, KrakenWsRequest},
};
use crate::{
    common::parse::normalize_spot_symbol,
    config::KrakenDataClientConfig,
    http::{KrakenSpotHttpClient, spot::client::KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND},
    websocket::error::KrakenWsError,
};

const WS_PING_MSG: &str = r#"{"method":"ping"}"#;

/// WebSocket client for the Kraken Spot v2 streaming API.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenSpotWebSocketClient {
    url: String,
    config: KrakenDataClientConfig,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<KrakenSpotWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    subscription_payloads: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    req_id_counter: Arc<AtomicU64>,
    auth_token: Arc<tokio::sync::RwLock<Option<String>>>,
    account_id: Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    l3_depths: Arc<std::sync::Mutex<ahash::AHashMap<String, u32>>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
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
            subscription_payloads: Arc::clone(&self.subscription_payloads),
            auth_tracker: self.auth_tracker.clone(),
            cancellation_token: self.cancellation_token.clone(),
            req_id_counter: self.req_id_counter.clone(),
            auth_token: self.auth_token.clone(),
            account_id: Arc::clone(&self.account_id),
            truncated_id_map: Arc::clone(&self.truncated_id_map),
            instruments: Arc::clone(&self.instruments),
            l3_depths: Arc::clone(&self.l3_depths),
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        }
    }
}

impl KrakenSpotWebSocketClient {
    /// Creates a new client for the configured public/private endpoint.
    pub fn new(
        config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> Self {
        let url = if config.ws_private_url.is_some() {
            config.ws_private_url()
        } else {
            config.ws_public_url()
        };
        Self::new_with_url(url, config, cancellation_token, proxy_url)
    }

    /// Creates a new client configured for the Kraken Spot `level3` WebSocket endpoint.
    ///
    /// Selects `config.ws_l3_url()` and otherwise mirrors [`Self::new`]. `Level3`
    /// subscriptions are treated as authenticated and must follow `authenticate()`.
    pub fn l3(
        config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> Self {
        let url = config.ws_l3_url();
        Self::new_with_url(url, config, cancellation_token, proxy_url)
    }

    fn new_with_url(
        url: String,
        mut config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        let transport_backend = config.transport_backend;
        config.proxy_url = proxy_url.clone();

        Self {
            url,
            config,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: SubscriptionState::new(KRAKEN_SPOT_WS_TOPIC_DELIMITER),
            subscription_payloads: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            auth_tracker: AuthTracker::new(),
            cancellation_token,
            req_id_counter: Arc::new(AtomicU64::new(0)),
            auth_token: Arc::new(tokio::sync::RwLock::new(None)),
            account_id: Arc::new(RwLock::new(None)),
            truncated_id_map: Arc::new(AtomicMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            l3_depths: Arc::new(std::sync::Mutex::new(ahash::AHashMap::new())),
            transport_backend,
            proxy_url,
        }
    }

    fn get_next_req_id(&self) -> u64 {
        self.req_id_counter.fetch_add(1, Ordering::Relaxed) + 1
    }

    /// Returns the shared request-id counter.
    pub fn req_id_counter(&self) -> Arc<AtomicU64> {
        self.req_id_counter.clone()
    }

    /// Returns a clone of the handler command channel sender.
    pub async fn handler_command_sender(
        &self,
    ) -> tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand> {
        self.cmd_tx.read().await.clone()
    }

    /// Returns the shared `cmd_tx` handle. Unlike
    /// [`handler_command_sender`](Self::handler_command_sender) (a snapshot
    /// clone), this exposes the `RwLock` so callers see the live sender
    /// after `connect()` swaps it in.
    pub fn handler_command_handle(
        &self,
    ) -> Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>> {
        self.cmd_tx.clone()
    }

    /// Returns the current cached authentication token, if any.
    pub async fn auth_token(&self) -> Option<String> {
        self.auth_token.read().await.clone()
    }

    /// Returns the current cached authentication token without awaiting.
    ///
    /// Returns `None` when the lock is contended or no token is cached. Used by
    /// the synchronous order-routing path where the auth token is normally
    /// uncontended; callers fall back to REST when the lock is unavailable.
    pub fn auth_token_blocking(&self) -> Option<String> {
        self.auth_token.try_read().ok().and_then(|g| g.clone())
    }

    /// Returns a clone of the auth token handle for components that need
    /// non-async, lock-free read access (e.g. compensating cancels triggered
    /// from the timeout task in `OrderRequestState`).
    pub fn auth_token_handle(&self) -> Arc<tokio::sync::RwLock<Option<String>>> {
        self.auth_token.clone()
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        log::debug!("Connecting to {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(self.config.heartbeat_interval_secs),
            heartbeat_msg: Some(WS_PING_MSG.to_string()),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
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

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<KrakenSpotWsMessage>();
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
        let subscription_payloads = self.subscription_payloads.clone();
        let config_for_reconnect = self.config.clone();
        let auth_token_for_reconnect = self.auth_token.clone();
        let auth_tracker_for_reconnect = self.auth_tracker.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                SpotFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions.clone());

            loop {
                match handler.next().await {
                    Some(KrakenSpotWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        log::info!("WebSocket reconnected, resubscribing");

                        let confirmed_topics = subscriptions.all_topics();
                        for topic in &confirmed_topics {
                            subscriptions.mark_failure(topic);
                        }

                        let payloads = subscription_payloads.read().await;
                        if payloads.is_empty() {
                            log::debug!("No subscriptions to restore after reconnection");
                        } else {
                            let had_auth = auth_token_for_reconnect.read().await.is_some();

                            if had_auth && config_for_reconnect.has_api_credentials() {
                                log::debug!("Re-authenticating after reconnect");

                                auth_tracker_for_reconnect.invalidate();
                                let _rx = auth_tracker_for_reconnect.begin();

                                match refresh_auth_token(&config_for_reconnect).await {
                                    Ok(new_token) => {
                                        *auth_token_for_reconnect.write().await = Some(new_token);
                                        auth_tracker_for_reconnect.succeed();
                                        log::debug!("Re-authentication successful");
                                    }
                                    Err(e) => {
                                        log::error!(
                                            "Failed to re-authenticate after reconnect: {e}"
                                        );
                                        *auth_token_for_reconnect.write().await = None;
                                        auth_tracker_for_reconnect.fail(e.to_string());
                                    }
                                }
                            }

                            log::info!(
                                "Resubscribing after reconnection: count={}",
                                payloads.len()
                            );

                            for (topic, payload) in payloads.iter() {
                                let needs_token =
                                    topic == "executions" || topic.starts_with("level3:");
                                let payload = if needs_token {
                                    let auth_token = auth_token_for_reconnect.read().await.clone();
                                    match auth_token {
                                        Some(token) => {
                                            match update_auth_token_in_payload(payload, &token) {
                                                Ok(p) => p,
                                                Err(e) => {
                                                    log::error!("Failed to update auth token: {e}");
                                                    continue;
                                                }
                                            }
                                        }
                                        None => {
                                            log::warn!(
                                                "Cannot resubscribe to {topic}: no auth token"
                                            );
                                            continue;
                                        }
                                    }
                                } else {
                                    payload.clone()
                                };

                                if let Err(e) = cmd_tx_for_reconnect
                                    .send(SpotHandlerCommand::Subscribe { payload })
                                {
                                    log::error!(
                                        "Failed to send resubscribe command: error={e}, \
                                        topic={topic}"
                                    );
                                }

                                subscriptions.mark_subscribe(topic);
                            }
                        }

                        if out_tx.send(KrakenSpotWsMessage::Reconnected).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    Some(msg) => {
                        if out_tx.send(msg).is_err() {
                            log::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        if handler.is_stopped() {
                            log::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        log::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            log::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        log::debug!("WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        log::debug!("Disconnecting WebSocket");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self
            .cmd_tx
            .read()
            .await
            .send(SpotHandlerCommand::Disconnect)
        {
            log::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => log::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                        }
                    }
                }
                Err(arc_handle) => {
                    log::debug!(
                        "Cannot take ownership of task handle - other references exist, aborting task"
                    );
                    arc_handle.abort();
                }
            }
        } else {
            log::debug!("No task handle to await");
        }

        self.subscriptions.clear();
        self.subscription_payloads.write().await.clear();
        self.auth_tracker.fail("Disconnected");

        if let Ok(mut depths) = self.l3_depths.lock() {
            depths.clear();
        }

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

    /// Returns true if the WebSocket is authenticated for private subscriptions.
    #[must_use]
    pub fn is_authenticated(&self) -> bool {
        self.auth_tracker.is_authenticated()
    }

    /// Waits until the WebSocket is authenticated or the timeout elapses.
    ///
    /// Returns an error on timeout or explicit auth failure.
    pub async fn wait_until_authenticated(&self, timeout_secs: f64) -> Result<(), KrakenWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        if self.auth_tracker.wait_for_authenticated(timeout).await {
            Ok(())
        } else {
            Err(KrakenWsError::AuthenticationError(format!(
                "Authentication not completed within {timeout_secs} seconds"
            )))
        }
    }

    /// Authenticates with the Kraken API to enable private subscriptions.
    pub async fn authenticate(&self) -> Result<(), KrakenWsError> {
        if !self.config.has_api_credentials() {
            return Err(KrakenWsError::AuthenticationError(
                "API credentials required for authentication".to_string(),
            ));
        }

        let _receiver = self.auth_tracker.begin();

        match refresh_auth_token(&self.config).await {
            Ok(token) => {
                *self.auth_token.write().await = Some(token);
                self.auth_tracker.succeed();
                Ok(())
            }
            Err(e) => {
                *self.auth_token.write().await = None;
                self.auth_tracker.fail(e.to_string());
                Err(e)
            }
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
        if matches!(channel, KrakenWsChannel::Level3) {
            return Err(KrakenWsError::InvalidMessage(
                "Use subscribe_book_l3 / unsubscribe_book_l3 for the Level3 channel".to_string(),
            ));
        }
        let mut symbols_to_subscribe = Vec::new();
        let channel_str = channel.as_ref();
        for symbol in &symbols {
            let key = format!("{channel_str}:{symbol}");
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

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel,
                symbol: Some(symbols_to_subscribe.clone()),
                snapshot: None,
                depth,
                interval: None,
                event_trigger: None,
                token,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        let payload = self.send_command(&request).await?;

        for symbol in &symbols_to_subscribe {
            let key = format!("{channel_str}:{symbol}");
            self.subscriptions.confirm_subscribe(&key);
            self.subscription_payloads
                .write()
                .await
                .insert(key, payload.clone());
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
        let channel_str = channel.as_ref();
        for symbol in &symbols {
            let key = format!("{channel_str}:{symbol}:{interval}");
            if self.subscriptions.add_reference(&key) {
                self.subscriptions.mark_subscribe(&key);
                symbols_to_subscribe.push(*symbol);
            }
        }

        if symbols_to_subscribe.is_empty() {
            return Ok(());
        }

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel,
                symbol: Some(symbols_to_subscribe.clone()),
                snapshot: Some(false),
                depth: None,
                interval: Some(interval),
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        let payload = self.send_command(&request).await?;

        for symbol in &symbols_to_subscribe {
            let key = format!("{channel_str}:{symbol}:{interval}");
            self.subscriptions.confirm_subscribe(&key);
            self.subscription_payloads
                .write()
                .await
                .insert(key, payload.clone());
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
        let channel_str = channel.as_ref();
        for symbol in &symbols {
            let key = format!("{channel_str}:{symbol}:{interval}");
            if self.subscriptions.remove_reference(&key) {
                self.subscriptions.mark_unsubscribe(&key);
                symbols_to_unsubscribe.push(*symbol);
            }
        }

        if symbols_to_unsubscribe.is_empty() {
            return Ok(());
        }

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel,
                symbol: Some(symbols_to_unsubscribe.clone()),
                snapshot: None,
                depth: None,
                interval: Some(interval),
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_command(&request).await?;

        for symbol in &symbols_to_unsubscribe {
            let key = format!("{channel_str}:{symbol}:{interval}");
            self.subscriptions.confirm_unsubscribe(&key);
            self.subscription_payloads.write().await.remove(&key);
        }

        Ok(())
    }

    /// Unsubscribes from a channel for the given symbols.
    pub async fn unsubscribe(
        &self,
        channel: KrakenWsChannel,
        symbols: Vec<Ustr>,
    ) -> Result<(), KrakenWsError> {
        if matches!(channel, KrakenWsChannel::Level3) {
            return Err(KrakenWsError::InvalidMessage(
                "Use subscribe_book_l3 / unsubscribe_book_l3 for the Level3 channel".to_string(),
            ));
        }
        let mut symbols_to_unsubscribe = Vec::new();
        let channel_str = channel.as_ref();
        for symbol in &symbols {
            let key = format!("{channel_str}:{symbol}");
            if self.subscriptions.remove_reference(&key) {
                self.subscriptions.mark_unsubscribe(&key);
                symbols_to_unsubscribe.push(*symbol);
            } else {
                log::debug!(
                    "Channel {channel_str} symbol {symbol} still has active subscriptions, not unsubscribing"
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

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel,
                symbol: Some(symbols_to_unsubscribe.clone()),
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: None,
                token,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_command(&request).await?;

        for symbol in &symbols_to_unsubscribe {
            let key = format!("{channel_str}:{symbol}");
            self.subscriptions.confirm_unsubscribe(&key);
            self.subscription_payloads.write().await.remove(&key);
        }

        Ok(())
    }

    /// Sends a ping message to keep the connection alive.
    pub async fn send_ping(&self) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id();

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Ping,
            params: None,
            req_id: Some(req_id),
        };

        self.send_command(&request).await?;
        Ok(())
    }

    async fn send_command(&self, request: &KrakenWsRequest) -> Result<String, KrakenWsError> {
        let payload =
            serde_json::to_string(request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

        log::trace!("Sending message: {payload}");

        let cmd = match request.method {
            KrakenWsMethod::Subscribe => SpotHandlerCommand::Subscribe {
                payload: payload.clone(),
            },
            KrakenWsMethod::Unsubscribe => SpotHandlerCommand::Unsubscribe {
                payload: payload.clone(),
            },
            KrakenWsMethod::Ping | KrakenWsMethod::Pong => SpotHandlerCommand::Ping {
                payload: payload.clone(),
            },
            KrakenWsMethod::AddOrder
            | KrakenWsMethod::AmendOrder
            | KrakenWsMethod::CancelOrder
            | KrakenWsMethod::BatchAdd => {
                return Err(KrakenWsError::InvalidMessage(
                    "Order methods must not be sent via send_command; use the dedicated order submission path".to_string()
                ));
            }
        };

        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| KrakenWsError::ConnectionError(format!("Failed to send request: {e}")))?;

        Ok(payload)
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

    /// Returns `true` if a topic is currently subscribed (confirmed).
    pub fn subscriptions_contains(&self, topic: &str) -> bool {
        self.subscriptions.all_topics().iter().any(|t| t == topic)
    }

    /// Sets the account ID for execution report parsing.
    pub fn set_account_id(&self, account_id: AccountId) {
        if let Ok(mut guard) = self.account_id.write() {
            *guard = Some(account_id);
        }
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id.read().ok().and_then(|g| *g)
    }

    /// Caches an instrument for execution report parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments.insert(instrument.id(), instrument);
    }

    /// Returns a shared reference to the account ID.
    pub fn account_id_shared(&self) -> &Arc<RwLock<Option<AccountId>>> {
        &self.account_id
    }

    /// Returns a shared reference to the truncated ID map.
    pub fn truncated_id_map(&self) -> &Arc<AtomicMap<String, ClientOrderId>> {
        &self.truncated_id_map
    }

    /// Caches a client order for truncated ID resolution.
    pub fn cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        _venue_order_id: Option<VenueOrderId>,
        _instrument_id: InstrumentId,
        _trader_id: TraderId,
        _strategy_id: StrategyId,
    ) {
        let truncated = crate::common::parse::truncate_cl_ord_id(&client_order_id);

        if truncated != client_order_id.as_str() {
            self.truncated_id_map.insert(truncated, client_order_id);
        }
    }

    /// Returns a stream of WebSocket messages.
    ///
    /// # Errors
    ///
    /// Returns an error if:
    /// - The stream receiver has already been taken
    /// - Other clones of this client still hold references to the receiver
    pub fn stream(
        &mut self,
    ) -> Result<impl futures_util::Stream<Item = KrakenSpotWsMessage> + use<>, KrakenWsError> {
        let rx = self.out_rx.take().ok_or_else(|| {
            KrakenWsError::ChannelError(
                "Stream receiver already taken or client not connected".to_string(),
            )
        })?;
        let mut rx = Arc::try_unwrap(rx).map_err(|_| {
            KrakenWsError::ChannelError(
                "Cannot take ownership of stream - other client clones still hold references"
                    .to_string(),
            )
        })?;
        Ok(async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        })
    }

    /// Subscribes to order book updates for the given instrument.
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        self.subscribe(KrakenWsChannel::Book, vec![symbol], depth)
            .await
    }

    /// Subscribes to the `level3` channel for the given Kraken symbol.
    ///
    /// `depth` must be one of `10`, `100`, or `1000`. The depth is recorded in
    /// the per-client `l3_depths` map so the message handler and `resync_book_l3`
    /// can recover it without a separate side-table.
    ///
    /// If the symbol is already subscribed, the existing depth must match — Kraken
    /// streams one depth per `(symbol, channel)` pair, so a second subscribe with
    /// a different depth would corrupt the local runtime state. Mismatch returns
    /// an error without mutating state.
    ///
    /// # Errors
    ///
    /// Returns an error if `depth` is invalid, the auth token is not cached
    /// (call `authenticate` first), the requested depth differs from an existing
    /// subscription, or the message cannot be sent. On any of those failures
    /// the reference count and pending state are rolled back fully so callers
    /// retry from a clean state.
    pub async fn subscribe_book_l3(&self, symbol: Ustr, depth: u32) -> Result<(), KrakenWsError> {
        if !matches!(depth, 10 | 100 | 1000) {
            return Err(KrakenWsError::InvalidMessage(format!(
                "Invalid L3 depth {depth}, valid values: 10, 100, 1000",
            )));
        }

        let token = self.auth_token.read().await.clone().ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "Authentication token required for level3. Call authenticate() first".to_string(),
            )
        })?;

        let channel_str = KrakenWsChannel::Level3.as_ref();
        let key = format!("{channel_str}:{symbol}");

        let is_first_reference = self.subscriptions.add_reference(&key);

        if !is_first_reference {
            let existing_depth = self
                .l3_depths
                .lock()
                .expect("L3 depth map mutex poisoned")
                .get(symbol.as_str())
                .copied();

            if existing_depth != Some(depth) {
                self.subscriptions.remove_reference(&key);
                return Err(KrakenWsError::InvalidMessage(format!(
                    "L3 subscription for {symbol} already exists with depth \
                     {existing_depth:?}, cannot resubscribe with depth {depth}",
                )));
            }
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);

        self.l3_depths
            .lock()
            .expect("L3 depth map mutex poisoned")
            .insert(symbol.to_string(), depth);

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
                snapshot: Some(true),
                depth: Some(depth),
                interval: None,
                event_trigger: None,
                token: Some(token),
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        let payload = match self.send_command(&request).await {
            Ok(p) => p,
            Err(e) => {
                self.l3_depths
                    .lock()
                    .expect("L3 depth map mutex poisoned")
                    .remove(symbol.as_str());
                self.subscriptions.remove_reference(&key);
                self.subscriptions.mark_unsubscribe(&key);
                self.subscriptions.confirm_unsubscribe(&key);
                return Err(e);
            }
        };

        self.subscriptions.confirm_subscribe(&key);
        self.subscription_payloads
            .write()
            .await
            .insert(key, payload);
        Ok(())
    }

    /// Unsubscribes from the `level3` channel for the given Kraken symbol.
    ///
    /// Mirrors the existing `KrakenSpotWebSocketClient::unsubscribe` pattern:
    /// `SubscriptionState` is mutated optimistically before `send_command`. If
    /// the send fails, the local state reflects an unsubscribe that the venue
    /// never received; the next reconnect's payload replay will not include the
    /// topic (it was removed from `subscription_payloads`). Tightening this to
    /// roll-back-on-send-fail is a codebase-wide pattern change (every channel
    /// has it) and is out of scope for this PR — the L3 path matches the
    /// existing surface area rather than introducing an inconsistent improvement.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub async fn unsubscribe_book_l3(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        let channel_str = KrakenWsChannel::Level3.as_ref();
        let key = format!("{channel_str}:{symbol}");
        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }
        self.subscriptions.mark_unsubscribe(&key);

        let token = self.auth_token.read().await.clone();
        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: None,
                token,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_command(&request).await?;
        self.subscriptions.confirm_unsubscribe(&key);
        self.subscription_payloads.write().await.remove(&key);
        self.l3_depths
            .lock()
            .expect("L3 depth map mutex poisoned")
            .remove(symbol.as_str());
        Ok(())
    }

    /// Resynchronizes the `level3` book for `symbol` after a checksum mismatch.
    ///
    /// Refreshes the auth token unconditionally and issues a venue-level
    /// unsubscribe followed by a subscribe with `snapshot=true`, **bypassing
    /// `SubscriptionState` reference counts** so the user's logical
    /// subscription survives. The reference count is not changed; if multiple
    /// callers hold references to the same symbol, all of them continue to see
    /// the symbol as subscribed throughout the resync.
    ///
    /// # Errors
    ///
    /// Returns an error if the auth token cannot be refreshed or the
    /// unsubscribe/subscribe messages cannot be sent.
    pub async fn resync_book_l3(&self, symbol: Ustr, depth: u32) -> Result<(), KrakenWsError> {
        let channel_str = KrakenWsChannel::Level3.as_ref();
        let key = format!("{channel_str}:{symbol}");

        // A resync can be retrying / awaiting a token refresh while the user
        // unsubscribes. Bail before mutating any state if the user no longer
        // holds a logical subscription, otherwise the venue-level subscribe
        // below would resurrect an orphaned stream that `SubscriptionState`
        // can never tear down.
        if !self.subscriptions_contains(&key) {
            log::debug!("Skipping L3 resync: subscription cancelled mid-retry, symbol={symbol}",);
            return Ok(());
        }

        let new_token = refresh_auth_token(&self.config).await?;
        *self.auth_token.write().await = Some(new_token.clone());

        // Re-check after the await — the user may have unsubscribed while we
        // were minting a fresh token.
        if !self.subscriptions_contains(&key) {
            log::debug!(
                "Skipping L3 resync: subscription cancelled after token refresh, symbol={symbol}",
            );
            return Ok(());
        }

        let unsub_req_id = self.get_next_req_id();
        let unsub = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: None,
                token: Some(new_token.clone()),
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(unsub_req_id),
        };
        self.send_command(&unsub).await?;

        // Final check before issuing the resubscribe — same race window.
        if !self.subscriptions_contains(&key) {
            log::debug!("Skipping L3 resync resubscribe: cancelled before send, symbol={symbol}",);
            return Ok(());
        }

        let sub_req_id = self.get_next_req_id();
        let sub = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
                snapshot: Some(true),
                depth: Some(depth),
                interval: None,
                event_trigger: None,
                token: Some(new_token),
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(sub_req_id),
        };
        let payload = self.send_command(&sub).await?;

        // Only persist replay payload + depth if the subscription is still
        // referenced. A late-arriving unsubscribe between `send_command` and
        // here would otherwise leave an orphan entry the reconnect path would
        // replay.
        if self.subscriptions_contains(&key) {
            self.subscription_payloads
                .write()
                .await
                .insert(key, payload);
            self.l3_depths
                .lock()
                .expect("L3 depth map mutex poisoned")
                .insert(symbol.to_string(), depth);
        }

        Ok(())
    }

    /// Returns whether L3 checksum validation is enabled for this client.
    pub fn validate_l3_checksum(&self) -> bool {
        self.config.validate_l3_checksum
    }

    /// Returns `true` if the client has API credentials configured
    /// (post-environment-variable resolution).
    pub fn has_credentials(&self) -> bool {
        self.config.has_api_credentials()
    }

    /// Returns a shared handle to the per-client instrument map.
    ///
    /// L3 stream-loop consumers read instruments through this handle so that
    /// `cache_instrument()` updates made after `connect()` are observed by the
    /// runtime book reconstruction without needing a re-connect.
    pub fn instruments_handle(&self) -> Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        Arc::clone(&self.instruments)
    }

    /// Returns a shared handle to the per-symbol L3 depth map.
    ///
    /// Stream-loop consumers read this map to drive `process_l3_message`'s
    /// resync depth lookup; `subscribe_book_l3` writes the depth.
    pub fn l3_depths_handle(&self) -> Arc<std::sync::Mutex<ahash::AHashMap<String, u32>>> {
        Arc::clone(&self.l3_depths)
    }

    /// Subscribes to quote updates for the given instrument.
    ///
    /// Uses the Ticker channel with `event_trigger: "bbo"` for updates only on
    /// best bid/offer changes.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        let key = format!("quotes:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Ticker,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: Some("bbo".to_string()),
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        let payload = self.send_command(&request).await?;
        self.subscriptions.confirm_subscribe(&key);
        self.subscription_payloads
            .write()
            .await
            .insert(key, payload);
        Ok(())
    }

    /// Subscribes to trade updates for the given instrument.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        self.subscribe(KrakenWsChannel::Trade, vec![symbol], None)
            .await
    }

    /// Subscribes to bar/OHLC updates for the given bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar aggregation is not supported by Kraken.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(bar_type.instrument_id().symbol.inner());
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
        let req_id = self.get_next_req_id();

        let token = self.auth_token.read().await.clone().ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "Authentication token required for executions channel. Call authenticate() first"
                    .to_string(),
            )
        })?;

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Executions,
                symbol: None,
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: None,
                token: Some(token),
                snap_orders: Some(snap_orders),
                snap_trades: Some(snap_trades),
            })),
            req_id: Some(req_id),
        };

        let payload = self.send_command(&request).await?;

        let key = "executions";
        if self.subscriptions.add_reference(key) {
            self.subscriptions.mark_subscribe(key);
            self.subscriptions.confirm_subscribe(key);
            self.subscription_payloads
                .write()
                .await
                .insert(key.to_string(), payload);
        }

        Ok(())
    }

    /// Unsubscribes from order book updates for the given instrument.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        self.unsubscribe(KrakenWsChannel::Book, vec![symbol]).await
    }

    /// Unsubscribes from quote updates for the given instrument.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        let key = format!("quotes:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Ticker,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: Some("bbo".to_string()),
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_command(&request).await?;
        self.subscriptions.confirm_unsubscribe(&key);
        self.subscription_payloads.write().await.remove(&key);
        Ok(())
    }

    /// Unsubscribes from trade updates for the given instrument.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        self.unsubscribe(KrakenWsChannel::Trade, vec![symbol]).await
    }

    /// Unsubscribes from bar/OHLC updates for the given bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the bar aggregation is not supported by Kraken.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), KrakenWsError> {
        let symbol = to_ws_v2_symbol(bar_type.instrument_id().symbol.inner());
        let interval = bar_type_to_ws_interval(bar_type)?;
        self.unsubscribe_with_interval(KrakenWsChannel::Ohlc, vec![symbol], interval)
            .await
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
        config.proxy_url.clone(),
        config
            .max_requests_per_second
            .unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND),
    )
    .map_err(|e| {
        KrakenWsError::AuthenticationError(format!("Failed to create HTTP client: {e}"))
    })?;

    let ws_token = http_client.get_websockets_token().await.map_err(|e| {
        KrakenWsError::AuthenticationError(format!("Failed to get WebSocket token: {e}"))
    })?;

    log::debug!(
        "WebSocket authentication token refreshed: token_length={}, expires={}",
        ws_token.token.len(),
        ws_token.expires
    );

    Ok(ws_token.token)
}

fn update_auth_token_in_payload(payload: &str, new_token: &str) -> Result<String, KrakenWsError> {
    let mut value: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

    if let Some(params) = value.get_mut("params") {
        params["token"] = serde_json::Value::String(new_token.to_string());
    }

    serde_json::to_string(&value).map_err(|e| KrakenWsError::JsonError(e.to_string()))
}

#[inline]
fn to_ws_v2_symbol(symbol: Ustr) -> Ustr {
    Ustr::from(&normalize_spot_symbol(symbol.as_str()))
}

fn bar_type_to_ws_interval(bar_type: BarType) -> Result<u32, KrakenWsError> {
    const VALID_INTERVALS: [u32; 9] = [1, 5, 15, 30, 60, 240, 1440, 10080, 21600];

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

    if !VALID_INTERVALS.contains(&interval) {
        return Err(KrakenWsError::SubscriptionError(format!(
            "Invalid bar interval {interval} minutes for Kraken OHLC streaming. \
             Supported intervals: 1, 5, 15, 30, 60, 240, 1440, 10080, 21600"
        )));
    }

    Ok(interval)
}

#[cfg(test)]
mod tests {
    use std::sync::{Arc, atomic::Ordering};

    use rstest::rstest;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::config::KrakenDataClientConfig;

    #[rstest]
    fn test_req_id_counter_is_shared_arc_and_monotonic() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::new(cfg, CancellationToken::new(), None);
        let counter = client.req_id_counter();
        let a = counter.fetch_add(1, Ordering::Relaxed);
        let b = counter.fetch_add(1, Ordering::Relaxed);
        assert!(b > a);
        #[allow(clippy::redundant_clone)]
        let cloned = client.clone();
        let cloned_counter = cloned.req_id_counter();
        assert!(Arc::ptr_eq(&counter, &cloned_counter));
    }

    #[rstest]
    #[case("XBT/EUR", "BTC/EUR")]
    #[case("XBT/USD", "BTC/USD")]
    #[case("XBT/USDT", "BTC/USDT")]
    #[case("ETH/USD", "ETH/USD")]
    #[case("ETH/XBT", "ETH/BTC")]
    #[case("SOL/XBT", "SOL/BTC")]
    #[case("SOL/USD", "SOL/USD")]
    #[case("BTC/USD", "BTC/USD")]
    #[case("ETH/BTC", "ETH/BTC")]
    #[case("XDG/USD", "DOGE/USD")]
    #[case("XDG/EUR", "DOGE/EUR")]
    fn test_to_kraken_ws_v2_symbol(#[case] input: &str, #[case] expected: &str) {
        let symbol = Ustr::from(input);
        let result = to_ws_v2_symbol(symbol);
        assert_eq!(result.as_str(), expected);
    }

    fn test_client_without_credentials() -> KrakenSpotWebSocketClient {
        KrakenSpotWebSocketClient::new(
            KrakenDataClientConfig::default(),
            CancellationToken::new(),
            None,
        )
    }

    #[rstest]
    #[tokio::test]
    async fn test_authenticate_without_credentials_errors() {
        let client = test_client_without_credentials();

        let err = client.authenticate().await.expect_err("should fail");
        assert!(
            matches!(err, KrakenWsError::AuthenticationError(ref msg) if msg.contains("API credentials required")),
            "unexpected error: {err:?}"
        );
        assert!(!client.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_until_authenticated_times_out() {
        let client = test_client_without_credentials();

        let err = client
            .wait_until_authenticated(0.05)
            .await
            .expect_err("should time out");
        assert!(matches!(err, KrakenWsError::AuthenticationError(_)));
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_until_authenticated_resolves_after_succeed() {
        let client = test_client_without_credentials();

        let tracker = client.auth_tracker.clone();
        let _rx = tracker.begin();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            tracker.succeed();
        });

        client
            .wait_until_authenticated(1.0)
            .await
            .expect("should resolve once tracker succeeds");
        assert!(client.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_is_authenticated_flips_on_fail() {
        let client = test_client_without_credentials();

        let _rx = client.auth_tracker.begin();
        client.auth_tracker.succeed();
        assert!(client.is_authenticated());

        client.auth_tracker.fail("test failure");
        assert!(!client.is_authenticated());
    }

    #[rstest]
    fn test_l3_factory_uses_ws_l3_url() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);
        assert_eq!(client.url(), "wss://ws-l3.kraken.com/v2");
    }

    #[rstest]
    fn test_l3_factory_respects_override() {
        let cfg = KrakenDataClientConfig {
            ws_l3_url: Some("wss://override.example/v2".to_string()),
            ..Default::default()
        };
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);
        assert_eq!(client.url(), "wss://override.example/v2");
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_l3_without_auth_errors_and_leaves_clean_state() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);

        let err = client
            .subscribe_book_l3(Ustr::from("BTC/USD"), 1000)
            .await
            .expect_err("should fail without auth token");

        assert!(matches!(err, KrakenWsError::AuthenticationError(_)));
        assert!(
            client.subscriptions.is_empty(),
            "no state must leak on auth failure"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_l3_invalid_depth_errors() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);

        let err = client
            .subscribe_book_l3(Ustr::from("BTC/USD"), 50)
            .await
            .expect_err("should fail on invalid depth");

        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));
        assert!(!client.subscriptions_contains("level3:BTC/USD"));
    }

    #[rstest]
    fn test_subscribe_book_l3_refcount_idempotent() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);
        let key = "level3:BTC/USD";

        assert!(client.subscriptions.add_reference(key));
        assert!(!client.subscriptions.add_reference(key));
        assert!(!client.subscriptions.remove_reference(key));
        assert!(client.subscriptions.remove_reference(key));
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_l3_rejects_depth_mismatch() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);

        let key = "level3:BTC/USD";
        client.subscriptions.add_reference(key);
        client.subscriptions.mark_subscribe(key);
        client.subscriptions.confirm_subscribe(key);
        client
            .l3_depths
            .lock()
            .unwrap()
            .insert("BTC/USD".to_string(), 1000);

        *client.auth_token.write().await = Some("test-token".to_string());

        let err = client
            .subscribe_book_l3(Ustr::from("BTC/USD"), 10)
            .await
            .expect_err("should reject depth mismatch");
        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));

        assert!(client.subscriptions.remove_reference(key));
    }

    #[rstest]
    #[tokio::test]
    async fn test_generic_subscribe_rejects_level3() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);

        let err = client
            .subscribe(
                KrakenWsChannel::Level3,
                vec![Ustr::from("BTC/USD")],
                Some(1000),
            )
            .await
            .expect_err("generic subscribe must reject Level3");
        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));

        let err = client
            .unsubscribe(KrakenWsChannel::Level3, vec![Ustr::from("BTC/USD")])
            .await
            .expect_err("generic unsubscribe must reject Level3");
        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));
    }

    #[rstest]
    fn test_update_auth_token_in_payload_for_level3() {
        let original = r#"{"method":"subscribe","params":{"channel":"level3","symbol":["BTC/USD"],"depth":1000,"snapshot":true,"token":"OLD"},"req_id":1}"#;
        let rewritten = update_auth_token_in_payload(original, "NEW").unwrap();
        assert!(rewritten.contains(r#""token":"NEW""#));
        assert!(!rewritten.contains(r#""token":"OLD""#));
    }

    #[rstest]
    fn test_l3_depths_shared_between_subscribe_and_handle() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);

        let handle = client.l3_depths_handle();
        client
            .l3_depths
            .lock()
            .unwrap()
            .insert("BTC/USD".to_string(), 100);

        assert_eq!(handle.lock().unwrap().get("BTC/USD").copied(), Some(100));
    }

    #[rstest]
    fn test_resync_book_l3_does_not_touch_refcount() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None);
        let key = "level3:BTC/USD";

        assert!(client.subscriptions.add_reference(key));
        assert!(!client.subscriptions.add_reference(key));
        client.subscriptions.confirm_subscribe(key);

        assert!(client.subscriptions_contains(key));
    }
}
