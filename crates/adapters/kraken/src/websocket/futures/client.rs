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
        Arc, RwLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use arc_swap::ArcSwap;
use nautilus_common::live::get_runtime;
use nautilus_core::AtomicMap;
use nautilus_model::{
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
    },
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

use super::{
    handler::{FuturesFeedHandler, FuturesHandlerCommand},
    messages::{
        KrakenFuturesChallengeRequest, KrakenFuturesEvent, KrakenFuturesFeed,
        KrakenFuturesPrivateSubscribeRequest, KrakenFuturesRequest, KrakenFuturesWsMessage,
    },
};
use crate::{
    common::{credential::KrakenCredential, parse::truncate_cl_ord_id},
    websocket::error::KrakenWsError,
};

/// Topic delimiter for Kraken Futures WebSocket subscriptions.
///
/// Topics use colon format: `feed:symbol` (e.g., `trades:PF_ETHUSD`).
pub const KRAKEN_FUTURES_WS_TOPIC_DELIMITER: char = ':';

/// WebSocket client for the Kraken Futures v1 streaming API.
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.kraken", from_py_object)
)]
#[cfg_attr(
    feature = "python",
    pyo3_stub_gen::derive::gen_stub_pyclass(module = "nautilus_trader.kraken")
)]
pub struct KrakenFuturesWebSocketClient {
    url: String,
    heartbeat_secs: u64,
    signal: Arc<AtomicBool>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<FuturesHandlerCommand>>>,
    out_rx: Option<Arc<tokio::sync::mpsc::UnboundedReceiver<KrakenFuturesWsMessage>>>,
    task_handle: Option<Arc<tokio::task::JoinHandle<()>>>,
    subscriptions: SubscriptionState,
    subscription_payloads: Arc<tokio::sync::RwLock<HashMap<String, String>>>,
    auth_tracker: AuthTracker,
    cancellation_token: CancellationToken,
    credential: Option<KrakenCredential>,
    original_challenge: Arc<tokio::sync::RwLock<Option<String>>>,
    signed_challenge: Arc<tokio::sync::RwLock<Option<String>>>,
    account_id: Arc<RwLock<Option<AccountId>>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: Arc<AtomicMap<String, InstrumentId>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
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
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
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
            proxy_url,
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
            .wait_for_result::<KrakenWsError>(tokio::time::Duration::from_secs(10), receiver)
            .await?;

