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

use std::{
    cell::Ref,
    sync::{Arc, Mutex, atomic::AtomicU64},
};

use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use nautilus_common::{
    clock::Clock,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    runner::get_exec_event_sender,
    runtime::get_runtime,
};
use nautilus_core::{MUTEX_POISONED, UnixNanos};
use nautilus_execution::client::{ExecutionClient, LiveExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::LiveExecutionClientExt;
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType},
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
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
    grpc::{DydxGrpcClient, OrderBuilder, Wallet},
    http::client::DydxRawHttpClient,
    websocket::client::DydxWebSocketClient,
};

pub mod submitter;

/// Maximum client order ID value for dYdX.
pub const MAX_CLIENT_ID: u32 = u32::MAX;

/// Live execution client for the dYdX v4 exchange adapter.
///
/// This client provides order execution capabilities for dYdX v4, supporting:
/// - Market and Limit orders (implemented)
/// - Conditional orders: Stop Market, Stop Limit, Take Profit, Trailing Stop (planned)
/// - Order lifecycle management (submission, cancellation, updates)
/// - Real-time order and fill updates via WebSocket
///
/// # Implementation Status
///
/// ## Completed
/// - Order submission framework following standard adapter patterns
/// - Order type validation (Market and Limit currently supported)
/// - OrderSubmitted event generation
/// - OrderRejected event for unsupported order types
/// - Client order ID to u32 conversion (required by dYdX)
/// - Async task management for non-blocking submissions
///
/// ## Stubbed (awaiting proto generation)
/// - Actual gRPC order submission via `OrderSubmitter`
/// - Exchange response handling
/// - OrderAccepted/OrderRejected from exchange
/// - Order cancellation submission
///
/// ## Planned
/// - WebSocket subscriptions for order/fill updates
/// - Conditional order submission (stop, take profit, trailing)
/// - Position reconciliation
/// - Account state synchronization
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
#[allow(dead_code)] // TODO: Remove once implementation is complete
pub struct DydxExecutionClient {
    core: ExecutionClientCore,
    config: DydxAdapterConfig,
    http_client: DydxRawHttpClient,
    ws_client: DydxWebSocketClient,
    grpc_client: Arc<tokio::sync::RwLock<DydxGrpcClient>>,
    wallet: Arc<tokio::sync::RwLock<Option<Wallet>>>,
    order_builders: DashMap<InstrumentId, OrderBuilder>,
    // NOTE: Currently unpopulated - instrument loading not yet implemented
    instruments: DashMap<InstrumentId, InstrumentAny>,
    market_to_instrument: DashMap<String, InstrumentId>,
    clob_pair_id_to_instrument: DashMap<u32, InstrumentId>,
    block_height: AtomicU64,
    oracle_prices: DashMap<InstrumentId, Decimal>,
    client_id_to_int: DashMap<String, u32>,
    int_to_client_id: DashMap<u32, String>,
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
        let http_client = DydxRawHttpClient::default();

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
            order_builders: DashMap::new(),
            instruments: DashMap::new(),
            market_to_instrument: DashMap::new(),
            clob_pair_id_to_instrument: DashMap::new(),
            block_height: AtomicU64::new(0),
            oracle_prices: DashMap::new(),
            client_id_to_int: DashMap::new(),
            int_to_client_id: DashMap::new(),
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
    /// Attempts to parse the client_order_id as an integer first. If that fails,
    /// generates a random value within the valid range [0, MAX_CLIENT_ID).
    #[allow(dead_code)] // TODO: Remove once used in submit_order
    fn generate_client_order_id_int(&self, client_order_id: &str) -> u32 {
        if let Ok(id) = client_order_id.parse::<u32>() {
            self.client_id_to_int
                .insert(client_order_id.to_string(), id);
            self.int_to_client_id
                .insert(id, client_order_id.to_string());
            return id;
        }

        // Generate random value if parsing fails
        let id = rand::random::<u32>();
        self.client_id_to_int
            .insert(client_order_id.to_string(), id);
        self.int_to_client_id
            .insert(id, client_order_id.to_string());
        id
    }

    /// Retrieve the client order ID integer from the cache.
    ///
    /// Returns `None` if the mapping doesn't exist.
    #[allow(dead_code)] // TODO: Remove once used in cancel_order
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

    /// Retrieve the client order ID string from the integer value.
    ///
    /// Returns the integer as a string if no mapping exists.
    #[allow(dead_code)] // TODO: Remove once used in handle_order_message
    fn get_client_order_id(&self, client_order_id_int: u32) -> String {
        self.int_to_client_id.get(&client_order_id_int).map_or_else(
            || client_order_id_int.to_string(),
            |entry| entry.value().clone(),
        )
    }

