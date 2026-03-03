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
    sync::Mutex,
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
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, Venue},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{consts::KRAKEN_VENUE, credential::KrakenCredential, parse::truncate_cl_ord_id},
    config::KrakenExecClientConfig,
    http::KrakenFuturesHttpClient,
    websocket::futures::{client::KrakenFuturesWebSocketClient, messages::KrakenFuturesWsMessage},
};

const MUTEX_POISONED: &str = "mutex poisoned";

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
            config.http_proxy.clone(),
            config.max_requests_per_second,
        )?;

        let credential = KrakenCredential::new(config.api_key.clone(), config.api_secret.clone());
        let ws = KrakenFuturesWebSocketClient::with_credentials(
            config.ws_url(),
            config.heartbeat_interval_secs,
            Some(credential),
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
        })
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

    fn submit_single_order(&self, order: &OrderAny, task_name: &'static str) -> anyhow::Result<()> {
        if order.is_closed() {
            log::warn!(
                "Cannot submit closed order: client_order_id={}",
                order.client_order_id()
            );
            return Ok(());
        }

        let account_id = self.core.account_id;
        let client_order_id = order.client_order_id();
        let trader_id = order.trader_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let is_reduce_only = order.is_reduce_only();
        let is_post_only = order.is_post_only();

        log::debug!("OrderSubmitted: client_order_id={client_order_id}");
        self.emitter.emit_order_submitted(order);

        self.ws
            .cache_client_order(client_order_id, None, instrument_id, trader_id, strategy_id);

        let kraken_cl_ord_id = truncate_cl_ord_id(&client_order_id);
        if kraken_cl_ord_id != client_order_id.as_str() {
            self.ws
                .cache_truncated_id(kraken_cl_ord_id, client_order_id);
        }

        let http = self.http.clone();
        let ws = self.ws.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

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
                    is_reduce_only,
                    is_post_only,
                )
                .await;

            match result {
                Ok(report) => {
                    // Update cache with venue_order_id so cancel messages without
                    // cli_ord_id can be mapped back to our orders
                    ws.cache_client_order(
                        client_order_id,
                        Some(report.venue_order_id),
                        instrument_id,
                        trader_id,
                        strategy_id,
                    );
                    Ok(())
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &format!("{task_name} error: {e}"),
                        ts_event,
                        false,
                    );
                    Err(e)
                }
            }
        });

        Ok(())
    }

    fn cancel_single_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
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
                log::error!("Cancel order failed: {e}");
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

        Ok(())
    }

    fn spawn_message_handler(&mut self) -> anyhow::Result<()> {
        let mut rx = self
            .ws
            .take_output_rx()
            .context("Failed to take futures WebSocket output receiver")?;
        let emitter = self.emitter.clone();
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
                                Self::handle_ws_message(ws_msg, &emitter);
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

    fn handle_ws_message(msg: KrakenFuturesWsMessage, emitter: &ExecutionEventEmitter) {
        match msg {
            KrakenFuturesWsMessage::OrderAccepted(event) => {
                emitter.send_order_event(OrderEventAny::Accepted(event));
            }
            KrakenFuturesWsMessage::OrderCanceled(event) => {
                emitter.send_order_event(OrderEventAny::Canceled(event));
            }
            KrakenFuturesWsMessage::OrderExpired(event) => {
                emitter.send_order_event(OrderEventAny::Expired(event));
            }
            KrakenFuturesWsMessage::OrderUpdated(event) => {
                emitter.send_order_event(OrderEventAny::Updated(event));
            }
            KrakenFuturesWsMessage::OrderStatusReport(report) => {
                emitter.send_order_status_report(*report);
            }
            KrakenFuturesWsMessage::FillReport(report) => {
                emitter.send_fill_report(*report);
            }
            KrakenFuturesWsMessage::Reconnected => {
                log::info!("Futures execution WebSocket reconnected");
            }
            // Data messages are handled by the data client
            KrakenFuturesWsMessage::BookDeltas(_)
            | KrakenFuturesWsMessage::Quote(_)
            | KrakenFuturesWsMessage::Trade(_)
            | KrakenFuturesWsMessage::MarkPrice(_)
            | KrakenFuturesWsMessage::IndexPrice(_)
            | KrakenFuturesWsMessage::FundingRate(_) => {}
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

    fn modify_single_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
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
                log::error!("Modify order failed: {e}");
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

        Ok(())
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
            self.http.cache_instruments(instruments);
            self.core.set_instruments_initialized();
        }

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

        self.ws.set_account_id(self.core.account_id);

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

        // Always cache to WS handler (reconnect spawns a fresh handler)
        let instruments: Vec<_> = self
            .http
            .instruments_cache
            .iter()
            .map(|entry| entry.value().clone())
            .collect();
        self.ws.cache_instruments(instruments);

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
        Ok(reports.into_iter().find(|r| {
            cmd.venue_order_id
                .is_some_and(|id| r.venue_order_id.as_str() == id.as_str())
                || cmd.client_order_id.is_some_and(|id| {
                    r.client_order_id
                        .as_ref()
                        .is_some_and(|r_id| r_id.as_str() == truncate_cl_ord_id(&id))
                })
        }))
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
        self.http
            .request_fill_reports(account_id, cmd.instrument_id, start, end)
            .await
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

    fn query_account(&self, cmd: &QueryAccount) -> anyhow::Result<()> {
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

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
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

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found in cache: {}", cmd.client_order_id))?;
        self.submit_single_order(&order, "submit_order")
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        log::info!(
            "Submitting order list: order_list_id={}, count={}",
            cmd.order_list.id,
            orders.len()
        );

        for order in &orders {
            self.submit_single_order(order, "submit_order_list")?;
        }

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        self.modify_single_order(cmd)
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        self.cancel_single_order(cmd)
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let instrument_id = cmd.instrument_id;

        if cmd.order_side == OrderSide::NoOrderSide {
            log::info!("Canceling all orders: instrument_id={instrument_id} (bulk)");

            let http = self.http.clone();
            let symbol = instrument_id.symbol.to_string();

            self.spawn_task("cancel_all_orders", async move {
                if let Err(e) = http.inner.cancel_all_orders(Some(symbol)).await {
                    log::error!("Cancel all orders failed: {e}");
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

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        log::info!(
            "Batch canceling orders: instrument_id={}, count={}",
            cmd.instrument_id,
            cmd.cancels.len()
        );

        for cancel in &cmd.cancels {
            self.cancel_single_order(cancel)?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::cache::Cache;
    use nautilus_model::{
        enums::AccountType,
        identifiers::{AccountId, ClientId, TraderId},
    };
    use rstest::rstest;

    use super::*;
    use crate::{common::enums::KrakenProductType, config::KrakenExecClientConfig};

    fn create_test_core() -> ExecutionClientCore {
        let cache = Rc::new(RefCell::new(Cache::default()));
        ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            ClientId::from("KRAKEN"),
            *KRAKEN_VENUE,
            OmsType::Netting,
            AccountId::from("KRAKEN-001"),
            AccountType::Margin,
            None,
            cache,
        )
    }

    #[rstest]
    fn test_futures_exec_client_new() {
        let config = KrakenExecClientConfig {
            product_type: KrakenProductType::Futures,
            api_key: "test_key".to_string(),
            api_secret: "test_secret".to_string(),
            ..Default::default()
        };

        let client = KrakenFuturesExecutionClient::new(create_test_core(), config);
        assert!(client.is_ok());

        let client = client.unwrap();
        assert_eq!(client.client_id(), ClientId::from("KRAKEN"));
        assert_eq!(client.account_id(), AccountId::from("KRAKEN-001"));
        assert_eq!(client.venue(), *KRAKEN_VENUE);
        assert!(!client.is_connected());
    }

    #[rstest]
    fn test_futures_exec_client_start_stop() {
        let (sender, _receiver) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::live::runner::set_exec_event_sender(sender);

        let config = KrakenExecClientConfig {
            product_type: KrakenProductType::Futures,
            api_key: "test_key".to_string(),
            api_secret: "test_secret".to_string(),
            ..Default::default()
        };

        let mut client = KrakenFuturesExecutionClient::new(create_test_core(), config).unwrap();

        assert!(client.start().is_ok());
        assert!(client.stop().is_ok());
        assert!(!client.is_connected());
    }
}
