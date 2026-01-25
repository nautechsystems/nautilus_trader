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

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, AtomicU64, Ordering},
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent, ExecutionReport as NautilusExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    env::get_or_env_var_opt,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType, TimeInForce},
    events::{AccountState, OrderCancelRejected, OrderEventAny, OrderRejected, OrderSubmitted},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use nautilus_network::retry::RetryConfig;
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{consts::DYDX_VENUE, credential::DydxCredential, parse::nanos_to_secs_i64},
    config::DydxAdapterConfig,
    execution::submitter::OrderSubmitter,
    grpc::{DydxGrpcClient, Wallet, types::ChainId},
    http::{
        client::DydxHttpClient,
        parse::{parse_http_account_state, parse_position_status_report},
    },
    websocket::{client::DydxWebSocketClient, enums::NautilusWsMessage},
};

pub mod submitter;

/// Maximum client order ID value for dYdX (informational - not enforced by adapter).
///
/// dYdX protocol accepts u32 client IDs. The current implementation uses sequential
/// allocation starting from 1, which will wrap at u32::MAX. If dYdX has a stricter
/// limit, this constant should be updated and enforced in `generate_client_order_id_int`.
pub const MAX_CLIENT_ID: u32 = u32::MAX;

/// Execution report types dispatched from WebSocket message handler.
///
/// This enum groups order and fill reports for unified dispatch handling,
/// following the pattern used by reference adapters (Hyperliquid, OKX).
enum ExecutionReport {
    Order(Box<OrderStatusReport>),
    Fill(Box<FillReport>),
}

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
/// 2. **Async submission** - Non-blocking gRPC calls via `OrderSubmitter`.
///
/// This matches the pattern used in OKX and other exchange adapters, ensuring
/// consistent behavior across the Nautilus ecosystem.
#[derive(Debug)]
pub struct DydxExecutionClient {
    clock: &'static AtomicTime,
    core: ExecutionClientCore,
    config: DydxAdapterConfig,
    http_client: DydxHttpClient,
    ws_client: DydxWebSocketClient,
    grpc_client: Arc<tokio::sync::RwLock<Option<DydxGrpcClient>>>,
    exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    wallet: Arc<tokio::sync::RwLock<Option<Wallet>>>,
    instruments: DashMap<InstrumentId, InstrumentAny>,
    market_to_instrument: DashMap<String, InstrumentId>,
    clob_pair_id_to_instrument: DashMap<u32, InstrumentId>,
    block_height: Arc<AtomicU64>,
    oracle_prices: Arc<DashMap<InstrumentId, Decimal>>,
    client_order_id_to_int: DashMap<ClientOrderId, u32>,
    int_to_client_order_id: Arc<DashMap<u32, ClientOrderId>>,
    next_client_order_id: AtomicU32,
    wallet_address: String,
    subaccount_number: u32,
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
    /// Returns an error if the WebSocket client fails to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: DydxAdapterConfig,
        wallet_address: String,
        subaccount_number: u32,
    ) -> anyhow::Result<Self> {
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

        // Use private WebSocket client for authenticated subaccount subscriptions
        let ws_client = if let Some(credential) = DydxCredential::resolve(
            config.mnemonic.clone(),
            config.is_testnet,
            subaccount_number,
            config.authenticator_ids.clone(),
        )? {
            DydxWebSocketClient::new_private(
                config.ws_url.clone(),
                credential,
                core.account_id,
                Some(20),
            )
        } else {
            DydxWebSocketClient::new_public(config.ws_url.clone(), Some(20))
        };

        let grpc_client = Arc::new(tokio::sync::RwLock::new(None));
        let exec_sender = get_exec_event_sender();

        Ok(Self {
            clock: get_atomic_clock_realtime(),
            core,
            config,
            http_client,
            ws_client,
            grpc_client,
            exec_sender,
            wallet: Arc::new(tokio::sync::RwLock::new(None)),
            instruments: DashMap::new(),
            market_to_instrument: DashMap::new(),
            clob_pair_id_to_instrument: DashMap::new(),
            block_height: Arc::new(AtomicU64::new(0)),
            oracle_prices: Arc::new(DashMap::new()),
            client_order_id_to_int: DashMap::new(),
            int_to_client_order_id: Arc::new(DashMap::new()),
            next_client_order_id: AtomicU32::new(1),
            wallet_address,
            subaccount_number,
            started: false,
            connected: false,
            instruments_initialized: false,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
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
        use dashmap::mapref::entry::Entry;

        // Fast path: already mapped
        if let Some(existing) = self.client_order_id_to_int.get(&client_order_id) {
            return *existing.value();
        }

        // Try parsing as direct integer
        if let Ok(id) = client_order_id.as_str().parse::<u32>() {
            self.client_order_id_to_int.insert(client_order_id, id);
            self.int_to_client_order_id.insert(id, client_order_id);
            return id;
        }

        // Allocate new ID from atomic counter
        match self.client_order_id_to_int.entry(client_order_id) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(vacant) => {
                let id = self
                    .next_client_order_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                vacant.insert(id);
                self.int_to_client_order_id.insert(id, client_order_id);
                id
            }
        }
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

    /// Cache instruments from HTTP client into execution client's lookup maps.
    ///
    /// This populates three data structures for efficient lookups:
    /// - `instruments`: InstrumentId → InstrumentAny
    /// - `market_to_instrument`: Market ticker (e.g., "BTC-USD") → InstrumentId
    /// - `clob_pair_id_to_instrument`: CLOB pair ID → InstrumentId
    fn cache_instruments_from_http(&mut self) {
        let instruments: Vec<InstrumentAny> = self
            .http_client
            .instruments_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        log::debug!(
            "Caching {} instruments in execution client",
            instruments.len()
        );

        for instrument in instruments {
            let instrument_id = instrument.id();
            let symbol = instrument_id.symbol.as_str();

            self.instruments.insert(instrument_id, instrument.clone());

            // dYdX API returns market tickers without the "-PERP" suffix (e.g., "ETH-USD")
            // but Nautilus symbols include it (e.g., "ETH-USD-PERP")
            let market_ticker = symbol.strip_suffix("-PERP").unwrap_or(symbol);
            self.market_to_instrument
                .insert(market_ticker.to_string(), instrument_id);
        }

        // Copy clob_pair_id → InstrumentId mapping from HTTP client
        // The HTTP client populates this from PerpetualMarket.clob_pair_id (authoritative source)
        let http_mapping = self.http_client.clob_pair_id_mapping();
        for entry in http_mapping.iter() {
            self.clob_pair_id_to_instrument
                .insert(*entry.key(), *entry.value());
        }

        self.instruments_initialized = true;
        log::debug!(
            "Cached {} instruments ({} CLOB pair IDs) with market mappings",
            self.instruments.len(),
            self.clob_pair_id_to_instrument.len()
        );
    }

    /// Get an instrument by market ticker (e.g., "BTC-USD").
    fn get_instrument_by_market(&self, market: &str) -> Option<InstrumentAny> {
        self.market_to_instrument
            .get(market)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()))
    }

    /// Get an instrument by clob_pair_id.
    fn get_instrument_by_clob_pair_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        let instrument = self
            .clob_pair_id_to_instrument
            .get(&clob_pair_id)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()));

        if instrument.is_none() {
            self.log_missing_instrument_for_clob_pair_id(clob_pair_id);
        }

        instrument
    }

    fn log_missing_instrument_for_clob_pair_id(&self, clob_pair_id: u32) {
        let known: Vec<(u32, String)> = self
            .clob_pair_id_to_instrument
            .iter()
            .filter_map(|entry| {
                let instrument_id = entry.value();
                self.instruments.get(instrument_id).map(|inst_entry| {
                    (
                        *entry.key(),
                        inst_entry.value().id().symbol.as_str().to_string(),
                    )
                })
            })
            .collect();

        log::warn!(
            "Instrument for clob_pair_id {clob_pair_id} not found in cache. Known CLOB pair IDs and symbols: {known:?}"
        );
    }

    fn send_order_rejected(
        &self,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: &str,
        ts_init: UnixNanos,
    ) {
        let ts_now = self.clock.get_time_ns();
        let event = OrderRejected::new(
            self.core.trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            self.core.account_id,
            reason.into(),
            UUID4::new(),
            ts_init,
            ts_now,
            false,
            false,
        );
        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
        {
            log::warn!("Failed to send OrderRejected event: {e}");
        }
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
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
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        // Capture necessary data for rejection event
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let sender = get_exec_event_sender();

        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                let error_msg = format!("{label} failed: {e:?}");
                log::error!("{error_msg}");

                let ts_now = UnixNanos::default(); // Use current time
                let event = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    error_msg.into(),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    false,
                );

                if let Err(send_err) =
                    sender.send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                {
                    log::error!("Failed to send OrderRejected event: {send_err}");
                }
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
        self.core.get_account()
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            log::warn!("dYdX execution client already started");
            return Ok(());
        }

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
        let order = self.core.get_order(&cmd.client_order_id)?;

        // Check connection status
        if !self.is_connected() {
            let reason = "Cannot submit order: execution client not connected";
            log::error!("{reason}");
            anyhow::bail!(reason);
        }

        // Check block height is available for short-term orders
        let current_block = self.block_height.load(Ordering::Relaxed);
        if current_block == 0 {
            let reason = "Block height not initialized";
            log::warn!(
                "Cannot submit order {}: {}",
                order.client_order_id(),
                reason
            );
            self.send_order_rejected(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                reason,
                cmd.ts_init,
            );
            return Ok(());
        }

        // Check if order is already closed
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Validate order type
        match order.order_type() {
            OrderType::Market | OrderType::Limit => {
                log::debug!(
                    "Submitting {} order: {}",
                    if matches!(order.order_type(), OrderType::Market) {
                        "MARKET"
                    } else {
                        "LIMIT"
                    },
                    order.client_order_id()
                );
            }
            // Conditional orders (stop/take-profit) - supported by dYdX
            OrderType::StopMarket | OrderType::StopLimit => {
                log::debug!(
                    "Submitting {} order: {}",
                    if matches!(order.order_type(), OrderType::StopMarket) {
                        "STOP_MARKET"
                    } else {
                        "STOP_LIMIT"
                    },
                    order.client_order_id()
                );
            }
            // dYdX TakeProfit/TakeProfitLimit map to MarketIfTouched/LimitIfTouched
            OrderType::MarketIfTouched | OrderType::LimitIfTouched => {
                log::debug!(
                    "Submitting {} order: {}",
                    if matches!(order.order_type(), OrderType::MarketIfTouched) {
                        "TAKE_PROFIT_MARKET"
                    } else {
                        "TAKE_PROFIT_LIMIT"
                    },
                    order.client_order_id()
                );
            }
            // Trailing stops not supported by dYdX v4 protocol
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit => {
                let reason = "Trailing stop orders not supported by dYdX v4 protocol";
                log::error!("{reason}");
                self.send_order_rejected(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason,
                    cmd.ts_init,
                );
                return Ok(());
            }
            order_type => {
                let reason = format!("Order type {order_type:?} not supported by dYdX");
                log::error!("{reason}");
                self.send_order_rejected(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    &reason,
                    cmd.ts_init,
                );
                return Ok(());
            }
        }

        let event = OrderSubmitted::new(
            self.core.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.core.account_id,
            UUID4::new(),
            cmd.ts_init,
            self.clock.get_time_ns(),
        );
        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
        {
            log::warn!("Failed to send OrderSubmitted event: {e}");
        }

        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let client_order_id = order.client_order_id();
        let instrument_id = order.instrument_id();
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        let chain_id = self.get_chain_id();
        let authenticator_ids = self.config.authenticator_ids.clone();
        #[allow(clippy::redundant_clone)]
        let order_clone = order.clone();

        // Generate client_order_id as u32 before async block (dYdX requires u32 client IDs)
        let client_id_u32 = self.generate_client_order_id_int(client_order_id);

        self.spawn_order_task(
            "submit_order",
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            async move {
                let wallet_guard = wallet.read().await;
                let wallet_ref = wallet_guard
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

                let grpc_guard = grpc_client.read().await;
                let grpc_ref = grpc_guard
                    .as_ref()
                    .ok_or_else(|| anyhow::anyhow!("gRPC client not initialized"))?;
                let submitter = OrderSubmitter::new(
                    grpc_ref.clone(),
                    http_client.clone(),
                    wallet_address,
                    subaccount_number,
                    chain_id,
                    authenticator_ids,
                );

                match order_clone.order_type() {
                    OrderType::Market => {
                        submitter
                            .submit_market_order(
                                wallet_ref,
                                instrument_id,
                                client_id_u32,
                                order_clone.order_side(),
                                order_clone.quantity(),
                                block_height,
                            )
                            .await?;
                        log::debug!("Successfully submitted market order: {client_order_id}");
                    }
                    OrderType::Limit => {
                        let expire_time = order_clone.expire_time().map(nanos_to_secs_i64);
                        submitter
                            .submit_limit_order(
                                wallet_ref,
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
                                expire_time,
                            )
                            .await?;
                        log::debug!("Successfully submitted limit order: {client_order_id}");
                    }
                    OrderType::StopMarket => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop market order missing trigger_price")
                        })?;
                        let expire_time = order_clone.expire_time().map(nanos_to_secs_i64);
                        submitter
                            .submit_stop_market_order(
                                wallet_ref,
                                instrument_id,
                                client_id_u32,
                                order_clone.order_side(),
                                trigger_price,
                                order_clone.quantity(),
                                order_clone.is_reduce_only(),
                                expire_time,
                            )
                            .await?;
                        log::debug!("Successfully submitted stop market order: {client_order_id}");
                    }
                    OrderType::StopLimit => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing trigger_price")
                        })?;
                        let limit_price = order_clone.price().ok_or_else(|| {
                            anyhow::anyhow!("Stop limit order missing limit price")
                        })?;
                        let expire_time = order_clone.expire_time().map(nanos_to_secs_i64);
                        submitter
                            .submit_stop_limit_order(
                                wallet_ref,
                                instrument_id,
                                client_id_u32,
                                order_clone.order_side(),
                                trigger_price,
                                limit_price,
                                order_clone.quantity(),
                                order_clone.time_in_force(),
                                order_clone.is_post_only(),
                                order_clone.is_reduce_only(),
                                expire_time,
                            )
                            .await?;
                        log::debug!("Successfully submitted stop limit order: {client_order_id}");
                    }
                    // dYdX TakeProfitMarket maps to Nautilus MarketIfTouched
                    OrderType::MarketIfTouched => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit market order missing trigger_price")
                        })?;
                        let expire_time = order_clone.expire_time().map(nanos_to_secs_i64);
                        submitter
                            .submit_take_profit_market_order(
                                wallet_ref,
                                instrument_id,
                                client_id_u32,
                                order_clone.order_side(),
                                trigger_price,
                                order_clone.quantity(),
                                order_clone.is_reduce_only(),
                                expire_time,
                            )
                            .await?;
                        log::debug!(
                            "Successfully submitted take profit market order: {client_order_id}"
                        );
                    }
                    // dYdX TakeProfitLimit maps to Nautilus LimitIfTouched
                    OrderType::LimitIfTouched => {
                        let trigger_price = order_clone.trigger_price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing trigger_price")
                        })?;
                        let limit_price = order_clone.price().ok_or_else(|| {
                            anyhow::anyhow!("Take profit limit order missing limit price")
                        })?;
                        let expire_time = order_clone.expire_time().map(nanos_to_secs_i64);
                        submitter
                            .submit_take_profit_limit_order(
                                wallet_ref,
                                instrument_id,
                                client_id_u32,
                                order_clone.order_side(),
                                trigger_price,
                                limit_price,
                                order_clone.quantity(),
                                order_clone.time_in_force(),
                                order_clone.is_post_only(),
                                order_clone.is_reduce_only(),
                                expire_time,
                            )
                            .await?;
                        log::debug!(
                            "Successfully submitted take profit limit order: {client_order_id}"
                        );
                    }
                    _ => unreachable!("Order type already validated"),
                }

                Ok(())
            },
        );

        Ok(())
    }

    fn submit_order_list(&self, _cmd: &SubmitOrderList) -> anyhow::Result<()> {
        anyhow::bail!("Order lists not supported by dYdX")
    }

    fn modify_order(&self, _cmd: &ModifyOrder) -> anyhow::Result<()> {
        anyhow::bail!("Order modification not supported by dYdX")
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
        let cache_borrow = cache.borrow();

        let order = match cache_borrow.order(&client_order_id) {
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
        let instrument = match cache_borrow.instrument(&instrument_id) {
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

        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        let chain_id = self.get_chain_id();
        let authenticator_ids = self.config.authenticator_ids.clone();
        let trader_id = cmd.trader_id;
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

        // Clone sender before spawning for use in async block
        let exec_sender = self.exec_sender.clone();

        self.spawn_task("cancel_order", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let grpc_ref = grpc_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("gRPC client not initialized"))?;
            let submitter = OrderSubmitter::new(
                grpc_ref.clone(),
                http_client.clone(),
                wallet_address,
                subaccount_number,
                chain_id,
                authenticator_ids,
            );

            // Attempt cancellation via submitter
            match submitter
                .cancel_order(wallet_ref, instrument_id, client_id_u32, block_height)
                .await
            {
                Ok(()) => {
                    log::debug!("Successfully cancelled order: {client_order_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel order {client_order_id}: {e:?}");

                    let ts_now = UnixNanos::default();
                    let event = OrderCancelRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        format!("Cancel order failed: {e:?}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        venue_order_id,
                        None, // account_id not available in async context
                    );
                    if let Err(send_err) = exec_sender
                        .send(ExecutionEvent::Order(OrderEventAny::CancelRejected(event)))
                    {
                        log::warn!("Failed to send OrderCancelRejected event: {send_err}");
                    }
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
        let cache = self.core.cache().borrow();
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

        // Cancel each order individually (dYdX requires separate transactions)
        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        let chain_id = self.get_chain_id();
        let authenticator_ids = self.config.authenticator_ids.clone();

        // Collect (instrument_id, client_id) tuples for batch cancel
        let mut orders_to_cancel = Vec::new();
        for order in &open_orders {
            let client_order_id = order.client_order_id();
            if let Some(client_id_u32) = self.get_client_order_id_int(client_order_id) {
                orders_to_cancel.push((instrument_id, client_id_u32));
            } else {
                log::warn!(
                    "Cannot cancel order {client_order_id}: client_order_id not found in cache"
                );
            }
        }

        self.spawn_task("cancel_all_orders", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let grpc_ref = grpc_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("gRPC client not initialized"))?;
            let submitter = OrderSubmitter::new(
                grpc_ref.clone(),
                http_client.clone(),
                wallet_address,
                subaccount_number,
                chain_id,
                authenticator_ids,
            );

            // Cancel orders using batch method (executes sequentially to avoid nonce conflicts)
            match submitter
                .cancel_orders_batch(wallet_ref, &orders_to_cancel, block_height)
                .await
            {
                Ok(()) => {
                    log::debug!("Successfully cancelled {} orders", orders_to_cancel.len());
                }
                Err(e) => {
                    log::error!("Batch cancel failed: {e:?}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            return Ok(());
        }

        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        // Convert ClientOrderIds to u32 before async block
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
            orders_to_cancel.push((cancel.instrument_id, client_id_u32));
        }

        if orders_to_cancel.is_empty() {
            log::warn!("No valid orders to cancel in batch");
            return Ok(());
        }

        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        let chain_id = self.get_chain_id();
        let authenticator_ids = self.config.authenticator_ids.clone();

        log::debug!(
            "Batch cancelling {} orders: {:?}",
            orders_to_cancel.len(),
            orders_to_cancel
        );

        self.spawn_task("batch_cancel_orders", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let grpc_ref = grpc_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("gRPC client not initialized"))?;
            let submitter = OrderSubmitter::new(
                grpc_ref.clone(),
                http_client.clone(),
                wallet_address,
                subaccount_number,
                chain_id,
                authenticator_ids,
            );

            match submitter
                .cancel_orders_batch(wallet_ref, &orders_to_cancel, block_height)
                .await
            {
                Ok(()) => {
                    log::debug!(
                        "Successfully batch cancelled {} orders",
                        orders_to_cancel.len()
                    );
                }
                Err(e) => {
                    log::error!("Batch cancel failed: {e:?}");
                }
            }

            Ok(())
        });

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
            "Loaded {} instruments from HTTP",
            self.http_client.instruments_cache.len()
        );

        // Populate execution client's instrument lookup maps
        self.cache_instruments_from_http();

        // Cache instruments to WebSocket client for handler to use
        let instruments: Vec<InstrumentAny> = self
            .instruments
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        self.ws_client.cache_instruments(instruments);

        // Initialize gRPC client (deferred from constructor to avoid blocking)
        let grpc_urls = self.config.get_grpc_urls();
        let grpc_client = DydxGrpcClient::new_with_fallback(&grpc_urls)
            .await
            .context("failed to construct dYdX gRPC client")?;
        *self.grpc_client.write().await = Some(grpc_client);
        log::debug!("gRPC client initialized");

        let mnemonic_resolved = get_or_env_var_opt(
            self.config.mnemonic.clone(),
            if self.config.is_testnet {
                "DYDX_TESTNET_MNEMONIC"
            } else {
                "DYDX_MNEMONIC"
            },
        );

        if let Some(ref mnemonic) = mnemonic_resolved {
            let wallet = Wallet::from_mnemonic(mnemonic)?;
            *self.wallet.write().await = Some(wallet);
            log::debug!("Wallet initialized");
        }

        // Connect WebSocket
        self.ws_client.connect().await?;
        log::debug!("WebSocket connected");

        // Subscribe to block height updates
        self.ws_client.subscribe_block_height().await?;
        log::debug!("Subscribed to block height updates");

        // Subscribe to markets for instrument data
        self.ws_client.subscribe_markets().await?;
        log::debug!("Subscribed to markets");

        // Subscribe to subaccount updates if authenticated
        if mnemonic_resolved.is_some() {
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

            let inst_map: std::collections::HashMap<_, _> = self
                .instruments
                .iter()
                .map(|entry| (*entry.key(), entry.value().clone()))
                .collect();

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

            self.core.generate_account_state(
                account_state.balances,
                account_state.margins,
                account_state.is_reported,
                ts_init,
            )?;

            // Spawn WebSocket message processing task following standard adapter pattern
            // Per docs/developer_guide/adapters.md: Parse -> Dispatch -> Engine handles events
            if let Some(mut rx) = self.ws_client.take_receiver() {
                log::debug!("Starting execution WebSocket message processing task");

                // Clone data needed for account state parsing in spawned task
                let account_id = self.core.account_id;
                let instruments = self.instruments.clone();
                let oracle_prices = self.oracle_prices.clone();
                let clob_pair_id_to_instrument = self.clob_pair_id_to_instrument.clone();
                let int_to_client_order_id = self.int_to_client_order_id.clone();
                let block_height = self.block_height.clone();
                let exec_sender = self.exec_sender.clone();
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
                            NautilusWsMessage::BlockHeight(_) => "BlockHeight",
                            NautilusWsMessage::Error(_) => "Error",
                            NautilusWsMessage::Reconnected => "Reconnected",
                        };
                        log::debug!("Execution client received: {msg_type}");
                        match msg {
                            NautilusWsMessage::Order(report) => {
                                log::debug!("Received order update: {:?}", report.order_status);
                                dispatch_execution_report(ExecutionReport::Order(report), &exec_sender);
                            }
                            NautilusWsMessage::Fill(report) => {
                                log::debug!("Received fill update");
                                dispatch_execution_report(ExecutionReport::Fill(report), &exec_sender);
                            }
                            NautilusWsMessage::Position(report) => {
                                log::debug!("Received position update");
                                // Dispatch position status reports via execution event system
                                let exec_report =
                                    NautilusExecutionReport::Position(Box::new(*report));
                                if let Err(e) = exec_sender.send(ExecutionEvent::Report(exec_report)) {
                                    log::warn!("Failed to send position status report: {e}");
                                }
                            }
                            NautilusWsMessage::AccountState(state) => {
                                log::debug!("Received account state update");
                                dispatch_account_state(*state, &exec_sender);
                            }
                            NautilusWsMessage::SubaccountSubscribed(msg) => {
                                log::debug!(
                                    "Parsing subaccount subscription with full context"
                                );

                                // Build instruments map for parsing (clone to avoid lifetime issues)
                                let inst_map: std::collections::HashMap<_, _> = instruments
                                    .iter()
                                    .map(|entry| (*entry.key(), entry.value().clone()))
                                    .collect();

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
                                        dispatch_account_state(account_state, &exec_sender);
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
                                            &instruments,
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
                                                let exec_report = NautilusExecutionReport::Position(
                                                    Box::new(report),
                                                );
                                                if let Err(e) =
                                                    exec_sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    log::warn!(
                                                        "Failed to send position status report: {e}"
                                                    );
                                                }
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
                                    for ws_order in orders {
                                        log::debug!(
                                            "Parsing WS order: clob_pair_id={}, status={:?}, client_id={}",
                                            ws_order.clob_pair_id,
                                            ws_order.status,
                                            ws_order.client_id
                                        );
                                        match crate::websocket::parse::parse_ws_order_report(
                                            ws_order,
                                            &clob_pair_id_to_instrument,
                                            &instruments,
                                            &int_to_client_order_id,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                log::debug!(
                                                    "Parsed order report: {} {} {:?} qty={} client_order_id={:?}",
                                                    report.instrument_id,
                                                    report.order_side,
                                                    report.order_status,
                                                    report.quantity,
                                                    report.client_order_id
                                                );
                                                let exec_report =
                                                    NautilusExecutionReport::Order(Box::new(
                                                        report,
                                                    ));
                                                if let Err(e) =
                                                    exec_sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    log::warn!(
                                                        "Failed to send order status report: {e}"
                                                    );
                                                }
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
                                            &instruments,
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
                                                let exec_report =
                                                    NautilusExecutionReport::Fill(Box::new(report));
                                                if let Err(e) =
                                                    exec_sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    log::warn!(
                                                        "Failed to send fill report: {e}"
                                                    );
                                                }
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
                                            // Find instrument by symbol (oracle uses raw symbol like "BTC-USD")
                                            // Append "-PERP" to match instrument IDs
                                            let symbol_with_perp = format!("{market_symbol}-PERP");

                                            // Find matching instrument
                                            if let Some(entry) = instruments.iter().find(|entry| {
                                                entry.value().id().symbol.as_str()
                                                    == symbol_with_perp
                                            }) {
                                                let instrument_id = *entry.key();
                                                oracle_prices.insert(instrument_id, price);
                                                log::trace!(
                                                    "Updated oracle price for {instrument_id}: {price}"
                                                );
                                            } else {
                                                log::debug!(
                                                    "No instrument found for market symbol '{market_symbol}' (tried '{symbol_with_perp}')"
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
                            NautilusWsMessage::BlockHeight(height) => {
                                log::debug!("Block height update: {height}");
                                block_height.store(height, std::sync::atomic::Ordering::Relaxed);
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

        let mnemonic_resolved = get_or_env_var_opt(
            self.config.mnemonic.clone(),
            if self.config.is_testnet {
                "DYDX_TESTNET_MNEMONIC"
            } else {
                "DYDX_MNEMONIC"
            },
        );

        if mnemonic_resolved.is_some() {
            let _ = self
                .ws_client
                .unsubscribe_subaccount(&self.wallet_address, self.subaccount_number)
                .await
                .map_err(|e| log::warn!("Failed to unsubscribe from subaccount: {e}"));
        }

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

/// Dispatches account state events to the portfolio.
///
/// AccountState events are routed to the Portfolio (not ExecEngine) via msgbus.
/// This follows the pattern used by BitMEX, OKX, and other reference adapters.
fn dispatch_account_state(
    state: AccountState,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    if let Err(e) = sender.send(ExecutionEvent::Account(state)) {
        log::warn!("Failed to send account state: {e}");
    }
}

/// Dispatches execution reports to the execution engine.
///
/// This follows the standard adapter pattern where WebSocket handlers parse messages
/// into reports, and a dispatch function sends them via the execution event system.
/// The execution engine then handles cache lookups and event generation.
///
/// # Architecture
///
/// Per `docs/developer_guide/adapters.md`, adapters should:
/// 1. Parse WebSocket messages into ExecutionReports in the handler.
/// 2. Dispatch reports via the execution event sender.
/// 3. Let the execution engine handle event generation (has cache access).
///
/// This pattern is used by Hyperliquid, OKX, BitMEX, and other reference adapters.
fn dispatch_execution_report(
    report: ExecutionReport,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    match report {
        ExecutionReport::Order(order_report) => {
            log::debug!(
                "Dispatching order report: status={:?}, venue_order_id={:?}, client_order_id={:?}",
                order_report.order_status,
                order_report.venue_order_id,
                order_report.client_order_id
            );
            let exec_report = NautilusExecutionReport::Order(order_report);
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send order status report: {e}");
            }
        }
        ExecutionReport::Fill(fill_report) => {
            log::debug!(
                "Dispatching fill report: venue_order_id={}, trade_id={}",
                fill_report.venue_order_id,
                fill_report.trade_id
            );
            let exec_report = NautilusExecutionReport::Fill(fill_report);
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send fill report: {e}");
            }
        }
    }
}
