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
//! # Connection pool
//!
//! The Indexer caps each WebSocket connection at 32 subscriptions per channel
//! (`v4_trades`, `v4_candles`, `v4_orderbook`, `v4_markets`). To scale past that
//! limit the client maintains a small pool of connection slots and routes each
//! new subscription to the first slot with capacity, lazily spawning additional
//! connections up to `max_ws_connections`. The shape mirrors
//! `BinanceFuturesWebSocketClient` (including its `connect_lock` race fix),
//! adapted so capacity is tracked per channel kind rather than as a single flat
//! stream count.
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
pub static DYDX_WS_SUBSCRIPTION_QUOTA: LazyLock<Quota> = LazyLock::new(|| {
    Quota::per_second(NonZeroU32::new(2).expect("non-zero")).expect("valid constant")
});

/// Default maximum number of WebSocket connections in the Indexer pool.
pub const DEFAULT_MAX_WS_CONNECTIONS: usize = 8;

/// Default per-connection subscription limit for sharded channels.
pub const DEFAULT_PER_CHANNEL_SUBSCRIPTION_LIMIT: usize = 32;

use std::{
    num::NonZeroU32,
    sync::{
        Arc, LazyLock,
        atomic::{AtomicBool, AtomicU8, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use arc_swap::ArcSwap;
use dashmap::DashMap;
use nautilus_live::{
    SocketControl, SocketControlFactory,
    task::{TaskJoinOutcome, TaskSlot, finish_task},
};
use nautilus_model::{
    data::BarType,
    identifiers::{AccountId, InstrumentId},
    instruments::InstrumentAny,
};
use nautilus_network::{
    mode::ConnectionMode,
    ratelimiter::quota::Quota,
    websocket::{
        AuthTracker, SubscriptionState, TransportBackend, WebSocketClient, WebSocketConfig,
        channel_message_handler,
    },
};
use parking_lot::Mutex;
use ustr::Ustr;

use super::{
    dispatch::DydxWsDispatchState,
    enums::{DydxWsChannel, DydxWsOperation, DydxWsOutputMessage},
    error::{DydxWsError, DydxWsResult},
    handler::{FeedHandler, HandlerCommand},
    messages::DydxSubscription,
};
use crate::{
    common::{credential::DydxCredential, instrument_cache::InstrumentCache},
    execution::encoder::ClientOrderIdEncoder,
};

/// Identifies a dYdX channel for per-channel capacity accounting in the pool.
#[derive(Copy, Clone, Debug)]
#[repr(u8)]
enum ChannelKind {
    Trades = 0,
    Candles = 1,
    Orderbook = 2,
    Markets = 3,
}

const CHANNEL_KIND_COUNT: usize = 4;

/// Per-connection state inside the pool.
#[derive(Debug)]
struct ConnectionSlot {
    cmd_tx: tokio::sync::mpsc::UnboundedSender<HandlerCommand>,
    topics: AHashMap<String, u32>,
    channel_counts: [u16; CHANNEL_KIND_COUNT],
    subscriptions_state: SubscriptionState,
    handler_task: TaskSlot<()>,
    connection_mode: Arc<AtomicU8>,
    socket_control: Option<SocketControl>,
}

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
/// The client owns a small pool of connection slots. Each slot has its own
/// `WebSocketClient`, [`FeedHandler`] task, command channel, and
/// [`SubscriptionState`]. All slots write parsed events into a single shared
/// output channel so callers see one merged stream.
#[derive(Debug)]
pub struct DydxWebSocketClient {
    url: String,
    credential: Option<Arc<DydxCredential>>,
    requires_auth: bool,
    auth_tracker: AuthTracker,
    slots: Arc<ConnectionSlots>,
    admission: Arc<Mutex<PoolAdmission>>,
    connect_lock: Arc<tokio::sync::Mutex<()>>,
    connection_mode: Arc<ArcSwap<AtomicU8>>,
    signal: Arc<AtomicBool>,
    instrument_cache: Arc<InstrumentCache>,
    account_id: Option<AccountId>,
    heartbeat: Option<u64>,
    out_tx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedSender<DydxWsOutputMessage>>>>,
    out_rx: Arc<Mutex<Option<tokio::sync::mpsc::UnboundedReceiver<DydxWsOutputMessage>>>>,
    encoder: Arc<ClientOrderIdEncoder>,
    bar_types: Arc<DashMap<String, BarType>>,
    bars_timestamp_on_close: Arc<AtomicBool>,
    ws_dispatch_state: Arc<DydxWsDispatchState>,
    transport_backend: TransportBackend,
    proxy_url: Option<String>,
    max_ws_connections: usize,
    per_channel_limit: usize,
    socket_factory: Option<SocketControlFactory>,
}

impl Clone for DydxWebSocketClient {
    fn clone(&self) -> Self {
        Self {
            url: self.url.clone(),
            credential: self.credential.clone(),
            requires_auth: self.requires_auth,
            auth_tracker: self.auth_tracker.clone(),
            slots: self.slots.clone(),
            admission: self.admission.clone(),
            connect_lock: self.connect_lock.clone(),
            connection_mode: self.connection_mode.clone(),
            signal: self.signal.clone(),
            instrument_cache: self.instrument_cache.clone(),
            account_id: self.account_id,
            heartbeat: self.heartbeat,
            out_tx: self.out_tx.clone(),
            out_rx: self.out_rx.clone(),
            encoder: self.encoder.clone(),
            bar_types: self.bar_types.clone(),
            bars_timestamp_on_close: self.bars_timestamp_on_close.clone(),
            ws_dispatch_state: self.ws_dispatch_state.clone(),
            transport_backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
            max_ws_connections: self.max_ws_connections,
            per_channel_limit: self.per_channel_limit,
            socket_factory: self.socket_factory.clone(),
        }
    }
}

impl DydxWebSocketClient {
    /// Creates a new public WebSocket client for market data.
    ///
    /// This creates a new independent instrument cache. To share a cache with
    /// the HTTP client, use [`Self::new_public_with_cache`] instead.
    #[must_use]
    pub fn new_public(url: String, heartbeat: Option<u64>, proxy_url: Option<String>) -> Self {
        Self::new_public_with_cache(
            url,
            Arc::new(InstrumentCache::new()),
            heartbeat,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Creates a new public WebSocket client with a shared instrument cache.
    ///
    /// Use this when you want to share instrument data with the HTTP client.
    #[must_use]
    pub fn new_public_with_cache(
        url: String,
        instrument_cache: Arc<InstrumentCache>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_public_with_cache_and_pool(
            url,
            instrument_cache,
            heartbeat,
            transport_backend,
            proxy_url,
            DEFAULT_MAX_WS_CONNECTIONS,
            DEFAULT_PER_CHANNEL_SUBSCRIPTION_LIMIT,
        )
    }

    /// Creates a new public WebSocket client with full pool configuration.
    #[must_use]
    pub fn new_public_with_cache_and_pool(
        url: String,
        instrument_cache: Arc<InstrumentCache>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        max_ws_connections: usize,
        per_channel_limit: usize,
    ) -> Self {
        Self::new_inner(
            url,
            None,
            false,
            instrument_cache,
            None,
            heartbeat,
            transport_backend,
            proxy_url,
            max_ws_connections,
            per_channel_limit,
        )
    }

    /// Creates a new private WebSocket client for account updates.
    ///
    /// This creates a new independent instrument cache. To share a cache with
    /// the HTTP client, use [`Self::new_private_with_cache`] instead.
    #[must_use]
    pub fn new_private(
        url: String,
        credential: DydxCredential,
        account_id: AccountId,
        heartbeat: Option<u64>,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_private_with_cache(
            url,
            credential,
            account_id,
            Arc::new(InstrumentCache::new()),
            heartbeat,
            TransportBackend::default(),
            proxy_url,
        )
    }

    /// Creates a new private WebSocket client with a shared instrument cache.
    ///
    /// Use this when you want to share instrument data with the HTTP client.
    #[must_use]
    pub fn new_private_with_cache(
        url: String,
        credential: DydxCredential,
        account_id: AccountId,
        instrument_cache: Arc<InstrumentCache>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
    ) -> Self {
        Self::new_inner(
            url,
            Some(Arc::new(credential)),
            true,
            instrument_cache,
            Some(account_id),
            heartbeat,
            transport_backend,
            proxy_url,
            DEFAULT_MAX_WS_CONNECTIONS,
            DEFAULT_PER_CHANNEL_SUBSCRIPTION_LIMIT,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn new_inner(
        url: String,
        credential: Option<Arc<DydxCredential>>,
        requires_auth: bool,
        instrument_cache: Arc<InstrumentCache>,
        account_id: Option<AccountId>,
        heartbeat: Option<u64>,
        transport_backend: TransportBackend,
        proxy_url: Option<String>,
        max_ws_connections: usize,
        per_channel_limit: usize,
    ) -> Self {
        Self {
            url,
            credential,
            requires_auth,
            auth_tracker: AuthTracker::new(),
            slots: Arc::new(ConnectionSlots::new()),
            admission: Arc::new(Mutex::new(PoolAdmission {
                generation: 0,
                closed: false,
            })),
            connect_lock: Arc::new(tokio::sync::Mutex::new(())),
            connection_mode: Arc::new(ArcSwap::from_pointee(AtomicU8::new(
                ConnectionMode::Closed as u8,
            ))),
            signal: Arc::new(AtomicBool::new(false)),
            instrument_cache,
            account_id,
            heartbeat,
            out_tx: Arc::new(Mutex::new(None)),
            out_rx: Arc::new(Mutex::new(None)),
            encoder: Arc::new(ClientOrderIdEncoder::new()),
            bar_types: Arc::new(DashMap::new()),
            bars_timestamp_on_close: Arc::new(AtomicBool::new(true)),
            ws_dispatch_state: Arc::new(DydxWsDispatchState::default()),
            transport_backend,
            proxy_url,
            max_ws_connections: max_ws_connections.max(1),
            per_channel_limit: per_channel_limit.max(1),
            socket_factory: None,
        }
    }

    pub(crate) fn begin_shutdown(&self) {
        let mut admission = self.admission.lock();
        admission.generation = admission.generation.wrapping_add(1);
        admission.closed = true;
        self.signal.store(true, Ordering::Release);

        for slot in self.slots.lock().iter() {
            let _ = slot.cmd_tx.send(HandlerCommand::Disconnect);
        }
    }

    fn open_generation(&self) -> u64 {
        let mut admission = self.admission.lock();
        admission.generation = admission.generation.wrapping_add(1);
        admission.closed = false;
        admission.generation
    }

    fn admission_generation(&self) -> DydxWsResult<u64> {
        let admission = self.admission.lock();
        if admission.closed {
            Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ))
        } else {
            Ok(admission.generation)
        }
    }

    /// Configures socket state reporting and reconnect control for each pool slot.
    #[must_use]
    pub fn with_socket_factory(mut self, factory: SocketControlFactory) -> Self {
        self.socket_factory = Some(factory);
        self
    }

    /// Returns the credential associated with this client, if any.
    #[must_use]
    pub fn credential(&self) -> Option<&Arc<DydxCredential>> {
        self.credential.as_ref()
    }

    /// Returns `true` when any connection in the pool is connected.
    #[must_use]
    pub fn is_connected(&self) -> bool {
        let slots = self.slots.lock();
        slots.iter().any(|s| {
            let mode = s.connection_mode.load(Ordering::Relaxed);
            mode == ConnectionMode::Active as u8 || mode == ConnectionMode::Reconnect as u8
        })
    }

    /// Returns the URL of this WebSocket client.
    #[must_use]
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Returns a clone of the connection mode atomic reference.
    ///
    /// With sharding, the returned atomic tracks the **primary** slot (slot 0)
    /// only; use [`Self::is_connected`] for a pool-wide check.
    #[must_use]
    pub fn connection_mode_atomic(&self) -> Arc<ArcSwap<AtomicU8>> {
        self.connection_mode.clone()
    }

    /// Returns the current number of active slots in the pool.
    #[must_use]
    pub fn pool_size(&self) -> usize {
        self.slots.lock().len()
    }

    /// Returns the configured maximum number of pool connections.
    #[must_use]
    pub const fn max_ws_connections(&self) -> usize {
        self.max_ws_connections
    }

    /// Returns the configured per-channel subscription limit.
    #[must_use]
    pub const fn per_channel_limit(&self) -> usize {
        self.per_channel_limit
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

    /// Replaces the instrument cache with an externally shared one.
    ///
    /// Use this to share the HTTP client's cache (which includes CLOB pair ID
    /// and market ticker indices) with the WebSocket client. Must be called
    /// before `connect()`.
    pub fn set_instrument_cache(&mut self, cache: Arc<InstrumentCache>) {
        self.instrument_cache = cache;
    }

    /// Caches a single instrument.
    ///
    /// Any existing instrument with the same ID will be replaced.
    pub fn cache_instrument(&self, instrument: InstrumentAny) {
        self.instrument_cache.insert_instrument_only(instrument);
    }

    /// Caches multiple instruments.
    ///
    /// Any existing instruments with the same IDs will be replaced.
    pub fn cache_instruments(&self, instruments: Vec<InstrumentAny>) {
        log::debug!(
            "Caching {} instruments in WebSocket client",
            instruments.len()
        );
        self.instrument_cache.insert_instruments_only(instruments);
    }

    /// Returns a reference to the shared instrument cache.
    #[must_use]
    pub fn instrument_cache(&self) -> &Arc<InstrumentCache> {
        &self.instrument_cache
    }

    /// Returns a reference to the shared client order ID encoder.
    #[must_use]
    pub fn encoder(&self) -> &Arc<ClientOrderIdEncoder> {
        &self.encoder
    }

    /// Returns a reference to the bar type registrations map.
    #[must_use]
    pub fn bar_types(&self) -> &Arc<DashMap<String, BarType>> {
        &self.bar_types
    }

    /// Returns a reference to the shared WebSocket dispatch state.
    pub fn ws_dispatch_state(&self) -> &Arc<DydxWsDispatchState> {
        &self.ws_dispatch_state
    }

    /// Sets whether bar timestamps use the close time.
    pub fn set_bars_timestamp_on_close(&self, value: bool) {
        self.bars_timestamp_on_close.store(value, Ordering::Relaxed);
    }

    /// Returns whether bar timestamps use the close time.
    #[must_use]
    pub fn bars_timestamp_on_close(&self) -> bool {
        self.bars_timestamp_on_close.load(Ordering::Relaxed)
    }

    /// Returns all cached instruments.
    ///
    /// This is a snapshot of the current cache contents.
    #[must_use]
    pub fn all_instruments(&self) -> Vec<InstrumentAny> {
        self.instrument_cache.all_instruments()
    }

    /// Returns the number of cached instruments.
    #[must_use]
    pub fn cached_instruments_count(&self) -> usize {
        self.instrument_cache.len()
    }

    /// Retrieves an instrument from the cache by InstrumentId.
    ///
    /// Returns `None` if the instrument is not found.
    #[must_use]
    pub fn get_instrument(&self, instrument_id: &InstrumentId) -> Option<InstrumentAny> {
        self.instrument_cache.get(instrument_id)
    }

    /// Retrieves an instrument from the cache by market ticker (e.g., "BTC-USD").
    ///
    /// Returns `None` if the instrument is not found.
    #[must_use]
    pub fn get_instrument_by_market(&self, ticker: &str) -> Option<InstrumentAny> {
        self.instrument_cache.get_by_market(ticker)
    }

    /// Takes ownership of the inbound message receiver.
    /// Returns None if the receiver has already been taken or not connected.
    pub fn take_receiver(
        &mut self,
    ) -> Option<tokio::sync::mpsc::UnboundedReceiver<DydxWsOutputMessage>> {
        self.out_rx.lock().take()
    }

    /// Returns a stream of venue-specific WebSocket messages.
    ///
    /// Takes ownership of the message receiver and returns it as a `Stream`.
    ///
    /// # Panics
    ///
    /// Panics if the message receiver has already been taken or the client is not connected.
    pub fn stream(
        &mut self,
    ) -> impl futures_util::Stream<Item = DydxWsOutputMessage> + Send + 'static {
        let mut rx = self
            .out_rx
            .lock()
            .take()
            .expect("Message stream receiver already taken or not connected");

        async_stream::stream! {
            while let Some(msg) = rx.recv().await {
                yield msg;
            }
        }
    }

    /// Connects the websocket client and opens the primary pool slot.
    ///
    /// Additional slots are spawned lazily by `subscribe_*` methods once the
    /// per-channel limit is reached on every existing slot.
    ///
    /// # Errors
    ///
    /// Returns an error if the connection cannot be established.
    pub async fn connect(&mut self) -> DydxWsResult<()> {
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;

        let already_connected = {
            let admission = self.admission.lock();
            !admission.closed && self.is_connected()
        };

        if already_connected {
            return Ok(());
        }

        if !self.slots.lock().is_empty() {
            self.disconnect_connections().await?;
        }

        let generation = self.open_generation();
        self.signal.store(false, Ordering::Release);

        let (out_tx, out_rx) = tokio::sync::mpsc::unbounded_channel::<DydxWsOutputMessage>();
        {
            let mut guard = self.out_tx.lock();
            *guard = Some(out_tx);
        }
        {
            let mut guard = self.out_rx.lock();
            *guard = Some(out_rx);
        }

        let slot = match self.create_connection(0).await {
            Ok(slot) => slot,
            Err(e) => {
                self.begin_shutdown();
                *self.out_tx.lock() = None;
                *self.out_rx.lock() = None;
                return Err(e);
            }
        };
        let admission = self.admission.lock();
        let mut slots = self.slots.lock();

        if admission.closed || admission.generation != generation {
            let _ = slot.cmd_tx.send(HandlerCommand::Disconnect);
            slots.push(slot);
            return Err(DydxWsError::Transport(
                "WebSocket connection was canceled by shutdown".to_string(),
            ));
        }
        self.connection_mode.store(slot.connection_mode.clone());
        slots.push(slot);
        drop(slots);
        drop(admission);

        log::debug!("Connected dYdX WebSocket pool: {}", self.url);
        Ok(())
    }

    /// Disconnects all websocket connections in the pool.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying clients cannot be accessed.
    pub async fn disconnect(&mut self) -> DydxWsResult<()> {
        self.begin_shutdown();
        let connect_lock = Arc::clone(&self.connect_lock);
        let _connect_guard = connect_lock.lock().await;
        self.disconnect_connections().await
    }

    async fn disconnect_connections(&self) -> DydxWsResult<()> {
        self.begin_shutdown();

        let mut slots = ConnectionSlotBatch::take(&self.slots);

        for slot in &mut slots.slots {
            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
            let _ = slot.cmd_tx.send(HandlerCommand::Disconnect);

            if let Some(outcome) = finish_task(
                &mut slot.handler_task,
                Duration::from_secs(2),
                Duration::from_secs(2),
            )
            .await
            {
                match outcome {
                    TaskJoinOutcome::Completed(()) => log::debug!("Handler task completed"),
                    TaskJoinOutcome::Aborted => {}
                    TaskJoinOutcome::Failed(error) => {
                        self.slots
                            .push_shutdown_error(format!("handler task failed: {error}"));
                    }
                    TaskJoinOutcome::Incomplete => {
                        self.slots.push_shutdown_error(
                            "handler task did not stop after abort".to_string(),
                        );
                    }
                }
            }
        }

        slots.slots.retain(|slot| slot.handler_task.is_some());
        let has_incomplete_tasks = !slots.slots.is_empty();

        if !has_incomplete_tasks {
            self.connection_mode
                .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));
            *self.out_tx.lock() = None;
            *self.out_rx.lock() = None;
        }

        let join_errors = self.slots.take_shutdown_errors();
        if !join_errors.is_empty() {
            return Err(DydxWsError::Transport(join_errors.join("; ")));
        }

        self.connection_mode
            .store(Arc::new(AtomicU8::new(ConnectionMode::Closed as u8)));

        log::debug!("Disconnected dYdX WebSocket pool");
        Ok(())
    }

    /// Sends a command directly to the primary slot (slot 0).
    ///
    /// # Errors
    ///
    /// Returns an error if no slot exists or the handler task has terminated.
    pub fn send_command(&self, cmd: HandlerCommand) -> DydxWsResult<()> {
        let admission = self.admission.lock();
        if admission.closed {
            return Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ));
        }
        let slots = self.slots.lock();
        let slot = slots
            .first()
            .ok_or_else(|| DydxWsError::Transport("No pool slots available".to_string()))?;
        slot.cmd_tx.send(cmd).map_err(|e| {
            DydxWsError::Transport(format!("Failed to send command to slot 0: {e}"))
        })?;
        Ok(())
    }

    async fn create_connection(&self, slot_index: usize) -> DydxWsResult<ConnectionSlot> {
        let (message_handler, raw_rx) = channel_message_handler();

        let cfg = WebSocketConfig {
            url: self.url.clone(),
            headers: vec![],
            heartbeat_interval_secs: self.heartbeat,
            heartbeat_payload: None,
            connect_timeout_ms: Some(15_000),
            reconnect_delay_initial_ms: Some(250),
            reconnect_delay_max_ms: Some(5_000),
            reconnect_backoff_factor: Some(2.0),
            reconnect_jitter_ms: Some(200),
            reconnect_max_attempts: None,
            heartbeat_timeout_secs: None,
            idle_timeout_ms: None,
            backend: self.transport_backend,
            proxy_url: self.proxy_url.clone(),
        };

        let socket_control = self.socket_factory.as_ref().map(|factory| {
            let kind = if self.requires_auth { "user" } else { "data" };
            let endpoint = format!("dydx-{kind}-streams");
            if slot_index == 0 {
                factory.control(endpoint)
            } else {
                factory.control(format!("{endpoint}-{slot_index}"))
            }
        });
        let client = WebSocketClient::builder()
            .config(cfg)
            .message_handler(message_handler)
            .default_quota(*DYDX_WS_SUBSCRIPTION_QUOTA)
            .maybe_state_sink(
                socket_control
                    .as_ref()
                    .map(nautilus_live::SocketControl::sink),
            )
            .connect()
            .await
            .map_err(|e| DydxWsError::Transport(e.to_string()))?;

        let connection_mode = client.connection_mode_atomic();
        let reconnect_handle = client.reconnect_handle();
        let subscriptions_state = SubscriptionState::new(DYDX_WS_TOPIC_DELIMITER);

        let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel::<HandlerCommand>();

        let out_tx =
            self.out_tx.lock().clone().ok_or_else(|| {
                DydxWsError::Transport("Output channel not initialized".to_string())
            })?;

        let signal = self.signal.clone();
        let subscriptions = subscriptions_state.clone();

        let mut handler_task = TaskSlot::new();
        if let Err(e) = handler_task.spawn(async move {
            let mut handler =
                FeedHandler::new(cmd_rx, out_tx, raw_rx, client, signal, subscriptions);
            handler.run().await;
        }) {
            let shutdown_error = match finish_task(
                &mut handler_task,
                std::time::Duration::ZERO,
                std::time::Duration::from_secs(2),
            )
            .await
            {
                Some(TaskJoinOutcome::Failed(error)) => {
                    Some(format!("handler task failed: {error}"))
                }
                Some(TaskJoinOutcome::Incomplete) => {
                    Some("handler task did not stop after abort".to_string())
                }
                None | Some(TaskJoinOutcome::Completed(()) | TaskJoinOutcome::Aborted) => None,
            };
            return Err(DydxWsError::Transport(match shutdown_error {
                Some(shutdown_error) => format!(
                    "Failed to start handler task: {e}; startup rollback failed: {shutdown_error}"
                ),
                None => format!("Failed to start handler task: {e}"),
            }));
        }

        if let Some(control) = &socket_control {
            control.register(move || reconnect_handle.request_reconnect());
        }

        Ok(ConnectionSlot {
            cmd_tx,
            topics: AHashMap::new(),
            channel_counts: [0; CHANNEL_KIND_COUNT],
            subscriptions_state,
            handler_task,
            connection_mode,
            socket_control,
        })
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

    async fn subscribe_topic(
        &self,
        channel: ChannelKind,
        topic: String,
        sub_msg: DydxSubscription,
    ) -> DydxWsResult<()> {
        let _connect_guard = self.connect_lock.lock().await;
        let generation = self.admission_generation()?;

        {
            let admission = self.admission.lock();
            if admission.closed || admission.generation != generation {
                return Err(DydxWsError::Transport(
                    "WebSocket connection pool is closed".to_string(),
                ));
            }
            let mut slots = self.slots.lock();
            if let Some(slot) = slots.iter_mut().find(|s| s.topics.contains_key(&topic)) {
                *slot.topics.get_mut(&topic).expect("topic refcount present") += 1;
                return Ok(());
            }
        }

        let target_idx = loop {
            {
                let admission = self.admission.lock();
                if admission.closed || admission.generation != generation {
                    return Err(DydxWsError::Transport(
                        "WebSocket connection pool is closed".to_string(),
                    ));
                }
                let slots = self.slots.lock();
                if let Some(idx) = slots.iter().position(|s| {
                    (s.channel_counts[channel as usize] as usize) < self.per_channel_limit
                }) {
                    break idx;
                }

                if slots.len() >= self.max_ws_connections {
                    return Err(DydxWsError::Subscription(format!(
                        "Pool exhausted: {} connections x {} {:?} subscriptions",
                        self.max_ws_connections, self.per_channel_limit, channel,
                    )));
                }
            }

            let slot_index = self.slots.lock().len();
            let new_slot = self.create_connection(slot_index).await?;
            let new_idx = {
                let admission = self.admission.lock();
                let mut slots = self.slots.lock();

                if admission.closed || admission.generation != generation {
                    let _ = new_slot.cmd_tx.send(HandlerCommand::Disconnect);
                    slots.push(new_slot);
                    return Err(DydxWsError::Transport(
                        "WebSocket connection was canceled by shutdown".to_string(),
                    ));
                }
                slots.push(new_slot);
                slots.len() - 1
            };
            log::debug!(
                "dYdX pool slot {new_idx} connected: url={}, channel={:?}",
                self.url,
                channel,
            );
        };

        let admission = self.admission.lock();
        if admission.closed || admission.generation != generation {
            return Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ));
        }
        let mut slots = self.slots.lock();
        let slot = &mut slots[target_idx];

        slot.subscriptions_state.mark_subscribe(&topic);
        slot.cmd_tx
            .send(HandlerCommand::RegisterSubscription {
                topic: topic.clone(),
                subscription: sub_msg.clone(),
            })
            .map_err(|e| {
                slot.subscriptions_state.mark_failure(&topic);
                DydxWsError::Transport(format!("Slot {target_idx} unavailable: {e}"))
            })?;

        let payload = serde_json::to_string(&sub_msg)?;
        if let Err(e) = slot.cmd_tx.send(HandlerCommand::SendText(payload)) {
            slot.subscriptions_state.mark_failure(&topic);
            let _ = slot.cmd_tx.send(HandlerCommand::UnregisterSubscription {
                topic: topic.clone(),
            });
            return Err(DydxWsError::Transport(format!(
                "Slot {target_idx} send failed: {e}"
            )));
        }

        slot.topics.insert(topic, 1);
        slot.channel_counts[channel as usize] =
            slot.channel_counts[channel as usize].saturating_add(1);

        Ok(())
    }

    async fn unsubscribe_topic(
        &self,
        channel: ChannelKind,
        topic: String,
        unsub_msg: DydxSubscription,
    ) -> DydxWsResult<()> {
        let _connect_guard = self.connect_lock.lock().await;
        let _generation = self.admission_generation()?;
        let admission = self.admission.lock();
        if admission.closed {
            return Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ));
        }
        let mut slots = self.slots.lock();
        let Some(slot_idx) = slots.iter().position(|s| s.topics.contains_key(&topic)) else {
            return Ok(());
        };

        let slot = &mut slots[slot_idx];
        let refcount = slot.topics.get_mut(&topic).expect("topic present");
        if *refcount > 1 {
            *refcount -= 1;
            return Ok(());
        }

        slot.subscriptions_state.mark_unsubscribe(&topic);
        let payload = serde_json::to_string(&unsub_msg)?;
        if let Err(e) = slot.cmd_tx.send(HandlerCommand::SendText(payload)) {
            slot.subscriptions_state.mark_subscribe(&topic);
            return Err(DydxWsError::Transport(format!(
                "Slot {slot_idx} send failed: {e}"
            )));
        }
        let _ = slot.cmd_tx.send(HandlerCommand::UnregisterSubscription {
            topic: topic.clone(),
        });

        slot.topics.remove(&topic);
        slot.channel_counts[channel as usize] =
            slot.channel_counts[channel as usize].saturating_sub(1);

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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Trades,
            id: Some(ticker),
        };
        self.subscribe_topic(ChannelKind::Trades, topic, sub).await
    }

    /// Unsubscribes from public trade updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_trades(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Trades, Some(&ticker));
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Trades,
            id: Some(ticker),
        };
        self.unsubscribe_topic(ChannelKind::Trades, topic, sub)
            .await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Orderbook,
            id: Some(ticker),
        };
        self.subscribe_topic(ChannelKind::Orderbook, topic, sub)
            .await
    }

    /// Unsubscribes from orderbook updates for a specific instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_orderbook(&self, instrument_id: InstrumentId) -> DydxWsResult<()> {
        let ticker = Self::ticker_from_instrument_id(&instrument_id);
        let topic = Self::topic(DydxWsChannel::Orderbook, Some(&ticker));
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Orderbook,
            id: Some(ticker),
        };
        self.unsubscribe_topic(ChannelKind::Orderbook, topic, sub)
            .await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Candles,
            id: Some(id),
        };
        self.subscribe_topic(ChannelKind::Candles, topic, sub).await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Candles,
            id: Some(id),
        };
        self.unsubscribe_topic(ChannelKind::Candles, topic, sub)
            .await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Markets,
            id: None,
        };
        self.subscribe_topic(ChannelKind::Markets, topic, sub).await
    }

    /// Unsubscribes from market updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_markets(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::Markets, None);
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Markets,
            id: None,
        };
        self.unsubscribe_topic(ChannelKind::Markets, topic, sub)
            .await
    }

    /// Subscribes to subaccount updates (orders, fills, positions, balances).
    ///
    /// This requires authentication and will only work for private WebSocket clients
    /// created with [`Self::new_private`]. Subaccount streams stay pinned to the
    /// primary slot: the Indexer caps them at 256 per connection, which is well
    /// above realistic per-process usage and keeps related fill/position events
    /// on a single in-order stream.
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::Subaccounts,
            id: Some(id),
        };
        self.subscribe_pinned(topic, sub).await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::Subaccounts,
            id: Some(id),
        };
        self.unsubscribe_pinned(topic, sub).await
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
        let sub = DydxSubscription {
            op: DydxWsOperation::Subscribe,
            channel: DydxWsChannel::BlockHeight,
            id: None,
        };
        self.subscribe_pinned(topic, sub).await
    }

    /// Unsubscribes from block height updates.
    ///
    /// # Errors
    ///
    /// Returns an error if the unsubscription request fails.
    pub async fn unsubscribe_block_height(&self) -> DydxWsResult<()> {
        let topic = Self::topic(DydxWsChannel::BlockHeight, None);
        let sub = DydxSubscription {
            op: DydxWsOperation::Unsubscribe,
            channel: DydxWsChannel::BlockHeight,
            id: None,
        };
        self.unsubscribe_pinned(topic, sub).await
    }

    async fn subscribe_pinned(&self, topic: String, sub_msg: DydxSubscription) -> DydxWsResult<()> {
        let _connect_guard = self.connect_lock.lock().await;
        let generation = self.admission_generation()?;

        {
            let admission = self.admission.lock();
            if admission.closed || admission.generation != generation {
                return Err(DydxWsError::Transport(
                    "WebSocket connection pool is closed".to_string(),
                ));
            }
            let mut slots = self.slots.lock();
            if let Some(slot) = slots.iter_mut().find(|s| s.topics.contains_key(&topic)) {
                *slot.topics.get_mut(&topic).expect("topic refcount present") += 1;
                return Ok(());
            }
        }

        if self.slots.lock().is_empty() {
            let new_slot = self.create_connection(0).await?;
            let admission = self.admission.lock();
            let mut slots = self.slots.lock();

            if admission.closed || admission.generation != generation {
                let _ = new_slot.cmd_tx.send(HandlerCommand::Disconnect);
                slots.push(new_slot);
                return Err(DydxWsError::Transport(
                    "WebSocket connection was canceled by shutdown".to_string(),
                ));
            }
            self.connection_mode.store(new_slot.connection_mode.clone());
            slots.push(new_slot);
        }

        let admission = self.admission.lock();
        if admission.closed || admission.generation != generation {
            return Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ));
        }
        let mut slots = self.slots.lock();
        let slot = slots.first_mut().expect("primary slot exists");
        slot.subscriptions_state.mark_subscribe(&topic);
        slot.cmd_tx
            .send(HandlerCommand::RegisterSubscription {
                topic: topic.clone(),
                subscription: sub_msg.clone(),
            })
            .map_err(|e| {
                slot.subscriptions_state.mark_failure(&topic);
                DydxWsError::Transport(format!("Primary slot unavailable: {e}"))
            })?;
        let payload = serde_json::to_string(&sub_msg)?;
        if let Err(e) = slot.cmd_tx.send(HandlerCommand::SendText(payload)) {
            slot.subscriptions_state.mark_failure(&topic);
            let _ = slot.cmd_tx.send(HandlerCommand::UnregisterSubscription {
                topic: topic.clone(),
            });
            return Err(DydxWsError::Transport(format!(
                "Primary slot send failed: {e}"
            )));
        }
        slot.topics.insert(topic, 1);
        Ok(())
    }

    async fn unsubscribe_pinned(
        &self,
        topic: String,
        unsub_msg: DydxSubscription,
    ) -> DydxWsResult<()> {
        let _connect_guard = self.connect_lock.lock().await;
        let _generation = self.admission_generation()?;
        let admission = self.admission.lock();
        if admission.closed {
            return Err(DydxWsError::Transport(
                "WebSocket connection pool is closed".to_string(),
            ));
        }
        let mut slots = self.slots.lock();
        let Some(slot) = slots.first_mut() else {
            return Ok(());
        };
        let Some(refcount) = slot.topics.get_mut(&topic) else {
            return Ok(());
        };

        if *refcount > 1 {
            *refcount -= 1;
            return Ok(());
        }
        slot.subscriptions_state.mark_unsubscribe(&topic);
        let payload = serde_json::to_string(&unsub_msg)?;
        if let Err(e) = slot.cmd_tx.send(HandlerCommand::SendText(payload)) {
            slot.subscriptions_state.mark_subscribe(&topic);
            return Err(DydxWsError::Transport(format!(
                "Primary slot send failed: {e}"
            )));
        }
        let _ = slot.cmd_tx.send(HandlerCommand::UnregisterSubscription {
            topic: topic.clone(),
        });
        slot.topics.remove(&topic);
        Ok(())
    }
}

