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

//! Provides the WebSocket client integration for the [BitMEX](https://bitmex.com) WebSocket API.
//!
//! This module defines and implements a [`BitmexWebSocketClient`] for
//! connecting to BitMEX WebSocket streams. It handles authentication (when credentials
//! are provided), manages subscriptions to market data and account update channels,
//! and parses incoming messages into structured Nautilus domain objects.

use std::{
    collections::HashSet,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
};

use ahash::AHashSet;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::runtime::get_runtime;
use nautilus_core::{consts::NAUTILUS_USER_AGENT, env::get_env_var};
use nautilus_model::{
    data::bar::BarType,
    enums::OrderType,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::websocket::{
    AUTHENTICATION_TIMEOUT_SECS, AuthTracker, PingHandler, SubscriptionState, WebSocketClient,
    WebSocketConfig, auth::AuthResultReceiver, channel_message_handler,
};
use reqwest::header::USER_AGENT;
use tokio::{sync::RwLock, time::Duration};
use tokio_tungstenite::tungstenite::Message;
use ustr::Ustr;

use super::{
    enums::{BitmexWsAuthAction, BitmexWsAuthChannel, BitmexWsOperation, BitmexWsTopic},
    error::BitmexWsError,
    handler::{FeedHandler, HandlerCommand},
    messages::{BitmexAuthentication, BitmexSubscription, NautilusWsMessage},
    parse::{is_index_symbol, topic_from_bar_spec},
};
use crate::common::{
    consts::{BITMEX_WS_TOPIC_DELIMITER, BITMEX_WS_URL},
    credential::Credential,
};

/// Provides a WebSocket client for connecting to the [BitMEX](https://bitmex.com) real-time API.
///
/// Key runtime patterns:
/// - Authentication handshakes are managed by the internal auth tracker, ensuring resubscriptions
///   occur only after BitMEX acknowledges `authKey` messages.
/// - The subscription state maintains pending and confirmed topics so reconnection replay is
///   deterministic and per-topic errors are surfaced.
#[derive(Clone, Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.adapters")
)]
pub struct BitmexWebSocketClient {
    url: String,
    credential: Option<Credential>,
    heartbeat: Option<u64>,
    inner: Arc<RwLock<Option<WebSocketClient>>>,
    cmd_tx: Arc<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    signal: Arc<AtomicBool>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    account_id: AccountId,
    auth_tracker: AuthTracker,
    subscriptions: SubscriptionState,
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    order_type_cache: Arc<DashMap<ClientOrderId, OrderType>>,
    order_symbol_cache: Arc<DashMap<ClientOrderId, Ustr>>,
}

