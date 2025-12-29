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

//! WebSocket client for the Kraken Futures v1 streaming API.

use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU8, Ordering},
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_model::{
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
    instruments::InstrumentAny,
};
use nautilus_network::{
    mode::ConnectionMode,
    websocket::{
        AuthTracker, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use tokio_util::sync::CancellationToken;

use super::{
    handler::{FuturesFeedHandler, HandlerCommand},
    messages::{KrakenFuturesFeed, KrakenFuturesWsMessage},
};
use crate::{common::credential::KrakenCredential, websocket::error::KrakenWsError};

/// Topic delimiter for Kraken Futures WebSocket subscriptions.
///
/// Topics use colon format: `feed:symbol` (e.g., `trades:PF_ETHUSD`).
pub const KRAKEN_FUTURES_WS_TOPIC_DELIMITER: char = ':';

const WS_PING_MSG: &str = r#"{"event":"ping"}"#;

/// WebSocket client for the Kraken Futures v1 streaming API.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken")
)]
pub struct KrakenFuturesWebSocketClient {
    url: String,
    heartbeat_secs: Option<u64>,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<KrakenFuturesWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    credential: Option<KrakenCredential>,
    original_challenge: Arc<tokio::sync::RwLock<Option<String>>>,
    signed_challenge: Arc<tokio::sync::RwLock<Option<String>>>,
}

impl Clone for KrakenFuturesWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            heartbeat_secs: self.heartbeat_secs,
            signal: Arc::clone(&self.signal),
            connection_mode: Arc::clone(&self.connection_mode),
            cmd_tx: Arc::clone(&self.cmd_tx),
            out_rx: self.out_rx.clone(),
            task_handle: self.task_handle.clone(),
            subscriptions: self.subscriptions.clone(),
            auth_tracker: self.auth_tracker.clone(),
            cancellation_token: self.cancellation_token.clone(),
            credential: self.credential.clone(),
            original_challenge: Arc::clone(&self.original_challenge),
            signed_challenge: Arc::clone(&self.signed_challenge),
        }
    }
}

impl KrakenFuturesWebSocketClient {
    /// Creates a new client with the given URL.
    #[must_use]
    pub fn new(url: String, heartbeat_secs: Option<u64>) -> Self {
        Self::with_credentials(url, heartbeat_secs, None)
    }

