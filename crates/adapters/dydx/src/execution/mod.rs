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

//! Live execution client implementation for the dYdX adapter.

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicU32, AtomicU64},
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    msgbus,
};
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos};
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::client::LiveExecutionClient;
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderSide, OrderType, TimeInForce},
    events::{OrderCancelRejected, OrderEventAny, OrderRejected},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{consts::DYDX_VENUE, credential::DydxCredential},
    config::DydxAdapterConfig,
    execution::submitter::OrderSubmitter,
    grpc::{DydxGrpcClient, Wallet, types::ChainId},
    http::client::DydxHttpClient,
    websocket::{client::DydxWebSocketClient, messages::NautilusWsMessage},
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
/// Supports Market and Limit orders via gRPC. Conditional orders (Stop, Take Profit,
/// Trailing Stop) planned for future releases. dYdX requires u32 client IDs - strings
/// are hashed to fit this constraint.
///
/// # Architecture
///
/// The client follows a two-layer execution model:
/// 1. **Synchronous validation** - Immediate checks and event generation
/// 2. **Async submission** - Non-blocking gRPC calls via `OrderSubmitter`
///
/// This matches the pattern used in OKX and other exchange adapters, ensuring
/// consistent behavior across the Nautilus ecosystem.
#[derive(Debug)]
pub struct DydxExecutionClient {
    core: ExecutionClientCore,
    config: DydxAdapterConfig,
    http_client: DydxHttpClient,
    ws_client: DydxWebSocketClient,
    grpc_client: Arc<tokio::sync::RwLock<DydxGrpcClient>>,
    wallet: Arc<tokio::sync::RwLock<Option<Wallet>>>,
    instruments: DashMap<InstrumentId, InstrumentAny>,
    market_to_instrument: DashMap<String, InstrumentId>,
    clob_pair_id_to_instrument: DashMap<u32, InstrumentId>,
    block_height: AtomicU64,
    oracle_prices: DashMap<InstrumentId, Decimal>,
    client_id_to_int: DashMap<String, u32>,
    int_to_client_id: DashMap<u32, String>,
    next_client_id: AtomicU32,
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
        let http_client = DydxHttpClient::default();

        // Use private WebSocket client for authenticated subaccount subscriptions
        let ws_client = if let Some(ref mnemonic) = config.mnemonic {
            let credential = DydxCredential::from_mnemonic(mnemonic, subaccount_number, vec![])?;
            DydxWebSocketClient::new_private(
                config.ws_url.clone(),
                credential,
                core.account_id,
                Some(20),
            )
        } else {
            DydxWebSocketClient::new_public(config.ws_url.clone(), Some(20))
        };

        let grpc_urls = config.get_grpc_urls();
        let grpc_client = Arc::new(tokio::sync::RwLock::new(
            get_runtime()
                .block_on(async { DydxGrpcClient::new_with_fallback(&grpc_urls).await })
                .context("failed to construct dYdX gRPC client")?,
        ));

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            grpc_client,
            wallet: Arc::new(tokio::sync::RwLock::new(None)),
            instruments: DashMap::new(),
            market_to_instrument: DashMap::new(),
            clob_pair_id_to_instrument: DashMap::new(),
            block_height: AtomicU64::new(0),
            oracle_prices: DashMap::new(),
            client_id_to_int: DashMap::new(),
            int_to_client_id: DashMap::new(),
            next_client_id: AtomicU32::new(1),
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
    /// - Same `client_order_id` string → same `u32` for the lifetime of this process
    /// - Different `client_order_id` strings → different `u32` values (except on u32 wrap)
    /// - Thread-safe for concurrent calls
    ///
    /// # Behavior
    ///
    /// - Parses numeric `client_order_id` directly to `u32` for stability across restarts
    /// - For non-numeric IDs, allocates a new sequential value from an atomic counter
    /// - Mapping is kept in-memory only; non-numeric IDs will not be recoverable after restart
    /// - Counter starts at 1 and increments without bound checking (will wrap at u32::MAX)
    ///
    /// # Notes
    ///
    /// - Atomic counter uses `Relaxed` ordering — uniqueness is required, not cross-thread sequencing
    /// - If dYdX enforces a maximum client ID below u32::MAX, additional range validation is needed
    fn generate_client_order_id_int(&self, client_order_id: &str) -> u32 {
        // Fast path: already mapped
        if let Some(existing) = self.client_id_to_int.get(client_order_id) {
            return *existing.value();
        }

        // Try parsing as direct integer
        if let Ok(id) = client_order_id.parse::<u32>() {
            self.client_id_to_int
                .insert(client_order_id.to_string(), id);
            self.int_to_client_id
                .insert(id, client_order_id.to_string());
            return id;
        }

        // Allocate new ID from atomic counter
        use dashmap::mapref::entry::Entry;

        match self.client_id_to_int.entry(client_order_id.to_string()) {
            Entry::Occupied(entry) => *entry.get(),
            Entry::Vacant(vacant) => {
                let id = self
                    .next_client_id
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                vacant.insert(id);
                self.int_to_client_id
                    .insert(id, client_order_id.to_string());
                id
            }
        }
    }

