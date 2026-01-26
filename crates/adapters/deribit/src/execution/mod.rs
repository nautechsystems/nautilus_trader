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

//! Live execution client implementation for the Deribit adapter.

use std::{
    future::Future,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
        GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
        GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
        SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UnixNanos,
    datetime::NANOSECONDS_IN_SECOND,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce, TriggerType},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, Venue},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;

use crate::{
    common::consts::DERIBIT_VENUE,
    config::DeribitExecClientConfig,
    http::{client::DeribitHttpClient, models::DeribitCurrency, query::GetOrderStateParams},
    websocket::{
        auth::DERIBIT_EXECUTION_SESSION_NAME,
        client::DeribitWebSocketClient,
        messages::{DeribitOrderParams, NautilusWsMessage},
        parse::parse_user_order_msg,
    },
};

/// Deribit live execution client.
#[derive(Debug)]
pub struct DeribitExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: DeribitExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: DeribitHttpClient,
    ws_client: DeribitWebSocketClient,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl DeribitExecutionClient {
    /// Creates a new [`DeribitExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: DeribitExecClientConfig) -> anyhow::Result<Self> {
        let http_client = if config.has_api_credentials() {
            DeribitHttpClient::new_with_env(
                config.api_key.clone(),
                config.api_secret.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        } else {
            DeribitHttpClient::new(
                config.base_url_http.clone(),
                config.use_testnet,
                config.http_timeout_secs,
                config.max_retries,
                config.retry_delay_initial_ms,
                config.retry_delay_max_ms,
                None, // proxy_url
            )?
        };

        let mut ws_client = DeribitWebSocketClient::new(
            config.base_url_ws.clone(),
            config.api_key.clone(),
            config.api_secret.clone(),
            Some(20),
            config.use_testnet,
        )
        .context("failed to create WebSocket client for execution")?;
        // Set account ID for order/fill reports
        ws_client.set_account_id(core.account_id);

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
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    /// Spawns an async task for execution operations.
    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
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

    /// Aborts all pending async tasks.
    fn abort_pending_tasks(&self) {
        let mut tasks = self.pending_tasks.lock().expect(MUTEX_POISONED);
        for handle in tasks.drain(..) {
            handle.abort();
        }
    }

    /// Builds Deribit order parameters from a Nautilus order.
    fn build_order_params(order: &dyn Order) -> DeribitOrderParams {
        let order_type = match order.order_type() {
            OrderType::Limit => "limit",
            OrderType::Market => "market",
            OrderType::StopLimit => "stop_limit",
            OrderType::StopMarket => "stop_market",
            other => {
                log::warn!(
                    "Unsupported order type {other:?} for Deribit, falling back to limit order"
                );
                "limit"
            }
        }
        .to_string();

        let time_in_force = Some(
            match order.time_in_force() {
                TimeInForce::Gtc => "good_til_cancelled",
                TimeInForce::Ioc => "immediate_or_cancel",
                TimeInForce::Fok => "fill_or_kill",
                TimeInForce::Gtd => {
                    if order.expire_time().is_some() {
                        log::warn!(
                            "Deribit GTD orders expire at 8:00 UTC only - custom expire_time is ignored. \
                            For custom expiry times, use managed GTD with emulation_trigger."
                        );
                    }
                    "good_til_day"
                }
                other => {
                    log::warn!(
                        "Unsupported time_in_force {other:?} for Deribit, falling back to GTC"
                    );
                    "good_til_cancelled"
                }
            }
            .to_string(),
        );

        // Deribit's `valid_until` is a REQUEST timeout, not order expiry.
        // Deribit's `good_til_day` expires at end of trading session (8 UTC).
        let valid_until = None;

        // Map trigger type for stop orders
        let trigger = order.trigger_type().and_then(|tt| {
            match tt {
                TriggerType::LastPrice => Some("last_price".to_string()),
                TriggerType::MarkPrice => Some("mark_price".to_string()),
                TriggerType::IndexPrice => Some("index_price".to_string()),
                TriggerType::Default => Some("last_price".to_string()), // Deribit default
                _ => None,
            }
        });

        DeribitOrderParams {
            instrument_name: order.instrument_id().symbol.to_string(),
            amount: order.quantity().as_decimal(),
            order_type,
            label: Some(order.client_order_id().to_string()),
            price: order.price().map(|p| p.as_decimal()),
            time_in_force,
            post_only: if order.is_post_only() {
                Some(true)
            } else {
                None
            },
            reject_post_only: if order.is_post_only() {
                Some(true)
            } else {
                None
            },
            reduce_only: if order.is_reduce_only() {
                Some(true)
            } else {
                None
            },
            trigger_price: order.trigger_price().map(|p| p.as_decimal()),
            trigger,
            max_show: None,
            valid_until,
        }
    }

    /// Submits a single order to Deribit.
    ///
    /// This is the core submission logic shared by `submit_order` and `submit_order_list`.
    fn submit_single_order(&self, order: &OrderAny, task_name: &'static str) -> anyhow::Result<()> {
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Validate instrument belongs to Deribit venue
        // TODO: We can do this in a cenrtalized place (execution client adapter?) upstream
        if order.instrument_id().venue != *DERIBIT_VENUE {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_rejected_event(
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                &format!(
                    "Instrument {} does not belong to DERIBIT venue (got {})",
                    order.instrument_id(),
                    order.instrument_id().venue
                ),
                ts_event,
                false,
            );

            log::error!(
                "Cannot submit order: instrument {} does not belong to DERIBIT venue",
                order.instrument_id()
            );
            return Ok(());
        }

        let params = Self::build_order_params(order);
        let client_order_id = order.client_order_id();
        let trader_id = order.trader_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();

        log::debug!("OrderSubmitted client_order_id={client_order_id}");
        self.emitter.emit_order_submitted(order);

        let ws_client = self.ws_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task(task_name, async move {
            let result = ws_client
                .submit_order(
                    order_side,
                    params,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await;

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &format!("{task_name}-error: {e}"),
                    ts_event,
                    false,
                );
                return Err(e.into());
            }

            Ok(())
        });

        Ok(())
    }

    /// Spawns a stream handler to dispatch WebSocket messages to the execution engine.
    fn spawn_stream_handler(
        &mut self,
        stream: impl futures_util::Stream<Item = NautilusWsMessage> + Send + 'static,
    ) {
        if self.ws_stream_handle.is_some() {
            return;
        }

        let emitter = self.emitter.clone();

        let handle = get_runtime().spawn(async move {
            pin_mut!(stream);
            while let Some(message) = stream.next().await {
                dispatch_ws_message(message, &emitter);
            }
        });

        self.ws_stream_handle = Some(handle);
        log::info!("WebSocket stream handler started");
    }
}

