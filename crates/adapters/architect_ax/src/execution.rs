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

//! Live execution client implementation for the AX Exchange adapter.

use std::{
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    AtomicMap, MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderExpired,
        OrderFilled, OrderRejected, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money, Price, Quantity},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            AX_ACCOUNT_REGISTRATION_TIMEOUT_SECS, AX_AUTH_TOKEN_TTL_EXEC_SECS, AX_POST_ONLY_REJECT,
            AX_VENUE,
        },
        credential::Credential,
        enums::AxOrderSide,
        parse::{ax_timestamp_stn_to_unix_nanos, cid_to_client_order_id, quantity_to_contracts},
    },
    config::AxExecClientConfig,
    http::{
        client::AxHttpClient,
        models::{AxOrderRejectReason, PreviewAggressiveLimitOrderRequest, ReplaceOrderRequest},
    },
    websocket::{
        AxOrdersWsMessage, AxWsOrderEvent,
        messages::{AxWsOrder, AxWsTradeExecution, OrderMetadata},
        orders::{AxOrdersWebSocketClient, OrdersCaches},
    },
};

/// Live execution client for the AX Exchange.
#[derive(Debug)]
pub struct AxExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: AxExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: AxHttpClient,
    ws_orders: AxOrdersWebSocketClient,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl AxExecutionClient {
    /// Creates a new [`AxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: AxExecClientConfig) -> anyhow::Result<Self> {
        let http_client = AxHttpClient::with_credentials(
            config.api_key.clone().unwrap_or_default(),
            config.api_secret.clone().unwrap_or_default(),
            Some(config.http_base_url()),
            Some(config.orders_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.proxy_url.clone(),
        )?;

        let clock = get_atomic_clock_realtime();
        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let mut ws_url = config.ws_private_url();
        if config.cancel_on_disconnect {
            let separator = if ws_url.contains('?') { "&" } else { "?" };
            ws_url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        }
        let ws_orders = AxOrdersWebSocketClient::new(
            ws_url,
            account_id,
            trader_id,
            config.heartbeat_interval_secs,
            config.transport_backend,
            config.proxy_url.clone(),
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_orders,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    async fn authenticate(&self) -> anyhow::Result<String> {
        let credential =
            Credential::resolve(self.config.api_key.clone(), self.config.api_secret.clone())
                .context("API credentials not configured")?;

        self.http_client
            .authenticate(
                credential.api_key(),
                credential.api_secret(),
                AX_AUTH_TOKEN_TTL_EXEC_SECS,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Authentication failed: {e}"))
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
                .await
                .context("failed to request AX account state")?;
            let ts_event = clock.get_time_ns();
            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                ts_event,
            );
            Ok(())
        });
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let (
            client_order_id,
            strategy_id,
            instrument_id,
            order_side,
            order_type,
            quantity,
            trigger_price,
            time_in_force,
            is_post_only,
            limit_price,
        ) = {
            let cache = self.core.cache();
            let order = cache.order(&cmd.client_order_id).ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;
            (
                order.client_order_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.order_side(),
                order.order_type(),
                order.quantity(),
                order.trigger_price(),
                order.time_in_force(),
                order.is_post_only(),
                order.price(),
            )
        };

        let ws_orders = self.ws_orders.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let trader_id = self.core.trader_id;

        let http_client = if order_type == OrderType::Market {
            Some(self.http_client.clone())
        } else {
            None
        };

        self.spawn_task("submit_order", async move {
            let result: anyhow::Result<()> = async {
                // For market orders, get the take-through price from AX.
                // The preview and submit are not atomic: the book can change
                // between the two calls. This is safe because submit_order
                // forces IOC time-in-force for market orders, so the order
                // fills immediately or is canceled (it cannot rest on the book).
                // If the book moves past the previewed take-through price the
                // order may partially fill with the remainder canceled.
                let price = if order_type == OrderType::Market {
                    let symbol = instrument_id.symbol.inner();
                    let ax_side = AxOrderSide::try_from(order_side)
                        .map_err(|e| anyhow::anyhow!("Invalid order side: {e}"))?;
                    let qty_contracts = quantity_to_contracts(quantity)?;

                    let request =
                        PreviewAggressiveLimitOrderRequest::new(symbol, qty_contracts, ax_side);
                    let response = http_client
                        .expect("HTTP client should be set for market orders")
                        .inner
                        .preview_aggressive_limit_order(&request)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to preview aggressive limit order: {e}")
                        })?;

                    if response.remaining_quantity > 0 {
                        log::warn!(
                            "Market order book depth insufficient: \
                             filled_qty={} remaining_qty={} for {instrument_id}",
                            response.filled_quantity,
                            response.remaining_quantity,
                        );
                    }

                    let limit_price_decimal = response.limit_price.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No liquidity available for market order on {instrument_id}"
                        )
                    })?;

                    let price = Price::from(limit_price_decimal.to_string().as_str());
                    log::info!("Market order take-through price: {price} for {instrument_id}",);
                    Some(price)
                } else {
                    limit_price
                };

                ws_orders
                    .submit_order(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        is_post_only,
                    )
                    .await
                    .map_err(|e| anyhow::anyhow!("Submit order failed: {e}"))?;

                Ok(())
            }
            .await;

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &format!("submit-order-error: {e}"),
                    ts_event,
                    false,
                );
                anyhow::bail!("{e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order_internal(&self, cmd: &CancelOrder) {
        let ws_orders = self.ws_orders.clone();

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("cancel_order", async move {
            let result = ws_orders
                .cancel_order(client_order_id, venue_order_id)
                .await
                .map_err(|e| anyhow::anyhow!("Cancel order failed: {e}"));

            if let Err(e) = &result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );
                anyhow::bail!("{e}");
            }

            Ok(())
        });
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let runtime = get_runtime();
        let handle = runtime.spawn(async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e}");
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

    /// Polls the cache until the account is registered or timeout is reached.
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
impl ExecutionClient for AxExecutionClient {
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
        *AX_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account(&self.core.account_id).cloned()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Reset so requests work after a previous disconnect
        self.http_client.reset_cancellation_token();

