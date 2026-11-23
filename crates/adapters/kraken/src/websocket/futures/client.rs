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

//! WebSocket client for the Kraken Futures v1 streaming API.

use std::{
    collections::HashMap,
    sync::{
        Arc,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use arc_swap::ArcSwap;
#[cfg(test)]
use nautilus_core::string::secret::REDACTED;
use nautilus_core::{
    AtomicMap,
    string::secret::{SecretString, zeroize_json_value},
};
use nautilus_live::{
    SocketControl,
    book::recovery::BookRecovery,
    task::{TaskGroup, TaskShutdownError},
};
use nautilus_model::{
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    ConnectionUrl,
    http::create_standard_nautilus_headers,
    mode::ConnectionMode,
    websocket::{
        AUTHENTICATION_TIMEOUT_SECS, AuthTracker, SubscriptionState, TransportBackend,
        WebSocketClient, WebSocketConfig, channel_epoch_message_handler,
    },
};
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;
use zeroize::Zeroizing;

use super::{
    book::{BookCommand, BookMessage},
    handler::{FuturesFeedHandler, FuturesHandlerCommand},
    messages::{
        KrakenFuturesChallengeRequest, KrakenFuturesEvent, KrakenFuturesFeed,
        KrakenFuturesPrivateSubscribeRequest, KrakenFuturesRequest, KrakenFuturesWsMessage,
    },
};
use crate::{
    common::{
        consts::{KRAKEN_FUTURES_WS_SUBSCRIPTION_QUOTA, KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION},
        credential::KrakenCredential,
        parse::truncate_cl_ord_id,
    },
    websocket::error::KrakenWsError,
};

/// Topic delimiter for Kraken Futures WebSocket subscriptions.
///
/// Topics use colon format: `feed:symbol` (e.g., `trades:PF_ETHUSD`).
pub const KRAKEN_FUTURES_WS_TOPIC_DELIMITER: char = ':';

/// WebSocket client for the Kraken Futures v1 streaming API.
#[derive(Debug)]
pub struct KrakenFuturesWebSocketClient {
    url: ConnectionUrl,
    heartbeat_secs: u64,
    auth_timeout_secs: u64,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<FuturesHandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<KrakenFuturesWsMessage>>>,
    book_out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<BookMessage>>,
    book_cmd_tx: Arc<RwLock<Option<tokio::sync::mpsc::UnboundedSender<BookCommand>>>>,
    handler_tasks: Arc<TaskGroup>,
    connect_lock: Arc<tokio::sync::Mutex<()>>,
    subscriptions: SubscriptionState,
    subscription_payloads: Arc<tokio::sync::RwLock<HashMap<String, SecretString>>>,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    credential: Option<KrakenCredential>,
    original_challenge: Arc<tokio::sync::RwLock<Option<SecretString>>>,
    signed_challenge: Arc<tokio::sync::RwLock<Option<SecretString>>>,
    account_id: Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: Arc<AtomicMap<String, InstrumentId>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    transport_backend: TransportBackend,
    proxy_url: Option<SecretString>,
    socket_control: Option<SocketControl>,
}

impl Clone for KrakenFuturesWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            heartbeat_secs: self.heartbeat_secs,
            auth_timeout_secs: self.auth_timeout_secs,
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: self.out_rx.clone(),
            book_out_rx: None,
            book_cmd_tx: Arc::clone(&self.book_cmd_tx),
            handler_tasks: Arc::clone(&self.handler_tasks),
            connect_lock: Arc::clone(&self.connect_lock),
            subscriptions: self.subscriptions.clone(),
            subscription_payloads: Arc::clone(&self.subscription_payloads),
            auth_tracker: self.auth_tracker.clone(),
            cancellation_token: self.cancellation_token.clone(),
            credential: self.credential.clone(),
            original_challenge: Arc::clone(&self.original_challenge),
            signed_challenge: Arc::clone(&self.signed_challenge),
            account_id: Arc::clone(&self.account_id),
            truncated_id_map: Arc::clone(&self.truncated_id_map),
            order_instrument_map: Arc::clone(&self.order_instrument_map),
            instruments: Arc::clone(&self.instruments),
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
            socket_control: self.socket_control.clone(),
        }
    }
}

impl KrakenFuturesWebSocketClient {
    /// Creates a new client with the given URL.
    #[must_use]
    pub fn new(url: String, heartbeat_secs: u64, proxy_url: Option<String>) -> Self {
        Self::with_credentials(
            url,
            heartbeat_secs,
            None,
            None,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Creates a new client with API credentials for authenticated feeds.
    #[must_use]
    pub fn with_credentials(
        url: String,
        heartbeat_secs: u64,
        credential: Option<KrakenCredential>,
        auth_timeout_secs: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url: url.into(),
            heartbeat_secs,
            auth_timeout_secs: auth_timeout_secs.unwrap_or(AUTHENTICATION_TIMEOUT_SECS),
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            book_out_rx: None,
            book_cmd_tx: Arc::new(RwLock::new(None)),
            handler_tasks: Arc::new(TaskGroup::new()),
            connect_lock: Arc::new(tokio::sync::Mutex::new(())),
            subscriptions: SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER),
            subscription_payloads: Arc::new(tokio::sync::RwLock::new(HashMap::new())),
            auth_tracker: AuthTracker::new(),
            cancellation_token: CancellationToken::new(),
            credential,
            original_challenge: Arc::new(tokio::sync::RwLock::new(None)),
            signed_challenge: Arc::new(tokio::sync::RwLock::new(None)),
            account_id: Arc::new(RwLock::new(None)),
            truncated_id_map: Arc::new(AtomicMap::new()),
            order_instrument_map: Arc::new(AtomicMap::new()),
            instruments: Arc::new(AtomicMap::new()),
            transport_backend,
            proxy_url: proxy_url.map(SecretString::from),
            socket_control: None,
        }
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

    /// Returns true if the client has API credentials set.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        self.url.expose()
    }

