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
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
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
    AtomicMap, MUTEX_POISONED, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderStatus, OrderType},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{consts::KRAKEN_VENUE, credential::KrakenCredential, parse::truncate_cl_ord_id},
    config::KrakenExecClientConfig,
    http::{
        KrakenFuturesHttpClient, futures::client::KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND,
    },
    websocket::{
        dispatch::{self, OrderIdentity, WsDispatchState},
        futures::{client::KrakenFuturesWebSocketClient, messages::KrakenFuturesWsMessage},
    },
};

/// Kraken Futures execution client.
///
/// Provides order management, account operations, and position management
/// for Kraken Futures markets.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenFuturesExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: KrakenExecClientConfig,
    emitter: ExecutionEventEmitter,
    http: KrakenFuturesHttpClient,
    ws: KrakenFuturesWebSocketClient,
    cancellation_token: CancellationToken,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: Arc<AtomicMap<String, Quantity>>,
    ws_dispatch_state: Arc<WsDispatchState>,
}

impl KrakenFuturesExecutionClient {
    /// Creates a new [`KrakenFuturesExecutionClient`].
    pub fn new(core: ExecutionClientCore, config: KrakenExecClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Margin,
            None,
        );

        let cancellation_token = CancellationToken::new();

        let http = KrakenFuturesHttpClient::with_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.environment,
            config.base_url.clone(),
            config.timeout_secs,
            None,
            None,
            None,
            config.proxy_url.clone(),
            config
                .max_requests_per_second
                .unwrap_or(KRAKEN_FUTURES_DEFAULT_RATE_LIMIT_PER_SECOND),
        )?;

        let credential = KrakenCredential::new(config.api_key.clone(), config.api_secret.clone());
        let ws = KrakenFuturesWebSocketClient::with_credentials(
            config.ws_url(),
            config.heartbeat_interval_secs,
            Some(credential),
            config.transport_backend,
            config.proxy_url.clone(),
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http,
            ws,
            cancellation_token,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
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

    fn submit_single_order(&self, order: &OrderAny, task_name: &'static str) {
        if order.is_closed() {
            log::warn!(
                "Cannot submit closed order: client_order_id={}",
                order.client_order_id()
            );
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

        let kraken_cl_ord_id = truncate_cl_ord_id(&client_order_id);

        if kraken_cl_ord_id != client_order_id.as_str() {
            self.truncated_id_map
                .insert(kraken_cl_ord_id, client_order_id);
        }

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
                Ok(_report) => Ok(()),
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    let error_msg = format!("{task_name} error: {e}");
                    let due_post_only = error_msg.contains("POST_ONLY_REJECTED");
                    // The order will never appear on the wire, so its
                    // dispatch identity has to be cleaned up here.
                    dispatch_state.cleanup_terminal(&client_order_id);
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &error_msg,
                        ts_event,
                        due_post_only,
                    );
                    Ok(())
                }
            }
        });
    }

    fn cancel_single_order(&self, cmd: &CancelOrder) {
        let account_id = self.core.account_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;

        log::info!(
            "Canceling order: venue_order_id={venue_order_id:?}, client_order_id={client_order_id}"
        );

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            if let Err(e) = http
                .cancel_order(
                    account_id,
                    instrument_id,
                    Some(client_order_id),
                    venue_order_id,
                )
                .await
            {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("cancel-order error: {e}"),
                    ts_event,
                );
                anyhow::bail!("Cancel order failed: {e}");
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

        let handle = get_runtime().spawn(async move {
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
        });

        self.ws_stream_handle = Some(handle);
        Ok(())
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

        log::info!(
            "Modifying order: venue_order_id={venue_order_id:?}, client_order_id={client_order_id}"
        );

        let http = self.http.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            if let Err(e) = http
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
                let ts_event = clock.get_time_ns();
                emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("modify-order error: {e}"),
                    ts_event,
                );
                anyhow::bail!("Modify order failed: {e}");
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

        self.cancellation_token.cancel();
        self.core.set_stopped();
        self.core.set_disconnected();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        if !self.core.instruments_initialized() {
            let instruments = self
                .http
                .request_instruments()
                .await
                .context("Failed to load Kraken futures instruments")?;
            log::info!("Loaded {} Futures instruments", instruments.len());
            self.http.cache_instruments(&instruments);
            self.core.set_instruments_initialized();
        }

        self.instruments.rcu(|m| {
            for instrument in self.http.instruments_cache.load().values() {
                m.insert(instrument.id(), instrument.clone());
            }
        });

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

        // Request and register account state before message handler
        let account_state = self
            .http
            .request_account_state(self.core.account_id)
            .await
            .context("Failed to request Kraken futures account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
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

        log::info!("Futures WebSocket authenticated and subscribed to executions");

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.cancellation_token.cancel();

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        let _ = self.ws.close().await;

        self.cancellation_token = CancellationToken::new();
        self.core.set_disconnected();
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
        let reports = self
            .http
            .request_order_status_reports(account_id, None, None, None, false)
            .await?;

        // Match by venue_order_id or client_order_id (comparing truncated form
        // since Kraken stores the truncated cl_ord_id for long IDs)
        let matched = reports.into_iter().find(|r| {
            cmd.venue_order_id
                .is_some_and(|id| r.venue_order_id.as_str() == id.as_str())
                || cmd.client_order_id.is_some_and(|id| {
                    r.client_order_id
                        .as_ref()
                        .is_some_and(|r_id| r_id.as_str() == truncate_cl_ord_id(&id))
                })
        });

        if matched.is_some() {
            return Ok(matched);
        }

        let Some(order) = self.get_cached_order_for_status_command(cmd) else {
            return Ok(None);
        };

        let now = Utc::now();
        let start = now - Duration::from_secs(5 * 60);
        let fills = self
            .http
            .request_fill_reports(
                account_id,
                Some(order.instrument_id()),
                Some(start),
                Some(now),
            )
            .await?;

        Ok(synthesize_filled_order_status_report(cmd, &order, &fills))
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
        let start = cmd.start.map(DateTime::<Utc>::from);
        let end = cmd.end.map(DateTime::<Utc>::from);
        self.http
            .request_order_status_reports(account_id, cmd.instrument_id, start, end, cmd.open_only)
            .await
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
        let start = cmd.start.map(DateTime::<Utc>::from);
        let end = cmd.end.map(DateTime::<Utc>::from);
        let mut reports = self
            .http
            .request_fill_reports(account_id, cmd.instrument_id, start, end)
            .await?;

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id == venue_order_id);
        }

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

        let start = lookback_mins.map(|mins| Utc::now() - Duration::from_secs(mins * 60));

        let account_id = self.core.account_id;
        let order_reports = self
            .http
            .request_order_status_reports(account_id, None, start, None, true)
            .await?;
        let fill_reports = self
            .http
            .request_fill_reports(account_id, None, start, None)
            .await?;
        let position_reports = self
            .http
            .request_position_status_reports(account_id, None)
            .await?;

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *KRAKEN_VENUE,
            self.clock.get_time_ns(),
            None,
        );
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        log::debug!("Querying account: {cmd:?}");

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
            );
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("Querying order: {cmd:?}");

        let venue_order_id = cmd
            .venue_order_id
            .context("venue_order_id required for query_order")?;
        let account_id = self.core.account_id;
        let http = self.http.clone();
        let emitter = self.emitter.clone();

        self.spawn_task("query_order", async move {
            let reports = http
                .request_order_status_reports(account_id, None, None, None, true)
                .await
                .context("Failed to query order")?;

            if let Some(report) = reports
                .into_iter()
                .find(|r| r.venue_order_id == venue_order_id)
            {
                emitter.send_order_status_report(report);
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found in cache: {}", cmd.client_order_id))?;
        self.submit_single_order(&order, "submit_order");
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        log::info!(
            "Submitting order list: order_list_id={}, count={}",
            cmd.order_list.id,
            orders.len()
        );

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
            let kraken_cl_ord_id = truncate_cl_ord_id(&client_order_id);

            if kraken_cl_ord_id != client_order_id.as_str() {
                self.truncated_id_map
                    .insert(kraken_cl_ord_id, client_order_id);
            }

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
            match http.submit_orders_batch(order_tuples).await {
                Ok(statuses) => {
                    for (i, status) in statuses.iter().enumerate() {
                        if status.status != "placed"
                            && status.status != "filled"
                            && let Some((strategy_id, instrument_id, client_order_id)) =
                                order_meta.get(i)
                        {
                            let ts_event = clock.get_time_ns();
                            let error_msg = format!(
                                "submit_order_list batch item rejected: {}",
                                status.status,
                            );
                            dispatch_state.cleanup_terminal(client_order_id);
                            emitter.emit_order_rejected_event(
                                *strategy_id,
                                *instrument_id,
                                *client_order_id,
                                &error_msg,
                                ts_event,
                                status.status == "postWouldExecute",
                            );
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();

                    for (strategy_id, instrument_id, client_order_id) in &order_meta {
                        let error_msg = format!("submit_order_list batch error: {e}");
                        dispatch_state.cleanup_terminal(client_order_id);
                        emitter.emit_order_rejected_event(
                            *strategy_id,
                            *instrument_id,
                            *client_order_id,
                            &error_msg,
                            ts_event,
                            false,
                        );
                    }
                    Ok(())
                }
            }
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

        if cmd.order_side == OrderSide::NoOrderSide {
            log::info!("Canceling all orders: instrument_id={instrument_id} (bulk)");

            let http = self.http.clone();
            let symbol = instrument_id.symbol.to_string();

            self.spawn_task("cancel_all_orders", async move {
                if let Err(e) = http.inner.cancel_all_orders(Some(symbol)).await {
                    anyhow::bail!("Cancel all orders failed: {e}");
                }
                Ok(())
            });

            return Ok(());
        }

        log::info!(
            "Canceling all orders: instrument_id={instrument_id}, side={:?}",
            cmd.order_side
        );

        let orders_to_cancel: Vec<_> = {
            let cache = self.core.cache();
            let open_orders = cache.orders_open(None, Some(&instrument_id), None, None, None);

            open_orders
                .into_iter()
                .filter(|order| order.order_side() == cmd.order_side)
                .filter_map(|order| {
                    Some((
                        order.venue_order_id()?,
                        order.client_order_id(),
                        order.instrument_id(),
                        order.strategy_id(),
                    ))
                })
                .collect()
        };

        let account_id = self.core.account_id;

        for (venue_order_id, client_order_id, order_instrument_id, strategy_id) in orders_to_cancel
        {
            let http = self.http.clone();
            let emitter = self.emitter.clone();
            let clock = self.clock;

            self.spawn_task("cancel_order_by_side", async move {
                if let Err(e) = http
                    .cancel_order(
                        account_id,
                        order_instrument_id,
                        Some(client_order_id),
                        Some(venue_order_id),
                    )
                    .await
                {
                    log::error!("Cancel order failed: {e}");
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        order_instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &format!("cancel-order error: {e}"),
                        ts_event,
                    );
                }
                Ok(())
            });
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        log::info!(
            "Batch canceling orders: instrument_id={}, count={}",
            cmd.instrument_id,
            cmd.cancels.len()
        );

        for cancel in &cmd.cancels {
            self.cancel_single_order(cancel);
        }

        Ok(())
    }
}

impl KrakenFuturesExecutionClient {
    fn get_cached_order_for_status_command(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Option<OrderAny> {
        let cache = self.core.cache();

        if let Some(client_order_id) = cmd.client_order_id {
            return cache.order(&client_order_id).cloned();
        }

        let venue_order_id = cmd.venue_order_id?;
        let client_order_id = *cache.client_order_id(&venue_order_id)?;
        cache.order(&client_order_id).cloned()
    }
}

fn synthesize_filled_order_status_report(
    cmd: &GenerateOrderStatusReport,
    order: &OrderAny,
    fills: &[FillReport],
) -> Option<OrderStatusReport> {
    let venue_order_id = cmd.venue_order_id.or(order.venue_order_id());
    let truncated_client_order_id = truncate_cl_ord_id(&order.client_order_id());

    let mut matched: Vec<&FillReport> = if let Some(venue_order_id) = venue_order_id {
        fills
            .iter()
            .filter(|fill| fill.venue_order_id == venue_order_id)
            .collect()
    } else {
        Vec::new()
    };

    if matched.is_empty() {
        matched = fills
            .iter()
            .filter(|fill| {
                fill.client_order_id == Some(order.client_order_id())
                    || fill
                        .client_order_id
                        .as_ref()
                        .is_some_and(|fill_client_order_id| {
                            fill_client_order_id.as_str() == truncated_client_order_id
                        })
            })
            .collect();
    }

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
        order.order_side(),
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
        identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
        orders::OrderTestBuilder,
        reports::FillReport,
        types::{Currency, Money, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;

    const TEST_INSTRUMENT_ID: &str = "PF_XBTUSD.KRAKEN";

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
            Money::new(0.0, Currency::USD()),
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

        let report = synthesize_filled_order_status_report(&cmd, &order, &fills)
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

        assert!(synthesize_filled_order_status_report(&cmd, &order, &fills).is_none());
    }

    #[rstest]
    fn test_synthesize_filled_order_status_report_matches_truncated_client_order_id() {
        let long_client_order_id = "O202602270023210040011";
        let order = make_order(long_client_order_id);
        let cmd = make_cmd(Some(long_client_order_id), None);
        let fills = vec![make_fill(
            "KRAKEN-791",
            Some(truncate_cl_ord_id(&ClientOrderId::from(long_client_order_id)).as_str()),
            "100",
            "50000.0",
            1,
        )];

        let report = synthesize_filled_order_status_report(&cmd, &order, &fills)
            .expect("expected a filled report");

        assert_eq!(
            report.client_order_id,
            Some(ClientOrderId::from(long_client_order_id))
        );
        assert_eq!(report.venue_order_id, VenueOrderId::from("KRAKEN-791"));
        assert_eq!(report.order_status, OrderStatus::Filled);
    }
}
