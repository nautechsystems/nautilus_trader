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

//! Live execution client implementation for the dYdX adapter.
//!
//! This module provides the execution client for submitting orders, cancellations,
//! and managing positions on dYdX v4.
//!
//! # Order Types
//!
//! dYdX supports the following order types:
//!
//! - **Market**: Execute immediately at best available price.
//! - **Limit**: Execute at specified price or better.
//! - **Stop Market**: Triggered when price crosses stop price, then executes as market order.
//! - **Stop Limit**: Triggered when price crosses stop price, then places limit order.
//! - **Take Profit Market**: Close position at profit target, executes as market order.
//! - **Take Profit Limit**: Close position at profit target, places limit order.
//!
//! See <https://docs.dydx.xyz/concepts/trading/orders#types> for details.
//!
//! # Order Lifetimes
//!
//! Orders can be short-term (expire by block height) or long-term/stateful (expire by timestamp).
//! Conditional orders (Stop/TakeProfit) are always stateful.
//!
//! See <https://docs.dydx.xyz/concepts/trading/orders#short-term-vs-long-term> for details.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicU32, AtomicU64, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::{OrderEventAny, OrderModifyRejected, OrderPendingUpdate},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use nautilus_network::retry::RetryConfig;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::DYDX_VENUE, credential::DydxCredential, instrument_cache::InstrumentCache,
        parse::nanos_to_secs_i64,
    },
    config::DydxAdapterConfig,
    execution::{
        broadcaster::TxBroadcaster,
        order_builder::OrderMessageBuilder,
        tx_manager::TransactionManager,
        types::{LimitOrderParams, OrderContext},
        wallet::Wallet,
    },
    grpc::{DydxGrpcClient, SHORT_TERM_ORDER_MAXIMUM_LIFETIME, types::ChainId},
    http::{
        client::DydxHttpClient,
        parse::{parse_http_account_state, parse_position_status_report},
    },
    websocket::{client::DydxWebSocketClient, enums::NautilusWsMessage},
};

pub mod block_time;
pub mod broadcaster;
pub mod order_builder;
pub mod submitter;
pub mod tx_manager;
pub mod types;
pub mod wallet;

use block_time::BlockTimeMonitor;

/// Maximum client order ID value for dYdX (informational - not enforced by adapter).
///
/// dYdX protocol accepts u32 client IDs. The current implementation uses sequential
/// allocation starting from 1, which will wrap at u32::MAX. If dYdX has a stricter
/// limit, this constant should be updated and enforced in `generate_client_order_id_int`.
pub const MAX_CLIENT_ID: u32 = u32::MAX;