        log::debug!("Futures WebSocket authentication successful");
        Ok(())
    }

    /// Connects to the WebSocket server.
    pub async fn connect(&mut self) -> Result<(), KrakenWsError> {
        log::debug!("Connecting to Futures WebSocket: {}", self.url);

        self.signal.store(false, Ordering::Relaxed);

        let (raw_handler, raw_rx) = channel_message_handler();

        let ws_config = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: Some(self.heartbeat_secs),
            heartbeat_msg: None, // Use WebSocket ping frames, not text messages
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

        let ws_client =
            WebSocketClient::connect(ws_config, Some(raw_handler), None, None, vec![], None)
                .await
                .map_err(|e| KrakenWsError::ConnectionError(e.to_string()))?;

        self.connection_mode
            .store(ws_client.connection_mode_atomic());

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<KrakenFuturesWsMessage>();
        self.out_rx = Some(Arc::new(out_rx));

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        *self.cmd_tx.write().await = cmd_tx.clone();

        if let Err(e) = cmd_tx.send(FuturesHandlerCommand::SetClient(ws_client)) {
            return Err(KrakenWsError::ConnectionError(format!(
                "Failed to send WebSocketClient to handler: {e}"
            )));
        }

        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();
        let subscription_payloads = self.subscription_payloads.clone();
        let cmd_tx_for_reconnect = cmd_tx.clone();
        let credential_for_reconnect = self.credential.clone();
        let original_challenge_for_reconnect = self.original_challenge.clone();
        let signed_challenge_for_reconnect = self.signed_challenge.clone();
        let auth_tracker_for_reconnect = self.auth_tracker.clone();

        let stream_handle = get_runtime().spawn(async move {
            let mut handler =
                FuturesFeedHandler::new(signal.clone(), cmd_rx, raw_rx, subscriptions.clone());
            let mut pending_resubscribe = false;

            loop {
                match handler.next().await {
                    Some(KrakenFuturesWsMessage::Reconnected) => {
                        if signal.load(Ordering::Relaxed) {
                            continue;
                        }
                        log::info!("WebSocket reconnected");

                        let confirmed_topics = subscriptions.all_topics();
                        for topic in &confirmed_topics {
                            subscriptions.mark_failure(topic);
                        }

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

                        if let Err(e) = out_tx.send(KrakenFuturesWsMessage::Reconnected) {
                            log::debug!("Output channel closed: {e}");
                            break;
                        }
                    }
                    Some(KrakenFuturesWsMessage::Challenge(challenge)) => {
                        let Some(ref cred) = credential_for_reconnect else {
                            log::warn!("Challenge received but no credentials configured");
                            auth_tracker_for_reconnect.fail("no credentials");
                            continue;
                        };

                        match cred.sign_ws_challenge(&challenge) {
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
                                        challenge.as_str(),
                                        signed.as_str(),
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
                        if let Err(e) = out_tx.send(msg) {
                            log::debug!("Output channel closed: {e}");
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
        });

        self.task_handle = Some(Arc::new(stream_handle));

        log::debug!("Futures WebSocket connected successfully");
        Ok(())
    }

    /// Disconnects from the WebSocket server.
    pub async fn disconnect(&mut self) -> Result<(), KrakenWsError> {
        log::debug!("Disconnecting Futures WebSocket");

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

        if let Some(task_handle) = self.task_handle.take() {
            match Arc::try_unwrap(task_handle) {
                Ok(handle) => {
                    match tokio::time::timeout(tokio::time::Duration::from_secs(2), handle).await {
                        Ok(Ok(())) => log::debug!("Task handle completed successfully"),
                        Ok(Err(e)) => log::error!("Task handle encountered an error: {e:?}"),
                        Err(_) => {
                            log::warn!("Timeout waiting for task handle");
                        }
                    }
                }
                Err(arc_handle) => {
                    log::debug!("Cannot take ownership of task handle, aborting");
                    arc_handle.abort();
                }
            }
        }

        self.subscriptions.clear();
        self.subscription_payloads.write().await.clear();
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
        if let Ok(mut guard) = self.account_id.write() {
            *guard = Some(account_id);
        }
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id.read().ok().and_then(|g| *g)
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

    /// Caches a client order for truncated ID resolution and instrument lookup.
    ///
    /// Kraken Futures limits client order IDs to 18 characters, so orders with
    /// longer IDs are truncated. This method stores the mapping from truncated
    /// to full ID, and from venue order ID to instrument ID for cancel messages.
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
    ) -> Result<String, KrakenWsError> {
        let request = KrakenFuturesRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed,
            product_ids,
        };
        let payload =
            serde_json::to_string(&request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;
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
        let payload =
            serde_json::to_string(&request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;
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
    ) -> Result<String, KrakenWsError> {
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

        let request = KrakenFuturesPrivateSubscribeRequest {
            event: KrakenFuturesEvent::Subscribe,
            feed,
            api_key: credential.api_key().to_string(),
            original_challenge,
            signed_challenge,
        };
        let payload =
            serde_json::to_string(&request).map_err(|e| KrakenWsError::JsonError(e.to_string()))?;
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
) -> Option<String> {
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
    serde_json::to_string(&value).ok()
}

fn build_challenge_payload(credential: &KrakenCredential) -> serde_json::Result<String> {
    let request = KrakenFuturesChallengeRequest {
        event: KrakenFuturesEvent::Challenge,
        api_key: credential.api_key().to_string(),
    };
    serde_json::to_string(&request)
}

fn is_private_feed_key(key: &str) -> bool {
    key == "open_orders" || key == "fills"
}

fn resubscribe_public(
    cmd_tx: &tokio::sync::mpsc::UnboundedSender<FuturesHandlerCommand>,
    subscriptions: &SubscriptionState,
    payloads: &HashMap<String, String>,
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
    payloads: &HashMap<String, String>,
    credential: &KrakenCredential,
    original_challenge: &str,
    signed_challenge: &str,
) {
    for (key, payload) in payloads {
        if !is_private_feed_key(key) {
            continue;
        }

        let Some(updated) = update_private_payload_credentials(
            payload,
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
    use base64::{Engine, engine::general_purpose::STANDARD};
    use nautilus_network::websocket::AuthTracker;
    use rstest::rstest;

    use super::*;

    fn test_credential() -> KrakenCredential {
        let secret = STANDARD.encode(b"test_secret_key_24bytes!");
        KrakenCredential::new("test_key", secret)
    }

    #[rstest]
    fn test_build_challenge_payload_emits_expected_event() {
        let credential = test_credential();
        let payload = build_challenge_payload(&credential).expect("serializes");
        assert!(payload.contains(r#""event":"challenge""#));
        assert!(payload.contains(r#""api_key":"test_key""#));
    }

    #[rstest]
    #[tokio::test]
    async fn test_resubscribe_public_skips_private_feeds() {
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel::<FuturesHandlerCommand>();
        let subscriptions = SubscriptionState::new(KRAKEN_FUTURES_WS_TOPIC_DELIMITER);

        let mut payloads = HashMap::new();
        payloads.insert(
            "trades:PI_XBTUSD".to_string(),
            r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
        );
        payloads.insert(
            "open_orders".to_string(),
            r#"{"event":"subscribe","feed":"open_orders"}"#.to_string(),
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
        assert!(subscribed[0].contains("PI_XBTUSD"));
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
            r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
        );

        resubscribe_public(&cmd_tx, &subscriptions, &payloads);

        match cmd_rx.try_recv().expect("public subscribe expected") {
            FuturesHandlerCommand::Subscribe { payload } => {
                assert!(payload.contains("PI_XBTUSD"));
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
            r#"{"event":"subscribe","feed":"open_orders","api_key":"","original_challenge":"","signed_challenge":""}"#.to_string(),
        );
        payloads.insert(
            "trades:PI_XBTUSD".to_string(),
            r#"{"event":"subscribe","feed":"trade","product_ids":["PI_XBTUSD"]}"#.to_string(),
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
            serde_json::from_str(&subscribed[0]).expect("payload is valid JSON");
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
