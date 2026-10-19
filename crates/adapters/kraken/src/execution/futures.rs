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

//! Kraken Futures execution client implementation.

use std::{
    collections::{HashMap, HashSet},
    future::Future,
    sync::Arc,
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use indexmap::IndexMap;
use jiff::Timestamp;
use nautilus_common::{
    cache::{Cache, InstrumentLookupError},
    clients::ExecutionClient,
    live::runner::get_exec_event_sender,
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
        ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    AtomicMap, DurationNanos, Params, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl, execution::failure::CommandFailure,
    task::TaskGroup,
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType},
    events::OrderDeniedReason,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Quantity},
};
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{
    absent_unless_unread, cached_order_for_status_command, command_failure_from_cancel_error,
    command_failure_from_futures_batch_error, command_failure_from_futures_batch_item,
    command_failure_from_modify_error, command_failure_from_submit_error, earliest_venue_time,
    status_command_label,
};
use crate::{
    common::{
        consts::{KRAKEN_FUTURES_MAX_CLIENT_ORDER_ID_LEN, KRAKEN_VENUE},
        credential::KrakenCredential,
        enums::{KrakenApiResult, KrakenProductType, KrakenSendStatus, product_type_from_symbol},
        parse::truncate_cl_ord_id,
    },
    config::KrakenExecutionClientConfig,
    http::{
        KrakenFuturesHttpClient, KrakenHttpError,
        futures::{
            client::{KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND, order_identifier, supersedes},
            models::FuturesBatchCancelStatus,
            query::KrakenFuturesBatchCancelItem,
        },
    },
    websocket::{
        dispatch::{self, OrderIdentity, WsDispatchState},
        futures::{client::KrakenFuturesWebSocketClient, messages::KrakenFuturesWsMessage},
    },
};

const FUTURES_BATCH_CANCEL_LIMIT: usize = 50;

/// Maximum order IDs per `/orders/status` request for Kraken Futures API.
const FUTURES_ORDERS_STATUS_LIMIT: usize = 50;

/// Allowance for skew between the node's clock, which stamps an order's creation, and the venue's,
/// which stamps its placement, kept short so an older order whose client order ID shortens alike
/// stays withheld.
const ORDERS_STATUS_CLOCK_SKEW: DurationNanos = DurationNanos::from_secs(5);

/// Kraken Futures execution client.
///
/// Provides order management, account operations, and position management
/// for Kraken Futures markets.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenFuturesExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: KrakenExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http: KrakenFuturesHttpClient,
    ws: KrakenFuturesWebSocketClient,
    cancellation_token: CancellationToken,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: Arc<AtomicMap<String, Quantity>>,
    ws_dispatch_state: Arc<WsDispatchState>,
}

impl KrakenFuturesExecutionClient {
    /// Creates a new [`KrakenFuturesExecutionClient`].
    pub fn new(
        core: ExecutionClientCore,
        config: KrakenExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Margin,
            None,
        );

        let session_tasks = TaskGroup::new();
        let cancellation_token = session_tasks.cancellation_token();
        let pending_tasks = TaskGroup::new();
        let api_key = config.api_key.expose_secret().to_owned();
        let api_secret = config.api_secret.expose_secret().to_owned();
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());

        let http = KrakenFuturesHttpClient::with_credentials(
            api_key.clone(),
            api_secret.clone(),
            config.environment,
            config.base_url.clone(),
            config.timeout_secs,
            Some(config.max_retries),
            None,
            None,
            proxy_url.clone(),
            config
                .max_requests_per_second
                .unwrap_or(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND),
        )?;

        let credential = KrakenCredential::new(api_key, api_secret);
        let ws = KrakenFuturesWebSocketClient::with_credentials(
            config.ws_url(),
            config.heartbeat_interval_secs,
            Some(credential),
            config.auth_timeout_secs,
            config.transport_backend,
            proxy_url,
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*KRAKEN_VENUE),
            "kraken-futures-user-streams",
        ));

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http,
            ws,
            cancellation_token,
            session_tasks,
            pending_tasks,
            instruments: Arc::new(AtomicMap::new()),
            truncated_id_map: Arc::new(AtomicMap::new()),
            order_instrument_map: Arc::new(AtomicMap::new()),
            venue_client_map: Arc::new(AtomicMap::new()),
            venue_order_qty: Arc::new(AtomicMap::new()),
            ws_dispatch_state: Arc::new(WsDispatchState::new()),
        })
    }

    fn register_order_identity(&self, order: &OrderAny) {
        self.ws_dispatch_state.register_identity(
            order.client_order_id(),
            OrderIdentity {
                strategy_id: order.strategy_id(),
                instrument_id: order.instrument_id(),
                order_side: order.order_side(),
                order_type: order.order_type(),
                quantity: order.quantity(),
            },
        );
    }

    /// Returns a reference to the clock.
    #[must_use]
    pub fn clock(&self) -> &'static AtomicTime {
        self.clock
    }

    /// Returns a reference to the event emitter.
    #[must_use]
    pub fn emitter(&self) -> &ExecutionEventEmitter {
        &self.emitter
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let future = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            log::warn!("Skipping Kraken Futures {description} after shutdown began: {e}");
        }
    }

    async fn finish_tasks(&self) -> anyhow::Result<()> {
        let (session_result, pending_result) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2)),
            self.pending_tasks
                .finish_shutdown(Duration::from_secs(2), Duration::from_secs(2)),
        );
        session_result.context("failed to finish Kraken Futures execution session tasks")?;
        pending_result.context("failed to finish Kraken Futures execution command tasks")?;
        Ok(())
    }

    async fn prepare_task_groups(&mut self) -> anyhow::Result<()> {
        if !self.session_tasks.is_open() || !self.pending_tasks.is_open() {
            self.session_tasks.begin_shutdown();
            self.pending_tasks.begin_shutdown();
            self.finish_tasks().await?;
            self.session_tasks
                .start_generation()
                .context("failed to start Kraken Futures execution session task generation")?;
            self.pending_tasks
                .start_generation()
                .context("failed to start Kraken Futures execution command task generation")?;
            self.cancellation_token = self.session_tasks.cancellation_token();
        }
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.http.cancel_all_requests();
        self.pending_tasks.begin_shutdown();
        let ws_result = self.ws.close().await;
        self.session_tasks.begin_shutdown();
        let tasks_result = self.finish_tasks().await;
        self.core.set_disconnected();
        tasks_result?;
        Ok(ws_result?)
    }

    fn submit_single_order(&self, order: &OrderAny, task_name: &'static str) {
        if order.is_closed() {
            log::warn!(
                "Cannot submit closed order: client_order_id={}",
                order.client_order_id()
            );
            return;
        }

        if let Err(reason) = validate_order(order) {
            self.emitter.emit_order_denied(order, &reason.to_string());
            return;
        }

        let account_id = self.core.account_id;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let trigger_type = order.trigger_type();
        let is_reduce_only = order.is_reduce_only();
        let is_post_only = order.is_post_only();

        log::debug!("OrderSubmitted: client_order_id={client_order_id}");
        self.register_order_identity(order);
        self.emitter.emit_order_submitted(order);

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let dispatch_state = self.ws_dispatch_state.clone();

        self.spawn_task(task_name, async move {
            let result = http
                .submit_order(
                    account_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    trigger_type,
                    is_reduce_only,
                    is_post_only,
                )
                .await;

            match result {
                Ok(_) => {}
                Err(e) => match command_failure_from_submit_error(&e) {
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "{task_name} outcome is ambiguous for client_order_id={client_order_id}: {reason}"
                        );
                    }
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        let ts_event = clock.get_time_ns();
                        let error_msg = format!("{task_name} error: {reason}");
                        let due_post_only = error_msg.contains("POST_ONLY_REJECTED");
                        dispatch_state.cleanup_terminal(&client_order_id);
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &error_msg,
                            ts_event,
                            due_post_only,
                        );
                    }
                },
            }
            Ok(())
        });
    }

    fn cancel_single_order(&self, cmd: &CancelOrder) {
        let account_id = self.core.account_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        log::debug!(
            "Canceling order: venue_order_id={venue_order_id:?}, client_order_id={client_order_id}"
        );

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            if let Err(failure) = cancel_order_for_futures(
                &http,
                account_id,
                instrument_id,
                Some(client_order_id),
                venue_order_id,
            )
            .await
            {
                handle_cancel_failure(
                    &emitter,
                    clock,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    failure,
                );
            }
            Ok(())
        });
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let mut rx = self
            .ws
            .take_output_rx()
            .context("Failed to take futures WebSocket output receiver")?;
        let emitter = self.emitter.clone();
        let instruments = self.instruments.clone();
        let truncated_id_map = self.truncated_id_map.clone();
        let order_instrument_map = self.order_instrument_map.clone();
        let venue_client_map = self.venue_client_map.clone();
        let venue_order_qty = self.venue_order_qty.clone();
        let dispatch_state = self.ws_dispatch_state.clone();
        let account_id = self.core.account_id;
        let clock = self.clock;
        let cancellation_token = self.cancellation_token.clone();

        let future = async move {
            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Futures execution message handler cancelled");
                        break;
                    }
                    msg = rx.recv() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(
                                    ws_msg,
                                    &emitter,
                                    &dispatch_state,
                                    &instruments,
                                    &truncated_id_map,
                                    &order_instrument_map,
                                    &venue_client_map,
                                    &venue_order_qty,
                                    account_id,
                                    clock,
                                );
                            }
                            None => {
                                log::debug!("Futures execution WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        };

        self.session_tasks
            .spawn(future)
            .context("failed to register Kraken Futures execution stream task")
    }

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: KrakenFuturesWsMessage,
        emitter: &ExecutionEventEmitter,
        dispatch_state: &Arc<WsDispatchState>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
        order_instrument_map: &Arc<AtomicMap<String, InstrumentId>>,
        venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
        venue_order_qty: &Arc<AtomicMap<String, Quantity>>,
        account_id: AccountId,
        clock: &'static AtomicTime,
    ) {
        let ts_init = clock.get_time_ns();

        match msg {
            KrakenFuturesWsMessage::OpenOrdersDelta(delta) => {
                dispatch::futures::open_orders_delta(
                    &delta,
                    dispatch_state,
                    emitter,
                    instruments,
                    truncated_id_map,
                    order_instrument_map,
                    venue_client_map,
                    venue_order_qty,
                    account_id,
                    ts_init,
                );
            }
            KrakenFuturesWsMessage::OpenOrdersCancel(cancel) => {
                dispatch::futures::open_orders_cancel(
                    &cancel,
                    dispatch_state,
                    emitter,
                    truncated_id_map,
                    order_instrument_map,
                    venue_client_map,
                    venue_order_qty,
                    account_id,
                    ts_init,
                );
            }
            KrakenFuturesWsMessage::FillsDelta(fills_delta) => {
                dispatch::futures::fills_delta(
                    &fills_delta,
                    dispatch_state,
                    emitter,
                    instruments,
                    truncated_id_map,
                    venue_client_map,
                    account_id,
                    ts_init,
                );
            }
            KrakenFuturesWsMessage::Challenge(challenge) => {
                log::debug!("Received challenge: length={}", challenge.len());
            }
            KrakenFuturesWsMessage::Reconnected => {
                log::info!("Futures execution WebSocket reconnected");
            }
            KrakenFuturesWsMessage::Ticker(_)
            | KrakenFuturesWsMessage::Trade(_)
            | KrakenFuturesWsMessage::BookSnapshot(_)
            | KrakenFuturesWsMessage::BookDelta(_) => {}
        }
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

    fn modify_single_order(&self, cmd: &ModifyOrder) {
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let quantity = cmd.quantity;
        let price = cmd.price;

        log::debug!(
            "Modifying order: venue_order_id={venue_order_id:?}, client_order_id={client_order_id}"
        );

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            match http
                .modify_order(
                    instrument_id,
                    Some(client_order_id),
                    venue_order_id,
                    quantity,
                    price,
                    None,
                )
                .await
            {
                Ok(_) => {}
                Err(e) => match command_failure_from_modify_error(&e) {
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "modify_order outcome is ambiguous for client_order_id={client_order_id}: {reason}"
                        );
                    }
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_modify_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            venue_order_id,
                            &format!("modify-order error: {reason}"),
                            ts_event,
                        );
                    }
                },
            }
            Ok(())
        });
    }
}

