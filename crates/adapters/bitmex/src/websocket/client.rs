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
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use futures_util::Stream;
use nautilus_common::live::runtime::get_runtime;
use nautilus_core::{
    consts::NAUTILUS_USER_AGENT,
    env::{get_env_var, get_or_env_var_opt},
};
use nautilus_model::{
    data::bar::BarType,
    enums::OrderType,
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AUTHENTICATION_TIMEOUT_SECS, AuthTracker, PingHandler, SubscriptionState, WebSocketClient,
        WebSocketConfig, channel_message_handler,
    },
};
use reqwest::header::USER_AGENT;
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
    account_id: AccountId,
    auth_tracker: AuthTracker,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    tracked_subscriptions: Arc<DashMap<String, ()>>,
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

        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        // We don't have a handler yet; this placeholder keeps cache_instrument() working,
        // connect() swaps in the real channel and replays any queued instruments so the
        // handler sees them once it starts.
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        Ok(Self {
            url: url.unwrap_or(BITMEX_WS_URL.to_string()),
            credential,
            heartbeat,
            account_id,
            auth_tracker: AuthTracker::new(),
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: SubscriptionState::new(BITMEX_WS_TOPIC_DELIMITER),
            tracked_subscriptions: Arc::new(DashMap::new()),
            instruments_cache: Arc::new(DashMap::new()),
            order_type_cache: Arc::new(DashMap::new()),
            order_symbol_cache: Arc::new(DashMap::new()),
        })
    }

    /// Creates a new [`BitmexWebSocketClient`] with environment variable credential resolution.
    ///
    /// If `api_key` or `api_secret` are not provided, they will be loaded from
    /// environment variables based on the `testnet` flag:
    /// - Testnet: `BITMEX_TESTNET_API_KEY`, `BITMEX_TESTNET_API_SECRET`
    /// - Mainnet: `BITMEX_API_KEY`, `BITMEX_API_SECRET`
    ///
    /// # Errors
    ///
    /// Returns an error if only one of `api_key` or `api_secret` is provided.
    pub fn new_with_env(
        url: Option<String>,
        api_key: Option<String>,
        api_secret: Option<String>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        testnet: bool,
    ) -> anyhow::Result<Self> {
        let (api_key_env, api_secret_env) = if testnet {
            ("BITMEX_TESTNET_API_KEY", "BITMEX_TESTNET_API_SECRET")
        } else {
            ("BITMEX_API_KEY", "BITMEX_API_SECRET")
        };

        let key = get_or_env_var_opt(api_key, api_key_env);
        let secret = get_or_env_var_opt(api_secret, api_secret_env);

        Self::new(url, key, secret, account_id, heartbeat)
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

    /// Returns a masked version of the API key for logging purposes.
    #[must_use]
    pub fn api_key_masked(&self) -> Option<String> {
        self.credential.as_ref().map(|c| c.api_key_masked())
    }

    /// Returns a value indicating whether the client is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_active()
            && !self.signal.load(Ordering::Relaxed)
    }

    /// Returns a value indicating whether the client is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        let connection_mode_arc = self.connection_mode.load();
        ConnectionMode::from_atomic(&connection_mode_arc).is_closed()
            || self.signal.load(Ordering::Relaxed)
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
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::UpdateInstrument(instrument))
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
        let (client, raw_rx) = self.connect_inner().await?;

        // Replace connection state so all clones see the underlying WebSocketClient's state
        self.connection_mode.store(client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        // Send WebSocketClient to handler
        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(client)) {
            return Err(BitmexWsError::ClientError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

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
        let credential = self.credential.clone();
        let auth_tracker = self.auth_tracker.clone();
        let subscriptions = self.subscriptions.clone();
        let order_type_cache = self.order_type_cache.clone();
        let order_symbol_cache = self.order_symbol_cache.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                signal.clone(),
                cmd_rx,
                raw_rx,
                out_tx,
                account_id,
                auth_tracker.clone(),
                subscriptions.clone(),
                order_type_cache,
                order_symbol_cache,
            );

            // Helper closure to resubscribe all tracked subscriptions after reconnection
            let resubscribe_all = || {
                // Use SubscriptionState as source of truth for what to restore
                let topics = subscriptions.all_topics();

                if topics.is_empty() {
                    return;
                }

                tracing::debug!(count = topics.len(), "Resubscribing to confirmed subscriptions");

                for topic in &topics {
                    subscriptions.mark_subscribe(topic.as_str());
                }

                // Serialize subscription messages
                let mut payloads = Vec::with_capacity(topics.len());
                for topic in &topics {
                    let message = BitmexSubscription {
                        op: BitmexWsOperation::Subscribe,
                        args: vec![Ustr::from(topic.as_ref())],
                    };
                    if let Ok(payload) = serde_json::to_string(&message) {
                        payloads.push(payload);
                    }
                }

                if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Subscribe { topics: payloads }) {
                    tracing::error!(error = %e, "Failed to send resubscribe command");
                }
            };

            // Run message processing with reconnection handling
            loop {
                match handler.next().await {
                    Some(NautilusWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }

                        log::info!("WebSocket reconnected");

                        // Mark all confirmed subscriptions as failed so they transition to pending state
                        let confirmed_topics: Vec<String> = {
                            let confirmed = subscriptions.confirmed();
                            let mut topics = Vec::new();

                            for entry in confirmed.iter() {
                                let (channel, symbols) = entry.pair();

                                if *channel == BitmexWsTopic::Instrument.as_ref() {
                                    continue;
                                }

                                for symbol in symbols {
                                    if symbol.is_empty() {
                                        topics.push(channel.to_string());
                                    } else {
                                        topics.push(format!("{channel}:{symbol}"));
                                    }
                                }
                            }

                            topics
                        };

                        if !confirmed_topics.is_empty() {
                            tracing::debug!(count = confirmed_topics.len(), "Marking confirmed subscriptions as pending for replay");
                            for topic in confirmed_topics {
                                subscriptions.mark_failure(&topic);
                            }
                        }

                        if let Some(cred) = &credential {
                            tracing::debug!("Re-authenticating after reconnection");

                            let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
                            let signature = cred.sign("GET", "/realtime", expires, "");

                            let auth_message = BitmexAuthentication {
                                op: BitmexWsAuthAction::AuthKeyExpires,
                                args: (cred.api_key.to_string(), expires, signature),
                            };

                            if let Ok(payload) = serde_json::to_string(&auth_message) {
                                if let Err(e) = cmd_tx_for_reconnect.send(HandlerCommand::Authenticate { payload }) {
                                    tracing::error!(error = %e, "Failed to send reconnection auth command");
                                }
                            } else {
                                tracing::error!("Failed to serialize reconnection auth message");
                            }
                        }

                        // Unauthenticated sessions resubscribe immediately after reconnection,
                        // authenticated sessions wait for Authenticated message
                        if credential.is_none() {
                            tracing::debug!("No authentication required, resubscribing immediately");
                            resubscribe_all();
                        }

                        // TODO: Implement proper Reconnected event forwarding to consumers.
                        // Currently intercepted for internal housekeeping only. Will add new
                        // message type from WebSocketClient to notify consumers of reconnections.

                        continue;
                    }
                    Some(NautilusWsMessage::Authenticated) => {
                        tracing::debug!("Authenticated after reconnection, resubscribing");
                        resubscribe_all();
                        continue;
                    }
                    Some(msg) => {
                        if handler.send(msg).is_err() {
                            tracing::error!("Failed to send message (receiver dropped)");
                            break;
                        }
                    }
                    None => {
                        // Stream ended - check if it's a stop signal
                        if handler.is_stopped() {
                            tracing::debug!("Stop signal received, ending message processing");
                            break;
                        }
                        // Otherwise it's an unexpected stream end
                        tracing::warn!("WebSocket stream ended unexpectedly");
                        break;
                    }
                }
            }

            tracing::debug!("Handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        if self.credential.is_some()
            && let Err(e) = self.authenticate().await
        {
            return Err(e);
        }

        // Subscribe to instrument topic
        let instrument_topic = BitmexWsTopic::Instrument.as_ref().to_string();
        self.subscriptions.mark_subscribe(&instrument_topic);
        self.tracked_subscriptions.insert(instrument_topic, ());

        let subscribe_msg = BitmexSubscription {
            op: BitmexWsOperation::Subscribe,
            args: vec![Ustr::from(BitmexWsTopic::Instrument.as_ref())],
        };

        match serde_json::to_string(&subscribe_msg) {
            Ok(subscribe_json) => {
                if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Subscribe {
                    topics: vec![subscribe_json],
                }) {
                    log::error!("Failed to send subscribe command for instruments: {e}");
                } else {
                    log::debug!("Subscribed to all instruments");
                }
            }
            Err(e) => {
                tracing::error!(error = %e, "Failed to serialize subscribe message");
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
    ) -> Result<
        (
            WebSocketClient,
            tokio::sync::mpsc::UnboundedReceiver<Message>,
        ),
        BitmexWsError,
    > {
        let (message_handler, rx) = channel_message_handler();

        // No-op ping handler: handler owns the WebSocketClient and responds to pings directly
        // in the message loop for minimal latency (see handler.rs pong response)
        let ping_handler: PingHandler = Arc::new(move |_payload: Vec<u8>| {
            // Handler responds to pings internally via select! loop
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
            reconnect_max_attempts: None,
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

        Ok((client, rx))
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

        let receiver = self.auth_tracker.begin();

        let expires = (chrono::Utc::now() + chrono::Duration::seconds(30)).timestamp();
        let signature = credential.sign("GET", "/realtime", expires, "");

        let auth_message = BitmexAuthentication {
            op: BitmexWsAuthAction::AuthKeyExpires,
            args: (credential.api_key.to_string(), expires, signature),
        };

        let auth_json = serde_json::to_string(&auth_message).map_err(|e| {
            let msg = format!("Failed to serialize auth message: {e}");
            self.auth_tracker.fail(msg.clone());
            BitmexWsError::AuthenticationError(msg)
        })?;

        // Send Authenticate command to handler
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::Authenticate { payload: auth_json })
            .map_err(|e| {
                let msg = format!("Failed to send authenticate command: {e}");
                self.auth_tracker.fail(msg.clone());
                BitmexWsError::AuthenticationError(msg)
            })?;

        self.auth_tracker
            .wait_for_result::<BitmexWsError>(
                Duration::from_secs(AUTHENTICATION_TIMEOUT_SECS),
                receiver,
            )
            .await
    }

    /// Wait until the WebSocket connection is active.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection times out.
    pub async fn wait_until_active(&self, timeout_secs: f64) -> Result<(), BitmexWsError> {
        let timeout = Duration::from_secs_f64(timeout_secs);

        tokio::time::timeout(timeout, async {
            while !self.is_active() {
                tokio::time::sleep(Duration::from_millis(10)).await;
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

        // Send Disconnect command to handler
        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            log::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
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
            self.tracked_subscriptions.insert(topic.clone(), ());
        }

        // Serialize subscription messages
        let mut payloads = Vec::with_capacity(topics.len());
        for topic in &topics {
            let message = BitmexSubscription {
                op: BitmexWsOperation::Subscribe,
                args: vec![Ustr::from(topic.as_ref())],
            };
            let payload = serde_json::to_string(&message).map_err(|e| {
                BitmexWsError::SubscriptionError(format!("Failed to serialize subscription: {e}"))
            })?;
            payloads.push(payload);
        }

        // Send Subscribe command to handler
        let cmd = HandlerCommand::Subscribe { topics: payloads };

        self.send_cmd(cmd).await.map_err(|e| {
            BitmexWsError::SubscriptionError(format!("Failed to send subscribe command: {e}"))
        })
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
            self.tracked_subscriptions.remove(topic);
        }

        // Serialize unsubscription messages
        let mut payloads = Vec::with_capacity(topics.len());
        for topic in &topics {
            let message = BitmexSubscription {
                op: BitmexWsOperation::Unsubscribe,
                args: vec![Ustr::from(topic.as_ref())],
            };
            if let Ok(payload) = serde_json::to_string(&message) {
                payloads.push(payload);
            }
        }

        // Send Unsubscribe command to handler
        let cmd = HandlerCommand::Unsubscribe { topics: payloads };

        if let Err(e) = self.send_cmd(cmd).await {
            tracing::debug!(error = %e, "Failed to send unsubscribe command");
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

    /// Sends a command to the handler.
    async fn send_cmd(&self, cmd: HandlerCommand) -> Result<(), BitmexWsError> {
        self.cmd_tx
            .read()
            .await
            .send(cmd)
            .map_err(|e| BitmexWsError::ClientError(format!("Handler not available: {e}")))
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
            for symbol in symbols {
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
            for symbol in symbols {
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
        );

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