impl BitmexWebSocketClient {
    /// Creates a new [`BitmexWebSocketClient`] instance.
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided (both or neither required).
    pub fn new(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
    ) -> anyhow::Result<Self> {
        let credential = match (api_key, api_secret) {
            (Some(key), Some(secret)) => Some(Credential::new(key, secret)),
            (None, None) => None,
            _ => anyhow::bail!("Both `api_key` and `api_secret` must be provided together"),
        };

        let account_id = account_id.unwrap_or(AccountId::from("BITMEX-master"));

        // We don't have a handler yet; this placeholder keeps cache_instrument() working.
        // connect() swaps in the real channel and replays any queued instruments so the
        // handler sees them once it starts.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        Ok(Self {
            url: url.unwrap_or(BITMEX_WS_URL.to_string()),
            credential,
            heartbeat,
            inner: Arc::new(RwLock::new(None)),
            out_rx: None,
            signal: Arc::new(AtomicBool::new(false)),
            task_handle: None,
            account_id,
            auth_tracker: AuthTracker::new(),
            subscriptions: SubscriptionState::new(BITMEX_WS_TOPIC_DELIMITER),
            instruments_cache: Arc::new(DashMap::new()),
            order_type_cache: Arc::new(DashMap::new()),
            order_symbol_cache: Arc::new(DashMap::new()),
            cmd_tx: Arc::new(cmd_tx),
        })
    }

    /// Creates a new authenticated [`BitmexWebSocketClient`] using environment variables.
    ///
    /// # Errors
    ///
    /// Returns an error if environment variables are not set or credentials are invalid.
    pub fn from_env() -> anyhow::Result<Self> {
        let url = get_env_var("BITMEX_WS_URL")?;
        let api_key = get_env_var("BITMEX_API_KEY")?;
        let api_secret = get_env_var("BITMEX_API_SECRET")?;

        Self::new(Some(url), Some(api_key), Some(api_secret), None, None)
    }

    /// Returns the websocket url being used by the client.
    #[must_use]
    pub const fn url(&self) -> &str {
        self.url.as_str()
    }

    /// Returns the public API key being used by the client.
    #[must_use]
    pub fn api_key(&self) -> Option<&str> {
        self.credential.as_ref().map(|c| c.api_key.as_str())
    }

    /// Returns a value indicating whether the client is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_active(),
                None => false,
            },
            Err(_) => false,
        }
    }

    /// Returns a value indicating whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        match self.inner.try_read() {
            Ok(guard) => match &*guard {
                Some(inner) => inner.is_closed(),
                None => true,
            },
            Err(_) => true,
        }
    }

    /// Sets the account ID.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = account_id;
    }

    /// Caches multiple instruments.
    ///
    /// Clears the existing cache first, then adds all provided instruments.
    pub fn cache_instruments(&mut self, instruments: Vec<InstrumentAny>) {
        self.instruments_cache.clear();
        let mut count = 0;

        log::debug!("Initializing BitMEX instrument cache");

        for inst in instruments {
            let symbol = inst.symbol().inner();
            self.instruments_cache.insert(symbol, inst.clone());
            log::debug!("Cached instrument: {symbol}");
            count += 1;
        }

        log::info!("BitMEX instrument cache initialized with {count} instruments");
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same symbol will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments_cache
            .insert(instrument.symbol().inner(), instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Err(e) = self
            .cmd_tx
            .send(HandlerCommand::UpdateInstrument(instrument))
        {
            log::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    /// Connect to the BitMEX WebSocket server.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or authentication fails (if credentials provided).
    ///
    /// # Panics
    ///
    /// Panics if subscription or authentication messages fail to serialize to JSON.
    pub async fn connect(&mut self) -> Result<(), BitmexWsError> {
        let reader = self.connect_inner().await?;

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        // Create fresh command channel for this connection
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        self.cmd_tx = Arc::new(cmd_tx.clone());

        // Replay cached instruments to the new handler via the new channel
        if !self.instruments_cache.is_empty() {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            if let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(cached_instruments)) {
                tracing::error!("Failed to replay instruments to handler: {e}");
            }
        }

        let signal = self.signal.clone();
        let account_id = self.account_id;
        let inner_client = self.inner.clone();
        let credential = self.credential.clone();
        let auth_tracker = self.auth_tracker.clone();
        let subscriptions = self.subscriptions.clone();
        let order_type_cache = self.order_type_cache.clone();
        let order_symbol_cache = self.order_symbol_cache.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                reader,
                signal,
                cmd_rx,
                out_tx,
                account_id,
                auth_tracker.clone(),
                subscriptions.clone(),
                order_type_cache,
                order_symbol_cache,
            );

            // Run message processing with reconnection handling
            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        log::info!("Reconnecting WebSocket");

                        let has_client = {
                            let guard = inner_client.read().await;
                            guard.is_some()
                        };

                        if !has_client {
                            log::warn!("Reconnection signaled but WebSocket client unavailable");
                            continue;
                        }

                        let confirmed = subscriptions.confirmed();
                        let pending = subscriptions.pending_subscribe();
                        let mut restore_set: HashSet<String> = HashSet::new();

                        let mut collect_topics = |map: &DashMap<Ustr, AHashSet<Ustr>>| {
                            for entry in map.iter() {
                                let (channel, symbols) = entry.pair();

                                if *channel == BitmexWsTopic::Instrument.as_ref() {
                                    continue;
                                }

                                for symbol in symbols.iter() {
                                    if symbol.is_empty() {
                                        restore_set.insert(channel.to_string());
                                    } else {
                                        restore_set.insert(format!("{channel}:{symbol}"));
                                    }
                                }
                            }
                        };

                        collect_topics(&confirmed);
                        collect_topics(&pending);

                        let mut topics_to_restore: Vec<String> = restore_set.into_iter().collect();
                        topics_to_restore.sort();

                        let auth_rx_opt = if let Some(cred) = &credential {
                            match Self::issue_authentication_request(
                                &inner_client,
                                cred,
                                &auth_tracker,
                            )
                            .await
                            {
                                Ok(rx) => Some(rx),
                                Err(e) => {
                                    log::error!(
                                        "Failed to send re-authentication request after reconnection: {e}"
                                    );
                                    continue;
                                }
                            }
                        } else {
                            None
                        };

                        let inner_for_task = inner_client.clone();
                        let state_for_task = subscriptions.clone();
                        let auth_tracker_for_task = auth_tracker.clone();
                        let auth_rx_for_task = auth_rx_opt;
                        let topics_to_restore_clone = topics_to_restore.clone();
                        get_runtime().spawn(async move {
                            if let Some(rx) = auth_rx_for_task {
                                if let Err(e) = auth_tracker_for_task
                                    .wait_for_result::<BitmexWsError>(
                                        Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS),
                                        rx,
                                    )
                                    .await
                                {
                                    log::error!("Authentication after reconnection failed: {e}");
                                    return;
                                }
                                log::info!("Re-authenticated after reconnection");
                            }

                            let mut all_topics =
                                Vec::with_capacity(1 + topics_to_restore_clone.len());
                            all_topics.push(BitmexWsTopic::Instrument.as_ref().to_string());
                            all_topics.extend(topics_to_restore_clone.iter().cloned());

                            for topic in &all_topics {
                                state_for_task.mark_subscribe(topic.as_str());
                            }

                            if let Err(e) = Self::send_topics(
                                &inner_for_task,
                                BitmexWsOperation::Subscribe,
                                all_topics.clone(),
                            )
                            .await
                            {
                                log::error!(
                                    "Failed to restore subscriptions after reconnection: {e}"
                                );
                                // Leave topics pending so the next reconnect attempt retries them.
                            } else {
                                log::info!(
                                    "Restored {} subscriptions after reconnection",
                                    all_topics.len()
                                );
                            }
                        });
                    }
                    Some(msg) => {
                        if let Err(e) = handler.out_tx.send(msg) {
                            tracing::error!("Error sending message: {e}");
                            break;
                        }
                    }
                    None => {
                        // Stream ended - check if it's a stop signal
                        if handler.signal.load(Ordering::Relaxed) {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        // Otherwise it's an unexpected stream end
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }
        });

        self.task_handle = Some(Arc::new(stream_handle));

        if self.credential.is_some() {
            self.authenticate().await?;
        }

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                self.subscriptions
                    .mark_subscribe(BitmexWsTopic::Instrument.as_ref());

                let subscribe_msg = BitmexSubscription {
                    op: BitmexWsOperation::Subscribe,
                    args: vec![Ustr::from(BitmexWsTopic::Instrument.as_ref())],
                };

                match serde_json::to_string(&subscribe_msg) {
                    Ok(subscribe_json) => {
                        if let Err(e) = inner.send_text(subscribe_json, None).await {
                            log::error!("Failed to subscribe to instruments: {e}");
                        } else {
                            log::debug!("Subscribed to all instruments");
                        }
                    }
                    Err(e) => {
                        tracing::error!(error = %e, "Failed to serialize resubscribe message");
                    }
                }
            }
        }

        Ok(())
    }

    /// Connect to the WebSocket and return a message receiver.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket connection fails or if authentication fails (when credentials are provided).
    async fn connect_inner(
        &mut self,
    ) -> Result<tokio::sync::mpsc::UnboundedReceiver<Message>, BitmexWsError> {
        let (message_handler, rx) = channel_message_handler();

        let inner_for_ping = self.inner.clone();
        let ping_handler: PingHandler = Arc::new(move |payload: Vec<u8>| {
            let inner = inner_for_ping.clone();

            get_runtime().spawn(async move {
                let len = payload.len();
                let guard = inner.read().await;

                if let Some(client) = guard.as_ref() {
                    if let Err(e) = client.send_pong(payload).await {
                        tracing::warn!(error = %e, "Failed to send pong frame");
                    } else {
                        tracing::trace!("Sent pong frame ({len} bytes)");
                    }
                } else {
                    tracing::debug!("Ping received with no active websocket client");
                }
            });
        });

        let config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![(USER_AGENT.to_string(), NAUTILUS_USER_AGENT.to_string())],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            message_handler: Some(message_handler),
            ping_handler: Some(ping_handler),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: None, // Use default
            reconnect_delay_max_ms: None,     // Use default
            reconnect_backoff_factor: None,   // Use default
            reconnect_jitter_ms: None,        // Use default
        };

        let keyed_quotas = vec![];
        let client = WebSocketClient::connect(
            config,
            None, // post_reconnection
            keyed_quotas,
            None, // default_quota
        )
        .await
        .map_err(|e| BitmexWsError::ClientError(e.to_string()))?;

        {
            let mut inner_guard = self.inner.write().await;
            *inner_guard = Some(client);
        }

        Ok(rx)
    }

    async fn issue_authentication_request(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        credential: &Credential,
        tracker: &AuthTracker,
    ) -> Result<AuthResultReceiver, BitmexWsError> {
        let receiver = tracker.begin();

        let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
        let signature = credential.sign("GET", "/realtime", expires, "");

        let auth_message = BitmexAuthentication {
            op: BitmexWsAuthAction::AuthKeyExpires,
            args: (credential.api_key.to_string(), expires, signature),
        };

        let auth_json = serde_json::to_string(&auth_message).map_err(|e| {
            let msg = format!("Failed to serialize auth message: {e}");
            tracker.fail(msg.clone());
            BitmexWsError::AuthenticationError(msg)
        })?;

        {
            let inner_guard = inner.read().await;
            let client = inner_guard.as_ref().ok_or_else(|| {
                tracker.fail("Cannot authenticate: not connected");
                BitmexWsError::AuthenticationError("Cannot authenticate: not connected".to_string())
            })?;

            client.send_text(auth_json, None).await.map_err(|e| {
                let error = e.to_string();
                tracker.fail(error.clone());
                BitmexWsError::AuthenticationError(error)
            })?;
        }

        Ok(receiver)
    }

    /// Authenticate the WebSocket connection using the provided credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, if authentication fails,
    /// or if credentials are not available.
    async fn authenticate(&self) -> Result<(), BitmexWsError> {
        let credential = match &self.credential {
            Some(credential) => credential,
            None => {
                return Err(BitmexWsError::AuthenticationError(
                    "API credentials not available to authenticate".to_string(),
                ));
            }
        };

        let rx =
            Self::issue_authentication_request(&self.inner, credential, &self.auth_tracker).await?;
        self.auth_tracker
            .wait_for_result::<BitmexWsError>(Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS), rx)
            .await
    }

    /// Wait until the WebSocket connection is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection times out.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), BitmexWsError> {
        let timeout = tokio::time::Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| {
            BitmexWsError::ClientError(format!(
                "WebSocket connection timeout after {timeout_secs} seconds"
            ))
        })?;

        Ok(())
    }

    /// Provides the internal stream as a channel-based stream.
    ///
    /// # Panics
    ///
    /// This function panics:
    /// - If the websocket is not connected.
    /// - If `stream` has already been called somewhere else (stream receiver is then taken).
    pub fn stream(&mut self) -> impl Stream<Item = NautilusWsMessage> + use<> {
        let rx = self
            .out_rx
            .take()
            .expect("Stream receiver already taken or not connected");
        let mut rx = Arc::try_unwrap(rx).expect("Cannot take ownership - other references exist");
        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Closes the client.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if closing fails.
    ///
    /// # Panics
    ///
    /// Panics if the task handle cannot be unwrapped (should never happen in normal usage).
    pub async fn close(&mut self) -> Result<(), BitmexWsError> {
        log::debug!("Starting close process");

        self.signal.store(true, Ordering::Relaxed);

        {
            let inner_guard = self.inner.read().await;
            if let Some(inner) = &*inner_guard {
                log::debug!("Disconnecting websocket");

                match tokio::time::timeout(Duration::from_secs(3), inner.disconnect()).await {
                    Ok(()) => log::debug!("Websocket disconnected successfully"),
                    Err(_) => {
                        log::warn!(
                            "Timeout waiting for websocket disconnect, continuing with cleanup"
                        );
                    }
                }
            } else {
                log::debug!("No active connection to disconnect");
            }
        }

        // Clean up task handle with timeout
        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    log::debug!("Waiting for task handle to complete");
                    match tokio::time::timeout(Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => log::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!(
                                "Timeout waiting for task handle, task may still be running"
                            );
                            // The task will be dropped and should clean up automatically
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

        log::debug!("Closed");

        Ok(())
    }

    async fn send_topics(
        inner: &Arc<RwLock<Option<WebSocketClient>>>,
        op: BitmexWsOperation,
        topics: Vec<String>,
    ) -> Result<(), BitmexWsError> {
        if topics.is_empty() {
            return Ok(());
        }

        let message = BitmexSubscription {
            op,
            args: topics
                .iter()
                .map(|topic| Ustr::from(topic.as_ref()))
                .collect(),
        };

        let op_name = message.op.as_ref().to_string();
        let payload = serde_json::to_string(&message).map_err(|e| {
            BitmexWsError::SubscriptionError(format!("Failed to serialize {op_name} message: {e}"))
        })?;

        let inner_guard = inner.read().await;
        if let Some(client) = &*inner_guard {
            client
                .send_text(payload, None)
                .await
                .map_err(|e| BitmexWsError::SubscriptionError(e.to_string()))?;
        } else {
            log::error!("Cannot send {op_name} message: not connected");
        }

        Ok(())
    }

    /// Subscribe to the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the subscription message fails.
    ///
    /// # Panics
    ///
    /// Panics if serialization of WebSocket messages fails (should never happen).
    pub async fn subscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        log::debug!("Subscribing to topics: {topics:?}");

        for topic in &topics {
            self.subscriptions.mark_subscribe(topic.as_str());
        }

        Self::send_topics(&self.inner, BitmexWsOperation::Subscribe, topics).await
    }

    /// Unsubscribe from the specified topics.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if sending the unsubscription message fails.
    async fn unsubscribe(&self, topics: Vec<String>) -> Result<(), BitmexWsError> {
        log::debug!("Attempting to unsubscribe from topics: {topics:?}");

        if self.signal.load(Ordering::Relaxed) {
            log::debug!("Shutdown signal detected, skipping unsubscribe");
            return Ok(());
        }

        for topic in &topics {
            self.subscriptions.mark_unsubscribe(topic.as_str());
        }

        let result = Self::send_topics(&self.inner, BitmexWsOperation::Unsubscribe, topics).await;
        if let Err(e) = result {
            tracing::debug!(error = %e, "Failed to send unsubscribe message");
        }
        Ok(())
    }

    /// Get the current number of active subscriptions.
    #[must_use]
    pub fn subscription_count(&self) -> usize {
        self.subscriptions.len()
    }

    pub fn get_subscriptions(&self, instrument_id: InstrumentId) -> Vec<String> {
        let symbol = instrument_id.symbol.inner();
        let confirmed = self.subscriptions.confirmed();
        let mut channels = Vec::with_capacity(confirmed.len());

        for entry in confirmed.iter() {
            let (channel, symbols) = entry.pair();
            if symbols.contains(&symbol) {
                // Return the full topic string (e.g., "orderBookL2:XBTUSD")
                channels.push(format!("{channel}:{symbol}"));
            } else {
                let has_channel_marker = symbols.iter().any(|s| s.is_empty());
                if has_channel_marker
                    && (*channel == BitmexWsAuthChannel::Execution.as_ref()
                        || *channel == BitmexWsAuthChannel::Order.as_ref())
                {
                    // These are account-level subscriptions without symbols
                    channels.push(channel.to_string());
                }
            }
        }

        channels
    }

    /// Subscribe to instrument updates for all instruments on the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_instruments(&self) -> Result<(), BitmexWsError> {
        // Already subscribed automatically on connection
        log::debug!("Already subscribed to all instruments on connection, skipping");
        Ok(())
    }

    /// Subscribe to instrument updates (mark/index prices) for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        // Already subscribed to all instruments on connection
        log::debug!(
            "Already subscribed to all instruments on connection (includes {instrument_id}), skipping"
        );
        Ok(())
    }

    /// Subscribe to order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.inner();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order book L2 (25 levels) updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.inner();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order book depth 10 updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBook10;
        let symbol = instrument_id.symbol.inner();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to quote updates for the specified instrument.
    ///
    /// Note: Index symbols (starting with '.') do not have quotes and will be silently ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have quotes (bid/ask), only a single price
        if is_index_symbol(&instrument_id.symbol.inner()) {
            tracing::warn!("Ignoring quote subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = BitmexWsTopic::Quote;
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to trade updates for the specified instrument.
    ///
    /// Note: Index symbols (starting with '.') do not have trades and will be silently ignored.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have trades
        if is_index_symbol(&symbol) {
            tracing::warn!("Ignoring trade subscription for index symbol: {symbol}");
            return Ok(());
        }

        let topic = BitmexWsTopic::Trade;
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        self.subscribe_instrument(instrument_id).await
    }

    /// Subscribe to index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        self.subscribe_instrument(instrument_id).await
    }

    /// Subscribe to funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::Funding;
        let symbol = instrument_id.symbol.inner();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to bar updates for the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the subscription fails.
    pub async fn subscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.subscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from instrument updates for all instruments on the venue.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_instruments(&self) -> Result<(), BitmexWsError> {
        // No-op: instruments are required for proper operation
        log::debug!(
            "Instruments subscription maintained for proper operation, skipping unsubscribe"
        );
        Ok(())
    }

    /// Unsubscribe from instrument updates (mark/index prices) for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_instrument(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        // No-op: instruments are required for proper operation
        log::debug!(
            "Instruments subscription maintained for proper operation (includes {instrument_id}), skipping unsubscribe"
        );
        Ok(())
    }

    /// Unsubscribe from order book updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2;
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from order book L2 (25 levels) updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book_25(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBookL2_25;
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from order book depth 10 updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_book_depth10(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let topic = BitmexWsTopic::OrderBook10;
        let symbol = instrument_id.symbol.inner();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from quote updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have quotes
        if is_index_symbol(&symbol) {
            return Ok(());
        }

        let topic = BitmexWsTopic::Quote;
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from trade updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        let symbol = instrument_id.symbol.inner();

        // Index symbols don't have trades
        if is_index_symbol(&symbol) {
            return Ok(());
        }

        let topic = BitmexWsTopic::Trade;
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Unsubscribe from mark price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_mark_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        // No-op: instrument channel shared with index prices
        log::debug!(
            "Mark prices for {instrument_id} uses shared instrument channel, skipping unsubscribe"
        );
        Ok(())
    }

    /// Unsubscribe from index price updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_index_prices(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        // No-op: instrument channel shared with mark prices
        log::debug!(
            "Index prices for {instrument_id} uses shared instrument channel, skipping unsubscribe"
        );
        Ok(())
    }

    /// Unsubscribe from funding rate updates for the specified instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_funding_rates(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), BitmexWsError> {
        // No-op: unsubscribing during shutdown causes race conditions
        log::debug!(
            "Funding rates for {instrument_id}, skipping unsubscribe to avoid shutdown race"
        );
        Ok(())
    }

    /// Unsubscribe from bar updates for the specified bar type.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_bars(&self, bar_type: BarType) -> Result<(), BitmexWsError> {
        let topic = topic_from_bar_spec(bar_type.spec());
        let symbol = bar_type.instrument_id().symbol.to_string();
        self.unsubscribe(vec![format!("{topic}:{symbol}")]).await
    }

    /// Subscribe to order updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_orders(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Order.to_string()])
            .await
    }

    /// Subscribe to execution updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_executions(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Execution.to_string()])
            .await
    }

    /// Subscribe to position updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_positions(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Position.to_string()])
            .await
    }

    /// Subscribe to margin updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_margin(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Margin.to_string()])
            .await
    }

    /// Subscribe to wallet updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected, not authenticated, or if the subscription fails.
    pub async fn subscribe_wallet(&self) -> Result<(), BitmexWsError> {
        if self.credential.is_none() {
            return Err(BitmexWsError::MissingCredentials);
        }
        self.subscribe(vec![BitmexWsAuthChannel::Wallet.to_string()])
            .await
    }

    /// Unsubscribe from order updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_orders(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Order.to_string()])
            .await
    }

    /// Unsubscribe from execution updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_executions(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Execution.to_string()])
            .await
    }

    /// Unsubscribe from position updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_positions(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Position.to_string()])
            .await
    }

    /// Unsubscribe from margin updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_margin(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Margin.to_string()])
            .await
    }

    /// Unsubscribe from wallet updates for the authenticated account.
    ///
    /// # Errors
    ///
    /// Returns an error if the WebSocket is not connected or if the unsubscription fails.
    pub async fn unsubscribe_wallet(&self) -> Result<(), BitmexWsError> {
        self.unsubscribe(vec![BitmexWsAuthChannel::Wallet.to_string()])
            .await
    }
}

