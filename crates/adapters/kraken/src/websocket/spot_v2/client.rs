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
        Arc,
        atomic::{AtomicBool, AtomicU8, AtomicU64, Ordering},
    },
};

use arc_swap::ArcSwap;
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::{AtomicMap, string::secret::SecretString};
use nautilus_live::{
    SocketControl,
    book::recovery::BookRecovery,
    task::{TaskGroup, TaskShutdownError},
};
use nautilus_model::{
    data::BarType,
    enums::BarAggregation,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    ConnectionUrl,
    http::create_standard_nautilus_headers,
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_epoch_message_handler,
    },
};
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

/// Topic delimiter for Kraken Spot v2 WebSocket subscriptions.
///
/// Topics use colon format: `channel:symbol` (e.g., `Trade:ETH/USD`).
pub const KRAKEN_SPOT_WS_TOPIC_DELIMITER: char = ':';

use super::{
    enums::{KrakenWsChannel, KrakenWsMethod},
    handler::{SpotBookOperation, SpotBookWrite, SpotFeedHandler, SpotHandlerCommand},
    level_2::{BookRequest, BookRequests, L2Depths},
    messages::{KrakenSpotWsMessage, KrakenWsChannelParams, KrakenWsParams, KrakenWsRequest},
};
use crate::{
    common::{
        consts::{
            KRAKEN_RATE_LIMIT_KEY_ORDER, KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION,
            KRAKEN_SPOT_WS_ORDER_QUOTA, KRAKEN_SPOT_WS_SUBSCRIPTION_QUOTA,
        },
        parse::normalize_spot_symbol,
    },
    config::KrakenDataClientConfig,
    http::{KrakenSpotHttpClient, spot::client::KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND},
    websocket::error::KrakenWsError,
};

const WS_PING_MSG: &str = r#"{"method":"ping"}"#;
type BookWriteResponse = tokio::sync::oneshot::Receiver<Result<(), KrakenWsError>>;

/// WebSocket client for the Kraken Spot v2 streaming API.
#[derive(Debug)]
pub struct KrakenSpotWebSocketClient {
    url: ConnectionUrl,
    config: KrakenDataClientConfig,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>>>,
    book_writer: Arc<RwLock<Option<SpotBookWriter>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<KrakenSpotWsMessage>>>,
    handler_tasks: Arc<TaskGroup>,
    connect_lock: Arc<tokio::sync::Mutex<()>>,
    subscriptions: SubscriptionState,
    subscription_payloads: Arc<tokio::sync::RwLock<HashMap<String, SecretString>>>,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    req_id_counter: Arc<AtomicU64>,
    auth_token: Arc<tokio::sync::RwLock<Option<SecretString>>>,
    account_id: Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    l2_depths: L2Depths,
    /// Serializes book subscription changes and replacements, so a recovery's check
    /// of the live subscription and its two commands cannot interleave with a replacement.
    book_commands: Arc<tokio::sync::Mutex<()>>,
    /// Every `book` subscribe sent and not yet answered, by request id, so the data client can
    /// match the venue's answer to the subscription behind it.
    book_requests: BookRequests,
    l3_subscriptions: L2Depths,
    l3_depths: Arc<parking_lot::Mutex<ahash::AHashMap<String, u32>>>,
    transport_backend: TransportBackend,
    proxy_url: Option<SecretString>,
    socket_control: Option<SocketControl>,
}

#[derive(Clone, Debug)]
struct SpotBookWriter {
    tx: tokio::sync::mpsc::UnboundedSender<SpotBookWrite>,
    epoch: Arc<AtomicU64>,
    transport_epoch: Arc<AtomicU64>,
}

impl SpotBookWriter {
    fn queue(&self, operation: SpotBookOperation) -> Result<u64, KrakenWsError> {
        let epoch = self.epoch.load(Ordering::Acquire);
        self.tx
            .send(SpotBookWrite { epoch, operation })
            .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;
        Ok(epoch)
    }
}

impl Clone for KrakenSpotWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            config: self.config.clone(),
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            book_writer: Arc::clone(&self.book_writer),
            out_rx: self.out_rx.clone(),
            handler_tasks: Arc::clone(&self.handler_tasks),
            connect_lock: Arc::clone(&self.connect_lock),
            subscriptions: self.subscriptions.clone(),
            subscription_payloads: Arc::clone(&self.subscription_payloads),
            auth_tracker: self.auth_tracker.clone(),
            cancellation_token: self.cancellation_token.clone(),
            req_id_counter: self.req_id_counter.clone(),
            auth_token: self.auth_token.clone(),
            account_id: Arc::clone(&self.account_id),
            truncated_id_map: Arc::clone(&self.truncated_id_map),
            instruments: Arc::clone(&self.instruments),
            l2_depths: self.l2_depths.clone(),
            book_commands: Arc::clone(&self.book_commands),
            book_requests: Arc::clone(&self.book_requests),
            l3_subscriptions: self.l3_subscriptions.clone(),
            l3_depths: Arc::clone(&self.l3_depths),
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
            socket_control: self.socket_control.clone(),
        }
    }
}

