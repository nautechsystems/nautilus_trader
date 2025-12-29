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

//! Live execution client implementation for the Hyperliquid adapter.

use std::{str::FromStr, sync::Mutex};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent, ExecutionReport as NautilusExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GeneratePositionReports, ModifyOrder, QueryAccount,
            QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{MUTEX_POISONED, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_live::execution::client::LiveExecutionClient;
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType},
    identifiers::{AccountId, ClientId, Venue},
    orders::{Order, any::OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use serde_json;
use tokio::task::JoinHandle;

use crate::{
    common::{
        HyperliquidProductType,
        consts::HYPERLIQUID_VENUE,
        credential::Secrets,
        parse::{
            client_order_id_to_cancel_request, extract_error_message, is_response_successful,
            order_any_to_hyperliquid_request, orders_to_hyperliquid_requests,
        },
    },
    config::HyperliquidExecClientConfig,
    http::{client::HyperliquidHttpClient, models::ClearinghouseState, query::ExchangeAction},
    websocket::{ExecutionReport, NautilusWsMessage, client::HyperliquidWebSocketClient},
};

#[derive(Debug)]
pub struct HyperliquidExecutionClient {
    core: ExecutionClientCore,
    config: HyperliquidExecClientConfig,
    http_client: HyperliquidHttpClient,
    ws_client: HyperliquidWebSocketClient,
    started: bool,
    connected: bool,
    instruments_initialized: bool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
}

impl HyperliquidExecutionClient {
    /// Returns a reference to the configuration.
    pub fn config(&self) -> &HyperliquidExecClientConfig {
        &self.config
    }

    /// Validates order before submission to catch issues early.
    ///
    /// # Errors
    ///
    /// Returns an error if the order cannot be submitted to Hyperliquid.
    ///
    /// # Supported Order Types
    ///
    /// - `Market`: Standard market orders
    /// - `Limit`: Limit orders with GTC/IOC/ALO time-in-force
    /// - `StopMarket`: Stop loss / protective stop with market execution
    /// - `StopLimit`: Stop loss / protective stop with limit price
    /// - `MarketIfTouched`: Profit taking / entry order with market execution
    /// - `LimitIfTouched`: Profit taking / entry order with limit price
    fn validate_order_submission(&self, order: &OrderAny) -> anyhow::Result<()> {
        // Check if instrument symbol is supported
        // Hyperliquid instruments: {base}-USD-PERP or {base}-{quote}-SPOT
        let instrument_id = order.instrument_id();
        let symbol = instrument_id.symbol.as_str();
        if !symbol.ends_with("-PERP") && !symbol.ends_with("-SPOT") {
            anyhow::bail!(
                "Unsupported instrument symbol format for Hyperliquid: {symbol} (expected -PERP or -SPOT suffix)"
            );
        }

        // Check if order type is supported
        match order.order_type() {
            OrderType::Market
            | OrderType::Limit
            | OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched => {}
            _ => anyhow::bail!(
                "Unsupported order type for Hyperliquid: {:?}",
                order.order_type()
            ),
        }

        // Check if conditional orders have trigger price
        if matches!(
            order.order_type(),
            OrderType::StopMarket
                | OrderType::StopLimit
                | OrderType::MarketIfTouched
                | OrderType::LimitIfTouched
        ) && order.trigger_price().is_none()
        {
            anyhow::bail!(
                "Conditional orders require a trigger price for Hyperliquid: {:?}",
                order.order_type()
            );
        }

        // Check if limit-based orders have price
        if matches!(
            order.order_type(),
            OrderType::Limit | OrderType::StopLimit | OrderType::LimitIfTouched
        ) && order.price().is_none()
        {
            anyhow::bail!(
                "Limit orders require a limit price for Hyperliquid: {:?}",
                order.order_type()
            );
        }

        Ok(())
    }

    /// Creates a new [`HyperliquidExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if either the HTTP or WebSocket client fail to construct.
    pub fn new(
        core: ExecutionClientCore,
        config: HyperliquidExecClientConfig,
    ) -> anyhow::Result<Self> {
        if !config.has_credentials() {
            anyhow::bail!("Hyperliquid execution client requires private key");
        }

        let secrets = Secrets::from_json(&format!(
            r#"{{"privateKey": "{}", "isTestnet": {}}}"#,
            config.private_key, config.is_testnet
        ))
        .context("failed to create secrets from private key")?;

        let http_client = HyperliquidHttpClient::with_credentials(
            &secrets,
            Some(config.http_timeout_secs),
            config.http_proxy_url.clone(),
        )
        .context("failed to create Hyperliquid HTTP client")?;

        // Create WebSocket client (will connect when needed)
        // Note: For execution WebSocket (private account messages), product type is less critical
        // since messages are account-scoped. Defaulting to Perp.
        let ws_client = HyperliquidWebSocketClient::new(
            None,
            config.is_testnet,
            HyperliquidProductType::Perp,
            Some(core.account_id),
        );

        Ok(Self {
            core,
            config,
            http_client,
            ws_client,
            started: false,
            connected: false,
            instruments_initialized: false,
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
        })
    }

    async fn ensure_instruments_initialized_async(&mut self) -> anyhow::Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Hyperliquid instruments")?;

        if instruments.is_empty() {
            tracing::warn!(
                "Instrument bootstrap yielded no instruments; WebSocket submissions may fail"
            );
        } else {
            tracing::info!("Initialized {} instruments", instruments.len());

            for instrument in &instruments {
                self.http_client.cache_instrument(instrument.clone());
            }
        }

        self.instruments_initialized = true;
        Ok(())
    }

    fn ensure_instruments_initialized(&mut self) -> anyhow::Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let runtime = get_runtime();
        runtime.block_on(self.ensure_instruments_initialized_async())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        // Get account information from Hyperliquid using the user address
        // We need to derive the user address from the private key in the config
        let user_address = self.get_user_address()?;

        // Use vault address if configured, otherwise use user address
        let account_address = self.config.vault_address.as_ref().unwrap_or(&user_address);

        // Query clearinghouseState endpoint to get balances and margin info
        let clearinghouse_state = self
            .http_client
            .info_clearinghouse_state(account_address)
            .await
            .context("failed to fetch clearinghouse state")?;

        // Deserialize the response
        let state: ClearinghouseState = serde_json::from_value(clearinghouse_state)
            .context("failed to deserialize clearinghouse state")?;

        tracing::debug!(
            "Received clearinghouse state: cross_margin_summary={:?}, asset_positions={}",
            state.cross_margin_summary,
            state.asset_positions.len()
        );

        // Parse balances and margins from cross margin summary
        if let Some(ref cross_margin_summary) = state.cross_margin_summary {
            let (balances, margins) =
                crate::common::parse::parse_account_balances_and_margins(cross_margin_summary)
                    .context("failed to parse account balances and margins")?;

            let ts_event = if let Some(time_ms) = state.time {
                nautilus_core::UnixNanos::from(time_ms * 1_000_000)
            } else {
                nautilus_core::time::get_atomic_clock_realtime().get_time_ns()
            };

            // Generate account state event
            self.core.generate_account_state(
                balances, margins, true, // reported
                ts_event,
            )?;

            tracing::info!("Account state updated successfully");
        } else {
            tracing::warn!("No cross margin summary in clearinghouse state");
        }

        Ok(())
    }

    fn get_user_address(&self) -> anyhow::Result<String> {
        let address = self
            .http_client
            .get_user_address()
            .context("failed to get user address from HTTP client")?;

        Ok(address)
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                tracing::warn!("{description} failed: {e:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for HyperliquidExecutionClient {
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
        *HYPERLIQUID_VENUE
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
            return Ok(());
        }

        tracing::info!(
            client_id = %self.core.client_id,
            account_id = %self.core.account_id,
            is_testnet = self.config.is_testnet,
            vault_address = ?self.config.vault_address,
            http_proxy_url = ?self.config.http_proxy_url,
            ws_proxy_url = ?self.config.ws_proxy_url,
            "Starting Hyperliquid execution client"
        );

        // Ensure instruments are initialized
        self.ensure_instruments_initialized()?;

        // Initialize account state
        if let Err(e) = self.update_account_state() {
            tracing::warn!("Failed to initialize account state: {e}");
        }

        self.connected = true;
        self.started = true;

        // Start WebSocket stream for execution updates
        if let Err(e) = get_runtime().block_on(self.start_ws_stream()) {
            tracing::warn!("Failed to start WebSocket stream: {e}");
        }

        tracing::info!("Hyperliquid execution client started");
        Ok(())
    }
    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        tracing::info!("Stopping Hyperliquid execution client");

        // Stop WebSocket stream
        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        // Abort any pending tasks
        self.abort_pending_tasks();

        // Disconnect WebSocket
        if self.connected {
            let runtime = get_runtime();
            runtime.block_on(async {
                if let Err(e) = self.ws_client.disconnect().await {
                    tracing::warn!("Error disconnecting WebSocket client: {e}");
                }
            });
        }

        self.connected = false;
        self.started = false;

        tracing::info!("Hyperliquid execution client stopped");
        Ok(())
    }

    fn submit_order(&self, command: &SubmitOrder) -> anyhow::Result<()> {
        let order = &command.order;

        if order.is_closed() {
            tracing::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Err(e) = self.validate_order_submission(order) {
            self.core.generate_order_rejected(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                &format!("validation-error: {e}"),
                command.ts_init,
                false,
            );
            return Err(e);
        }

        self.core.generate_order_submitted(
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            command.ts_init,
        );

        let http_client = self.http_client.clone();
        let order_clone = order.clone();

        self.spawn_task("submit_order", async move {
            match order_any_to_hyperliquid_request(&order_clone) {
                Ok(hyperliquid_order) => {
                    // Create exchange action for order placement with typed struct
                    let action = ExchangeAction::order(vec![hyperliquid_order]);

                    match http_client.post_action(&action).await {
                        Ok(response) => {
                            if is_response_successful(&response) {
                                tracing::info!("Order submitted successfully: {:?}", response);
                                // Order acceptance/rejection events will be generated from WebSocket updates
                                // which provide the venue_order_id and definitive status
                            } else {
                                let error_msg = extract_error_message(&response);
                                tracing::warn!(
                                    "Order submission rejected by exchange: {}",
                                    error_msg
                                );
                                // Order rejection event will be generated from WebSocket updates
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Order submission HTTP request failed: {e}");
                            // WebSocket reconnection and order reconciliation will handle recovery
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to convert order to Hyperliquid format: {e}");
                    // This indicates a client-side bug or unsupported order configuration
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, command: &SubmitOrderList) -> anyhow::Result<()> {
        tracing::debug!(
            "Submitting order list with {} orders",
            command.order_list.orders.len()
        );

        let http_client = self.http_client.clone();
        let orders: Vec<OrderAny> = command.order_list.orders.clone();

        // Generate submitted events for all orders
        for order in &orders {
            self.core.generate_order_submitted(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                command.ts_init,
            );
        }

        self.spawn_task("submit_order_list", async move {
            // Convert all orders to Hyperliquid format
            let order_refs: Vec<&OrderAny> = orders.iter().collect();
            match orders_to_hyperliquid_requests(&order_refs) {
                Ok(hyperliquid_orders) => {
                    // Create exchange action for order placement with typed struct
                    let action = ExchangeAction::order(hyperliquid_orders);
                    match http_client.post_action(&action).await {
                        Ok(response) => {
                            if is_response_successful(&response) {
                                tracing::info!("Order list submitted successfully: {:?}", response);
                                // Order acceptance/rejection events will be generated from WebSocket updates
                            } else {
                                let error_msg = extract_error_message(&response);
                                tracing::warn!(
                                    "Order list submission rejected by exchange: {}",
                                    error_msg
                                );
                                // Individual order rejection events will be generated from WebSocket updates
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Order list submission HTTP request failed: {e}");
                            // WebSocket reconciliation will handle recovery
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!("Failed to convert order list to Hyperliquid format: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, command: &ModifyOrder) -> anyhow::Result<()> {
        tracing::debug!("Modifying order: {:?}", command);

        // Parse venue_order_id as u64
        let oid: u64 = match command.venue_order_id.as_str().parse() {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!(
                    "Failed to parse venue_order_id '{}' as u64: {}",
                    command.venue_order_id,
                    e
                );
                return Ok(());
            }
        };

        let http_client = self.http_client.clone();
        let price = command.price;
        let quantity = command.quantity;
        let symbol = command.instrument_id.symbol.inner();

        self.spawn_task("modify_order", async move {
            use crate::{
                common::parse::extract_asset_id_from_symbol,
                http::models::HyperliquidExecModifyOrderRequest,
            };

            // Extract asset ID from instrument symbol
            let asset = match extract_asset_id_from_symbol(&symbol) {
                Ok(asset) => asset,
                Err(e) => {
                    tracing::warn!("Failed to extract asset ID from symbol {}: {}", symbol, e);
                    return Ok(());
                }
            };

            // Build typed modify request with new price and/or quantity
            let modify_request = HyperliquidExecModifyOrderRequest {
                asset,
                oid,
                price: price.map(|p| (*p).into()),
                size: quantity.map(|q| (*q).into()),
                reduce_only: None,
                kind: None,
            };

            let action = ExchangeAction::modify(oid, modify_request);

            match http_client.post_action(&action).await {
                Ok(response) => {
                    if is_response_successful(&response) {
                        tracing::info!("Order modified successfully: {:?}", response);
                        // Order update events will be generated from WebSocket updates
                    } else {
                        let error_msg = extract_error_message(&response);
                        tracing::warn!("Order modification rejected by exchange: {}", error_msg);
                        // Order modify rejected events will be generated from WebSocket updates
                    }
                }
                Err(e) => {
                    tracing::warn!("Order modification HTTP request failed: {e}");
                    // WebSocket reconciliation will handle recovery
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, command: &CancelOrder) -> anyhow::Result<()> {
        tracing::debug!("Cancelling order: {:?}", command);

        let http_client = self.http_client.clone();
        let client_order_id = command.client_order_id.inner();
        let symbol = command.instrument_id.symbol.inner();

        self.spawn_task("cancel_order", async move {
            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(cancel_request) => {
                    // Create exchange action for order cancellation with typed struct
                    let action = ExchangeAction::cancel_by_cloid(vec![cancel_request]);
                    match http_client.post_action(&action).await {
                        Ok(response) => {
                            if is_response_successful(&response) {
                                tracing::info!("Order cancelled successfully: {:?}", response);
                                // Order cancelled events will be generated from WebSocket updates
                                // which provide definitive confirmation and venue_order_id
                            } else {
                                let error_msg = extract_error_message(&response);
                                tracing::warn!(
                                    "Order cancellation rejected by exchange: {}",
                                    error_msg
                                );
                                // Order cancel rejected events will be generated from WebSocket updates
                            }
                        }
                        Err(e) => {
                            tracing::warn!("Order cancellation HTTP request failed: {e}");
                            // WebSocket reconnection and reconciliation will handle recovery
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to convert order to Hyperliquid cancel format: {:?}",
                        e
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, command: &CancelAllOrders) -> anyhow::Result<()> {
        tracing::debug!("Cancelling all orders: {:?}", command);

        // Query cache for all open orders matching the instrument and side
        let cache = self.core.cache().borrow();
        let open_orders = cache.orders_open(
            Some(&self.core.venue),
            Some(&command.instrument_id),
            None,
            Some(command.order_side),
        );

        if open_orders.is_empty() {
            tracing::debug!("No open orders to cancel for {:?}", command.instrument_id);
            return Ok(());
        }

        // Convert orders to cancel requests
        let mut cancel_requests = Vec::new();
        let symbol = command.instrument_id.symbol.inner();
        for order in open_orders {
            let client_order_id = order.client_order_id().inner();

            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(req) => cancel_requests.push(req),
                Err(e) => {
                    tracing::warn!(
                        "Failed to convert order {} to cancel request: {}",
                        client_order_id,
                        e
                    );
                    continue;
                }
            }
        }

        if cancel_requests.is_empty() {
            tracing::debug!("No valid cancel requests to send");
            return Ok(());
        }

        // Create exchange action for cancellation with typed struct
        let action = ExchangeAction::cancel_by_cloid(cancel_requests);

        // Send cancel request via HTTP API
        // Note: The WebSocket connection will authoritatively handle the OrderCancelled events
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            if let Err(e) = http_client.post_action(&action).await {
                tracing::warn!("Failed to send cancel all orders request: {e}");
            }
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, command: &BatchCancelOrders) -> anyhow::Result<()> {
        tracing::debug!("Batch cancelling orders: {:?}", command);

        if command.cancels.is_empty() {
            tracing::debug!("No orders to cancel in batch");
            return Ok(());
        }

        // Convert each CancelOrder to a cancel request
        let mut cancel_requests = Vec::new();
        for cancel_cmd in &command.cancels {
            let client_order_id = cancel_cmd.client_order_id.inner();
            let symbol = cancel_cmd.instrument_id.symbol.inner();

            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(req) => cancel_requests.push(req),
                Err(e) => {
                    tracing::warn!(
                        "Failed to convert order {} to cancel request: {}",
                        client_order_id,
                        e
                    );
                    continue;
                }
            }
        }

        if cancel_requests.is_empty() {
            tracing::warn!("No valid cancel requests in batch");
            return Ok(());
        }

        let action = ExchangeAction::cancel_by_cloid(cancel_requests);

        // Send batch cancel request via HTTP API
        // Note: The WebSocket connection will authoritatively handle the OrderCancelled events
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            if let Err(e) = http_client.post_action(&action).await {
                tracing::warn!("Failed to send batch cancel orders request: {e}");
            }
        });

        Ok(())
    }

    fn query_account(&self, command: &QueryAccount) -> anyhow::Result<()> {
        tracing::debug!("Querying account: {:?}", command);

        // Use existing infrastructure to refresh account state
        let runtime = get_runtime();
        runtime.block_on(async {
            if let Err(e) = self.refresh_account_state().await {
                tracing::warn!("Failed to query account state: {e}");
            }
        });

        Ok(())
    }

    fn query_order(&self, command: &QueryOrder) -> anyhow::Result<()> {
        tracing::debug!("Querying order: {:?}", command);

        // Get venue order ID from cache
        let cache = self.core.cache().borrow();
        let venue_order_id = cache.venue_order_id(&command.client_order_id);

        let venue_order_id = match venue_order_id {
            Some(oid) => *oid,
            None => {
                tracing::warn!(
                    "No venue order ID found for client order {}",
                    command.client_order_id
                );
                return Ok(());
            }
        };
        drop(cache);

        // Parse venue order ID to u64
        let oid = match u64::from_str(venue_order_id.as_ref()) {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("Failed to parse venue order ID {}: {}", venue_order_id, e);
                return Ok(());
            }
        };

        // Get user address for the query
        let user_address = self.get_user_address()?;

        // Query order status via HTTP API
        // Note: The WebSocket connection is the authoritative source for order updates,
        // this is primarily for reconciliation or when WebSocket is unavailable
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            match http_client.info_order_status(&user_address, oid).await {
                Ok(status) => {
                    tracing::debug!("Order status for oid {}: {:?}", oid, status);
                }
                Err(e) => {
                    tracing::warn!("Failed to query order status for oid {}: {}", oid, e);
                }
            }
        });

        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected {
            return Ok(());
        }

        tracing::info!("Connecting Hyperliquid execution client");

        // Ensure instruments are initialized
        self.ensure_instruments_initialized_async().await?;

        // Connect WebSocket client
        self.ws_client.connect().await?;

        // Subscribe to user-specific order updates and fills
        let user_address = self.get_user_address()?;
        self.ws_client
            .subscribe_all_user_channels(&user_address)
            .await?;

        // Initialize account state
        self.refresh_account_state().await?;

        self.connected = true;
        self.core.set_connected(true);

        // Start WebSocket stream for execution updates
        if let Err(e) = self.start_ws_stream().await {
            tracing::warn!("Failed to start WebSocket stream: {e}");
        }

        tracing::info!(client_id = %self.core.client_id, "Connected");
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected {
            return Ok(());
        }

        tracing::info!("Disconnecting Hyperliquid execution client");

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.connected = false;
        self.core.set_connected(false);

        tracing::info!(client_id = %self.core.client_id, "Disconnected");
        Ok(())
    }
}

#[async_trait(?Send)]
impl LiveExecutionClient for HyperliquidExecutionClient {
    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // NOTE: Single order status report generation requires instrument cache integration.
        // The HTTP client methods and parsing functions are implemented and ready to use.
        // When implemented: query via info_order_status(), parse with parse_order_status_report_from_basic().
        tracing::warn!("generate_order_status_report not yet fully implemented");
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let user_address = self.get_user_address()?;

        let reports = self
            .http_client
            .request_order_status_reports(&user_address, cmd.instrument_id)
            .await
            .context("failed to generate order status reports")?;

        // Filter by client_order_id if specified
        let reports = if let Some(client_order_id) = cmd.client_order_id {
            reports
                .into_iter()
                .filter(|r| r.client_order_id == Some(client_order_id))
                .collect()
        } else {
            reports
        };

        // Note: cmd.venue_order_id is Option<ClientOrderId> in the struct definition,
        // but report venue_order_id is VenueOrderId - type mismatch prevents filtering here

        tracing::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let user_address = self.get_user_address()?;

        let reports = self
            .http_client
            .request_fill_reports(&user_address, cmd.instrument_id)
            .await
            .context("failed to generate fill reports")?;

        // Filter by time range if specified
        let reports = if let (Some(start), Some(end)) = (cmd.start, cmd.end) {
            reports
                .into_iter()
                .filter(|r| r.ts_event >= start && r.ts_event <= end)
                .collect()
        } else if let Some(start) = cmd.start {
            reports
                .into_iter()
                .filter(|r| r.ts_event >= start)
                .collect()
        } else if let Some(end) = cmd.end {
            reports.into_iter().filter(|r| r.ts_event <= end).collect()
        } else {
            reports
        };

        tracing::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let user_address = self.get_user_address()?;

        let reports = self
            .http_client
            .request_position_status_reports(&user_address, cmd.instrument_id)
            .await
            .context("failed to generate position status reports")?;

        tracing::info!("Generated {} position status reports", reports.len());
        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        tracing::warn!(
            "generate_mass_status not yet implemented (lookback_mins={lookback_mins:?})"
        );
        // Full implementation would require:
        // 1. Query all orders within lookback window
        // 2. Query all fills within lookback window
        // 3. Query all positions
        // 4. Combine into ExecutionMassStatus
        Ok(None)
    }
}

impl HyperliquidExecutionClient {
    async fn start_ws_stream(&mut self) -> anyhow::Result<()> {
        {
            let handle_guard = self.ws_stream_handle.lock().expect(MUTEX_POISONED);
            if handle_guard.is_some() {
                return Ok(());
            }
        }

        let user_address = self.get_user_address()?;
        let _account_id = self.core.account_id;
        let mut ws_client = self.ws_client.clone();

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .unwrap_or_default();

        for instrument in instruments {
            ws_client.cache_instrument(instrument);
        }

        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = ws_client.connect().await {
                tracing::warn!("Failed to connect WebSocket: {e}");
                return;
            }

            if let Err(e) = ws_client.subscribe_order_updates(&user_address).await {
                tracing::warn!("Failed to subscribe to order updates: {e}");
                return;
            }

            if let Err(e) = ws_client.subscribe_user_events(&user_address).await {
                tracing::warn!("Failed to subscribe to user events: {e}");
                return;
            }

            tracing::info!("Subscribed to Hyperliquid execution updates");

            let _clock = get_atomic_clock_realtime();

            loop {
                let event = ws_client.next_event().await;

                match event {
                    Some(msg) => {
                        match msg {
                            NautilusWsMessage::ExecutionReports(reports) => {
                                // Handler already parsed the messages, just dispatch them
                                for report in reports {
                                    dispatch_execution_report(report);
                                }
                            }
                            NautilusWsMessage::Reconnected => {
                                tracing::info!("WebSocket reconnected");
                                // TODO: Resubscribe to user channels if needed
                            }
                            NautilusWsMessage::Error(e) => {
                                tracing::error!("WebSocket error: {e}");
                            }
                            // Handled by data client
                            NautilusWsMessage::Trades(_)
                            | NautilusWsMessage::Quote(_)
                            | NautilusWsMessage::Deltas(_)
                            | NautilusWsMessage::Candle(_)
                            | NautilusWsMessage::MarkPrice(_)
                            | NautilusWsMessage::IndexPrice(_)
                            | NautilusWsMessage::FundingRate(_) => {}
                        }
                    }
                    None => {
                        tracing::warn!("WebSocket next_event returned None");
                        break;
                    }
                }
            }
        });

        *self.ws_stream_handle.lock().expect(MUTEX_POISONED) = Some(handle);
        tracing::info!("Hyperliquid WebSocket execution stream started");
        Ok(())
    }
}

fn dispatch_execution_report(report: ExecutionReport) {
    let sender = get_exec_event_sender();
    match report {
        ExecutionReport::Order(order_report) => {
            let exec_report = NautilusExecutionReport::OrderStatus(Box::new(order_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                tracing::warn!("Failed to send order status report: {e}");
            }
        }
        ExecutionReport::Fill(fill_report) => {
            let exec_report = NautilusExecutionReport::Fill(Box::new(fill_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                tracing::warn!("Failed to send fill report: {e}");
            }
        }
    }
}