    /// Creates a new client with API credentials for authenticated feeds.
    #[must_use]
    pub fn with_credentials(
        url: String,
        heartbeat_secs: Option<u64>,
        credential: Option<KrakenCredential>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let initial_mode = AtomicU8::new(ConnectionMode::Closed.as_u8());
        let connection_mode = Arc::new(ArcSwap::from_pointee(initial_mode));

        Self {
            url,
            heartbeat_secs,
            signal: Arc::new(AtomicBool::new(false)),
            connection_mode,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            task_handle: None,
            subscriptions: SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER),
            auth_tracker: AuthTracker::new(),
            cancellation_token: CancellationToken::new(),
            credential,
            original_challenge: Arc::new(tokio::sync::RwLock::new(None)),
            signed_challenge: Arc::new(tokio::sync::RwLock::new(None)),
        }
    }

    /// Returns true if the client has API credentials set.
    #[must_use]
    pub fn has_credentials(&self) -> bool {
        self.credential.is_some()
    }

    /// Returns the WebSocket URL.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
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

    /// Authenticates the WebSocket connection for private feeds.
    ///
    /// This sends a challenge request, waits for the response, signs it,
    /// and stores the credentials for use in private subscriptions.
    pub async fn authenticate(&self) -> Result<(), KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        let api_key = credential.api_key().to_string();
        let (tx, rx) = tokio::sync::oneshot::channel();

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::RequestChallenge {
                api_key: api_key.clone(),
                response_tx: tx,
            })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        let challenge = tokio::time::timeout(tokio::time::Duration::from_secs(10), rx)
            .await
            .map_err(|_| {
                KrakenWsError::AuthenticationError("Timeout waiting for challenge".to_string())
            })?
            .map_err(|_| {
                KrakenWsError::AuthenticationError("Challenge channel closed".to_string())
            })?;

        let signed_challenge = credential.sign_ws_challenge(&challenge).map_err(|e| {
            KrakenWsError::AuthenticationError(format!("Failed to sign challenge: {e}"))
        })?;

        *self.original_challenge.write().await = Some(challenge.clone());
        *self.signed_challenge.write().await = Some(signed_challenge.clone());

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SetAuthCredentials {
                api_key,
                original_challenge: challenge,
                signed_challenge,
            })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        tracing::debug!("Futures WebSocket authentication successful");
        Ok(())
    }

    /// Caches instruments for price precision lookup (bulk replace).
    ///
    /// Must be called after `connect()` when the handler is ready to receive commands.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::InitializeInstruments(instruments))
        {
            tracing::debug!("Failed to send instruments to handler: {e}");
        }
    }

    /// Caches a single instrument for price precision lookup (upsert).
    ///
    /// Must be called after `connect()` when the handler is ready to receive commands.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::UpdateInstrument(instrument))
        {
            tracing::debug!("Failed to send instrument update to handler: {e}");
        }
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Connecting to Futures WebSocket: {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: self.heartbeat_secs,
            heartbeat_msg: Some(WS_PING_MSG.to_string()),
            reconnect_timeout_ms: Some(5_000),
            reconnect_delay_initial_ms: Some(500),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(1.5),
            reconnect_jitter_ms: Some(250),
            reconnect_max_attempts: None,
        };

        let ws_client =
            WebSocketClient::connect(ws_config, Some(raw_handler), None, None, vec![], None)
                .await
                .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<KrakenFuturesWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(HandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let credential_for_reconnect = self.credential.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                FuturesFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions.clone());

            loop {
                match handler.next().await {
                    Some(KrakenFuturesWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        tracing::info!("WebSocket reconnected, resubscribing");

                        // Mark all confirmed as failed to transition to pending for replay
                        let confirmed_topics = subscriptions.all_topics();
                        for topic in &confirmed_topics {
                            subscriptions.mark_failure(topic);
                        }

                        let topics = subscriptions.all_topics();
                        if topics.is_empty() {
                            tracing::debug!("No subscriptions to restore after reconnection");
                        } else {
                            // Check if we have private subscriptions that need re-authentication
                            let has_private_subs = topics.iter().any(|t| {
                                t == "open_orders"
                                    || t == "fills"
                                    || t.starts_with("open_orders:")
                                    || t.starts_with("fills:")
                            });

                            if has_private_subs {
                                if let Some(ref cred) = credential_for_reconnect {
                                    // Request fresh challenge for the new connection
                                    let (tx, rx) = tokio::sync::oneshot::channel();
                                    if let Err(e) = cmd_tx_for_reconnect.send(
                                        HandlerCommand::RequestChallenge {
                                            api_key: cred.api_key().to_string(),
                                            response_tx: tx,
                                        },
                                    ) {
                                        tracing::error!(
                                            error = %e,
                                            "Failed to request challenge for reconnect"
                                        );
                                    } else {
                                        match tokio::time::timeout(
                                            tokio::time::Duration::from_secs(10),
                                            rx,
                                        )
                                        .await
                                        {
                                            Ok(Ok(challenge)) => {
                                                match cred.sign_ws_challenge(&challenge) {
                                                    Ok(signed) => {
                                                        if let Err(e) = cmd_tx_for_reconnect.send(
                                                            HandlerCommand::SetAuthCredentials {
                                                                api_key: cred.api_key().to_string(),
                                                                original_challenge: challenge,
                                                                signed_challenge: signed,
                                                            },
                                                        ) {
                                                            tracing::error!(
                                                                error = %e,
                                                                "Failed to set auth credentials"
                                                            );
                                                        } else {
                                                            tracing::debug!(
                                                                "Re-authenticated after reconnect"
                                                            );
                                                        }
                                                    }
                                                    Err(e) => {
                                                        tracing::error!(
                                                            error = %e,
                                                            "Failed to sign challenge for reconnect"
                                                        );
                                                    }
                                                }
                                            }
                                            Ok(Err(_)) => {
                                                tracing::error!(
                                                    "Challenge channel closed during reconnect"
                                                );
                                            }
                                            Err(_) => {
                                                tracing::error!(
                                                    "Timeout waiting for challenge during reconnect"
                                                );
                                            }
                                        }
                                    }
                                } else {
                                    tracing::warn!(
                                        "Private subscriptions exist but no credentials available"
                                    );
                                }
                            }

                            tracing::info!(
                                count = topics.len(),
                                "Resubscribing after reconnection"
                            );

                            for topic in &topics {
                                let cmd =
                                    if let Some((feed_str, symbol_str)) = topic.split_once(':') {
                                        let symbol = Symbol::from(symbol_str);
                                        match feed_str.parse::<KrakenFuturesFeed>() {
                                            Ok(KrakenFuturesFeed::Trade) => {
                                                Some(HandlerCommand::SubscribeTrade(symbol))
                                            }
                                            Ok(KrakenFuturesFeed::Book) => {
                                                Some(HandlerCommand::SubscribeBook(symbol))
                                            }
                                            Ok(KrakenFuturesFeed::Ticker) => {
                                                Some(HandlerCommand::SubscribeTicker(symbol))
                                            }
                                            Ok(KrakenFuturesFeed::OpenOrders) => {
                                                Some(HandlerCommand::SubscribeOpenOrders)
                                            }
                                            Ok(KrakenFuturesFeed::Fills) => {
                                                Some(HandlerCommand::SubscribeFills)
                                            }
                                            Ok(_) | Err(_) => None,
                                        }
                                    } else {
                                        match topic.parse::<KrakenFuturesFeed>() {
                                            Ok(KrakenFuturesFeed::OpenOrders) => {
                                                Some(HandlerCommand::SubscribeOpenOrders)
                                            }
                                            Ok(KrakenFuturesFeed::Fills) => {
                                                Some(HandlerCommand::SubscribeFills)
                                            }
                                            Ok(_) | Err(_) => None,
                                        }
                                    };

                                if let Some(cmd) = cmd
                                    && let Err(e) = cmd_tx_for_reconnect.send(cmd)
                                {
                                    tracing::error!(
                                        error = %e, topic,
                                        "Failed to send resubscribe command"
                                    );
                                }

                                subscriptions.mark_subscribe(topic);
                            }
                        }

                        if let Err(e) = out_tx.send(KrakenFuturesWsMessage::Reconnected) {
                            tracing::debug!("Output channel closed: {e}");
                            break;
                        }
                        continue;
                    }
                    Some(msg) => {
                        if let Err(e) = out_tx.send(msg) {
                            tracing::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    None => {
                        tracing::debug!("Handler stream ended");
                        break;
                    }
                }
            }

            tracing::debug!("Futures handler task exiting");
        });

        self.task_handle = Some(Arc::new(stream_handle));

        tracing::debug!("Futures WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        tracing::debug!("Disconnecting Futures WebSocket");

        self.signal.store(true, Ordering::Relaxed);

        if let Err(e) = self.cmd_tx.read().await.send(HandlerCommand::Disconnect) {
            tracing::debug!(
                "Failed to send disconnect command (handler may already be shut down): {e}"
            );
        }

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => tracing::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => tracing::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            tracing::warn!("Timeout waiting for task handle");
                        }
                    }
                }
                Err(arc_handle) => {
                    tracing::debug!("Cannot take ownership of task handle, aborting");
                    arc_handle.abort();
                }
            }
        }

        self.subscriptions.clear();
        self.auth_tracker.fail("Disconnected");
        Ok(())
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

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeTrade(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_subscribe(&key);
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

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeTrade(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_unsubscribe(&key);
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
        let key = format!("book:{symbol}");

        if !self.subscriptions.add_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeBook(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_subscribe(&key);
        Ok(())
    }

    /// Unsubscribes from order book updates for the given instrument.
    pub async fn unsubscribe_book(&self, instrument_id: InstrumentId) -> Result<(), KrakenWsError> {
        let symbol = instrument_id.symbol;
        let key = format!("book:{symbol}");

        if !self.subscriptions.remove_reference(&key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeBook(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_unsubscribe(&key);
        Ok(())
    }

    /// Ensures ticker feed is subscribed for the given symbol.
    async fn ensure_ticker_subscribed(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let ticker_key = format!("ticker:{symbol}");

        if !self.subscriptions.add_reference(&ticker_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&ticker_key);
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeTicker(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        self.subscriptions.confirm_subscribe(&ticker_key);
        Ok(())
    }

    /// Unsubscribes from ticker if no more dependent subscriptions.
    async fn maybe_unsubscribe_ticker(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let ticker_key = format!("ticker:{symbol}");

        if !self.subscriptions.remove_reference(&ticker_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&ticker_key);
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeTicker(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        self.subscriptions.confirm_unsubscribe(&ticker_key);
        Ok(())
    }

    /// Ensures book feed is subscribed for the given symbol (for quotes).
    async fn ensure_book_subscribed(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.add_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(&book_key);
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeBook(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        self.subscriptions.confirm_subscribe(&book_key);
        Ok(())
    }

    /// Unsubscribes from book if no more dependent subscriptions.
    async fn maybe_unsubscribe_book(&self, symbol: Symbol) -> Result<(), KrakenWsError> {
        let book_key = format!("book:{symbol}");

        if !self.subscriptions.remove_reference(&book_key) {
            return Ok(());
        }

        self.subscriptions.mark_unsubscribe(&book_key);
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::UnsubscribeBook(symbol))
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;
        self.subscriptions.confirm_unsubscribe(&book_key);
        Ok(())
    }

    /// Gets the output receiver for processed messages.
    pub fn take_output_rx(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<KrakenFuturesWsMessage>> {
        self.out_rx.take().and_then(|arc| Arc::try_unwrap(arc).ok())
    }

    /// Sets the account ID for execution reports.
    ///
    /// Must be called before subscribing to execution feeds to properly generate
    /// OrderStatusReport and FillReport objects.
    pub fn set_account_id(&self, account_id: AccountId) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::SetAccountId(account_id))
        {
            tracing::debug!("Failed to send account_id to handler: {e}");
        }
    }

    /// Caches a client order ID mapping for order tracking.
    ///
    /// This caches the trader_id, strategy_id, and instrument_id for an order,
    /// allowing the handler to emit proper order events with correct identifiers
    /// when WebSocket messages arrive.
    pub fn cache_client_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        instrument_id: InstrumentId,
        trader_id: TraderId,
        strategy_id: StrategyId,
    ) {
        if let Ok(tx) = self.cmd_tx.try_read()
            && let Err(e) = tx.send(HandlerCommand::CacheClientOrder {
                client_order_id,
                venue_order_id,
                instrument_id,
                trader_id,
                strategy_id,
            })
        {
            tracing::debug!("Failed to cache client order: {e}");
        }
    }

    /// Requests a challenge from the WebSocket for authentication.
    ///
    /// After calling this, listen for the challenge response message and then
    /// call `authenticate_with_challenge()` to complete authentication.
    pub async fn request_challenge(&self) -> Result<(), KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError(
                "API credentials required for authentication".to_string(),
            )
        })?;

        // TODO: Send via WebSocket client when we have direct access
        // For now, the Python layer will handle the challenge request/response flow
        tracing::debug!(
            "Challenge request prepared for API key: {}",
            credential.api_key_masked()
        );

        Ok(())
    }

    /// Set authentication credentials directly (for when challenge is obtained externally).
    pub async fn set_auth_credentials(
        &self,
        original_challenge: String,
        signed_challenge: String,
    ) -> Result<(), KrakenWsError> {
        let credential = self.credential.as_ref().ok_or_else(|| {
            KrakenWsError::AuthenticationError("API credentials required".to_string())
        })?;

        *self.original_challenge.write().await = Some(original_challenge.clone());
        *self.signed_challenge.write().await = Some(signed_challenge.clone());

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SetAuthCredentials {
                api_key: credential.api_key().to_string(),
                original_challenge,
                signed_challenge,
            })
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

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

    /// Subscribes to open orders feed (private, requires authentication).
    pub async fn subscribe_open_orders(&self) -> Result<(), KrakenWsError> {
        if self.original_challenge.read().await.is_none() {
            return Err(KrakenWsError::AuthenticationError(
                "Must authenticate before subscribing to private feeds".to_string(),
            ));
        }

        let key = "open_orders";
        if !self.subscriptions.add_reference(key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeOpenOrders)
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_subscribe(key);
        Ok(())
    }

    /// Subscribes to fills feed (private, requires authentication).
    pub async fn subscribe_fills(&self) -> Result<(), KrakenWsError> {
        if self.original_challenge.read().await.is_none() {
            return Err(KrakenWsError::AuthenticationError(
                "Must authenticate before subscribing to private feeds".to_string(),
            ));
        }

        let key = "fills";
        if !self.subscriptions.add_reference(key) {
            return Ok(());
        }

        self.subscriptions.mark_subscribe(key);

        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SubscribeFills)
            .map_err(|e| KrakenWsError::ChannelError(e.to_string()))?;

        self.subscriptions.confirm_subscribe(key);
        Ok(())
    }

    /// Subscribes to both open orders and fills (convenience method).
    pub async fn subscribe_executions(&self) -> Result<(), KrakenWsError> {
        self.subscribe_open_orders().await?;
        self.subscribe_fills().await?;
        Ok(())
    }
}
