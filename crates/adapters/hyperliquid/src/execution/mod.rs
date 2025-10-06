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

use alloy_signer_local::PrivateKeySigner;
use anyhow::{Context, Result};
use nautilus_common::{
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, ModifyOrder, QueryAccount, QueryOrder,
        SubmitOrder, SubmitOrderList,
    },
    runtime::get_runtime,
};
use nautilus_core::UnixNanos;
use nautilus_execution::client::{ExecutionClient, base::ExecutionClientCore};
use nautilus_model::{
    accounts::AccountAny,
    enums::{OmsType, OrderType},
    identifiers::{AccountId, ClientId, Venue},
    orders::{Order, any::OrderAny},
    types::{AccountBalance, MarginBalance},
};
use serde_json;
use tokio::task::JoinHandle;
use tracing::{debug, info, warn};

use crate::{
    common::{
        consts::HYPERLIQUID_VENUE,
        credential::Secrets,
        parse::{
            cancel_requests_to_hyperliquid_action_value, client_order_id_to_cancel_request,
            extract_error_message, is_response_successful, order_any_to_hyperliquid_request,
            orders_to_hyperliquid_requests,
        },
    },
    config::HyperliquidExecClientConfig,
    http::{client::HyperliquidHttpClient, query::ExchangeAction},
    websocket::client::HyperliquidWebSocketClient,
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
    fn validate_order_submission(&self, order: &OrderAny) -> Result<()> {
        // Check if instrument symbol is supported
        let symbol = order.instrument_id().symbol.to_string();
        if !symbol.ends_with("-USD") {
            anyhow::bail!("Unsupported instrument symbol format for Hyperliquid: {symbol}");
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
    pub fn new(core: ExecutionClientCore, config: HyperliquidExecClientConfig) -> Result<Self> {
        if !config.has_credentials() {
            anyhow::bail!("Hyperliquid execution client requires private key");
        }

        let secrets = Secrets::from_json(&format!(
            r#"{{"privateKey": "{}", "isTestnet": {}}}"#,
            config.private_key, config.is_testnet
        ))
        .context("failed to create secrets from private key")?;

        let http_client =
            HyperliquidHttpClient::with_credentials(&secrets, Some(config.http_timeout_secs));

        // Create WebSocket client (will connect when needed)
        let ws_client = HyperliquidWebSocketClient::new(
            crate::common::consts::ws_url(config.is_testnet).to_string(),
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
        })
    }

    async fn ensure_instruments_initialized_async(&mut self) -> Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Hyperliquid instruments")?;

        if instruments.is_empty() {
            warn!("Instrument bootstrap yielded no instruments; WebSocket submissions may fail");
        } else {
            info!("Initialized {} instruments", instruments.len());
        }

        self.instruments_initialized = true;
        Ok(())
    }

    fn ensure_instruments_initialized(&mut self) -> Result<()> {
        if self.instruments_initialized {
            return Ok(());
        }

        let runtime = get_runtime();
        runtime.block_on(self.ensure_instruments_initialized_async())
    }

    async fn refresh_account_state(&self) -> Result<()> {
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
            .context("Failed to fetch clearinghouse state")?;

        // Deserialize the response
        let state: crate::http::models::ClearinghouseState =
            serde_json::from_value(clearinghouse_state)
                .context("Failed to deserialize clearinghouse state")?;

        debug!(
            "Received clearinghouse state: cross_margin_summary={:?}, asset_positions={}",
            state.cross_margin_summary,
            state.asset_positions.len()
        );

        // Parse balances and margins from cross margin summary
        if let Some(ref cross_margin_summary) = state.cross_margin_summary {
            let (balances, margins) =
                crate::common::parse::parse_account_balances_and_margins(cross_margin_summary)
                    .context("Failed to parse account balances and margins")?;

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

            info!("Account state updated successfully");
        } else {
            warn!("No cross margin summary in clearinghouse state");
        }

        Ok(())
    }

    fn get_user_address(&self) -> Result<String> {
        // Derive Ethereum address from private key using alloy-signer
        // Strip "0x" prefix if present
        let key_str = self
            .config
            .private_key
            .strip_prefix("0x")
            .unwrap_or(&self.config.private_key);

        // Create signer from hex private key
        let signer = PrivateKeySigner::from_str(key_str)
            .context("Failed to create signer from private key")?;

        // Get address directly from signer
        let address = format!("{:?}", signer.address());

        debug!("Derived Ethereum address: {}", address);
        Ok(address)
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(err) = fut.await {
                warn!("{description} failed: {err:?}");
            }
        });

        let mut tasks = self.pending_tasks.lock().unwrap();
        tasks.retain(|handle| !handle.is_finished());
        tasks.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().unwrap();
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    fn update_account_state(&self) -> Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }
}

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
    ) -> Result<()> {
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> Result<()> {
        if self.started {
            return Ok(());
        }

        info!("Starting Hyperliquid execution client");

        // Ensure instruments are initialized
        self.ensure_instruments_initialized()?;

        // Initialize account state
        if let Err(e) = self.update_account_state() {
            warn!("Failed to initialize account state: {}", e);
        }

        self.connected = true;
        self.started = true;

        info!("Hyperliquid execution client started");
        Ok(())
    }
    fn stop(&mut self) -> Result<()> {
        if !self.started {
            return Ok(());
        }

        info!("Stopping Hyperliquid execution client");

        // Abort any pending tasks
        self.abort_pending_tasks();

        // Disconnect WebSocket
        if self.connected {
            let runtime = get_runtime();
            runtime.block_on(async {
                if let Err(e) = self.ws_client.disconnect().await {
                    warn!("Error disconnecting WebSocket client: {e}");
                }
            });
        }

        self.connected = false;
        self.started = false;

        info!("Hyperliquid execution client stopped");
        Ok(())
    }

    fn submit_order(&self, command: &SubmitOrder) -> Result<()> {
        let order = &command.order;

        if order.is_closed() {
            warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Validate order before submission
        if let Err(err) = self.validate_order_submission(order) {
            self.core.generate_order_rejected(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                &format!("validation-error: {err}"),
                command.ts_init,
                false,
            );
            return Err(err);
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
                    // Convert single order to JSON array format for the exchange action
                    match serde_json::to_value(vec![hyperliquid_order]) {
                        Ok(orders_json) => {
                            // Create exchange action for order placement
                            let action = ExchangeAction::order(orders_json);

                            match http_client.post_action(&action).await {
                                Ok(response) => {
                                    if is_response_successful(&response) {
                                        info!("Order submitted successfully: {:?}", response);
                                        // Order acceptance/rejection events will be generated from WebSocket updates
                                        // which provide the venue_order_id and definitive status
                                    } else {
                                        let error_msg = extract_error_message(&response);
                                        warn!(
                                            "Order submission rejected by exchange: {}",
                                            error_msg
                                        );
                                        // Order rejection event will be generated from WebSocket updates
                                    }
                                }
                                Err(err) => {
                                    warn!("Order submission HTTP request failed: {err}");
                                    // WebSocket reconnection and order reconciliation will handle recovery
                                }
                            }
                        }
                        Err(err) => {
                            warn!("Failed to serialize order to JSON: {err}");
                            // This indicates a client-side bug that should be fixed
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to convert order to Hyperliquid format: {err}");
                    // This indicates a client-side bug or unsupported order configuration
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, command: &SubmitOrderList) -> Result<()> {
        debug!(
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
                    match serde_json::to_value(hyperliquid_orders) {
                        Ok(orders_json) => {
                            let action = ExchangeAction::order(orders_json);
                            match http_client.post_action(&action).await {
                                Ok(response) => {
                                    if is_response_successful(&response) {
                                        info!("Order list submitted successfully: {:?}", response);
                                        // Order acceptance/rejection events will be generated from WebSocket updates
                                    } else {
                                        let error_msg = extract_error_message(&response);
                                        warn!(
                                            "Order list submission rejected by exchange: {}",
                                            error_msg
                                        );
                                        // Individual order rejection events will be generated from WebSocket updates
                                    }
                                }
                                Err(err) => {
                                    warn!("Order list submission HTTP request failed: {err}");
                                    // WebSocket reconciliation will handle recovery
                                }
                            }
                        }
                        Err(err) => {
                            warn!("Failed to serialize order list to JSON: {err}");
                        }
                    }
                }
                Err(err) => {
                    warn!("Failed to convert order list to Hyperliquid format: {err}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, command: &ModifyOrder) -> Result<()> {
        debug!("Modifying order: {:?}", command);

        // Parse venue_order_id as u64
        let oid: u64 = match command.venue_order_id.as_str().parse() {
            Ok(id) => id,
            Err(err) => {
                warn!(
                    "Failed to parse venue_order_id '{}' as u64: {}",
                    command.venue_order_id, err
                );
                return Ok(());
            }
        };

        let http_client = self.http_client.clone();
        let price = command.price;
        let quantity = command.quantity;

        self.spawn_task("modify_order", async move {
            // Build modify request with new price and/or quantity
            let mut modify_params = serde_json::Map::new();

            if let Some(new_price) = price {
                modify_params.insert(
                    "limitPx".to_string(),
                    serde_json::json!(new_price.to_string()),
                );
            }

            if let Some(new_qty) = quantity {
                modify_params.insert("sz".to_string(), serde_json::json!(new_qty.to_string()));
            }

            let action = ExchangeAction::modify(oid, serde_json::Value::Object(modify_params));

            match http_client.post_action(&action).await {
                Ok(response) => {
                    if is_response_successful(&response) {
                        info!("Order modified successfully: {:?}", response);
                        // Order update events will be generated from WebSocket updates
                    } else {
                        let error_msg = extract_error_message(&response);
                        warn!("Order modification rejected by exchange: {}", error_msg);
                        // Order modify rejected events will be generated from WebSocket updates
                    }
                }
                Err(err) => {
                    warn!("Order modification HTTP request failed: {err}");
                    // WebSocket reconciliation will handle recovery
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, command: &CancelOrder) -> Result<()> {
        debug!("Cancelling order: {:?}", command);

        let http_client = self.http_client.clone();
        let client_order_id = command.client_order_id.to_string();
        let symbol = command.instrument_id.symbol.to_string();

        self.spawn_task("cancel_order", async move {
            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(cancel_request) => {
                    // Convert single cancel request to JSON array format for the exchange action
                    match serde_json::to_value(vec![cancel_request]) {
                        Ok(cancels_json) => {
                            // Create exchange action for order cancellation
                            let action = ExchangeAction::cancel_by_cloid(cancels_json);
                            match http_client.post_action(&action).await {
                                Ok(response) => {
                                    if is_response_successful(&response) {
                                        info!("Order cancelled successfully: {:?}", response);
                                        // Order cancelled events will be generated from WebSocket updates
                                        // which provide definitive confirmation and venue_order_id
                                    } else {
                                        let error_msg = extract_error_message(&response);
                                        warn!(
                                            "Order cancellation rejected by exchange: {}",
                                            error_msg
                                        );
                                        // Order cancel rejected events will be generated from WebSocket updates
                                    }
                                }
                                Err(err) => {
                                    warn!("Order cancellation HTTP request failed: {err}");
                                    // WebSocket reconnection and reconciliation will handle recovery
                                }
                            }
                        }
                        Err(err) => {
                            warn!("Failed to serialize cancel request to JSON: {:?}", err);
                        }
                    }
                }
                Err(err) => {
                    warn!(
                        "Failed to convert order to Hyperliquid cancel format: {:?}",
                        err
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, command: &CancelAllOrders) -> Result<()> {
        debug!("Cancelling all orders: {:?}", command);

        // Query cache for all open orders matching the instrument and side
        let cache = self.core.cache().borrow();
        let open_orders = cache.orders_open(
            Some(&self.core.venue),
            Some(&command.instrument_id),
            None,
            Some(command.order_side),
        );

        if open_orders.is_empty() {
            debug!("No open orders to cancel for {:?}", command.instrument_id);
            return Ok(());
        }

        // Convert orders to cancel requests
        let mut cancel_requests = Vec::new();
        for order in open_orders {
            let client_order_id = order.client_order_id().to_string();
            let symbol = command.instrument_id.symbol.to_string();

            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(req) => cancel_requests.push(req),
                Err(e) => {
                    warn!(
                        "Failed to convert order {} to cancel request: {}",
                        client_order_id, e
                    );
                    continue;
                }
            }
        }

        if cancel_requests.is_empty() {
            debug!("No valid cancel requests to send");
            return Ok(());
        }

        let cancels_value = cancel_requests_to_hyperliquid_action_value(&cancel_requests)
            .context("Failed to convert cancel requests to JSON")?;

        let action = ExchangeAction::cancel_by_cloid(cancels_value);

        // Send cancel request via HTTP API
        // Note: The WebSocket connection will authoritatively handle the OrderCancelled events
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            if let Err(e) = http_client.post_action(&action).await {
                warn!("Failed to send cancel all orders request: {}", e);
            }
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, command: &BatchCancelOrders) -> Result<()> {
        debug!("Batch cancelling orders: {:?}", command);

        if command.cancels.is_empty() {
            debug!("No orders to cancel in batch");
            return Ok(());
        }

        // Convert each CancelOrder to a cancel request
        let mut cancel_requests = Vec::new();
        for cancel_cmd in &command.cancels {
            let client_order_id = cancel_cmd.client_order_id.to_string();
            let symbol = cancel_cmd.instrument_id.symbol.to_string();

            match client_order_id_to_cancel_request(&client_order_id, &symbol) {
                Ok(req) => cancel_requests.push(req),
                Err(e) => {
                    warn!(
                        "Failed to convert order {} to cancel request: {}",
                        client_order_id, e
                    );
                    continue;
                }
            }
        }

        if cancel_requests.is_empty() {
            warn!("No valid cancel requests in batch");
            return Ok(());
        }

        let cancels_value = cancel_requests_to_hyperliquid_action_value(&cancel_requests)
            .context("Failed to convert cancel requests to JSON")?;

        let action = ExchangeAction::cancel_by_cloid(cancels_value);

        // Send batch cancel request via HTTP API
        // Note: The WebSocket connection will authoritatively handle the OrderCancelled events
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            if let Err(e) = http_client.post_action(&action).await {
                warn!("Failed to send batch cancel orders request: {}", e);
            }
        });

        Ok(())
    }

    fn query_account(&self, command: &QueryAccount) -> Result<()> {
        debug!("Querying account: {:?}", command);

        // Use existing infrastructure to refresh account state
        let runtime = get_runtime();
        runtime.block_on(async {
            if let Err(e) = self.refresh_account_state().await {
                warn!("Failed to query account state: {}", e);
            }
        });

        Ok(())
    }

    fn query_order(&self, command: &QueryOrder) -> Result<()> {
        debug!("Querying order: {:?}", command);

        // Get venue order ID from cache
        let cache = self.core.cache().borrow();
        let venue_order_id = cache.venue_order_id(&command.client_order_id);

        let venue_order_id = match venue_order_id {
            Some(oid) => *oid,
            None => {
                warn!(
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
                warn!("Failed to parse venue order ID {}: {}", venue_order_id, e);
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
                    debug!("Order status for oid {}: {:?}", oid, status);
                }
                Err(e) => {
                    warn!("Failed to query order status for oid {}: {}", oid, e);
                }
            }
        });

        Ok(())
    }
}

////////////////////////////////////////////////////////////////////////////////
// LiveExecutionClient Implementation
////////////////////////////////////////////////////////////////////////////////

use async_trait::async_trait;
use nautilus_common::messages::execution::{
    GenerateFillReports, GenerateOrderStatusReport, GeneratePositionReports,
};
use nautilus_execution::client::LiveExecutionClient;
use nautilus_model::reports::{
    ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport,
};

#[async_trait(?Send)]
impl LiveExecutionClient for HyperliquidExecutionClient {
    async fn connect(&mut self) -> Result<()> {
        if self.connected {
            return Ok(());
        }

        info!("Connecting Hyperliquid execution client");

        // Ensure instruments are initialized
        self.ensure_instruments_initialized_async().await?;

        // Connect WebSocket client
        let url = crate::common::consts::ws_url(self.config.is_testnet);
        self.ws_client = HyperliquidWebSocketClient::connect(url).await?;

        // Subscribe to user-specific order updates and fills
        let user_address = self.get_user_address()?;
        self.ws_client
            .subscribe_all_user_channels(&user_address)
            .await?;

        // Initialize account state
        self.refresh_account_state().await?;

        // Note: Order reconciliation is handled by the execution engine
        // which will call generate_order_status_reports() after connection

        self.connected = true;
        self.core.set_connected(true);

        info!(
            "Hyperliquid execution client {} connected",
            self.core.client_id
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> Result<()> {
        if !self.connected {
            return Ok(());
        }

        info!("Disconnecting Hyperliquid execution client");

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.connected = false;
        self.core.set_connected(false);

        info!(
            "Hyperliquid execution client {} disconnected",
            self.core.client_id
        );
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> Result<Option<OrderStatusReport>> {
        // NOTE: Single order status report generation requires instrument cache integration.
        // The HTTP client methods and parsing functions are implemented and ready to use.
        // When implemented: query via info_order_status(), parse with parse_order_status_report_from_basic().
        warn!("generate_order_status_report not yet fully implemented");
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Result<Vec<OrderStatusReport>> {
        // NOTE: Order status reports generation infrastructure is complete:
        // HTTP methods: info_open_orders(), info_frontend_open_orders()
        // Parsing: parse_order_status_report_from_basic() and parse_order_status_report_from_ws()
        // Status mapping: All order statuses and types supported
        //  Pending: Instrument cache integration to look up instruments by ID
        // Implementation: Fetch via info_frontend_open_orders(), parse each order, filter by cmd params

        warn!("generate_order_status_reports requires instrument cache integration");

        // Log what would be queried
        if let Some(instrument_id) = cmd.instrument_id {
            debug!("Would query orders for instrument: {}", instrument_id);
        }
        if let Some(client_order_id) = cmd.client_order_id {
            debug!("Would filter by client_order_id: {}", client_order_id);
        }
        if let Some(venue_order_id) = cmd.venue_order_id {
            debug!("Would filter by venue_order_id: {}", venue_order_id);
        }

        Ok(Vec::new())
    }

    async fn generate_fill_reports(&self, cmd: GenerateFillReports) -> Result<Vec<FillReport>> {
        // NOTE: Fill reports generation infrastructure is complete:
        // HTTP methods: info_user_fills() returns HyperliquidFills
        // Parsing: parse_fill_report() with fee handling, liquidity side detection
        // Money/Currency: Proper USDC fee integration
        //  Pending: Instrument cache integration to look up instruments by symbol
        // Implementation: Fetch via info_user_fills(), filter by time range, parse each fill

        warn!("generate_fill_reports requires instrument cache integration");

        // Log what would be queried
        if let Some(start) = cmd.start {
            debug!("Would filter fills from: {}", start);
        }
        if let Some(end) = cmd.end {
            debug!("Would filter fills until: {}", end);
        }
        if let Some(instrument_id) = cmd.instrument_id {
            debug!("Would filter fills for instrument: {}", instrument_id);
        }

        Ok(Vec::new())
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionReports,
    ) -> Result<Vec<PositionStatusReport>> {
        // Get user address for API queries
        let user_address = self.get_user_address()?;

        // Query clearinghouse state from the API
        let _response = self
            .http_client
            .info_clearinghouse_state(&user_address)
            .await
            .context("Failed to fetch clearinghouse state")?;

        // NOTE: Position status reports infrastructure is complete:
        // HTTP methods: info_clearinghouse_state() queries API successfully
        // Models: ClearinghouseState, AssetPosition, PositionData all defined
        // Parsing: parse_position_status_report() fully implemented
        //  Pending: Instrument cache integration to look up instruments by coin symbol
        // Implementation: Deserialize response to ClearinghouseState, iterate asset_positions,
        //                parse each with parse_position_status_report(), filter by cmd params
        warn!("Position status report parsing requires instrument cache integration");

        // When cache available:
        // 1. Deserialize clearinghouse state: serde_json::from_value::<ClearinghouseState>(response)
        // 2. For each asset_position: look up instrument by position.coin
        // 3. Parse: parse_position_status_report(&asset_position_json, instrument, account_id, ts_init)
        // 4. Filter by cmd.instrument_id if specified

        if cmd.instrument_id.is_some() {
            debug!(
                "Would filter positions by instrument_id: {:?}",
                cmd.instrument_id
            );
        }

        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> Result<Option<ExecutionMassStatus>> {
        warn!("generate_mass_status not yet implemented (lookback_mins={lookback_mins:?})");
        // Full implementation would require:
        // 1. Query all orders within lookback window
        // 2. Query all fills within lookback window
        // 3. Query all positions
        // 4. Combine into ExecutionMassStatus
        Ok(None)
    }
}

// Re-export execution models from the http module
pub use crate::http::models::{
    AssetId, Cloid, HyperliquidExecAction, HyperliquidExecBuilderFee,
    HyperliquidExecCancelByCloidRequest, HyperliquidExecCancelOrderRequest,
    HyperliquidExecCancelResponseData, HyperliquidExecCancelStatus, HyperliquidExecFilledInfo,
    HyperliquidExecGrouping, HyperliquidExecLimitParams, HyperliquidExecModifyOrderRequest,
    HyperliquidExecModifyResponseData, HyperliquidExecModifyStatus, HyperliquidExecOrderKind,
    HyperliquidExecOrderResponseData, HyperliquidExecOrderStatus, HyperliquidExecPlaceOrderRequest,
    HyperliquidExecRequest, HyperliquidExecResponse, HyperliquidExecResponseData,
    HyperliquidExecRestingInfo, HyperliquidExecTif, HyperliquidExecTpSl,
    HyperliquidExecTriggerParams, HyperliquidExecTwapRequest, OrderId,
};