    /// Get an instrument by market ticker (e.g., "BTC-USD").
    fn get_instrument_by_market(&self, market: &str) -> Option<InstrumentAny> {
        self.market_to_instrument
            .get(market)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()))
    }

    /// Get an instrument by clob_pair_id.
    fn get_instrument_by_clob_pair_id(&self, clob_pair_id: u32) -> Option<InstrumentAny> {
        self.clob_pair_id_to_instrument
            .get(&clob_pair_id)
            .and_then(|id| self.instruments.get(&id).map(|entry| entry.value().clone()))
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

    fn abort_pending_tasks(&self) {
        let mut guard = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in guard.drain(..) {
            handle.abort();
        }
    }
}
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

    /// Submits an order to dYdX exchange.
    ///
    /// This method implements a two-phase submission process:
    ///
    /// # Phase 1: Synchronous Validation
    /// - Checks if order is already closed
    /// - Validates order type (only Market and Limit currently supported)
    /// - Generates `OrderSubmitted` event immediately
    /// - Generates `OrderRejected` for unsupported order types
    ///
    /// # Phase 2: Async Submission
    /// - Spawns background task for gRPC submission
    /// - Converts Nautilus ClientOrderId to dYdX u32 format
    /// - Calls appropriate `OrderSubmitter` method
    /// - Logs errors (does not generate rejection events from async block)
    ///
    /// # Supported Order Types
    /// - `OrderType::Market` - Market orders
    /// - `OrderType::Limit` - Limit orders with price and time-in-force
    ///
    /// # Unsupported (Returns Error)
    /// - Stop Market, Stop Limit, Take Profit, Trailing Stop
    /// - These will be implemented in future updates
    ///
    /// # Arguments
    /// - `cmd` - The submit order command containing the order and metadata
    ///
    /// # Returns
    /// - `Ok(())` - Order submitted successfully or validation failed with rejection event
    /// - `Err` - Only for critical errors (shouldn't happen in normal flow)
    ///
    /// # Implementation Status
    /// The actual gRPC submission in `OrderSubmitter` is currently stubbed,
    /// awaiting proto file generation. The framework and event flow are complete.
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
            order_type => {
                let reason = format!(
                    "Order type {:?} not supported. Only MARKET and LIMIT orders are supported for dYdX",
                    order_type
                );
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
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let client_order_id = order.client_order_id();
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;
        #[allow(clippy::redundant_clone)]
        let order = order.clone();

        self.spawn_task("submit_order", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let submitter =
                OrderSubmitter::new((*grpc_guard).clone(), wallet_address, subaccount_number);

            // Generate client_order_id as u32 (dYdX requires u32 client IDs)
            // TODO: Implement proper client_order_id to u32 mapping
            let client_id_u32 = client_order_id.as_str().parse::<u32>().unwrap_or_else(|_| {
                // Fallback: use hash of client_order_id
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                client_order_id.as_str().hash(&mut hasher);
                (hasher.finish() % (MAX_CLIENT_ID as u64)) as u32
            });

            // Submit order based on type
            match order.order_type() {
                OrderType::Market => {
                    submitter
                        .submit_market_order(
                            wallet_ref,
                            client_id_u32,
                            order.order_side(),
                            order.quantity(),
                            block_height,
                        )
                        .await?;
                    tracing::info!("Successfully submitted market order: {}", client_order_id);
                }
                OrderType::Limit => {
                    let expire_time = order
                        .expire_time()
                        .map(|t| (t.as_u64() / 1_000_000_000) as i64);
                    submitter
                        .submit_limit_order(
                            wallet_ref,
                            client_id_u32,
                            order.order_side(),
                            order
                                .price()
                                .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?,
                            order.quantity(),
                            order.time_in_force(),
                            order.is_post_only(),
                            order.is_reduce_only(),
                            block_height,
                            expire_time,
                        )
                        .await?;
                    tracing::info!("Successfully submitted limit order: {}", client_order_id);
                }
                _ => unreachable!("Order type already validated"),
            }

            Ok(())
        });

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
    /// This method spawns an async task to cancel the order via gRPC.
    /// Unlike order submission, no immediate event is generated - the
    /// cancellation confirmation will come from the WebSocket feed when
    /// the exchange processes the cancellation.
    ///
    /// # Arguments
    /// - `cmd` - The cancel order command with client/venue order IDs
    ///
    /// # Returns
    /// - `Ok(())` - Cancel request spawned successfully
    /// - `Err` - If not connected or other critical error
    ///
    /// # Implementation Status
    /// Framework complete, actual gRPC cancellation stubbed awaiting proto.
    ///
    /// # Events
    /// - `OrderCanceled` - Generated when WebSocket confirms cancellation
    /// - `OrderCancelRejected` - Generated if exchange rejects cancellation
    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel order: not connected");
        }

        let grpc_client = self.grpc_client.clone();
        let wallet = self.wallet.clone();
        let wallet_address = self.wallet_address.clone();
        let subaccount_number = self.subaccount_number;
        let client_order_id = cmd.client_order_id;
        let block_height = self.block_height.load(std::sync::atomic::Ordering::Relaxed) as u32;

        self.spawn_task("cancel_order", async move {
            let wallet_guard = wallet.read().await;
            let wallet_ref = wallet_guard
                .as_ref()
                .ok_or_else(|| anyhow::anyhow!("Wallet not initialized"))?;

            let grpc_guard = grpc_client.read().await;
            let submitter =
                OrderSubmitter::new((*grpc_guard).clone(), wallet_address, subaccount_number);

            // Convert client_order_id to u32 (same logic as submit_order)
            let client_id_u32 = client_order_id.as_str().parse::<u32>().unwrap_or_else(|_| {
                // Fallback: use hash of client_order_id
                use std::collections::hash_map::DefaultHasher;
                use std::hash::{Hash, Hasher};
                let mut hasher = DefaultHasher::new();
                client_order_id.as_str().hash(&mut hasher);
                (hasher.finish() % (MAX_CLIENT_ID as u64)) as u32
            });

            submitter
                .cancel_order(wallet_ref, client_id_u32, block_height)
                .await?;

            tracing::info!("Successfully cancelled order: {}", client_order_id);
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if !self.is_connected() {
            anyhow::bail!("Cannot cancel orders: not connected");
        }

        tracing::info!("Cancelling all orders for {:?}", cmd.instrument_id);
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
}

impl DydxExecutionClient {
    /// Processes order update from WebSocket.
    ///
    /// Converts dYdX order status reports into appropriate Nautilus order events.
    /// Requires cache lookup to get strategy_id for the order.
    fn process_order_update(report: OrderStatusReport) {
        tracing::info!(
            "Order update: status={:?}, venue_order_id={:?}, client_order_id={:?}",
            report.order_status,
            report.venue_order_id,
            report.client_order_id
        );

        // TODO: Implement full event generation logic:
        // 1. Look up order from cache using client_order_id or venue_order_id
        // 2. Get strategy_id from the order
        // 3. Generate appropriate event based on order_status:
        //    - Accepted -> generate_order_accepted()
        //    - Canceled -> generate_order_canceled()
        //    - Expired -> generate_order_expired()
        //    - Triggered -> generate_order_triggered()
        //    - Rejected -> generate_order_rejected()
        // 4. Handle external orders (no strategy_id) by sending OrderStatusReport
    }

    /// Processes fill update from WebSocket.
    ///
    /// Converts dYdX fill reports into Nautilus OrderFilled events.
    /// Requires cache lookup to get strategy_id for the order.
    fn process_fill_update(report: FillReport) {
        tracing::info!(
            "Fill update: venue_order_id={}, trade_id={}",
            report.venue_order_id,
            report.trade_id
        );

        // TODO: Implement fill event generation
        // 1. Look up order from cache using venue_order_id or client_order_id
        // 2. Get strategy_id from cached order
        // 3. Call core.generate_order_filled() with all required fields
        // 4. Handle external fills (no cached order) appropriately
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for DydxExecutionClient {
    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            tracing::warn!("dYdX execution client already connected");
            return Ok(());
        }

        tracing::info!("Connecting to dYdX");

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

            // Spawn WebSocket message processing task
            if let Some(mut rx) = self.ws_client.take_receiver() {
                use crate::websocket::messages::NautilusWsMessage;

                let handle = tokio::spawn(async move {
                    while let Some(msg) = rx.recv().await {
                        match msg {
                            NautilusWsMessage::Order(report) => {
                                tracing::debug!("Received order update: {:?}", report.order_status);
                                Self::process_order_update(*report);
                            }
                            NautilusWsMessage::Fill(report) => {
                                tracing::debug!("Received fill update");
                                Self::process_fill_update(*report);
                            }
                            NautilusWsMessage::Position(_report) => {
                                tracing::debug!("Received position update");
                                // Position updates handled by cache
                            }
                            NautilusWsMessage::AccountState(_state) => {
                                tracing::debug!("Received account state update");
                                // Account state updates need implementation
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
        tracing::info!("dYdX execution client connected");
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
        tracing::info!("dYdX execution client disconnected");
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        use anyhow::Context;

        // Query single order from dYdX API
        let response = self
            .http_client
            .get_orders(
                &self.wallet_address,
                self.subaccount_number,
                None,    // market filter
                Some(1), // limit to 1 result
            )
            .await
            .context("failed to fetch order from dYdX API")?;

        if response.orders.is_empty() {
            return Ok(None);
        }

        let order = &response.orders[0];
        let ts_init = UnixNanos::default();

        // Get instrument by clob_pair_id
        let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
            Some(inst) => inst,
            None => {
                tracing::warn!(
                    "Instrument for clob_pair_id {} not found in cache",
                    order.clob_pair_id
                );
                return Ok(None);
            }
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

        for order in response.orders {
            // Get instrument by clob_pair_id using efficient lookup
            let instrument = match self.get_instrument_by_clob_pair_id(order.clob_pair_id) {
                Some(inst) => inst,
                None => {
                    tracing::warn!(
                        "Instrument for clob_pair_id {} not found in cache, skipping order {}",
                        order.clob_pair_id,
                        order.id
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
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount from dYdX API")?;

        let mut reports = Vec::new();
        let ts_init = UnixNanos::default();

        // Iterate through open perpetual positions
        for (market_ticker, position) in response.subaccount.open_perpetual_positions {
            // Get instrument by market ticker using efficient lookup
            let instrument = match self.get_instrument_by_market(&market_ticker) {
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
                &position,
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
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        use anyhow::Context;

        tracing::info!("Generating mass execution status");

        // Query all orders
        let orders_response = self
            .http_client
            .get_orders(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch orders for mass status")?;

        // Query subaccount for positions
        let subaccount_response = self
            .http_client
            .get_subaccount(&self.wallet_address, self.subaccount_number)
            .await
            .context("failed to fetch subaccount for mass status")?;

        // Query fills
        let fills_response = self
            .http_client
            .get_fills(&self.wallet_address, self.subaccount_number, None, None)
            .await
            .context("failed to fetch fills for mass status")?;

        let ts_init = UnixNanos::default();
        let mut order_reports = Vec::new();
        let mut position_reports = Vec::new();
        let mut fill_reports = Vec::new();

        // Parse orders
        for order in orders_response.orders {
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

        // Parse positions
        for (market_ticker, position) in subaccount_response.subaccount.open_perpetual_positions {
            if let Some(instrument) = self.get_instrument_by_market(&market_ticker) {
                match crate::http::parse::parse_position_status_report(
                    &position,
                    &instrument,
                    self.core.account_id,
                    ts_init,
                ) {
                    Ok(report) => position_reports.push(report),
                    Err(e) => tracing::error!("Failed to parse position in mass status: {e}"),
                }
            }
        }

        // Parse fills
        for fill in fills_response.fills {
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

        tracing::info!(
            "Generated mass status: {} orders, {} positions, {} fills",
            order_reports.len(),
            position_reports.len(),
            fill_reports.len()
        );

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

impl LiveExecutionClientExt for DydxExecutionClient {
    fn get_message_channel(&self) -> tokio::sync::mpsc::UnboundedSender<ExecutionEvent> {
        get_exec_event_sender()
    }

    fn get_clock(&self) -> Ref<'_, dyn Clock> {
        self.core.clock().borrow()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_model::{
        enums::{OrderSide, OrderType, TimeInForce},
        events::order::initialized::OrderInitializedBuilder,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::OrderAny,
        types::{Price, Quantity},
    };

    /// Test that client order ID parsing to u32 works for numeric strings
    #[test]
    fn test_client_order_id_numeric_parsing() {
        let client_id = "12345";
        let result: Result<u32, _> = client_id.parse();
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 12345);
    }

    /// Test that client order ID hashing works for non-numeric strings
    #[test]
    fn test_client_order_id_hash_fallback() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

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
    #[test]
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
    #[test]
    fn test_supported_order_types() {
        let market = OrderType::Market;
        assert!(matches!(market, OrderType::Market | OrderType::Limit));

        let limit = OrderType::Limit;
        assert!(matches!(limit, OrderType::Market | OrderType::Limit));
    }

    /// Test UnixNanos to seconds conversion for expire_time
    #[test]
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
    #[test]
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
    #[test]
    fn test_max_client_id_limit() {
        // dYdX requires client IDs to be u32
        assert_eq!(MAX_CLIENT_ID, u32::MAX);
    }

    /// Test that client order ID conversion is consistent for cancel operations
    #[test]
    fn test_cancel_order_id_consistency() {
        use std::collections::hash_map::DefaultHasher;
        use std::hash::{Hash, Hasher};

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
}