#[async_trait(?Send)]
impl ExecutionClient for DeribitExecutionClient {
    fn is_connected(&self) -> bool {
        self.connected.load(Ordering::Acquire)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *DERIBIT_VENUE
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
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.started = true;

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, instrument_kinds={:?}, use_testnet={}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.instrument_kinds,
            self.config.use_testnet
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if !self.started {
            return Ok(());
        }

        self.started = false;
        self.connected.store(false, Ordering::Release);
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        // Check if credentials are available before requesting account state
        if !self.config.has_api_credentials() {
            anyhow::bail!("Missing API credentials; set Deribit environment variables");
        }

        // Set account ID for order/fill reports
        self.ws_client.set_account_id(self.core.account_id);

        // Fetch and cache instruments in both HTTP client and WebSocket client
        if !self.instruments_initialized.load(Ordering::Acquire) {
            for kind in &self.config.instrument_kinds {
                let instruments = self
                    .http_client
                    .request_instruments(DeribitCurrency::ANY, Some(*kind))
                    .await
                    .with_context(|| format!("failed to request instruments for {kind:?}"))?;

                if instruments.is_empty() {
                    log::warn!("No instruments returned for {kind:?}");
                    continue;
                }

                log::info!("Fetched {} {kind:?} instruments", instruments.len());
                self.ws_client.cache_instruments(instruments.clone());
                self.http_client.cache_instruments(instruments);
            }
            self.instruments_initialized.store(true, Ordering::Release);
        }

        // Fetch initial account state
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request account state")?;

        self.emitter.send_account_state(account_state);

        self.ws_client
            .connect()
            .await
            .context("failed to connect WebSocket client for execution")?;

        self.ws_client
            .authenticate_session(DERIBIT_EXECUTION_SESSION_NAME)
            .await
            .map_err(|e| anyhow::anyhow!("failed to authenticate WebSocket session: {e}"))?;

        log::info!("WebSocket client authenticated for execution");

        // Subscribe to user order and trade updates for all instruments
        self.ws_client
            .subscribe_user_orders()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to user orders: {e}"))?;
        self.ws_client
            .subscribe_user_trades()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to user trades: {e}"))?;
        self.ws_client
            .subscribe_user_portfolio()
            .await
            .map_err(|e| anyhow::anyhow!("failed to subscribe to user portfolio: {e}"))?;

        log::info!("Subscribed to user order, trade, and portfolio updates");

        // Spawn stream handler to dispatch WebSocket messages to the execution engine
        let stream = self.ws_client.stream();
        self.spawn_stream_handler(stream);

        self.connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        self.abort_pending_tasks();

        // Abort stream handler
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        // Close WebSocket client
        if let Err(e) = self.ws_client.close().await {
            log::warn!("Error closing WebSocket client: {e}");
        }

        self.connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // If venue_order_id is provided, fetch the specific order by ID
        if let Some(venue_order_id) = &cmd.venue_order_id {
            let params = GetOrderStateParams {
                order_id: venue_order_id.to_string(),
            };
            let ts_init = self.clock.get_time_ns();

            match self.http_client.inner.get_order_state(params).await {
                Ok(response) => {
                    if let Some(order) = response.result {
                        let symbol = ustr::Ustr::from(&order.instrument_name);
                        if let Some(instrument) = self.http_client.get_instrument(&symbol) {
                            let report = parse_user_order_msg(
                                &order,
                                &instrument,
                                self.core.account_id,
                                ts_init,
                            )?;
                            return Ok(Some(report));
                        } else {
                            log::warn!(
                                "Instrument {} not in cache for order {}",
                                order.instrument_name,
                                order.order_id
                            );
                        }
                    }
                }
                Err(e) => {
                    log::warn!("Failed to get order state: {e}");
                }
            }
            return Ok(None);
        }

        // If client_order_id is provided, search through open orders
        if let Some(client_order_id) = &cmd.client_order_id {
            let reports = self
                .http_client
                .request_order_status_reports(
                    self.core.account_id,
                    cmd.instrument_id,
                    None,
                    None,
                    true, // open_only for efficiency
                )
                .await?;

            // Filter by client_order_id
            for report in reports {
                if report.client_order_id == Some(*client_order_id) {
                    return Ok(Some(report));
                }
            }
        }

        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.http_client
            .request_order_status_reports(
                self.core.account_id,
                cmd.instrument_id,
                cmd.start,
                cmd.end,
                cmd.open_only,
            )
            .await
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let mut reports = self
            .http_client
            .request_fill_reports(self.core.account_id, cmd.instrument_id, cmd.start, cmd.end)
            .await?;

        // Filter by venue_order_id if provided
        if let Some(venue_order_id) = &cmd.venue_order_id {
            reports.retain(|r| r.venue_order_id.to_string() == venue_order_id.to_string());
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.http_client
            .request_position_status_reports(self.core.account_id, cmd.instrument_id)
            .await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");
        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins
                .saturating_mul(60)
                .saturating_mul(NANOSECONDS_IN_SECOND);
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false) // get all orders for mass status
            .start(start)
            .build()
            .context("Failed to build GenerateOrderStatusReports")?;

        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .context("Failed to build GenerateFillReports")?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .context("Failed to build GeneratePositionStatusReports")?;

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
            *DERIBIT_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
                .await
                .context("failed to query account state (check API credentials are valid)")?;

            emitter.send_account_state(account_state);
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();

        // Extract venue order ID (Deribit's order_id)
        let order_id = cmd
            .venue_order_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("venue_order_id required for query_order"))?
            .to_string();

        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        log::info!("Querying order state: order_id={order_id}, client_order_id={client_order_id}");