        if !self.core.instruments_initialized() {
            let instruments = self
                .http_client
                .request_instruments(None, None)
                .await
                .context("failed to request AX instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned from AX");
            } else {
                log::info!("Loaded {} instruments", instruments.len());
                self.http_client.cache_instruments(&instruments);
                self.ws_orders.cache_instruments(&instruments);
            }
            self.core.set_instruments_initialized();
        }

        let token = self.authenticate().await?;
        self.ws_orders.connect(&token).await?;
        log::info!("Connected to orders WebSocket");

        let should_spawn = match &self.ws_stream_handle {
            None => true,
            Some(handle) => handle.is_finished(),
        };

        if should_spawn {
            let stream = self.ws_orders.stream();
            let emitter = self.emitter.clone();
            let caches = self.ws_orders.caches().clone();
            let account_id = self.core.account_id;
            let instruments_cache = self.ws_orders.instruments_cache();
            let clock = self.clock;

            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(
                        message,
                        &emitter,
                        &caches,
                        account_id,
                        &instruments_cache,
                        clock,
                    );
                }
            });
            self.ws_stream_handle = Some(handle);
        }

        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request AX account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

        self.await_account_registered(AX_ACCOUNT_REGISTRATION_TIMEOUT_SECS)
            .await?;

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();

        self.ws_orders.close().await;

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let instrument_id = cmd.instrument_id;
        let emitter = self.emitter.clone();

        // Read immutable order fields from cache before spawning
        let (order_side, order_type, time_in_force) = {
            let cache = self.core.cache();
            match cache.order(&client_order_id) {
                Some(order) => (
                    order.order_side(),
                    order.order_type(),
                    order.time_in_force(),
                ),
                None => (OrderSide::NoOrderSide, OrderType::Limit, TimeInForce::Gtc),
            }
        };

        self.spawn_task("query_order", async move {
            match http_client
                .request_order_status(
                    account_id,
                    instrument_id,
                    Some(client_order_id),
                    venue_order_id,
                    order_side,
                    order_type,
                    time_in_force,
                )
                .await
            {
                Ok(report) => emitter.send_order_status_report(report),
                Err(e) => log::error!("AX query order failed: {e}"),
            }
            Ok(())
        });

        Ok(())
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

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();
        log::info!(
            "Started: client_id={}, account_id={}, environment={}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        {
            let cache = self.core.cache();
            let order = cache.order(&cmd.client_order_id).ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            if !matches!(
                order.order_type(),
                OrderType::Market | OrderType::Limit | OrderType::StopLimit
            ) {
                self.emitter.emit_order_denied(
                    order,
                    &format!(
                        "Unsupported order type: {:?}, \
                         AX supports MARKET, LIMIT and STOP_LIMIT",
                        order.order_type(),
                    ),
                );
                return Ok(());
            }

            if order.time_in_force() == TimeInForce::Gtd {
                self.emitter.emit_order_denied(
                    order,
                    "Unsupported time in force: GTD, \
                     AX supports GTC, IOC, FOK, and DAY",
                );
                return Ok(());
            }

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        self.submit_order_internal(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        for (client_order_id, order_init) in cmd
            .order_list
            .client_order_ids
            .iter()
            .zip(cmd.order_inits.iter())
        {
            let submit_cmd = SubmitOrder::new(
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                *client_order_id,
                order_init.clone(),
                cmd.exec_algorithm_id,
                cmd.position_id,
                cmd.params.clone(),
                UUID4::new(),
                cmd.ts_init,
            );
            self.submit_order(submit_cmd)?;
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let venue_order_id = match cmd.venue_order_id {
            Some(ref voi) => *voi,
            None => {
                let reason = "Cannot modify order without venue_order_id";
                log::error!("{reason}");
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_modify_rejected_event(
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    cmd.venue_order_id,
                    reason,
                    ts_event,
                );
                return Ok(());
            }
        };

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let caches = self.ws_orders.caches().clone();
        let clock = self.clock;
        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let quantity = cmd.quantity;
        let price = cmd.price;
        let trigger_price = cmd.trigger_price;

        self.spawn_task("modify_order", async move {
            let mut request = ReplaceOrderRequest::new(venue_order_id.as_str());

            if let Some(price) = price {
                request = request.with_price(price.as_decimal());
            }

            if let Some(qty) = quantity {
                let contracts = quantity_to_contracts(qty)?;
                request = request.with_quantity(contracts);
            }

            if let Some(trigger) = trigger_price {
                request = request.with_trigger_price(trigger.as_decimal());
            }

            match http_client.inner.replace_order(&request).await {
                Ok(resp) => {
                    let new_venue_order_id = VenueOrderId::new(&resp.oid);
                    caches
                        .venue_to_client_id
                        .insert(new_venue_order_id, client_order_id);
                    if let Some(mut entry) = caches.orders_metadata.get_mut(&client_order_id) {
                        entry.venue_order_id = Some(new_venue_order_id);
                        entry.pending_trigger_price = trigger_price;
                    }
                    log::info!("Order replaced: old={} new={}", request.oid, resp.oid);
                }
                Err(e) => {
                    let reason = format!("modify-order-error: {e}");
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(VenueOrderId::new(&request.oid)),
                        &reason,
                        ts_event,
                    );
                    anyhow::bail!("{reason}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.account_id;
        let trader_id = self.core.trader_id;

        // Snapshot open orders so we can emit cancel events after the HTTP request
        let open_orders: Vec<(ClientOrderId, Option<VenueOrderId>, StrategyId)> = {
            let cache = self.core.cache();
            cache
                .orders_open(None, Some(&instrument_id), None, None, None)
                .iter()
                .map(|o| (o.client_order_id(), o.venue_order_id(), o.strategy_id()))
                .collect()
        };

        let caches = self.ws_orders.caches().clone();

        self.spawn_task("cancel_all_orders", async move {
            match http_client.cancel_all_orders(instrument_id).await {
                Ok(()) => {
                    log::info!("Canceled all orders for {instrument_id}");

                    // AX does not push WS cancel confirmations for HTTP-initiated
                    // cancels, so emit OrderCanceled events locally and clean up
                    // tracking state to prevent duplicates if WS events arrive
                    let ts_event = clock.get_time_ns();

                    for (client_order_id, venue_order_id, strategy_id) in &open_orders {
                        let event = OrderCanceled::new(
                            trader_id,
                            *strategy_id,
                            instrument_id,
                            *client_order_id,
                            UUID4::new(),
                            ts_event,
                            clock.get_time_ns(),
                            false,
                            *venue_order_id,
                            Some(account_id),
                        );
                        emitter.send_order_event(OrderEventAny::Canceled(event));

                        if let Some(voi) = venue_order_id {
                            caches.venue_to_client_id.remove(voi);
                        }
                        caches.orders_metadata.remove(client_order_id);
                    }
                }
                Err(e) => {
                    log::error!("Failed to cancel all orders for {instrument_id}: {e}");
                    let ts_event = clock.get_time_ns();

                    for (client_order_id, venue_order_id, strategy_id) in &open_orders {
                        emitter.emit_order_cancel_rejected_event(
                            *strategy_id,
                            instrument_id,
                            *client_order_id,
                            *venue_order_id,
                            &format!("cancel-all-orders-error: {e}"),
                            ts_event,
                        );
                    }
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        for cancel in &cmd.cancels {
            self.cancel_order_internal(cancel);
        }
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let cid_map = self.ws_orders.cid_to_client_order_id().clone();
        let cid_resolver = move |cid: u64| cid_map.get(&cid).map(|v| *v);

        let mut reports = self
            .http_client
            .request_order_status_reports(self.core.account_id, Some(cid_resolver))
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if let Some(client_order_id) = cmd.client_order_id {
            reports.retain(|report| report.client_order_id == Some(client_order_id));
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports.into_iter().next())
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let cid_map = self.ws_orders.cid_to_client_order_id().clone();
        let cid_resolver = move |cid: u64| cid_map.get(&cid).map(|v| *v);

        let mut reports = self
            .http_client
            .request_order_status_reports(self.core.account_id, Some(cid_resolver))
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

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
        let mut reports = self
            .http_client
            .request_fill_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = self
            .http_client
            .request_position_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins * 60 * 1_000_000_000;
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_now,
            false, // open_only
            None,  // instrument_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        let fill_cmd = GenerateFillReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            None, // venue_order_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        let position_cmd = GeneratePositionStatusReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        let (order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *AX_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        _ts_init: UnixNanos,
    ) {
        self.ws_orders.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
        );
    }
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(
    message: AxOrdersWsMessage,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) {
    match message {
        AxOrdersWsMessage::Event(event) => {
            dispatch_order_event(*event, emitter, caches, account_id, instruments, clock);
        }
        AxOrdersWsMessage::PlaceOrderResponse(resp) => {
            log::debug!(
                "Place order response: rid={} oid={}",
                resp.rid,
                resp.res.oid
            );
        }
        AxOrdersWsMessage::CancelOrderResponse(resp) => {
            log::debug!(
                "Cancel order response: rid={} accepted={}",
                resp.rid,
                resp.res.cxl_rx
            );
        }
        AxOrdersWsMessage::OpenOrdersResponse(resp) => {
            log::debug!("Open orders response: {} orders", resp.res.len());
        }
        AxOrdersWsMessage::Error(err) => {
            log::error!("WebSocket error: {}", err.message);
        }
        AxOrdersWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        AxOrdersWsMessage::Authenticated => {
            log::debug!("WebSocket authenticated");
        }
    }
}

fn dispatch_order_event(
    event: AxWsOrderEvent,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) {
    match event {
        AxWsOrderEvent::Heartbeat => {}
        AxWsOrderEvent::Acknowledged(msg) => {
            if let Some(event) =
                create_order_accepted(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Accepted(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Accepted,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        AxWsOrderEvent::PartiallyFilled(msg) => {
            dispatch_fill_event(
                &msg.o,
                &msg.xs,
                msg.ts,
                msg.tn,
                emitter,
                caches,
                account_id,
                instruments,
                clock,
            );
        }
        AxWsOrderEvent::Filled(msg) => {
            dispatch_fill_event(
                &msg.o,
                &msg.xs,
                msg.ts,
                msg.tn,
                emitter,
                caches,
                account_id,
                instruments,
                clock,
            );
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Canceled(msg) => {
            if let Some(event) =
                create_order_canceled(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Canceled(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Canceled,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Rejected(msg) => {
            let known_reason = msg.r.filter(|r| !matches!(r, AxOrderRejectReason::Unknown));
            let reason = known_reason
                .as_ref()
                .map(AsRef::as_ref)
                .or(msg.txt.as_deref())
                .unwrap_or("UNKNOWN");

            if let Some(event) =
                create_order_rejected(&msg.o, reason, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Rejected(event));
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Expired(msg) => {
            if let Some(event) =
                create_order_expired(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Expired(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Expired,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Replaced(msg) => {
            if let Some(event) =
                create_order_updated(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Updated(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Accepted,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        AxWsOrderEvent::DoneForDay(msg) => {
            if let Some(event) =
                create_order_expired(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Expired(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Expired,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::CancelRejected(msg) => {
            let venue_order_id = VenueOrderId::new(&msg.oid);
            if let Some(client_order_id) = caches.venue_to_client_id.get(&venue_order_id)
                && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
            {
                let event = OrderCancelRejected::new(
                    metadata.trader_id,
                    metadata.strategy_id,
                    metadata.instrument_id,
                    metadata.client_order_id,
                    Ustr::from(msg.r.as_ref()),
                    UUID4::new(),
                    clock.get_time_ns(),
                    metadata.ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::CancelRejected(event));
            } else {
                log::warn!(
                    "Could not find metadata for cancel rejected order {}",
                    msg.oid
                );
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_fill_event(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    ts: i64,
    tn: i64,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) {
    if let Some(event) = create_order_filled(order, execution, ts, tn, caches, account_id, clock) {
        emitter.send_order_event(OrderEventAny::Filled(event));
    } else if let Some(report) = create_fill_report(
        order,
        execution,
        ts,
        tn,
        caches,
        account_id,
        instruments,
        clock,
    ) {
        emitter.send_fill_report(report);
    }
}

pub(crate) fn lookup_order_metadata<'a>(
    order: &AxWsOrder,
    caches: &'a OrdersCaches,
) -> Option<dashmap::mapref::one::Ref<'a, ClientOrderId, OrderMetadata>> {
    let venue_order_id = VenueOrderId::new(&order.oid);

    if let Some(client_order_id) = caches.venue_to_client_id.get(&venue_order_id)
        && let Some(metadata) = caches.orders_metadata.get(&*client_order_id)
    {
        return Some(metadata);
    }

    if let Some(cid) = order.cid
        && let Some(client_order_id) = caches.cid_to_client_order_id.get(&cid)
        && let Some(metadata) = caches.orders_metadata.get(&*client_order_id)
    {
        return Some(metadata);
    }

    None
}

pub(crate) fn create_order_accepted(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderAccepted> {
    let venue_order_id = VenueOrderId::new(&order.oid);
    let metadata = lookup_order_metadata(order, caches)?;

    let client_order_id = metadata.client_order_id;
    let trader_id = metadata.trader_id;
    let strategy_id = metadata.strategy_id;
    let instrument_id = metadata.instrument_id;
    drop(metadata);

    caches
        .venue_to_client_id
        .insert(venue_order_id, client_order_id);

    if let Some(mut entry) = caches.orders_metadata.get_mut(&client_order_id) {
        entry.venue_order_id = Some(venue_order_id);
    }

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    Some(OrderAccepted::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
    ))
}

pub(crate) fn create_order_updated(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderUpdated> {
    let metadata = lookup_order_metadata(order, caches)?;

    let client_order_id = metadata.client_order_id;
    let trader_id = metadata.trader_id;
    let strategy_id = metadata.strategy_id;
    let instrument_id = metadata.instrument_id;
    let price_precision = metadata.price_precision;
    let size_precision = metadata.size_precision;
    let pending_trigger_price = metadata.pending_trigger_price;
    // Use cached venue_order_id (set by HTTP handler) over the WS event oid,
    // because AX may report the old oid in the replaced event
    let venue_order_id = metadata
        .venue_order_id
        .unwrap_or_else(|| VenueOrderId::new(&order.oid));
    drop(metadata);

    caches
        .venue_to_client_id
        .insert(venue_order_id, client_order_id);

    // Consume the pending trigger price now that the replace is confirmed
    if let Some(mut entry) = caches.orders_metadata.get_mut(&client_order_id) {
        entry.pending_trigger_price = None;
    }

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    let quantity = Quantity::new(order.q as f64, size_precision);
    let price = Price::from_decimal_dp(order.p, price_precision).ok();

    Some(OrderUpdated::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        quantity,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(account_id),
        price,
        pending_trigger_price,
        None, // protection_price
        false,
    ))
}

pub(crate) fn create_order_filled(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderFilled> {
    let venue_order_id = VenueOrderId::new(&order.oid);
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    let last_qty = Quantity::new(execution.q as f64, metadata.size_precision);
    let last_px = Price::from_decimal_dp(execution.p, metadata.price_precision).ok()?;

    let order_side: OrderSide = order.d.into();

    let liquidity_side = if execution.agg {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    Some(OrderFilled::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        venue_order_id,
        account_id,
        TradeId::new(&execution.tid),
        order_side,
        OrderType::Limit,
        last_qty,
        last_px,
        metadata.quote_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        None,
        None,
    ))
}

pub(crate) fn create_order_canceled(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderCanceled> {
    let venue_order_id = VenueOrderId::new(&order.oid);
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    Some(OrderCanceled::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(account_id),
    ))
}

pub(crate) fn create_order_expired(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderExpired> {
    let venue_order_id = VenueOrderId::new(&order.oid);
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    Some(OrderExpired::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(account_id),
    ))
}

pub(crate) fn create_order_rejected(
    order: &AxWsOrder,
    reason: &str,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderRejected> {
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let due_post_only = reason.contains(AX_POST_ONLY_REJECT);

    Some(OrderRejected::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        account_id,
        Ustr::from(reason),
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        due_post_only,
    ))
}

pub(crate) fn cleanup_terminal_order_tracking(order: &AxWsOrder, caches: &OrdersCaches) {
    let venue_order_id = VenueOrderId::new(&order.oid);
    let client_order_id = caches
        .venue_to_client_id
        .remove(&venue_order_id)
        .map(|(_, v)| v)
        .or_else(|| {
            order
                .cid
                .and_then(|cid| caches.cid_to_client_order_id.remove(&cid).map(|(_, v)| v))
        });

    if let Some(client_order_id) = client_order_id {
        caches.orders_metadata.remove(&client_order_id);
    }

    if let Some(cid) = order.cid {
        caches.cid_to_client_order_id.remove(&cid);
    }
}

#[expect(clippy::too_many_arguments)]
fn create_order_status_report(
    order: &AxWsOrder,
    order_status: OrderStatus,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) -> Option<OrderStatusReport> {
    let instruments_snap = instruments.load();
    let instrument = instruments_snap.get(&order.s)?;
    let venue_order_id = VenueOrderId::new(&order.oid);
    let instrument_id = instrument.id();
    let order_side = order.d.into();
    let time_in_force = order.tif.into();

    let quantity = Quantity::new(order.q as f64, instrument.size_precision());
    let filled_qty = Quantity::new(order.xq as f64, instrument.size_precision());

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let ts_init = clock.get_time_ns();

    let client_order_id = order.cid.map(|cid| {
        caches
            .cid_to_client_order_id
            .get(&cid)
            .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
    });

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side,
        OrderType::Limit,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    if let Ok(price) = Price::from_decimal_dp(order.p, instrument.price_precision()) {
        report = report.with_price(price);
    }

    Some(report)
}

#[expect(clippy::too_many_arguments)]
fn create_fill_report(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) -> Option<FillReport> {
    let instruments_snap = instruments.load();
    let instrument = instruments_snap.get(&order.s)?;
    let venue_order_id = VenueOrderId::new(&order.oid);
    let instrument_id = instrument.id();
    let order_side = order.d.into();

    let last_qty = Quantity::new(execution.q as f64, instrument.size_precision());
    let last_px = Price::from_decimal_dp(execution.p, instrument.price_precision()).ok()?;

    let liquidity_side = if execution.agg {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let ts_init = clock.get_time_ns();

    let client_order_id = order.cid.map(|cid| {
        caches
            .cid_to_client_order_id
            .get(&cid)
            .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
    });

    // The WS trade execution payload does not include fee data so
    // commission is zero here. The REST /fills endpoint (used during
    // reconciliation via parse_fill_report) includes accurate fees.
    let commission = Money::new(0.0, instrument.quote_currency());

    Some(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        TradeId::new(&execution.tid),
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    ))
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use dashmap::DashMap;
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{AxOrderSide, AxOrderStatus, AxTimeInForce},
        websocket::{
            messages::{AxWsTradeExecution, OrderMetadata},
            orders::OrdersCaches,
        },
    };

    fn test_caches() -> OrdersCaches {
        OrdersCaches {
            orders_metadata: Arc::new(DashMap::new()),
            venue_to_client_id: Arc::new(DashMap::new()),
            cid_to_client_order_id: Arc::new(DashMap::new()),
        }
    }

    fn test_ws_order(oid: &str, price: Decimal, qty: u64) -> AxWsOrder {
        AxWsOrder {
            oid: oid.to_string(),
            u: "user".to_string(),
            s: Ustr::from("BTC-PERP"),
            p: price,
            q: qty,
            xq: 0,
            rq: qty,
            o: AxOrderStatus::Accepted,
            d: AxOrderSide::Buy,
            tif: AxTimeInForce::Gtc,
            ts: 1609459200,
            tn: 0,
            cid: None,
            tag: None,
            txt: None,
        }
    }

    #[rstest]
    fn test_create_order_updated_uses_cached_venue_order_id() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-001");
        let new_venue_id = VenueOrderId::new("NEW-OID");
        let trigger = Price::from("49000.00");

        let metadata = OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("BTC-PERP.AX"),
            client_order_id,
            venue_order_id: Some(new_venue_id),
            ts_init: 0.into(),
            size_precision: 0,
            price_precision: 2,
            quote_currency: Currency::USD(),
            pending_trigger_price: Some(trigger),
        };
        caches.orders_metadata.insert(client_order_id, metadata);
        caches
            .venue_to_client_id
            .insert(new_venue_id, client_order_id);

        // WS event carries the OLD oid
        let ws_order = test_ws_order("OLD-OID", dec!(50500.00), 100);

        // Lookup needs cid path since OLD-OID is not in venue_to_client_id.
        // Seed it via cid instead.
        let cid_value = 42u64;
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);
        let mut ws_order_with_cid = ws_order;
        ws_order_with_cid.cid = Some(cid_value);

        let event = create_order_updated(
            &ws_order_with_cid,
            1609459200,
            0,
            &caches,
            account_id,
            clock,
        )
        .expect("should produce OrderUpdated");

        // Uses cached NEW-OID, not the WS event's OLD-OID
        assert_eq!(event.venue_order_id, Some(new_venue_id));
        assert_eq!(event.trigger_price, Some(trigger));
        assert_eq!(event.quantity, Quantity::new(100.0, 0));
        assert_eq!(event.price, Some(Price::from("50500.00")));

        // Pending trigger consumed
        let meta = caches.orders_metadata.get(&client_order_id).unwrap();
        assert!(meta.pending_trigger_price.is_none());
    }

    #[rstest]
    fn test_create_order_updated_falls_back_to_ws_oid() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-002");
        let ws_oid = VenueOrderId::new("WS-OID");

        let metadata = OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id: InstrumentId::from("BTC-PERP.AX"),
            client_order_id,
            venue_order_id: None,
            ts_init: 0.into(),
            size_precision: 0,
            price_precision: 2,
            quote_currency: Currency::USD(),
            pending_trigger_price: None,
        };
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.venue_to_client_id.insert(ws_oid, client_order_id);

        let ws_order = test_ws_order("WS-OID", dec!(50500.00), 200);

        let event = create_order_updated(&ws_order, 1609459200, 0, &caches, account_id, clock)
            .expect("should produce OrderUpdated");

        assert_eq!(event.venue_order_id, Some(ws_oid));
        assert!(event.trigger_price.is_none());
    }

    fn test_metadata(client_order_id: ClientOrderId, instrument_id: InstrumentId) -> OrderMetadata {
        OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            venue_order_id: None,
            ts_init: 0.into(),
            size_precision: 0,
            price_precision: 2,
            quote_currency: Currency::USD(),
            pending_trigger_price: None,
        }
    }

    fn test_execution(tid: &str, price: Decimal, qty: u64, agg: bool) -> AxWsTradeExecution {
        AxWsTradeExecution {
            tid: tid.to_string(),
            s: Ustr::from("BTC-PERP"),
            q: qty,
            p: price,
            d: AxOrderSide::Buy,
            agg,
        }
    }

    #[rstest]
    fn test_create_order_accepted_populates_cache_and_event() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-ACK");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-ACK");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        let cid_value = 7u64;
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        let mut ws_order = test_ws_order(venue_order_id.as_str(), dec!(50500.00), 100);
        ws_order.cid = Some(cid_value);

        let event = create_order_accepted(&ws_order, 1609459200, 500, &caches, account_id, clock)
            .expect("should produce OrderAccepted");

        assert_eq!(event.venue_order_id, venue_order_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.account_id, account_id);
        assert_eq!(event.instrument_id, instrument_id);
        assert_eq!(event.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(event.strategy_id, StrategyId::from("S-001"));
        assert_eq!(
            event.ts_event,
            UnixNanos::from(1_609_459_200_000_000_500u64)
        );

        // Side effects on caches
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id,
        );
        let meta = caches.orders_metadata.get(&client_order_id).unwrap();
        assert_eq!(meta.venue_order_id, Some(venue_order_id));
    }

    #[rstest]
    fn test_create_order_accepted_returns_none_without_metadata() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let ws_order = test_ws_order("OID-UNKNOWN", dec!(100.00), 10);

        let result = create_order_accepted(&ws_order, 1609459200, 0, &caches, account_id, clock);
        assert!(result.is_none());
        assert!(caches.venue_to_client_id.is_empty());
    }

    #[rstest]
    fn test_lookup_order_metadata_cid_fallback() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CID");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches.cid_to_client_order_id.insert(99, client_order_id);

        let mut ws_order = test_ws_order("UNKNOWN-OID", dec!(0), 0);
        ws_order.cid = Some(99);

        let found = lookup_order_metadata(&ws_order, &caches).expect("cid fallback should find");
        assert_eq!(found.client_order_id, client_order_id);
    }

    #[rstest]
    fn test_lookup_order_metadata_returns_none_when_unknown() {
        let caches = test_caches();
        let ws_order = test_ws_order("UNKNOWN-OID", dec!(0), 0);
        assert!(lookup_order_metadata(&ws_order, &caches).is_none());
    }

    #[rstest]
    #[case(true, LiquiditySide::Taker)]
    #[case(false, LiquiditySide::Maker)]
    fn test_create_order_filled_maps_liquidity_side(
        #[case] agg: bool,
        #[case] expected: LiquiditySide,
    ) {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-FILL");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-FILL");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(50500.00), 100);
        let execution = test_execution("TID-1", dec!(50500.00), 25, agg);

        let event = create_order_filled(
            &order, &execution, 1609459200, 0, &caches, account_id, clock,
        )
        .expect("should produce OrderFilled");

        assert_eq!(event.venue_order_id, venue_order_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.trade_id, TradeId::new("TID-1"));
        assert_eq!(event.last_qty, Quantity::new(25.0, 0));
        assert_eq!(event.last_px, Price::from("50500.00"));
        assert_eq!(event.liquidity_side, expected);
    }

    #[rstest]
    fn test_create_order_canceled_populates_identifiers() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-CXL");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-CXL");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        let event = create_order_canceled(&order, 1609459200, 0, &caches, account_id, clock)
            .expect("should produce OrderCanceled");

        assert_eq!(event.venue_order_id, Some(venue_order_id));
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.account_id, Some(account_id));
        assert_eq!(event.instrument_id, instrument_id);
    }

    #[rstest]
    fn test_create_order_expired_populates_identifiers() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-EXP");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-EXP");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        let event = create_order_expired(&order, 1609459200, 0, &caches, account_id, clock)
            .expect("should produce OrderExpired");

        assert_eq!(event.venue_order_id, Some(venue_order_id));
        assert_eq!(event.client_order_id, client_order_id);
    }

    #[rstest]
    fn test_create_order_rejected_sets_due_post_only_when_reason_matches() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-REJ");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(VenueOrderId::new("OID-REJ"), client_order_id);

        let order = test_ws_order("OID-REJ", dec!(100.00), 10);
        let reason = AX_POST_ONLY_REJECT;
        let event =
            create_order_rejected(&order, reason, 1609459200, 0, &caches, account_id, clock)
                .expect("should produce OrderRejected");

        assert_eq!(event.due_post_only, 1, "post-only reason should set flag");
        assert_eq!(event.reason, Ustr::from(reason));
    }

    #[rstest]
    fn test_create_order_rejected_clears_due_post_only_for_other_reasons() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-REJ-2");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(VenueOrderId::new("OID-REJ-2"), client_order_id);

        let order = test_ws_order("OID-REJ-2", dec!(100.00), 10);
        let event = create_order_rejected(
            &order,
            "INSUFFICIENT_MARGIN",
            1609459200,
            0,
            &caches,
            account_id,
            clock,
        )
        .expect("should produce OrderRejected");

        assert_eq!(event.due_post_only, 0);
        assert_eq!(event.reason, Ustr::from("INSUFFICIENT_MARGIN"));
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_removes_all_caches() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CLEAN");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-CLEAN");
        let cid_value = 123u64;

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        let mut order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        order.cid = Some(cid_value);

        cleanup_terminal_order_tracking(&order, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_via_cid_when_venue_missing() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CLEAN-CID");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let cid_value = 321u64;

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        // Venue id missing from cache
        let mut order = test_ws_order("OID-UNKNOWN", dec!(100.00), 10);
        order.cid = Some(cid_value);

        cleanup_terminal_order_tracking(&order, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_noop_when_unknown() {
        let caches = test_caches();
        let other = ClientOrderId::from("OTHER");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        caches
            .orders_metadata
            .insert(other, test_metadata(other, instrument_id));

        let order = test_ws_order("OID-NOT-TRACKED", dec!(100.00), 10);
        cleanup_terminal_order_tracking(&order, &caches);

        // Unrelated metadata still present
        assert_eq!(caches.orders_metadata.len(), 1);
    }

    #[rstest]
    fn test_cancel_on_disconnect_url_no_existing_query() {
        let mut url = "wss://example.com/orders/ws".to_string();
        let separator = if url.contains('?') { "&" } else { "?" };
        url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        assert_eq!(url, "wss://example.com/orders/ws?cancel_on_disconnect=true");
    }

    #[rstest]
    fn test_cancel_on_disconnect_url_with_existing_query() {
        let mut url = "wss://example.com/orders/ws?token=abc".to_string();
        let separator = if url.contains('?') { "&" } else { "?" };
        url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        assert_eq!(
            url,
            "wss://example.com/orders/ws?token=abc&cancel_on_disconnect=true"
        );
    }
}