    /// Returns true if the connection is closed.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        ConnectionMode::from_u8(self.connection_mode.load().load(Ordering::Relaxed))
            == ConnectionMode::Closed
    }

    /// Returns true if the connection is active.
    #[must_use]
    pub fn is_active(&self) -> bool {
        ConnectionMode::from_u8(self.connection_mode.load().load(Ordering::Relaxed))
            == ConnectionMode::Active
    }

    /// Waits until the WebSocket connection is active or timeout.
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

    /// Returns true if the WebSocket is authenticated for private feeds.
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

    /// Authenticates the WebSocket connection for private feeds.
    ///
    /// Sends a challenge request and waits for the handler to parse the response,
    /// sign it, and mark the `AuthTracker` successful. Private subscriptions gate
    /// on the stored challenge / signed-challenge pair.
    pub async fn authenticate(&self) -> Result<(), KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        let payload = build_challenge_payload(credential)
            .map_err(|e| KrakenWsError::JsonError(e.to_string()))?;

        let receiver = self.auth_tracker.begin();

        self.cmd_tx
            .read()
            .await
            .send(FuturesHandlerCommand::RequestChallenge { payload })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.auth_tracker
            .wait_for_result::<KrakenWsError>(
                tokio::time::Duration::from_secs(self.auth_timeout_secs),
                receiver,
            )
            .await?;

        log::debug!("Futures WebSocket authentication successful");
        Ok(())
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        self.connect_inner(false).await
    }

    pub(crate) async fn connect_books(&mut self) -> Result<(), KrakenWsError> {
        self.connect_inner(true).await
    }

    async fn connect_inner(&mut self, manage_books: bool) -> Result<(), KrakenWsError> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;

        log::debug!("Connecting to Futures WebSocket: {}", self.url.origin());

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
            heartbeat_interval_secs: Some(self.heartbeat_secs),
            heartbeat_payload: None, // Use WebSocket ping frames, not text messages
            connect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: None,
            idle_timeout_ms: None,
            writer_capacity: None,
            backend: self.transport_backend,
            proxy_url: self
                .proxy_url
                .as_ref()
                .map(|value| value.expose_secret().to_owned()),
            max_message_size_bytes: None,
            max_frame_size_bytes: None,
        };

        let keyed_quotas = vec![(
            KRAKEN_RATE_LIMIT_KEY_SUBSCRIPTION[0].to_string(),
            *KRAKEN_FUTURES_WS_SUBSCRIPTION_QUOTA,
        )];

        let ws_client = WebSocketClient::epoch_builder()
            .config(ws_config)
            .epoch_handler(raw_handler)
            .keyed_quotas(keyed_quotas)
            .maybe_state_sink(self.socket_control.as_ref().map(SocketControl::sink))
            .connect()
            .await
            .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        let connection_mode = ws_client.connection_mode_atomic();
        let reconnect_handle = ws_client.reconnect_handle();

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel();
        let (book_tx, book_rx) = tokio::sync::mpsc::unbounded_channel();
        let (book_cmd_tx, book_cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        self.out_rx = (!manage_books).then_some(Arc::new(out_rx));
        self.book_out_rx = manage_books.then_some(book_rx);
        *self.book_cmd_tx.write() = manage_books.then_some(book_cmd_tx);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();

        if let Err(e) = cmd_tx.send(FuturesHandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        {
            let mut command_sender = self.cmd_tx.write().await;
            *command_sender = cmd_tx.clone();
            self.connection_mode.store(connection_mode);
        }

        if let Some(control) = &self.socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();
        let subscription_payloads = self.subscription_payloads.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let credential_for_reconnect = self.credential.clone();
        let original_challenge_for_reconnect = self.original_challenge.clone();
        let signed_challenge_for_reconnect = self.signed_challenge.clone();
        let auth_tracker_for_reconnect = self.auth_tracker.clone();

        let handler_task = async move {
            let mut handler = FuturesFeedHandler::with_epochs(
                signal.clone(),
                cmd_rx,
                raw_rx,
                subscriptions.clone(),
            );

            if manage_books {
                handler.manage_books(book_cmd_rx, book_tx.clone());
            }

            let send = |msg, stream| {
                if manage_books {
                    book_tx.send(BookMessage::Data(msg, stream)).is_ok()
                } else {
                    out_tx.send(msg).is_ok()
                }
            };

            let mut pending_resubscribe = false;

            loop {
                match handler.next().await {
                    Some(KrakenFuturesWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        log::info!("WebSocket reconnected");

                        subscriptions.reset_after_reconnect();

                        auth_tracker_for_reconnect.invalidate();
                        *original_challenge_for_reconnect.write().await = None;
                        *signed_challenge_for_reconnect.write().await = None;

                        let payloads = subscription_payloads.read().await.clone();

                        // Resubscribe public topics straight away; they don't depend on auth,
                        // so don't tie their restoration to the challenge outcome.
                        resubscribe_public(&cmd_tx_for_reconnect, &subscriptions, &payloads);

                        let has_private =
                            payloads.keys().any(|k| k == "open_orders" || k == "fills");

                        pending_resubscribe = false;

                        if has_private {
                            if let Some(ref cred) = credential_for_reconnect {
                                match build_challenge_payload(cred) {
                                    Ok(payload) => {
                                        let _rx = auth_tracker_for_reconnect.begin();

                                        if let Err(e) = cmd_tx_for_reconnect.send(
                                            FuturesHandlerCommand::RequestChallenge { payload },
                                        ) {
                                            log::error!("Failed to queue reconnect challenge: {e}");
                                        } else {
                                            pending_resubscribe = true;
                                        }
                                    }
                                    Err(e) => {
                                        log::error!("Failed to serialize reconnect challenge: {e}");
                                    }
                                }
                            } else {
                                log::warn!(
                                    "Private subscriptions exist but no credentials available"
                                );
                            }
                        }

                        if !send(KrakenFuturesWsMessage::Reconnected, None) {
                            log::debug!("Output channel closed");
                            break;
                        }
                    }
                    Some(KrakenFuturesWsMessage::Challenge(challenge)) => {
                        let Some(ref cred) = credential_for_reconnect else {
                            log::warn!("Challenge received but no credentials configured");
                            auth_tracker_for_reconnect.fail("no credentials");
                            continue;
                        };

                        let challenge = SecretString::from(challenge);

                        match cred
                            .sign_ws_challenge(challenge.expose_secret())
                            .map(SecretString::from)
                        {
                            Ok(signed) => {
                                *original_challenge_for_reconnect.write().await =
                                    Some(challenge.clone());
                                *signed_challenge_for_reconnect.write().await =
                                    Some(signed.clone());
                                auth_tracker_for_reconnect.succeed();
                                log::debug!("Signed WebSocket challenge");

                                if pending_resubscribe {
                                    let payloads = subscription_payloads.read().await;
                                    resubscribe_private(
                                        &cmd_tx_for_reconnect,
                                        &subscriptions,
                                        &payloads,
                                        cred,
                                        challenge.expose_secret(),
                                        signed.expose_secret(),
                                    );
                                    pending_resubscribe = false;
                                }
                            }
                            Err(e) => {
                                log::error!("Failed to sign challenge: {e}");
                                auth_tracker_for_reconnect.fail(e.to_string());
                                pending_resubscribe = false;
                            }
                        }
                    }
                    Some(msg) => {
                        let stream = handler.book_stream(&msg);
                        if !send(msg, stream) {
                            log::debug!("Output channel closed");
                            break;
                        }
                    }
                    None => {
                        log::debug!("Handler stream ended");
                        break;
                    }
                }
            }

            log::debug!("Futures handler task exiting");
        };

        if let Err(e) = handler_spawner.spawn(handler_task) {
            if let Some(control) = &self.socket_control {
                control.deregister();
            }
            self.out_rx = None;
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to register WebSocket handler task: {e}"
            )));
        }

        log::debug!("Futures WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;
        self.disconnect_locked().await
    }

    async fn disconnect_locked(&self) -> Result<(), KrakenWsError> {
        log::debug!("Disconnecting Futures WebSocket");

        self.handler_tasks.begin_shutdown();
        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self
            .cmd_tx
            .read()
            .await
            .send(FuturesHandlerCommand::Disconnect)
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
        self.book_cmd_tx.write().take();
        self.subscription_payloads.write().await.clear();
        self.auth_tracker.fail("Disconnected");

        if let Some(control) = &self.socket_control {
            control.deregister();
        }

        match task_result {
            Ok(()) => Ok(()),
            Err(error @ TaskShutdownError::Timeout { .. }) => Err(KrakenWsError::Timeout(format!(
                "Futures WebSocket handler shutdown timed out: {error}"
            ))),
            Err(e) => Err(KrakenWsError::Disconnected(format!(
                "Futures WebSocket handler shutdown failed: {e}"
            ))),
        }
    }

    /// Closes the WebSocket connection.
    pub async fn close(&mut self) -> Result<(), KrakenWsError> {
        self.disconnect().await
    }

    /// Subscribes to mark price updates for the given instrument.
    pub async fn subscribe_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("mark:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        self.subscriptions.confirm_subscribe(&key);
        self.ensure_ticker_subscribed(symbol).await
    }

    /// Unsubscribes from mark price updates for the given instrument.
    pub async fn unsubscribe_mark_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("mark:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);
        self.subscriptions.confirm_unsubscribe(&key);
        self.maybe_unsubscribe_ticker(symbol).await
    }

    /// Subscribes to index price updates for the given instrument.
    pub async fn subscribe_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("index:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        self.subscriptions.confirm_subscribe(&key);
        self.ensure_ticker_subscribed(symbol).await
    }

    /// Unsubscribes from index price updates for the given instrument.
    pub async fn unsubscribe_index_price(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("index:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);
        self.subscriptions.confirm_unsubscribe(&key);
        self.maybe_unsubscribe_ticker(symbol).await
    }

    /// Subscribes to funding rate updates for the given instrument.
    pub async fn subscribe_funding_rate(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("funding:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        self.subscriptions.confirm_subscribe(&key);
        self.ensure_ticker_subscribed(symbol).await
    }

    /// Unsubscribes from funding rate updates for the given instrument.
    pub async fn unsubscribe_funding_rate(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("funding:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);
        self.subscriptions.confirm_unsubscribe(&key);
        self.maybe_unsubscribe_ticker(symbol).await
    }

    /// Subscribes to quote updates for the given instrument.
    ///
    /// Uses the order book channel for low-latency top-of-book quotes.
    pub async fn subscribe_quotes(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("quotes:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        self.subscriptions.confirm_subscribe(&key);

        // Use book feed for low-latency quotes (not throttled ticker)
        self.ensure_book_subscribed(symbol).await
    }

    /// Unsubscribes from quote updates for the given instrument.
    pub async fn unsubscribe_quotes(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("quotes:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);
        self.subscriptions.confirm_unsubscribe(&key);
        self.maybe_unsubscribe_book(symbol).await
    }

    /// Subscribes to trade updates for the given instrument.
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("trades:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);
        let payload = self
            .send_subscribe_feed(KrakenFuturesFeed::Trade, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_subscribe(&key);
        self.subscription_payloads
            .write()
            .await
            .insert(key, payload);
        Ok(())
    }

    /// Unsubscribes from trade updates for the given instrument.
    pub async fn unsubscribe_trades(
        &self,
        instrument_id: InstrumentId,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("trades:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);
        self.send_unsubscribe_feed(KrakenFuturesFeed::Trade, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_unsubscribe(&key);
        self.subscription_payloads.write().await.remove(&key);
        Ok(())
    }

    /// Subscribes to order book updates for the given instrument.
    ///
    /// Note: The `depth` parameter is accepted for API compatibility with spot client but is
    /// not used by Kraken Futures (full book is always returned).
    pub async fn subscribe_book(
        &self,
        instrument_id: InstrumentId,
        _depth: Option<u32>,
    ) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;

        let deltas_key = format!("deltas:{symbol}");
        self.subscriptions.add_reference(&deltas_key);
        self.subscriptions.mark_subscribe(&deltas_key);
        self.subscriptions.confirm_subscribe(&deltas_key);

        self.ensure_book_subscribed(symbol).await
    }

    /// Unsubscribes from order book updates for the given instrument.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;

        let deltas_key = format!("deltas:{symbol}");
        self.subscriptions.remove_reference(&deltas_key);
        self.subscriptions.mark_unsubscribe(&deltas_key);
        self.subscriptions.confirm_unsubscribe(&deltas_key);

        self.maybe_unsubscribe_book(symbol).await
    }

    async fn ensure_ticker_subscribed(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let ticker_key = format!("ticker:{symbol}");

        if !self.subscriptions.add_reference(&ticker_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&ticker_key);
        let payload = self
            .send_subscribe_feed(KrakenFuturesFeed::Ticker, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_subscribe(&ticker_key);
        self.subscription_payloads
            .write()
            .await
            .insert(ticker_key, payload);
        Ok(())
    }

    async fn maybe_unsubscribe_ticker(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let ticker_key = format!("ticker:{symbol}");

        if !self.subscriptions.remove_reference(&ticker_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&ticker_key);
        self.send_unsubscribe_feed(KrakenFuturesFeed::Ticker, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_unsubscribe(&ticker_key);
        self.subscription_payloads.write().await.remove(&ticker_key);
        Ok(())
    }

    pub(crate) async fn replace_book(
        &self,
        instrument_id: InstrumentId,
        generation: u64,
        cancel: CancellationToken,
        recovery: Arc<BookRecovery<KrakenWsError>>,
    ) -> Result<(), KrakenWsError> {
        let (result, response) = tokio::sync::oneshot::channel();
        self.send_book(BookCommand::Replace {
            symbol: instrument_id.symbol.inner(),
            generation,
            cancel,
            recovery,
            result,
        })?;

        response
            .await
            .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?
    }

    pub(crate) fn send_book(&self, command: BookCommand) -> Result<(), KrakenWsError> {
        self.book_cmd_tx
            .read()
            .as_ref()
            .ok_or_else(|| KrakenWsError::Disconnected("No managed Futures WebSocket".into()))?
            .send(command)
            .map_err(|_| KrakenWsError::Disconnected("Managed Futures WebSocket closed".into()))
    }

    pub(crate) fn take_book_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<BookMessage>> {
        self.book_out_rx.take()
    }

    async fn ensure_book_subscribed(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.add_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&book_key);
        let payload = self
            .send_subscribe_feed(KrakenFuturesFeed::Book, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_subscribe(&book_key);
        self.subscription_payloads
            .write()
            .await
            .insert(book_key, payload);
        Ok(())
    }

    async fn maybe_unsubscribe_book(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.remove_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&book_key);
        self.send_unsubscribe_feed(KrakenFuturesFeed::Book, vec![symbol.to_string()])
            .await?;
        self.subscriptions.confirm_unsubscribe(&book_key);
        self.subscription_payloads.write().await.remove(&book_key);
        Ok(())
    }

    /// Gets the output receiver for processed messages.
    pub fn take_output_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<KrakenFuturesWsMessage>> {
        self.out_rx.take().and_then(|arc| Arc::try_unwrap(arc).ok())
    }

    /// Set authentication credentials directly (for when challenge is obtained externally).
    pub async fn set_auth_credentials(
        &self,
        original_challenge: String,
        signed_challenge: String,
    ) -> Result<(), KrakenWsError> {
        let _credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        let original_challenge = SecretString::from(original_challenge);
        let signed_challenge = SecretString::from(signed_challenge);
        *self.original_challenge.write().await = Some(original_challenge);
        *self.signed_challenge.write().await = Some(signed_challenge);
        self.auth_tracker.succeed();

        Ok(())
    }

    /// Sign a challenge with the API credentials.
    ///
    /// Returns the signed challenge on success.
    pub fn sign_challenge(&self, challenge: &str) -> Result<String, KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        credential.sign_ws_challenge(challenge).map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to sign challenge: {e}"))
        })
    }

    /// Complete authentication with a received challenge.
    pub async fn authenticate_with_challenge(&self, challenge: &str) -> Result<(), KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        let signed_challenge = credential.sign_ws_challenge(challenge).map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to sign challenge: {e}"))
        })?;

        self.set_auth_credentials(challenge.to_string(), signed_challenge)
            .await
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

    /// Returns a reference to the shared account ID.
    #[must_use]
    pub fn account_id_shared(&self) -> &Arc<RwLock<Option<AccountId>>> {
        &self.account_id
    }

    /// Returns a reference to the truncated ID map.
    #[must_use]
    pub fn truncated_id_map(&self) -> &Arc<AtomicMap<String, ClientOrderId>> {
        &self.truncated_id_map
    }

    /// Returns a reference to the order-to-instrument map.
    #[must_use]
    pub fn order_instrument_map(&self) -> &Arc<AtomicMap<String, InstrumentId>> {
        &self.order_instrument_map
    }

    /// Returns a reference to the shared instruments map.
    #[must_use]
    pub fn instruments_shared(&self) -> &Arc<AtomicMap<InstrumentId, InstrumentAny>> {
        &self.instruments
    }

    /// Returns a reference to the subscription state.
    #[must_use]
    pub fn subscriptions(&self) -> &SubscriptionState {
        &self.subscriptions
    }

    /// Caches an instrument for execution report parsing.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instruments.insert(instrument.id(), instrument);
    }

    /// Caches multiple instruments for execution report parsing.
    pub fn cache_instruments(&self, instruments: &[InstrumentAny]) {
        self.instruments.rcu(|m| {
            for instrument in instruments {
                m.insert(instrument.id(), instrument.clone());
            }
        });
    }

    /// Caches a client order for shortened ID resolution and instrument lookup.
    ///
    /// An order sent with its client order ID shortened by `truncate_cl_ord_id` is echoed in
    /// that form. This method stores the mapping from the shortened to the full ID, and from
    /// venue order ID to instrument ID for cancel messages.
    pub fn cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        _trader_id: TraderId,
        _strategy_id: StrategyId,
    ) {
        let truncated = truncate_cl_ord_id(&client_order_id);

        if truncated != client_order_id.as_str() {
            self.truncated_id_map.insert(truncated, client_order_id);
        }

        if let Some(venue_id) = venue_order_id {
            self.order_instrument_map
                .insert(venue_id.to_string(), instrument_id);
        }
    }

    /// Subscribes to open orders feed (private, requires authentication).
    pub async fn subscribe_open_orders(&self) -> Result<(), KrakenWsError> {
        let key = "open_orders";
        if !self.subscriptions.add_reference(key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(key);
        let payload = self
            .send_private_subscribe_feed(KrakenFuturesFeed::OpenOrders)
            .await?;
        self.subscriptions.confirm_subscribe(key);
        self.subscription_payloads
            .write()
            .await
            .insert(key.to_string(), payload);
        Ok(())
    }

    /// Subscribes to fills feed (private, requires authentication).
    pub async fn subscribe_fills(&self) -> Result<(), KrakenWsError> {
        let key = "fills";
        if !self.subscriptions.add_reference(key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(key);
        let payload = self
            .send_private_subscribe_feed(KrakenFuturesFeed::Fills)
            .await?;
        self.subscriptions.confirm_subscribe(key);
        self.subscription_payloads
            .write()
            .await
            .insert(key.to_string(), payload);
        Ok(())
    }

    /// Subscribes to both open orders and fills (convenience method).
    pub async fn subscribe_executions(&self) -> Result<(), KrakenWsError> {
        self.subscribe_open_orders().await?;
        self.subscribe_fills().await?;
        Ok(())
    }

    async fn send_subscribe_feed(
        &self,
        feed: KrakenFuturesFeed,
        product_ids: Vec<String>,
    ) -> Result<SecretString, KrakenWsError> {
        let request = KrakenFuturesRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed,
            product_ids,
        };
        let payload = SecretString::from(
            serde_json::to_string(&request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?,
        );
        self.cmd_tx
            .read()
            .await
            .send(FuturesHandlerCommand::Subscribe {
                payload: payload.clone(),
            })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        Ok(payload)
    }

    async fn send_unsubscribe_feed(
        &self,
        feed: KrakenFuturesFeed,
        product_ids: Vec<String>,
    ) -> Result<(), KrakenWsError> {
        let request = KrakenFuturesRequest {
            event: KrakenFuturesEvent::Unsubscribe,
            feed,
            product_ids,
        };
        let payload = SecretString::from(
            serde_json::to_string(&request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?,
        );
        self.cmd_tx
            .read()
            .await
            .send(FuturesHandlerCommand::Unsubscribe { payload })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        Ok(())
    }

    async fn send_private_subscribe_feed(
        &self,
        feed: KrakenFuturesFeed,
    ) -> Result<SecretString, KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;
        let original_challenge = self
            .original_challenge
            .read()
            .await
            .clone()
            .ok_or_else(|| {
                KrakenWsError::AuthenticationError(
                    "Must authenticate before subscribing to private feeds".to_string(),
                )
            })?;
        let signed_challenge = self.signed_challenge.read().await.clone().ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "Must authenticate before subscribing to private feeds".to_string(),
            )
        })?;

        let request = Zeroizing::new(KrakenFuturesPrivateSubscribeRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed,
            api_key: credential.api_key().into(),
            original_challenge,
            signed_challenge,
        });
        let payload = SecretString::from(
            serde_json::to_string(&*request)
                .map_err(|e| KrakenWsError::JsonError(e.to_string()))?,
        );
        drop(request);
        self.cmd_tx
            .read()
            .await
            .send(FuturesHandlerCommand::Subscribe {
                payload: payload.clone(),
            })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        Ok(payload)
    }
}

