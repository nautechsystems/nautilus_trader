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

//! Live execution client implementation for the Hyperliquid adapter.

use std::{
    str::FromStr,
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::fifo::FifoCache,
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderStatus, OrderType},
    identifiers::{AccountId, ClientId, ClientOrderId, Venue},
    orders::{Order, any::OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        builder_fee::{resolve_builder_fee, resolve_builder_fee_batch},
        consts::HYPERLIQUID_VENUE,
        credential::Secrets,
        parse::{
            client_order_id_to_cancel_request_with_asset, extract_error_message,
            order_to_hyperliquid_request_with_asset, parse_account_balances_and_margins,
        },
    },
    config::HyperliquidExecClientConfig,
    http::{
        client::HyperliquidHttpClient,
        models::{
            ClearinghouseState, Cloid, HyperliquidExecAction, HyperliquidExecGrouping,
            HyperliquidExecModifyOrderRequest,
        },
    },
    websocket::{ExecutionReport, NautilusWsMessage, client::HyperliquidWebSocketClient},
};

#[derive(Debug)]
pub struct HyperliquidExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: HyperliquidExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: HyperliquidHttpClient,
    ws_client: HyperliquidWebSocketClient,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
}

impl HyperliquidExecutionClient {
    /// Returns a reference to the configuration.
    pub fn config(&self) -> &HyperliquidExecClientConfig {
        &self.config
    }

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

        let secrets = Secrets::from_private_key(
            &config.private_key,
            config.vault_address.as_deref(),
            config.is_testnet,
        )
        .context("failed to create secrets from private key")?;

        let mut http_client = HyperliquidHttpClient::with_secrets(
            &secrets,
            Some(config.http_timeout_secs),
            config.http_proxy_url.clone(),
        )
        .context("failed to create Hyperliquid HTTP client")?;

        http_client.set_account_id(core.account_id);

        // Apply URL overrides from config (used for testing with mock servers)
        if let Some(url) = &config.base_url_http {
            http_client.set_base_info_url(url.clone());
        }
        if let Some(url) = &config.base_url_exchange {
            http_client.set_base_exchange_url(url.clone());
        }

