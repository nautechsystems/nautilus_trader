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

//! WebSocket client for dYdX v4 API.
//!
//! This client provides streaming connectivity to dYdX's WebSocket API for both
//! public market data and private account updates.
//!
//! # Authentication
//!
//! dYdX v4 uses Cosmos SDK wallet-based authentication. Unlike traditional exchanges:
//! - **Public channels** require no authentication.
//! - **Private channels** (subaccounts) only require the wallet address in the subscription message.
//! - No signature or API key is needed for WebSocket connections themselves.
//!
//! # References
//!
//! <https://docs.dydx.trade/developers/indexer/websockets>

/// Pre-interned rate limit key for subscription operations (subscribe/unsubscribe).
///
/// dYdX allows up to 2 subscription messages per second per connection.
/// See: <https://docs.dydx.trade/developers/indexer/websockets#rate-limits>
pub static DYDX_RATE_LIMIT_KEY_SUBSCRIPTION: LazyLock<[Ustr; 1]> =
    LazyLock::new(|| [Ustr::from("subscription")]);

/// WebSocket topic delimiter for dYdX (channel:symbol format).
pub const DYDX_WS_TOPIC_DELIMITER: char = ':';

/// Default WebSocket quota for dYdX subscriptions (2 messages per second).
pub static DYDX_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> =
    LazyLock::new(|| Quota::per_second(NonZeroU32::new(2).expect("non-zero")));

use std::{
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
};

use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_common::live::get_runtime;
use nautilus_model::{
    identifiers::{AccountId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
};
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        AuthTracker, SubscriptionState, WebSocketClient, WebSocketConfig, channel_message_handler,
    },
};
use ustr::Ustr;

use super::{
    enums::{DydxWsChannel, DydxWsOperation, NautilusWsMessage},
    error::{DydxWsError, DydxWsResult},
    handler::{FeedHandler, HandlerCommand},
    messages::DydxSubscription,
};
use crate::common::credential::DydxCredential;

/// WebSocket client for dYdX v4 market data and account streams.
///
/// # Authentication
///
/// dYdX v4 does not require traditional API key signatures for WebSocket connections.
/// Public channels work without any credentials. Private channels (subaccounts) only
/// need the wallet address included in the subscription message.
///
/// The [`DydxCredential`] stored in this client is used for:
/// - Providing the wallet address for private channel subscriptions
/// - Transaction signing (when placing orders via the validator node)
///
/// It is **NOT** used for WebSocket message signing or authentication.
///
/// # Architecture
///
/// This client follows a two-layer architecture:
/// - **Outer client** (this struct): Orchestrates connection and maintains Python-accessible state
/// - **Inner handler**: Owns WebSocketClient exclusively and processes messages in a dedicated task
///
/// Communication uses lock-free channels:
/// - Commands flow from client → handler via `cmd_tx`
/// - Parsed events flow from handler → client via `out_rx`
#[derive(Debug)]
#[cfg_attr(
    feature = "python",
    pyo3::pyclass(module = "nautilus_trader.core.nautilus_pyo3.dydx")
)]
pub struct DydxWebSocketClient {
    /// The WebSocket connection URL.
    url: String,
    /// Optional credential for private channels (only wallet address is used).
    credential: Option<Arc<DydxCredential>>,
    /// Whether authentication is required for this client.
    requires_auth: bool,
    /// Authentication tracker for WebSocket connections.
    auth_tracker: AuthTracker,
    /// Subscription state tracker for managing channel subscriptions.
    subscriptions: SubscriptionState,
    /// Shared connection state (lock-free atomic).
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    /// Manual disconnect signal.
    signal: Arc<AtomicBool>,
    /// Cached instruments for parsing market data (Python-accessible).
    instruments_cache: Arc<DashMap<Ustr, InstrumentAny>>,
    /// Optional account ID for account message parsing.
    account_id: Option<AccountId>,
    /// Optional heartbeat interval in seconds.
    heartbeat: Option<u64>,
    /// Command channel sender to handler (wrapped in RwLock so updates are visible across clones).
    cmd_tx: Arc<tokio::sync::RwLock<tokio::sync::mpsc::UnboundedSender<HandlerCommand>>>,
    /// Receiver for parsed Nautilus messages from handler.
    out_rx: Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>>,
    /// Background handler task handle.
    handler_task: Option<tokio::task::JoinHandle<()>>,
}