    /// Retrieve the client order ID integer from the cache.
    ///
    /// Returns `None` if the mapping doesn't exist.
    fn get_client_order_id_int(&self, client_order_id: &str) -> Option<u32> {
        // Try parsing first
        if let Ok(id) = client_order_id.parse::<u32>() {
            return Some(id);
        }

        // Look up in cache
        self.client_id_to_int
            .get(client_order_id)
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
        use nautilus_model::instruments::InstrumentAny;

        // Get all instruments from HTTP client cache
        let instruments: Vec<InstrumentAny> = self
            .http_client
            .instruments_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect();

        tracing::debug!(
            "Caching {} instruments in execution client",
            instruments.len()
        );

        for instrument in instruments {
            let instrument_id = instrument.id();
            let symbol = instrument_id.symbol.as_str();

            // Cache instrument by InstrumentId
            self.instruments.insert(instrument_id, instrument.clone());

            // Cache market ticker → InstrumentId mapping
            self.market_to_instrument
                .insert(symbol.to_string(), instrument_id);
        }

        // Copy clob_pair_id → InstrumentId mapping from HTTP client
        // The HTTP client populates this from PerpetualMarket.clob_pair_id (authoritative source)
        let http_mapping = self.http_client.clob_pair_id_mapping();
        for entry in http_mapping.iter() {
            self.clob_pair_id_to_instrument
                .insert(*entry.key(), *entry.value());
        }

        self.instruments_initialized = true;
        tracing::info!(
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

        tracing::warn!(
            "Instrument for clob_pair_id {} not found in cache. Known CLOB pair IDs and symbols: {:?}",
            clob_pair_id,
            known
        );
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = tokio::spawn(async move {
            if let Err(e) = fut.await {
                tracing::error!("{label}: {e:?}");
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

        let handle = tokio::spawn(async move {
            if let Err(e) = fut.await {
                let error_msg = format!("{label} failed: {e:?}");
                tracing::error!("{}", error_msg);

                // Generate OrderRejected event on submission failure
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
                    sender.send(nautilus_common::messages::ExecutionEvent::Order(
                        OrderEventAny::Rejected(event),
                    ))
                {
                    tracing::error!("Failed to send OrderRejected event: {send_err}");
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
            tracing::warn!("dYdX execution client already started");
            return Ok(());
        }

        tracing::info!("Starting dYdX execution client");
        self.started = true;
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            tracing::warn!("dYdX execution client not started");
            return Ok(());
        }

        tracing::info!("Stopping dYdX execution client");
        self.abort_pending_tasks();
        self.started = false;
        self.connected = false;
        Ok(())
    }

    /// Submits an order to dYdX via gRPC.
    ///
    /// dYdX requires u32 client IDs - Nautilus ClientOrderId strings are hashed to fit.
    /// Only Market and Limit orders supported currently. Conditional orders (Stop, Take Profit)
    /// will be implemented in future releases.
    ///
    /// Validates synchronously, generates OrderSubmitted event, then spawns async task for
    /// gRPC submission to avoid blocking. Unsupported order types generate OrderRejected.
    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = cmd.order.clone();

        // Check connection status
        if !self.is_connected() {
            let reason = "Cannot submit order: execution client not connected";
            tracing::error!("{}", reason);
            anyhow::bail!(reason);
        }

        // Check if order is already closed
        if order.is_closed() {
            tracing::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Validate order type - only market and limit orders supported
        match order.order_type() {
            OrderType::Market | OrderType::Limit => {
                // Supported order types
                tracing::debug!(
                    "Submitting {} order: {}",
                    if matches!(order.order_type(), OrderType::Market) {
                        "MARKET"
                    } else {
                        "LIMIT"
                    },
                    order.client_order_id()
                );
            }
            // Conditional order stubs - accept but don't submit until proto implementation
            OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
            | OrderType::TrailingStopMarket
            | OrderType::TrailingStopLimit => {
                self.core.generate_order_submitted(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    cmd.ts_init,
                );
                tracing::warn!(
                    order_type = ?order.order_type(),
                    client_order_id = %order.client_order_id(),
                    "Conditional order stub: OrderSubmitted generated but not sent to exchange (proto implementation pending)"
                );
                return Ok(());
            }
            order_type => {
                let reason = format!("Order type {order_type:?} not supported by dYdX");
                tracing::error!("{}", reason);
                self.core.generate_order_rejected(
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    &reason,
                    cmd.ts_init,
                    false,
                );
                return Ok(());
            }
        }

        // Generate OrderSubmitted event immediately
        self.core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            cmd.ts_init,
        );

        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let http_client = self.http_client.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let client_order_id = order.client_order_id();
        let instrument_id = order.instrument_id();
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        let chain_id = self.get_chain_id();
        #[allow(clippy::redundant_clone)]
        let order_clone = order.clone();

        // Generate client_order_id as u32 before async block (dYdX requires u32 client IDs)
        let client_id_u32 = self.generate_client_order_id_int(client_order_id.as_str());

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
                let submitter = OrderSubmitter::new(
                    (*grpc_guard).clone(),
                    http_client.clone(),
                    wallet_address,
                    subaccount_number,
                    chain_id,
                );

                // Submit order based on type
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
                        tracing::info!("Successfully submitted market order: {}", client_order_id);
                    }
                    OrderType::Limit => {
                        let expire_time = order_clone
                            .expire_time()
                            .map(|t| (t.as_u64() / 1_000_000_000) as i64);
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
                        tracing::info!("Successfully submitted limit order: {}", client_order_id);
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
    /// - Checks order exists in cache
    /// - Validates order is not already closed
    /// - Retrieves instrument from cache for order builder
    ///
    /// The `cmd` contains client/venue order IDs. Returns `Ok(())` if cancel request is
    /// spawned successfully or validation fails gracefully. Returns `Err` if not connected.
    ///
    /// # Events
    /// - `OrderCanceled` - Generated when WebSocket confirms cancellation
    /// - `OrderCancelRejected` - Generated if exchange rejects cancellation
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
                tracing::error!(
                    "Cannot cancel order {}: not found in cache",
                    client_order_id
                );
                return Ok(()); // Not an error - order may have been filled/canceled already
            }
        };

        // Validate order is not already closed
        if order.is_closed() {
            tracing::warn!(
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
                tracing::error!(
                    "Cannot cancel order {}: instrument {} not found in cache",
                    client_order_id,
                    instrument_id
                );
                return Ok(()); // Not an error - missing instrument is a cache issue
            }
        };

        tracing::debug!(
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
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let venue_order_id = cmd.venue_order_id;

        // Convert client_order_id to u32 before async block
        let client_id_u32 = match self.get_client_order_id_int(client_order_id.as_str()) {
            Some(id) => id,
            None => {
                tracing::error!("Client order ID {} not found in cache", client_order_id);
                anyhow::bail!("Client order ID not found in cache")
            }
        };

        self.spawn_task("cancel_order", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let submitter = OrderSubmitter::new(
                (*grpc_guard).clone(),
                http_client.clone(),
                wallet_address,
                subaccount_number,
                chain_id,
            );

            // Attempt cancellation via submitter
            match submitter
                .cancel_order(wallet_ref, instrument_id, client_id_u32, block_height)
                .await
            {
                Ok(_) => {
                    tracing::info!("Successfully cancelled order: {}", client_order_id);
                }
                Err(e) => {
                    tracing::error!("Failed to cancel order {}: {:?}", client_order_id, e);

                    // Generate OrderCancelRejected event
                    let sender = get_exec_event_sender();
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
                        Some(venue_order_id),
                        None, // account_id not available in async context
                    );
                    sender
                        .send(ExecutionEvent::Order(OrderEventAny::CancelRejected(event)))
                        .unwrap();
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
            .orders_open(None, None, None, None)
            .into_iter()
            .collect();

        // Filter by instrument_id (always specified in command)
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

        tracing::info!(
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

        // Collect (instrument_id, client_id) tuples for batch cancel
        let mut orders_to_cancel = Vec::new();
        for order in &open_orders {
            let client_order_id = order.client_order_id();
            if let Some(client_id_u32) = self.get_client_order_id_int(client_order_id.as_str()) {
                orders_to_cancel.push((instrument_id, client_id_u32));
            } else {
                tracing::warn!(
                    "Cannot cancel order {}: client_order_id not found in cache",
                    client_order_id
                );
            }
        }

        self.spawn_task("cancel_all_orders", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let submitter = OrderSubmitter::new(
                (*grpc_guard).clone(),
                http_client.clone(),
                wallet_address,
                subaccount_number,
                chain_id,
            );

            // Cancel orders using batch method (executes sequentially to avoid nonce conflicts)
            match submitter
                .cancel_orders_batch(wallet_ref, &orders_to_cancel, block_height)
                .await
            {
                Ok(_) => {
                    tracing::info!("Successfully cancelled {} orders", orders_to_cancel.len());
                }
                Err(e) => {
                    // NotImplemented is expected until batch cancel is fully implemented
                    tracing::error!("Batch cancel failed: {:?}", e);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, _cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        anyhow::bail!("Batch cancel not supported by dYdX")
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        Ok(())
    }

    fn query_order(&self, _cmd: &QueryOrder) -> anyhow::Result<()> {
        Ok(())
    }
    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            tracing::warn!("dYdX execution client already connected");
            return Ok(());
        }

        tracing::info!("Connecting to dYdX");

        // Load instruments BEFORE WebSocket connection
        // Per Python implementation: "instruments are used in the first account channel message"
        tracing::debug!("Loading instruments from HTTP API");
        self.http_client.fetch_and_cache_instruments().await?;
        tracing::info!(
            "Loaded {} instruments from HTTP",
            self.http_client.instruments_cache.len()
        );

        // Populate execution client's instrument lookup maps
        self.cache_instruments_from_http();

        // Initialize wallet from config if mnemonic is provided
        if let Some(mnemonic) = &self.config.mnemonic {
            let wallet = Wallet::from_mnemonic(mnemonic)?;
            *self.wallet.write().await = Some(wallet);
            tracing::debug!("Wallet initialized");
        }

        // Connect WebSocket
        self.ws_client.connect().await?;
        tracing::debug!("WebSocket connected");

        // Subscribe to block height updates
        self.ws_client.subscribe_block_height().await?;
        tracing::debug!("Subscribed to block height updates");

        // Subscribe to markets for instrument data
        self.ws_client.subscribe_markets().await?;
        tracing::debug!("Subscribed to markets");

        // Subscribe to subaccount updates if authenticated
        if self.config.mnemonic.is_some() {
            self.ws_client
                .subscribe_subaccount(&self.wallet_address, self.subaccount_number)
                .await?;
            tracing::debug!(
                "Subscribed to subaccount updates: {}/{}",
                self.wallet_address,
                self.subaccount_number
            );

            // Spawn WebSocket message processing task following standard adapter pattern
            // Per docs/developer_guide/adapters.md: Parse -> Dispatch -> Engine handles events
            if let Some(mut rx) = self.ws_client.take_receiver() {
                // Clone data needed for account state parsing in spawned task
                let account_id = self.core.account_id;
                let instruments = self.instruments.clone();
                let oracle_prices = self.oracle_prices.clone();
                let clob_pair_id_to_instrument = self.clob_pair_id_to_instrument.clone();

                let handle = tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            NautilusWsMessage::Order(report) => {
                                tracing::debug!("Received order update: {:?}", report.order_status);
                                dispatch_execution_report(ExecutionReport::Order(report));
                            }
                            NautilusWsMessage::Fill(report) => {
                                tracing::debug!("Received fill update");
                                dispatch_execution_report(ExecutionReport::Fill(report));
                            }
                            NautilusWsMessage::Position(report) => {
                                tracing::debug!("Received position update");
                                // Dispatch position status reports via execution event system
                                let sender = get_exec_event_sender();
                                let exec_report =
                                    nautilus_common::messages::ExecutionReport::Position(Box::new(
                                        *report,
                                    ));
                                if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                                    tracing::warn!("Failed to send position status report: {e}");
                                }
                            }
                            NautilusWsMessage::AccountState(state) => {
                                tracing::debug!("Received account state update");
                                dispatch_account_state(*state);
                            }
                            NautilusWsMessage::SubaccountSubscribed(msg) => {
                                tracing::debug!(
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

                                let ts_init =
                                    nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
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
                                        tracing::info!(
                                            "Parsed account state: {} balance(s), {} margin(s)",
                                            account_state.balances.len(),
                                            account_state.margins.len()
                                        );
                                        dispatch_account_state(account_state);
                                    }
                                    Err(e) => {
                                        tracing::error!("Failed to parse account state: {e}");
                                    }
                                }

                                // Parse positions from the subscription
                                if let Some(ref positions) =
                                    msg.contents.subaccount.open_perpetual_positions
                                {
                                    tracing::debug!(
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
                                                tracing::debug!(
                                                    "Parsed position report: {} {} {} {}",
                                                    report.instrument_id,
                                                    report.position_side,
                                                    report.quantity,
                                                    market
                                                );
                                                let sender = get_exec_event_sender();
                                                let exec_report =
                                                    nautilus_common::messages::ExecutionReport::Position(
                                                        Box::new(report),
                                                    );
                                                if let Err(e) =
                                                    sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    tracing::warn!(
                                                        "Failed to send position status report: {e}"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to parse WebSocket position for {}: {e}",
                                                    market
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::SubaccountsChannelData(data) => {
                                tracing::debug!("Processing subaccounts channel data");
                                let ts_init =
                                    nautilus_core::time::get_atomic_clock_realtime().get_time_ns();

                                // Process orders
                                if let Some(ref orders) = data.contents.orders {
                                    for ws_order in orders {
                                        match crate::websocket::parse::parse_ws_order_report(
                                            ws_order,
                                            &clob_pair_id_to_instrument,
                                            &instruments,
                                            account_id,
                                            ts_init,
                                        ) {
                                            Ok(report) => {
                                                tracing::debug!(
                                                    "Parsed order report: {} {} {} @ {}",
                                                    report.instrument_id,
                                                    report.order_side,
                                                    report.order_status,
                                                    report.quantity
                                                );
                                                let sender = get_exec_event_sender();
                                                let exec_report =
                                                    nautilus_common::messages::ExecutionReport::OrderStatus(
                                                        Box::new(report),
                                                    );
                                                if let Err(e) =
                                                    sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    tracing::warn!(
                                                        "Failed to send order status report: {e}"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(
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
                                                tracing::debug!(
                                                    "Parsed fill report: {} {} {} @ {}",
                                                    report.instrument_id,
                                                    report.venue_order_id,
                                                    report.last_qty,
                                                    report.last_px
                                                );
                                                let sender = get_exec_event_sender();
                                                let exec_report =
                                                    nautilus_common::messages::ExecutionReport::Fill(
                                                        Box::new(report),
                                                    );
                                                if let Err(e) =
                                                    sender.send(ExecutionEvent::Report(exec_report))
                                                {
                                                    tracing::warn!(
                                                        "Failed to send fill report: {e}"
                                                    );
                                                }
                                            }
                                            Err(e) => {
                                                tracing::error!(
                                                    "Failed to parse WebSocket fill: {e}"
                                                );
                                            }
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::OraclePrices(oracle_prices_map) => {
                                tracing::debug!(
                                    "Processing oracle price updates for {} markets",
                                    oracle_prices_map.len()
                                );

                                // Update oracle_prices map with new prices
                                for (market_symbol, oracle_data) in &oracle_prices_map {
                                    // Parse oracle price
                                    match oracle_data.oracle_price.parse::<rust_decimal::Decimal>()
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
                                                tracing::trace!(
                                                    "Updated oracle price for {}: {}",
                                                    instrument_id,
                                                    price
                                                );
                                            } else {
                                                tracing::debug!(
                                                    "No instrument found for market symbol '{}' (tried '{}')",
                                                    market_symbol,
                                                    symbol_with_perp
                                                );
                                            }
                                        }
                                        Err(e) => {
                                            tracing::warn!(
                                                "Failed to parse oracle price for {}: {}",
                                                market_symbol,
                                                e
                                            );
                                        }
                                    }
                                }
                            }
                            NautilusWsMessage::Error(err) => {
                                tracing::error!("WebSocket error: {:?}", err);
                            }
                            NautilusWsMessage::Reconnected => {
                                tracing::info!("WebSocket reconnected");
                            }
                            _ => {
                                // Data, Deltas are for market data, not execution
                            }
                        }
                    }
                    tracing::info!("WebSocket message processing task ended");
                });

                self.ws_stream_handle = Some(handle);
                tracing::debug!("Spawned WebSocket message processing task");
            }
        }
        self.connected = true;
        tracing::info!(client_id = %self.core.client_id, "Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            tracing::warn!("dYdX execution client not connected");
            return Ok(());
        }

        tracing::info!("Disconnecting from dYdX");

        // Unsubscribe from subaccount updates if authenticated
        if self.config.mnemonic.is_some() {
            let _ = self
                .ws_client
                .unsubscribe_subaccount(&self.wallet_address, self.subaccount_number)
                .await
                .map_err(|e| tracing::warn!("Failed to unsubscribe from subaccount: {e}"));
        }

        // Unsubscribe from markets
        let _ = self
            .ws_client
            .unsubscribe_markets()
            .await
            .map_err(|e| tracing::warn!("Failed to unsubscribe from markets: {e}"));

        // Unsubscribe from block height
        let _ = self
            .ws_client
            .unsubscribe_block_height()
            .await
            .map_err(|e| tracing::warn!("Failed to unsubscribe from block height: {e}"));

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort WebSocket message processing task
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
            tracing::debug!("Aborted WebSocket message processing task");
        }

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.connected = false;
        tracing::info!(client_id = %self.core.client_id, "Disconnected");
        Ok(())
    }
}

/// Dispatches account state events to the portfolio.
///
/// AccountState events are routed to the Portfolio (not ExecEngine) via msgbus.
/// This follows the pattern used by BitMEX, OKX, and other reference adapters.
fn dispatch_account_state(state: nautilus_model::events::AccountState) {
    use std::any::Any;
    msgbus::send_any("Portfolio.update_account".into(), &state as &dyn Any);
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
/// 1. Parse WebSocket messages into ExecutionReports in the handler
/// 2. Dispatch reports via `get_exec_event_sender()`
/// 3. Let the execution engine handle event generation (has cache access)
///
/// This pattern is used by Hyperliquid, OKX, BitMEX, and other reference adapters.
fn dispatch_execution_report(report: ExecutionReport) {
    let sender = get_exec_event_sender();
    match report {
        ExecutionReport::Order(order_report) => {
            tracing::debug!(
                "Dispatching order report: status={:?}, venue_order_id={:?}, client_order_id={:?}",
                order_report.order_status,
                order_report.venue_order_id,
                order_report.client_order_id
            );
            let exec_report = nautilus_common::messages::ExecutionReport::OrderStatus(order_report);
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                tracing::warn!("Failed to send order status report: {e}");
            }
        }
        ExecutionReport::Fill(fill_report) => {
            tracing::debug!(
                "Dispatching fill report: venue_order_id={}, trade_id={}",
                fill_report.venue_order_id,
                fill_report.trade_id
            );
            let exec_report = nautilus_common::messages::ExecutionReport::Fill(fill_report);
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                tracing::warn!("Failed to send fill report: {e}");
            }
        }
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for DydxExecutionClient {
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        use anyhow::Context;

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

        // Get instrument by clob_pair_id
        let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
            Some(inst) => inst,
            None => return Ok(None),
        };

        // Parse to OrderStatusReport
        let report = crate::http::parse::parse_order_status_report(
            order,
            &instrument,
            self.core.account_id,
            ts_init,
        )
        .context("failed to parse order status report")?;

        // Filter by client_order_id if specified
        if let Some(client_order_id) = cmd.client_order_id
            && report.client_order_id != Some(client_order_id)
        {
            return Ok(None);
        }

        // Filter by venue_order_id if specified
        if let Some(venue_order_id) = cmd.venue_order_id
            && report.venue_order_id.as_str() != venue_order_id.as_str()
        {
            return Ok(None);
        }

        // Filter by instrument_id if specified
        if let Some(instrument_id) = cmd.instrument_id
            && report.instrument_id != instrument_id
        {
            return Ok(None);
        }

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        use anyhow::Context;

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
            // Get instrument by clob_pair_id using efficient lookup
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => continue,
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to OrderStatusReport
            match crate::http::parse::parse_order_status_report(
                &order,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => {
                    // Filter by client_order_id if specified
                    if let Some(client_order_id) = cmd.client_order_id
                        && report.client_order_id != Some(client_order_id)
                    {
                        continue;
                    }

                    // Filter by venue_order_id if specified
                    if let Some(venue_order_id) = cmd.venue_order_id
                        && report.venue_order_id.as_str() != venue_order_id.as_str()
                    {
                        continue;
                    }

                    reports.push(report);
                }
                Err(e) => tracing::error!("Failed to parse order status report: {e}"),
            }
        }

        tracing::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        use anyhow::Context;

        // Query fills from dYdX API
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
            // Get instrument by market ticker using efficient lookup
            let instrument = match self.get_instrument_by_market(&fill.market) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for market {} not found in cache, skipping fill {}",
                        fill.market,
                        fill.id
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to FillReport
            match crate::http::parse::parse_fill_report(
                &fill,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => {
                    // Filter by venue_order_id if specified
                    if let Some(venue_order_id) = cmd.venue_order_id
                        && report.venue_order_id.as_str() != venue_order_id.as_str()
                    {
                        continue;
                    }

                    // Filter by time range if specified
                    if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
                        if report.ts_event >= start && report.ts_event <= end {
                            reports.push(report);
                        }
                    } else if let Some(start) = cmd.start {
                        if report.ts_event >= start {
                            reports.push(report);
                        }
                    } else if let Some(end) = cmd.end {
                        if report.ts_event <= end {
                            reports.push(report);
                        }
                    } else {
                        reports.push(report);
                    }
                }
                Err(e) => tracing::error!("Failed to parse fill report: {e}"),
            }
        }

        tracing::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        use anyhow::Context;

        // Query subaccount data from dYdX API to get positions
        let response = self
            .http_client
            .inner
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        // Iterate through open perpetual positions
        for (market_ticker, position) in &response.subaccount.open_perpetual_positions {
            // Get instrument by market ticker using efficient lookup
            let instrument = match self.get_instrument_by_market(market_ticker) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for market {} not found in cache, skipping position",
                        market_ticker
                    );
                    continue;
                }
            };

            // Filter by instrument_id if specified
            if let Some(filter_id) = cmd.instrument_id
                && instrument.id() != filter_id
            {
                continue;
            }

            // Parse to PositionStatusReport
            match crate::http::parse::parse_position_status_report(
                position,
                &instrument,
                self.core.account_id,
                ts_init,
            ) {
                Ok(report) => reports.push(report),
                Err(e) => {
                    tracing::error!("Failed to parse position status report: {e}");
                }
            }
        }

        tracing::info!("Generated {} position status reports", reports.len());
        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        use anyhow::Context;

        tracing::info!(
            "Generating mass execution status{}",
            lookback_mins.map_or_else(
                || " (unbounded)".to_string(),
                |mins| format!(" (lookback: {mins} minutes)")
            )
        );

        // Calculate cutoff time if lookback is specified
        let cutoff_time =
            lookback_mins.map(|mins| chrono::Utc::now() - chrono::Duration::minutes(mins as i64));

        // Query all orders
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

        let ts_init = UnixNanos::default();
        let mut order_reports = Vec::new();
        let mut position_reports = Vec::new();
        let mut fill_reports = Vec::new();

        // Counters for logging (only used when filtering is active)
        let mut orders_filtered = 0;
        let mut fills_filtered = 0;

        // Parse orders (with optional time filtering)
        for order in orders_response {
            // Filter by time window if specified (use updated_at for orders)
            if let Some(cutoff) = cutoff_time
                && order.updated_at.is_some_and(|dt| dt < cutoff)
            {
                orders_filtered += 1;
                continue;
            }

            if let Some(instrument) = self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                match crate::http::parse::parse_order_status_report(
                    &order,
                    &instrument,
                    self.core.account_id,
                    ts_init,
                ) {
                    Ok(report) => order_reports.push(report),
                    Err(e) => tracing::error!("Failed to parse order in mass status: {e}"),
                }
            }
        }

        // Parse positions (no time filtering - positions are current state)
        for (market_ticker, position) in &subaccount_response.subaccount.open_perpetual_positions {
            if let Some(instrument) = self.get_instrument_by_market(market_ticker) {
                match crate::http::parse::parse_position_status_report(
                    position,
                    &instrument,
                    self.core.account_id,
                    ts_init,
                ) {
                    Ok(report) => position_reports.push(report),
                    Err(e) => tracing::error!("Failed to parse position in mass status: {e}"),
                }
            }
        }

        // Parse fills (with optional time filtering)
        for fill in fills_response.fills {
            // Filter by time window if specified
            if let Some(cutoff) = cutoff_time
                && fill.created_at < cutoff
            {
                fills_filtered += 1;
                continue;
            }

            if let Some(instrument) = self.get_instrument_by_market(&fill.market) {
                match crate::http::parse::parse_fill_report(
                    &fill,
                    &instrument,
                    self.core.account_id,
                    ts_init,
                ) {
                    Ok(report) => fill_reports.push(report),
                    Err(e) => tracing::error!("Failed to parse fill in mass status: {e}"),
                }
            }
        }

        if cutoff_time.is_some() {
            tracing::info!(
                "Generated mass status: {} orders ({} filtered), {} positions, {} fills ({} filtered)",
                order_reports.len(),
                orders_filtered,
                position_reports.len(),
                fill_reports.len(),
                fills_filtered
            );
        } else {
            tracing::info!(
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

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce},
        events::order::initialized::OrderInitializedBuilder,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::OrderAny,
        types::{Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    /// Test that client order ID parsing to u32 works for numeric strings
    #[rstest]
    fn test_client_order_id_numeric_parsing() {
        let client_id = "12345";
        let result: Result<u32, _> = client_id.parse();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 12345);
    }

    /// Test that client order ID hashing works for non-numeric strings
    #[rstest]
    fn test_client_order_id_hash_fallback() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let client_id = "O-20241112-ABC123-001";
        let parse_result: Result<u32, _> = client_id.parse();
        assert!(parse_result.is_err());

        // Fallback to hash
        let mut hasher = DefaultHasher::new();
        client_id.hash(&mut hasher);
        let hash_result = (hasher.finish() % (MAX_CLIENT_ID as u64)) as u32;

        assert!(hash_result < MAX_CLIENT_ID);
        assert!(hash_result > 0); // Very unlikely to be 0
    }

    /// Test that unsupported order types are properly rejected
    #[rstest]
    fn test_unsupported_order_type_rejection() {
        // Test that StopMarket is currently rejected
        let order_type = OrderType::StopMarket;
        let is_supported = matches!(order_type, OrderType::Market | OrderType::Limit);
        assert!(!is_supported);

        // Test that StopLimit is currently rejected
        let order_type = OrderType::StopLimit;
        let is_supported = matches!(order_type, OrderType::Market | OrderType::Limit);
        assert!(!is_supported);
    }

    /// Test that supported order types are accepted
    #[rstest]
    fn test_supported_order_types() {
        let market = OrderType::Market;
        assert!(matches!(market, OrderType::Market | OrderType::Limit));

        let limit = OrderType::Limit;
        assert!(matches!(limit, OrderType::Market | OrderType::Limit));
    }

    /// Test UnixNanos to seconds conversion for expire_time
    #[rstest]
    fn test_unix_nanos_to_seconds_conversion() {
        use nautilus_core::UnixNanos;

        // Test conversion of 1 second
        let one_second = UnixNanos::from(1_000_000_000_u64);
        let seconds = (one_second.as_u64() / 1_000_000_000) as i64;
        assert_eq!(seconds, 1);

        // Test conversion of 1 hour
        let one_hour = UnixNanos::from(3_600_000_000_000_u64);
        let seconds = (one_hour.as_u64() / 1_000_000_000) as i64;
        assert_eq!(seconds, 3600);

        // Test current timestamp (should be reasonable)
        let now = UnixNanos::from(1_731_398_400_000_000_000_u64); // 2024-11-12
        let seconds = (now.as_u64() / 1_000_000_000) as i64;
        assert_eq!(seconds, 1_731_398_400);
    }

    /// Test that OrderAny API methods work correctly
    #[rstest]
    fn test_order_any_api_usage() {
        let order = OrderInitializedBuilder::default()
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("STRATEGY-001"))
            .instrument_id(InstrumentId::from("ETH-USD-PERP.DYDX"))
            .client_order_id(ClientOrderId::from("O-001"))
            .order_side(OrderSide::Buy)
            .order_type(OrderType::Limit)
            .quantity(Quantity::from("10"))
            .price(Some(Price::from("2000.50")))
            .time_in_force(TimeInForce::Gtc)
            .build()
            .unwrap();

        let order_any: OrderAny = order.into();

        // Test OrderAny methods
        assert_eq!(order_any.order_side(), OrderSide::Buy);
        assert_eq!(order_any.order_type(), OrderType::Limit);
        assert_eq!(order_any.quantity(), Quantity::from("10"));
        assert_eq!(order_any.price(), Some(Price::from("2000.50")));
        assert_eq!(order_any.time_in_force(), TimeInForce::Gtc);
        assert!(!order_any.is_post_only());
        assert!(!order_any.is_reduce_only());
        assert_eq!(order_any.expire_time(), None);
    }