#[derive(Debug)]
struct ConnectionSlots {
    slots: Mutex<Vec<ConnectionSlot>>,
    shutdown_errors: Mutex<Vec<String>>,
}

impl ConnectionSlots {
    fn new() -> Self {
        Self {
            slots: Mutex::new(Vec::new()),
            shutdown_errors: Mutex::new(Vec::new()),
        }
    }

    fn push_shutdown_error(&self, error: String) {
        self.shutdown_errors.lock().push(error);
    }

    fn take_shutdown_errors(&self) -> Vec<String> {
        std::mem::take(&mut *self.shutdown_errors.lock())
    }
}

impl std::ops::Deref for ConnectionSlots {
    type Target = Mutex<Vec<ConnectionSlot>>;

    fn deref(&self) -> &Self::Target {
        &self.slots
    }
}

impl Drop for ConnectionSlots {
    fn drop(&mut self) {
        for slot in self.slots.get_mut().iter() {
            if let Some(handle) = slot.handler_task.as_ref() {
                handle.abort();
            }

            if let Some(control) = &slot.socket_control {
                control.deregister();
            }
        }
    }
}

#[derive(Debug)]
struct PoolAdmission {
    generation: u64,
    closed: bool,
}

struct ConnectionSlotBatch<'a> {
    owner: &'a Mutex<Vec<ConnectionSlot>>,
    slots: Vec<ConnectionSlot>,
}