#[async_trait(?Send)]
impl ExecutionClient for KrakenFuturesExecutionClient {
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
        *KRAKEN_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event, info);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.is_started() {
            return Ok(());
        }

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}, product_type=Futures, environment={:?}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.http.cancel_all_requests();
        self.session_tasks.begin_shutdown();
        self.pending_tasks.begin_shutdown();
        self.ws.begin_shutdown();
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.session_tasks.is_open() && self.pending_tasks.is_open()
        {
            return Ok(());
        }

        self.http.reset_cancellation_token();
        self.prepare_task_groups().await?;

        if !self.core.instruments_initialized() {
            let instruments = self
                .http
                .request_instruments()
                .await
                .context("Failed to load Kraken futures instruments")?;
            log::debug!("Loaded {} Futures instruments", instruments.len());
            self.http.cache_instruments(&instruments);
            self.core.set_instruments_initialized();
        }

        self.instruments.rcu(|m| {
            for instrument in self.http.instruments_cache.load().values() {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        let session_result = async {
            self.ws
                .connect()
                .await
                .context("Failed to connect futures WebSocket")?;
            self.ws
                .wait_until_active(10.0)
                .await
                .context("Futures WebSocket failed to become active")?;

            self.ws
                .authenticate()
                .await
                .context("Failed to authenticate futures WebSocket")?;

            let account_state = self
                .http
                .request_account_state(self.core.account_id)
                .await
                .context("Failed to request Kraken futures account state")?;

            if !account_state.balances.is_empty() {
                log::debug!(
                    "Received account state with {} balance(s)",
                    account_state.balances.len()
                );
            }
            self.emitter.send_account_state(account_state);
            self.await_account_registered(30.0).await?;

            self.spawn_message_handler()?;

            self.ws
                .subscribe_executions()
                .await
                .context("Failed to subscribe to executions")?;

            log::debug!("Futures WebSocket authenticated and subscribed to executions");

            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Kraken Futures execution startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.teardown_partial_connect().await?;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        log::debug!(
            "Generating order status report: venue_order_id={:?}, client_order_id={:?}",
            cmd.venue_order_id,
            cmd.client_order_id
        );

        let account_id = self.core.account_id;
        let cached = cached_order_for_status_command(&self.core.cache(), cmd);
        let queried = QueriedOrder::new(cmd, cached.as_ref());

        // Scoped to the command's instrument, so an unresolvable open order on another contract
        // is out of scope for this lookup rather than failing it.
        let (mut reports, coverage) = self
            .http
            .request_order_status_reports_checked(account_id, cmd.instrument_id, None, None, false)
            .await?;
        name_reports_by_cached_order(&self.core.cache(), &mut reports);

        let matched = queried.find_report(&self.core.cache(), &reports)?;
        let mut stray_rows = queried.has_stray_report(&self.core.cache(), &reports);

        if let Some(mut report) = matched {
            // A terminal history match is priced from its fills; short of them the query fails so
            // the engine defers, as the orders-status window does below. A failed fills read counts
            // as an empty page, so a cached order holding every execution still prices it.
            if is_unpriced_terminal_report(&report) {
                let fills = self
                    .http
                    .request_fill_reports(account_id, Some(report.instrument_id), None, None)
                    .await;
                let page = fills.as_deref().unwrap_or_default();

                if let Err(covered) = price_from_fills(&mut report, cached.as_ref(), page) {
                    fills?;
                    anyhow::bail!(
                        "Order {} executed per the order history with fills covering {covered} \
                         of {}; deferring until the fills feed prices it",
                        report.venue_order_id,
                        report.filled_qty,
                    );
                }
            }

            return Ok(Some(report));
        }

        // Absence answers that the venue has no such order, so it needs a read that rules the
        // order out; otherwise reconciliation defers.
        let Some(order) = cached else {
            if stray_rows {
                anyhow::bail!(
                    "No report names order {:?}, but a venue row of another venue order carries \
                     its client order ID; deferring rather than reporting it missing",
                    queried.client_order_id,
                );
            }
            return absent_unless_unread(
                &coverage,
                cmd.venue_order_id,
                None,
                &status_command_label(cmd, cmd.venue_order_id),
            );
        };

        // Held orders never appear on /openorders; query the 5-second window
        let order_ids: Vec<String> = queried
            .venue_order_id
            .map(|id| id.to_string())
            .into_iter()
            .collect();
        let cli_ord_ids: Vec<String> = venue_client_order_ids(&order.client_order_id()).collect();

        let mut recent_reports = self
            .http
            .request_orders_status_reports(account_id, &order_ids, &cli_ord_ids)
            .await?;
        name_reports_by_cached_order(&self.core.cache(), &mut recent_reports);

        let matched_recent = queried.find_report(&self.core.cache(), &recent_reports)?;
        stray_rows |= queried.has_stray_report(&self.core.cache(), &recent_reports);

        // Window filled reports have no avg_px; price them from fills below
        if matched_recent
            .as_ref()
            .is_some_and(|report| report.order_status != OrderStatus::Filled)
        {
            return Ok(matched_recent);
        }

        let now = Timestamp::now();
        let start = now - Duration::from_secs(5 * 60);
        let mut fills = self
            .http
            .request_fill_reports(
                account_id,
                Some(order.instrument_id()),
                Some(start),
                Some(now),
            )
            .await?;
        name_fills_by_cached_order(&self.core.cache(), &mut fills);

        let synthesized =
            synthesize_filled_order_status_report(&self.core.cache(), &queried, &order, &fills);

        match (synthesized, matched_recent) {
            (Some(report), _) => Ok(Some(report)),
            // Unpriced filled reports would close at the order price
            (None, Some(_)) => anyhow::bail!(
                "Order {} fully executed in the orders-status window without visible \
                 fills; deferring until the fills feed prices it",
                order.client_order_id(),
            ),
            (None, None) if stray_rows || queried.has_stray_fill(&self.core.cache(), &fills) => {
                anyhow::bail!(
                    "No report names order {}, yet a venue row or fill that may be its own carries \
                     its client order ID; deferring rather than reporting it missing",
                    order.client_order_id(),
                )
            }
            (None, None) => absent_unless_unread(
                &coverage,
                cmd.venue_order_id.or(order.venue_order_id()),
                Some(earliest_venue_time(&order)),
                order.client_order_id().as_str(),
            ),
        }
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        log::debug!(
            "Generating order status reports: instrument_id={:?}, open_only={}",
            cmd.instrument_id,
            cmd.open_only
        );

        let account_id = self.core.account_id;
        let start = cmd.start.map(Timestamp::from);
        let end = cmd.end.map(Timestamp::from);
        let mut reports = self
            .http
            .request_order_status_reports(account_id, cmd.instrument_id, start, end, cmd.open_only)
            .await?;

        // A terminal history report with an executed quantity carries no price: it is priced from
        // its fills, else withheld. A withheld cached order counts as missing and the engine
        // resolves it through the single-order query, which prices it or defers; an uncached one
        // stays withheld until its fills reach the fills page.
        if reports.iter().any(is_unpriced_terminal_report) {
            // A failed fills read counts as an empty page: a cached order holding every execution
            // still prices its report, and only the reports nothing covers are withheld.
            let fills = match self
                .http
                .request_fill_reports(account_id, cmd.instrument_id, None, None)
                .await
            {
                Ok(fills) => fills,
                Err(e) => {
                    log::warn!("Failed to read fills to price executed history orders: {e}");
                    Vec::new()
                }
            };
            let cache = self.core.cache();
            reports.retain_mut(|report| {
                if !is_unpriced_terminal_report(report) {
                    return true;
                }

                let cached = cached_order_for_report(&cache, report);

                match price_from_fills(report, cached.as_ref(), &fills) {
                    Ok(()) => true,
                    Err(covered) => {
                        log::warn!(
                            "Withholding executed order {} from the bulk response: fills cover \
                             {covered} of {}",
                            report.venue_order_id,
                            report.filled_qty,
                        );
                        false
                    }
                }
            });
        }

        if cmd.open_only {
            let (extension, _) = self
                .reports_for_open_orders_absent_from_venue(account_id, cmd.instrument_id, &reports)
                .await?;

            let (window_only, window_stated) = fold_window_into_venue_read(&mut reports, extension);

            // The window carries no fill price, so a filled report would be inferred at the
            // order price. A canceled one with executions passes, since reconciliation waits for
            // its fills rather than infer them.
            reports.retain(|report| {
                let withhold = window_stated.contains(&report.venue_order_id)
                    && report.order_status == OrderStatus::Filled;

                if withhold {
                    log::warn!(
                        "Withholding fully executed order {} from the bulk response: the \
                         orders-status window carries no fill price",
                        report.venue_order_id,
                    );
                }
                !withhold
            });

            for report in window_only {
                if report.order_status == OrderStatus::Filled {
                    log::warn!(
                        "Withholding fully executed order {} from the bulk response: the \
                         orders-status window carries no fill price",
                        report.venue_order_id,
                    );
                    continue;
                }

                reports.push(report);
            }
        }

        name_reports_by_cached_order(&self.core.cache(), &mut reports);
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        log::debug!(
            "Generating fill reports: instrument_id={:?}",
            cmd.instrument_id
        );

        let account_id = self.core.account_id;
        let start = cmd.start.map(Timestamp::from);
        let end = cmd.end.map(Timestamp::from);
        let mut reports = self
            .http
            .request_fill_reports(account_id, cmd.instrument_id, start, end)
            .await?;

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id == venue_order_id);
        }

        name_fills_by_cached_order(&self.core.cache(), &mut reports);
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        log::debug!(
            "Generating position status reports: instrument_id={:?}",
            cmd.instrument_id
        );

        let account_id = self.core.account_id;
        self.http
            .request_position_status_reports(account_id, cmd.instrument_id)
            .await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::debug!("Generating mass status: lookback_mins={lookback_mins:?}");

        let ts_init = self.clock.get_time_ns();
        // Saturating arithmetic: an unclamped lookback must not overflow, and a cutoff before the
        // epoch must not panic converting into `UnixNanos`.
        let lookback_start = lookback_mins.map(|mins| {
            let nanos = mins.saturating_mul(60).saturating_mul(1_000_000_000);
            ts_init.saturating_sub(DurationNanos::new(nanos))
        });
        let start = lookback_start.map(Timestamp::from);
        let account_id = self.core.account_id;

        // Read the order-event history as well as open orders, matching the shared default. An
        // order that reached a terminal state while the node was down is only visible there. The
        // venue can refuse the history page, since every `/history` endpoint draws on one token
        // pool; a refusal degrades startup to the open-only read and marks the set incomplete.
        // Any other failure fails the mass status, as a failed open-order read does.
        let (mut order_reports, orders_complete) = match self
            .http
            .request_order_status_reports_checked(account_id, None, start, None, false)
            .await
        {
            Ok((reports, coverage)) => (reports, coverage.is_complete()),
            Err(e) if is_refused_history_read(&e) => {
                log::warn!(
                    "Order history read refused for mass status, reading open orders only and \
                     marking the set incomplete: {e}"
                );
                let (reports, _) = self
                    .http
                    .request_order_status_reports_checked(account_id, None, start, None, true)
                    .await?;
                (reports, false)
            }
            Err(e) => return Err(e),
        };
        // Captured before the orders-status extension is merged in, so the safeguard below applies
        // to the venue read alone and leaves the extension to its own rule.
        let mut from_venue_read: HashSet<VenueOrderId> = order_reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect();

        let (extension, mut extension_withheld) = self
            .reports_for_open_orders_absent_from_venue(account_id, None, &order_reports)
            .await?;

        let (window_only, window_stated) =
            fold_window_into_venue_read(&mut order_reports, extension);

        // The window carries no fill price, so a filled report would be inferred at the order
        // price. A canceled one with executions passes, since reconciliation waits for its fills
        // rather than infer them. A withheld window report leaves the set incomplete. A venue-read
        // report that took a window state answers to these rules, not the venue read's.
        order_reports.retain(|report| {
            let withhold = window_stated.contains(&report.venue_order_id)
                && report.order_status == OrderStatus::Filled;

            if withhold {
                log::warn!(
                    "Withholding fully executed order {} from mass status: the orders-status \
                     window carries no fill price",
                    report.venue_order_id,
                );
                extension_withheld = true;
            }
            !withhold
        });

        for venue_order_id in &window_stated {
            from_venue_read.remove(venue_order_id);
        }

        for report in window_only {
            if report.order_status == OrderStatus::Filled {
                log::warn!(
                    "Withholding fully executed order {} from mass status: the orders-status \
                     window carries no fill price",
                    report.venue_order_id,
                );
                extension_withheld = true;
                continue;
            }

            order_reports.push(report);
        }

        let orders_complete = orders_complete && !extension_withheld;

        let (mut fill_reports, fills_complete) = self
            .http
            .request_fill_reports_checked(account_id, None, start, None)
            .await?;

        // A terminal history report with an executed quantity carries no price, so it is priced
        // from its fills as on the bulk path, which requires exact coverage. The adapter reads
        // only the latest fills page, so an execution older than that page is absent and does
        // not come back on a later read. Unpriced, reconciliation would infer the remainder at
        // the order's limit price, so such a report is withheld and the set marked incomplete.
        // A withheld report's page fills stay for a cached order, since the engine reconciles
        // priced fills against a cached order with no report. An uncached order's fills go with
        // its report. With no order, and no venue position ID on a Kraken fill, the engine drops
        // them, but on an instrument with a position report its partial-window adjustment first
        // counts every fill there when sizing a synthetic opening, so they would skew that
        // opening without ever being applied.
        let mut withheld = false;
        let mut withheld_fills: HashSet<VenueOrderId> = HashSet::new();
        {
            let cache = self.core.cache();
            order_reports.retain_mut(|report| {
                if !from_venue_read.contains(&report.venue_order_id)
                    || !is_unpriced_terminal_report(report)
                {
                    return true;
                }

                let cached = cached_order_for_report(&cache, report);

                match price_from_fills(report, cached.as_ref(), &fill_reports) {
                    Ok(()) => true,
                    Err(covered) => {
                        log::warn!(
                            "Withholding executed order {} from mass status: fills cover \
                             {covered} of {}; {}",
                            report.venue_order_id,
                            report.filled_qty,
                            if cached.is_some() {
                                "its page fills stay for the cached order"
                            } else {
                                "its page fills are withheld with it"
                            },
                        );
                        withheld = true;

                        if cached.is_none() {
                            withheld_fills.insert(report.venue_order_id);
                        }
                        false
                    }
                }
            });
        }
        fill_reports.retain(|fill| !withheld_fills.contains(&fill.venue_order_id));
        let orders_complete = orders_complete && !withheld;

        let position_reports = self
            .http
            .request_position_status_reports(account_id, None)
            .await?;

        // Terminal history orders must not open a position on an instrument the venue reports
        // flat. The position read returns open positions only, so an instrument with no position
        // report is flat at the venue. With no lookback declared the engine applies every kept
        // fill to positions, and the fills read is a single page, so a round trip whose opening
        // fill is older than that page would leave its closing side alone and open a position
        // opposite to the one the venue closed. On a flat instrument the fills of every order on
        // the read pages that the cache does not hold are netted, open orders included, since an
        // open order's fill can offset a terminal one. When they do not net to zero and a terminal
        // order is among them, the terminal orders are withheld with their fills and the set is
        // marked incomplete; open orders stay, and an open order's own fill can still open a
        // position. With a bounded lookback the engine projects
        // terminal orders onto order state only, so this applies to the unbounded read alone.
        // Cached orders are left out, since their fills reconcile against the cached order and
        // its position.
        let orders_complete = if lookback_start.is_none() {
            let held: HashSet<InstrumentId> = position_reports
                .iter()
                .map(|report| report.instrument_id)
                .collect();
            let netted: HashSet<VenueOrderId> = {
                let cache = self.core.cache();
                order_reports
                    .iter()
                    .filter(|report| {
                        from_venue_read.contains(&report.venue_order_id)
                            && !held.contains(&report.instrument_id)
                            && cached_order_for_report(&cache, report).is_none()
                    })
                    .map(|report| report.venue_order_id)
                    .collect()
            };
            let candidates: HashMap<VenueOrderId, InstrumentId> = order_reports
                .iter()
                .filter(|report| {
                    netted.contains(&report.venue_order_id)
                        && matches!(
                            report.order_status,
                            OrderStatus::Filled
                                | OrderStatus::Canceled
                                | OrderStatus::Expired
                                | OrderStatus::Voided
                        )
                })
                .map(|report| (report.venue_order_id, report.instrument_id))
                .collect();

            let mut net: HashMap<InstrumentId, Decimal> = HashMap::new();

            for fill in fill_reports
                .iter()
                .filter(|fill| netted.contains(&fill.venue_order_id))
            {
                let qty = fill.last_qty.as_decimal();
                let signed = match fill.order_side {
                    OrderSide::Buy => qty,
                    OrderSide::Sell => -qty,
                };
                *net.entry(fill.instrument_id).or_default() += signed;
            }
            net.retain(|instrument_id, net| {
                !net.is_zero() && candidates.values().any(|id| id == instrument_id)
            });

            if net.is_empty() {
                orders_complete
            } else {
                for (instrument_id, net) in &net {
                    log::warn!(
                        "Withholding terminal orders on {instrument_id} from mass status: the venue \
                         reports it flat but the fills of orders the cache does not hold net to \
                         {net}, so reconciliation would open a position; marking the set incomplete",
                    );
                }
                let withheld_flat: HashSet<VenueOrderId> = candidates
                    .iter()
                    .filter(|(_, instrument_id)| net.contains_key(instrument_id))
                    .map(|(venue_order_id, _)| *venue_order_id)
                    .collect();
                order_reports.retain(|report| !withheld_flat.contains(&report.venue_order_id));
                fill_reports.retain(|fill| !withheld_flat.contains(&fill.venue_order_id));
                false
            }
        } else {
            orders_complete
        };

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *KRAKEN_VENUE,
            ts_init,
            None,
        );
        {
            let cache = self.core.cache();
            name_reports_by_cached_order(&cache, &mut order_reports);
            name_fills_by_cached_order(&cache, &mut fill_reports);
        }
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);
        // As for spot: one cutoff for every historical query, recorded with its completeness.
        mass_status.set_report_window(lookback_start, orders_complete && fills_complete);

        Ok(Some(mass_status))
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        log::debug!("Querying account: {cmd}");

        let account_id = self.core.account_id;
        let http = self.http.clone();
        let emitter = self.emitter.clone();

        self.spawn_task("query_account", async move {
            let account_state = http.request_account_state(account_id).await?;
            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                account_state.ts_event,
                account_state.info,
            );
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: {cmd}");

        let venue_order_id = cmd
            .venue_order_id
            .context("venue_order_id required for query_order")?;
        // The cached order the venue order ID indexes names the report, as on every other path
        let cached_client_order_id = self.core.cache().client_order_id(&venue_order_id).copied();
        let account_id = self.core.account_id;
        let instrument_id = cmd.instrument_id;
        let http = self.http.clone();
        let emitter = self.emitter.clone();

        self.spawn_task("query_order", async move {
            // Scoped to the queried instrument, so an unresolvable open order on another contract
            // is out of scope for this lookup rather than failing it.
            let reports = http
                .request_order_status_reports(account_id, Some(instrument_id), None, None, true)
                .await
                .context("Failed to query order")?;

            match reports
                .into_iter()
                .find(|r| r.venue_order_id == venue_order_id)
            {
                Some(mut report) => {
                    if let Some(client_order_id) = cached_client_order_id {
                        report.client_order_id = Some(client_order_id);
                    }
                    emitter.send_order_status_report(report);
                }
                None => log::warn!(
                    "No open order {venue_order_id} found for {instrument_id}; no report emitted"
                ),
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;
        self.submit_single_order(&order, "submit_order");
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        log::debug!(
            "Submitting order list: order_list_id={}, count={}",
            cmd.order_list.id,
            orders.len()
        );

        // A list's open members go out together or not at all, so no leg is stranded
        if let Some(reason) = orders
            .iter()
            .filter(|order| !order.is_closed())
            .find_map(|order| validate_order(order).err())
        {
            for order in orders.iter().filter(|order| !order.is_closed()) {
                self.emitter.emit_order_denied(order, &reason.to_string());
            }
            return Ok(());
        }

        let mut order_tuples = Vec::with_capacity(orders.len());
        let mut order_meta = Vec::with_capacity(orders.len());

        for order in &orders {
            if order.is_closed() {
                log::warn!(
                    "Cannot submit closed order: client_order_id={}",
                    order.client_order_id()
                );
                continue;
            }

            // Kraken batch endpoint only supports limit and stop orders,
            // submit market orders individually
            if order.order_type() == OrderType::Market {
                self.submit_single_order(order, "submit_order_list");
                continue;
            }

            let client_order_id = order.client_order_id();
            self.register_order_identity(order);
            self.emitter.emit_order_submitted(order);

            order_tuples.push((
                order.instrument_id(),
                client_order_id,
                order.order_side(),
                order.order_type(),
                order.quantity(),
                order.time_in_force(),
                order.price(),
                order.trigger_price(),
                order.trigger_type(),
                order.is_reduce_only(),
                order.is_post_only(),
            ));

            order_meta.push((order.strategy_id(), order.instrument_id(), client_order_id));
        }

        if order_tuples.is_empty() {
            return Ok(());
        }

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let dispatch_state = self.ws_dispatch_state.clone();

        self.spawn_task("submit_order_list", async move {
            let results = http.send_order_batches(order_tuples).await;
            for (result, (strategy_id, instrument_id, client_order_id)) in
                results.into_iter().zip(&order_meta)
            {
                let outcome = match result {
                    Ok(item) => command_failure_from_futures_batch_item(item),
                    Err(e) => Err(command_failure_from_futures_batch_error(&e)),
                };

                match outcome {
                    Ok(()) => {}
                    Err(CommandFailure::Ambiguous(reason)) => {
                        log::warn!(
                            "submit_order_list outcome is ambiguous for client_order_id={client_order_id}: {reason}"
                        );
                    }
                    Err(
                        CommandFailure::NotSent(reason)
                        | CommandFailure::VenueRejected(reason),
                    ) => {
                        let ts_event = clock.get_time_ns();
                        let error_msg =
                            format!("submit_order_list batch item rejected: {reason}");
                        dispatch_state.cleanup_terminal(client_order_id);
                        emitter.emit_order_rejected_event(
                            *strategy_id,
                            *instrument_id,
                            *client_order_id,
                            &error_msg,
                            ts_event,
                            reason == "postWouldExecute",
                        );
                    }
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.modify_single_order(&cmd);
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_single_order(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if cmd.order_side.is_none() {
            log::debug!("Canceling all orders: instrument_id={instrument_id} (bulk)");

            let http = self.http.clone();
            let symbol = instrument_id.symbol.to_string();

            self.spawn_task("cancel_all_orders", async move {
                match http.inner.cancel_all_orders(Some(symbol)).await {
                    Ok(response) => {
                        if response.result != KrakenApiResult::Success
                            && response.cancel_status.cancelled_orders.is_empty()
                        {
                            log::warn!(
                                "Cancel-all failed without per-order results, awaiting reconciliation: status={}",
                                response.cancel_status.status
                            );
                        }
                    }
                    Err(e) => match command_failure_from_cancel_error(e) {
                        CommandFailure::NotSent(reason) => {
                            log::warn!("Cancel-all failed local validation: {reason}");
                        }
                        CommandFailure::Ambiguous(reason)
                        | CommandFailure::VenueRejected(reason) => {
                            log::warn!(
                                "Cancel-all ambiguous failure, awaiting reconciliation: {reason}"
                            );
                        }
                    },
                }
                Ok(())
            });

            return Ok(());
        }

        log::debug!(
            "Canceling all orders: instrument_id={instrument_id}, side={:?}",
            cmd.order_side
        );

        // Side-filtered cancellation reuses the explicit-id batch path rather than sending one
        // HTTP cancel per order.
        let cancels: Vec<CancelOrder> = {
            let cache = self.core.cache();
            let ts_init = self.clock.get_time_ns();
            let correlation_id = cmd.correlation_id.or(Some(cmd.command_id));

            // As for spot: the venue can have accepted an order the cache still records as
            // `Submitted`, so in-flight orders are selected alongside open ones.
            let mut seen = HashSet::new();

            cache
                .orders_open(None, Some(&instrument_id), None, None, None)
                .into_iter()
                .chain(cache.orders_inflight(None, Some(&instrument_id), None, None, None))
                .filter(|order| Some(order.order_side()) == cmd.order_side)
                .filter(|order| seen.insert(order.client_order_id()))
                .map(|order| {
                    CancelOrder::new(
                        cmd.trader_id,
                        cmd.client_id,
                        // Each cancel keeps the owning strategy of the order it targets.
                        order.strategy_id(),
                        order.instrument_id(),
                        order.client_order_id(),
                        order.venue_order_id(),
                        UUID4::new(),
                        ts_init,
                        cmd.params.clone(),
                        correlation_id,
                    )
                })
                .collect()
        };

        if cancels.is_empty() {
            log::debug!("No open orders to cancel for {instrument_id}");
            return Ok(());
        }

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_all_orders_by_side", async move {
            batch_cancel_orders_for_futures(&http, &emitter, clock, &cancels).await;
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        log::debug!(
            "Batch canceling orders: instrument_id={}, count={}",
            cmd.instrument_id,
            cmd.cancels.len()
        );

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let cancels = cmd.cancels;

        self.spawn_task("batch_cancel_orders", async move {
            batch_cancel_orders_for_futures(&http, &emitter, clock, &cancels).await;
            Ok(())
        });

        Ok(())
    }
}

#[derive(Debug, Clone)]
struct CancelRequestContext {
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    /// The client order ID an item without a venue order ID sends, which the venue echoes on its
    /// rejection.
    cli_ord_id: String,
    venue_order_id: Option<VenueOrderId>,
}

async fn cancel_order_for_futures(
    http: &KrakenFuturesHttpClient,
    _account_id: AccountId,
    instrument_id: InstrumentId,
    client_order_id: Option<ClientOrderId>,
    venue_order_id: Option<VenueOrderId>,
) -> Result<(), CommandFailure> {
    http.get_cached_instrument(&instrument_id.symbol.inner())
        .ok_or_else(|| {
            CommandFailure::not_sent(InstrumentLookupError::not_found(instrument_id).to_string())
        })?;

    let (order_id, cli_ord_id) =
        order_identifier(client_order_id.as_ref(), venue_order_id.as_ref());

    if order_id.is_none() && cli_ord_id.is_none() {
        return Err(CommandFailure::not_sent(
            "Either client_order_id or venue_order_id must be provided",
        ));
    }

    let response = http
        .inner
        .cancel_order(order_id, cli_ord_id)
        .await
        .map_err(command_failure_from_cancel_error)?;

    if response.result != KrakenApiResult::Success
        || response.cancel_status.status != KrakenSendStatus::Cancelled
    {
        return Err(CommandFailure::venue_rejected(format!(
            "cancel-order rejected: status={}",
            response.cancel_status.status
        )));
    }

    Ok(())
}

async fn batch_cancel_orders_for_futures(
    http: &KrakenFuturesHttpClient,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    cancels: &[CancelOrder],
) {
    let mut contexts = Vec::new();
    let mut items = Vec::new();

    for cancel in cancels {
        match batch_cancel_item_for_futures(http, cancel) {
            Ok((context, item)) => {
                contexts.push(context);
                items.push(item);
            }
            Err(CommandFailure::NotSent(reason)) => {
                log::warn!(
                    "Batch cancel command failed local validation for {}: {reason}",
                    cancel.client_order_id
                );
            }
            Err(CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason)) => {
                log::warn!(
                    "Batch cancel command ambiguous failure for {}, awaiting reconciliation: {reason}",
                    cancel.client_order_id
                );
            }
        }
    }

    for (item_chunk, context_chunk) in items
        .chunks(FUTURES_BATCH_CANCEL_LIMIT)
        .zip(contexts.chunks(FUTURES_BATCH_CANCEL_LIMIT))
    {
        let response = match http
            .inner
            .cancel_order_items_batch(item_chunk.to_vec())
            .await
        {
            Ok(response) => response,
            Err(e) => {
                match command_failure_from_cancel_error(e) {
                    CommandFailure::NotSent(reason) => {
                        log::warn!("Batch cancel failed local validation: {reason}");
                    }
                    CommandFailure::Ambiguous(reason) | CommandFailure::VenueRejected(reason) => {
                        log::warn!(
                            "Batch cancel failed without per-order results, awaiting reconciliation: {reason}"
                        );
                    }
                }
                continue;
            }
        };

        if response.batch_status.is_empty() {
            if response.result != KrakenApiResult::Success {
                let reason = response.error.as_deref().unwrap_or("Unknown error");
                log::warn!(
                    "Batch cancel failed without per-order results, awaiting reconciliation: {reason}"
                );
            }
            continue;
        }

        if response.batch_status.len() != context_chunk.len() {
            log::warn!(
                "Batch cancel returned {} per-order result(s) for {} request(s); unmatched results await reconciliation",
                response.batch_status.len(),
                context_chunk.len()
            );
        }

        for (index, status) in response.batch_status.iter().enumerate() {
            let Some(cancel_status) = batch_cancel_status(status) else {
                log::warn!("Batch cancel result without status at index {index}");
                continue;
            };

            if cancel_status == KrakenSendStatus::Cancelled {
                continue;
            }

            let Some(context) = batch_cancel_context(status, context_chunk, index) else {
                log::warn!(
                    "Batch cancel rejected item without matching request context at index {index}: status={cancel_status}"
                );
                continue;
            };

            emitter.emit_order_cancel_rejected_event(
                context.strategy_id,
                context.instrument_id,
                context.client_order_id,
                context.venue_order_id,
                &format!("batch-cancel rejected: status={cancel_status}"),
                clock.get_time_ns(),
            );
        }
    }
}

fn batch_cancel_item_for_futures(
    http: &KrakenFuturesHttpClient,
    cancel: &CancelOrder,
) -> Result<(CancelRequestContext, KrakenFuturesBatchCancelItem), CommandFailure> {
    http.get_cached_instrument(&cancel.instrument_id.symbol.inner())
        .ok_or_else(|| {
            CommandFailure::not_sent(
                InstrumentLookupError::not_found(cancel.instrument_id).to_string(),
            )
        })?;

    let cli_ord_id = cancel.client_order_id.to_string();
    let item = if let Some(venue_order_id) = cancel.venue_order_id {
        KrakenFuturesBatchCancelItem::from_order_id(venue_order_id.to_string())
    } else {
        KrakenFuturesBatchCancelItem::from_client_order_id(cli_ord_id.clone())
    };

    Ok((
        CancelRequestContext {
            strategy_id: cancel.strategy_id,
            instrument_id: cancel.instrument_id,
            client_order_id: cancel.client_order_id,
            cli_ord_id,
            venue_order_id: cancel.venue_order_id,
        },
        item,
    ))
}

fn batch_cancel_status(status: &FuturesBatchCancelStatus) -> Option<KrakenSendStatus> {
    status
        .cancel_status
        .as_ref()
        .map(|cancel_status| cancel_status.status)
        .or(status.status)
}

fn batch_cancel_context<'a>(
    status: &FuturesBatchCancelStatus,
    contexts: &'a [CancelRequestContext],
    index: usize,
) -> Option<&'a CancelRequestContext> {
    if let Some(order_id) = status.order_id.as_deref()
        && let Some(context) = contexts.iter().find(|context| {
            context
                .venue_order_id
                .is_some_and(|venue_order_id| venue_order_id.as_str() == order_id)
        })
    {
        return Some(context);
    }

    if let Some(cli_ord_id) = status.cli_ord_id.as_deref()
        && let Some(context) = contexts
            .iter()
            .find(|context| context.cli_ord_id == cli_ord_id)
    {
        return Some(context);
    }

    if index < contexts.len() && status.order_id.is_none() && status.cli_ord_id.is_none() {
        return contexts.get(index);
    }

    None
}

fn handle_cancel_failure(
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    failure: CommandFailure,
) {
    match failure {
        CommandFailure::VenueRejected(reason) => {
            emitter.emit_order_cancel_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                &reason,
                clock.get_time_ns(),
            );
        }
        CommandFailure::NotSent(reason) => {
            log::warn!("Cancel command failed local validation for {client_order_id}: {reason}");
        }
        CommandFailure::Ambiguous(reason) => {
            log::warn!(
                "Ambiguous cancel failure for {client_order_id}, awaiting reconciliation: {reason}"
            );
        }
    }
}

impl KrakenFuturesExecutionClient {
    /// Returns `/orders/status` reports for cached-open orders the given
    /// venue reports do not cover, and whether it withheld any report.
    ///
    /// A Maker Protection hold never reaches the book, so it is invisible to
    /// an open-orders snapshot while the venue still knows the order within
    /// its 5-second `/orders/status` window. Orders without a venue order ID
    /// yet (their submit acknowledgement is still inside the hold window) are
    /// queried by their client order ID, in both forms `venue_client_order_ids`
    /// gives, the only venue handle that exists during the window. Each report
    /// is named by the cached order it describes, as
    /// `attribute_orders_status_reports` resolves it.
    ///
    /// # Errors
    ///
    /// Returns an error if the underlying request fails or the venue rejects
    /// it, so callers defer rather than treat the orders as missing.
    async fn reports_for_open_orders_absent_from_venue(
        &self,
        account_id: AccountId,
        instrument_id: Option<InstrumentId>,
        reported: &[OrderStatusReport],
    ) -> anyhow::Result<(Vec<OrderStatusReport>, bool)> {
        let mut order_ids = Vec::new();
        let mut queried_by_cli_ord_id = Vec::new();

        {
            let cache = self.core.cache();
            for order in cache.orders_open(
                Some(&*KRAKEN_VENUE),
                instrument_id.as_ref(),
                None,
                None,
                None,
            ) {
                // Spot and Futures share the KRAKEN venue and one cache
                if product_type_from_symbol(order.instrument_id().symbol.inner().as_str())
                    != KrakenProductType::Futures
                {
                    continue;
                }

                match order.venue_order_id() {
                    Some(venue_order_id) => {
                        let already_reported = reported
                            .iter()
                            .any(|report| report.venue_order_id == venue_order_id);
                        if !already_reported {
                            order_ids.push(venue_order_id.to_string());
                        }
                    }
                    None => {
                        queried_by_cli_ord_id.push(HeldOrder {
                            client_order_id: order.client_order_id(),
                            instrument_id: order.instrument_id(),
                            side: order.order_side(),
                            ts_init: order.ts_init(),
                        });
                    }
                }
            }
        }

        if order_ids.is_empty() && queried_by_cli_ord_id.is_empty() {
            return Ok((Vec::new(), false));
        }

        let cli_ord_ids: Vec<String> = queried_by_cli_ord_id
            .iter()
            .flat_map(|order| venue_client_order_ids(&order.client_order_id))
            .collect();

        log::debug!(
            "Resolving {} venue order ID(s) and {} client order ID(s) from the orders-status window",
            order_ids.len(),
            cli_ord_ids.len(),
        );

        let mut reports = Vec::new();
        for chunk in order_ids.chunks(FUTURES_ORDERS_STATUS_LIMIT) {
            reports.extend(
                self.http
                    .request_orders_status_reports(account_id, chunk, &[])
                    .await?,
            );
        }

        for chunk in cli_ord_ids.chunks(FUTURES_ORDERS_STATUS_LIMIT) {
            reports.extend(
                self.http
                    .request_orders_status_reports(account_id, &[], chunk)
                    .await?,
            );
        }

        let (mut attributed, withheld) = attribute_orders_status_reports(
            &self.core.cache(),
            &queried_by_cli_ord_id,
            latest_report_per_order(reports),
        );

        // A client order ID request returns every order holding the ID, so a run scoped to one
        // instrument keeps only that instrument's reports.
        if let Some(instrument_id) = instrument_id {
            attributed.retain(|report| report.instrument_id == instrument_id);
        }

        Ok((attributed, withheld))
    }
}

/// Checks an order against the venue's documented limits before it is submitted.
fn validate_order(order: &OrderAny) -> Result<(), OrderDeniedReason> {
    let len = order.client_order_id().as_str().len();

    if len > KRAKEN_FUTURES_MAX_CLIENT_ORDER_ID_LEN {
        return Err(OrderDeniedReason::InvalidClientOrderId {
            detail: format!(
                "Kraken Futures accepts a client order ID of at most \
                 {KRAKEN_FUTURES_MAX_CLIENT_ORDER_ID_LEN} characters, was {len}"
            ),
        });
    }

    Ok(())
}

/// The cached order a report describes, found by its venue order ID.
fn cached_order_for_report(cache: &Cache, report: &OrderStatusReport) -> Option<OrderAny> {
    let client_order_id = cache.client_order_id(&report.venue_order_id)?;
    cache.order(client_order_id).map(|order| order.clone())
}

/// The order a single-order query asks about, and how a venue row is tied to it.
///
/// With its venue order ID known, the row carrying that ID is the order. Otherwise a row is the
/// order's when it echoes the order's client order ID in either form `venue_client_order_ids`
/// gives, on the order's instrument and side, placed no more than `ORDERS_STATUS_CLOCK_SKEW`
/// before the order's creation: an older row is another order whose ID shortens alike or that
/// reused the ID. A fill carries no placement time, so its fill time stands in, which rules out
/// only a fill executed before the order's creation. Without a cached order to check against, the
/// echo alone ties a row to it. A row of a venue order the cache indexes for another order is that
/// order's whatever it echoes, since another order's full ID can be this order's shortened one.
/// A row echoing the full ID of another cached order sent but carrying no venue order ID, on that
/// order's instrument and side and placed no more than `ORDERS_STATUS_CLOCK_SKEW` before its
/// creation, can be either order's while the cache indexes its venue order for neither, and leaves
/// the answer open.
struct QueriedOrder {
    venue_order_id: Option<VenueOrderId>,
    client_order_id: Option<ClientOrderId>,
    cached: Option<(InstrumentId, OrderSide, UnixNanos)>,
}

impl QueriedOrder {
    fn new(cmd: &GenerateOrderStatusReport, cached: Option<&OrderAny>) -> Self {
        Self {
            venue_order_id: cmd
                .venue_order_id
                .or_else(|| cached.and_then(Order::venue_order_id)),
            client_order_id: cmd
                .client_order_id
                .or_else(|| cached.map(Order::client_order_id)),
            cached: cached
                .map(|order| (order.instrument_id(), order.order_side(), order.ts_init())),
        }
    }

    /// Whether a row echoing `echoed` on `instrument_id` and `side`, placed at `placed`, can be
    /// this order by its client order ID.
    fn echoes(
        &self,
        echoed: Option<ClientOrderId>,
        instrument_id: InstrumentId,
        side: OrderSide,
        placed: UnixNanos,
    ) -> bool {
        let echoes_id = echoed
            .zip(self.client_order_id)
            .is_some_and(|(echoed, queried)| {
                venue_client_order_ids(&queried).any(|id| id == echoed.as_str())
            });

        echoes_id
            && self
                .cached
                .is_none_or(|(order_instrument, order_side, created)| {
                    instrument_id == order_instrument
                        && side == order_side
                        && placed.saturating_add(ORDERS_STATUS_CLOCK_SKEW) >= created
                })
    }

    /// Whether the cache indexes `venue_order_id` for an order other than this one.
    fn indexed_for_another(&self, cache: &Cache, venue_order_id: &VenueOrderId) -> bool {
        cache
            .client_order_id(venue_order_id)
            .is_some_and(|owner| Some(*owner) != self.client_order_id)
    }

    fn echoes_report(&self, cache: &Cache, report: &OrderStatusReport) -> bool {
        !self.indexed_for_another(cache, &report.venue_order_id)
            && report.order_side.is_some_and(|side| {
                self.echoes(
                    report.client_order_id,
                    report.instrument_id,
                    side,
                    report.ts_accepted,
                )
            })
    }

    fn echoes_fill(&self, cache: &Cache, fill: &FillReport) -> bool {
        !self.indexed_for_another(cache, &fill.venue_order_id)
            && self.echoes(
                fill.client_order_id,
                fill.instrument_id,
                fill.order_side,
                fill.ts_event,
            )
    }

    /// Whether another cached order, sent but without a venue order ID, holds `echoed` as its full
    /// client order ID and a row on `instrument_id` and `side`, placed at `placed`, can be that
    /// order's.
    fn echo_is_another_order(
        &self,
        cache: &Cache,
        echoed: Option<ClientOrderId>,
        instrument_id: InstrumentId,
        side: OrderSide,
        placed: UnixNanos,
    ) -> bool {
        echoed
            .filter(|echoed| Some(*echoed) != self.client_order_id)
            .and_then(|echoed| cache.order(&echoed))
            .is_some_and(|other| {
                other.ts_submitted().is_some()
                    && other.venue_order_id().is_none()
                    && other.instrument_id() == instrument_id
                    && other.order_side() == side
                    && placed.saturating_add(ORDERS_STATUS_CLOCK_SKEW) >= other.ts_init()
            })
    }

    fn report_may_be_another_order(&self, cache: &Cache, report: &OrderStatusReport) -> bool {
        report.order_side.is_some_and(|side| {
            self.echo_is_another_order(
                cache,
                report.client_order_id,
                report.instrument_id,
                side,
                report.ts_accepted,
            )
        })
    }

    fn fill_may_be_another_order(&self, cache: &Cache, fill: &FillReport) -> bool {
        self.echo_is_another_order(
            cache,
            fill.client_order_id,
            fill.instrument_id,
            fill.order_side,
            fill.ts_event,
        )
    }

    /// The report of this order, named by its client order ID when matched by it.
    ///
    /// # Errors
    ///
    /// Returns an error if reports of several venue orders can be this order, or a report that can
    /// be this order can also be another cached order's by its full client order ID, so the query
    /// defers rather than take one of them.
    fn find_report(
        &self,
        cache: &Cache,
        reports: &[OrderStatusReport],
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        if let Some(venue_order_id) = self.venue_order_id {
            return Ok(reports
                .iter()
                .find(|report| report.venue_order_id == venue_order_id)
                .cloned());
        }

        let mut candidates = reports
            .iter()
            .filter(|report| self.echoes_report(cache, report));
        let Some(first) = candidates.next() else {
            return Ok(None);
        };

        if candidates.any(|report| report.venue_order_id != first.venue_order_id) {
            anyhow::bail!(
                "Reports of several venue orders carry the client order ID of order {:?}; \
                 deferring rather than taking one of them",
                self.client_order_id,
            );
        }

        if let Some(shared) = reports.iter().find(|report| {
            self.echoes_report(cache, report) && self.report_may_be_another_order(cache, report)
        }) {
            anyhow::bail!(
                "Report of venue order {} carries {:?}, the full ID of another cached order and a \
                 form of order {:?}; deferring until the cache ties the venue order to one of them",
                shared.venue_order_id,
                shared.client_order_id,
                self.client_order_id,
            );
        }

        let mut report = first.clone();
        report.client_order_id = self.client_order_id;
        Ok(Some(report))
    }

    /// Whether a report of another venue order can be this order, which leaves absence open:
    /// the venue may hold the order under that report.
    fn has_stray_report(&self, cache: &Cache, reports: &[OrderStatusReport]) -> bool {
        self.venue_order_id.is_some_and(|venue_order_id| {
            reports.iter().any(|report| {
                report.venue_order_id != venue_order_id && self.echoes_report(cache, report)
            })
        })
    }

    /// Whether a fill other than one of this order's known venue order can be this order's, which
    /// leaves absence open. With the venue order ID unknown, any fill echoing the order counts: it
    /// may be this order's, short of its quantity or beside another venue order's.
    fn has_stray_fill(&self, cache: &Cache, fills: &[FillReport]) -> bool {
        fills.iter().any(|fill| {
            Some(fill.venue_order_id) != self.venue_order_id && self.echoes_fill(cache, fill)
        })
    }
}

/// The client order IDs the venue can hold `client_order_id` under: the full ID the adapter sends
/// and, when it differs, the form `truncate_cl_ord_id` gives, which an adapter that shortened
/// futures IDs past 18 characters sent and which another order's ID can share.
fn venue_client_order_ids(client_order_id: &ClientOrderId) -> impl Iterator<Item = String> {
    let full = client_order_id.to_string();
    let shortened = truncate_cl_ord_id(client_order_id);
    let shortened = (shortened != full).then_some(shortened);
    std::iter::once(full).chain(shortened)
}

/// An open order the orders-status window is asked about by its client order ID.
#[derive(Clone, Copy, Debug)]
struct HeldOrder {
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    side: OrderSide,
    ts_init: UnixNanos,
}

/// Names each report by the cached order indexing its venue order ID. The venue echoes the client
/// order ID an order was sent with, which for an order sent with a shortened ID is not the cached
/// one, and reconciliation matches a report carrying a client order ID by that ID alone.
fn name_reports_by_cached_order(cache: &Cache, reports: &mut [OrderStatusReport]) {
    for report in reports {
        if let Some(client_order_id) = cache.client_order_id(&report.venue_order_id) {
            report.client_order_id = Some(*client_order_id);
        }
    }
}

/// Names each fill by the cached order indexing its venue order ID, as
/// [`name_reports_by_cached_order`] names order reports.
fn name_fills_by_cached_order(cache: &Cache, fills: &mut [FillReport]) {
    for fill in fills {
        if let Some(client_order_id) = cache.client_order_id(&fill.venue_order_id) {
            fill.client_order_id = Some(*client_order_id);
        }
    }
}

/// Folds window reports into the venue read by venue order ID, returning the reports of the orders
/// the venue read did not return and the venue order IDs of the reports that took a window state.
///
/// A window copy of an order the venue read returned names that report by the cached order the
/// window attributed it to, then is dropped, so the order reaches reconciliation once with the
/// venue read's fields, such as time in force, post-only and trigger fields, which the window
/// lacks. When the report is open and the copy describes a later state, by `supersedes`, the
/// report takes the copy's status, quantities, price, average price, cancel reason and last
/// update, and answers to the window's rules; a closed report keeps its own state and the pricing
/// path it has.
fn fold_window_into_venue_read(
    venue_read: &mut [OrderStatusReport],
    window: Vec<OrderStatusReport>,
) -> (Vec<OrderStatusReport>, HashSet<VenueOrderId>) {
    let positions: HashMap<VenueOrderId, usize> = venue_read
        .iter()
        .enumerate()
        .map(|(position, report)| (report.venue_order_id, position))
        .collect();
    let mut window_stated = HashSet::new();

    let window_only = window
        .into_iter()
        .filter_map(|copy| {
            let Some(&position) = positions.get(&copy.venue_order_id) else {
                return Some(copy);
            };
            let report = &mut venue_read[position];
            report.client_order_id = copy.client_order_id;

            if !report.order_status.is_closed() && supersedes(&copy, report) {
                report.order_status = copy.order_status;
                report.quantity = copy.quantity;
                report.filled_qty = copy.filled_qty;
                report.price = copy.price;
                report.avg_px = copy.avg_px;
                report.cancel_reason = copy.cancel_reason;
                report.ts_last = copy.ts_last;
                window_stated.insert(copy.venue_order_id);
            }
            None
        })
        .collect();

    (window_only, window_stated)
}

/// Keeps one report per venue order ID, the one describing the order's latest state.
///
/// The venue-ID and client-order-ID requests run one after the other, so both can return one order
/// in different states. `supersedes` decides between them rather than the order the requests ran
/// in: a closed state over an open one, else the later venue update, else the larger filled
/// quantity.
fn latest_report_per_order(reports: Vec<OrderStatusReport>) -> Vec<OrderStatusReport> {
    let mut latest: IndexMap<VenueOrderId, OrderStatusReport> =
        IndexMap::with_capacity(reports.len());

    for report in reports {
        match latest.get(&report.venue_order_id) {
            Some(existing) if !supersedes(&report, existing) => {}
            _ => {
                latest.insert(report.venue_order_id, report);
            }
        }
    }

    latest.into_values().collect()
}

/// Names each `/orders/status` report by the client order ID of the cached order it describes,
/// returning the reports it attributes and whether it withheld any. `reports` holds one report per
/// venue order ID.
///
/// The venue echoes the client order ID an order was sent with, in either form
/// `venue_client_order_ids` gives, and reconciliation matches a report carrying a client order ID
/// by that ID alone. A report takes the ID of the cached order indexing its venue order ID.
/// Otherwise its echoed ID names an order queried by client order ID only when exactly one such
/// order holds it, exactly one report without an indexed venue order ID carries it, the report's
/// instrument and side are the order's, and the venue placed it no more than
/// `ORDERS_STATUS_CLOCK_SKEW` before the order's creation, which keeps out an older order whose ID
/// shortens alike or that reused the ID. Two reports that would name one order, one echoing each
/// form, name neither. Any other report is withheld with a warning naming the reason, since it
/// cannot be tied to exactly one cached order: reconciliation would take it for an external order
/// or for the wrong one.
fn attribute_orders_status_reports(
    cache: &Cache,
    queried_by_cli_ord_id: &[HeldOrder],
    reports: Vec<OrderStatusReport>,
) -> (Vec<OrderStatusReport>, bool) {
    // None marks an echoed ID that several queried orders share
    let mut held: HashMap<String, Option<HeldOrder>> = HashMap::new();

    for order in queried_by_cli_ord_id {
        for echoed in venue_client_order_ids(&order.client_order_id) {
            held.entry(echoed)
                .and_modify(|named| *named = None)
                .or_insert(Some(*order));
        }
    }

    let indexed =
        |report: &OrderStatusReport| cache.client_order_id(&report.venue_order_id).copied();

    let mut unindexed: HashMap<ClientOrderId, HashSet<VenueOrderId>> = HashMap::new();

    for report in &reports {
        if indexed(report).is_none()
            && let Some(echoed) = report.client_order_id
        {
            unindexed
                .entry(echoed)
                .or_default()
                .insert(report.venue_order_id);
        }
    }

    let by_echoed_id = |report: &OrderStatusReport| -> Result<ClientOrderId, &'static str> {
        let echoed = report
            .client_order_id
            .ok_or("it carries no client order ID")?;
        if unindexed
            .get(&echoed)
            .is_none_or(|venue_order_ids| venue_order_ids.len() != 1)
        {
            return Err("several reports carry its client order ID");
        }
        let order = held
            .get(echoed.as_str())
            .ok_or("no order queried by client order ID holds its client order ID")?
            .ok_or("several queried orders hold its client order ID")?;

        if report.instrument_id != order.instrument_id || report.order_side != Some(order.side) {
            return Err("its instrument or side is not the queried order's");
        }

        if report.ts_accepted.saturating_add(ORDERS_STATUS_CLOCK_SKEW) < order.ts_init {
            return Err(
                "the venue placed it before the queried order's creation, beyond the clock skew \
                 allowance; check the node clock if the order is recent",
            );
        }

        Ok(order.client_order_id)
    };

    let named: Vec<(OrderStatusReport, Result<ClientOrderId, &'static str>, bool)> = reports
        .into_iter()
        .map(|report| match indexed(&report) {
            Some(client_order_id) => (report, Ok(client_order_id), false),
            None => {
                let named = by_echoed_id(&report);
                (report, named, true)
            }
        })
        .collect();

    // A queried order echoed in full on one report and shortened on another is named by both
    let mut named_by_echo: HashMap<ClientOrderId, usize> = HashMap::new();

    for (_, named, by_echo) in &named {
        if *by_echo && let Ok(client_order_id) = named {
            *named_by_echo.entry(*client_order_id).or_default() += 1;
        }
    }

    let mut attributed = Vec::with_capacity(named.len());
    let mut withheld = false;

    for (mut report, named, by_echo) in named {
        let named = match named {
            Ok(client_order_id)
                if by_echo && named_by_echo.get(&client_order_id).is_some_and(|n| *n > 1) =>
            {
                Err("several reports name the queried order it would be")
            }
            named => named,
        };

        match named {
            Ok(client_order_id) => {
                report.client_order_id = Some(client_order_id);
                attributed.push(report);
            }
            Err(reason) => {
                log::warn!(
                    "Withholding orders-status report {} with client order ID {:?}: {reason}",
                    report.venue_order_id,
                    report.client_order_id,
                );
                withheld = true;
            }
        }
    }

    (attributed, withheld)
}

/// Whether a combined order read failed because the venue refused its history page.
///
/// The rule goes by the response's shape: an error body under a success status, whatever its
/// code, or HTTP 429 is a refusal. Any other HTTP status, a transport failure and a body that
/// cannot be parsed are faults, as is any failure of the open-order read, which carries no typed
/// error. A credential failure fails that open-order read, which runs first with the same key.
fn is_refused_history_read(error: &anyhow::Error) -> bool {
    error
        .chain()
        .filter_map(|cause| cause.downcast_ref::<KrakenHttpError>())
        .any(|e| match e {
            KrakenHttpError::ApiError(_) => true,
            KrakenHttpError::NetworkError(message) => message.starts_with("HTTP error 429"),
            _ => false,
        })
}

/// Whether `report` is terminal with an executed quantity it carries no price for.
///
/// An order history row has no average price. Unpriced, reconciliation infers the executed
/// quantity at the report's limit price, and cannot price a market order at all.
fn is_unpriced_terminal_report(report: &OrderStatusReport) -> bool {
    matches!(
        report.order_status,
        OrderStatus::Filled | OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Voided
    ) && !report.filled_qty.is_zero()
        && report.avg_px.is_none()
}

/// Sets `report.avg_px` to the quantity-weighted price of the order's executions.
///
/// The venue's own fills price the report when they cover its filled quantity exactly. Otherwise
/// the fills a cached order has recorded complete the page, each counted once by trade ID, and
/// must cover it exactly too. Exact coverage rejects a double count: an execution the engine
/// inferred carries a synthetic trade ID, so it would otherwise be counted beside the venue fill
/// it stands for. The adapter reads only the latest fills page, so an older execution can be absent
/// from it.
///
/// # Errors
///
/// Returns an error if neither source covers the report's filled quantity exactly. The error
/// contains the covered quantity, and the report remains unpriced.
fn price_from_fills(
    report: &mut OrderStatusReport,
    cached: Option<&OrderAny>,
    fills: &[FillReport],
) -> Result<(), Decimal> {
    let target = report.filled_qty.as_decimal();
    let order_fills: Vec<&FillReport> = fills
        .iter()
        .filter(|fill| fill.venue_order_id == report.venue_order_id)
        .collect();
    let total = |fills: &mut dyn Iterator<Item = &&FillReport>| {
        fills.fold((Decimal::ZERO, Decimal::ZERO), |(qty, notional), fill| {
            let fill_qty = fill.last_qty.as_decimal();
            (
                qty + fill_qty,
                notional + fill_qty * fill.last_px.as_decimal(),
            )
        })
    };

    let (page_qty, page_notional) = total(&mut order_fills.iter());

    if !target.is_zero() && page_qty == target {
        report.avg_px = Some(page_notional / target);
        return Ok(());
    }

    let Some(order) = cached else {
        return Err(page_qty);
    };
    let recorded: HashSet<TradeId> = order.trade_ids().into_iter().copied().collect();
    let (new_qty, new_notional) = total(
        &mut order_fills
            .iter()
            .filter(|fill| !recorded.contains(&fill.trade_id)),
    );
    let cached_qty = order.filled_qty().as_decimal();
    let covered = cached_qty + new_qty;

    if !target.is_zero() && covered == target {
        let cached_notional = order.avg_px().unwrap_or(Decimal::ZERO) * cached_qty;
        report.avg_px = Some((cached_notional + new_notional) / target);
        return Ok(());
    }

    Err(covered.max(page_qty))
}

fn synthesize_filled_order_status_report(
    cache: &Cache,
    queried: &QueriedOrder,
    order: &OrderAny,
    fills: &[FillReport],
) -> Option<OrderStatusReport> {
    let venue_order_id = queried.venue_order_id;

    // A fill of another venue order can carry the same client order ID, so with the venue order
    // ID known only its fills count, and without it the fills must belong to one venue order that
    // no other cached order can hold by its full client order ID.
    let mut matched: Vec<&FillReport> = match venue_order_id {
        Some(venue_order_id) => fills
            .iter()
            .filter(|fill| fill.venue_order_id == venue_order_id)
            .collect(),
        None => {
            let matched: Vec<&FillReport> = fills
                .iter()
                .filter(|fill| queried.echoes_fill(cache, fill))
                .collect();
            let first_venue_order_id = matched.first()?.venue_order_id;

            if matched.iter().any(|fill| {
                fill.venue_order_id != first_venue_order_id
                    || queried.fill_may_be_another_order(cache, fill)
            }) {
                return None;
            }
            matched
        }
    };

    if matched.is_empty() {
        return None;
    }

    matched.sort_by_key(|fill| fill.ts_event);
    let first_fill = *matched.first()?;
    let last_fill = *matched.last()?;

    let total_filled = matched
        .iter()
        .fold(Decimal::ZERO, |acc, fill| acc + fill.last_qty.as_decimal());
    if total_filled < order.quantity().as_decimal() {
        return None;
    }

    let total_notional = matched.iter().fold(Decimal::ZERO, |acc, fill| {
        acc + fill.last_qty.as_decimal() * fill.last_px.as_decimal()
    });
    let avg_px = if total_filled.is_zero() {
        None
    } else {
        Some(total_notional / total_filled)
    };
    let venue_order_id = venue_order_id.unwrap_or(first_fill.venue_order_id);

    let mut report = OrderStatusReport::new(
        first_fill.account_id,
        order.instrument_id(),
        Some(order.client_order_id()),
        venue_order_id,
        order.order_side().into(),
        order.order_type(),
        order.time_in_force(),
        OrderStatus::Filled,
        order.quantity(),
        order.quantity(),
        first_fill.ts_event,
        last_fill.ts_event,
        last_fill.ts_init,
        None,
    );
    report.order_list_id = order.order_list_id();
    report.venue_position_id = matched.iter().rev().find_map(|fill| fill.venue_position_id);
    report.linked_order_ids = order
        .linked_order_ids()
        .map(|linked_order_ids| linked_order_ids.to_vec());
    report.parent_order_id = order.parent_order_id();
    report.expire_time = order.expire_time();
    report.price = order.price();
    report.trigger_price = order.trigger_price();
    report.trigger_type = order.trigger_type();
    report.avg_px = avg_px;
    report.display_qty = order.display_qty();
    report.post_only = order.is_post_only();
    report.reduce_only = order.is_reduce_only();
    Some(report)
}

#[cfg(test)]
mod tests {
    use nautilus_core::{UUID4, UnixNanos};
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderType, TimeInForce},
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
        },
        orders::OrderTestBuilder,
        reports::FillReport,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    const TEST_INSTRUMENT_ID: &str = "PF_XBTUSD.KRAKEN";

    #[tokio::test]
    async fn test_cancel_order_for_futures_missing_cached_instrument_returns_canonical_error() {
        let http = KrakenFuturesHttpClient::default();
        let instrument_id = InstrumentId::from(TEST_INSTRUMENT_ID);
        let client_order_id = ClientOrderId::from("C-001");
        let venue_order_id = VenueOrderId::from("V-001");

        let result = cancel_order_for_futures(
            &http,
            AccountId::from("KRAKEN-001"),
            instrument_id,
            Some(client_order_id),
            Some(venue_order_id),
        )
        .await;

        match result {
            Err(CommandFailure::NotSent(reason)) => {
                assert_eq!(
                    reason,
                    InstrumentLookupError::not_found(instrument_id).to_string()
                );
            }
            _ => panic!("Expected local validation failure"),
        }
    }

    #[rstest]
    fn test_batch_cancel_item_for_futures_missing_cached_instrument_returns_canonical_error() {
        let http = KrakenFuturesHttpClient::default();
        let instrument_id = InstrumentId::from(TEST_INSTRUMENT_ID);
        let cancel = CancelOrder::new(
            TraderId::from("TESTER-001"),
            None,
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("C-001"),
            Some(VenueOrderId::from("V-001")),
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        );

        let result = batch_cancel_item_for_futures(&http, &cancel);

        match result {
            Err(CommandFailure::NotSent(reason)) => {
                assert_eq!(
                    reason,
                    InstrumentLookupError::not_found(instrument_id).to_string()
                );
            }
            _ => panic!("Expected local validation failure"),
        }
    }

    fn make_fill(
        venue_order_id: &str,
        client_order_id: Option<&str>,
        quantity: &str,
        price: &str,
        ts_event: u64,
    ) -> FillReport {
        FillReport::new(
            AccountId::from("KRAKEN-001"),
            InstrumentId::from(TEST_INSTRUMENT_ID),
            VenueOrderId::from(venue_order_id),
            TradeId::from(format!("T-{ts_event}").as_str()),
            OrderSide::Buy,
            Quantity::from(quantity),
            Price::from(price),
            Money::from_decimal(Decimal::ZERO, Currency::USD()).unwrap(),
            LiquiditySide::Taker,
            client_order_id.map(ClientOrderId::from),
            None,
            UnixNanos::from(ts_event),
            UnixNanos::from(ts_event),
            None,
        )
    }

    fn make_cmd(
        client_order_id: Option<&str>,
        venue_order_id: Option<&str>,
    ) -> GenerateOrderStatusReport {
        GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(InstrumentId::from(TEST_INSTRUMENT_ID)),
            client_order_id.map(ClientOrderId::from),
            venue_order_id.map(VenueOrderId::from),
            None,
            None,
        )
    }

    fn make_order(client_order_id: &str) -> OrderAny {
        OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from(TEST_INSTRUMENT_ID))
            .client_order_id(ClientOrderId::from(client_order_id))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .time_in_force(TimeInForce::Ioc)
            .build()
    }

    #[rstest]
    fn test_synthesize_filled_order_status_report_matches_full_fill_by_venue_order_id() {
        let order = make_order("O-123456");
        let cmd = make_cmd(Some("O-123456"), Some("KRAKEN-789"));
        let fills = vec![
            make_fill("KRAKEN-789", Some("O-123456"), "40", "50000.0", 1),
            make_fill("KRAKEN-789", Some("O-123456"), "60", "50010.0", 2),
            make_fill("KRAKEN-OTHER", Some("O-123456"), "999", "1.0", 3),
        ];

        let report = synthesize_filled_order_status_report(
            &Cache::default(),
            &QueriedOrder::new(&cmd, Some(&order)),
            &order,
            &fills,
        )
        .expect("expected a filled report");

        assert_eq!(report.venue_order_id, VenueOrderId::from("KRAKEN-789"));
        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from("O-123456"))
        );
        assert_eq!(report.order_status, OrderStatus::Filled);
        assert_eq!(report.order_type, OrderType::Market);
        assert_eq!(report.time_in_force, TimeInForce::Ioc);
        assert_eq!(report.quantity, Quantity::from("100"));
        assert_eq!(report.filled_qty, Quantity::from("100"));
        assert_eq!(
            report.avg_px,
            Some(Decimal::from_str_exact("50006.0").unwrap())
        );
    }

    #[rstest]
    fn test_synthesize_filled_order_status_report_requires_full_fill_size() {
        let order = make_order("O-123457");
        let cmd = make_cmd(Some("O-123457"), Some("KRAKEN-790"));
        let fills = vec![make_fill(
            "KRAKEN-790",
            Some("O-123457"),
            "40",
            "50000.0",
            1,
        )];

        assert!(
            synthesize_filled_order_status_report(
                &Cache::default(),
                &QueriedOrder::new(&cmd, Some(&order)),
                &order,
                &fills
            )
            .is_none()
        );
    }

    /// Without a venue order ID the fills must carry the order's client order ID, in full or in the
    /// shortened form the venue holds an order sent with one under, be executed no earlier than the
    /// clock skew allowance before the order's creation, and belong to one venue order. Fills of
    /// two venue orders sharing the ID, or executed earlier than that, prove nothing about this
    /// one.
    #[rstest]
    #[case::full_id_on_one_venue_order(&[("KRAKEN-791", false)], 0, Some("KRAKEN-791"))]
    #[case::shortened_id_on_one_venue_order(&[("KRAKEN-791", true)], 0, Some("KRAKEN-791"))]
    #[case::full_id_on_two_venue_orders(&[("KRAKEN-791", false), ("KRAKEN-792", false)], 0, None)]
    #[case::executed_before_the_order(&[("KRAKEN-791", true)], 10_000_000_000, None)]
    fn test_synthesize_filled_order_status_report_without_a_venue_order_id(
        #[case] fills: &[(&str, bool)],
        #[case] created_ns: u64,
        #[case] expected_venue_order_id: Option<&str>,
    ) {
        let long_client_order_id = "O-20260227-002321-004-001-1";
        let shortened = truncate_cl_ord_id(&ClientOrderId::from(long_client_order_id));
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from(TEST_INSTRUMENT_ID))
            .client_order_id(ClientOrderId::from(long_client_order_id))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .time_in_force(TimeInForce::Ioc)
            .ts_init(UnixNanos::from(created_ns))
            .build();
        let cmd = make_cmd(Some(long_client_order_id), None);
        let quantity = format!("{}", 100 / fills.len());
        let fills: Vec<FillReport> = fills
            .iter()
            .enumerate()
            .map(|(i, (venue_order_id, shortened_echo))| {
                let echoed = if *shortened_echo {
                    shortened.as_str()
                } else {
                    long_client_order_id
                };
                make_fill(
                    venue_order_id,
                    Some(echoed),
                    &quantity,
                    "50000.0",
                    i as u64 + 1,
                )
            })
            .collect();

        let report = synthesize_filled_order_status_report(
            &Cache::default(),
            &QueriedOrder::new(&cmd, Some(&order)),
            &order,
            &fills,
        );

        assert_eq!(
            report.as_ref().map(|report| report.venue_order_id),
            expected_venue_order_id.map(VenueOrderId::from)
        );

        if let Some(report) = report {
            assert_eq!(
                report.client_order_id,
                Some(ClientOrderId::from(long_client_order_id))
            );
            assert_eq!(report.order_status, OrderStatus::Filled);
        }
    }
}