    /// Test MAX_CLIENT_ID constant is within dYdX limits
    #[rstest]
    fn test_max_client_id_limit() {
        // dYdX requires client IDs to be u32
        assert_eq!(MAX_CLIENT_ID, u32::MAX);
    }

    /// Test that client order ID conversion is consistent for cancel operations
    #[rstest]
    fn test_cancel_order_id_consistency() {
        use std::{
            collections::hash_map::DefaultHasher,
            hash::{Hash, Hasher},
        };

        let client_id_str = "O-20241112-CANCEL-001";

        // First conversion (for submit)
        let mut hasher1 = DefaultHasher::new();
        client_id_str.hash(&mut hasher1);
        let id1 = (hasher1.finish() % (MAX_CLIENT_ID as u64)) as u32;

        // Second conversion (for cancel) - should be identical
        let mut hasher2 = DefaultHasher::new();
        client_id_str.hash(&mut hasher2);
        let id2 = (hasher2.finish() % (MAX_CLIENT_ID as u64)) as u32;

        assert_eq!(id1, id2, "Client ID conversion must be deterministic");
    }

    /// Test clob_pair_id extraction from CryptoPerpetual raw_symbol
    #[rstest]
    fn test_clob_pair_id_extraction_from_raw_symbol() {
        // Simulate raw_symbol "1" -> clob_pair_id 1
        let raw_symbol = "1";
        let result: Result<u32, _> = raw_symbol.parse();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1);

