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

//! Kraken Spot execution client implementation.

use std::{
    future::Future,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::StreamExt;
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
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce, TrailingOffsetType},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Venue},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use crate::{
    common::{
        consts::{KRAKEN_SPOT_POST_ONLY_ERROR, KRAKEN_VENUE},
        parse::truncate_cl_ord_id,
    },
    config::KrakenExecClientConfig,
    http::{KrakenSpotHttpClient, spot::client::KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND},
    websocket::{
        dispatch::{self, OrderIdentity, WsDispatchState},
        spot_v2::{client::KrakenSpotWebSocketClient, messages::KrakenSpotWsMessage},
    },
};

/// Kraken Spot execution client.
///
/// Provides order management and account operations for Kraken Spot markets.
#[allow(dead_code)]
#[derive(Debug)]
pub struct KrakenSpotExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: KrakenExecClientConfig,
    emitter: ExecutionEventEmitter,
    http: KrakenSpotHttpClient,
    ws: KrakenSpotWebSocketClient,
    cancellation_token: CancellationToken,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    order_qty_cache: Arc<AtomicMap<String, f64>>,
    truncated_id_map: Arc<AtomicMap<String, ClientOrderId>>,
    ws_dispatch_state: Arc<WsDispatchState>,
}

impl KrakenSpotExecutionClient {
    /// Creates a new [`KrakenSpotExecutionClient`].
    pub fn new(core: ExecutionClientCore, config: KrakenExecClientConfig) -> anyhow::Result<Self> {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Cash,
            None,
        );

        let cancellation_token = CancellationToken::new();

        let http = KrakenSpotHttpClient::with_credentials(
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
                .unwrap_or(KRAKEN_SPOT_DEFAULT_RATE_LIMIT_PER_SECOND),
        )?;