fn update_private_payload_credentials(
    payload: &str,
    api_key: &str,
    original_challenge: &str,
    signed_challenge: &str,
) -> Option<SecretString> {
    let mut value: serde_json::Value = serde_json::from_str(payload).ok()?;
    let obj = value.as_object_mut()?;
    obj.insert(
        "api_key".to_string(),
        serde_json::Value::String(api_key.to_string()),
    );
    obj.insert(
        "original_challenge".to_string(),
        serde_json::Value::String(original_challenge.to_string()),
    );
    obj.insert(
        "signed_challenge".to_string(),
        serde_json::Value::String(signed_challenge.to_string()),
    );
    let payload = serde_json::to_string(&value).ok().map(SecretString::from);
    zeroize_json_value(&mut value);
    payload
}

fn build_challenge_payload(credential: &KrakenCredential) -> serde_json::Result<SecretString> {
    let request = Zeroizing::new(KrakenFuturesChallengeRequest {
        event: KrakenFuturesEvent::Challenge,
        api_key: credential.api_key().into(),
    });
    serde_json::to_string(&*request).map(SecretString::from)
}

fn is_private_feed_key(key: &str) -> bool {
    key == "open_orders" || key == "fills"
}

fn resubscribe_public(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<FuturesHandlerCommand>,
    subscriptions: &SubscriptionState,
    payloads: &HashMap<String, SecretString>,
) {
    for (key, payload) in payloads {
        if is_private_feed_key(key) {
            continue;
        }

        if let Err(e) = cmd_tx.send(FuturesHandlerCommand::Subscribe {
            payload: payload.clone(),
        }) {
            log::error!("Failed to send resubscribe: error={e}, topic={key}");
            continue;
        }

        subscriptions.mark_subscribe(key);
    }
}