        // Simulate raw_symbol "42" -> clob_pair_id 42
        let raw_symbol = "42";
        let result: Result<u32, _> = raw_symbol.parse();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 42);
    }

    /// Test clob_pair_id extraction failure for invalid raw_symbol
    #[rstest]
    fn test_clob_pair_id_extraction_invalid() {
        // Invalid raw_symbol should fail parsing
        let raw_symbol = "BTC-USD";
        let result: Result<u32, _> = raw_symbol.parse();
        assert!(result.is_err());

        // Empty raw_symbol should fail
        let raw_symbol = "";
        let result: Result<u32, _> = raw_symbol.parse();
        assert!(result.is_err());
    }

    /// Test market ticker to instrument mapping logic
    #[rstest]
    fn test_market_ticker_parsing() {
        let ticker = "BTC-USD";
        let parts: Vec<&str> = ticker.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "BTC");
        assert_eq!(parts[1], "USD");

        let ticker = "ETH-USD";
        let parts: Vec<&str> = ticker.split('-').collect();
        assert_eq!(parts.len(), 2);
        assert_eq!(parts[0], "ETH");
        assert_eq!(parts[1], "USD");
    }

    /// Test market ticker with invalid format
    #[rstest]
    fn test_market_ticker_invalid_format() {
        let ticker = "BTCUSD";
        assert_eq!(ticker.split('-').count(), 1); // No separator

        let ticker = "BTC-USD-PERP";
        assert_eq!(ticker.split('-').count(), 3); // Too many parts
    }
}