        let data_config = crate::config::KrakenDataClientConfig {
            api_key: Some(config.api_key.clone()),
            api_secret: Some(config.api_secret.clone()),
            product_type: config.product_type,
            environment: config.environment,
            base_url: config.base_url.clone(),
            ws_public_url: None,
            ws_private_url: Some(config.ws_url()),
            proxy_url: config.proxy_url.clone(),
            timeout_secs: config.timeout_secs,
            heartbeat_interval_secs: config.heartbeat_interval_secs,
            max_requests_per_second: config.max_requests_per_second,
            transport_backend: config.transport_backend,
        };
        let ws = KrakenSpotWebSocketClient::new(
            data_config,
            cancellation_token.clone(),
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
            order_qty_cache: Arc::new(AtomicMap::new()),
            truncated_id_map: Arc::new(AtomicMap::new()),
            ws_dispatch_state: Arc::new(WsDispatchState::new()),
        })
    }

    fn register_order_identity(&self, order: &OrderAny) {
        // Quote-quantity orders submit a quote amount (e.g. 100 USD), but the
        // venue reports fills in base units (e.g. 0.001 BTC). Registering the
        // raw `order.quantity()` would make the cumulative-fill comparison in
        // the fill-side dispatch mismatch base against quote, leaving the
        // order "open" forever. These orders instead flow through the
        // untracked path and the engine reconciles them from status reports.
        if order.is_quote_quantity() {
            return;
        }
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

        let order_type = order.order_type();
        let time_in_force = order.time_in_force();

        // FOK only supported for plain limit orders on Kraken Spot
        if time_in_force == TimeInForce::Fok && order_type != OrderType::Limit {
            self.emitter.emit_order_denied(
                order,
                "FOK time in force only supported for LIMIT orders on Kraken Spot",
            );
            return;
        }

        // Kraken only supports price-based trailing offsets
        if matches!(
            order_type,
            OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
        ) && let Some(offset_type) = order.trailing_offset_type()
            && offset_type != TrailingOffsetType::Price
        {
            self.emitter.emit_order_denied(
                order,
                &format!(
                    "Kraken Spot only supports Price trailing offset type: received {offset_type:?}"
                ),
            );
            return;
        }

        let account_id = self.core.account_id;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let quantity = order.quantity();
        let expire_time = order.expire_time();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let trigger_type = order.trigger_type();
        let trailing_offset = order.trailing_offset();
        let limit_offset = order.limit_offset();
        let is_reduce_only = order.is_reduce_only();
        let is_post_only = order.is_post_only();
        let is_quote_quantity = order.is_quote_quantity();
        let display_qty = order.display_qty();

        log::debug!("OrderSubmitted: client_order_id={client_order_id}");
        self.register_order_identity(order);
        self.emitter.emit_order_submitted(order);

        let kraken_cl_ord_id = truncate_cl_ord_id(&client_order_id);

        // Only cache base-denominated quantities; quote quantities
        // are not valid for the WS order report fallback path
        if !is_quote_quantity {
            self.order_qty_cache
                .insert(kraken_cl_ord_id.clone(), quantity.as_f64());
        }

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
                    expire_time,
                    price,
                    trigger_price,
                    trigger_type,
                    trailing_offset,
                    limit_offset,
                    is_reduce_only,
                    is_post_only,
                    is_quote_quantity,
                    display_qty,
                )
                .await;

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                let error_msg = format!("{task_name} error: {e}");
                let due_post_only = error_msg.contains("POST_ONLY_REJECTED")
                    || error_msg.contains(KRAKEN_SPOT_POST_ONLY_ERROR);
                // The order will never appear on the wire, so its dispatch
                // identity has to be cleaned up here.
                dispatch_state.cleanup_terminal(&client_order_id);
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &error_msg,
                    ts_event,
                    due_post_only,
                );
                return Ok(());
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
        let stream = self.ws.stream().map_err(|e| anyhow::anyhow!("{e}"))?;
        let emitter = self.emitter.clone();
        let instruments = self.instruments.clone();
        let order_qty_cache = self.order_qty_cache.clone();
        let truncated_id_map = self.truncated_id_map.clone();
        let dispatch_state = self.ws_dispatch_state.clone();
        let account_id = self.core.account_id;
        let clock = self.clock;
        let cancellation_token = self.cancellation_token.clone();

        let handle = get_runtime().spawn(async move {
            tokio::pin!(stream);

            loop {
                tokio::select! {
                    () = cancellation_token.cancelled() => {
                        log::debug!("Spot execution message handler cancelled");
                        break;
                    }
                    msg = stream.next() => {
                        match msg {
                            Some(ws_msg) => {
                                Self::handle_ws_message(
                                    ws_msg,
                                    &emitter,
                                    &dispatch_state,
                                    &instruments,
                                    &order_qty_cache,
                                    &truncated_id_map,
                                    account_id,
                                    clock,
                                );
                            }
                            None => {
                                log::debug!("Spot execution WebSocket stream ended");
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

    #[expect(clippy::too_many_arguments)]
    fn handle_ws_message(
        msg: KrakenSpotWsMessage,
        emitter: &ExecutionEventEmitter,
        dispatch_state: &Arc<WsDispatchState>,
        instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
        order_qty_cache: &Arc<AtomicMap<String, f64>>,
        truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
        account_id: AccountId,
        clock: &'static AtomicTime,
    ) {
        match msg {
            KrakenSpotWsMessage::Execution(executions) => {
                let ts_init = clock.get_time_ns();

                for exec in &executions {
                    dispatch::spot::execution(
                        exec,
                        dispatch_state,
                        emitter,
                        instruments,
                        truncated_id_map,
                        order_qty_cache,
                        account_id,
                        ts_init,
                    );
                }
            }
            KrakenSpotWsMessage::Reconnected => {
                log::info!("Spot execution WebSocket reconnected");
            }
            KrakenSpotWsMessage::Ticker(_)
            | KrakenSpotWsMessage::Trade(_)
            | KrakenSpotWsMessage::Book { .. }
            | KrakenSpotWsMessage::Ohlc(_) => {}
        }
    }
}

#[async_trait(?Send)]
impl ExecutionClient for KrakenSpotExecutionClient {
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
            "Started: client_id={}, account_id={}, product_type=Spot, environment={:?}",
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
                .request_instruments(None)
                .await
                .context("Failed to load Kraken spot instruments")?;
            log::info!("Loaded {} Spot instruments", instruments.len());
            self.http.cache_instruments(&instruments);
            self.core.set_instruments_initialized();
        }

        self.ws
            .connect()
            .await
            .context("Failed to connect spot WebSocket")?;
        self.ws
            .wait_until_active(10.0)
            .await
            .context("Spot WebSocket failed to become active")?;

        self.ws
            .authenticate()
            .await
            .context("Failed to authenticate spot WebSocket")?;

        // Request initial account state and await registration before spawning
        // the message handler. Report events from execution snapshots conflict
        // with ExecEngine borrows during startup, so account registration must
        // complete first.
        let account_state = self
            .http
            .request_account_state(self.core.account_id)
            .await
            .context("Failed to request Kraken account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }

        self.emitter.send_account_state(account_state);
        self.await_account_registered(30.0).await?;

        self.spawn_message_handler()?;

        self.instruments.rcu(|m| {
            for instrument in self.http.instruments_cache.load().values() {
                m.insert(instrument.id(), instrument.clone());
            }
        });

        self.ws
            .subscribe_executions(false, false)
            .await
            .context("Failed to subscribe to executions")?;

        log::info!("Spot WebSocket authenticated and subscribed to executions");

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

            if order.time_in_force() == TimeInForce::Fok && order.order_type() != OrderType::Limit {
                self.emitter.emit_order_denied(
                    order,
                    "FOK time in force only supported for LIMIT orders on Kraken Spot",
                );
                continue;
            }

            if matches!(
                order.order_type(),
                OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
            ) && let Some(offset_type) = order.trailing_offset_type()
                && offset_type != TrailingOffsetType::Price
            {
                self.emitter.emit_order_denied(
                    order,
                    &format!(
                        "Kraken Spot only supports Price trailing offset type: received {offset_type:?}"
                    ),
                );
                continue;
            }

            let client_order_id = order.client_order_id();
            let kraken_cl_ord_id = truncate_cl_ord_id(&client_order_id);

            self.register_order_identity(order);
            self.emitter.emit_order_submitted(order);

            if !order.is_quote_quantity() {
                self.order_qty_cache
                    .insert(kraken_cl_ord_id.clone(), order.quantity().as_f64());
            }

            if kraken_cl_ord_id != client_order_id.as_str() {
                self.truncated_id_map
                    .insert(kraken_cl_ord_id, client_order_id);
            }

            order_tuples.push((
                order.instrument_id(),
                client_order_id,
                order.order_side(),
                order.order_type(),
                order.quantity(),
                order.time_in_force(),
                order.expire_time(),
                order.price(),
                order.trigger_price(),
                order.trigger_type(),
                order.trailing_offset(),
                order.limit_offset(),
                order.is_reduce_only(),
                order.is_post_only(),
                order.is_quote_quantity(),
                order.display_qty(),
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
                    // The HTTP helper returns one status per input tuple, including validation failures
                    for (i, status) in statuses.iter().enumerate() {
                        if status != "placed"
                            && let Some((strategy_id, instrument_id, client_order_id)) =
                                order_meta.get(i)
                        {
                            let ts_event = clock.get_time_ns();
                            let due_post_only = status.contains("POST_ONLY_REJECTED")
                                || status.contains(KRAKEN_SPOT_POST_ONLY_ERROR);
                            dispatch_state.cleanup_terminal(client_order_id);
                            emitter.emit_order_rejected_event(
                                *strategy_id,
                                *instrument_id,
                                *client_order_id,
                                &format!("submit_order_list batch item rejected: {status}"),
                                ts_event,
                                due_post_only,
                            );
                        }
                    }
                    Ok(())
                }
                Err(e) => {
                    let ts_event = clock.get_time_ns();
                    let error_msg = format!("submit_order_list batch error: {e}");

                    for (strategy_id, instrument_id, client_order_id) in &order_meta {
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

            self.spawn_task("cancel_all_orders", async move {
                if let Err(e) = http.inner.cancel_all_orders().await {
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