impl Clone for DydxWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            credential: self.credential.clone(),
            requires_auth: self.requires_auth,
            auth_tracker: self.auth_tracker.clone(),
            subscriptions: self.subscriptions.clone(),
            connection_mode: self.connection_mode.clone(),
            signal: self.signal.clone(),
            instruments_cache: self.instruments_cache.clone(),
            account_id: self.account_id,
            heartbeat: self.heartbeat,
            cmd_tx: self.cmd_tx.clone(),
            out_rx: None,       // Cannot clone receiver - only one owner allowed
            handler_task: None, // Cannot clone task handle
        }
    }
}

impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    #[must_use]
    pub fn new_public(url: String, _heartbeat: Option<u64>) -> Self {
        // Create dummy command channel (will be replaced on connect)
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        Self {
            url,
            credential: None,
            requires_auth: false,
            auth_tracker: AuthTracker::new(),
            subscriptions: SubscriptionState::new(DYDX_WS_TOPIC_DELIMITER),
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed as u8,
            ))),
            signal: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: None,
            heartbeat: _heartbeat,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            handler_task: None,
        }
    }

    /// Creates a new private WebSocket client for account updates.
    #[must_use]
    pub fn new_private(
        url: String,
        credential: DydxCredential,
        account_id: AccountId,
        _heartbeat: Option<u64>,
    ) -> Self {
        // Create dummy command channel (will be replaced on connect)
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        Self {
            url,
            credential: Some(Arc::new(credential)),
            requires_auth: true,
            auth_tracker: AuthTracker::new(),
            subscriptions: SubscriptionState::new(DYDX_WS_TOPIC_DELIMITER),
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed as u8,
            ))),
            signal: Arc::new(AtomicBool::new(false)),
            instruments_cache: Arc::new(DashMap::new()),
            account_id: Some(account_id),
            heartbeat: _heartbeat,
            cmd_tx: Arc::new(tokio::sync::RwLock::new(cmd_tx)),
            out_rx: None,
            handler_task: None,
        }
    }

    /// Returns the credential associated with this client, if any.
    #[must_use]
    pub fn credential(&self) -> Option<&Arc<DydxCredential>> {
        self.credential.as_ref()
    }

    /// Returns `true` when the client is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        let mode = self.connection_mode.load();
        let mode_u8 = mode.load(Ordering::Relaxed);
        matches!(
            mode_u8,
            x if x == ConnectionMode::Active as u8 || x == ConnectionMode::Reconnect as u8
        )
    }

    /// Returns the URL of this WebSocket client.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns a clone of the connection mode atomic reference.
    ///
    /// This is primarily used for Python bindings that need to monitor connection state.
    #[must_use]
    pub fn connection_mode_atomic(&self) -> Arc<ArcSwap<AtomicU8>> {
        self.connection_mode.clone()
    }

    /// Sets the account ID for account message parsing.
    pub fn set_account_id(&mut self, account_id: AccountId) {
        self.account_id = Some(account_id);
    }

    /// Returns the account ID if set.
    #[must_use]
    pub fn account_id(&self) -> Option<AccountId> {
        self.account_id
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same ID will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        let symbol = instrument.id().symbol.inner();
        self.instruments_cache.insert(symbol, instrument.clone());

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::UpdateInstrument(Box::new(instrument)))
        {
            log::debug!("Failed to send UpdateInstrument command to handler: {e}");
        }
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same IDs will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        log::debug!(
            "Caching {} instruments in WebSocket client",
            instruments.len()
        );
        for instrument in &instruments {
            self.instruments_cache
                .insert(instrument.id().symbol.inner(), instrument.clone());
        }

        // Before connect() the handler isn't running; this send will fail and that's expected
        // because connect() replays the instruments via InitializeInstruments
        if !instruments.is_empty()
            && let Ok(cmd_tx) = self.cmd_tx.try_read()
            && let Err(e) = cmd_tx.send(HandlerCommand::InitializeInstruments(instruments))
        {
            log::debug!("Failed to send InitializeInstruments command to handler: {e}");
        }
    }

    /// Returns a reference to the instruments cache.
    #[must_use]
    pub fn instruments(&self) -> &Arc<DashMap<Ustr, InstrumentAny>> {
        &self.instruments_cache
    }

    /// Retrieves an instrument from the cache by symbol.
    ///
    /// Returns `None` if the instrument is not found.
    #[must_use]
    pub fn get_instrument(&self, symbol: &Ustr) -> Option<InstrumentAny> {
        self.instruments_cache.get(symbol).map(|r| r.clone())
    }

    /// Takes ownership of the inbound typed message receiver.
    /// Returns None if the receiver has already been taken or not connected.
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<NautilusWsMessage>> {
        self.out_rx.take()
    }

    /// Connects the websocket client in handler mode with automatic reconnection.
    ///
    /// Spawns a background handler task that owns the WebSocketClient and processes
    /// raw messages into typed [`NautilusWsMessage`] values.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> DydxWsResult<()> {
        if self.is_connected() {
            return Ok(());
        }

        // Reset stop signal from any previous disconnect
        self.signal.store(false, Ordering::Relaxed);

        let (message_handler, raw_rx) = channel_message_handler();

        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat: self.heartbeat,
            heartbeat_msg: None,
            reconnect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
        };

        let client = WebSocketClient::connect(
            cfg,
            Some(message_handler),
            None,
            None,
            vec![],
            Some(*DYDX_WS_SUBSCRIPTION_QUOTA),
        )
        .await
        .map_err(|e| DydxWsError::Transport(e.to_string()))?;

        // Update connection state atomically
        self.connection_mode.store(client.connection_mode_atomic());

        // Create fresh channels for this connection
        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();
        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<NautilusWsMessage>();

        // Update the shared cmd_tx so all clones see the new sender
        {
            let mut guard = self.cmd_tx.write().await;
            *guard = cmd_tx;
        }
        self.out_rx = Some(out_rx);

        // Replay cached instruments to the new handler
        if self.instruments_cache.is_empty() {
            log::warn!("No cached instruments to replay to WebSocket handler");
        } else {
            let cached_instruments: Vec<InstrumentAny> = self
                .instruments_cache
                .iter()
                .map(|entry| entry.value().clone())
                .collect();
            log::debug!(
                "Replaying {} cached instruments to WebSocket handler",
                cached_instruments.len()
            );
            let cmd_tx_guard = self.cmd_tx.read().await;
            if let Err(e) =
                cmd_tx_guard.send(HandlerCommand::InitializeInstruments(cached_instruments))
            {
                log::error!("Failed to replay instruments to handler: {e}");
            }
        }

        // Spawn handler task
        let account_id = self.account_id;
        let signal = self.signal.clone();
        let subscriptions = self.subscriptions.clone();

        let handler_task = get_runtime().spawn(async move {
            let mut handler = FeedHandler::new(
                account_id,
                cmd_rx,
                out_tx,
                raw_rx,
                client,
                signal,
                subscriptions,
            );
            handler.run().await;
        });

        self.handler_task = Some(handler_task);
        log::info!("Connected dYdX WebSocket: {}", self.url);
        Ok(())
    }

    /// Disconnects the websocket client.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying client cannot be accessed.
    pub async fn disconnect(&mut self) -> DydxWsResult<()> {
        // Set stop signal
        self.signal.store(true, Ordering::Relaxed);

        // Reset connection mode to Closed so is_connected() returns false
        // and subsequent connect() calls will create new channels
        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));

        // Abort handler task if it exists
        if let Some(handle) = self.handler_task.take() {
            handle.abort();
        }

        // Drop receiver to stop any consumers
        self.out_rx = None;

        log::info!("Disconnected dYdX WebSocket");
        Ok(())
    }

    /// Sends a text message via the handler.
    async fn send_text_inner(&self, text: &str) -> DydxWsResult<()> {
        self.cmd_tx
            .read()
            .await
            .send(HandlerCommand::SendText(text.to_string()))
            .map_err(|e| {
                DydxWsError::Transport(format!("Failed to send command to handler: {e}"))
            })?;
        Ok(())
    }

    /// Sends a command to the handler.
    ///
    /// # Errors
    ///
    /// Returns an error if the handler task has terminated.
    pub fn send_command(&self, cmd: HandlerCommand) -> DydxWsResult<()> {
        if let Ok(guard) = self.cmd_tx.try_read() {
            guard.send(cmd).map_err(|e| {
                DydxWsError::Transport(format!("Failed to send command to handler: {e}"))
            })?;
        } else {
            return Err(DydxWsError::Transport(
                "Failed to acquire lock on command channel".to_string(),
            ));
        }
        Ok(())
    }

    fn ticker_from_instrument_id(instrument_id: &InstrumentId) -> String {
        let mut s = instrument_id.symbol.as_str().to_string();
        if let Some(stripped) = s.strip_suffix("-PERP") {
            s = stripped.to_string();
        }
        s
    }

    fn topic(channel: DydxWsChannel, id: Option<&str>) -> String {
        match id {
            Some(id) => format!("{}{}{}", channel.as_ref(), DYDX_WS_TOPIC_DELIMITER, id),
            None => channel.as_ref().to_string(),
        }
    }

    async fn send_and_track_subscribe(
        &self,
        sub: DydxSubscription,
        topic: &str,
    ) -> DydxWsResult<()> {
        self.subscriptions.mark_subscribe(topic);

        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::RegisterSubscription {
                topic: topic.to_string(),
                subscription: sub.clone(),
            });
        }

        let payload = serde_json::to_string(&sub)?;
        if let Err(e) = self.send_text_inner(&payload).await {
            self.subscriptions.mark_failure(topic);
            self.subscriptions.remove_reference(topic);
            return Err(e);
        }
        Ok(())
    }

    async fn send_and_track_unsubscribe(
        &self,
        sub: DydxSubscription,
        topic: &str,
    ) -> DydxWsResult<()> {
        self.subscriptions.mark_unsubscribe(topic);

        let payload = serde_json::to_string(&sub)?;
        if let Err(e) = self.send_text_inner(&payload).await {
            self.subscriptions.add_reference(topic);
            self.subscriptions.mark_subscribe(topic);
            return Err(e);
        }

        if let Ok(cmd_tx) = self.cmd_tx.try_read() {
            let _ = cmd_tx.send(HandlerCommand::UnregisterSubscription {
                topic: topic.to_string(),
            });
        }

        Ok(())
    }

    /// Subscribes to public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#trades-channel>
    pub async fn subscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Trades, Some(&ticker));
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Trades,
            id: Some(ticker),
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Trades, Some(&ticker));
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Trades,
            id: Some(ticker),
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }

    /// Subscribes to orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#orderbook-channel>
    pub async fn subscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Orderbook, Some(&ticker));
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Orderbook,
            id: Some(ticker),
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Orderbook, Some(&ticker));
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Orderbook,
            id: Some(ticker),
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }

    /// Subscribes to candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#candles-channel>
    pub async fn subscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let id = format!("{ticker}/{resolution}");
        let topic = Self::topic(DydxWsChannel::Candles, Some(&id));
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Candles,
            id: Some(id),
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from candle/kline updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_candles(
        &self,
        instrument_id: InstrumentId,
        resolution: &str,
    ) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let id = format!("{ticker}/{resolution}");
        let topic = Self::topic(DydxWsChannel::Candles, Some(&id));
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Candles,
            id: Some(id),
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }

    /// Subscribes to market updates for all instruments.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#markets-channel>
    pub async fn subscribe_markets(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::Markets, None);
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Markets,
            id: None,
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from market updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_markets(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::Markets, None);
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Markets,
            id: None,
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }

    /// Subscribes to subaccount updates (orders, fills, positions, balances).
    ///
    /// This requires authentication and will only work for private WebSocket clients
    /// created with [`Self::new_private`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client was not created with credentials or if the
    /// subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#subaccounts-channel>
    pub async fn subscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        if !self.requires_auth {
            return Err(DydxWsError::Authentication(
                "Subaccount subscriptions require authentication. Use new_private() to create an authenticated client".to_string(),
            ));
        }
        let id = format!("{address}/{subaccount_number}");
        let topic = Self::topic(DydxWsChannel::Subaccounts, Some(&id));
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Subaccounts,
            id: Some(id),
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from subaccount updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_subaccount(
        &self,
        address: &str,
        subaccount_number: u32,
    ) -> DydxWsResult<()> {
        let id = format!("{address}/{subaccount_number}");
        let topic = Self::topic(DydxWsChannel::Subaccounts, Some(&id));
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Subaccounts,
            id: Some(id),
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }

    /// Subscribes to block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the subscription request fails.
    ///
    /// # References
    ///
    /// <https://docs.dydx.trade/developers/indexer/websockets#block-height-channel>
    pub async fn subscribe_block_height(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::BlockHeight, None);
        if !self.subscriptions.add_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::BlockHeight,
            id: None,
        };

        self.send_and_track_subscribe(sub, &topic).await
    }

    /// Unsubscribes from block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_block_height(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::BlockHeight, None);
        if !self.subscriptions.remove_reference(&topic) {
            return Ok(());
        }

        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::BlockHeight,
            id: None,
        };

        self.send_and_track_unsubscribe(sub, &topic).await
    }
}