        // Spawn async task to query order state via WebSocket
        // Response will be dispatched through the WebSocket stream handler as OrderStatusReport
        self.spawn_task("query_order", async move {
            ws_client
                .query_order(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Query order state failed: {e}"))?;
            Ok(())
        });

        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;
        self.submit_single_order(&order, "submit_order")
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        if cmd.order_list.orders.is_empty() {
            log::debug!("submit_order_list called with empty order list");
            return Ok(());
        }

        log::info!(
            "Submitting order list {} with {} orders for instrument={}",
            cmd.order_list.id,
            cmd.order_list.orders.len(),
            cmd.instrument_id
        );

        // Deribit doesn't have native batch order submission
        // Loop through and submit each order individually using shared helper
        for order in &cmd.order_list.orders {
            self.submit_single_order(order, "submit_order_list_item")?;
        }

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();

        // Extract venue order ID (Deribit's order_id)
        let order_id = cmd
            .venue_order_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("venue_order_id required for modify_order"))?
            .to_string();

        // Extract quantity - if not provided, get from order in cache
        let quantity = if let Some(qty) = cmd.quantity {
            qty
        } else {
            // Get order from cache to use its current quantity
            let cache = self.core.cache();
            let order = cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;
            order.quantity()
        };

        let price = cmd
            .price
            .ok_or_else(|| anyhow::anyhow!("price required for modify_order"))?;

        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        log::info!(
            "Modifying order: order_id={order_id}, quantity={quantity}, price={price}, client_order_id={client_order_id}"
        );