fn resubscribe_private(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<FuturesHandlerCommand>,
    subscriptions: &SubscriptionState,
    payloads: &HashMap<String, SecretString>,
    credential: &KrakenCredential,
    original_challenge: &str,
    signed_challenge: &str,
) {
    for (key, payload) in payloads {
        if !is_private_feed_key(key) {
            continue;
        }

        let Some(updated) = update_private_payload_credentials(
            payload.expose_secret(),
            credential.api_key(),
            original_challenge,
            signed_challenge,
        ) else {
            log::error!("Failed to update private payload for {key}");
            continue;
        };

        if let Err(e) = cmd_tx.send(FuturesHandlerCommand::Subscribe { payload: updated }) {
            log::error!("Failed to send resubscribe: error={e}, topic={key}");
            continue;
        }

        subscriptions.mark_subscribe(key);
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use base64::{Engine, engine::general_purpose::STANDARD};
    use futures_util::{SinkExt, StreamExt};
    use nautilus_network::websocket::AuthTracker;
    use nautilus_testkit::connection::race_command_with_publication;
    use rstest::rstest;

    use super::*;

    struct DropSignal(Arc<AtomicBool>);

    impl Drop for DropSignal {
        fn drop(&mut self) {
            self.0.store(true, Ordering::Release);
        }
    }

    fn test_credential() -> KrakenCredential {
        let secret = STANDARD.encode(b"test_secret_key_24bytes!");
        KrakenCredential::new("test_key", secret)
    }

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
        let mut ws_client = KrakenFuturesWebSocketClient::new(url, 30, None);
        let observer = ws_client.clone();
        let expected = serde_json::json!({
            "event": "subscribe",
            "feed": "trade",
            "product_ids": [
                "PI_XBTUSD"
            ]
        });

        let command = FuturesHandlerCommand::Subscribe {
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
    #[case(false, false, false)]
    #[case(true, false, false)]
    #[case(false, true, false)]
    #[case(false, false, true)]
    #[tokio::test]
    async fn test_managed_book_frames_follow_confirmed_subscriptions(
        #[case] cancel_before_reconnect: bool,
        #[case] cancel_during_replay: bool,
        #[case] reject_before_reconnect: bool,
    ) {
        use serde_json::{Value, json};
        use tokio_tungstenite::{WebSocketStream, tungstenite::Message};

        async fn expect_command(
            socket: &mut WebSocketStream<tokio::net::TcpStream>,
            expected: &str,
        ) {
            let frame = socket.next().await.unwrap().unwrap();
            let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
            assert_eq!(
                request,
                json!({"event": expected, "feed": "book", "product_ids": ["PI_XBTUSD"]})
            );
        }

        async fn subscribed(socket: &mut WebSocketStream<tokio::net::TcpStream>, sequence: u64) {
            socket
                .send(Message::text(
                    r#"{"event":"subscribed","feed":"book","product_ids":["PI_XBTUSD"]}"#,
                ))
                .await
                .unwrap();
            snapshot(socket, sequence).await;
        }

        async fn snapshot(socket: &mut WebSocketStream<tokio::net::TcpStream>, sequence: u64) {
            let mut snapshot: Value = serde_json::from_str(include_str!(
                "../../../test_data/ws_futures_book_snapshot.json"
            ))
            .unwrap();
            snapshot["seq"] = json!(sequence);
            socket
                .send(Message::text(snapshot.to_string()))
                .await
                .unwrap();
        }

        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let mut client = KrakenFuturesWebSocketClient::new(
                format!("ws://{}", listener.local_addr().unwrap()),
                30,
                None,
            );

            let (confirm_initial, initial_confirmation) = tokio::sync::oneshot::channel();
            let (confirm_replay, replay_confirmation) = tokio::sync::oneshot::channel();
            let (confirm_turnover, turnover_started) = tokio::sync::oneshot::channel();
            let (unsubscribe_sent, unsubscribe_seen) = tokio::sync::oneshot::channel();
            let (cut, cut_connection) = tokio::sync::oneshot::channel();

            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                expect_command(&mut socket, "subscribe").await;
                initial_confirmation.await.unwrap();
                subscribed(&mut socket, 41).await;
                expect_command(&mut socket, "unsubscribe").await;
                turnover_started.await.unwrap();
                socket
                    .send(Message::text(
                        json!({
                            "event": "alert",
                            "message": "Couldn't subscribe to invalid product `PI_XBTUSD`",
                        })
                        .to_string(),
                    ))
                    .await
                    .unwrap();
                subscribed(&mut socket, 42).await;
                socket
                    .send(Message::text(
                        r#"{"event":"unsubscribed","feed":"book","product_ids":["PI_XBTUSD"]}"#,
                    ))
                    .await
                    .unwrap();
                expect_command(&mut socket, "subscribe").await;
                subscribed(&mut socket, 53).await;
                expect_command(&mut socket, "unsubscribe").await;
                if reject_before_reconnect {
                    socket
                        .send(Message::text(
                            r#"{"event":"unsubscribed_failed","feed":"book","product_ids":["PI_XBTUSD"]}"#,
                        ))
                        .await
                        .unwrap();
                }

                unsubscribe_sent.send(()).unwrap();
                cut_connection.await.unwrap();
                drop(socket);

                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                expect_command(&mut socket, "subscribe").await;
                snapshot(&mut socket, 58).await;
                replay_confirmation.await.unwrap();
                subscribed(&mut socket, 59).await;
                let _ = socket.next().await;
            });

            client.connect_books().await.unwrap();
            let mut rx = client.take_book_rx().unwrap();
            let instrument_id = InstrumentId::from("PI_XBTUSD.KRAKEN");
            let symbol = instrument_id.symbol.inner();
            client.send_book(BookCommand::Subscribe(symbol, 1)).unwrap();
            let initial_recovery: Arc<BookRecovery<KrakenWsError>> = Arc::default();
            let initial_gate = initial_recovery.gate.clone();
            initial_gate.lock().close();

            {
                let initial = client.replace_book(
                    instrument_id,
                    1,
                    CancellationToken::new(),
                    Arc::clone(&initial_recovery),
                );
                let mut initial = std::pin::pin!(initial);
                assert!(futures_util::poll!(&mut initial).is_pending());
                let retired = client
                    .replace_book(
                        instrument_id,
                        0,
                        CancellationToken::new(),
                        Arc::default(),
                    )
                    .await;
                assert!(matches!(retired, Err(KrakenWsError::Disconnected(_))));
                assert!(initial_gate.lock().is_closed());
                confirm_initial.send(()).unwrap();
                initial.await.unwrap();
            }

            assert!(!initial_gate.lock().is_closed());

            let BookMessage::Data(_, first) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

            let first = first.expect("confirmed initial subscription");
            assert_eq!(first.generation, 1);

            client.send_book(BookCommand::Unsubscribe(symbol)).unwrap();
            client.send_book(BookCommand::Subscribe(symbol, 2)).unwrap();
            let old = client
                .replace_book(instrument_id, 1, CancellationToken::new(), Arc::default())
                .await;
            assert!(matches!(old, Err(KrakenWsError::Disconnected(_))));
            confirm_turnover.send(()).unwrap();

            let BookMessage::Data(message, late) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

            let KrakenFuturesWsMessage::BookSnapshot(snapshot) = message else {
                panic!("expected a late snapshot");
            };

            assert_eq!(snapshot.seq, 42);
            assert!(late.is_none());
            assert!(first.lock_active().is_none());

            let BookMessage::Data(message, current) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

            let KrakenFuturesWsMessage::BookSnapshot(snapshot) = message else {
                panic!("expected fresh snapshot");
            };

            assert_eq!(snapshot.seq, 53);
            let current = current.expect("confirmed second subscription");
            assert_eq!(current.generation, 2);
            assert!(current.lock_active().is_some());

            let recovery: Arc<BookRecovery<KrakenWsError>> = Arc::default();
            let gate = recovery.gate.clone();
            gate.lock().close();

            {
                let cancellation = CancellationToken::new();
                let mut replacement = Box::pin(client.replace_book(
                    instrument_id,
                    2,
                    cancellation.clone(),
                    Arc::clone(&recovery),
                ));
                assert!(futures_util::poll!(&mut replacement).is_pending());
                unsubscribe_seen.await.unwrap();

                let mut replacement = if reject_before_reconnect {
                    assert!(matches!(replacement.await, Err(KrakenWsError::SubscriptionError(_))));
                    None
                } else if cancel_before_reconnect {
                    cancellation.cancel();
                    drop(replacement);
                    None
                } else {
                    Some(replacement)
                };

                cut.send(()).unwrap();

                let BookMessage::Data(reconnected, _) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

                assert!(matches!(reconnected, KrakenFuturesWsMessage::Reconnected));
                assert!(current.lock_active().is_none());

                let BookMessage::Data(pre_ack, stream) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

                assert!(matches!(pre_ack, KrakenFuturesWsMessage::BookSnapshot(s) if s.seq == 58));
                assert!(stream.is_none());
                assert!(gate.lock().is_closed());

                if cancel_during_replay {
                    cancellation.cancel();
                    replacement = None;
                }

                confirm_replay.send(()).unwrap();

                if let Some(replacement) = replacement {
                    replacement.await.unwrap();
                }
            }

            let BookMessage::Data(replayed, stream) = rx.recv().await.unwrap() else {
                panic!("Expected a Futures feed message");
            };

            assert!(matches!(replayed, KrakenFuturesWsMessage::BookSnapshot(s) if s.seq == 59));
            let stream = stream.expect("confirmed replay subscription");
            assert_eq!(stream.generation, 2);
            assert!(stream.lock_active().is_some());
            assert!(!gate.lock().is_closed());
            assert!(recovery.accept());
            assert!(matches!(
                client.replace_book(instrument_id, 2, CancellationToken::new(), recovery).await,
                Err(KrakenWsError::Disconnected(_))
            ));
            assert!(stream.lock_active().is_some());
            client.close().await.unwrap();
            server.await.unwrap();
        })
        .await
        .expect("Futures book turnover and replacement completed");
    }

    #[tokio::test]
    async fn test_managed_book_unsubscribe_waits_for_in_flight_subscribe() {
        use serde_json::{Value, json};
        use tokio_tungstenite::tungstenite::Message;

        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let mut client = KrakenFuturesWebSocketClient::new(
                format!("ws://{}", listener.local_addr().unwrap()), 30, None,
            );

            let (sent, initial_sent) = tokio::sync::oneshot::channel();
            let (confirm, final_requested) = tokio::sync::oneshot::channel();

            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let mut pending_confirmation = Some((sent, final_requested));

                for (request, ack) in [("subscribe", "subscribed"), ("unsubscribe", "unsubscribed")] {
                    let frame = socket.next().await.unwrap().unwrap();
                    let payload: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                    assert_eq!(payload, json!({"event": request, "feed": "book", "product_ids": ["PI_XBTUSD"]}));

                    if let Some((sent, final_requested)) = pending_confirmation.take() {
                        // Hold this acknowledgement until the managed lane has processed the final unsubscribe.
                        sent.send(()).unwrap();
                        final_requested.await.unwrap();
                    }

                    let reply = json!({"event": ack, "feed": "book", "product_ids": ["PI_XBTUSD"]});
                    socket.send(Message::text(reply.to_string())).await.unwrap();
                }
            });

            client.connect_books().await.unwrap();
            let instrument = InstrumentId::from("PI_XBTUSD.KRAKEN");
            let symbol = instrument.symbol.inner();
            client.send_book(BookCommand::Subscribe(symbol, 1)).unwrap();
            initial_sent.await.unwrap();
            client.send_book(BookCommand::Unsubscribe(symbol)).unwrap();
            let superseded = client.replace_book(instrument, 1, CancellationToken::new(), Arc::default()).await;
            assert!(matches!(superseded, Err(KrakenWsError::Disconnected(message)) if message == "Futures book subscription retired"));
            confirm.send(()).unwrap();
            server.await.unwrap();
            client.close().await.unwrap();
        }).await.expect("Futures turnover settled on the original socket");
    }

    #[tokio::test]
    async fn test_managed_book_invalid_product_alert_settles_only_its_subscribe() {
        use serde_json::{Value, json};
        use tokio_tungstenite::tungstenite::Message;

        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let mut client = KrakenFuturesWebSocketClient::new(
                format!("ws://{}", listener.local_addr().unwrap()),
                30,
                None,
            );

            let (reject, ready) = tokio::sync::oneshot::channel();

            let server = tokio::spawn(async move {
                let (tcp, _) = listener.accept().await.unwrap();
                let mut socket = tokio_tungstenite::accept_async(tcp).await.unwrap();
                let mut products = Vec::new();

                for _ in 0..2 {
                    let frame = socket.next().await.unwrap().unwrap();
                    let request: Value = serde_json::from_str(frame.to_text().unwrap()).unwrap();
                    assert_eq!(request["event"], "subscribe");
                    assert_eq!(request["feed"], "book");
                    products.push(request["product_ids"][0].as_str().unwrap().to_owned());
                }

                products.sort_unstable();
                assert_eq!(products, ["PI_NOT_A_REAL_CONTRACT", "PI_XBTUSD"]);
                ready.await.unwrap();

                for reply in [
                    json!({"event": "alert", "message": "Couldn't subscribe to invalid product `PI_NOT_A_REAL_CONTRACT`"}),
                    json!({"event": "subscribed", "feed": "book", "product_ids": ["PI_XBTUSD"]}),
                ] {
                    socket.send(Message::text(reply.to_string())).await.unwrap();
                }

                let _ = socket.next().await;
            });

            client.connect_books().await.unwrap();
            let invalid = InstrumentId::from("PI_NOT_A_REAL_CONTRACT.KRAKEN");
            let healthy = InstrumentId::from("PI_XBTUSD.KRAKEN");
            client.send_book(BookCommand::Subscribe(invalid.symbol.inner(), 1)).unwrap();
            client.send_book(BookCommand::Subscribe(healthy.symbol.inner(), 2)).unwrap();

            {
                let invalid = client.replace_book(invalid, 1, CancellationToken::new(), Arc::default());
                let barrier = client.replace_book(healthy, 0, CancellationToken::new(), Arc::default());
                let healthy = client.replace_book(healthy, 2, CancellationToken::new(), Arc::default());
                tokio::pin!(invalid, healthy);
                assert!(futures_util::poll!(&mut invalid).is_pending());
                assert!(futures_util::poll!(&mut healthy).is_pending());
                assert!(matches!(barrier.await, Err(KrakenWsError::Disconnected(_))));
                reject.send(()).unwrap();

                assert!(matches!(invalid.await, Err(KrakenWsError::SubscriptionError(_))));
                healthy.await.unwrap();
            }

            assert!(client.is_active());
            client.close().await.unwrap();
            server.await.unwrap();
        }).await.expect("invalid product and healthy subscribe settled on the original socket");
    }

    #[rstest]
    #[case("wss://stream.example.com/feed", "wss://stream.example.com")]
    #[case(
        "wss://SYN-user:SYN-password@stream.example.com:9443/SYN-path?token=SYN-query#SYN-fragment",
        "wss://stream.example.com:9443"
    )]
    fn test_debug_preserves_url_origin(#[case] url: &str, #[case] origin: &str) {
        let client = KrakenFuturesWebSocketClient::new(url.to_string(), 30, None);
        let debug = format!("{client:?}");
        assert!(debug.contains(&format!("url: {origin:?}")));
        assert!(!debug.contains("SYN-"));
        assert_eq!(client.url.expose(), url);
    }

    #[rstest]
    #[tokio::test]
    async fn test_debug_redacts_auth_state_and_proxy_url() {
        let client = KrakenFuturesWebSocketClient::with_credentials(
            "wss://test".to_string(),
            30,
            Some(test_credential()),
            None,
            TransportBackend::default(),
            Some("http://user:proxy-secret@localhost".to_string()),
        );
        client
            .set_auth_credentials(
                "original-challenge".to_string(),
                "signed-challenge".to_string(),
            )
            .await
            .unwrap();

        let debug = format!("{client:?}");

        assert!(debug.contains(REDACTED));
        assert!(!debug.contains("proxy-secret"));
        assert!(!debug.contains("original-challenge"));
        assert!(!debug.contains("signed-challenge"));
    }

    #[tokio::test]
    async fn test_last_client_owner_drop_aborts_handler_task() {
        let client = KrakenFuturesWebSocketClient::new("wss://test".to_string(), 30, None);
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();
        let dropped = Arc::new(AtomicBool::new(false));
        let drop_signal = DropSignal(Arc::clone(&dropped));
        client
            .handler_tasks
            .spawn(async move {
                let _drop_signal = drop_signal;
                started_tx.send(()).expect("started receiver");
                std::future::pending::<()>().await;
            })
            .expect("handler task should register");
        started_rx.await.expect("handler task started");
        let clone = client.clone();

        drop(client);
        assert!(!dropped.load(Ordering::Acquire));
        drop(clone);

        tokio::time::timeout(tokio::time::Duration::from_secs(1), async {
            while !dropped.load(Ordering::Acquire) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("handler task aborted");
    }

    #[rstest]
    fn test_build_challenge_payload_emits_expected_event() {
        let credential = test_credential();
        let payload = build_challenge_payload(&credential).expect("serializes");
        assert!(payload.expose_secret().contains(r#""event":"challenge""#));
        assert!(payload.expose_secret().contains(r#""api_key":"test_key""#));
    }

    #[rstest]
    #[tokio::test]
    async fn test_resubscribe_public_skips_private_feeds() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        let subscriptions = SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER);

        let mut payloads = HashMap::new();
        payloads.insert(
            "trades:PI_XBTUSD".to_string(),
            SecretString::from(
                r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
            ),
        );
        payloads.insert(
            "open_orders".to_string(),
            SecretString::from(r#"{"event":"subscribe","feed":"open_orders"}"#.to_string()),
        );

        resubscribe_public(&cmd_tx, &subscriptions, &payloads);

        let mut subscribed = Vec::new();
        while let Ok(FuturesHandlerCommand::Subscribe { payload }) = cmd_rx.try_recv() {
            subscribed.push(payload);
        }

        assert_eq!(
            subscribed.len(),
            1,
            "only the public feed should resubscribe"
        );
        assert!(subscribed[0].expose_secret().contains("PI_XBTUSD"));
    }

    #[rstest]
    #[tokio::test]
    async fn test_resubscribe_public_restores_publics_even_with_credentialed_client() {
        // The reconnect path runs resubscribe_public() unconditionally, so a
        // credentialed client's public feeds keep flowing even if re-auth fails.
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        let subscriptions = SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER);

        let mut payloads = HashMap::new();
        payloads.insert(
            "trades:PI_XBTUSD".to_string(),
            SecretString::from(
                r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
            ),
        );

        resubscribe_public(&cmd_tx, &subscriptions, &payloads);

        match cmd_rx.try_recv().expect("public subscribe expected") {
            FuturesHandlerCommand::Subscribe { payload } => {
                assert!(payload.expose_secret().contains("PI_XBTUSD"));
            }
            other => panic!("expected Subscribe, was {other:?}"),
        }
    }

    #[rstest]
    #[tokio::test]
    async fn test_resubscribe_private_patches_credentials() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        let subscriptions = SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER);
        let credential = test_credential();

        let mut payloads = HashMap::new();
        payloads.insert(
            "open_orders".to_string(),
            SecretString::from(
                r#"{"event":"subscribe","feed":"open_orders","api_key":"","original_challenge":"","signed_challenge":""}"#
                    .to_string(),
            ),
        );
        payloads.insert(
            "trades:PI_XBTUSD".to_string(),
            SecretString::from(
                r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
            ),
        );

        resubscribe_private(
            &cmd_tx,
            &subscriptions,
            &payloads,
            &credential,
            "server-challenge",
            "signed-value",
        );

        let mut subscribed = Vec::new();
        while let Ok(FuturesHandlerCommand::Subscribe { payload }) = cmd_rx.try_recv() {
            subscribed.push(payload);
        }

        assert_eq!(
            subscribed.len(),
            1,
            "only the private feed should resubscribe"
        );
        let value: serde_json::Value =
            serde_json::from_str(subscribed[0].expose_secret()).expect("payload is valid JSON");
        assert_eq!(value["event"], "subscribe");
        assert_eq!(value["feed"], "open_orders");
        assert_eq!(value["api_key"], "test_key");
        assert_eq!(value["original_challenge"], "server-challenge");
        assert_eq!(value["signed_challenge"], "signed-value");
    }

    #[rstest]
    #[tokio::test]
    async fn test_auth_tracker_succeed_completes_wait_for_result() {
        let tracker = AuthTracker::new();
        let receiver = tracker.begin();

        let tracker_for_responder = tracker.clone();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            tracker_for_responder.succeed();
        });

        tracker
            .wait_for_result::<KrakenWsError>(tokio::time::Duration::from_secs(1), receiver)
            .await
            .expect("auth should succeed");

        assert!(tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_auth_tracker_wait_for_result_times_out() {
        let tracker = AuthTracker::new();
        let receiver = tracker.begin();

        let err = tracker
            .wait_for_result::<KrakenWsError>(tokio::time::Duration::from_millis(20), receiver)
            .await
            .expect_err("should time out");

        assert!(matches!(err, KrakenWsError::AuthenticationError(_)));
        assert!(!tracker.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_authenticate_without_credentials_errors() {
        let client = KrakenFuturesWebSocketClient::new(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            None,
        );

        let err = client.authenticate().await.expect_err("should fail");
        assert!(
            matches!(err, KrakenWsError::AuthenticationError(ref msg) if msg.contains("API credentials required")),
            "unexpected error: {err:?}"
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_set_auth_credentials_marks_tracker_authenticated() {
        let client = KrakenFuturesWebSocketClient::with_credentials(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            Some(test_credential()),
            None,
            TransportBackend::default(),
            None,
        );

        assert!(!client.is_authenticated());

        client
            .set_auth_credentials("orig-challenge".to_string(), "signed-challenge".to_string())
            .await
            .expect("should succeed");

        assert!(client.is_authenticated());
        client
            .wait_until_authenticated(0.05)
            .await
            .expect("should return immediately");
    }

    #[rstest]
    #[tokio::test]
    async fn test_set_auth_credentials_without_credentials_errors() {
        let client = KrakenFuturesWebSocketClient::new(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            None,
        );

        let err = client
            .set_auth_credentials("orig".to_string(), "signed".to_string())
            .await
            .expect_err("should fail");
        assert!(matches!(err, KrakenWsError::AuthenticationError(_)));
        assert!(!client.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_authenticate_with_challenge_updates_state() {
        let client = KrakenFuturesWebSocketClient::with_credentials(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            Some(test_credential()),
            None,
            TransportBackend::default(),
            None,
        );

        client
            .authenticate_with_challenge("server-challenge")
            .await
            .expect("should succeed");

        assert!(client.is_authenticated());
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_until_authenticated_resolves_after_success() {
        let client = KrakenFuturesWebSocketClient::with_credentials(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            Some(test_credential()),
            None,
            TransportBackend::default(),
            None,
        );

        let client_for_responder = client.clone();

        tokio::spawn(async move {
            tokio::time::sleep(tokio::time::Duration::from_millis(20)).await;
            client_for_responder
                .set_auth_credentials("orig".to_string(), "signed".to_string())
                .await
                .expect("succeeds");
        });

        client
            .wait_until_authenticated(1.0)
            .await
            .expect("should resolve once credentials are set");
    }

    #[rstest]
    #[tokio::test]
    async fn test_wait_until_authenticated_times_out() {
        let client = KrakenFuturesWebSocketClient::with_credentials(
            "wss://futures.kraken.com/ws/v1".to_string(),
            60,
            Some(test_credential()),
            None,
            TransportBackend::default(),
            None,
        );

        let err = client
            .wait_until_authenticated(0.05)
            .await
            .expect_err("should time out");
        assert!(matches!(err, KrakenWsError::AuthenticationError(_)));
    }
}