        let ws_url = config.base_url_ws.clone();
        let ws_client =
            HyperliquidWebSocketClient::new(ws_url, config.is_testnet, Some(core.account_id));

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Margin,
            None,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_client,
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
        })
    }

    async fn ensure_instruments_initialized_async(&mut self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .context("failed to request Hyperliquid instruments")?;

        if instruments.is_empty() {
            log::warn!(
                "Instrument bootstrap yielded no instruments; WebSocket submissions may fail"
            );
        } else {
            log::info!("Initialized {} instruments", instruments.len());

            for instrument in &instruments {
                self.http_client.cache_instrument(instrument.clone());
            }
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_address = self.get_account_address()?;

        let clearinghouse_state = self
            .http_client
            .info_clearinghouse_state(&account_address)
            .await
            .context("failed to fetch clearinghouse state")?;

        // Deserialize the response
        let state: ClearinghouseState = serde_json::from_value(clearinghouse_state)
            .context("failed to deserialize clearinghouse state")?;

        log::debug!(
            "Received clearinghouse state: cross_margin_summary={:?}, asset_positions={}",
            state.cross_margin_summary,
            state.asset_positions.len()
        );

        // Parse balances and margins from cross margin summary
        if let Some(ref cross_margin_summary) = state.cross_margin_summary {
            let (balances, margins) = parse_account_balances_and_margins(cross_margin_summary)
                .context("failed to parse account balances and margins")?;

            // Generate account state event
            let ts_event = self.clock.get_time_ns();
            self.emitter
                .emit_account_state(balances, margins, true, ts_event);

            log::info!("Account state updated successfully");
        } else {
            log::warn!("No cross margin summary in clearinghouse state");
        }

        Ok(())
    }

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

    fn get_user_address(&self) -> anyhow::Result<String> {
        self.http_client
            .get_user_address()
            .context("failed to get user address from HTTP client")
    }

    fn get_account_address(&self) -> anyhow::Result<String> {
        match &self.config.vault_address {
            Some(vault) => Ok(vault.clone()),
            None => self.get_user_address(),
        }
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
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
}

#[async_trait(?Send)]
impl ExecutionClient for HyperliquidExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.is_connected()
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
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}, is_testnet={}, vault_address={:?}, http_proxy_url={:?}, ws_proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.config.is_testnet,
            self.config.vault_address,
            self.config.http_proxy_url,
            self.config.ws_proxy_url,
        );

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        log::info!("Stopping Hyperliquid execution client");

        // Stop WebSocket stream
        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        // Abort any pending tasks
        self.abort_pending_tasks();

        // Disconnect WebSocket
        if self.core.is_connected() {
            let runtime = get_runtime();
            runtime.block_on(async {
                if let Err(e) = self.ws_client.disconnect().await {
                    log::warn!("Error disconnecting WebSocket client: {e}");
                }
            });
        }

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Hyperliquid execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Err(e) = self.validate_order_submission(&order) {
            self.emitter
                .emit_order_denied(&order, &format!("Validation failed: {e}"));
            return Err(e);
        }

        let http_client = self.http_client.clone();
        let symbol = order.instrument_id().symbol.to_string();

        // Validate asset index exists before marking as submitted
        let asset = match http_client.get_asset_index(&symbol) {
            Some(a) => a,
            None => {
                self.emitter
                    .emit_order_denied(&order, &format!("Asset index not found for {symbol}"));
                return Ok(());
            }
        };

        // Validate order conversion before marking as submitted
        let hyperliquid_order = match order_to_hyperliquid_request_with_asset(&order, asset) {
            Ok(req) => req,
            Err(e) => {
                self.emitter
                    .emit_order_denied(&order, &format!("Order conversion failed: {e}"));
                return Ok(());
            }
        };

        // Cache cloid mapping before emitting submitted so WS handler
        // can resolve order/fill reports back to this client_order_id
        let cloid = Cloid::from_client_order_id(order.client_order_id());
        self.ws_client
            .cache_cloid_mapping(Ustr::from(&cloid.to_hex()), order.client_order_id());

        self.emitter.emit_order_submitted(&order);

        let builder = resolve_builder_fee(&symbol, order.is_post_only());

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ws_client = self.ws_client.clone();
        let cloid_hex = Ustr::from(&cloid.to_hex());

        self.spawn_task("submit_order", async move {
            let action = HyperliquidExecAction::Order {
                orders: vec![hyperliquid_order],
                grouping: HyperliquidExecGrouping::Na,
                builder,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        log::info!("Order submitted successfully: {response:?}");
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order submission rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();
                        emitter.emit_order_rejected(&order, &error_msg, ts, false);
                        ws_client.remove_cloid_mapping(&cloid_hex);
                    }
                }
                Err(e) => {
                    // Don't reject on transport errors: the order may have
                    // landed and WS events will drive the lifecycle. If it
                    // didn't land, reconciliation on reconnect resolves it.
                    log::error!("Order submission HTTP request failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::debug!(
            "Submitting order list with {} orders",
            cmd.order_list.client_order_ids.len()
        );

        let http_client = self.http_client.clone();

        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        // Validate all orders synchronously and collect valid ones
        let mut valid_orders = Vec::new();
        let mut hyperliquid_orders = Vec::new();

        for order in &orders {
            let symbol = order.instrument_id().symbol.to_string();
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    self.emitter
                        .emit_order_denied(order, &format!("Asset index not found for {symbol}"));
                    continue;
                }
            };

            match order_to_hyperliquid_request_with_asset(order, asset) {
                Ok(req) => {
                    hyperliquid_orders.push(req);
                    valid_orders.push(order.clone());
                }
                Err(e) => {
                    self.emitter
                        .emit_order_denied(order, &format!("Order conversion failed: {e}"));
                }
            }
        }

        if valid_orders.is_empty() {
            log::warn!("No valid orders to submit in order list");
            return Ok(());
        }

        for order in &valid_orders {
            let cloid = Cloid::from_client_order_id(order.client_order_id());
            self.ws_client
                .cache_cloid_mapping(Ustr::from(&cloid.to_hex()), order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        let order_props: Vec<(String, bool)> = valid_orders
            .iter()
            .map(|o| (o.instrument_id().symbol.to_string(), o.is_post_only()))
            .collect();
        let batch_refs: Vec<(&str, bool)> =
            order_props.iter().map(|(s, p)| (s.as_str(), *p)).collect();
        let builder = resolve_builder_fee_batch(&batch_refs);

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ws_client = self.ws_client.clone();
        let cloid_hexes: Vec<Ustr> = valid_orders
            .iter()
            .map(|o| Ustr::from(&Cloid::from_client_order_id(o.client_order_id()).to_hex()))
            .collect();

        self.spawn_task("submit_order_list", async move {
            let action = HyperliquidExecAction::Order {
                orders: hyperliquid_orders,
                grouping: HyperliquidExecGrouping::Na,
                builder,
            };
            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        log::info!("Order list submitted successfully: {response:?}");
                    } else {
                        // Hyperliquid batch endpoint rejects all-or-nothing
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order list submission rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();
                        for order in &valid_orders {
                            emitter.emit_order_rejected(order, &error_msg, ts, false);
                        }
                        for cloid_hex in &cloid_hexes {
                            ws_client.remove_cloid_mapping(cloid_hex);
                        }
                    }
                }
                Err(e) => {
                    // Don't reject on transport errors: orders may have
                    // landed and WS events will drive the lifecycle. If they
                    // didn't land, reconciliation on reconnect resolves it.
                    log::error!("Order list submission HTTP request failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        log::debug!("Modifying order: {cmd:?}");

        // Parse venue_order_id as u64
        let venue_order_id = match cmd.venue_order_id {
            Some(id) => id,
            None => {
                log::warn!("Cannot modify order: venue_order_id is None");
                return Ok(());
            }
        };

        let oid: u64 = match venue_order_id.as_str().parse() {
            Ok(id) => id,
            Err(e) => {
                log::warn!("Failed to parse venue_order_id '{venue_order_id}' as u64: {e}");
                return Ok(());
            }
        };

        let http_client = self.http_client.clone();
        let price = cmd.price;
        let quantity = cmd.quantity;
        let symbol = cmd.instrument_id.symbol.to_string();

        self.spawn_task("modify_order", async move {
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    log::warn!(
                        "Asset index not found for symbol {symbol}, ensure instruments are loaded"
                    );
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

            let action = HyperliquidExecAction::Modify {
                modify: modify_request,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        log::info!("Order modified successfully: {response:?}");
                        // Order update events will be generated from WebSocket updates
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order modification rejected by exchange: {error_msg}");
                        // Order modify rejected events will be generated from WebSocket updates
                    }
                }
                Err(e) => {
                    log::warn!("Order modification HTTP request failed: {e}");
                    // WebSocket reconciliation will handle recovery
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        log::debug!("Cancelling order: {cmd:?}");

        let http_client = self.http_client.clone();
        let client_order_id = cmd.client_order_id.to_string();
        let symbol = cmd.instrument_id.symbol.to_string();

        self.spawn_task("cancel_order", async move {
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    log::warn!(
                        "Asset index not found for symbol {symbol}, ensure instruments are loaded"
                    );
                    return Ok(());
                }
            };

            let cancel_request =
                client_order_id_to_cancel_request_with_asset(&client_order_id, asset);
            let action = HyperliquidExecAction::CancelByCloid {
                cancels: vec![cancel_request],
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        log::info!("Order cancelled successfully: {response:?}");
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order cancellation rejected by exchange: {error_msg}");
                    }
                }
                Err(e) => {
                    log::warn!("Order cancellation HTTP request failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        log::debug!("Cancelling all orders: {cmd:?}");

        let cache = self.core.cache();
        let open_orders = cache.orders_open(
            Some(&self.core.venue),
            Some(&cmd.instrument_id),
            None,
            None,
            Some(cmd.order_side),
        );

        if open_orders.is_empty() {
            log::debug!("No open orders to cancel for {:?}", cmd.instrument_id);
            return Ok(());
        }

        let symbol = cmd.instrument_id.symbol.to_string();
        let client_order_ids: Vec<String> = open_orders
            .iter()
            .map(|o| o.client_order_id().to_string())
            .collect();

        let http_client = self.http_client.clone();

        self.spawn_task("cancel_all_orders", async move {
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    log::warn!(
                        "Asset index not found for symbol {symbol}, ensure instruments are loaded"
                    );
                    return Ok(());
                }
            };

            let cancel_requests: Vec<_> = client_order_ids
                .iter()
                .map(|id| client_order_id_to_cancel_request_with_asset(id, asset))
                .collect();

            if cancel_requests.is_empty() {
                log::debug!("No valid cancel requests to send");
                return Ok(());
            }

            let action = HyperliquidExecAction::CancelByCloid {
                cancels: cancel_requests,
            };
            if let Err(e) = http_client.post_action_exec(&action).await {
                log::warn!("Failed to send cancel all orders request: {e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        log::debug!("Batch cancelling orders: {cmd:?}");

        if cmd.cancels.is_empty() {
            log::debug!("No orders to cancel in batch");
            return Ok(());
        }

        let cancel_info: Vec<(String, String)> = cmd
            .cancels
            .iter()
            .map(|c| {
                (
                    c.client_order_id.to_string(),
                    c.instrument_id.symbol.to_string(),
                )
            })
            .collect();

        let http_client = self.http_client.clone();

        self.spawn_task("batch_cancel_orders", async move {
            let mut cancel_requests = Vec::new();

            for (client_order_id, symbol) in &cancel_info {
                let asset = match http_client.get_asset_index(symbol) {
                    Some(a) => a,
                    None => {
                        log::warn!("Asset index not found for symbol {symbol}, skipping cancel");
                        continue;
                    }
                };
                cancel_requests.push(client_order_id_to_cancel_request_with_asset(
                    client_order_id,
                    asset,
                ));
            }

            if cancel_requests.is_empty() {
                log::warn!("No valid cancel requests in batch");
                return Ok(());
            }

            let action = HyperliquidExecAction::CancelByCloid {
                cancels: cancel_requests,
            };
            if let Err(e) = http_client.post_action_exec(&action).await {
                log::warn!("Failed to send batch cancel orders request: {e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
        log::debug!("Querying account: {cmd:?}");

        let runtime = get_runtime();
        runtime.block_on(async {
            if let Err(e) = self.refresh_account_state().await {
                log::warn!("Failed to query account state: {e}");
            }
        });

        Ok(())
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: {cmd:?}");

        let cache = self.core.cache();
        let venue_order_id = cache.venue_order_id(&cmd.client_order_id);

        let venue_order_id = match venue_order_id {
            Some(oid) => *oid,
            None => {
                log::warn!(
                    "No venue order ID found for client order {}",
                    cmd.client_order_id
                );
                return Ok(());
            }
        };
        drop(cache);

        let oid = match u64::from_str(venue_order_id.as_ref()) {
            Ok(id) => id,
            Err(e) => {
                log::warn!("Failed to parse venue order ID {venue_order_id}: {e}");
                return Ok(());
            }
        };

        let account_address = self.get_account_address()?;

        // Query order status via HTTP API
        // Note: The WebSocket connection is the authoritative source for order updates,
        // this is primarily for reconciliation or when WebSocket is unavailable
        let http_client = self.http_client.clone();
        let runtime = get_runtime();
        runtime.spawn(async move {
            match http_client.info_order_status(&account_address, oid).await {
                Ok(status) => {
                    log::debug!("Order status for oid {oid}: {status:?}");
                }
                Err(e) => {
                    log::warn!("Failed to query order status for oid {oid}: {e}");
                }
            }
        });

        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        log::info!("Connecting Hyperliquid execution client");

        // Ensure instruments are initialized
        self.ensure_instruments_initialized_async().await?;

        // Start WebSocket stream (connects and subscribes to user channels)
        self.start_ws_stream().await?;

        // Initialize account state and wait for it to be registered in cache
        self.refresh_account_state().await?;
        self.await_account_registered(30.0).await?;

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        log::info!("Disconnecting Hyperliquid execution client");

        // Disconnect WebSocket
        self.ws_client.disconnect().await?;

        // Abort any pending tasks
        self.abort_pending_tasks();

        self.core.set_disconnected();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // NOTE: Single order status report generation requires instrument cache integration.
        // The HTTP client methods and parsing functions are implemented and ready to use.
        // When implemented: query via info_order_status(), parse with parse_order_status_report_from_basic().
        log::warn!("generate_order_status_report not yet fully implemented");
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let account_address = self.get_account_address()?;

        let reports = self
            .http_client
            .request_order_status_reports(&account_address, cmd.instrument_id)
            .await
            .context("failed to generate order status reports")?;

        // Filter by open_only if specified
        let reports = if cmd.open_only {
            reports
                .into_iter()
                .filter(|r| r.order_status.is_open())
                .collect()
        } else {
            reports
        };

        // Filter by time range if specified
        let reports = match (cmd.start, cmd.end) {
            (Some(start), Some(end)) => reports
                .into_iter()
                .filter(|r| r.ts_last >= start && r.ts_last <= end)
                .collect(),
            (Some(start), None) => reports.into_iter().filter(|r| r.ts_last >= start).collect(),
            (None, Some(end)) => reports.into_iter().filter(|r| r.ts_last <= end).collect(),
            (None, None) => reports,
        };

        log::info!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let account_address = self.get_account_address()?;

        let reports = self
            .http_client
            .request_fill_reports(&account_address, cmd.instrument_id)
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

        log::info!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let account_address = self.get_account_address()?;

        let reports = self
            .http_client
            .request_position_status_reports(&account_address, cmd.instrument_id)
            .await
            .context("failed to generate position status reports")?;

        log::info!("Generated {} position status reports", reports.len());
        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = self.clock.get_time_ns();

        let order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_init,
            true, // open_only
            None,
            None,
            None,
            None,
            None,
        );
        let fill_cmd =
            GenerateFillReports::new(UUID4::new(), ts_init, None, None, None, None, None, None);
        let position_cmd =
            GeneratePositionStatusReports::new(UUID4::new(), ts_init, None, None, None, None, None);

        let order_reports = self.generate_order_status_reports(&order_cmd).await?;
        let mut fill_reports = self.generate_fill_reports(fill_cmd).await?;
        let position_reports = self.generate_position_status_reports(&position_cmd).await?;

        // Apply lookback filter to fills only (positions are current state,
        // and open orders must always be included for correct reconciliation)
        if let Some(mins) = lookback_mins {
            let cutoff_ns = ts_init
                .as_u64()
                .saturating_sub(mins.saturating_mul(60).saturating_mul(1_000_000_000));
            let cutoff = UnixNanos::from(cutoff_ns);

            fill_reports.retain(|r| r.ts_event >= cutoff);
        }

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            ts_init,
            None,
        );
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        log::info!(
            "Generated mass status: {} orders, {} fills, {} positions",
            mass_status.order_reports().len(),
            mass_status.fill_reports().len(),
            mass_status.position_reports().len(),
        );

        Ok(Some(mass_status))
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

        // Use vault address for WS subscriptions when vault trading,
        // otherwise order/fill updates for the vault will be missed
        let subscription_address = self
            .config
            .vault_address
            .as_ref()
            .unwrap_or(&user_address)
            .clone();

        let mut ws_client = self.ws_client.clone();

        let instruments = self
            .http_client
            .request_instruments()
            .await
            .unwrap_or_default();

        for instrument in instruments {
            ws_client.cache_instrument(instrument);
        }

        // Connect and subscribe before spawning the event loop
        ws_client.connect().await?;
        ws_client
            .subscribe_order_updates(&subscription_address)
            .await?;
        ws_client
            .subscribe_user_events(&subscription_address)
            .await?;
        log::info!("Subscribed to Hyperliquid execution updates for {subscription_address}");

        // Transfer task handle to original so disconnect() can await it
        if let Some(handle) = ws_client.take_task_handle() {
            self.ws_client.set_task_handle(handle);
        }

        let emitter = self.emitter.clone();
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            // Deferred cloid cleanup for FILLED orders. We keep the
            // mapping alive until a fill arrives after the FILLED
            // status so partial fills don't lose client_order_id.
            // Auto-eviction at capacity bounds orphaned entries.
            let mut pending_filled: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

            loop {
                let event = ws_client.next_event().await;

                match event {
                    Some(msg) => {
                        match msg {
                            NautilusWsMessage::ExecutionReports(reports) => {
                                let mut immediate_cleanup: Vec<ClientOrderId> = Vec::new();

                                for report in &reports {
                                    if let ExecutionReport::Order(order_report) = report
                                        && let Some(id) = order_report.client_order_id
                                        && !order_report.order_status.is_open()
                                    {
                                        if order_report.order_status == OrderStatus::Filled {
                                            pending_filled.add(id);
                                        } else {
                                            immediate_cleanup.push(id);
                                        }
                                    }
                                }

                                for report in &reports {
                                    if let ExecutionReport::Fill(fill_report) = report
                                        && let Some(id) = fill_report.client_order_id
                                        && pending_filled.contains(&id)
                                    {
                                        pending_filled.remove(&id);
                                        immediate_cleanup.push(id);
                                    }
                                }

                                for report in reports {
                                    match report {
                                        ExecutionReport::Order(r) => {
                                            emitter.send_order_status_report(r);
                                        }
                                        ExecutionReport::Fill(r) => {
                                            emitter.send_fill_report(r);
                                        }
                                    }
                                }

                                for id in immediate_cleanup {
                                    let cloid = Cloid::from_client_order_id(id);
                                    ws_client.remove_cloid_mapping(&Ustr::from(&cloid.to_hex()));
                                }
                            }
                            // Reconnected is handled by WS client internally
                            // (resubscribe_all) and never forwarded here
                            NautilusWsMessage::Reconnected => {}
                            NautilusWsMessage::Error(e) => {
                                log::error!("WebSocket error: {e}");
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
                        log::warn!("WebSocket next_event returned None");
                        break;
                    }
                }
            }
        });

        *self.ws_stream_handle.lock().expect(MUTEX_POISONED) = Some(handle);
        log::info!("Hyperliquid WebSocket execution stream started");
        Ok(())
    }
}