        // Spawn async task to send modify via WebSocket
        self.spawn_task("modify_order", async move {
            if let Err(e) = ws_client
                .modify_order(
                    &order_id,
                    quantity,
                    price,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
            {
                log::error!(
                    "Modify order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                );

                let ts_event = clock.get_time_ns();
                emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("modify-order-error: {e}"),
                    ts_event,
                );

                anyhow::bail!("Modify order failed: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let ws_client = self.ws_client.clone();

        // Extract venue order ID (Deribit's order_id)
        let order_id = cmd
            .venue_order_id
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("venue_order_id required for cancel_order"))?
            .to_string();

        let client_order_id = cmd.client_order_id;
        let trader_id = cmd.trader_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        log::info!("Canceling order: order_id={order_id}, client_order_id={client_order_id}");

        // Spawn async task to send cancel via WebSocket
        self.spawn_task("cancel_order", async move {
            if let Err(e) = ws_client
                .cancel_order(
                    &order_id,
                    client_order_id,
                    trader_id,
                    strategy_id,
                    instrument_id,
                )
                .await
            {
                log::error!(
                    "Cancel order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                );

                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );

                anyhow::bail!("Cancel order failed: {e}");
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        // If NoOrderSide, use efficient bulk cancel via Deribit API
        if cmd.order_side == OrderSide::NoOrderSide {
            log::info!(
                "Cancelling all orders: instrument={instrument_id}, order_side=NoOrderSide (bulk)"
            );

            let ws_client = self.ws_client.clone();
            self.spawn_task("cancel_all_orders", async move {
                if let Err(e) = ws_client.cancel_all_orders(instrument_id, None).await {
                    log::error!("Cancel all orders failed for instrument {instrument_id}: {e}");
                    anyhow::bail!("Cancel all orders failed: {e}");
                }
                Ok(())
            });

            return Ok(());
        }

        // For specific side (Buy/Sell), filter from cache and cancel individually
        // Deribit API doesn't support side filtering, so we implement it locally
        log::info!(
            "Cancelling orders by side: instrument={}, order_side={}",
            instrument_id,
            cmd.order_side
        );

        let orders_to_cancel: Vec<_> = {
            let cache = self.core.cache();
            let open_orders = cache.orders_open(None, Some(&instrument_id), None, None, None);

            open_orders
                .into_iter()
                .filter(|order| order.order_side() == cmd.order_side)
                .filter_map(|order| {
                    let venue_order_id = order.venue_order_id()?;
                    Some((
                        venue_order_id.to_string(),
                        order.client_order_id(),
                        order.instrument_id(),
                        Some(venue_order_id),
                    ))
                })
                .collect()
        };

        if orders_to_cancel.is_empty() {
            log::debug!(
                "No open {} orders to cancel for {}",
                cmd.order_side,
                instrument_id
            );
            return Ok(());
        }

        log::info!(
            "Cancelling {} {} orders for {}",
            orders_to_cancel.len(),
            cmd.order_side,
            instrument_id
        );

        // Cancel each matching order individually
        for (venue_order_id_str, client_order_id, order_instrument_id, venue_order_id) in
            orders_to_cancel
        {
            let ws_client = self.ws_client.clone();
            let trader_id = cmd.trader_id;
            let strategy_id = cmd.strategy_id;
            let emitter = self.emitter.clone();
            let clock = self.clock;

            self.spawn_task("cancel_order_by_side", async move {
                if let Err(e) = ws_client
                    .cancel_order(
                        &venue_order_id_str,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        order_instrument_id,
                    )
                    .await
                {
                    log::error!(
                        "Cancel order failed: order_id={venue_order_id_str}, client_order_id={client_order_id}, error={e}"
                    );

                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        order_instrument_id,
                        client_order_id,
                        venue_order_id,
                        &format!("cancel-order-error: {e}"),
                        ts_event,
                    );
                }
                Ok(())
            });
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        if cmd.cancels.is_empty() {
            log::debug!("batch_cancel_orders called with empty cancels list");
            return Ok(());
        }

        log::info!(
            "Batch cancelling {} orders for instrument={}",
            cmd.cancels.len(),
            cmd.instrument_id
        );

        // Deribit doesn't have native batch cancel by order ID
        // Loop through and cancel each order individually
        for cancel in &cmd.cancels {
            let order_id = match &cancel.venue_order_id {
                Some(id) => id.to_string(),
                None => {
                    log::warn!(
                        "Cannot cancel order {} - no venue_order_id",
                        cancel.client_order_id
                    );

                    // Emit OrderCancelRejected event for missing venue_order_id
                    let ts_event = self.clock.get_time_ns();
                    self.emitter.emit_order_cancel_rejected_event(
                        cancel.strategy_id,
                        cancel.instrument_id,
                        cancel.client_order_id,
                        None,
                        "venue_order_id required for cancel",
                        ts_event,
                    );
                    continue;
                }
            };

            let ws_client = self.ws_client.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;
            let client_order_id = cancel.client_order_id;
            let trader_id = cancel.trader_id;
            let strategy_id = cancel.strategy_id;
            let instrument_id = cancel.instrument_id;

            self.spawn_task("batch_cancel_order", async move {
                if let Err(e) = ws_client
                    .cancel_order(
                        &order_id,
                        client_order_id,
                        trader_id,
                        strategy_id,
                        instrument_id,
                    )
                    .await
                {
                    log::error!(
                        "Batch cancel order failed: order_id={order_id}, client_order_id={client_order_id}, error={e}"
                    );

                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        None,
                        &format!("batch-cancel-error: {e}"),
                        ts_event,
                    );

                    anyhow::bail!("Batch cancel order failed: {e}");
                }
                Ok(())
            });
        }

        Ok(())
    }
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(message: NautilusWsMessage, emitter: &ExecutionEventEmitter) {
    match message {
        NautilusWsMessage::AccountState(state) => {
            emitter.send_account_state(state);
        }
        NautilusWsMessage::OrderStatusReports(reports) => {
            log::debug!("Processing {} order status report(s)", reports.len());
            for report in reports {
                emitter.send_order_status_report(report);
            }
        }
        NautilusWsMessage::FillReports(reports) => {
            log::debug!("Processing {} fill report(s)", reports.len());
            for report in reports {
                emitter.send_fill_report(report);
            }
        }
        NautilusWsMessage::OrderRejected(event) => {
            emitter.send_order_event(OrderEventAny::Rejected(event));
        }
        NautilusWsMessage::OrderAccepted(event) => {
            emitter.send_order_event(OrderEventAny::Accepted(event));
        }
        NautilusWsMessage::OrderCanceled(event) => {
            emitter.send_order_event(OrderEventAny::Canceled(event));
        }
        NautilusWsMessage::OrderExpired(event) => {
            emitter.send_order_event(OrderEventAny::Expired(event));
        }
        NautilusWsMessage::OrderUpdated(event) => {
            emitter.send_order_event(OrderEventAny::Updated(event));
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            emitter.send_order_event(OrderEventAny::CancelRejected(event));
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            emitter.send_order_event(OrderEventAny::ModifyRejected(event));
        }
        NautilusWsMessage::Error(e) => {
            log::warn!("WebSocket error: {e}");
        }
        NautilusWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        NautilusWsMessage::Authenticated(auth) => {
            log::debug!("WebSocket authenticated: scope={}", auth.scope);
        }
        NautilusWsMessage::Data(_)
        | NautilusWsMessage::Deltas(_)
        | NautilusWsMessage::Instrument(_)
        | NautilusWsMessage::FundingRates(_)
        | NautilusWsMessage::Raw(_) => {
            // Data messages are handled by the data client, not execution
            log::trace!("Ignoring data message in execution client");
        }
    }
}