impl KrakenSpotWebSocketClient {
    /// Creates a new client for the configured public/private endpoint.
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub fn new(
        config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        config.validate()?;
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
    ///
    /// # Errors
    ///
    /// Returns an error if the configuration is invalid.
    pub fn l3(
        config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let url = config.ws_l3_url();
        Self::new_with_url(url, config, cancellation_token, proxy_url)
    }

    fn new_with_url(
        url: String,
        mut config: KrakenDataClientConfig,
        cancellation_token: CancellationToken,
        proxy_url: Option<String>,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        let transport_backend = config.transport_backend;
        config.proxy_url = proxy_url.clone().map(SecretString::from);

        Ok(Self {
            url: url.into(),
            config,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            book_writer: Arc::new(RwLock::new(None)),
            out_rx: None,
            handler_tasks: Arc::new(TaskGroup::new()),
            connect_lock: Arc::new(tokio::sync::Mutex::new(())),
            subscriptions: SubscriptionState::new(KRAKEN_SPOT_WS_TOPIC_DELIMITER),
            subscription_payloads: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            auth_tracker: AuthTracker::new(),
            cancellation_token,
            req_id_counter: Arc::new(AtomicU64::new(0)),
            auth_token: Arc::new(tokio::sync::RwLock::new(None)),
            account_id: Arc::new(RwLock::new(None)),
            truncated_id_map: Arc::new(AtomicMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            l2_depths: L2Depths::default(),
            book_commands: Arc::new(tokio::sync::Mutex::new(())),
            book_requests: Arc::new(parking_lot::Mutex::new(ahash::AHashMap::new())),
            l3_subscriptions: L2Depths::default(),
            l3_depths: Arc::new(parking_lot::Mutex::new(ahash::AHashMap::new())),
            transport_backend,
            proxy_url: proxy_url.map(SecretString::from),
            socket_control: None,
        })
    }

    pub(crate) fn begin_shutdown(&self) {
        self.handler_tasks.begin_shutdown();
        self.cancellation_token.cancel();
        self.signal.store(true, Ordering::Relaxed);
    }

    /// Configures socket state reporting and reconnect control.
    #[must_use]
    pub fn with_socket_control(mut self, control: SocketControl) -> Self {
        self.socket_control = Some(control);
        self
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
    pub async fn auth_token(&self) -> Option<SecretString> {
        self.auth_token.read().await.clone()
    }

    /// Returns the current cached authentication token without awaiting.
    ///
    /// Returns `None` when the lock is contended or no token is cached. Used by
    /// the synchronous order-routing path where the auth token is normally
    /// uncontended; callers fall back to REST when the lock is unavailable.
    pub fn auth_token_blocking(&self) -> Option<SecretString> {
        self.auth_token
            .try_read()
            .ok()
            .and_then(|guard| guard.clone())
    }

    /// Returns the shared auth token handle for internal components that need
    /// non-async access, such as timeout-triggered compensating cancels.
    pub(crate) fn auth_token_handle(&self) -> Arc<tokio::sync::RwLock<Option<SecretString>>> {
        self.auth_token.clone()
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;

        log::debug!("Connecting to {}", self.url.origin());

        if !self.handler_tasks.is_open() || !self.handler_tasks.is_empty() {
            self.disconnect_locked().await?;
            self.handler_tasks.start_generation().map_err(|e| {
                KrakenWsError::ConnectionError(format!(
                    "Failed to start WebSocket handler task generation: {e}"
                ))
            })?;
        }
        let handler_spawner = self.handler_tasks.spawner().map_err(|e| {
            KrakenWsError::ConnectionError(format!(
                "Failed to acquire WebSocket handler task spawner: {e}"
            ))
        })?;

        if self.cancellation_token.is_cancelled() {
            self.cancellation_token = CancellationToken::new();
        }

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_epoch_message_handler();
        let headers = create_standard_nautilus_headers();

        let ws_config = WebSocketConfig {
            url: self.url.expose().to_owned(),
            headers,
            heartbeat_interval_secs: Some(self.config.heartbeat_interval_secs),
            heartbeat_payload: Some(WS_PING_MSG.to_string()),
            connect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            // Treat a silent connection as dead so the reconnect + resubscribe
            // path runs. `0` disables; see `ws_idle_timeout_ms` docs (issue #4255).
            heartbeat_timeout_secs: None,
            idle_timeout_ms: (self.config.ws_idle_timeout_ms != 0)
                .then_some(self.config.ws_idle_timeout_ms),
            writer_capacity: None,
            backend: self.transport_backend,
            proxy_url: self
                .proxy_url
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            max_message_size_bytes: None,
            max_frame_size_bytes: None,
        };

        let keyed_quotas = vec![
            (
                KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION[0].to_string(),
                *KRAKEN_SPOT_WS_SUBSCRIPTION_QUOTA,
            ),
            (
                KRAKEN_RATE_LIMIT_KEY_ORDER[0].to_string(),
                *KRAKEN_SPOT_WS_ORDER_QUOTA,
            ),
        ];

        let ws_client = WebSocketClient::epoch_builder()
            .config(ws_config)
            .epoch_handler(raw_handler)
            .keyed_quotas(keyed_quotas)
            .maybe_state_sink(self.socket_control.as_ref().map(SocketControl::sink))
            .connect()
            .await
            .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        let connection_mode = ws_client.connection_mode_atomic();
        let transport_epoch = ws_client.connection_epoch_atomic();
        let reconnect_handle = ws_client.reconnect_handle();

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<KrakenSpotWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<SpotHandlerCommand>();
        let (book_tx, book_rx) = tokio::sync::mpsc::unbounded_channel();

        let book_writer = SpotBookWriter {
            tx: book_tx,
            epoch: Arc::new(AtomicU64::new(0)),
            transport_epoch,
        };

        if let Err(e) = cmd_tx.send(SpotHandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        {
            let mut command_sender = self.cmd_tx.write().await;
            *command_sender = cmd_tx.clone();
            *self.book_writer.write() = Some(book_writer.clone());
            self.connection_mode.store(connection_mode);
        }

        if let Some(control) = &self.socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        let subscriptions = self.subscriptions.clone();
        let signal = self.signal.clone();

        let replay = SpotReplay::new(self, book_writer);

        let handler_task = async move {
            let mut handler = SpotFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions);
            handler.manage_book_writes(book_rx, Arc::clone(&replay.book_requests));
            replay.run_feed(handler, signal, cmd_tx, out_tx).await;
        };

        if let Err(e) = handler_spawner.spawn(handler_task) {
            if let Some(control) = &self.socket_control {
                control.deregister();
            }
            self.out_rx = None;
            *self.book_writer.write() = None;
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to register WebSocket handler task: {e}"
            )));
        }

        log::debug!("WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;
        self.disconnect_locked().await
    }

    async fn disconnect_locked(&self) -> Result<(), KrakenWsError> {
        log::debug!("Disconnecting WebSocket");

        self.handler_tasks.begin_shutdown();
        self.signal.store(true, Ordering::Relaxed);
        *self.book_writer.write() = None;

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

        let task_result = self
            .handler_tasks
            .finish_shutdown(
                tokio::time::Duration::from_secs(2),
                tokio::time::Duration::from_secs(2),
            )
            .await;

        self.subscriptions.clear();
        self.subscription_payloads.write().await.clear();
        self.auth_tracker.fail("Disconnected");

        self.l3_depths.lock().clear();
        self.l3_subscriptions.clear();
        self.l2_depths.clear();
        self.book_requests.lock().clear();

        if let Some(control) = &self.socket_control {
            control.deregister();
        }

        match task_result {
            Ok(()) => Ok(()),
            Err(error @ TaskShutdownError::Timeout { .. }) => Err(KrakenWsError::Timeout(format!(
                "Spot WebSocket handler shutdown timed out: {error}"
            ))),
            Err(e) => Err(KrakenWsError::Disconnected(format!(
                "Spot WebSocket handler shutdown failed: {e}"
            ))),
        }
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
        if channel == KrakenWsChannel::Book {
            return self.subscribe_book_symbols(&symbols, depth).await;
        }

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
            Some(self.auth_token().await.ok_or_else(|| {
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
        if channel == KrakenWsChannel::Book {
            for symbol in symbols {
                self.unsubscribe_book_symbol(symbol).await?;
            }

            return Ok(());
        }

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
            Some(self.auth_token().await.ok_or_else(|| {
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

    async fn send_command(&self, request: &KrakenWsRequest) -> Result<SecretString, KrakenWsError> {
        let payload = SecretString::from(
            serde_json::to_string(request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?,
        );

        log::trace!(
            "Sending WebSocket request: method={:?} ({} bytes)",
            request.method,
            payload.expose_secret().len(),
        );

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
        self.url.expose()
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
        *self.account_id.write() = Some(account_id);
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        *self.account_id.read()
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
    /// - The stream receiver has already been taken.
    /// - Other clones of this client still hold references to the receiver.
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
        self.subscribe_book_symbols(&[to_ws_v2_symbol(instrument_id.symbol.inner())], depth)
            .await
    }

    async fn subscribe_book_symbols(
        &self,
        symbols: &[Ustr],
        depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        if symbols.is_empty() {
            return Ok(());
        }

        let _book_commands = self.book_commands.lock().await;
        let depth = depth.unwrap_or(10);

        if !matches!(depth, 10 | 25 | 100 | 500 | 1000) {
            return Err(KrakenWsError::InvalidMessage(format!(
                "Invalid L2 depth {depth}, valid values: 10, 25, 100, 500, 1000",
            )));
        }

        for symbol in symbols {
            if let Some(existing) = self.l2_depths.get(symbol.as_str())
                && existing != depth
            {
                return Err(KrakenWsError::InvalidMessage(format!(
                    "L2 subscription for {symbol} already exists with depth {existing}, cannot resubscribe with depth {depth}"
                )));
            }
        }

        for (index, &symbol) in symbols.iter().enumerate() {
            if let Err(e) = self.subscribe_book_symbol(symbol, depth).await {
                for &acquired in symbols[..index].iter().rev() {
                    let _ = self.remove_book_reference(acquired).await;
                }

                return Err(e);
            }
        }

        Ok(())
    }

    async fn subscribe_book_symbol(&self, symbol: Ustr, depth: u32) -> Result<(), KrakenWsError> {
        let channel_str = KrakenWsChannel::Book.as_ref();
        let key = format!("{channel_str}:{symbol}");
        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        let req_id = self.get_next_req_id();
        self.l2_depths.insert(symbol.as_str(), depth, req_id);

        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Book,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth: Some(depth),
                interval: None,
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };
        self.record_book_request(req_id, symbol);

        let payload = match self.send_book_command(&request) {
            Ok(payload) => payload,
            Err(e) => {
                self.book_requests.lock().remove(&req_id);
                self.l2_depths.remove(symbol.as_str());
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

    /// Subscribes to the `level3` channel for the given Kraken symbol.
    ///
    /// `depth` must be one of `10`, `100`, or `1000`. The depth is recorded in
    /// the per-client `l3_depths` map so the message handler and `resync_book_l3`
    /// can recover it without a separate side-table.
    ///
    /// If the symbol is already subscribed, the existing depth must match - Kraken
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
        self.subscribe_book_l3_guarded(symbol, depth, None)
            .await
            .map(|_| ())
    }

    pub(crate) async fn subscribe_book_l3_guarded(
        &self,
        symbol: Ustr,
        depth: u32,
        cancellation: Option<&CancellationToken>,
    ) -> Result<bool, KrakenWsError> {
        let _book_commands = self.book_commands.lock().await;

        if !matches!(depth, 10 | 100 | 1000) {
            return Err(KrakenWsError::InvalidMessage(format!(
                "Invalid L3 depth {depth}, valid values: 10, 100, 1000",
            )));
        }

        let token = self.auth_token().await.ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "Authentication token required for level3. Call authenticate() first".to_string(),
            )
        })?;

        if cancellation.is_some_and(CancellationToken::is_cancelled) {
            return Ok(false);
        }

        let channel_str = KrakenWsChannel::Level3.as_ref();
        let key = format!("{channel_str}:{symbol}");

        let is_first_reference = self.subscriptions.add_reference(&key);

        if !is_first_reference {
            let existing_depth = self.l3_depths.lock().get(symbol.as_str()).copied();

            if existing_depth != Some(depth) {
                self.subscriptions.remove_reference(&key);
                return Err(KrakenWsError::InvalidMessage(format!(
                    "L3 subscription for {symbol} already exists with depth \
                     {existing_depth:?}, cannot resubscribe with depth {depth}",
                )));
            }

            return Ok(true);
        }

        self.subscriptions.mark_subscribe(&key);

        self.l3_depths.lock().insert(symbol.to_string(), depth);

        let req_id = self.get_next_req_id();
        self.l3_subscriptions.insert(symbol.as_str(), depth, req_id);
        self.record_book_request(req_id, symbol);

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

        let payload = match self.send_book_command(&request) {
            Ok(p) => p,
            Err(e) => {
                self.book_requests.lock().remove(&req_id);
                self.l3_subscriptions.remove(symbol.as_str());
                self.l3_depths.lock().remove(symbol.as_str());
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
        Ok(true)
    }

    /// Unsubscribes from the `level3` channel for the given Kraken symbol.
    ///
    /// Mirrors the existing `KrakenSpotWebSocketClient::unsubscribe` pattern:
    /// `SubscriptionState` is mutated optimistically before `send_command`. If
    /// the send fails, the local state reflects an unsubscribe that the venue
    /// never received; the next reconnect's payload replay will not include the
    /// topic (it was removed from `subscription_payloads`). Tightening this to
    /// roll-back-on-send-fail is a codebase-wide pattern change (every channel
    /// has it) and is out of scope for this PR - the L3 path matches the
    /// existing surface area rather than introducing an inconsistent improvement.
    ///
    /// # Errors
    ///
    /// Returns an error if the message cannot be sent.
    pub async fn unsubscribe_book_l3(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        let _book_commands = self.book_commands.lock().await;
        let channel_str = KrakenWsChannel::Level3.as_ref();
        let key = format!("{channel_str}:{symbol}");
        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }
        self.subscriptions.mark_unsubscribe(&key);

        self.l3_subscriptions.remove(symbol.as_str());
        self.book_requests
            .lock()
            .retain(|_, held| held.symbol != symbol);
        let depth = self.l3_depths.lock().remove(symbol.as_str());
        self.subscription_payloads.write().await.remove(&key);

        let token = self.auth_token().await;
        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
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

        self.send_book_command(&request)?;
        self.subscriptions.confirm_unsubscribe(&key);
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
        let Some(subscription) = self.l3_subscriptions.subscription(symbol.as_str()) else {
            return Ok(());
        };

        if subscription.depth != depth {
            return Err(KrakenWsError::InvalidMessage(format!(
                "Book depth changed for {symbol}"
            )));
        }

        self.replace_l3_book(symbol, subscription.generation, None)
            .await
    }

    pub(crate) async fn recover_l3_book(
        &self,
        symbol: Ustr,
        generation: u64,
        cancel: CancellationToken,
    ) -> Result<(), KrakenWsError> {
        self.replace_l3_book(symbol, generation, Some(cancel)).await
    }

    async fn replace_l3_book(
        &self,
        symbol: Ustr,
        generation: u64,
        recovery: Option<CancellationToken>,
    ) -> Result<(), KrakenWsError> {
        if !self
            .l3_subscriptions
            .subscription(symbol.as_str())
            .is_some_and(|sub| sub.generation == generation)
        {
            return Ok(());
        }

        let token = refresh_auth_token(&self.config).await?;
        let book_commands = self.book_commands.lock().await;

        let Some(subscription) = self
            .l3_subscriptions
            .subscription(symbol.as_str())
            .filter(|sub| sub.generation == generation)
        else {
            return Ok(());
        };

        *self.auth_token.write().await = Some(token.clone());
        self.auth_tracker.succeed();

        let req_id = self.get_next_req_id();

        let current = match &recovery {
            Some(cancel) => self.l3_subscriptions.begin_recovery(
                symbol,
                generation,
                subscription.snapshot_epoch,
                req_id,
                cancel,
            ),
            None => self.l3_subscriptions.begin_resync(
                symbol.as_str(),
                generation,
                subscription.snapshot_epoch,
                req_id,
            ),
        };

        let Some(subscription) = current else {
            return Ok(());
        };

        let depth = Some(subscription.depth);

        let mut request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Level3,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth,
                interval: None,
                event_trigger: None,
                token: Some(token),
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(self.get_next_req_id()),
        };

        let unsubscribe = request.clone();
        request.method = KrakenWsMethod::Subscribe;
        request.req_id = Some(req_id);
        if let Some(KrakenWsParams::Channel(params)) = &mut request.params {
            params.snapshot = Some(true);
        }

        self.send_book_replacement(
            book_commands,
            [unsubscribe, request],
            symbol,
            req_id,
            KrakenWsChannel::Level3,
            recovery,
        )
        .await
    }

    /// Returns whether L2 `book` checksum validation is enabled for this client.
    pub fn validate_l2_checksum(&self) -> bool {
        self.config.validate_l2_checksum
    }

    /// Resubscribes the `book` channel for `instrument_id` to recover its stream.
    ///
    /// The logical subscription is kept throughout, so the reconnect path still replays it.
    /// `generation` names the subscription the request belongs to and `epoch` the symbol's
    /// snapshot epoch when the request was issued. The check that this subscription is still the
    /// live one and the two commands form one step under the book command lock, which
    /// `subscribe_book` and `unsubscribe_book` also take, so a replacement or a cancel cannot land
    /// between them: a recovery for a replaced or canceled subscription sends nothing, and one that
    /// is admitted resubscribes at the live depth with an explicit snapshot, which the recovery
    /// depends on to end the wait.
    ///
    /// The epoch check and the recording of the resubscribe as the symbol's latest request are one
    /// compare-and-swap against the data client accepting a snapshot, so a late snapshot either
    /// serves the request, which then sends nothing, or is dropped as a retired stream's. The
    /// latest request stays recorded when a send fails so frames from the prior stream stay
    /// suppressed.
    ///
    /// # Errors
    ///
    /// Returns an error if either command fails to send.
    pub async fn resync_book(
        &self,
        instrument_id: InstrumentId,
        generation: u64,
        epoch: u64,
    ) -> Result<(), KrakenWsError> {
        self.replace_l2_book(instrument_id, generation, Some(epoch), None)
            .await
    }

    pub(crate) async fn recover_l2_book(
        &self,
        instrument_id: InstrumentId,
        generation: u64,
        cancel: CancellationToken,
    ) -> Result<(), KrakenWsError> {
        self.replace_l2_book(instrument_id, generation, None, Some(cancel))
            .await
    }

    async fn replace_l2_book(
        &self,
        instrument_id: InstrumentId,
        generation: u64,
        epoch: Option<u64>,
        recovery: Option<CancellationToken>,
    ) -> Result<(), KrakenWsError> {
        let book_commands = self.book_commands.lock().await;
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        let channel_str = KrakenWsChannel::Book.as_ref();
        let key = format!("{channel_str}:{symbol}");
        let req_id = self.get_next_req_id();

        let epoch = epoch.or_else(|| {
            self.l2_depths
                .subscription(symbol.as_str())
                .map(|sub| sub.snapshot_epoch)
        });

        let live = self
            .subscriptions_contains(&key)
            .then(|| {
                epoch.and_then(|epoch| match &recovery {
                    Some(cancel) => self
                        .l2_depths
                        .begin_recovery(symbol, generation, epoch, req_id, cancel),
                    None => self
                        .l2_depths
                        .begin_resync(symbol.as_str(), generation, epoch, req_id),
                })
            })
            .flatten();

        let Some(live) = live else {
            // Nothing is withheld here, so the skip is not a warning: a canceled subscription has
            // no holder to serve, a replacement has a snapshot of its own on the way, a snapshot
            // accepted since the request has served it, and a reconnect replays the stream.
            log::debug!(
                "Skipping L2 resync: subscription canceled or replaced, or its snapshot served \
                 since, symbol={symbol}, generation={generation}, epoch={epoch:?}"
            );
            return Ok(());
        };
        let depth = live.depth;

        let unsub = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Book,
                symbol: Some(vec![symbol]),
                snapshot: None,
                // The venue keys a `book` subscription by symbol and depth, and takes an
                // unsubscribe without a depth as depth 10.
                depth: Some(depth),
                interval: None,
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(self.get_next_req_id()),
        };

        let sub = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Book,
                symbol: Some(vec![symbol]),
                // Asked for explicitly: the recovery depends on this snapshot to end the wait.
                snapshot: Some(true),
                depth: Some(depth),
                interval: None,
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_book_replacement(
            book_commands,
            [unsub, sub],
            symbol,
            req_id,
            KrakenWsChannel::Book,
            recovery,
        )
        .await
    }

    async fn send_book_replacement(
        &self,
        book_commands: tokio::sync::MutexGuard<'_, ()>,
        [unsubscribe, subscribe]: [KrakenWsRequest; 2],
        symbol: Ustr,
        req_id: u64,
        channel: KrakenWsChannel,
        recovery: Option<CancellationToken>,
    ) -> Result<(), KrakenWsError> {
        let request_cancel = self.record_book_request(req_id, symbol);
        let key = format!("{}:{symbol}", channel.as_ref());

        let payload = SecretString::from(serde_json::to_string(&subscribe)?);
        let unsubscribe = SecretString::from(serde_json::to_string(&unsubscribe)?);

        let (result, response) = match &recovery {
            Some(_) => {
                let (result, response) = tokio::sync::oneshot::channel();
                (Some(result), Some(response))
            }
            None => (None, None),
        };

        let epoch = self
            .queue_book(SpotBookOperation::Replace {
                req_id,
                payloads: [unsubscribe, payload.clone()],
                cancel: recovery.clone().unwrap_or(request_cancel),
                result,
            })
            .inspect_err(|_| {
                self.book_requests.lock().remove(&req_id);
            })?;

        self.subscription_payloads
            .write()
            .await
            .insert(key, payload);
        drop(book_commands);

        if let (Some(cancel), Some(response)) = (recovery, response) {
            self.await_book_recovery(req_id, cancel, response, epoch)
                .await
        } else {
            Ok(())
        }
    }

    fn send_book_command(&self, request: &KrakenWsRequest) -> Result<SecretString, KrakenWsError> {
        let payload = SecretString::from(serde_json::to_string(request)?);

        let cancel = request.req_id.and_then(|req_id| {
            self.book_requests
                .lock()
                .get(&req_id)
                .map(|held| held.cancel.clone())
        });

        self.queue_book(SpotBookOperation::Send(payload.clone(), cancel))?;
        Ok(payload)
    }

    fn queue_book(&self, operation: SpotBookOperation) -> Result<u64, KrakenWsError> {
        self.book_writer
            .read()
            .as_ref()
            .ok_or_else(|| KrakenWsError::ConnectionError("No active Spot book writer".into()))?
            .queue(operation)
    }

    async fn await_book_recovery(
        &self,
        req_id: u64,
        cancel: CancellationToken,
        response: BookWriteResponse,
        epoch: u64,
    ) -> Result<(), KrakenWsError> {
        let sent = tokio::select! {
            biased;
            () = cancel.cancelled() => Err(KrakenWsError::Disconnected("Book recovery cancelled".into())),
            sent = response => sent.unwrap_or_else(|e| Err(KrakenWsError::ConnectionError(e.to_string()))),
        };

        if sent.is_err() {
            self.book_requests.lock().remove(&req_id);

            if !cancel.is_cancelled()
                && self
                    .book_writer
                    .read()
                    .as_ref()
                    .is_some_and(|writer| writer.transport_epoch.load(Ordering::Acquire) != epoch)
            {
                // The current replay acknowledgement can open this attempt's gate.
                return Ok(());
            }
        }

        sent
    }

    fn record_book_request(&self, req_id: u64, symbol: Ustr) -> CancellationToken {
        let mut requests = self.book_requests.lock();
        requests.retain(|_, held| held.symbol != symbol);
        let request = BookRequest::new(symbol);
        let cancel = request.cancel.clone();
        requests.insert(req_id, request);
        cancel
    }

    pub(crate) fn register_l2_recovery(
        &self,
        instrument_id: InstrumentId,
        generation: u64,
        recovery: &Arc<BookRecovery<KrakenWsError>>,
    ) {
        let symbol = to_ws_v2_symbol(instrument_id.symbol.inner());
        self.l2_depths
            .register_recovery(symbol, generation, recovery);
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
    pub fn l3_depths_handle(&self) -> Arc<parking_lot::Mutex<ahash::AHashMap<String, u32>>> {
        Arc::clone(&self.l3_depths)
    }

    pub(crate) fn l3_subscriptions(&self) -> L2Depths {
        self.l3_subscriptions.clone()
    }

    /// Returns a shared handle to the per-symbol L2 depth map.
    pub(crate) fn l2_depths_handle(&self) -> L2Depths {
        self.l2_depths.clone()
    }

    /// Returns a shared handle to the `book` subscribes awaiting the venue's answer.
    pub(crate) fn book_requests_handle(&self) -> BookRequests {
        Arc::clone(&self.book_requests)
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
        self.subscribe_executions_with_replay(snap_orders, snap_trades, false)
            .await
    }

    pub(crate) async fn subscribe_executions_for_reconciliation(
        &self,
    ) -> Result<(), KrakenWsError> {
        self.subscribe_executions_with_replay(false, false, true)
            .await
    }

    async fn subscribe_executions_with_replay(
        &self,
        snap_orders: bool,
        snap_trades: bool,
        recover_open_orders: bool,
    ) -> Result<(), KrakenWsError> {
        let req_id = self.get_next_req_id();

        let token = self.auth_token().await.ok_or_else(|| {
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
        let payload = if recover_open_orders {
            request_order_snapshot_in_payload(payload.expose_secret())?
        } else {
            payload
        };

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
    ///
    /// The unsubscribe carries the recorded depth: the venue keys a `book` subscription by symbol
    /// and depth, and takes an unsubscribe without a depth as depth 10.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        self.unsubscribe_book_symbol(to_ws_v2_symbol(instrument_id.symbol.inner()))
            .await
    }

    async fn unsubscribe_book_symbol(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        let _book_commands = self.book_commands.lock().await;
        self.remove_book_reference(symbol).await
    }

    async fn remove_book_reference(&self, symbol: Ustr) -> Result<(), KrakenWsError> {
        let channel_str = KrakenWsChannel::Book.as_ref();
        let key = format!("{channel_str}:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);

        let depth = self.l2_depths.get(symbol.as_str());
        self.l2_depths.remove(symbol.as_str());
        self.book_requests
            .lock()
            .retain(|_, held| held.symbol != symbol);
        self.subscription_payloads.write().await.remove(&key);

        let req_id = self.get_next_req_id();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Unsubscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Book,
                symbol: Some(vec![symbol]),
                snapshot: None,
                depth,
                interval: None,
                event_trigger: None,
                token: None,
                snap_orders: None,
                snap_trades: None,
            })),
            req_id: Some(req_id),
        };

        self.send_book_command(&request)?;
        self.subscriptions.confirm_unsubscribe(&key);
        Ok(())
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

struct SpotReplay {
    config: KrakenDataClientConfig,
    auth_token: Arc<tokio::sync::RwLock<Option<SecretString>>>,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    payloads: Arc<tokio::sync::RwLock<HashMap<String, SecretString>>>,
    l2_depths: L2Depths,
    l3_subscriptions: L2Depths,
    book_commands: Arc<tokio::sync::Mutex<()>>,
    book_writer: SpotBookWriter,
    book_requests: BookRequests,
    req_ids: Arc<AtomicU64>,
}

impl SpotReplay {
    fn new(client: &KrakenSpotWebSocketClient, book_writer: SpotBookWriter) -> Self {
        Self {
            config: client.config.clone(),
            auth_token: client.auth_token.clone(),
            auth_tracker: client.auth_tracker.clone(),
            subscriptions: client.subscriptions.clone(),
            payloads: Arc::clone(&client.subscription_payloads),
            l2_depths: client.l2_depths.clone(),
            l3_subscriptions: client.l3_subscriptions.clone(),
            book_commands: Arc::clone(&client.book_commands),
            book_writer,
            book_requests: Arc::clone(&client.book_requests),
            req_ids: Arc::clone(&client.req_id_counter),
        }
    }

    async fn run_feed(
        &self,
        mut handler: SpotFeedHandler,
        signal: Arc<AtomicBool>,
        cmd_tx: tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>,
        out_tx: tokio::sync::mpsc::UnboundedSender<KrakenSpotWsMessage>,
    ) {
        while let Some(message) = handler.next().await {
            if matches!(message, KrakenSpotWsMessage::Reconnected) {
                if signal.load(Ordering::Relaxed) {
                    continue;
                }

                // A request the client never sends blocks old frames while replay authenticates
                let reserved = self.req_ids.fetch_add(1, Ordering::Relaxed) + 1;
                self.l2_depths.retire_requests(reserved);
                self.l3_subscriptions.retire_requests(reserved);
                self.replay_subscriptions(handler.book_epoch(), &cmd_tx)
                    .await;
            }

            if out_tx.send(message).is_err() {
                if handler.is_stopped() {
                    log::debug!("Failed to send message (receiver dropped)");
                } else {
                    log::error!("Failed to send message (receiver dropped)");
                }

                return;
            }
        }

        if handler.is_stopped() {
            log::debug!("Stop signal received, ending message processing");
        } else {
            log::warn!("WebSocket stream ended unexpectedly");
        }
    }

    async fn replay_subscriptions(
        &self,
        epoch: u64,
        cmd_tx: &tokio::sync::mpsc::UnboundedSender<SpotHandlerCommand>,
    ) {
        log::info!("WebSocket reconnected, resubscribing");
        let _books = self.book_commands.lock().await;
        self.book_writer.epoch.store(epoch, Ordering::Release);
        begin_resubscribe(&self.subscriptions, &self.book_requests);

        let payloads = self.payloads.read().await;
        if payloads.is_empty() {
            return;
        }

        let needs_auth = self.auth_token.read().await.is_some()
            || payloads.keys().any(|topic| topic.starts_with("level3:"));

        if needs_auth && self.config.has_api_credentials() {
            self.auth_tracker.invalidate();
            self.refresh_authentication().await;
        }

        let token = self.auth_token.read().await.clone();
        for (topic, payload) in payloads.iter() {
            let payload = self.replay_payload(topic, payload, token.as_ref());

            let Some((payload, cancel)) = payload else {
                continue;
            };

            self.subscriptions.mark_subscribe(topic);
            if topic.starts_with("book:") || topic.starts_with("level3:") {
                if let Err(e) = self
                    .book_writer
                    .queue(SpotBookOperation::Send(payload, cancel))
                {
                    log::error!("Failed to replay book subscription for {topic}: {e}");
                }
            } else if let Err(e) = cmd_tx.send(SpotHandlerCommand::Subscribe { payload }) {
                log::error!("Failed to send resubscribe command for {topic}: {e}");
            }
        }
    }

    async fn refresh_authentication(&self) {
        let _response = self.auth_tracker.begin();

        match refresh_auth_token(&self.config).await {
            Ok(token) => {
                *self.auth_token.write().await = Some(token);
                self.auth_tracker.succeed();
            }
            Err(e) => {
                *self.auth_token.write().await = None;
                self.auth_tracker.fail(e.to_string());
                log::error!("Failed to re-authenticate after reconnect: {e}");
            }
        }
    }

    fn replay_payload(
        &self,
        topic: &str,
        payload: &SecretString,
        token: Option<&SecretString>,
    ) -> Option<(SecretString, Option<CancellationToken>)> {
        let payload = if topic == "executions" || topic.starts_with("level3:") {
            let Some(token) = token else {
                log::warn!("Cannot resubscribe to {topic}: no auth token");
                return None;
            };

            update_auth_token_in_payload(payload.expose_secret(), token.expose_secret())
                .inspect_err(|e| log::error!("Failed to update auth token: {e}"))
                .ok()?
        } else {
            payload.clone()
        };

        if let Some(symbol) = topic.strip_prefix("book:") {
            record_book_replay(
                &payload,
                symbol,
                &self.l2_depths,
                &self.book_requests,
                &self.req_ids,
            )
            .map(|(payload, cancel)| (payload, Some(cancel)))
        } else if let Some(symbol) = topic.strip_prefix("level3:") {
            record_book_replay(
                &payload,
                symbol,
                &self.l3_subscriptions,
                &self.book_requests,
                &self.req_ids,
            )
            .map(|(payload, cancel)| (payload, Some(cancel)))
        } else {
            Some((payload, None))
        }
    }
}

/// Retires confirmations and outstanding book requests from the old connection.
fn begin_resubscribe(subscriptions: &SubscriptionState, book_requests: &BookRequests) {
    subscriptions.reset_after_reconnect();
    book_requests.lock().clear();
}

fn record_book_replay(
    payload: &SecretString,
    symbol: &str,
    held: &L2Depths,
    requests: &BookRequests,
    req_ids: &AtomicU64,
) -> Option<(SecretString, CancellationToken)> {
    let mut request: KrakenWsRequest = serde_json::from_str(payload.expose_secret())
        .inspect_err(|e| log::error!("Failed to parse stored book subscription for {symbol}: {e}"))
        .ok()?;
    let req_id = req_ids.fetch_add(1, Ordering::Relaxed) + 1;
    request.req_id = Some(req_id);
    let payload = serde_json::to_string(&request)
        .inspect_err(|e| log::error!("Failed to serialize book replay for {symbol}: {e}"))
        .ok()?;
    held.begin_replay(symbol, req_id)?;
    let request = BookRequest::new(Ustr::from(symbol));
    let cancel = request.cancel.clone();
    requests.lock().insert(req_id, request);

    Some((SecretString::from(payload), cancel))
}

/// Refreshes the authentication token via the HTTP API.
async fn refresh_auth_token(
    config: &KrakenDataClientConfig,
) -> Result<SecretString, KrakenWsError> {
    let api_key = config
        .api_key
        .clone()
        .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API key".to_string()))?;
    let api_secret = config
        .api_secret
        .clone()
        .ok_or_else(|| KrakenWsError::AuthenticationError("Missing API secret".to_string()))?;

    let http_client = KrakenSpotHttpClient::with_credentials(
        api_key.into_inner(),
        api_secret.into_inner(),
        config.environment,
        Some(config.http_base_url()),
        config.timeout_secs,
        None,
        None,
        None,
        config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned()),
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
        ws_token.token.expose_secret().len(),
        ws_token.expires
    );

    Ok(ws_token.into_token())
}

fn update_auth_token_in_payload(
    payload: &str,
    new_token: &str,
) -> Result<SecretString, KrakenWsError> {
    let mut value: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

    if let Some(params) = value.get_mut("params") {
        params["token"] = serde_json::Value::String(new_token.to_string());
    }

    serde_json::to_string(&value)
        .map(SecretString::from)
        .map_err(|e| KrakenWsError::JsonError(e.to_string()))
}

/// Returns an executions subscribe `payload` that requests a snapshot of the open orders.
fn request_order_snapshot_in_payload(payload: &str) -> Result<SecretString, KrakenWsError> {
    let mut value: serde_json::Value =
        serde_json::from_str(payload).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

    if let Some(params) = value.get_mut("params") {
        params["snap_orders"] = serde_json::Value::Bool(true);
    }

    serde_json::to_string(&value)
        .map(SecretString::from)
        .map_err(|e| KrakenWsError::JsonError(e.to_string()))
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
    use std::{
        sync::{Arc, atomic::Ordering},
        time::Duration,
    };

    use futures_util::StreamExt;
    use log::{Level, LevelFilter, Log, Metadata, Record};
    use nautilus_testkit::connection::race_command_with_publication;
    use parking_lot::Mutex;
    use rstest::rstest;
    use tokio_util::sync::CancellationToken;

    use super::*;
    use crate::config::KrakenDataClientConfig;

    #[tokio::test]
    async fn command_during_sender_publication_reaches_initialized_handler() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (received_tx, mut received_rx) = tokio::sync::mpsc::unbounded_channel();

        let server = tokio::spawn(async move {
            let (socket, _) = listener.accept().await.unwrap();
            let mut socket = tokio_tungstenite::accept_async(socket).await.unwrap();
            while let Some(Ok(message)) = socket.next().await {
                if let tokio_tungstenite::tungstenite::Message::Text(text) = message {
                    let payload: serde_json::Value = serde_json::from_str(text.as_str()).unwrap();
                    let _ = received_tx.send(payload);
                }
            }
        });

        let url = format!("ws://{addr}");

        let mut ws_client = KrakenSpotWebSocketClient::new(
            KrakenDataClientConfig {
                ws_public_url: Some(url),
                ..Default::default()
            },
            CancellationToken::new(),
            None,
        )
        .unwrap();

        let observer = ws_client.clone();
        let expected = serde_json::json!({
            "method": "subscribe",
            "params": {
                "channel": "trade",
                "symbol": [
                    "BTC/USD"
                ]
            },
            "req_id": 79
        });

        let command = SpotHandlerCommand::Subscribe {
            payload: SecretString::from(expected.to_string()),
        };

        race_command_with_publication(
            Arc::clone(&ws_client.cmd_tx),
            ws_client.connect(),
            move || command,
            || observer.is_active(),
        )
        .await
        .expect("connect failed");
        let received = tokio::time::timeout(Duration::from_secs(2), received_rx.recv()).await;
        ws_client.close().await.expect("disconnect failed");
        server.abort();

        let payload = received
            .expect("startup command must reach server")
            .unwrap();
        assert_eq!(payload, expected);
    }

    #[rstest]
    #[case("wss://stream.example.com/feed", "wss://stream.example.com")]
    #[case(
        "wss://SYN-user:SYN-password@stream.example.com:9443/SYN-path?token=SYN-query#SYN-fragment",
        "wss://stream.example.com:9443"
    )]
    fn test_debug_preserves_url_origin(#[case] url: &str, #[case] origin: &str) {
        let client = KrakenSpotWebSocketClient::new(
            KrakenDataClientConfig {
                ws_public_url: Some(url.to_string()),
                ..Default::default()
            },
            CancellationToken::new(),
            None,
        )
        .unwrap();

        let debug = format!("{client:?}");
        assert!(debug.contains(&format!("url: {origin:?}")));
        assert!(!debug.contains("SYN-"));
        assert_eq!(client.url.expose(), url);
    }

    #[rstest]
    #[tokio::test]
    async fn test_debug_redacts_auth_state_and_proxy_url() {
        let client = KrakenSpotWebSocketClient::new(
            KrakenDataClientConfig::default(),
            CancellationToken::new(),
            Some("http://user:proxy-secret@localhost".to_string()),
        )
        .unwrap();
        *client.auth_token.write().await = Some(SecretString::from("auth-token".to_string()));

        let debug = format!("{client:?}");

        assert!(debug.contains(REDACTED));
        assert!(!debug.contains("proxy-secret"));
        assert!(!debug.contains("auth-token"));
    }

    const SECRET_MARKER: &str = "OUTBOUND_SECRET_MARKER";

    struct OutboundLogCapture {
        messages: Mutex<Vec<String>>,
    }

    static OUTBOUND_LOG_CAPTURE: OutboundLogCapture = OutboundLogCapture {
        messages: Mutex::new(Vec::new()),
    };

    impl OutboundLogCapture {
        fn clear(&self) {
            self.messages.lock().clear();
        }

        fn messages(&self) -> Vec<String> {
            self.messages.lock().clone()
        }
    }

    impl Log for OutboundLogCapture {
        fn enabled(&self, metadata: &Metadata<'_>) -> bool {
            metadata.level() == Level::Trace
                && metadata.target() == "nautilus_kraken::websocket::spot_v2::client"
        }

        fn log(&self, record: &Record<'_>) {
            if self.enabled(record.metadata()) {
                let message = record.args().to_string();
                if message.starts_with("Sending WebSocket request") {
                    self.messages.lock().push(message);
                }
            }
        }

        fn flush(&self) {}
    }

    #[rstest]
    fn test_a_reconnect_replay_retires_the_unanswered_book_requests() {
        let client = test_client_without_credentials();
        client.record_book_request(7, Ustr::from("BTC/USD"));
        assert_eq!(client.book_requests.lock().len(), 1);

        begin_resubscribe(&client.subscriptions, &client.book_requests);

        assert!(client.book_requests.lock().is_empty());
    }

    #[rstest]
    fn test_req_id_counter_is_shared_arc_and_monotonic() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::new(cfg, CancellationToken::new(), None).unwrap();
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
        assert_eq!(result, expected);
    }

    fn test_client_without_credentials() -> KrakenSpotWebSocketClient {
        KrakenSpotWebSocketClient::new(
            KrakenDataClientConfig::default(),
            CancellationToken::new(),
            None,
        )
        .unwrap()
    }

    #[rstest]
    #[tokio::test]
    async fn test_outbound_logs_omit_payload_bodies() {
        log::set_logger(&OUTBOUND_LOG_CAPTURE).expect("test logger already installed");
        log::set_max_level(LevelFilter::Trace);

        let client = test_client_without_credentials();
        let request = KrakenWsRequest {
            method: KrakenWsMethod::Subscribe,
            params: Some(KrakenWsParams::Channel(KrakenWsChannelParams {
                channel: KrakenWsChannel::Executions,
                symbol: None,
                snapshot: None,
                depth: None,
                interval: None,
                event_trigger: None,
                token: Some(SecretString::from(SECRET_MARKER)),
                snap_orders: Some(true),
                snap_trades: Some(false),
            })),
            req_id: Some(426),
        };
        let payload_len = serde_json::to_string(&request).unwrap().len();
        OUTBOUND_LOG_CAPTURE.clear();

        let error = client.send_command(&request).await.unwrap_err();
        let messages = OUTBOUND_LOG_CAPTURE.messages();

        assert!(matches!(error, KrakenWsError::ConnectionError(_)));
        assert!(
            messages
                .iter()
                .all(|message| !message.contains(SECRET_MARKER)),
            "outbound logs exposed the secret marker: {messages:?}"
        );
        assert!(
            messages.iter().any(|message| {
                message
                    == &format!("Sending WebSocket request: method=Subscribe ({payload_len} bytes)")
            }),
            "subscribe metadata missing or inaccurate: {messages:?}"
        );
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
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();
        assert_eq!(client.url(), "wss://ws-l3.kraken.com/v2");
    }

    #[rstest]
    fn test_l3_factory_respects_override() {
        let cfg = KrakenDataClientConfig {
            ws_l3_url: Some("wss://override.example/v2".to_string()),
            ..Default::default()
        };

        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();
        assert_eq!(client.url(), "wss://override.example/v2");
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_l3_without_auth_errors_and_leaves_clean_state() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();

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
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();

        let err = client
            .subscribe_book_l3(Ustr::from("BTC/USD"), 50)
            .await
            .expect_err("should fail on invalid depth");

        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));
        assert!(!client.subscriptions_contains("level3:BTC/USD"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_invalid_depth_errors() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");

        let err = client
            .subscribe_book(instrument_id, Some(50))
            .await
            .expect_err("should fail on invalid depth");

        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));
        assert!(!client.subscriptions_contains("book:BTC/USD"));
        assert_eq!(client.l2_depths.get("BTC/USD"), None);
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_defaults_to_depth_10_and_stores_state() {
        let client = test_client_without_credentials();
        let key = "book:BTC/USD";
        let mut book_rx = book_queue(&client);

        client
            .subscribe_book(InstrumentId::from("BTC/USD.KRAKEN"), None)
            .await
            .expect("subscribe should succeed");

        let cmd = book_rx.try_recv().expect("expected subscribe command");

        let SpotBookOperation::Send(payload, _) = cmd.operation else {
            panic!("expected subscribe command");
        };
        assert_book_subscribe_payload(payload.expose_secret(), "BTC/USD", 10);

        assert_eq!(client.subscriptions.get_reference_count(key), 1);
        assert!(client.subscriptions_contains(key));
        assert_eq!(client.l2_depths.get("BTC/USD"), Some(10));
        assert_eq!(
            client.subscription_payloads.read().await.get(key),
            Some(&payload)
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_send_failure_rolls_back_l2_state() {
        let client = test_client_without_credentials();
        let key = "book:BTC/USD";

        let err = client
            .subscribe_book(InstrumentId::from("BTC/USD.KRAKEN"), Some(10))
            .await
            .expect_err("should fail when command receiver is closed");

        assert!(matches!(err, KrakenWsError::ConnectionError(_)));
        assert_eq!(client.subscriptions.get_reference_count(key), 0);
        assert!(!client.subscriptions_contains(key));
        assert!(client.subscriptions.pending_subscribe_topics().is_empty());
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(client.l2_depths.get("BTC/USD"), None);
        assert!(!client.subscription_payloads.read().await.contains_key(key));
    }

    #[rstest]
    fn test_subscribe_book_l3_refcount_idempotent() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();
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
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();

        let key = "level3:BTC/USD";
        client.subscriptions.add_reference(key);
        client.subscriptions.mark_subscribe(key);
        client.subscriptions.confirm_subscribe(key);
        client.l3_depths.lock().insert("BTC/USD".to_string(), 1000);

        *client.auth_token.write().await = Some(SecretString::from("test-token".to_string()));

        let err = client
            .subscribe_book_l3(Ustr::from("BTC/USD"), 10)
            .await
            .expect_err("should reject depth mismatch");
        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));

        assert!(client.subscriptions.remove_reference(key));
    }

    #[rstest]
    #[tokio::test]
    async fn test_subscribe_book_rejects_depth_mismatch() {
        let client = test_client_without_credentials();
        let key = "book:BTC/USD";
        client.subscriptions.add_reference(key);
        client.subscriptions.mark_subscribe(key);
        client.subscriptions.confirm_subscribe(key);
        client.l2_depths.insert("BTC/USD", 10, 1);

        let err = client
            .subscribe_book(InstrumentId::from("BTC/USD.KRAKEN"), Some(25))
            .await
            .expect_err("should reject depth mismatch");
        assert!(matches!(err, KrakenWsError::InvalidMessage(_)));

        assert!(client.subscriptions.remove_reference(key));
    }

    #[rstest]
    #[tokio::test]
    async fn test_spot_book_depth_conflict_leaves_batch_references_unchanged() {
        let client = test_client_without_credentials();
        let mut book_rx = book_queue(&client);
        let btc = Ustr::from("BTC/USD");
        let eth = Ustr::from("ETH/USD");
        client
            .subscribe(KrakenWsChannel::Book, vec![eth], Some(25))
            .await
            .unwrap();
        book_rx.try_recv().unwrap();

        let result = client
            .subscribe(KrakenWsChannel::Book, vec![btc, eth], Some(10))
            .await;

        assert!(matches!(result, Err(KrakenWsError::InvalidMessage(_))));
        assert_eq!(client.l2_depths.get(btc.as_str()), None);
        assert_eq!(client.l2_depths.get(eth.as_str()), Some(25));
        assert!(matches!(
            book_rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
        assert!(client.subscriptions.remove_reference("book:ETH/USD"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_unsubscribe_book_removes_l2_depth_on_last_reference() {
        let client = test_client_without_credentials();
        let key = "book:BTC/USD";
        let mut book_rx = book_queue(&client);

        client.subscriptions.add_reference(key);
        client.subscriptions.mark_subscribe(key);
        client.subscriptions.confirm_subscribe(key);
        client.l2_depths.insert("BTC/USD", 10, 1);
        client
            .subscription_payloads
            .write()
            .await
            .insert(key.to_string(), SecretString::from("payload".to_string()));

        client
            .unsubscribe_book(InstrumentId::from("BTC/USD.KRAKEN"))
            .await
            .expect("unsubscribe should succeed");

        assert_eq!(client.subscriptions.get_reference_count(key), 0);
        assert!(!client.subscriptions_contains(key));
        assert_eq!(client.l2_depths.get("BTC/USD"), None);
        assert!(!client.subscription_payloads.read().await.contains_key(key));

        let cmd = book_rx.try_recv().expect("expected unsubscribe command");

        let SpotBookOperation::Send(payload, _) = cmd.operation else {
            panic!("expected unsubscribe command");
        };
        assert_book_unsubscribe_payload(payload.expose_secret(), "BTC/USD", 10);
    }

    #[rstest]
    #[tokio::test]
    async fn test_generic_book_subscribe_replays_and_retires_request_ids() {
        let client = test_client_without_credentials();
        let symbol = Ustr::from("BTC/USD");
        let mut book_rx = book_queue(&client);

        client
            .subscribe(KrakenWsChannel::Book, vec![symbol], Some(25))
            .await
            .unwrap();

        let SpotBookOperation::Send(payload, _) = book_rx.try_recv().unwrap().operation else {
            panic!("expected a book command")
        };

        let original = payload_req_id(payload.expose_secret());
        assert_book_subscribe_payload(payload.expose_secret(), symbol.as_str(), 25);
        let generation = client
            .l2_depths
            .subscription(symbol.as_str())
            .unwrap()
            .generation;

        let reserved = client.req_id_counter.fetch_add(1, Ordering::Relaxed) + 1;
        client.l2_depths.retire_requests(reserved);
        begin_resubscribe(&client.subscriptions, &client.book_requests);
        let (replay, _) = record_book_replay(
            &payload,
            symbol.as_str(),
            &client.l2_depths,
            &client.book_requests,
            &client.req_id_counter,
        )
        .unwrap();
        let replay_id = payload_req_id(replay.expose_secret());
        assert_eq!(replay_id, original + 2);
        assert_book_subscribe_payload(replay.expose_secret(), symbol.as_str(), 25);
        assert_eq!(
            client
                .l2_depths
                .subscription(symbol.as_str())
                .unwrap()
                .generation,
            generation
        );

        client.record_book_request(replay_id + 1, symbol);
        assert!(!client.book_requests.lock().contains_key(&replay_id));
        assert_eq!(
            client
                .book_requests
                .lock()
                .get(&(replay_id + 1))
                .map(|held| held.symbol),
            Some(symbol)
        );

        client
            .unsubscribe(KrakenWsChannel::Book, vec![symbol])
            .await
            .unwrap();

        let SpotBookOperation::Send(payload, _) = book_rx.try_recv().unwrap().operation else {
            panic!("expected a book command")
        };

        assert_book_unsubscribe_payload(payload.expose_secret(), symbol.as_str(), 25);
        assert!(client.book_requests.lock().is_empty());
        assert_eq!(client.l2_depths.subscription(symbol.as_str()), None);
    }

    #[rstest]
    #[tokio::test]
    async fn test_replay_orders_book_turnover_on_the_observed_connection() {
        let client = test_client_without_credentials();
        let instrument = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        let writer = client.book_writer.read().as_ref().unwrap().clone();
        let replay = SpotReplay::new(&client, writer.clone());
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        client.subscribe_book(instrument, Some(25)).await.unwrap();
        writer.transport_epoch.store(1, Ordering::Release);
        client.unsubscribe_book(instrument).await.unwrap();
        client.subscribe_book(instrument, Some(25)).await.unwrap();
        replay.replay_subscriptions(1, &tx).await;
        let request = client
            .l2_depths
            .subscription("BTC/USD")
            .unwrap()
            .latest_request;
        client.unsubscribe_book(instrument).await.unwrap();

        for (epoch, subscribe) in [(0, true), (0, false), (0, true), (1, true), (1, false)] {
            let write = book_rx.try_recv().unwrap();
            assert_eq!(write.epoch, epoch);

            let SpotBookOperation::Send(payload, cancellation) = write.operation else {
                panic!("expected a single book command");
            };

            assert_eq!(
                cancellation.as_ref().map(CancellationToken::is_cancelled),
                subscribe.then_some(true)
            );

            if subscribe {
                assert_book_subscribe_payload(payload.expose_secret(), "BTC/USD", 25);
            } else {
                assert_book_unsubscribe_payload(payload.expose_secret(), "BTC/USD", 25);
            }

            if epoch == 1 && subscribe {
                assert_eq!(payload_req_id(payload.expose_secret()), request);
            }
        }

        assert!(book_rx.try_recv().is_err());
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_managed_spot_replacement_skips_a_snapshot_accepted_before_admission() {
        let client = test_client_without_credentials();
        let instrument = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client.subscribe_book(instrument, Some(10)).await.unwrap();
        let initial = book_rx.try_recv().unwrap();

        let SpotBookOperation::Send(..) = initial.operation else {
            panic!("expected the initial subscribe")
        };

        let held = client.l2_depths.subscription("BTC/USD").unwrap();
        let episode = Arc::new(BookRecovery::default());
        client.register_l2_recovery(instrument, held.generation, &episode);
        assert!(episode.begin_replacement());
        let cancel = episode.cancellation.child_token();
        let lock = client.book_commands.lock().await;
        let mut replace =
            std::pin::pin!(client.recover_l2_book(instrument, held.generation, cancel));
        assert!(futures_util::poll!(&mut replace).is_pending());

        let symbol = Ustr::from("BTC/USD");
        assert_eq!(
            client
                .l2_depths
                .confirm_recovery(symbol, held.latest_request),
            Some(held)
        );
        let accepted = client
            .l2_depths
            .accept_snapshot("BTC/USD", held.latest_request, || episode.accept())
            .unwrap();
        drop(lock);
        replace.await.unwrap();

        assert_eq!(client.l2_depths.subscription("BTC/USD"), Some(accepted));
        assert_eq!(accepted.snapshot_epoch, held.snapshot_epoch + 1);
        assert_eq!(accepted.latest_request, held.latest_request);
        assert!(book_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_interrupted_book_write_waits_for_replay_only_after_connection_changes() {
        let client = test_client_without_credentials();
        let symbol = Ustr::from("BTC/USD");
        let (tx, _rx) = tokio::sync::mpsc::unbounded_channel();
        let epoch = Arc::new(AtomicU64::new(18));
        *client.book_writer.write() = Some(SpotBookWriter {
            tx,
            epoch: Arc::clone(&epoch),
            transport_epoch: epoch,
        });

        for (id, admitted_epoch, expected) in [
            (30, 17, Ok(())),
            (
                31,
                18,
                Err(KrakenWsError::ConnectionError("write interrupted".into())),
            ),
        ] {
            client.record_book_request(id, symbol);
            let (result, response) = tokio::sync::oneshot::channel();
            result
                .send(Err(KrakenWsError::ConnectionError(
                    "write interrupted".into(),
                )))
                .unwrap();
            let actual = client
                .await_book_recovery(id, CancellationToken::new(), response, admitted_epoch)
                .await;
            assert_eq!(
                actual.map_err(|e| e.to_string()),
                expected.map_err(|e| e.to_string())
            );
            assert!(!client.book_requests.lock().contains_key(&id));
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_unsubscribe_book_cancels_pending_recovery_write() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();
        assert!(matches!(
            book_rx.try_recv(),
            Ok(SpotBookWrite {
                operation: SpotBookOperation::Send(..),
                ..
            })
        ));

        let live = client.l2_depths.subscription("BTC/USD").unwrap();
        let episode = Arc::new(BookRecovery::default());
        client.register_l2_recovery(instrument_id, live.generation, &episode);
        client
            .book_writer
            .read()
            .as_ref()
            .unwrap()
            .epoch
            .store(17, Ordering::Release);
        let cancel = episode.cancellation.child_token();
        let mut recovery =
            std::pin::pin!(client.recover_l2_book(instrument_id, live.generation, cancel.clone(),));
        assert!(futures_util::poll!(&mut recovery).is_pending());
        let pending = book_rx.try_recv().expect("pending recovery write");
        assert_eq!(pending.epoch, 17);

        let mut unsubscribe = std::pin::pin!(client.unsubscribe_book(instrument_id));
        assert!(matches!(
            futures_util::poll!(&mut unsubscribe),
            std::task::Poll::Ready(Ok(()))
        ));
        assert!(cancel.is_cancelled());
        assert_eq!(client.l2_depths.subscription("BTC/USD"), None);

        let SpotBookOperation::Send(payload, _) =
            book_rx.try_recv().expect("unsubscribe").operation
        else {
            panic!("expected a book command")
        };

        assert_book_unsubscribe_payload(payload.expose_secret(), "BTC/USD", 10);
        assert!(book_rx.try_recv().is_err());
    }

    #[rstest]
    #[tokio::test]
    async fn test_generic_subscribe_rejects_level3() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();

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

    /// An executions subscribe replayed after a reconnect asks for the open orders and keeps its
    /// other params.
    #[rstest]
    fn test_request_order_snapshot_in_payload_for_executions() {
        let original = r#"{"method":"subscribe","params":{"channel":"executions","token":"T","snap_orders":false,"snap_trades":false},"req_id":7}"#;

        let rewritten = request_order_snapshot_in_payload(original).unwrap();

        let value: serde_json::Value = serde_json::from_str(rewritten.expose_secret()).unwrap();
        assert_eq!(
            value,
            serde_json::json!({
                "method": "subscribe",
                "params": {"channel": "executions", "token": "T", "snap_orders": true, "snap_trades": false},
                "req_id": 7,
            })
        );
    }

    #[rstest]
    fn test_update_auth_token_in_payload_for_level3() {
        let original = r#"{"method":"subscribe","params":{"channel":"level3","symbol":["BTC/USD"],"depth":1000,"snapshot":true,"token":"OLD"},"req_id":1}"#;
        let rewritten = update_auth_token_in_payload(original, "NEW").unwrap();
        assert!(rewritten.expose_secret().contains(r#""token":"NEW""#));
        assert!(!rewritten.expose_secret().contains(r#""token":"OLD""#));
    }

    #[rstest]
    fn test_l3_depths_shared_between_subscribe_and_handle() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();

        let handle = client.l3_depths_handle();
        client.l3_depths.lock().insert("BTC/USD".to_string(), 100);

        assert_eq!(handle.lock().get("BTC/USD").copied(), Some(100));
    }

    #[rstest]
    fn test_l2_depths_shared_between_subscribe_and_handle() {
        let client = test_client_without_credentials();

        let handle = client.l2_depths_handle();
        client.l2_depths.insert("BTC/USD", 10, 1);

        assert_eq!(handle.get("BTC/USD"), Some(10));
    }

    /// Only the latest book acknowledgement can identify the current stream.
    #[rstest]
    #[tokio::test]
    async fn test_subscribe_and_resync_book_record_their_requests() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        let handle = client.book_requests_handle();

        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();
        let subscription = client.l2_depths.subscription("BTC/USD").unwrap();

        let SpotBookOperation::Send(payload, _) = book_rx.try_recv().expect("subscribe").operation
        else {
            panic!("expected a book command")
        };
        let subscribe_req_id = payload_req_id(payload.expose_secret());

        assert_eq!(
            handle.lock().get(&subscribe_req_id).map(|held| held.symbol),
            Some(Ustr::from("BTC/USD"))
        );
        assert_eq!(subscription.latest_request, subscribe_req_id);

        client
            .resync_book(
                instrument_id,
                subscription.generation,
                subscription.snapshot_epoch,
            )
            .await
            .unwrap();
        let [unsubscribe, subscribe] = read_book_replacement(&mut book_rx, 10);
        let unsubscribe_req_id = payload_req_id(unsubscribe.expose_secret());
        let resync_req_id = payload_req_id(subscribe.expose_secret());

        let requests = handle.lock();
        assert_eq!(
            requests.get(&resync_req_id).map(|held| held.symbol),
            Some(Ustr::from("BTC/USD"))
        );
        assert_eq!(
            client
                .l2_depths
                .subscription("BTC/USD")
                .unwrap()
                .latest_request,
            resync_req_id
        );
        assert!(
            !requests.contains_key(&unsubscribe_req_id),
            "only subscribes are on record"
        );
        assert_eq!(
            requests
                .iter()
                .map(|(req, held)| (*req, held.symbol))
                .collect::<ahash::AHashMap<_, _>>(),
            ahash::AHashMap::from([(resync_req_id, Ustr::from("BTC/USD"))])
        );
    }

    /// A disconnect clears the record of in-flight subscribes along with the subscriptions: the
    /// venue answers none of them on a new connection.
    #[rstest]
    #[tokio::test]
    async fn test_disconnect_clears_the_book_requests() {
        let client = test_client_without_credentials();
        let _book_rx = book_queue(&client);
        client
            .subscribe_book(InstrumentId::from("BTC/USD.KRAKEN"), Some(10))
            .await
            .unwrap();
        assert_eq!(client.book_requests.lock().len(), 1);

        let _ = client.disconnect_locked().await;

        assert!(client.book_requests.lock().is_empty());
    }

    fn payload_req_id(payload: &str) -> u64 {
        let value: serde_json::Value = serde_json::from_str(payload).unwrap();
        value["req_id"].as_u64().expect("a req_id")
    }

    /// A recovery whose snapshot the data client has accepted since it was issued sends nothing,
    /// leaving the recovered stream subscribed; a recovery carrying the epoch that snapshot moved
    /// to, as a mismatch on the snapshot issues, resubscribes.
    #[rstest]
    #[tokio::test]
    async fn test_resync_book_skips_once_a_snapshot_is_accepted() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();

        while book_rx.try_recv().is_ok() {}
        let issued = client.l2_depths.subscription("BTC/USD").unwrap();

        let accepted = client
            .l2_depths
            .accept_snapshot("BTC/USD", issued.latest_request, || true)
            .expect("the live stream's snapshot is accepted");
        client
            .resync_book(instrument_id, issued.generation, issued.snapshot_epoch)
            .await
            .unwrap();

        assert!(
            book_rx.try_recv().is_err(),
            "a served recovery must send nothing"
        );
        assert_eq!(
            client
                .l2_depths
                .subscription("BTC/USD")
                .unwrap()
                .latest_request,
            issued.latest_request
        );

        client
            .resync_book(instrument_id, accepted.generation, accepted.snapshot_epoch)
            .await
            .unwrap();
        assert_book_recovery_commands(&mut book_rx, 10);
    }

    /// A recovery issued before a reconnect sends nothing once the replay has begun, so it does
    /// not cycle the replayed stream; one issued after it resubscribes.
    #[rstest]
    #[tokio::test]
    async fn test_a_recovery_issued_before_a_reconnect_sends_nothing() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();

        while book_rx.try_recv().is_ok() {}
        let issued = client.l2_depths.subscription("BTC/USD").unwrap();

        let reserved = client.req_id_counter.fetch_add(1, Ordering::Relaxed) + 1;
        client.l2_depths.retire_requests(reserved);
        begin_resubscribe(&client.subscriptions, &client.book_requests);
        client
            .resync_book(instrument_id, issued.generation, issued.snapshot_epoch)
            .await
            .unwrap();

        assert!(
            book_rx.try_recv().is_err(),
            "a recovery from before the reconnect must send nothing"
        );

        let replayed = client.l2_depths.subscription("BTC/USD").unwrap();
        client
            .resync_book(instrument_id, replayed.generation, replayed.snapshot_epoch)
            .await
            .unwrap();
        assert_book_recovery_commands(&mut book_rx, 10);
    }

    /// A recovery records its resubscribe as the symbol's latest request before sending and keeps
    /// it when a send fails, so the data client sees the request unconfirmed and its watchdog asks
    /// again rather than accepting the old stream as current.
    #[rstest]
    #[tokio::test]
    async fn test_resync_book_keeps_its_latest_request_when_a_send_fails() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();
        let issued = client.l2_depths.subscription("BTC/USD").unwrap();
        drop(book_rx);

        client
            .resync_book(instrument_id, issued.generation, issued.snapshot_epoch)
            .await
            .expect_err("the command channel is closed");

        let latest = client
            .l2_depths
            .subscription("BTC/USD")
            .unwrap()
            .latest_request;
        assert_ne!(latest, issued.latest_request);
        assert!(
            !client.book_requests.lock().contains_key(&latest),
            "an unsent subscribe has no answer on record"
        );
    }

    #[rstest]
    fn test_resync_book_l3_does_not_touch_refcount() {
        let cfg = KrakenDataClientConfig::default();
        let client = KrakenSpotWebSocketClient::l3(cfg, CancellationToken::new(), None).unwrap();
        let key = "level3:BTC/USD";

        assert!(client.subscriptions.add_reference(key));
        assert!(!client.subscriptions.add_reference(key));
        client.subscriptions.mark_subscribe(key);
        client.subscriptions.confirm_subscribe(key);

        assert!(client.subscriptions_contains(key));
    }

    /// A recovery queued for a subscription the user has since replaced leaves the replacement
    /// alone: it sends nothing and the recorded depth is the replacement's.
    #[rstest]
    #[tokio::test]
    async fn test_resync_book_leaves_a_replacement_subscription_alone() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);

        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();
        let retired = client.l2_depths.subscription("BTC/USD").unwrap().generation;
        client.unsubscribe_book(instrument_id).await.unwrap();
        client
            .subscribe_book(instrument_id, Some(100))
            .await
            .unwrap();

        while book_rx.try_recv().is_ok() {}

        client.resync_book(instrument_id, retired, 0).await.unwrap();

        assert!(
            book_rx.try_recv().is_err(),
            "a retired subscription's recovery must send nothing"
        );
        assert_eq!(client.l2_depths.get("BTC/USD"), Some(100));

        // Control: the live subscription's own recovery resubscribes at its depth.
        let live = client.l2_depths.subscription("BTC/USD").unwrap();
        client
            .resync_book(instrument_id, live.generation, live.snapshot_epoch)
            .await
            .unwrap();

        read_book_replacement(&mut book_rx, 100);
        assert_eq!(client.l2_depths.get("BTC/USD"), Some(100));
    }

    /// Spawns `command` and yields until it has finished or parked on a lock, so any command it
    /// could send without waiting is already in the queue.
    async fn spawn_parked<F>(command: F) -> tokio::task::JoinHandle<Result<(), KrakenWsError>>
    where
        F: Future<Output = Result<(), KrakenWsError>> + Send + 'static,
    {
        let handle = tokio::spawn(command);

        for _ in 0..10 {
            tokio::task::yield_now().await;
        }

        handle
    }

    fn read_book_replacement(
        book_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SpotBookWrite>,
        depth: u32,
    ) -> [SecretString; 2] {
        let operation = book_rx.try_recv().expect("replacement").operation;

        let SpotBookOperation::Replace {
            payloads: [unsubscribe, subscribe],
            ..
        } = operation
        else {
            panic!("expected a book replacement");
        };

        assert_book_unsubscribe_payload(unsubscribe.expose_secret(), "BTC/USD", depth);
        assert_book_subscribe_payload(subscribe.expose_secret(), "BTC/USD", depth);
        let value: serde_json::Value = serde_json::from_str(subscribe.expose_secret()).unwrap();
        assert_eq!(value["params"]["snapshot"], serde_json::json!(true));
        [unsubscribe, subscribe]
    }

    fn assert_book_recovery_commands(
        book_rx: &mut tokio::sync::mpsc::UnboundedReceiver<SpotBookWrite>,
        depth: u32,
    ) {
        read_book_replacement(book_rx, depth);
        assert!(book_rx.try_recv().is_err(), "nothing else is sent");
    }

    fn book_queue(
        client: &KrakenSpotWebSocketClient,
    ) -> tokio::sync::mpsc::UnboundedReceiver<SpotBookWrite> {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        *client.book_writer.write() = Some(SpotBookWriter {
            tx,
            epoch: Arc::new(AtomicU64::new(0)),
            transport_epoch: Arc::new(AtomicU64::new(0)),
        });

        rx
    }

    /// A recovery waits for the book command lock, so its check of the live subscription and its
    /// two commands cannot interleave with a subscription change holding the lock.
    #[rstest]
    #[tokio::test]
    async fn test_resync_book_waits_for_the_book_command_lock() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(25))
            .await
            .unwrap();

        while book_rx.try_recv().is_ok() {}
        let live = client.l2_depths.subscription("BTC/USD").unwrap();

        let guard = client.book_commands.lock().await;
        let task_client = client.clone();
        let recovery = spawn_parked(async move {
            task_client
                .resync_book(instrument_id, live.generation, live.snapshot_epoch)
                .await
        })
        .await;
        assert!(
            book_rx.try_recv().is_err(),
            "no command while a subscription change holds the lock"
        );

        drop(guard);
        recovery.await.unwrap().unwrap();
        assert_book_recovery_commands(&mut book_rx, 25);
    }

    /// A subscription change waits for the book command lock, so it cannot land between a
    /// recovery's check and its commands.
    #[rstest]
    #[tokio::test]
    async fn test_subscribe_and_unsubscribe_book_wait_for_the_book_command_lock() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);

        let guard = client.book_commands.lock().await;
        let task_client = client.clone();
        let subscribe =
            spawn_parked(async move { task_client.subscribe_book(instrument_id, Some(10)).await })
                .await;
        assert!(
            book_rx.try_recv().is_err(),
            "no subscribe while the lock is held"
        );

        drop(guard);
        subscribe.await.unwrap().unwrap();

        let SpotBookOperation::Send(payload, _) = book_rx.try_recv().expect("subscribe").operation
        else {
            panic!("expected a book command")
        };
        assert_book_subscribe_payload(payload.expose_secret(), "BTC/USD", 10);
        assert!(book_rx.try_recv().is_err());

        let guard = client.book_commands.lock().await;
        let task_client = client.clone();
        let unsubscribe =
            spawn_parked(async move { task_client.unsubscribe_book(instrument_id).await }).await;
        assert!(
            book_rx.try_recv().is_err(),
            "no unsubscribe while the lock is held"
        );

        drop(guard);
        unsubscribe.await.unwrap().unwrap();

        let SpotBookOperation::Send(payload, _) =
            book_rx.try_recv().expect("unsubscribe").operation
        else {
            panic!("expected a book command")
        };
        assert_book_unsubscribe_payload(payload.expose_secret(), "BTC/USD", 10);
        assert!(book_rx.try_recv().is_err());
        assert_eq!(client.l2_depths.get("BTC/USD"), None);
    }

    #[rstest]
    #[tokio::test]
    async fn test_final_unsubscribe_cancels_queued_manual_replacement() {
        let client = test_client_without_credentials();
        let instrument_id = InstrumentId::from("BTC/USD.KRAKEN");
        let mut book_rx = book_queue(&client);
        client
            .subscribe_book(instrument_id, Some(10))
            .await
            .unwrap();

        while book_rx.try_recv().is_ok() {}
        let live = client.l2_depths.subscription("BTC/USD").unwrap();

        client
            .resync_book(instrument_id, live.generation, live.snapshot_epoch)
            .await
            .unwrap();
        let queued = book_rx.try_recv().unwrap();

        let SpotBookOperation::Replace {
            cancel,
            payloads: [unsubscribe, subscribe],
            ..
        } = queued.operation
        else {
            panic!("expected a manual replacement");
        };

        assert_book_unsubscribe_payload(unsubscribe.expose_secret(), "BTC/USD", 10);
        assert_book_subscribe_payload(subscribe.expose_secret(), "BTC/USD", 10);
        assert!(!cancel.is_cancelled());
        client.unsubscribe_book(instrument_id).await.unwrap();
        assert!(cancel.is_cancelled());

        let command = book_rx.try_recv().unwrap();

        let SpotBookOperation::Send(payload, _) = command.operation else {
            panic!("expected the final unsubscribe");
        };

        assert_book_unsubscribe_payload(payload.expose_secret(), "BTC/USD", 10);
        assert!(book_rx.try_recv().is_err());
    }

    fn assert_book_subscribe_payload(payload: &str, symbol: &str, depth: u32) {
        let value: serde_json::Value =
            serde_json::from_str(payload).expect("payload should parse as JSON");

        assert_eq!(value["method"], serde_json::json!("subscribe"));
        assert_eq!(value["params"]["channel"], serde_json::json!("book"));
        assert_eq!(value["params"]["symbol"], serde_json::json!([symbol]));
        assert_eq!(value["params"]["depth"], serde_json::json!(depth));
    }

    /// The unsubscribe names the depth: the venue keys a `book` subscription by symbol and depth.
    fn assert_book_unsubscribe_payload(payload: &str, symbol: &str, depth: u32) {
        let value: serde_json::Value =
            serde_json::from_str(payload).expect("payload should parse as JSON");

        assert_eq!(value["method"], serde_json::json!("unsubscribe"));
        assert_eq!(value["params"]["channel"], serde_json::json!("book"));
        assert_eq!(value["params"]["symbol"], serde_json::json!([symbol]));
        assert_eq!(value["params"]["depth"], serde_json::json!(depth));
    }
}
