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
    sync::{Arc, Mutex},
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
    MUTEX_POISONED, Params, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    orders::{Order, any::OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::{HYPERLIQUID_VENUE, NAUTILUS_BUILDER_ADDRESS},
        credential::Secrets,
        parse::{
            clamp_price_to_precision, client_order_id_to_cancel_request_with_asset,
            derive_limit_from_trigger, derive_market_order_price, extract_error_message,
            extract_inner_error, extract_inner_errors, normalize_price,
            order_to_hyperliquid_request_with_asset, parse_combined_account_balances_and_margins,
            round_to_sig_figs,
        },
    },
    config::HyperliquidExecClientConfig,
    http::{
        client::HyperliquidHttpClient,
        models::{
            ClearinghouseState, Cloid, HyperliquidExecAction, HyperliquidExecBuilderFee,
            HyperliquidExecGrouping, HyperliquidExecModifyOrderRequest, HyperliquidExecOrderKind,
            SpotClearinghouseState,
        },
    },
    websocket::{
        ExecutionReport, NautilusWsMessage,
        client::HyperliquidWebSocketClient,
        dispatch::{
            DispatchOutcome, OrderIdentity, WsDispatchState, dispatch_fill_report,
            dispatch_order_status_report,
        },
    },
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
    ws_dispatch_state: Arc<WsDispatchState>,
}

impl HyperliquidExecutionClient {
    /// Returns a reference to the configuration.
    pub fn config(&self) -> &HyperliquidExecClientConfig {
        &self.config
    }

    /// Returns a reference to the shared WebSocket dispatch state.
    ///
    /// Exposes the identity map, pending-modify markers, and cached venue
    /// order ids used by the two-tier dispatch contract. The state is
    /// read-write via an [`Arc`]; callers must not mutate it directly, but
    /// it is useful for inspection in tests and for live debugging.
    #[must_use]
    pub fn ws_dispatch_state(&self) -> &Arc<WsDispatchState> {
        &self.ws_dispatch_state
    }