impl<'a> ConnectionSlotBatch<'a> {
    fn take(owner: &'a Mutex<Vec<ConnectionSlot>>) -> Self {
        let slots = std::mem::take(&mut *owner.lock());
        Self { owner, slots }
    }
}

impl Drop for ConnectionSlotBatch<'_> {
    fn drop(&mut self) {
        self.owner.lock().extend(self.slots.drain(..));
    }
}

// Scopes per-slot reconnect cleanup of in-progress bars to the candle topics
// owned by the reconnecting connection, so one slot's reconnect does not
// discard bars still aggregating on other healthy connections.
pub(crate) fn candle_ids_from_topics(topics: &[String]) -> AHashSet<String> {
    let prefix = format!(
        "{}{}",
        DydxWsChannel::Candles.as_ref(),
        DYDX_WS_TOPIC_DELIMITER
    );
    topics
        .iter()
        .filter_map(|topic| topic.strip_prefix(&prefix).map(ToString::to_string))
        .collect()
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;

    #[rstest]
    fn test_candle_ids_from_topics_extracts_only_candle_ids() {
        let topics = vec![
            "v4_candles:BTC-USD/1MIN".to_string(),
            "v4_trades:BTC-USD".to_string(),
            "v4_orderbook:ETH-USD".to_string(),
            "v4_candles:ETH-USD/5MINS".to_string(),
        ];

        let ids = candle_ids_from_topics(&topics);

        assert_eq!(ids.len(), 2);
        assert!(ids.contains("BTC-USD/1MIN"));
        assert!(ids.contains("ETH-USD/5MINS"));
        assert!(!ids.contains("BTC-USD"));
    }

    #[tokio::test]
    async fn test_drop_clone_does_not_stop_connection_pool() {
        let client = DydxWebSocketClient::new_public("wss://test".to_string(), None, None);
        let (cmd_tx, _cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        client.slots.lock().push(ConnectionSlot {
            cmd_tx,
            topics: AHashMap::new(),
            channel_counts: [0; CHANNEL_KIND_COUNT],
            subscriptions_state: SubscriptionState::new(DYDX_WS_TOPIC_DELIMITER),
            handler_task: TaskSlot::from_handle(tokio::spawn(std::future::pending())),
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Active as u8)),
            socket_control: None,
        });
        let clone = client.clone();

        drop(clone);

        let slots = client.slots.lock();
        assert_eq!(slots.len(), 1);
        assert!(
            !slots[0]
                .handler_task
                .as_ref()
                .expect("handler task")
                .is_finished()
        );
    }

    #[tokio::test]
    async fn test_cancelled_disconnect_retains_connection_slot() {
        let mut client = DydxWebSocketClient::new_public("wss://test".to_string(), None, None);
        let (cmd_tx, mut cmd_rx) = tokio::sync::mpsc::unbounded_channel();
        client.slots.lock().push(ConnectionSlot {
            cmd_tx,
            topics: AHashMap::new(),
            channel_counts: [0; CHANNEL_KIND_COUNT],
            subscriptions_state: SubscriptionState::new(DYDX_WS_TOPIC_DELIMITER),
            handler_task: TaskSlot::from_handle(tokio::spawn(std::future::pending())),
            connection_mode: Arc::new(AtomicU8::new(ConnectionMode::Active as u8)),
            socket_control: None,
        });

        {
            let disconnect = client.disconnect();
            tokio::pin!(disconnect);
            tokio::select! {
                result = &mut disconnect => panic!("disconnect completed unexpectedly: {result:?}"),
                command = cmd_rx.recv() => assert!(command.is_some()),
            }
        }

        let slots = client.slots.lock();
        assert_eq!(slots.len(), 1);
        assert!(slots[0].handler_task.is_some());
    }
}