////////////////////////////////////////////////////////////////////////////////
// Tests
////////////////////////////////////////////////////////////////////////////////

#[cfg(test)]
mod tests {
    use ahash::AHashSet;
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;

    #[rstest]
    fn test_reconnect_topics_restoration_logic() {
        // Create real client with credentials
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Populate subscriptions like they would be during normal operation
        let subs = client.subscriptions.confirmed();
        subs.insert(Ustr::from(BitmexWsTopic::Trade.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set.insert(Ustr::from("ETHUSD"));
            set
        });

        subs.insert(Ustr::from(BitmexWsTopic::OrderBookL2.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set
        });

        // Private channels (no symbols)
        subs.insert(Ustr::from(BitmexWsAuthChannel::Order.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from(""));
            set
        });
        subs.insert(Ustr::from(BitmexWsAuthChannel::Position.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from(""));
            set
        });

        // Test the actual reconnection topic building logic
        let mut topics_to_restore = Vec::new();
        for entry in subs.iter() {
            let (channel, symbols) = entry.pair();
            for symbol in symbols.iter() {
                if symbol.is_empty() {
                    topics_to_restore.push(channel.to_string());
                } else {
                    topics_to_restore.push(format!("{channel}:{symbol}"));
                }
            }
        }

        // Verify it builds the correct restoration topics
        assert!(topics_to_restore.contains(&format!("{}:XBTUSD", BitmexWsTopic::Trade.as_ref())));
        assert!(topics_to_restore.contains(&format!("{}:ETHUSD", BitmexWsTopic::Trade.as_ref())));
        assert!(
            topics_to_restore.contains(&format!("{}:XBTUSD", BitmexWsTopic::OrderBookL2.as_ref()))
        );
        assert!(topics_to_restore.contains(&BitmexWsAuthChannel::Order.as_ref().to_string()));
        assert!(topics_to_restore.contains(&BitmexWsAuthChannel::Position.as_ref().to_string()));
        assert_eq!(topics_to_restore.len(), 5);
    }

    #[rstest]
    fn test_reconnect_auth_message_building() {
        // Test with credentials
        let client_with_creds = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Test the actual auth message building logic from lines 220-228
        if let Some(cred) = &client_with_creds.credential {
            let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
            let signature = cred.sign("GET", "/realtime", expires, "");

            let auth_message = BitmexAuthentication {
                op: BitmexWsAuthAction::AuthKeyExpires,
                args: (cred.api_key.to_string(), expires, signature),
            };

            // Verify auth message structure
            assert_eq!(auth_message.op, BitmexWsAuthAction::AuthKeyExpires);
            assert_eq!(auth_message.args.0, "test_key");
            assert!(auth_message.args.1 > 0); // expires should be positive
            assert!(!auth_message.args.2.is_empty()); // signature should exist
        } else {
            panic!("Client should have credentials");
        }

        // Test without credentials
        let client_no_creds = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            None,
            None,
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        assert!(client_no_creds.credential.is_none());
    }

    #[rstest]
    fn test_subscription_state_after_unsubscribe() {
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Set up initial subscriptions
        let subs = client.subscriptions.confirmed();
        subs.insert(Ustr::from(BitmexWsTopic::Trade.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set.insert(Ustr::from("ETHUSD"));
            set
        });

        subs.insert(Ustr::from(BitmexWsTopic::OrderBookL2.as_ref()), {
            let mut set = AHashSet::new();
            set.insert(Ustr::from("XBTUSD"));
            set
        });

        // Simulate unsubscribe logic (like from unsubscribe() method lines 586-599)
        let topic = format!("{}:ETHUSD", BitmexWsTopic::Trade.as_ref());
        if let Some((channel, symbol)) = topic.split_once(':')
            && let Some(mut entry) = subs.get_mut(&Ustr::from(channel))
        {
            entry.remove(&Ustr::from(symbol));
            if entry.is_empty() {
                drop(entry);
                subs.remove(&Ustr::from(channel));
            }
        }

        // Build restoration topics after unsubscribe
        let mut topics_to_restore = Vec::new();
        for entry in subs.iter() {
            let (channel, symbols) = entry.pair();
            for symbol in symbols.iter() {
                if symbol.is_empty() {
                    topics_to_restore.push(channel.to_string());
                } else {
                    topics_to_restore.push(format!("{channel}:{symbol}"));
                }
            }
        }

        // Should have XBTUSD trade but not ETHUSD trade
        let trade_xbt = format!("{}:XBTUSD", BitmexWsTopic::Trade.as_ref());
        let trade_eth = format!("{}:ETHUSD", BitmexWsTopic::Trade.as_ref());
        let book_xbt = format!("{}:XBTUSD", BitmexWsTopic::OrderBookL2.as_ref());

        assert!(topics_to_restore.contains(&trade_xbt));
        assert!(!topics_to_restore.contains(&trade_eth));
        assert!(topics_to_restore.contains(&book_xbt));
        assert_eq!(topics_to_restore.len(), 2);
    }

    #[rstest]
    fn test_race_unsubscribe_failure_recovery() {
        // Simulates the race condition where venue rejects an unsubscribe request.
        // The adapter must perform the 3-step recovery:
        // 1. confirm_unsubscribe() - clear pending_unsubscribe
        // 2. mark_subscribe() - mark as subscribing again
        // 3. confirm_subscribe() - restore to confirmed state
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            None,
            None,
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        let topic = format!("{}:XBTUSD", BitmexWsTopic::Trade.as_ref());

        // Initial subscribe flow
        client.subscriptions.mark_subscribe(&topic);
        client.subscriptions.confirm_subscribe(&topic);
        assert_eq!(client.subscriptions.len(), 1);

        // User unsubscribes
        client.subscriptions.mark_unsubscribe(&topic);
        assert_eq!(client.subscriptions.len(), 0);
        assert_eq!(
            client.subscriptions.pending_unsubscribe_topics(),
            vec![topic.clone()]
        );

        // Venue REJECTS the unsubscribe (error message)
        // Adapter must perform 3-step recovery (from lines 1884-1891)
        client.subscriptions.confirm_unsubscribe(&topic); // Step 1: clear pending_unsubscribe
        client.subscriptions.mark_subscribe(&topic); // Step 2: mark as subscribing
        client.subscriptions.confirm_subscribe(&topic); // Step 3: confirm subscription

        // Verify recovery: topic should be back in confirmed state
        assert_eq!(client.subscriptions.len(), 1);
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
        assert!(client.subscriptions.pending_subscribe_topics().is_empty());

        // Verify topic is in all_topics() for reconnect
        let all = client.subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic));
    }

    #[rstest]
    fn test_race_resubscribe_before_unsubscribe_ack() {
        // Simulates: User unsubscribes, then immediately resubscribes before
        // the unsubscribe ACK arrives from the venue.
        // This is the race condition fixed in the subscription tracker.
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            None,
            None,
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        let topic = format!("{}:XBTUSD", BitmexWsTopic::OrderBookL2.as_ref());

        // Initial subscribe
        client.subscriptions.mark_subscribe(&topic);
        client.subscriptions.confirm_subscribe(&topic);
        assert_eq!(client.subscriptions.len(), 1);

        // User unsubscribes
        client.subscriptions.mark_unsubscribe(&topic);
        assert_eq!(client.subscriptions.len(), 0);
        assert_eq!(
            client.subscriptions.pending_unsubscribe_topics(),
            vec![topic.clone()]
        );

        // User immediately changes mind and resubscribes (before unsubscribe ACK)
        client.subscriptions.mark_subscribe(&topic);
        assert_eq!(
            client.subscriptions.pending_subscribe_topics(),
            vec![topic.clone()]
        );

        // NOW the unsubscribe ACK arrives - should NOT clear pending_subscribe
        client.subscriptions.confirm_unsubscribe(&topic);
        assert!(client.subscriptions.pending_unsubscribe_topics().is_empty());
        assert_eq!(
            client.subscriptions.pending_subscribe_topics(),
            vec![topic.clone()]
        ); // CRITICAL

        // Subscribe ACK arrives
        client.subscriptions.confirm_subscribe(&topic);
        assert_eq!(client.subscriptions.len(), 1);
        assert!(client.subscriptions.pending_subscribe_topics().is_empty());

        // Verify final state is correct
        let all = client.subscriptions.all_topics();
        assert_eq!(all.len(), 1);
        assert!(all.contains(&topic));
    }

    #[rstest]
    fn test_race_channel_level_reconnection_with_pending_states() {
        // Simulates reconnection with mixed pending states including channel-level subscriptions.
        let client = BitmexWebSocketClient::new(
            Some("ws://test.com".to_string()),
            Some("test_key".to_string()),
            Some("test_secret".to_string()),
            Some(AccountId::new("BITMEX-TEST")),
            None,
        )
        .unwrap();

        // Set up mixed state before reconnection
        // Confirmed: trade:XBTUSD
        let trade_xbt = format!("{}:XBTUSD", BitmexWsTopic::Trade.as_ref());
        client.subscriptions.mark_subscribe(&trade_xbt);
        client.subscriptions.confirm_subscribe(&trade_xbt);

        // Confirmed: order (channel-level, no symbol)
        let order_channel = BitmexWsAuthChannel::Order.as_ref();
        client.subscriptions.mark_subscribe(order_channel);
        client.subscriptions.confirm_subscribe(order_channel);

        // Pending subscribe: trade:ETHUSD
        let trade_eth = format!("{}:ETHUSD", BitmexWsTopic::Trade.as_ref());
        client.subscriptions.mark_subscribe(&trade_eth);

        // Pending unsubscribe: orderBookL2:XBTUSD (user cancelled)
        let book_xbt = format!("{}:XBTUSD", BitmexWsTopic::OrderBookL2.as_ref());
        client.subscriptions.mark_subscribe(&book_xbt);
        client.subscriptions.confirm_subscribe(&book_xbt);
        client.subscriptions.mark_unsubscribe(&book_xbt);

        // Get topics for reconnection
        let topics_to_restore = client.subscriptions.all_topics();

        // Should include: confirmed + pending_subscribe (NOT pending_unsubscribe)
        assert_eq!(topics_to_restore.len(), 3);
        assert!(topics_to_restore.contains(&trade_xbt));
        assert!(topics_to_restore.contains(&order_channel.to_string()));
        assert!(topics_to_restore.contains(&trade_eth));
        assert!(!topics_to_restore.contains(&book_xbt)); // Excluded

        // Verify channel-level marker is handled correctly
        // order channel should not have ':' delimiter
        for topic in &topics_to_restore {
            if topic == order_channel {
                assert!(
                    !topic.contains(':'),
                    "Channel-level topic should not have delimiter"
                );
            }
        }
    }
}