    /// Returns `true` when every background task spawned via `spawn_task`
    /// has completed.
    ///
    /// Used in tests to wait for submit / modify / cancel HTTP round-trips
    /// that fire on the runtime to finish before asserting on dispatch
    /// state, avoiding bare `sleep` calls when a negative condition needs
    /// to be checked after the spawned work is done.
    #[allow(
        clippy::missing_panics_doc,
        reason = "pending_tasks mutex poisoning is not expected"
    )]
    #[must_use]
    pub fn pending_tasks_all_finished(&self) -> bool {
        let tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        tasks.iter().all(|h| h.is_finished())
    }

    fn resolve_slippage_bps(&self, params: Option<&Params>) -> u32 {
        params
            .and_then(|p| p.get_u64("market_order_slippage_bps"))
            .map_or(self.config.market_order_slippage_bps, |v| v as u32)
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
        let secrets = Secrets::resolve(
            config.private_key.as_deref(),
            config.vault_address.as_deref(),
            config.environment,
        )
        .context("Hyperliquid execution client requires private key")?;

        let mut http_client = HyperliquidHttpClient::with_secrets(
            &secrets,
            config.http_timeout_secs,
            config.proxy_url.clone(),
        )
        .context("failed to create Hyperliquid HTTP client")?;

        http_client.set_account_id(core.account_id);
        http_client.set_account_address(config.account_address.clone());
        http_client.set_normalize_prices(config.normalize_prices);
        http_client.set_market_order_slippage_bps(config.market_order_slippage_bps);

        // Apply URL overrides from config (used for testing with mock servers)
        if let Some(url) = &config.base_url_http {
            http_client.set_base_info_url(url.clone());
        }

        if let Some(url) = &config.base_url_exchange {
            http_client.set_base_exchange_url(url.clone());
        }

        let ws_url = config.base_url_ws.clone();
        let ws_client = HyperliquidWebSocketClient::new(
            ws_url,
            config.environment,
            Some(core.account_id),
            config.transport_backend,
            config.proxy_url.clone(),
        );

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
            ws_dispatch_state: Arc::new(WsDispatchState::new()),
        })
    }

    fn register_order_identity(&self, order: &OrderAny) {
        register_order_identity_into(&self.ws_dispatch_state, order);
    }

    async fn ensure_instruments_initialized_async(&self) -> anyhow::Result<()> {
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
                self.http_client.cache_instrument(instrument);
            }
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_address = self.get_account_address()?;

        let (perp_state, spot_state) = self
            .fetch_combined_clearinghouse_state(&account_address)
            .await?;

        log::debug!(
            "Received clearinghouse state: cross_margin_summary={:?}, asset_positions={}, spot_balances={}",
            perp_state.cross_margin_summary,
            perp_state.asset_positions.len(),
            spot_state.balances.len(),
        );

        let (balances, margins) =
            parse_combined_account_balances_and_margins(&perp_state, &spot_state)
                .context("failed to parse combined account balances and margins")?;

        // Emit even when both sides are empty so the account registers for
        // await_account_registered on unfunded wallets.
        let ts_event = self.clock.get_time_ns();
        self.emitter
            .emit_account_state(balances, margins, true, ts_event);

        log::info!("Account state updated successfully");
        Ok(())
    }

    async fn fetch_combined_clearinghouse_state(
        &self,
        account_address: &str,
    ) -> anyhow::Result<(ClearinghouseState, SpotClearinghouseState)> {
        let perp_json = self
            .http_client
            .info_clearinghouse_state(account_address)
            .await
            .context("failed to fetch clearinghouse state")?;
        let perp_state: ClearinghouseState = serde_json::from_value(perp_json)
            .context("failed to deserialize clearinghouse state")?;

        let spot_json = self
            .http_client
            .info_spot_clearinghouse_state(account_address)
            .await
            .context("failed to fetch spot clearinghouse state")?;
        let spot_state: SpotClearinghouseState = serde_json::from_value(spot_json)
            .context("failed to deserialize spot clearinghouse state")?;

        Ok((perp_state, spot_state))
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
        if let Some(addr) = &self.config.account_address {
            return Ok(addr.clone());
        }

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
            "Started: client_id={}, account_id={}, environment={:?}, vault_address={:?}, proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment,
            self.config.vault_address,
            self.config.proxy_url,
        );

        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        log::info!("Stopping Hyperliquid execution client");

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.ws_client.abort();

        self.core.set_disconnected();
        self.core.set_stopped();

        log::info!("Hyperliquid execution client stopped");
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
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
        let price_decimals = http_client.get_price_precision(&symbol).unwrap_or(2);
        let slippage_bps = self.resolve_slippage_bps(cmd.params.as_ref());
        let mut hyperliquid_order = match order_to_hyperliquid_request_with_asset(
            &order,
            asset,
            price_decimals,
            self.config.normalize_prices,
            slippage_bps,
        ) {
            Ok(req) => req,
            Err(e) => {
                self.emitter
                    .emit_order_denied(&order, &format!("Order conversion failed: {e}"));
                return Ok(());
            }
        };

        // Market orders need a limit price derived from the cached quote
        if order.order_type() == OrderType::Market {
            let instrument_id = order.instrument_id();
            let cache = self.core.cache();
            match cache.quote(&instrument_id) {
                Some(quote) => {
                    let is_buy = order.order_side() == OrderSide::Buy;
                    hyperliquid_order.price =
                        derive_market_order_price(quote, is_buy, price_decimals, slippage_bps);
                }
                None => {
                    self.emitter.emit_order_denied(
                        &order,
                        &format!(
                            "No cached quote for {instrument_id}: \
                             subscribe to quote data before submitting market orders"
                        ),
                    );
                    return Ok(());
                }
            }
        }

        log::info!(
            "Submitting order: id={}, type={:?}, side={:?}, price={}, size={}, kind={:?}",
            order.client_order_id(),
            order.order_type(),
            order.order_side(),
            hyperliquid_order.price,
            hyperliquid_order.size,
            hyperliquid_order.kind,
        );

        // Cache cloid mapping before emitting submitted so WS handler
        // can resolve order/fill reports back to this client_order_id
        let cloid = Cloid::from_client_order_id(order.client_order_id());
        self.ws_client
            .cache_cloid_mapping(Ustr::from(&cloid.to_hex()), order.client_order_id());

        self.register_order_identity(&order);

        self.emitter.emit_order_submitted(&order);

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ws_client = self.ws_client.clone();
        let cloid_hex = Ustr::from(&cloid.to_hex());
        let dispatch_state = self.ws_dispatch_state.clone();
        let client_order_id = order.client_order_id();

        // Vaults cannot approve builder fees, so skip builder attribution
        // for vault orders to avoid "Builder fee has not been approved" rejection
        let builder = if self.http_client.has_vault_address() {
            None
        } else {
            Some(HyperliquidExecBuilderFee {
                address: NAUTILUS_BUILDER_ADDRESS.to_string(),
                fee_tenths_bp: 0,
            })
        };

        self.spawn_task("submit_order", async move {
            let action = HyperliquidExecAction::Order {
                orders: vec![hyperliquid_order],
                grouping: HyperliquidExecGrouping::Na,
                builder,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        if let Some(inner_error) = extract_inner_error(&response) {
                            log::warn!("Order submission rejected by exchange: {inner_error}");
                            let ts = clock.get_time_ns();
                            emitter.emit_order_rejected(&order, &inner_error, ts, false);
                            ws_client.remove_cloid_mapping(&cloid_hex);
                            dispatch_state.cleanup_terminal(&client_order_id);
                        } else {
                            log::info!("Order submitted successfully: {response:?}");
                        }
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order submission rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();
                        emitter.emit_order_rejected(&order, &error_msg, ts, false);
                        ws_client.remove_cloid_mapping(&cloid_hex);
                        dispatch_state.cleanup_terminal(&client_order_id);
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

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        log::debug!(
            "Submitting order list with {} orders",
            cmd.order_list.client_order_ids.len()
        );

        let http_client = self.http_client.clone();
        let slippage_bps = self.resolve_slippage_bps(cmd.params.as_ref());

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

            let price_decimals = http_client.get_price_precision(&symbol).unwrap_or(2);

            match order_to_hyperliquid_request_with_asset(
                order,
                asset,
                price_decimals,
                self.config.normalize_prices,
                slippage_bps,
            ) {
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

        let grouping = determine_order_list_grouping(&valid_orders);
        log::info!("Order list grouping: {grouping:?}");

        for order in &valid_orders {
            let cloid = Cloid::from_client_order_id(order.client_order_id());
            self.ws_client
                .cache_cloid_mapping(Ustr::from(&cloid.to_hex()), order.client_order_id());
            self.register_order_identity(order);
            self.emitter.emit_order_submitted(order);
        }

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ws_client = self.ws_client.clone();
        let dispatch_state = self.ws_dispatch_state.clone();
        let cloid_hexes: Vec<Ustr> = valid_orders
            .iter()
            .map(|o| Ustr::from(&Cloid::from_client_order_id(o.client_order_id()).to_hex()))
            .collect();
        let client_order_ids: Vec<ClientOrderId> =
            valid_orders.iter().map(|o| o.client_order_id()).collect();

        let builder = if self.http_client.has_vault_address() {
            None
        } else {
            Some(HyperliquidExecBuilderFee {
                address: NAUTILUS_BUILDER_ADDRESS.to_string(),
                fee_tenths_bp: 0,
            })
        };

        self.spawn_task("submit_order_list", async move {
            let action = HyperliquidExecAction::Order {
                orders: hyperliquid_orders,
                grouping,
                builder,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        let inner_errors = extract_inner_errors(&response);

                        // For grouped orders (NormalTpsl/PositionTpsl), the
                        // exchange returns a single status for the whole group
                        // rather than one per order. If fewer statuses than
                        // orders are returned, broadcast the first error (if
                        // any) to all orders, or treat all as successful.
                        if inner_errors.len() < valid_orders.len() {
                            if let Some(error_msg) = inner_errors.iter().find_map(|e| e.as_ref()) {
                                let ts = clock.get_time_ns();

                                for ((order, cloid_hex), cid) in valid_orders
                                    .iter()
                                    .zip(cloid_hexes.iter())
                                    .zip(client_order_ids.iter())
                                {
                                    log::warn!(
                                        "Order {} rejected by exchange: {error_msg}",
                                        order.client_order_id(),
                                    );
                                    emitter.emit_order_rejected(order, error_msg, ts, false);
                                    ws_client.remove_cloid_mapping(cloid_hex);
                                    dispatch_state.cleanup_terminal(cid);
                                }
                            } else {
                                log::info!("Order list submitted successfully: {response:?}");
                            }
                        } else if inner_errors.iter().any(|e| e.is_some()) {
                            let ts = clock.get_time_ns();

                            for (i, error) in inner_errors.iter().enumerate() {
                                if let Some(error_msg) = error {
                                    if let Some(order) = valid_orders.get(i) {
                                        log::warn!(
                                            "Order {} rejected by exchange: {error_msg}",
                                            order.client_order_id(),
                                        );
                                        emitter.emit_order_rejected(order, error_msg, ts, false);
                                    }

                                    if let Some(cloid_hex) = cloid_hexes.get(i) {
                                        ws_client.remove_cloid_mapping(cloid_hex);
                                    }

                                    if let Some(cid) = client_order_ids.get(i) {
                                        dispatch_state.cleanup_terminal(cid);
                                    }
                                }
                            }
                        } else {
                            log::info!("Order list submitted successfully: {response:?}");
                        }
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order list submission rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();
                        for order in &valid_orders {
                            emitter.emit_order_rejected(order, &error_msg, ts, false);
                        }

                        for cloid_hex in &cloid_hexes {
                            ws_client.remove_cloid_mapping(cloid_hex);
                        }

                        for cid in &client_order_ids {
                            dispatch_state.cleanup_terminal(cid);
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

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        log::debug!("Modifying order: {cmd:?}");

        let venue_order_id = match cmd.venue_order_id {
            Some(id) => id,
            None => {
                let reason = "venue_order_id is required for modify";
                log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
                self.emitter.emit_order_modify_rejected_event(
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    None,
                    reason,
                    self.clock.get_time_ns(),
                );
                return Ok(());
            }
        };

        let oid: u64 = match venue_order_id.as_str().parse() {
            Ok(id) => id,
            Err(e) => {
                let reason = format!("Failed to parse venue_order_id '{venue_order_id}': {e}");
                log::warn!("{reason}");
                self.emitter.emit_order_modify_rejected_event(
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    Some(venue_order_id),
                    &reason,
                    self.clock.get_time_ns(),
                );
                return Ok(());
            }
        };

        // Look up cached order to get side, reduce_only, post_only, TIF
        let order = match self.core.cache().order(&cmd.client_order_id).cloned() {
            Some(o) => o,
            None => {
                let reason = "order not found in cache";
                log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
                self.emitter.emit_order_modify_rejected_event(
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    Some(venue_order_id),
                    reason,
                    self.clock.get_time_ns(),
                );
                return Ok(());
            }
        };

        let http_client = self.http_client.clone();
        let symbol = cmd.instrument_id.symbol.to_string();
        let should_normalize = self.config.normalize_prices;
        let slippage_bps = self.resolve_slippage_bps(cmd.params.as_ref());

        let quantity = cmd.quantity.unwrap_or(order.leaves_qty());
        let price_decimals = http_client.get_price_precision(&symbol).unwrap_or(2);
        let asset = match http_client.get_asset_index(&symbol) {
            Some(a) => a,
            None => {
                log::warn!(
                    "Asset index not found for symbol {symbol}, ensure instruments are loaded",
                );
                return Ok(());
            }
        };

        // Build base request from cached order (derives slippage-adjusted
        // limit for trigger-market types like StopMarket/MarketIfTouched)
        let hyperliquid_order = match order_to_hyperliquid_request_with_asset(
            &order,
            asset,
            price_decimals,
            should_normalize,
            slippage_bps,
        ) {
            Ok(mut req) => {
                // Only override price when explicitly provided
                if let Some(p) = cmd.price.or(order.price()) {
                    let price_dec = p.as_decimal();
                    req.price = if should_normalize {
                        normalize_price(price_dec, price_decimals).normalize()
                    } else {
                        price_dec.normalize()
                    };
                } else if let Some(tp) = cmd.trigger_price {
                    // Trigger changed but no explicit price: re-derive the
                    // slippage-adjusted limit from the new trigger
                    let is_buy = order.order_side() == OrderSide::Buy;
                    let base = tp.as_decimal().normalize();
                    let derived = derive_limit_from_trigger(base, is_buy, slippage_bps);
                    let sig_rounded = round_to_sig_figs(derived, 5);
                    req.price =
                        clamp_price_to_precision(sig_rounded, price_decimals, is_buy).normalize();
                }
                // else: keep the derived price from order_to_hyperliquid_request

                req.size = quantity.as_decimal().normalize();

                // Update trigger_px if the command provides a new trigger
                if let (Some(tp), HyperliquidExecOrderKind::Trigger { trigger }) =
                    (cmd.trigger_price, &mut req.kind)
                {
                    let tp_dec = tp.as_decimal();
                    trigger.trigger_px = if should_normalize {
                        normalize_price(tp_dec, price_decimals).normalize()
                    } else {
                        tp_dec.normalize()
                    };
                }

                req
            }
            Err(e) => {
                log::warn!("Order conversion failed for modify: {e}");
                return Ok(());
            }
        };

        let dispatch_state = self.ws_dispatch_state.clone();
        let client_order_id = cmd.client_order_id;
        let old_venue_order_id = venue_order_id;

        self.spawn_task("modify_order", async move {
            let action = HyperliquidExecAction::Modify {
                modify: HyperliquidExecModifyOrderRequest {
                    oid,
                    order: hyperliquid_order,
                },
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        if let Some(inner_error) = extract_inner_error(&response) {
                            log::warn!("Order modification rejected by exchange: {inner_error}");
                        } else {
                            // Mark the old venue_order_id as in-flight only
                            // after a confirmed HTTP success. A failed modify
                            // never leaves stale race state behind, so the
                            // cancel-before-accept branch never fires on a
                            // cancel following an independent failed modify.
                            dispatch_state.mark_pending_modify(client_order_id, old_venue_order_id);
                            log::info!("Order modified successfully: {response:?}");
                        }
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Order modification rejected by exchange: {error_msg}");
                    }
                }
                Err(e) => {
                    log::warn!("Order modification HTTP request failed: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        log::debug!("Cancelling order: {cmd:?}");

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let client_order_id = cmd.client_order_id;
        let client_order_id_str = cmd.client_order_id.to_string();
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let venue_order_id = cmd.venue_order_id;
        let symbol = cmd.instrument_id.symbol.to_string();

        self.spawn_task("cancel_order", async move {
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Asset index not found for symbol {symbol}"),
                        clock.get_time_ns(),
                    );
                    return Ok(());
                }
            };

            let cancel_request =
                client_order_id_to_cancel_request_with_asset(&client_order_id_str, asset);
            let action = HyperliquidExecAction::CancelByCloid {
                cancels: vec![cancel_request],
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        if let Some(inner_error) = extract_inner_error(&response) {
                            emitter.emit_order_cancel_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                venue_order_id,
                                &inner_error,
                                clock.get_time_ns(),
                            );
                        } else {
                            log::info!("Order cancelled successfully: {response:?}");
                        }
                    } else {
                        emitter.emit_order_cancel_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            venue_order_id,
                            &extract_error_message(&response),
                            clock.get_time_ns(),
                        );
                    }
                }
                Err(e) => {
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("Cancel HTTP request failed: {e}"),
                        clock.get_time_ns(),
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
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
        let instrument_id = cmd.instrument_id;
        let strategy_id = cmd.strategy_id;
        let entries: Vec<CancelEntry> = open_orders
            .iter()
            .map(|o| CancelEntry {
                strategy_id,
                instrument_id,
                client_order_id: o.client_order_id(),
                venue_order_id: o.venue_order_id(),
                symbol: symbol.clone(),
            })
            .collect();

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_all_orders", async move {
            let asset = match http_client.get_asset_index(&symbol) {
                Some(a) => a,
                None => {
                    let reason = format!("Asset index not found for symbol {symbol}");
                    log::warn!("{reason}");
                    let ts = clock.get_time_ns();

                    for entry in &entries {
                        emitter.emit_order_cancel_rejected_event(
                            entry.strategy_id,
                            entry.instrument_id,
                            entry.client_order_id,
                            entry.venue_order_id,
                            &reason,
                            ts,
                        );
                    }
                    return Ok(());
                }
            };

            let cancel_requests: Vec<_> = entries
                .iter()
                .map(|e| {
                    client_order_id_to_cancel_request_with_asset(e.client_order_id.as_ref(), asset)
                })
                .collect();

            if cancel_requests.is_empty() {
                return Ok(());
            }

            let action = HyperliquidExecAction::CancelByCloid {
                cancels: cancel_requests,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        let inner_errors = extract_inner_errors(&response);
                        let ts = clock.get_time_ns();

                        if inner_errors.is_empty() {
                            log::info!("Cancel-all submitted successfully: {response:?}");
                        } else {
                            for (i, entry) in entries.iter().enumerate() {
                                if let Some(Some(error_msg)) = inner_errors.get(i) {
                                    log::warn!(
                                        "Cancel for {} rejected by exchange: {error_msg}",
                                        entry.client_order_id,
                                    );
                                    emitter.emit_order_cancel_rejected_event(
                                        entry.strategy_id,
                                        entry.instrument_id,
                                        entry.client_order_id,
                                        entry.venue_order_id,
                                        error_msg,
                                        ts,
                                    );
                                }
                            }
                        }
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Cancel-all rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();

                        for entry in &entries {
                            emitter.emit_order_cancel_rejected_event(
                                entry.strategy_id,
                                entry.instrument_id,
                                entry.client_order_id,
                                entry.venue_order_id,
                                &error_msg,
                                ts,
                            );
                        }
                    }
                }
                Err(e) => {
                    let reason = format!("Cancel-all HTTP request failed: {e}");
                    log::warn!("{reason}");
                    let ts = clock.get_time_ns();

                    for entry in &entries {
                        emitter.emit_order_cancel_rejected_event(
                            entry.strategy_id,
                            entry.instrument_id,
                            entry.client_order_id,
                            entry.venue_order_id,
                            &reason,
                            ts,
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        log::debug!("Batch cancelling orders: {cmd:?}");

        if cmd.cancels.is_empty() {
            log::debug!("No orders to cancel in batch");
            return Ok(());
        }

        let entries: Vec<CancelEntry> = cmd
            .cancels
            .iter()
            .map(|c| CancelEntry {
                strategy_id: c.strategy_id,
                instrument_id: c.instrument_id,
                client_order_id: c.client_order_id,
                venue_order_id: c.venue_order_id,
                symbol: c.instrument_id.symbol.to_string(),
            })
            .collect();

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            let mut cancel_requests = Vec::new();
            let mut sent_entries: Vec<&CancelEntry> = Vec::new();

            for entry in &entries {
                let asset = match http_client.get_asset_index(&entry.symbol) {
                    Some(a) => a,
                    None => {
                        let reason = format!("Asset index not found for symbol {}", entry.symbol);
                        log::warn!("{reason}, skipping cancel for {}", entry.client_order_id);
                        emitter.emit_order_cancel_rejected_event(
                            entry.strategy_id,
                            entry.instrument_id,
                            entry.client_order_id,
                            entry.venue_order_id,
                            &reason,
                            clock.get_time_ns(),
                        );
                        continue;
                    }
                };
                cancel_requests.push(client_order_id_to_cancel_request_with_asset(
                    entry.client_order_id.as_ref(),
                    asset,
                ));
                sent_entries.push(entry);
            }

            if cancel_requests.is_empty() {
                log::warn!("No valid cancel requests in batch");
                return Ok(());
            }

            let action = HyperliquidExecAction::CancelByCloid {
                cancels: cancel_requests,
            };

            match http_client.post_action_exec(&action).await {
                Ok(response) => {
                    if response.is_ok() {
                        let inner_errors = extract_inner_errors(&response);
                        let ts = clock.get_time_ns();

                        if inner_errors.is_empty() {
                            log::info!("Batch cancel submitted successfully: {response:?}");
                        } else {
                            for (i, entry) in sent_entries.iter().enumerate() {
                                if let Some(Some(error_msg)) = inner_errors.get(i) {
                                    log::warn!(
                                        "Cancel for {} rejected by exchange: {error_msg}",
                                        entry.client_order_id,
                                    );
                                    emitter.emit_order_cancel_rejected_event(
                                        entry.strategy_id,
                                        entry.instrument_id,
                                        entry.client_order_id,
                                        entry.venue_order_id,
                                        error_msg,
                                        ts,
                                    );
                                }
                            }
                        }
                    } else {
                        let error_msg = extract_error_message(&response);
                        log::warn!("Batch cancel rejected by exchange: {error_msg}");
                        let ts = clock.get_time_ns();

                        for entry in &sent_entries {
                            emitter.emit_order_cancel_rejected_event(
                                entry.strategy_id,
                                entry.instrument_id,
                                entry.client_order_id,
                                entry.venue_order_id,
                                &error_msg,
                                ts,
                            );
                        }
                    }
                }
                Err(e) => {
                    let reason = format!("Batch cancel HTTP request failed: {e}");
                    log::warn!("{reason}");
                    let ts = clock.get_time_ns();

                    for entry in &sent_entries {
                        emitter.emit_order_cancel_rejected_event(
                            entry.strategy_id,
                            entry.instrument_id,
                            entry.client_order_id,
                            entry.venue_order_id,
                            &reason,
                            ts,
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_address = self.get_account_address()?;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_account", async move {
            let perp_json = http_client
                .info_clearinghouse_state(&account_address)
                .await
                .context("failed to fetch clearinghouse state")?;

            let perp_state: ClearinghouseState = serde_json::from_value(perp_json)
                .context("failed to deserialize clearinghouse state")?;

            let spot_json = http_client
                .info_spot_clearinghouse_state(&account_address)
                .await
                .context("failed to fetch spot clearinghouse state")?;
            let spot_state: SpotClearinghouseState = serde_json::from_value(spot_json)
                .context("failed to deserialize spot clearinghouse state")?;

            let (balances, margins) =
                parse_combined_account_balances_and_margins(&perp_state, &spot_state)
                    .context("failed to parse combined account balances and margins")?;
            let ts_event = clock.get_time_ns();
            emitter.emit_account_state(balances, margins, true, ts_event);

            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: {cmd:?}");

        let client_order_id = cmd.client_order_id;
        let venue_order_id = match cmd.venue_order_id {
            Some(voi) => Some(voi),
            None => self.core.cache().venue_order_id(&client_order_id).copied(),
        };

        let account_address = self.get_account_address()?;
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();

        self.spawn_task("query_order", async move {
            // Search open orders by cloid first so modify/cancel-replace
            // resolves to the live replacement rather than a stale cached oid.
            // Request errors here are logged and the oid fallback is still tried;
            // a transient frontendOpenOrders failure must not abort the whole query.
            match http_client
                .request_order_status_report_by_client_order_id(&account_address, &client_order_id)
                .await
            {
                Ok(Some(report)) => {
                    log::info!("Queried order status for {client_order_id}");
                    emitter.send_order_status_report(report);
                    return Ok(());
                }
                Ok(None) => {}
                Err(e) => {
                    log::warn!(
                        "Failed to query order status for {client_order_id}: {e}; falling back to oid lookup"
                    );
                }
            }

            let Some(venue_order_id) = venue_order_id else {
                log::info!("No order status report found for {client_order_id}");
                return Ok(());
            };

            let oid: u64 = match venue_order_id.as_str().parse() {
                Ok(oid) => oid,
                Err(e) => {
                    log::warn!("Failed to parse venue order ID {venue_order_id}: {e}");
                    return Ok(());
                }
            };

            match http_client
                .request_order_status_report(&account_address, oid)
                .await
            {
                Ok(Some(report)) => {
                    log::info!("Queried order status for oid {oid}");
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::info!("No order status report found for oid {oid}");
                }
                Err(e) => {
                    log::warn!("Failed to query order status for oid {oid}: {e}");
                }
            }

            Ok(())
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

        // Post-WS setup: if any step fails, tear down WS before returning
        let post_ws = async {
            self.refresh_account_state().await?;
            self.await_account_registered(30.0).await?;

            Ok::<(), anyhow::Error>(())
        };

        if let Err(e) = post_ws.await {
            log::warn!("Connect failed after WS started, tearing down: {e}");
            let _ = self.ws_client.disconnect().await;
            self.abort_pending_tasks();
            return Err(e);
        }

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
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let account_address = self.get_account_address()?;

        if cmd.venue_order_id.is_none() && cmd.client_order_id.is_none() {
            log::warn!(
                "Cannot generate order status report without venue_order_id or client_order_id"
            );
            return Ok(None);
        }

        // Search open orders by cloid first when supplied. Hyperliquid modify
        // produces a new venue oid while preserving cloid, so a cached oid can
        // point at the canceled leg rather than the live replacement.
        if let Some(client_order_id) = &cmd.client_order_id
            && let Some(report) = self
                .http_client
                .request_order_status_report_by_client_order_id(&account_address, client_order_id)
                .await
                .context("failed to generate order status report by client_order_id")?
        {
            log::info!("Generated order status report for {client_order_id}");
            return Ok(Some(report));
        }

        let oid = match &cmd.venue_order_id {
            Some(venue_order_id) => venue_order_id
                .as_str()
                .parse::<u64>()
                .context("failed to parse venue_order_id as oid")?,
            None => match &cmd.client_order_id {
                Some(client_order_id) => {
                    let cached_oid: Option<u64> = self
                        .core
                        .cache()
                        .venue_order_id(client_order_id)
                        .and_then(|v| v.as_str().parse::<u64>().ok());

                    match cached_oid {
                        Some(oid) => oid,
                        None => {
                            log::info!("No order status report found for {client_order_id}");
                            return Ok(None);
                        }
                    }
                }
                None => unreachable!("cmd must carry at least one identifier"),
            },
        };

        let report = self
            .http_client
            .request_order_status_report(&account_address, oid)
            .await
            .context("failed to generate order status report")?;

        if report.is_some() {
            log::info!("Generated order status report for oid {oid}");
        } else {
            log::info!("No order status report found for oid {oid}");
        }
        Ok(report)
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

        // request_position_status_reports already merges spot holdings
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

        // Use account_address (agent wallet) or vault address for WS subscriptions,
        // otherwise order/fill updates will be missed
        let subscription_address = self
            .config
            .account_address
            .as_ref()
            .or(self.config.vault_address.as_ref())
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
        let dispatch_state = self.ws_dispatch_state.clone();
        let clock = self.clock;
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            // Cloids for external / untracked orders that reach a terminal
            // state: we evict their mapping immediately so long-running
            // sessions do not leak. Tracked orders clear their own cloid
            // mapping from the dispatch `cleanup_terminal` path below.
            //
            // For a tracked order that hits a status-only `FILLED` marker
            // without an accompanying fill, we defer the cloid cleanup until
            // the matching `FillReport` arrives so partial fills do not lose
            // their `client_order_id` link. The bounded FIFO cache keeps
            // orphaned entries from growing unbounded.
            let mut pending_filled_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

            loop {
                let event = ws_client.next_event().await;

                match event {
                    Some(msg) => match msg {
                        NautilusWsMessage::ExecutionReports(reports) => {
                            for report in reports {
                                handle_execution_report(
                                    report,
                                    &dispatch_state,
                                    &emitter,
                                    &ws_client,
                                    &mut pending_filled_cloids,
                                    clock.get_time_ns(),
                                );
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
                        | NautilusWsMessage::Depth10(_)
                        | NautilusWsMessage::Candle(_)
                        | NautilusWsMessage::MarkPrice(_)
                        | NautilusWsMessage::IndexPrice(_)
                        | NautilusWsMessage::FundingRate(_) => {}
                    },
                    None => {
                        log::debug!("WebSocket next_event returned None, stream closed");
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

/// Registers an order's identity in the dispatch state so its subsequent
/// WebSocket lifecycle can route through the typed-event path.
///
/// Quote-quantity orders submit a quote amount (e.g. 100 USD) but the venue
struct CancelEntry {
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    symbol: String,
}

/// reports fills in base units. Comparing those two when deciding whether an
/// order is fully filled would leave the order stuck "open" forever, so they
/// flow through the untracked path and the engine reconciles them from
/// status reports instead.
fn register_order_identity_into(state: &WsDispatchState, order: &OrderAny) {
    if order.is_quote_quantity() {
        return;
    }
    state.register_identity(
        order.client_order_id(),
        OrderIdentity {
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
            quantity: order.quantity(),
            price: order.price(),
        },
    );
}

/// Routes a single execution report through the two-tier dispatch.
///
/// For tracked orders this emits typed `OrderEventAny` events via the
/// dispatch module; external / untracked orders fall back to the raw report
/// so the engine can reconcile. Cloid-mapping cleanup is handled here so
/// long-running sessions do not leak mapping entries.
fn handle_execution_report(
    report: ExecutionReport,
    dispatch_state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    ws_client: &HyperliquidWebSocketClient,
    pending_filled_cloids: &mut FifoCache<ClientOrderId, 10_000>,
    ts_init: UnixNanos,
) {
    match report {
        ExecutionReport::Order(order_report) => {
            let is_filled_marker = matches!(order_report.order_status, OrderStatus::Filled);
            let is_open = order_report.order_status.is_open();
            let client_order_id = order_report.client_order_id;

            let outcome =
                dispatch_order_status_report(&order_report, dispatch_state, emitter, ts_init);

            if outcome == DispatchOutcome::External {
                emitter.send_order_status_report(order_report);
            }

            // Cloid cleanup:
            //
            // * `Skip` (stale cancel leg of a cancel-replace, cancel-before-accept
            //   race, or replay after terminal): leave the mapping intact. The
            //   still-open replacement order depends on it for subsequent events,
            //   and a genuinely terminal replay had its mapping evicted earlier.
            // * `Tracked` + status-only FILLED marker: defer the eviction until
            //   the matching `FillReport` lands so the partial fill preceding it
            //   keeps its client-order-id link.
            // * `Tracked` non-marker terminal and `External` terminal: evict now
            //   so long-running sessions do not leak cloid mappings.
            if let Some(id) = client_order_id
                && !is_open
            {
                match outcome {
                    DispatchOutcome::Skip => {}
                    DispatchOutcome::Tracked if is_filled_marker => {
                        pending_filled_cloids.add(id);
                    }
                    DispatchOutcome::Tracked | DispatchOutcome::External => {
                        let cloid = Cloid::from_client_order_id(id);
                        ws_client.remove_cloid_mapping(&Ustr::from(&cloid.to_hex()));
                    }
                }
            }
        }
        ExecutionReport::Fill(fill_report) => {
            let client_order_id = fill_report.client_order_id;

            let outcome = dispatch_fill_report(&fill_report, dispatch_state, emitter, ts_init);

            if outcome == DispatchOutcome::External {
                emitter.send_fill_report(fill_report);
            }

            // If this fill matches a deferred FILLED marker, drop the cloid
            // mapping now that the fill has landed.
            if let Some(id) = client_order_id
                && pending_filled_cloids.contains(&id)
            {
                pending_filled_cloids.remove(&id);
                let cloid = Cloid::from_client_order_id(id);
                ws_client.remove_cloid_mapping(&Ustr::from(&cloid.to_hex()));
            }
        }
    }
}

use crate::common::parse::determine_order_list_grouping;

#[cfg(test)]
mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
    use nautilus_live::ExecutionEventEmitter;
    use nautilus_model::{
        enums::{
            AccountType, ContingencyType, LiquiditySide, OrderSide, OrderStatus, OrderType,
            TimeInForce, TriggerType,
        },
        events::OrderEventAny,
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
        },
        orders::{OrderAny, limit::LimitOrder, stop_market::StopMarketOrder},
        reports::{FillReport, OrderStatusReport},
        types::{Currency, Money, Price, Quantity},
    };
    use nautilus_network::websocket::TransportBackend;
    use rstest::rstest;
    use ustr::Ustr;

    use super::{
        Cloid, ExecutionReport, FifoCache, HyperliquidWebSocketClient, OrderIdentity,
        WsDispatchState, determine_order_list_grouping, handle_execution_report,
        register_order_identity_into,
    };
    use crate::{common::enums::HyperliquidEnvironment, http::models::HyperliquidExecGrouping};

    const TEST_INSTRUMENT_ID: &str = "BTC-USD-PERP.HYPERLIQUID";

    fn test_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let clock = get_atomic_clock_realtime();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("HYPERLIQUID-001"),
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn drain_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<ExecutionEvent> {
        let mut out = Vec::new();
        while let Ok(e) = rx.try_recv() {
            out.push(e);
        }
        out
    }

    fn make_ws_client() -> HyperliquidWebSocketClient {
        // `HyperliquidWebSocketClient::new` does not connect, so this is a
        // cheap unit-test shim that still exercises the real `cloid_cache`
        // mapping APIs used by `handle_execution_report`.
        HyperliquidWebSocketClient::new(
            Some("wss://test.invalid".to_string()),
            HyperliquidEnvironment::Testnet,
            None,
            TransportBackend::default(),
            None,
        )
    }

    fn test_identity() -> OrderIdentity {
        OrderIdentity {
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from(TEST_INSTRUMENT_ID),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: Quantity::from("0.0001"),
            price: Some(Price::from("56730.0")),
        }
    }

    fn make_status_report(
        client_order_id: Option<&str>,
        venue_order_id: &str,
        status: OrderStatus,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("HYPERLIQUID-001"),
            InstrumentId::from(TEST_INSTRUMENT_ID),
            client_order_id.map(ClientOrderId::new),
            VenueOrderId::new(venue_order_id),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            status,
            Quantity::from("0.0001"),
            Quantity::from("0"),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            Some(UUID4::new()),
        )
        .with_price(Price::from("56730.0"))
    }

    fn make_fill_report(
        client_order_id: Option<&str>,
        venue_order_id: &str,
        trade_id: &str,
    ) -> FillReport {
        FillReport::new(
            AccountId::from("HYPERLIQUID-001"),
            InstrumentId::from(TEST_INSTRUMENT_ID),
            VenueOrderId::new(venue_order_id),
            TradeId::new(trade_id),
            OrderSide::Buy,
            Quantity::from("0.0001"),
            Price::from("56730.0"),
            Money::new(0.0, Currency::USD()),
            LiquiditySide::Taker,
            client_order_id.map(ClientOrderId::new),
            None,
            UnixNanos::default(),
            UnixNanos::default(),
            Some(UUID4::new()),
        )
    }

    fn cloid_for(id: &str) -> Ustr {
        let cloid = Cloid::from_client_order_id(ClientOrderId::from(id));
        Ustr::from(&cloid.to_hex())
    }

    fn limit_order(
        id: &str,
        reduce_only: bool,
        contingency: ContingencyType,
        linked_ids: Option<Vec<&str>>,
        parent_id: Option<&str>,
    ) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
            ClientOrderId::from(id),
            OrderSide::Buy,
            Quantity::from(1),
            Price::from("3000.00"),
            TimeInForce::Gtc,
            None,  // expire_time
            false, // post_only
            reduce_only,
            false, // quote_quantity
            None,  // display_qty
            None,  // emulation_trigger
            None,  // trigger_instrument_id
            Some(contingency),
            None, // order_list_id
            linked_ids.map(|ids| ids.into_iter().map(ClientOrderId::from).collect()),
            parent_id.map(ClientOrderId::from),
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            None, // tags
            Default::default(),
            Default::default(),
        ))
    }

    fn stop_order(
        id: &str,
        reduce_only: bool,
        contingency: ContingencyType,
        linked_ids: Option<Vec<&str>>,
        parent_id: Option<&str>,
    ) -> OrderAny {
        OrderAny::StopMarket(StopMarketOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("ETH-USD-PERP.HYPERLIQUID"),
            ClientOrderId::from(id),
            OrderSide::Sell,
            Quantity::from(1),
            Price::from("2800.00"),
            TriggerType::LastPrice,
            TimeInForce::Gtc,
            None, // expire_time
            reduce_only,
            false, // quote_quantity
            None,  // display_qty
            None,  // emulation_trigger
            None,  // trigger_instrument_id
            Some(contingency),
            None, // order_list_id
            linked_ids.map(|ids| ids.into_iter().map(ClientOrderId::from).collect()),
            parent_id.map(ClientOrderId::from),
            None, // exec_algorithm_id
            None, // exec_algorithm_params
            None, // exec_spawn_id
            None, // tags
            Default::default(),
            Default::default(),
        ))
    }

    #[rstest]
    #[case::independent_orders(
        vec![
            limit_order("O-001", false, ContingencyType::NoContingency, None, None),
            limit_order("O-002", false, ContingencyType::NoContingency, None, None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::bracket_oto(
        vec![
            limit_order("O-001", false, ContingencyType::Oto, Some(vec!["O-002", "O-003"]), None),
            limit_order("O-002", true, ContingencyType::Oco, Some(vec!["O-003"]), Some("O-001")),
            stop_order("O-003", true, ContingencyType::Oco, Some(vec!["O-002"]), Some("O-001")),
        ],
        HyperliquidExecGrouping::NormalTpsl,
    )]
    #[case::oto_not_bracket_shaped(
        vec![
            limit_order("O-001", false, ContingencyType::Oto, Some(vec!["O-002"]), None),
            limit_order("O-002", false, ContingencyType::Oto, Some(vec!["O-001"]), None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::oco_all_reduce_only(
        vec![
            limit_order("O-001", true, ContingencyType::Oco, Some(vec!["O-002"]), None),
            stop_order("O-002", true, ContingencyType::Oco, Some(vec!["O-001"]), None),
        ],
        HyperliquidExecGrouping::PositionTpsl,
    )]
    #[case::oco_not_all_reduce_only(
        vec![
            limit_order("O-001", false, ContingencyType::Oco, Some(vec!["O-002"]), None),
            stop_order("O-002", true, ContingencyType::Oco, Some(vec!["O-001"]), None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::oto_with_non_oco_children(
        vec![
            limit_order("O-001", false, ContingencyType::Oto, Some(vec!["O-002", "O-003"]), None),
            limit_order("O-002", true, ContingencyType::NoContingency, None, None),
            stop_order("O-003", true, ContingencyType::NoContingency, None, None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::mixed_oco_and_plain_reduce_only(
        vec![
            limit_order("O-001", true, ContingencyType::Oco, Some(vec!["O-002"]), None),
            stop_order("O-002", true, ContingencyType::NoContingency, None, None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::unlinked_oco_reduce_only(
        vec![
            limit_order("O-001", true, ContingencyType::Oco, Some(vec!["O-099"]), None),
            stop_order("O-002", true, ContingencyType::Oco, Some(vec!["O-098"]), None),
        ],
        HyperliquidExecGrouping::Na,
    )]
    #[case::single_order(
        vec![limit_order("O-001", false, ContingencyType::NoContingency, None, None)],
        HyperliquidExecGrouping::Na,
    )]
    fn test_determine_order_list_grouping(
        #[case] orders: Vec<OrderAny>,
        #[case] expected: HyperliquidExecGrouping,
    ) {
        let result = determine_order_list_grouping(&orders);
        assert_eq!(result, expected);
    }

    fn limit_order_with_quote_quantity(id: &str, quote_quantity: bool) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from(TEST_INSTRUMENT_ID),
            ClientOrderId::from(id),
            OrderSide::Buy,
            Quantity::from("0.0001"),
            Price::from("56730.0"),
            TimeInForce::Gtc,
            None,
            false,
            false,
            quote_quantity,
            None,
            None,
            None,
            Some(ContingencyType::NoContingency),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Default::default(),
            Default::default(),
        ))
    }

    #[rstest]
    fn test_register_order_identity_registers_regular_order() {
        let state = WsDispatchState::new();
        let order = limit_order_with_quote_quantity("O-REG-001", false);

        register_order_identity_into(&state, &order);

        let found = state
            .lookup_identity(&ClientOrderId::from("O-REG-001"))
            .expect("identity should be registered");
        assert_eq!(found.strategy_id, StrategyId::from("S-001"));
        assert_eq!(found.instrument_id, InstrumentId::from(TEST_INSTRUMENT_ID));
        assert_eq!(found.order_side, OrderSide::Buy);
        assert_eq!(found.order_type, OrderType::Limit);
        assert_eq!(found.quantity, Quantity::from("0.0001"));
        assert_eq!(found.price, Some(Price::from("56730.0")));
    }

    #[rstest]
    fn test_register_order_identity_skips_quote_quantity_order() {
        let state = WsDispatchState::new();
        let order = limit_order_with_quote_quantity("O-QQ-001", true);

        register_order_identity_into(&state, &order);

        // Quote-quantity orders flow through the untracked path so the engine
        // reconciles them from status reports; registering would make the
        // cumulative-fill comparison mismatch base-unit fills against the
        // quote-unit tracked quantity and leave the order stuck "open".
        assert!(
            state
                .lookup_identity(&ClientOrderId::from("O-QQ-001"))
                .is_none()
        );
    }

    #[rstest]
    fn test_handle_execution_report_skip_keeps_cloid_mapping() {
        // Regression guard for GH-3827: when the dispatch returns Skip (e.g.
        // the stale cancel leg of a cancel-replace), the cloid mapping must
        // stay in place so the still-open replacement order can still be
        // resolved by subsequent events.
        let ws_client = make_ws_client();
        let (emitter, mut rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-SKIP");
        state.register_identity(cid, test_identity());
        // Prime state so the later CANCELED(old_voi) is classified as stale.
        state.insert_accepted(cid);
        state.record_venue_order_id(cid, VenueOrderId::new("new-voi"));

        ws_client.cache_cloid_mapping(cloid_for("O-HER-SKIP"), cid);

        let stale_cancel = make_status_report(Some("O-HER-SKIP"), "old-voi", OrderStatus::Canceled);
        handle_execution_report(
            ExecutionReport::Order(stale_cancel),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        assert!(drain_events(&mut rx).is_empty());
        // Cloid mapping preserved; the replacement order still resolves.
        assert_eq!(
            ws_client.get_cloid_mapping(&cloid_for("O-HER-SKIP")),
            Some(cid)
        );
        // Identity is still tracked (the skip path did not clean up).
        assert!(state.lookup_identity(&cid).is_some());
    }

    #[rstest]
    fn test_handle_execution_report_tracked_terminal_evicts_cloid() {
        // A tracked CANCELED that reaches a genuine terminal state should
        // emit OrderCanceled and evict the cloid mapping so long-running
        // sessions do not leak.
        let ws_client = make_ws_client();
        let (emitter, mut rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-CANCEL");
        state.register_identity(cid, test_identity());
        state.insert_accepted(cid);
        state.record_venue_order_id(cid, VenueOrderId::new("v-cancel"));

        ws_client.cache_cloid_mapping(cloid_for("O-HER-CANCEL"), cid);

        let report = make_status_report(Some("O-HER-CANCEL"), "v-cancel", OrderStatus::Canceled);
        handle_execution_report(
            ExecutionReport::Order(report),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Canceled(_))
        ));
        assert_eq!(
            ws_client.get_cloid_mapping(&cloid_for("O-HER-CANCEL")),
            None
        );
        assert!(state.filled_orders.contains(&cid));
    }

    #[rstest]
    fn test_handle_execution_report_filled_marker_then_fill_evicts_on_fill() {
        // The status-only FILLED marker defers the cloid eviction to the
        // pending cache; the matching FillReport emits OrderFilled and then
        // evicts the cloid mapping as part of the deferred-cleanup path.
        let ws_client = make_ws_client();
        let (emitter, mut rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-FILL");
        state.register_identity(cid, test_identity());
        state.insert_accepted(cid);
        state.record_venue_order_id(cid, VenueOrderId::new("v-fill"));

        ws_client.cache_cloid_mapping(cloid_for("O-HER-FILL"), cid);

        let status_marker = make_status_report(Some("O-HER-FILL"), "v-fill", OrderStatus::Filled);
        handle_execution_report(
            ExecutionReport::Order(status_marker),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        // Marker arrived: no event, cloid cleanup deferred, mapping retained.
        assert!(drain_events(&mut rx).is_empty());
        assert_eq!(
            ws_client.get_cloid_mapping(&cloid_for("O-HER-FILL")),
            Some(cid)
        );

        let fill = make_fill_report(Some("O-HER-FILL"), "v-fill", "trade-fill");
        handle_execution_report(
            ExecutionReport::Fill(fill),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(matches!(
            events[0],
            ExecutionEvent::Order(OrderEventAny::Filled(_))
        ));
        // Deferred cleanup fires once the fill lands.
        assert_eq!(ws_client.get_cloid_mapping(&cloid_for("O-HER-FILL")), None);
    }

    #[rstest]
    fn test_handle_execution_report_external_terminal_evicts_cloid() {
        // External (untracked) terminal reports forward to the engine via
        // send_order_status_report and immediately evict the cloid mapping
        // so the client does not leak mappings for orders it does not own.
        let ws_client = make_ws_client();
        let (emitter, mut rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-EXT");
        ws_client.cache_cloid_mapping(cloid_for("O-HER-EXT"), cid);

        let report = make_status_report(Some("O-HER-EXT"), "v-ext", OrderStatus::Canceled);
        handle_execution_report(
            ExecutionReport::Order(report),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(events[0], ExecutionEvent::Report(_)),
            "external terminal report should forward to the engine as a report",
        );
        assert_eq!(ws_client.get_cloid_mapping(&cloid_for("O-HER-EXT")), None);
    }

    #[rstest]
    fn test_handle_execution_report_open_status_preserves_cloid() {
        // An open (non-terminal) status must never touch the cloid mapping.
        let ws_client = make_ws_client();
        let (emitter, _rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-OPEN");
        state.register_identity(cid, test_identity());
        ws_client.cache_cloid_mapping(cloid_for("O-HER-OPEN"), cid);

        let report = make_status_report(Some("O-HER-OPEN"), "v-open", OrderStatus::Accepted);
        handle_execution_report(
            ExecutionReport::Order(report),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        // Accepted is open → no cloid eviction regardless of outcome.
        assert_eq!(
            ws_client.get_cloid_mapping(&cloid_for("O-HER-OPEN")),
            Some(cid)
        );
    }

    #[rstest]
    fn test_handle_execution_report_tracked_accepted_emits_typed_event() {
        // A tracked open ACCEPTED must flow through the typed-event path,
        // NOT the raw report fallback. Catches a mutation that swaps the
        // branch polarity inside `handle_execution_report`.
        let ws_client = make_ws_client();
        let (emitter, mut rx) = test_emitter();
        let state = WsDispatchState::new();
        let mut pending_cloids: FifoCache<ClientOrderId, 10_000> = FifoCache::new();

        let cid = ClientOrderId::from("O-HER-ACC");
        state.register_identity(cid, test_identity());
        ws_client.cache_cloid_mapping(cloid_for("O-HER-ACC"), cid);

        let report = make_status_report(Some("O-HER-ACC"), "v-acc", OrderStatus::Accepted);
        handle_execution_report(
            ExecutionReport::Order(report),
            &state,
            &emitter,
            &ws_client,
            &mut pending_cloids,
            UnixNanos::default(),
        );

        let events = drain_events(&mut rx);
        assert_eq!(events.len(), 1);
        assert!(
            matches!(events[0], ExecutionEvent::Order(OrderEventAny::Accepted(_))),
            "tracked accepted should route through the typed-event path",
        );
        // Mapping is unchanged because the status is still open.
        assert_eq!(
            ws_client.get_cloid_mapping(&cloid_for("O-HER-ACC")),
            Some(cid)
        );
    }
}