/// Live execution client for the dYdX v4 exchange adapter.
///
/// Supports Market, Limit, Stop Market, Stop Limit, Take Profit Market (MarketIfTouched),
/// and Take Profit Limit (LimitIfTouched) orders via gRPC. Trailing stops are NOT supported
/// by the dYdX v4 protocol. dYdX requires u32 client IDs - strings are hashed to fit.
///
/// # Architecture
///
/// The client follows a two-layer execution model:
/// 1. **Synchronous validation** - Immediate checks and event generation.
/// 2. **Async submission** - Non-blocking gRPC calls via `TransactionManager`, `TxBroadcaster`, and `OrderMessageBuilder`.
///
/// This matches the pattern used in OKX and other exchange adapters, ensuring
/// consistent behavior across the Nautilus ecosystem.
#[derive(Debug)]
pub struct DydxExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: DydxAdapterConfig,
    emitter: ExecutionEventEmitter,
    http_client: DydxHttpClient,
    ws_client: DydxWebSocketClient,
    grpc_client: Arc<tokio::sync::RwLock<Option<DydxGrpcClient>>>,
    wallet: Arc<tokio::sync::RwLock<Option<Wallet>>>,
    instrument_cache: Arc<InstrumentCache>,
    /// Block time monitor for tracking rolling average block times and expiration estimation.
    block_time_monitor: Arc<BlockTimeMonitor>,
    oracle_prices: Arc<DashMap<InstrumentId, Decimal>>,
    client_order_id_to_int: DashMap<ClientOrderId, u32>,
    order_contexts: Arc<DashMap<u32, OrderContext>>,
    next_client_order_id: AtomicU32,
    wallet_address: String,
    subaccount_number: u32,
    /// Resolved authenticator IDs for permissioned key trading.
    /// Populated during connect if using an API wallet.
    authenticator_ids: Vec<u64>,
    /// Atomic sequence number for transaction ordering.
    /// Initialized from chain on connect, incremented atomically for each tx.
    /// Value 0 means uninitialized (fetch from chain on first use).
    sequence_number: Arc<AtomicU64>,
    /// Transaction manager for sequence tracking and tx building.
    /// Wrapped in Arc for sharing with async order tasks.
    tx_manager: Option<Arc<TransactionManager>>,
    /// Transaction broadcaster with retry logic.
    /// Wrapped in Arc for sharing with async order tasks.
    broadcaster: Option<Arc<TxBroadcaster>>,
    /// Order message builder for creating dYdX proto messages.
    /// Wrapped in Arc for sharing with async order tasks.
    order_builder: Option<Arc<OrderMessageBuilder>>,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DydxExecutionClient {
    /// Creates a new [`DydxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if credentials are not found or client fails to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: DydxAdapterConfig,
        wallet_address: String,
        subaccount_number: u32,
    ) -> anyhow::Result<Self> {
        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let clock = get_atomic_clock_realtime();
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);

        // Resolve wallet credentials (required for execution client)
        // Priority: 1. config private_key, 2. env DYDX_PRIVATE_KEY
        let wallet = Self::resolve_wallet(&config)?;

        let retry_config = RetryConfig {
            max_retries: config.max_retries,
            initial_delay_ms: config.retry_delay_initial_ms,
            max_delay_ms: config.retry_delay_max_ms,
            ..Default::default()
        };
        let http_client = DydxHttpClient::new(
            Some(config.base_url.clone()),
            Some(config.timeout_secs),
            None, // proxy_url - not in DydxAdapterConfig currently
            config.is_testnet,
            Some(retry_config),
        )?;

        // Share the HTTP client's instrument cache with WebSocket client
        let instrument_cache = http_client.instrument_cache().clone();

        // Use private WebSocket client for authenticated subaccount subscriptions
        let credential = DydxCredential::resolve(
            config.private_key.clone(),
            config.is_testnet,
            config.authenticator_ids.clone(),
        )?
        .ok_or_else(|| anyhow::anyhow!("Credentials required for execution client"))?;

        // Create WS client with shared instrument cache
        let ws_client = DydxWebSocketClient::new_private_with_cache(
            config.ws_url.clone(),
            credential,
            core.account_id,
            instrument_cache.clone(),
            Some(20),
        );

        let grpc_client = Arc::new(tokio::sync::RwLock::new(None));

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_client,
            grpc_client,
            wallet: Arc::new(tokio::sync::RwLock::new(Some(wallet))),
            instrument_cache,
            block_time_monitor: Arc::new(BlockTimeMonitor::new()),
            oracle_prices: Arc::new(DashMap::new()),
            client_order_id_to_int: DashMap::new(),
            order_contexts: Arc::new(DashMap::new()),
            next_client_order_id: AtomicU32::new(1),
            wallet_address,
            subaccount_number,
            authenticator_ids: Vec::new(), // Resolved during connect() if using permissioned keys
            sequence_number: Arc::new(AtomicU64::new(0)), // 0 = uninitialized
            tx_manager: None,
            broadcaster: None,
            order_builder: None,
            started: false,
            connected: false,
            instruments_initialized: false,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Resolves wallet credentials from config or environment.
    ///
    /// Priority: 1. config private_key, 2. env DYDX_PRIVATE_KEY
    fn resolve_wallet(config: &DydxAdapterConfig) -> anyhow::Result<Wallet> {
        let private_key_env = if config.is_testnet {
            "DYDX_TESTNET_PRIVATE_KEY"
        } else {
            "DYDX_PRIVATE_KEY"
        };

        // 1. Try private key from config
        if let Some(ref pk) = config.private_key
            && !pk.trim().is_empty()
        {
            return Wallet::from_private_key(pk);
        }

        // 2. Try private key from env var
        if let Some(pk) = std::env::var(private_key_env)
            .ok()
            .filter(|s| !s.trim().is_empty())
        {
            return Wallet::from_private_key(&pk);
        }

        anyhow::bail!("{private_key_env} not found in config or environment")
    }

    /// Auto-fetches authenticator IDs for permissioned key trading.
    ///
    /// When using an API wallet (signing key different from main account), this method:
    /// 1. Gets the API wallet's public key
    /// 2. Queries the chain for authenticators registered to the main account
    /// 3. Finds authenticators that match the API wallet's public key
    /// 4. Updates `self.authenticator_ids` with the matching IDs
    ///
    /// If authenticator_ids are already configured, this is a no-op.
    async fn resolve_authenticators(
        &mut self,
        grpc_client: &mut DydxGrpcClient,
    ) -> anyhow::Result<()> {
        // Check if we already have authenticator IDs configured
        if !self.authenticator_ids.is_empty() {
            log::debug!(
                "Using pre-configured authenticator IDs: {:?}",
                self.authenticator_ids
            );
            return Ok(());
        }

        // Get the wallet's address (derived from private key)
        let wallet_guard = self.wallet.read().await;
        let wallet = wallet_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;
        let account = wallet.account_offline()?;
        let signing_address = account.address.clone();
        let signing_pubkey = account.public_key();
        drop(wallet_guard);

        // Check if we're using an API wallet (signing address != main account)
        if signing_address == self.wallet_address {
            log::debug!(
                "Signing wallet matches main account {}, no authenticator needed",
                self.wallet_address
            );
            return Ok(());
        }

        log::info!(
            "Detected permissioned key setup: signing with {} for main account {}",
            signing_address,
            self.wallet_address
        );

        // Fetch authenticators for the main account
        let authenticators = grpc_client
            .get_authenticators(&self.wallet_address)
            .await
            .context("Failed to fetch authenticators from chain")?;

        if authenticators.is_empty() {
            anyhow::bail!(
                "No authenticators found for {}. \
                 Please create an API Trading Key in the dYdX UI first.",
                self.wallet_address
            );
        }

        log::debug!(
            "Found {} authenticator(s) for {}",
            authenticators.len(),
            self.wallet_address
        );

        // Find authenticators matching the API wallet's public key
        let signing_pubkey_bytes = signing_pubkey.to_bytes();
        let signing_pubkey_b64 = base64::Engine::encode(
            &base64::engine::general_purpose::STANDARD,
            &signing_pubkey_bytes,
        );

        let mut matching_ids = Vec::new();
        for auth in &authenticators {
            if self.authenticator_matches_pubkey(auth, &signing_pubkey_b64) {
                matching_ids.push(auth.id);
                log::info!("Found matching authenticator: id={}", auth.id);
            }
        }

        if matching_ids.is_empty() {
            anyhow::bail!(
                "No authenticator matches the API wallet's public key. \
                 Ensure the API Trading Key was created for wallet {}. \
                 Available authenticators: {:?}",
                signing_address,
                authenticators.iter().map(|a| a.id).collect::<Vec<_>>()
            );
        }

        // Store the resolved authenticator IDs
        self.authenticator_ids = matching_ids.clone();
        log::info!("Resolved authenticator IDs: {matching_ids:?}");

        Ok(())
    }

    /// Checks if an authenticator contains a SignatureVerification matching the public key.
    fn authenticator_matches_pubkey(
        &self,
        auth: &crate::proto::AccountAuthenticator,
        pubkey_b64: &str,
    ) -> bool {
        // Parse as JSON array of sub-authenticators
        #[derive(serde::Deserialize)]
        struct SubAuth {
            #[serde(rename = "type")]
            auth_type: String,
            config: String,
        }

        // auth.config is raw bytes (Vec<u8>) containing JSON, not base64-encoded
        let config_str = match String::from_utf8(auth.config.clone()) {
            Ok(s) => s,
            Err(_) => return false,
        };

        log::debug!(
            "Checking authenticator id={}, type={}, config={}",
            auth.id,
            auth.r#type,
            config_str
        );

        if let Ok(sub_auths) = serde_json::from_str::<Vec<SubAuth>>(&config_str) {
            for sub in sub_auths {
                log::debug!(
                    "  Sub-authenticator: type={}, config={}",
                    sub.auth_type,
                    sub.config
                );
                if sub.auth_type == "SignatureVerification" && sub.config == pubkey_b64 {
                    log::debug!("  -> MATCH! pubkey_b64={pubkey_b64}");
                    return true;
                }
            }
        }

        false
    }

    /// Generate a unique client order ID integer and store the mapping.
    ///
    /// # Invariants
    ///
    /// - Same `client_order_id` string → same `u32` for the lifetime of this process.
    /// - Different `client_order_id` strings → different `u32` values (except on u32 wrap).
    /// - Thread-safe for concurrent calls.
    ///
    /// # Behavior
    ///
    /// - Parses numeric `client_order_id` directly to `u32` for stability across restarts.
    /// - For non-numeric IDs, allocates a new sequential value from an atomic counter.
    /// - Mapping is kept in-memory only; non-numeric IDs will not be recoverable after restart.
    /// - Counter starts at 1 and increments without bound checking (will wrap at u32::MAX).
    ///
    /// # Notes
    ///
    /// - Atomic counter uses `Relaxed` ordering — uniqueness is required, not cross-thread sequencing.
    /// - If dYdX enforces a maximum client ID below u32::MAX, additional range validation is needed.
    fn generate_client_order_id_int(&self, client_order_id: ClientOrderId) -> u32 {
        use std::hash::{Hash, Hasher};

        use dashmap::mapref::entry::Entry;

        // Fast path: already mapped
        if let Some(existing) = self.client_order_id_to_int.get(&client_order_id) {
            return *existing.value();
        }

        // Try parsing as direct integer
        if let Ok(id) = client_order_id.as_str().parse::<u32>() {
            self.client_order_id_to_int.insert(client_order_id, id);
            return id;
        }

        // Use deterministic hash of the ClientOrderId string.
        match self.client_order_id_to_int.entry(client_order_id) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(vacant) => {
                let mut hasher = ahash::AHasher::default();
                client_order_id.as_str().hash(&mut hasher);
                let id = hasher.finish() as u32;
                vacant.insert(id);
                id
            }
        }
    }

    /// Registers a full order context for WebSocket correlation and cancellation.
    fn register_order_context(&self, client_id_u32: u32, context: OrderContext) {
        self.order_contexts.insert(client_id_u32, context);
    }

    /// Gets the order context for a given dYdX client ID.
    ///
    /// Returns `None` if no context has been registered for this ID.
    fn get_order_context(&self, client_id_u32: u32) -> Option<OrderContext> {
        self.order_contexts
            .get(&client_id_u32)
            .map(|r| r.value().clone())
    }

    /// Retrieve the client order ID integer from the cache.
    ///
    /// Returns `None` if the mapping doesn't exist.
    fn get_client_order_id_int(&self, client_order_id: ClientOrderId) -> Option<u32> {
        // Try parsing first
        if let Ok(id) = client_order_id.as_str().parse::<u32>() {
            return Some(id);
        }

        // Look up in cache
        self.client_order_id_to_int
            .get(&client_order_id)
            .map(|entry| *entry.value())
    }

    /// Get chain ID from config network field.
    ///
    /// This is the recommended way to get chain_id for all transaction submissions.
    fn get_chain_id(&self) -> ChainId {
        self.config.get_chain_id()
    }

    /// Marks instruments as initialized after HTTP client has fetched them.
    ///
    /// The instruments are stored in the shared `InstrumentCache` which is automatically
    /// populated by the HTTP client during `fetch_and_cache_instruments()`.
    fn mark_instruments_initialized(&mut self) {
        let count = self.instrument_cache.len();
        self.instruments_initialized = true;
        log::debug!("Instruments initialized: {count} instruments in shared cache");
    }

    /// Get an instrument by market ticker (e.g., "BTC-USD").
    fn get_instrument_by_market(&self, market: &str) -> Option<InstrumentAny> {
        self.instrument_cache.get_by_market(market)
    }

    /// Get an instrument by clob_pair_id.
    fn get_instrument_by_clob_pair_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        let instrument = self.instrument_cache.get_by_clob_id(clob_pair_id);

        if instrument.is_none() {
            self.instrument_cache.log_missing_clob_pair_id(clob_pair_id);
        }

        instrument
    }

    /// Gets the execution components, returning an error if not initialized.
    ///
    /// This should only be called after `connect()` has completed.
    fn get_execution_components(
        &self,
    ) -> anyhow::Result<(
        Arc<TransactionManager>,
        Arc<TxBroadcaster>,
        Arc<OrderMessageBuilder>,
    )> {
        let tx_manager = self
            .tx_manager
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("TransactionManager not initialized - call connect() first")
            })?
            .clone();
        let broadcaster = self
            .broadcaster
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("TxBroadcaster not initialized - call connect() first"))?
            .clone();
        let order_builder = self
            .order_builder
            .as_ref()
            .ok_or_else(|| {
                anyhow::anyhow!("OrderMessageBuilder not initialized - call connect() first")
            })?
            .clone();
        Ok((tx_manager, broadcaster, order_builder))
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{label}: {e:?}");
            }
        });

        self.pending_tasks
            .lock()
            .expect(MUTEX_POISONED)
            .push(handle);
    }

    /// Spawns an order submission task with error handling and rejection generation.
    ///
    /// If the submission fails, generates an `OrderRejected` event with the error details.
    fn spawn_order_task<F>(
        &self,
        label: &'static str,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        fut: F,
    ) where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                let error_msg = format!("{label} failed: {e:?}");
                log::error!("{error_msg}");

                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &error_msg,
                    ts_event,
                    false,
                );
            }
        });

        self.pending_tasks
            .lock()
            .expect(MUTEX_POISONED)
            .push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut guard = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in guard.drain(..) {
            handle.abort();
        }
    }

    /// Sends an OrderModifyRejected event.
    fn send_modify_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: &str,
    ) {
        let ts_event = self.clock.get_time_ns();
        self.emitter.emit_order_modify_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            reason,
            ts_event,
        );
    }

    /// Waits for the account to be registered in the cache.
    ///
    /// This method polls the cache until the account is registered, ensuring that
    /// execution state reconciliation can process fills correctly (fills require
    /// the account to be registered for portfolio updates).
    ///
    /// # Errors
    ///
    /// Returns an error if the account is not registered within the timeout period.
    async fn await_account_registered(&self, timeout_secs: f64) -> anyhow::Result<()> {
        let account_id = self.core.account_id;

        if self.core.cache().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        let start = Instant::now();
        let timeout = Duration::from_secs_f64(timeout_secs);
        let interval = Duration::from_millis(10);

        loop {
            tokio::time::sleep(interval).await;

            if self.core.cache().account(&account_id).is_some() {
                log::info!("Account {account_id} registered");
                return Ok(());
            }

            if start.elapsed() >= timeout {
                anyhow::bail!(
                    "Timeout waiting for account {account_id} to be registered after {timeout_secs}s"
                );
            }
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for DydxExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *DYDX_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            log::warn!("dYdX execution client already started");
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        log::info!("Starting dYdX execution client");
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            log::warn!("dYdX execution client not started");
            return Ok(());
        }

        log::info!("Stopping dYdX execution client");
        self.abort_pending_tasks();
        self.started = false;
        self.connected = false;
        Ok(())
    }

    /// Submits an order to dYdX via gRPC.
    ///
    /// dYdX requires u32 client IDs - Nautilus ClientOrderId strings are hashed to fit.
    ///
    /// Supported order types:
    /// - Market orders (short-term, IOC).
    /// - Limit orders (short-term or long-term based on TIF).
    /// - Stop Market orders (conditional, triggered at stop price).
    /// - Stop Limit orders (conditional, triggered at stop price, executed at limit).
    /// - Take Profit Market (MarketIfTouched - triggered at take profit price).
    /// - Take Profit Limit (LimitIfTouched - triggered at take profit price, executed at limit).
    ///
    /// Trailing stop orders are NOT supported by dYdX v4 protocol.
    ///
    /// Validates synchronously, generates OrderSubmitted event, then spawns async task for
    /// gRPC submission to avoid blocking. Unsupported order types generate OrderRejected.
    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        // Check connection status first (doesn't need order)
        if !self.is_connected() {
            let reason = "Cannot submit order: execution client not connected";
            log::error!("{reason}");
            anyhow::bail!(reason);
        }

        // Check block height is available for short-term orders
        let current_block = self.block_time_monitor.current_block_height();

        // Hold cache borrow for all order access, clone only when needed for async
        let cache = self.core.cache();
        let order = cache.order(&cmd.client_order_id).ok_or_else(|| {
            anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
        })?;

        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!(
                "Cannot submit order {}: {}",
                order.client_order_id(),
                reason
            );
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_rejected_event(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                reason,
                ts_event,
                false,
            );
            return Ok(());
        }

        // Check if order is already closed
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Reject unsupported order types
        match order.order_type() {
            OrderType::Market
            | OrderType::Limit
            | OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => {}
            // Trailing stops not supported by dYdX v4 protocol
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit => {
                let reason = "Trailing stop orders not supported by dYdX v4 protocol";
                log::error!("{reason}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason,
                    ts_event,
                    false,
                );
                return Ok(());
            }
            order_type => {
                let reason = format!("Order type {order_type:?} not supported by dYdX");
                log::error!("{reason}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    &reason,
                    ts_event,
                    false,
                );
                return Ok(());
            }
        }

        self.emitter.emit_order_submitted(order);

        // Get execution components (must be initialized after connect())
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components: {e}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    &e.to_string(),
                    ts_event,
                    false,
                );
                return Ok(());
            }
        };

        let client_order_id = order.client_order_id();
        let instrument_id = order.instrument_id();
        let block_height = self.block_time_monitor.current_block_height() as u32;
        #[allow(clippy::redundant_clone)]
        let order_clone = order.clone();

        // Generate client_order_id as u32 before async block (dYdX requires u32 client IDs)
        let client_id_u32 = self.generate_client_order_id_int(client_order_id);

        // Convert expire_time from nanoseconds to seconds if present
        let expire_time = order.expire_time().map(nanos_to_secs_i64);

        // Determine order_flags based on order type for later cancellation
        let order_flags = match order.order_type() {
            // Conditional orders always use ORDER_FLAG_CONDITIONAL
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => types::ORDER_FLAG_CONDITIONAL,
            // Market orders are always short-term
            OrderType::Market => types::ORDER_FLAG_SHORT_TERM,
            // Limit orders depend on time_in_force and expire_time
            OrderType::Limit => {
                let lifetime = types::OrderLifetime::from_time_in_force(
                    order.time_in_force(),
                    expire_time,
                    false,
                    order_builder.max_short_term_secs(),
                );
                lifetime.order_flags()
            }
            // Default to long-term for unknown types
            _ => types::ORDER_FLAG_LONG_TERM,
        };

        // Register order context for WebSocket correlation and cancellation
        let ts_submitted = self.clock.get_time_ns();
        self.register_order_context(
            client_id_u32,
            OrderContext {
                client_order_id,
                trader_id: order.trader_id(),
                strategy_id: order.strategy_id(),
                instrument_id,
                submitted_at: ts_submitted,
                order_flags,
            },
        );

        self.spawn_order_task(
            "submit_order",
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            async move {
                // Build the order message based on order type
                let (msg, order_type_str) = match order_clone.order_type() {
                    OrderType::Market => {
                        let msg = order_builder.build_market_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            order_clone.quantity(),
                            block_height,
                        )?;
                        (msg, "market")
                    }
                    OrderType::Limit => {
                        // Use pre-computed expire_time (with default_short_term_expiry applied)
                        let msg = order_builder.build_limit_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            order_clone
                                .price()
                                .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?,
                            order_clone.quantity(),
                            order_clone.time_in_force(),
                            order_clone.is_post_only(),
                            order_clone.is_reduce_only(),
                            block_height,
                            expire_time, // Uses default_short_term_expiry if configured
                        )?;
                        (msg, "limit")
                    }
                    // Conditional orders use their own expiration logic (not affected by default_short_term_expiry)
                    // They are always stored on-chain with long-term semantics
                    OrderType::StopMarket => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop market order missing trigger_price")
                        })?;
                        let cond_expire = order_clone.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_stop_market_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            trigger_price,
                            order_clone.quantity(),
                            order_clone.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "stop_market")
                    }
                    OrderType::StopLimit => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing trigger_price")
                        })?;
                        let limit_price = order_clone.price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing limit price")
                        })?;
                        let cond_expire = order_clone.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_stop_limit_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            trigger_price,
                            limit_price,
                            order_clone.quantity(),
                            order_clone.time_in_force(),
                            order_clone.is_post_only(),
                            order_clone.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "stop_limit")
                    }
                    // dYdX TakeProfitMarket maps to Nautilus MarketIfTouched
                    OrderType::MarketIfTouched => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit market order missing trigger_price")
                        })?;
                        let cond_expire = order_clone.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_take_profit_market_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            trigger_price,
                            order_clone.quantity(),
                            order_clone.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "take_profit_market")
                    }
                    // dYdX TakeProfitLimit maps to Nautilus LimitIfTouched
                    OrderType::LimitIfTouched => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing trigger_price")
                        })?;
                        let limit_price = order_clone.price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing limit price")
                        })?;
                        let cond_expire = order_clone.expire_time().map(nanos_to_secs_i64);
                        let msg = order_builder.build_take_profit_limit_order(
                            instrument_id,
                            client_id_u32,
                            order_clone.order_side(),
                            trigger_price,
                            limit_price,
                            order_clone.quantity(),
                            order_clone.time_in_force(),
                            order_clone.is_post_only(),
                            order_clone.is_reduce_only(),
                            cond_expire,
                        )?;
                        (msg, "take_profit_limit")
                    }
                    _ => unreachable!("Order type already validated"),
                };

                // Broadcast with retry
                let operation = format!("Submit {order_type_str} order {client_order_id}");
                broadcaster
                    .broadcast_with_retry(&tx_manager, vec![msg], &operation)
                    .await?;
                log::debug!("Successfully submitted {order_type_str} order: {client_order_id}");

                Ok(())
            },
        );

        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        let order_count = cmd.order_list.orders.len();

        // Check connection status
        if !self.is_connected() {
            let reason = "Cannot submit order list: execution client not connected";
            log::error!("{reason}");
            anyhow::bail!(reason);
        }

        // Check block height is available
        let current_block = self.block_time_monitor.current_block_height();
        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!("Cannot submit order list: {reason}");
            // Reject all orders in the list
            let ts_event = self.clock.get_time_ns();
            for order in &cmd.order_list.orders {
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason,
                    ts_event,
                    false,
                );
            }
            return Ok(());
        }

        // Get execution components early so we can register order contexts
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for batch: {e}");
                // Reject all orders in the list
                let ts_event = self.clock.get_time_ns();
                for order in &cmd.order_list.orders {
                    self.emitter.emit_order_rejected_event(
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        &e.to_string(),
                        ts_event,
                        false,
                    );
                }
                return Ok(());
            }
        };

        // Collect limit order parameters for batch submission
        let mut order_params: Vec<LimitOrderParams> = Vec::with_capacity(order_count);
        let mut order_info: Vec<(ClientOrderId, InstrumentId, StrategyId)> =
            Vec::with_capacity(order_count);

        for order in &cmd.order_list.orders {
            // Only limit orders can be batched
            if order.order_type() != OrderType::Limit {
                log::warn!(
                    "Order {} has type {:?}, falling back to individual submission",
                    order.client_order_id(),
                    order.order_type()
                );
                // Fall back to individual submission for non-limit orders
                let submit_cmd = SubmitOrder::new(
                    cmd.trader_id,
                    cmd.client_id,
                    cmd.strategy_id,
                    order.instrument_id(),
                    order.client_order_id(),
                    order.init_event().clone(),
                    cmd.exec_algorithm_id,
                    cmd.position_id,
                    cmd.params.clone(),
                    UUID4::new(),
                    cmd.ts_init,
                );
                if let Err(e) = self.submit_order(&submit_cmd) {
                    log::error!(
                        "Failed to submit order {} from order list: {e}",
                        order.client_order_id()
                    );
                }
                continue;
            }

            // Get price (required for limit orders)
            let Some(price) = order.price() else {
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_rejected_event(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    "Limit order missing price",
                    ts_event,
                    false,
                );
                continue;
            };

            // Generate client order ID as u32
            let client_id_u32 = self.generate_client_order_id_int(order.client_order_id());

            // Send OrderSubmitted event
            self.emitter.emit_order_submitted(order);

            // Determine order_flags for limit orders
            let expire_time_secs = order.expire_time().map(nanos_to_secs_i64);
            let lifetime = types::OrderLifetime::from_time_in_force(
                order.time_in_force(),
                expire_time_secs,
                false,
                order_builder.max_short_term_secs(),
            );

            // Register order context for WebSocket correlation and cancellation
            let ts_submitted = self.clock.get_time_ns();
            self.register_order_context(
                client_id_u32,
                OrderContext {
                    client_order_id: order.client_order_id(),
                    trader_id: order.trader_id(),
                    strategy_id: order.strategy_id(),
                    instrument_id: order.instrument_id(),
                    submitted_at: ts_submitted,
                    order_flags: lifetime.order_flags(),
                },
            );

            // Collect order parameters (builder will apply default_short_term_expiry if needed)
            order_params.push(LimitOrderParams {
                instrument_id: order.instrument_id(),
                client_order_id: client_id_u32,
                side: order.order_side(),
                price,
                quantity: order.quantity(),
                time_in_force: order.time_in_force(),
                post_only: order.is_post_only(),
                reduce_only: order.is_reduce_only(),
                expire_time_ns: order.expire_time(),
            });
            order_info.push((
                order.client_order_id(),
                order.instrument_id(),
                order.strategy_id(),
            ));
        }

        // If no limit orders to batch, we're done
        if order_params.is_empty() {
            return Ok(());
        }

        // Check if any orders are short-term
        // dYdX protocol restriction: short-term orders CANNOT be batched
        // Each short-term order must be in its own transaction
        let has_short_term = order_params
            .iter()
            .any(|params| order_builder.is_short_term_order(params));

        let block_height = current_block as u32;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if has_short_term {
            // Submit each order individually (short-term orders cannot be batched)
            log::info!(
                "Submitting {} limit orders individually (short-term orders cannot be batched)",
                order_params.len()
            );

            // Submit each order in parallel using separate transactions
            let handle = get_runtime().spawn(async move {
                let mut handles = Vec::with_capacity(order_params.len());

                for (params, (client_order_id, instrument_id, strategy_id)) in
                    order_params.into_iter().zip(order_info.into_iter())
                {
                    let tx_manager = tx_manager.clone();
                    let broadcaster = broadcaster.clone();
                    let order_builder = order_builder.clone();
                    let emitter = emitter.clone();

                    let handle = get_runtime().spawn(async move {
                        // Build order message
                        let msg = match order_builder
                            .build_limit_order_from_params(&params, block_height)
                        {
                            Ok(m) => m,
                            Err(e) => {
                                let error_msg = format!("Failed to build order message: {e:?}");
                                log::error!("{error_msg}");
                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    &error_msg,
                                    ts_event,
                                    false,
                                );
                                return;
                            }
                        };

                        // Broadcast with retry (single message per transaction)
                        let operation = format!("Submit order {client_order_id}");
                        if let Err(e) = broadcaster
                            .broadcast_with_retry(&tx_manager, vec![msg], &operation)
                            .await
                        {
                            let error_msg = format!("Order submission failed: {e:?}");
                            log::error!("{error_msg}");
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                &error_msg,
                                ts_event,
                                false,
                            );
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all orders to be submitted
                for handle in handles {
                    let _ = handle.await;
                }
            });

            // Track the task
            self.pending_tasks
                .lock()
                .expect(MUTEX_POISONED)
                .push(handle);
        } else {
            // All orders are long-term - can batch in single transaction
            log::info!(
                "Batch submitting {} long-term limit orders in single transaction",
                order_params.len()
            );

            let handle = get_runtime().spawn(async move {
                // Build all order messages
                let msgs: Result<Vec<_>, _> = order_params
                    .iter()
                    .map(|params| order_builder.build_limit_order_from_params(params, block_height))
                    .collect();

                let msgs = match msgs {
                    Ok(m) => m,
                    Err(e) => {
                        let error_msg = format!("Failed to build batch order messages: {e:?}");
                        log::error!("{error_msg}");
                        // Send OrderRejected for all orders
                        let ts_event = clock.get_time_ns();
                        for (client_order_id, instrument_id, strategy_id) in order_info {
                            emitter.emit_order_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                &error_msg,
                                ts_event,
                                false,
                            );
                        }
                        return;
                    }
                };

                // Broadcast batch with retry
                let operation = format!("Submit batch of {} limit orders", msgs.len());
                if let Err(e) = broadcaster
                    .broadcast_with_retry(&tx_manager, msgs, &operation)
                    .await
                {
                    let error_msg = format!("Batch order submission failed: {e:?}");
                    log::error!("{error_msg}");

                    // Send OrderRejected for all orders in the batch
                    let ts_event = clock.get_time_ns();
                    for (client_order_id, instrument_id, strategy_id) in order_info {
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &error_msg,
                            ts_event,
                            false,
                        );
                    }
                }
            });

            // Track the task
            self.pending_tasks
                .lock()
                .expect(MUTEX_POISONED)
                .push(handle);
        }

        Ok(())
    }

    /// Modifies an order on dYdX by canceling and replacing.
    ///
    /// dYdX doesn't support native order modification, so this implements
    /// cancel-and-replace: the existing order is canceled and a new order
    /// is submitted with the modified parameters.
    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot modify order: not connected");
        }

        let client_order_id = cmd.client_order_id;
        let instrument_id = cmd.instrument_id;

        // Validate order exists in cache and is open
        let cache = self.core.cache();

        let order = match cache.order(&client_order_id).cloned() {
            Some(order) => order,
            None => {
                log::error!("Cannot modify order {client_order_id}: not found in cache");
                return Ok(());
            }
        };

        if order.is_closed() {
            log::warn!(
                "ModifyOrder command for {} when order already {} (will not send to exchange)",
                client_order_id,
                order.status()
            );
            return Ok(());
        }

        // Only support limit order modification for now
        if order.order_type() != OrderType::Limit {
            let reason = format!(
                "Order modification only supported for Limit orders, was {:?}",
                order.order_type()
            );
            log::error!("{reason}");
            self.send_modify_rejected(
                cmd.strategy_id,
                instrument_id,
                client_order_id,
                cmd.venue_order_id,
                &reason,
            );
            return Ok(());
        }

        // Get the modified values (use existing if not specified)
        let new_quantity = cmd.quantity.unwrap_or_else(|| order.quantity());
        let new_price = match cmd.price.or_else(|| order.price()) {
            Some(p) => p,
            None => {
                let reason = "Cannot modify order: no price specified and order has no price";
                log::error!("{reason}");
                self.send_modify_rejected(
                    cmd.strategy_id,
                    instrument_id,
                    client_order_id,
                    cmd.venue_order_id,
                    reason,
                );
                return Ok(());
            }
        };

        // Check block height is available
        let current_block = self.block_time_monitor.current_block_height();
        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!("Cannot modify order {client_order_id}: {reason}");
            self.send_modify_rejected(
                cmd.strategy_id,
                instrument_id,
                client_order_id,
                cmd.venue_order_id,
                reason,
            );
            return Ok(());
        }

        log::info!(
            "Modifying order {} via cancel-and-replace: qty={} -> {}, price={:?} -> {}",
            client_order_id,
            order.quantity(),
            new_quantity,
            order.price(),
            new_price
        );

        // Send OrderPendingUpdate event
        let ts_now = self.clock.get_time_ns();
        let pending_event = OrderPendingUpdate::new(
            cmd.trader_id,
            cmd.strategy_id,
            instrument_id,
            client_order_id,
            self.core.account_id,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            cmd.venue_order_id,
        );
        let sender = get_exec_event_sender();
        if let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::PendingUpdate(
            pending_event,
        ))) {
            log::warn!("Failed to send OrderPendingUpdate event: {e}");
        }

        // Get the OLD client_id for cancellation
        let old_client_id_u32 = match self.get_client_order_id_int(client_order_id) {
            Some(id) => id,
            None => {
                log::error!("Client order ID {client_order_id} not found in cache");
                self.send_modify_rejected(
                    cmd.strategy_id,
                    instrument_id,
                    client_order_id,
                    cmd.venue_order_id,
                    "Client order ID not found in mapping",
                );
                return Ok(());
            }
        };

        // Get old order context (needed for order_flags during cancellation)
        let old_order_context = self.get_order_context(old_client_id_u32);
        let old_order_flags = old_order_context.as_ref().map_or(0, |ctx| ctx.order_flags); // Default to short-term if not found

        // Generate NEW client_id for replacement order
        // dYdX doesn't allow reusing client_id even after cancellation
        let new_client_id_u32 = self.next_client_order_id.fetch_add(1, Ordering::Relaxed);

        // Update mappings: ClientOrderId now points to new client_id
        self.client_order_id_to_int
            .insert(client_order_id, new_client_id_u32);
        // Remove old context, new context will be registered after determining new order_flags
        self.order_contexts.remove(&old_client_id_u32);

        log::debug!(
            "Modify order {client_order_id}: old_client_id={old_client_id_u32}, new_client_id={new_client_id_u32}"
        );

        // Get execution components
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for modify: {e}");
                self.send_modify_rejected(
                    cmd.strategy_id,
                    instrument_id,
                    client_order_id,
                    cmd.venue_order_id,
                    &e.to_string(),
                );
                return Ok(());
            }
        };

        let block_height = current_block as u32;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;
        let order_side = order.order_side();
        let time_in_force = order.time_in_force();
        let post_only = order.is_post_only();
        let reduce_only = order.is_reduce_only();
        // Capture raw expire_time (builder will apply default_short_term_expiry)
        let expire_time_ns = order.expire_time();
        let account_id = self.core.account_id;

        // Create params for the replacement order (needed for short-term check)
        let new_params = LimitOrderParams {
            instrument_id,
            client_order_id: new_client_id_u32,
            side: order_side,
            price: new_price,
            quantity: new_quantity,
            time_in_force,
            post_only,
            reduce_only,
            expire_time_ns,
        };

        // Check if either cancel or place is short-term
        // dYdX protocol restriction: short-term orders CANNOT be batched
        let cancel_is_short_term =
            order_builder.is_short_term_cancel(time_in_force, expire_time_ns);
        let place_is_short_term = order_builder.is_short_term_order(&new_params);
        let requires_sequential = cancel_is_short_term || place_is_short_term;

        if requires_sequential {
            log::info!(
                "Modifying order {client_order_id} via sequential cancel+place (short-term: cancel={cancel_is_short_term}, place={place_is_short_term})"
            );
        }

        self.spawn_task("modify_order", async move {
            log::debug!(
                "Modify order {client_order_id}: old_id={old_client_id_u32}, new_id={new_client_id_u32}, qty={new_quantity}, price={new_price}"
            );

            if requires_sequential {
                // Short-term orders cannot be batched - execute cancel then place sequentially
                // Step 1: Build and send cancel using stored order_flags
                let cancel_msg = match order_builder.build_cancel_order_with_flags(
                    instrument_id,
                    old_client_id_u32,
                    old_order_flags,
                    block_height,
                ) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::error!("Modify failed - cancel build failed for {client_order_id}: {e:?}");
                        let ts_now = UnixNanos::default();
                        let event = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("Cancel build failed: {e:?}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        let sender = get_exec_event_sender();
                        let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                        return Ok(());
                    }
                };

                // Broadcast cancel first
                if let Err(e) = broadcaster
                    .broadcast_with_retry(
                        &tx_manager,
                        vec![cancel_msg],
                        &format!("Modify cancel {client_order_id}"),
                    )
                    .await
                {
                    log::error!("Modify failed - cancel broadcast failed for {client_order_id}: {e:?}");
                    let ts_now = UnixNanos::default();
                    let event = OrderModifyRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        format!("Cancel failed: {e:?}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        venue_order_id,
                        Some(account_id),
                    );
                    let sender = get_exec_event_sender();
                    let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                    return Ok(());
                }

                log::debug!("Modify cancel succeeded for {client_order_id}, now placing replacement");

                // Step 2: Build and send place
                let place_msg = match order_builder.build_limit_order_from_params(&new_params, block_height) {
                    Ok(msg) => msg,
                    Err(e) => {
                        log::error!("Modify failed - place build failed for {client_order_id}: {e:?}");
                        let ts_now = UnixNanos::default();
                        let event = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("Place build failed: {e:?}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        let sender = get_exec_event_sender();
                        let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                        return Ok(());
                    }
                };

                // Broadcast place
                match broadcaster
                    .broadcast_with_retry(
                        &tx_manager,
                        vec![place_msg],
                        &format!("Modify place {client_order_id}"),
                    )
                    .await
                {
                    Ok(_) => {
                        log::info!(
                            "Modify complete: Order {client_order_id} sequentially modified (qty={new_quantity}, price={new_price})"
                        );
                    }
                    Err(e) => {
                        log::error!("Modify failed - place broadcast failed for {client_order_id}: {e:?}");
                        let ts_now = UnixNanos::default();
                        let event = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("Place failed after cancel: {e:?}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        let sender = get_exec_event_sender();
                        let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                    }
                }
            } else {
                // Both long-term - can batch cancel+place in single transaction
                log::debug!(
                    "Modify order {client_order_id}: building atomic cancel+replace batch"
                );

                let batch_msgs = match order_builder.build_cancel_and_replace_with_flags(
                    instrument_id,
                    old_client_id_u32,
                    old_order_flags,
                    &new_params,
                    block_height,
                ) {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        log::error!("Modify failed - batch build failed for {client_order_id}: {e:?}");
                        let ts_now = UnixNanos::default();
                        let event = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("Batch build failed: {e:?}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        let sender = get_exec_event_sender();
                        let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                        return Ok(());
                    }
                };

                // Broadcast atomic cancel+replace as single transaction
                match broadcaster
                    .broadcast_with_retry(&tx_manager, batch_msgs, &format!("Modify order {client_order_id}"))
                    .await
                {
                    Ok(_) => {
                        log::info!(
                            "Modify complete: Order {client_order_id} atomically modified (qty={new_quantity}, price={new_price})"
                        );
                    }
                    Err(e) => {
                        log::error!("Modify failed for {client_order_id}: {e:?}");
                        let ts_now = UnixNanos::default();
                        let event = OrderModifyRejected::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            format!("Modify failed: {e:?}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            venue_order_id,
                            Some(account_id),
                        );
                        let sender = get_exec_event_sender();
                        let _ = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)));
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    /// Cancels an order on dYdX exchange.
    ///
    /// Validates the order state and retrieves instrument details before
    /// spawning an async task to cancel via gRPC.
    ///
    /// # Validation
    ///
    /// - Checks order exists in cache.
    /// - Validates order is not already closed.
    /// - Retrieves instrument from cache for order builder.
    ///
    /// The `cmd` contains client/venue order IDs. Returns `Ok(())` if cancel request is
    /// spawned successfully or validation fails gracefully. Returns `Err` if not connected.
    ///
    /// # Events
    ///
    /// - `OrderCanceled` - Generated when WebSocket confirms cancellation.
    /// - `OrderCancelRejected` - Generated if exchange rejects cancellation.
    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel order: not connected");
        }

        let client_order_id = cmd.client_order_id;

        // Validate order exists in cache and is not closed
        let cache = self.core.cache();

        let order = match cache.order(&client_order_id) {
            Some(order) => order,
            None => {
                log::error!("Cannot cancel order {client_order_id}: not found in cache");
                return Ok(()); // Not an error - order may have been filled/canceled already
            }
        };

        // Validate order is not already closed
        if order.is_closed() {
            log::warn!(
                "CancelOrder command for {} when order already {} (will not send to exchange)",
                client_order_id,
                order.status()
            );
            return Ok(());
        }

        // Retrieve instrument from cache
        let instrument_id = cmd.instrument_id;
        let instrument = match cache.instrument(&instrument_id) {
            Some(instrument) => instrument,
            None => {
                log::error!(
                    "Cannot cancel order {client_order_id}: instrument {instrument_id} not found in cache"
                );
                return Ok(()); // Not an error - missing instrument is a cache issue
            }
        };

        log::debug!(
            "Cancelling order {} for instrument {}",
            client_order_id,
            instrument.id()
        );

        // Get execution components
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for cancel: {e}");
                return Ok(());
            }
        };

        let block_height = self.block_time_monitor.current_block_height() as u32;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;

        // Convert client_order_id to u32 before async block
        let client_id_u32 = match self.get_client_order_id_int(client_order_id) {
            Some(id) => id,
            None => {
                log::error!("Client order ID {client_order_id} not found in cache");
                anyhow::bail!("Client order ID not found in cache")
            }
        };

        // Get stored order_flags from order context (set at submission time)
        // This ensures we use the correct flags even if the order has expired
        let order_flags = self.get_order_context(client_id_u32).map_or_else(
            || {
                // Fallback: derive from order parameters if context not found
                log::warn!(
                    "Order context not found for {client_order_id}, deriving flags from order"
                );
                let expire_time = order.expire_time().map(nanos_to_secs_i64);
                types::OrderLifetime::from_time_in_force(
                    order.time_in_force(),
                    expire_time,
                    false,
                    order_builder.max_short_term_secs(),
                )
                .order_flags()
            },
            |ctx| ctx.order_flags,
        );

        let clock = self.clock;
        let emitter = self.emitter.clone();

        self.spawn_task("cancel_order", async move {
            // Build cancel message using stored order_flags
            let cancel_msg = match order_builder.build_cancel_order_with_flags(
                instrument_id,
                client_id_u32,
                order_flags,
                block_height,
            ) {
                Ok(msg) => msg,
                Err(e) => {
                    log::error!("Failed to build cancel message for {client_order_id}: {e:?}");
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Cancel build failed: {e:?}"),
                        ts_event,
                    );
                    return Ok(());
                }
            };

            // Broadcast cancel with retry
            match broadcaster
                .broadcast_with_retry(
                    &tx_manager,
                    vec![cancel_msg],
                    &format!("Cancel order {client_order_id}"),
                )
                .await
            {
                Ok(_) => {
                    log::debug!("Successfully cancelled order: {client_order_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel order {client_order_id}: {e:?}");

                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Cancel order failed: {e:?}"),
                        ts_event,
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        // Query all open orders from cache
        let cache = self.core.cache();
        let mut open_orders: Vec<_> = cache
            .orders_open(None, None, None, None, None)
            .into_iter()
            .collect();

        let instrument_id = cmd.instrument_id;
        open_orders.retain(|order| order.instrument_id() == instrument_id);

        // Filter by order_side if specified (NoOrderSide means all sides)
        if cmd.order_side != OrderSide::NoOrderSide {
            let order_side = cmd.order_side;
            open_orders.retain(|order| order.order_side() == order_side);
        }

        // Split orders into short-term and long-term based on TimeInForce
        // Short-term: IOC, FOK (expire by block height)
        // Long-term: GTC, GTD, DAY, POST_ONLY (expire by timestamp)
        let mut short_term_orders = Vec::new();
        let mut long_term_orders = Vec::new();

        for order in &open_orders {
            match order.time_in_force() {
                TimeInForce::Ioc | TimeInForce::Fok => short_term_orders.push(order),
                TimeInForce::Gtc
                | TimeInForce::Gtd
                | TimeInForce::Day
                | TimeInForce::AtTheOpen
                | TimeInForce::AtTheClose => long_term_orders.push(order),
            }
        }

        log::debug!(
            "Cancel all orders: total={}, short_term={}, long_term={}, instrument_id={}, order_side={:?}",
            open_orders.len(),
            short_term_orders.len(),
            long_term_orders.len(),
            instrument_id,
            cmd.order_side
        );

        // Get execution components
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for cancel_all: {e}");
                return Ok(());
            }
        };

        let block_height = self.block_time_monitor.current_block_height() as u32;

        // Collect (instrument_id, client_id, order_flags) tuples for cancel
        // Use stored order_flags from order context to ensure correct cancellation
        let mut orders_to_cancel = Vec::new();
        for order in &open_orders {
            let client_order_id = order.client_order_id();
            if let Some(client_id_u32) = self.get_client_order_id_int(client_order_id) {
                // Get stored order_flags from order context
                let order_flags = self.get_order_context(client_id_u32).map_or_else(
                    || {
                        // Fallback: derive from order parameters if context not found
                        log::warn!(
                            "Order context not found for {client_order_id}, deriving flags from order"
                        );
                        let expire_time = order.expire_time().map(nanos_to_secs_i64);
                        types::OrderLifetime::from_time_in_force(
                            order.time_in_force(),
                            expire_time,
                            false,
                            order_builder.max_short_term_secs(),
                        )
                        .order_flags()
                    },
                    |ctx| ctx.order_flags,
                );
                orders_to_cancel.push((instrument_id, client_id_u32, order_flags));
            } else {
                log::warn!(
                    "Cannot cancel order {client_order_id}: client_order_id not found in cache"
                );
            }
        }

        if orders_to_cancel.is_empty() {
            return Ok(());
        }

        // Check if any orders are short-term (order_flags == 0)
        // dYdX protocol restriction: short-term MsgCancelOrder cannot be batched
        let has_short_term = orders_to_cancel
            .iter()
            .any(|(_, _, flags)| *flags == types::ORDER_FLAG_SHORT_TERM);

        if has_short_term {
            // Cancel each order individually (short-term cancels cannot be batched)
            log::info!(
                "Cancelling {} orders individually (short-term cancels cannot be batched)",
                orders_to_cancel.len()
            );

            self.spawn_task("cancel_all_orders", async move {
                let mut handles = Vec::with_capacity(orders_to_cancel.len());

                for (inst_id, client_id, order_flags) in orders_to_cancel {
                    let tx_manager = tx_manager.clone();
                    let broadcaster = broadcaster.clone();
                    let order_builder = order_builder.clone();

                    let handle = get_runtime().spawn(async move {
                        // Build cancel message using stored order_flags
                        let msg = match order_builder.build_cancel_order_with_flags(
                            inst_id,
                            client_id,
                            order_flags,
                            block_height,
                        ) {
                            Ok(m) => m,
                            Err(e) => {
                                log::error!(
                                    "Failed to build cancel message for client_id={client_id}: {e:?}"
                                );
                                return;
                            }
                        };

                        // Broadcast cancel (single message per transaction)
                        if let Err(e) = broadcaster
                            .broadcast_with_retry(
                                &tx_manager,
                                vec![msg],
                                &format!("Cancel order {client_id}"),
                            )
                            .await
                        {
                            log::error!("Failed to cancel order client_id={client_id}: {e:?}");
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all cancels to complete
                for handle in handles {
                    let _ = handle.await;
                }

                Ok(())
            });
        } else {
            // All orders are long-term - can batch cancels
            log::info!(
                "Batch cancelling {} long-term orders in single transaction",
                orders_to_cancel.len()
            );

            self.spawn_task("cancel_all_orders", async move {
                // Build all cancel messages using stored order_flags
                let msgs: Result<Vec<_>, _> = orders_to_cancel
                    .iter()
                    .map(|(inst_id, client_id, order_flags)| {
                        order_builder.build_cancel_order_with_flags(
                            *inst_id,
                            *client_id,
                            *order_flags,
                            block_height,
                        )
                    })
                    .collect();

                let msgs = match msgs {
                    Ok(m) => m,
                    Err(e) => {
                        log::error!("Failed to build cancel messages: {e:?}");
                        return Ok(());
                    }
                };

                if msgs.is_empty() {
                    return Ok(());
                }

                // Broadcast batch cancel
                match broadcaster
                    .broadcast_with_retry(
                        &tx_manager,
                        msgs,
                        &format!("Cancel {} orders", orders_to_cancel.len()),
                    )
                    .await
                {
                    Ok(_) => {
                        log::debug!("Successfully cancelled {} orders", orders_to_cancel.len());
                    }
                    Err(e) => {
                        log::error!("Batch cancel failed: {e:?}");
                    }
                }

                Ok(())
            });
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        // Get execution components early for order_flags derivation
        let (tx_manager, broadcaster, order_builder) = match self.get_execution_components() {
            Ok(components) => components,
            Err(e) => {
                log::error!("Failed to get execution components for batch cancel: {e}");
                return Ok(());
            }
        };

        // Convert ClientOrderIds to u32 and get order_flags
        let cache = self.core.cache();

        let mut orders_to_cancel = Vec::with_capacity(cmd.cancels.len());
        for cancel in &cmd.cancels {
            let client_order_id = cancel.client_order_id;
            let client_id_u32 = match self.get_client_order_id_int(client_order_id) {
                Some(id) => id,
                None => {
                    log::warn!(
                        "No u32 mapping found for client_order_id={client_order_id}, skipping cancel"
                    );
                    continue;
                }
            };

            // Get stored order_flags from order context
            let order_flags = self.get_order_context(client_id_u32).map_or_else(
                || {
                    // Fallback: derive from order parameters if context not found
                    log::warn!(
                        "Order context not found for {client_order_id}, deriving flags from order"
                    );
                    match cache.order(&client_order_id) {
                        Some(order) => {
                            let expire_time = order.expire_time().map(nanos_to_secs_i64);
                            types::OrderLifetime::from_time_in_force(
                                order.time_in_force(),
                                expire_time,
                                false,
                                order_builder.max_short_term_secs(),
                            )
                            .order_flags()
                        }
                        None => types::ORDER_FLAG_LONG_TERM, // Default to long-term if not found
                    }
                },
                |ctx| ctx.order_flags,
            );

            orders_to_cancel.push((cancel.instrument_id, client_id_u32, order_flags));
        }
        drop(cache);

        if orders_to_cancel.is_empty() {
            log::warn!("No valid orders to cancel in batch");
            return Ok(());
        }

        let block_height = self.block_time_monitor.current_block_height() as u32;

        // Check if any orders are short-term (order_flags == 0)
        // dYdX protocol restriction: short-term MsgCancelOrder cannot be batched
        let has_short_term = orders_to_cancel
            .iter()
            .any(|(_, _, flags)| *flags == types::ORDER_FLAG_SHORT_TERM);

        if has_short_term {
            // Cancel each order individually (short-term cancels cannot be batched)
            log::info!(
                "Cancelling {} orders individually (short-term cancels cannot be batched)",
                orders_to_cancel.len()
            );

            self.spawn_task("batch_cancel_orders", async move {
                let mut handles = Vec::with_capacity(orders_to_cancel.len());

                for (inst_id, client_id, order_flags) in orders_to_cancel {
                    let tx_manager = tx_manager.clone();
                    let broadcaster = broadcaster.clone();
                    let order_builder = order_builder.clone();

                    let handle = get_runtime().spawn(async move {
                        // Build cancel message using stored order_flags
                        let msg = match order_builder.build_cancel_order_with_flags(
                            inst_id,
                            client_id,
                            order_flags,
                            block_height,
                        ) {
                            Ok(m) => m,
                            Err(e) => {
                                log::error!(
                                    "Failed to build cancel message for client_id={client_id}: {e:?}"
                                );
                                return;
                            }
                        };

                        // Broadcast cancel (single message per transaction)
                        if let Err(e) = broadcaster
                            .broadcast_with_retry(
                                &tx_manager,
                                vec![msg],
                                &format!("Cancel order {client_id}"),
                            )
                            .await
                        {
                            log::error!("Failed to cancel order client_id={client_id}: {e:?}");
                        }
                    });

                    handles.push(handle);
                }

                // Wait for all cancels to complete
                for handle in handles {
                    let _ = handle.await;
                }

                Ok(())
            });
        } else {
            // All orders are long-term - can batch cancels
            log::debug!(
                "Batch cancelling {} long-term orders: {:?}",
                orders_to_cancel.len(),
                orders_to_cancel
            );

            self.spawn_task("batch_cancel_orders", async move {
                // Build cancel messages using stored order_flags
                let cancel_msgs = match order_builder
                    .build_cancel_orders_batch_with_flags(&orders_to_cancel, block_height)
                {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        log::error!("Failed to build batch cancel messages: {e:?}");
                        return Ok(());
                    }
                };

                // Broadcast with retry
                match broadcaster
                    .broadcast_with_retry(&tx_manager, cancel_msgs, "BatchCancelOrders")
                    .await
                {
                    Ok(tx_hash) => {
                        log::debug!(
                            "Successfully batch cancelled {} orders, tx_hash: {}",
                            orders_to_cancel.len(),
                            tx_hash
                        );
                    }
                    Err(e) => {
                        log::error!("Batch cancel failed: {e:?}");
                    }
                }

                Ok(())
            });
        }

        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            log::warn!("dYdX execution client already connected");
            return Ok(());
        }

        log::info!("Connecting to dYdX");

        // Load instruments BEFORE WebSocket connection
        // Per Python implementation: "instruments are used in the first account channel message"
        log::debug!("Loading instruments from HTTP API");
        self.http_client.fetch_and_cache_instruments().await?;
        log::debug!(
            "Loaded {} instruments from HTTP into shared cache",
            self.http_client.cached_instruments_count()
        );

        // Mark instruments as initialized (shared cache is already populated)
        self.mark_instruments_initialized();

        // Initialize gRPC client (deferred from constructor to avoid blocking)
        let grpc_urls = self.config.get_grpc_urls();
        let mut grpc_client = DydxGrpcClient::new_with_fallback(&grpc_urls)
            .await
            .context("failed to construct dYdX gRPC client")?;
        log::debug!("gRPC client initialized");

        // Auto-fetch authenticator IDs if using permissioned keys (API wallet)
        self.resolve_authenticators(&mut grpc_client).await?;

        // Fetch initial block height synchronously so orders can be submitted immediately after connect()
        let initial_height = grpc_client
            .latest_block_height()
            .await
            .context("failed to fetch initial block height")?;
        // Use current time as approximation; actual timestamps will come from WebSocket updates
        self.block_time_monitor
            .record_block(initial_height.0 as u64, chrono::Utc::now());
        log::info!("Initial block height: {}", initial_height.0);

        // Initialize sequence number from chain (proactive initialization).
        // This ensures orders can be submitted immediately after connect() without
        // first-transaction latency penalty, and catches auth errors early.
        let base_account = grpc_client
            .get_account(&self.wallet_address)
            .await
            .context("failed to fetch account for sequence initialization")?;
        self.sequence_number
            .store(base_account.sequence, Ordering::SeqCst);
        log::info!("Initial sequence: {}", base_account.sequence);

        *self.grpc_client.write().await = Some(grpc_client.clone());

        // Initialize new execution components
        let wallet_guard = self.wallet.read().await;
        let wallet_clone = wallet_guard
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?
            .clone();
        drop(wallet_guard);

        self.tx_manager = Some(Arc::new(TransactionManager::new(
            grpc_client.clone(),
            wallet_clone,
            self.wallet_address.clone(),
            self.get_chain_id(),
            self.authenticator_ids.clone(),
            self.sequence_number.clone(),
        )));
        log::debug!("TransactionManager initialized");

        self.broadcaster = Some(Arc::new(TxBroadcaster::new(grpc_client)));
        log::debug!("TxBroadcaster initialized");

        self.order_builder = Some(Arc::new(OrderMessageBuilder::new(
            self.http_client.clone(),
            self.wallet_address.clone(),
            self.subaccount_number,
            self.block_time_monitor.clone(),
        )));
        log::debug!(
            "OrderMessageBuilder initialized (block_time_monitor ready: {}, max_short_term: {:.1}s)",
            self.block_time_monitor.is_ready(),
            SHORT_TERM_ORDER_MAXIMUM_LIFETIME as f64
                * self.block_time_monitor.seconds_per_block_or_default()
        );

        // Connect WebSocket
        self.ws_client.connect().await?;
        log::debug!("WebSocket connected");

        // Subscribe to block height updates
        self.ws_client.subscribe_block_height().await?;
        log::debug!("Subscribed to block height updates");

        // Subscribe to markets for instrument data
        self.ws_client.subscribe_markets().await?;
        log::debug!("Subscribed to markets");

        // Subscribe to subaccount updates (wallet is always initialized for execution client)
        log::info!(
            "Using wallet address for queries: {} (subaccount {})",
            self.wallet_address,
            self.subaccount_number
        );
        self.ws_client
            .subscribe_subaccount(&self.wallet_address, self.subaccount_number)
            .await?;
        log::debug!(
            "Subscribed to subaccount updates: {}/{}",
            self.wallet_address,
            self.subaccount_number
        );

        // Fetch initial account state via HTTP (required before orders can be submitted)
        log::debug!("Fetching initial account state from HTTP API");
        let subaccount_response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch initial account state")?;

        let inst_map = self.instrument_cache.to_instrument_id_map();

        // Build empty oracle prices map (not available yet during connect)
        let oracle_map: std::collections::HashMap<_, _> = self
            .oracle_prices
            .iter()
            .map(|entry| (*entry.key(), *entry.value()))
            .collect();

        let ts_init = self.clock.get_time_ns();
        let account_state = parse_http_account_state(
            &subaccount_response.subaccount,
            self.core.account_id,
            &inst_map,
            &oracle_map,
            ts_init,
            ts_init,
        )
        .context("failed to parse initial account state")?;

        log::debug!(
            "Initial account state: {} balance(s), {} margin(s)",
            account_state.balances.len(),
            account_state.margins.len()
        );

        let ts_event = self.clock.get_time_ns();
        self.emitter.emit_account_state(
            account_state.balances,
            account_state.margins,
            account_state.is_reported,
            ts_event,
        );

        // Wait for account to be registered in cache before continuing.
        // This ensures execution state reconciliation can process fills correctly
        // (fills require the account to be registered for portfolio updates).
        self.await_account_registered(30.0).await?;

        // Spawn WebSocket message processing task following standard adapter pattern
        // Per docs/developer_guide/adapters.md: Parse -> Dispatch -> Engine handles events
        if let Some(mut rx) = self.ws_client.take_receiver() {
            log::debug!("Starting execution WebSocket message processing task");

            // Clone data needed for account state parsing in spawned task
            let account_id = self.core.account_id;
            let instrument_cache = self.instrument_cache.clone();
            let oracle_prices = self.oracle_prices.clone();
            let order_contexts = self.order_contexts.clone();
            let block_time_monitor = self.block_time_monitor.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                    log::debug!("Execution WebSocket message loop started");
                    while let Some(msg) = rx.recv().await {
                        let msg_type = match &msg {
                            NautilusWsMessage::Data(_) => "Data",
                            NautilusWsMessage::Deltas(_) => "Deltas",
                            NautilusWsMessage::Order(_) => "Order",
                            NautilusWsMessage::Fill(_) => "Fill",
                            NautilusWsMessage::Position(_) => "Position",
                            NautilusWsMessage::AccountState(_) => "AccountState",
                            NautilusWsMessage::SubaccountSubscribed(_) => "SubaccountSubscribed",
                            NautilusWsMessage::SubaccountsChannelData(_) => "SubaccountsChannelData",
                            NautilusWsMessage::OraclePrices(_) => "OraclePrices",
                            NautilusWsMessage::BlockHeight { .. } => "BlockHeight",
                            NautilusWsMessage::Error(_) => "Error",
                            NautilusWsMessage::Reconnected => "Reconnected",
                        };
                        log::debug!("Execution client received: {msg_type}");
                        match msg {
                            NautilusWsMessage::Order(report) => {
                                log::debug!("Received order update: {:?}", report.order_status);
                                emitter.send_order_status_report(*report);
                            }
                            NautilusWsMessage::Fill(report) => {
                                log::debug!("Received fill update");
                                emitter.send_fill_report(*report);
                            }
                            NautilusWsMessage::Position(report) => {
                                log::debug!("Received position update");
                                emitter.send_position_report(*report);
                            }
                            NautilusWsMessage::AccountState(state) => {
                                log::debug!("Received account state update");
                                emitter.send_account_state(*state);
                            }
                            NautilusWsMessage::SubaccountSubscribed(msg) => {
                                log::debug!(
                                    "Parsing subaccount subscription with full context"
                                );

                                // Build instruments map for parsing from shared cache
                                let inst_map = instrument_cache.to_instrument_id_map();

                                // Build oracle prices map (copy Decimals)
                                let oracle_map: std::collections::HashMap<_, _> = oracle_prices
                                    .iter()
                                    .map(|entry| (*entry.key(), *entry.value()))
                                    .collect();

                                let ts_init = clock.get_time_ns();
                                let ts_event = ts_init;

                                match crate::http::parse::parse_account_state(
                                    &msg.contents.subaccount,
                                    account_id,
                                    &inst_map,
                                    &oracle_map,
                                    ts_event,
                                    ts_init,
                                ) {
                                    Ok(account_state) => {
                                        log::debug!(
                                            "Parsed account state: {} balance(s), {} margin(s)",
                                            account_state.balances.len(),
                                            account_state.margins.len()
                                        );
                                        emitter.send_account_state(account_state);
                                    }
                                    Err(e) => {
                                        log::error!("Failed to parse account state: {e}");
                                    }
                                }

                                // Parse positions from the subscription
                                if let Some(ref positions) =
                                    msg.contents.subaccount.open_perpetual_positions
                                {
                                    log::debug!(
                                        "Parsing {} position(s) from subscription",
                                        positions.len()
                                    );

                                    for (market, ws_position) in positions {
                                        match crate::websocket::parse::parse_ws_position_report(
                                            ws_position,
                                            &instrument_cache,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                log::debug!(
                                                    "Parsed position report: {} {} {} {}",
                                                    report.instrument_id,
                                                    report.position_side,
                                                    report.quantity,
                                                    market
                                                );
                                                emitter.send_position_report(report);
                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Failed to parse WebSocket position for {market}: {e}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::SubaccountsChannelData(data) => {
                                log::debug!("Processing subaccounts channel data (orders={:?}, fills={:?})",
                                    data.contents.orders.as_ref().map(|o| o.len()),
                                    data.contents.fills.as_ref().map(|f| f.len())
                                );
                                let ts_init = clock.get_time_ns();

                                // Process orders
                                if let Some(ref orders) = data.contents.orders {
                                    log::info!("Processing {} orders from SubaccountsChannelData", orders.len());
                                    for ws_order in orders {
                                        log::info!(
                                            "Parsing WS order: clob_pair_id={}, status={:?}, client_id={}",
                                            ws_order.clob_pair_id,
                                            ws_order.status,
                                            ws_order.client_id
                                        );
                                        match crate::websocket::parse::parse_ws_order_report(
                                            ws_order,
                                            &instrument_cache,
                                            &order_contexts,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                log::info!(
                                                    "Parsed order report: {} {} {:?} qty={} client_order_id={:?}",
                                                    report.instrument_id,
                                                    report.order_side,
                                                    report.order_status,
                                                    report.quantity,
                                                    report.client_order_id
                                                );
                                                emitter.send_order_status_report(report);
                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Failed to parse WebSocket order: {e}"
                                                );
                                            }
                                        }
                                    }
                                }

                                // Process fills
                                if let Some(ref fills) = data.contents.fills {
                                    for ws_fill in fills {
                                        match crate::websocket::parse::parse_ws_fill_report(
                                            ws_fill,
                                            &instrument_cache,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                log::debug!(
                                                    "Parsed fill report: {} {} {} @ {}",
                                                    report.instrument_id,
                                                    report.venue_order_id,
                                                    report.last_qty,
                                                    report.last_px
                                                );
                                                emitter.send_fill_report(report);
                                            }
                                            Err(e) => {
                                                log::error!(
                                                    "Failed to parse WebSocket fill: {e}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::OraclePrices(oracle_prices_map) => {
                                log::debug!(
                                    "Processing oracle price updates for {} markets",
                                    oracle_prices_map.len()
                                );

                                // Update oracle_prices map with new prices
                                for (market_symbol, oracle_data) in &oracle_prices_map {
                                    // Parse oracle price
                                    match oracle_data.oracle_price.parse::<Decimal>()
                                    {
                                        Ok(price) => {
                                            // Find instrument by market ticker (oracle uses "BTC-USD")
                                            if let Some(instrument) = instrument_cache.get_by_market(market_symbol) {
                                                let instrument_id = instrument.id();
                                                oracle_prices.insert(instrument_id, price);
                                                log::trace!(
                                                    "Updated oracle price for {instrument_id}: {price}"
                                                );
                                            } else {
                                                log::debug!(
                                                    "No instrument found for market symbol '{market_symbol}'"
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            log::warn!(
                                                "Failed to parse oracle price for {market_symbol}: {e}"
                                            );
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::BlockHeight { height, time } => {
                                log::debug!("Block height update: {height} at {time}");
                                block_time_monitor.record_block(height, time);
                            }
                            NautilusWsMessage::Error(err) => {
                                log::error!("WebSocket error: {err:?}");
                            }
                            NautilusWsMessage::Reconnected => {
                                log::info!("WebSocket reconnected");
                            }
                            _ => {
                                // Data, Deltas are for market data, not execution
                            }
                        }
                    }
                    log::debug!("WebSocket message processing task ended");
                });

            self.ws_stream_handle = Some(handle);
            log::debug!("Spawned WebSocket message processing task");
        } else {
            log::error!("Failed to take WebSocket receiver - no messages will be processed");
        }

        self.connected = true;
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            log::warn!("dYdX execution client not connected");
            return Ok(());
        }

        log::info!("Disconnecting from dYdX");

        // Unsubscribe from subaccount (execution client always has credentials)
        let _ = self
            .ws_client
            .unsubscribe_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from subaccount: {e}"));

        // Unsubscribe from markets
        let _ = self
            .ws_client
            .unsubscribe_markets()
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from markets: {e}"));

        // Unsubscribe from block height
        let _ = self
            .ws_client
            .unsubscribe_block_height()
            .await
            .map_err(|e| log::warn!("Failed to unsubscribe from block height: {e}"));

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort WebSocket message processing task
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
            log::debug!("Aborted WebSocket message processing task");
        }

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.connected = false;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // Query single order from dYdX API
        let response = self
            .http_client
            .inner
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None,    // market filter
                Some(1), // limit to 1 result
            )
            .await
            .context("failed to fetch order from dYdX API")?;

        if response.is_empty() {
            return Ok(None);
        }

        let order = &response[0];
        let ts_init = UnixNanos::default();

        let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
            Some(inst) => inst,
            None => return Ok(None),
        };

        let report = crate::http::parse::parse_order_status_report(
            order,
            &instrument,
            self.core.account_id,
            ts_init,
        )
        .context("failed to parse order status report")?;

        if let Some(client_order_id) = cmd.client_order_id
            && report.client_order_id != Some(client_order_id)
        {
            return Ok(None);
        }

        if let Some(venue_order_id) = cmd.venue_order_id
            && report.venue_order_id.as_str() != venue_order_id.as_str()
        {
            return Ok(None);
        }

        if let Some(instrument_id) = cmd.instrument_id
            && report.instrument_id != instrument_id
        {
            return Ok(None);
        }

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Query orders from dYdX API
        let response = self
            .http_client
            .inner
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch orders from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for order in response {
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => continue,
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            let report = match crate::http::parse::parse_order_status_report(
                &order,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Failed to parse order status report: {e}");
                    continue;
                }
            };

            reports.push(report);
        }

        // Filter by open_only if specified
        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

        // Filter by time range if specified
        if let Some(start) = cmd.start {
            reports.retain(|r| r.ts_last >= start);
        }
        if let Some(end) = cmd.end {
            reports.retain(|r| r.ts_last <= end);
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let response = self
            .http_client
            .inner
            .get_fills(
                &self.wallet_address,
                self.subaccount_number,
                None, // market filter
                None, // limit
            )
            .await
            .context("failed to fetch fills from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for fill in response.fills {
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    log::warn!("Unknown market in fill: {}", fill.market);
                    continue;
                }
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            let report = match crate::http::parse::parse_fill_report(
                &fill,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Failed to parse fill report: {e}");
                    continue;
                }
            };

            reports.push(report);
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|r| r.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Query subaccount positions from dYdX API
        let response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        for (market_ticker, perp_position) in &response.subaccount.open_perpetual_positions {
            let instrument = match self.get_instrument_by_market(market_ticker) {
                Some(inst) => inst,
                None => {
                    log::warn!("Unknown market in position: {market_ticker}");
                    continue;
                }
            };

            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            let report = match crate::http::parse::parse_position_status_report(
                perp_position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => r,
                Err(e) => {
                    log::warn!("Failed to parse position status report: {e}");
                    continue;
                }
            };

            reports.push(report);
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = UnixNanos::default();

        // Query orders
        let orders_response = self
            .http_client
            .inner
            .get_orders(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch orders for mass status")?;

        // Query subaccount for positions
        let subaccount_response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount for mass status")?;

        // Query fills
        let fills_response = self
            .http_client
            .inner
            .get_fills(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch fills for mass status")?;

        // Parse order reports
        let mut order_reports = Vec::new();
        let mut orders_filtered = 0usize;
        for order in orders_response {
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => {
                    orders_filtered += 1;
                    continue;
                }
            };

            match crate::http::parse::parse_order_status_report(
                &order,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => order_reports.push(r),
                Err(e) => {
                    log::warn!("Failed to parse order status report: {e}");
                    orders_filtered += 1;
                }
            }
        }

        // Parse position reports
        let mut position_reports = Vec::new();
        for (market_ticker, perp_position) in
            &subaccount_response.subaccount.open_perpetual_positions
        {
            let instrument = match self.get_instrument_by_market(market_ticker) {
                Some(inst) => inst,
                None => continue,
            };

            match parse_position_status_report(
                perp_position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => position_reports.push(r),
                Err(e) => {
                    log::warn!("Failed to parse position status report: {e}");
                }
            }
        }

        // Parse fill reports
        let mut fill_reports = Vec::new();
        let mut fills_filtered = 0usize;
        for fill in fills_response.fills {
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    fills_filtered += 1;
                    continue;
                }
            };

            match crate::http::parse::parse_fill_report(
                &fill,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(r) => fill_reports.push(r),
                Err(e) => {
                    log::warn!("Failed to parse fill report: {e}");
                    fills_filtered += 1;
                }
            }
        }

        if lookback_mins.is_some() {
            log::debug!(
                "lookback_mins={:?} filtering not yet implemented. Returning all: {} orders ({} filtered), {} positions, {} fills ({} filtered)",
                lookback_mins,
                order_reports.len(),
                orders_filtered,
                position_reports.len(),
                fill_reports.len(),
                fills_filtered
            );
        } else {
            log::debug!(
                "Generated mass status: {} orders, {} positions, {} fills",
                order_reports.len(),
                position_reports.len(),
                fill_reports.len()
            );
        }

        // Create mass status and add reports
        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            ts_init,
            None, // report_id will be auto-generated
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_position_reports(position_reports);
        mass_status.add_fill_reports(fill_reports);

        Ok(Some(mass_status))
    }
}
