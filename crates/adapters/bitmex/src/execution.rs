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

//! Live execution client implementation for the BitMEX adapter.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    enums::LogLevel,
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
    UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType, TrailingOffsetType},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use rust_decimal::prelude::ToPrimitive;
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    broadcast::{
        canceller::{CancelBroadcaster, CancelBroadcasterConfig},
        submitter::{DEFINITIVE_SUBMIT_REJECTION, SubmitBroadcaster, SubmitBroadcasterConfig},
    },
    common::{
        enums::{BitmexContingencyType, BitmexOrderType, BitmexPegPriceType, BitmexTimeInForce},
        parse::{parse_peg_offset_value, parse_peg_price_type},
    },
    config::BitmexExecClientConfig,
    http::{client::BitmexHttpClient, error::BitmexHttpError},
    websocket::{
        client::BitmexWebSocketClient,
        dispatch::{self, OrderIdentity, WsDispatchState},
    },
};

#[derive(Debug)]
pub struct BitmexExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: BitmexExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: BitmexHttpClient,
    ws_client: BitmexWebSocketClient,
    ws_dispatch_state: Arc<WsDispatchState>,
    _submitter: SubmitBroadcaster,
    _canceller: CancelBroadcaster,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    dms_task_handle: Option<JoinHandle<()>>,
    dms_running: Arc<AtomicBool>,
}

impl BitmexExecutionClient {
    fn log_report_receipt(count: usize, report_type: &str, log_level: LogLevel) {
        let plural = if count == 1 { "" } else { "s" };
        let message = format!("Received {count} {report_type}{plural}");

        match log_level {
            LogLevel::Off => {}
            LogLevel::Trace => log::trace!("{message}"),
            LogLevel::Debug => log::debug!("{message}"),
            LogLevel::Info => log::info!("{message}"),
            LogLevel::Warning => log::warn!("{message}"),
            LogLevel::Error => log::error!("{message}"),
        }
    }

    /// Creates a new [`BitmexExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if either the HTTP or WebSocket client fail to construct.
    pub fn new(
        mut core: ExecutionClientCore,
        config: BitmexExecClientConfig,
    ) -> anyhow::Result<Self> {
        if !config.has_api_credentials() {
            anyhow::bail!("BitMEX execution client requires API key and secret");
        }

        if let Some(account_id) = config.account_id {
            core.set_account_id(account_id);
        }

        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let clock = get_atomic_clock_realtime();
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let http_client = BitmexHttpClient::new(
            Some(config.http_base_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.environment,
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            config.max_requests_per_second,
            config.max_requests_per_minute,
            config.proxy_url.clone(),
        )
        .context("failed to construct BitMEX HTTP client")?;
        let ws_client = BitmexWebSocketClient::new_with_env(
            Some(config.ws_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            Some(account_id),
            config.heartbeat_interval_secs,
            config.environment,
            config.transport_backend,
            config.proxy_url.clone(),
        )
        .context("failed to construct BitMEX execution websocket client")?;

        let pool_size = config.submitter_pool_size.unwrap_or(1);
        let submitter_proxy_urls = match &config.submitter_proxy_urls {
            Some(urls) => urls.iter().map(|url| Some(url.clone())).collect(),
            None => vec![config.proxy_url.clone(); pool_size],
        };

        let submitter_config = SubmitBroadcasterConfig {
            pool_size,
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            base_url: config.base_url_http.clone(),
            environment: config.environment,
            timeout_secs: config.http_timeout_secs,
            max_retries: config.max_retries,
            retry_delay_ms: config.retry_delay_initial_ms,
            retry_delay_max_ms: config.retry_delay_max_ms,
            recv_window_ms: config.recv_window_ms,
            max_requests_per_second: config.max_requests_per_second,
            max_requests_per_minute: config.max_requests_per_minute,
            proxy_urls: submitter_proxy_urls,
            ..Default::default()
        };

        let _submitter = SubmitBroadcaster::new(submitter_config)
            .context("failed to create SubmitBroadcaster")?;

        let canceller_pool_size = config.canceller_pool_size.unwrap_or(1);
        let canceller_proxy_urls = match &config.canceller_proxy_urls {
            Some(urls) => urls.iter().map(|url| Some(url.clone())).collect(),
            None => vec![config.proxy_url.clone(); canceller_pool_size],
        };

        let canceller_config = CancelBroadcasterConfig {
            pool_size: canceller_pool_size,
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            base_url: config.base_url_http.clone(),
            environment: config.environment,
            timeout_secs: config.http_timeout_secs,
            max_retries: config.max_retries,
            retry_delay_ms: config.retry_delay_initial_ms,
            retry_delay_max_ms: config.retry_delay_max_ms,
            recv_window_ms: config.recv_window_ms,
            max_requests_per_second: config.max_requests_per_second,
            max_requests_per_minute: config.max_requests_per_minute,
            proxy_urls: canceller_proxy_urls,
            ..Default::default()
        };

        let _canceller = CancelBroadcaster::new(canceller_config)
            .context("failed to create CancelBroadcaster")?;

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_client,
            ws_dispatch_state: Arc::new(WsDispatchState::default()),
            _submitter,
            _canceller,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
            dms_task_handle: None,
            dms_running: Arc::new(AtomicBool::new(false)),
        })
    }

    fn spawn_task<F>(&self, label: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let handle = get_runtime().spawn(async move {
            if let Err(e) = fut.await {
                log::error!("{label}: {e:?}");
            }
        });

        let mut guard = self
            .pending_tasks
            .lock()
            .expect("pending task lock poisoned");

        // Remove completed tasks to prevent unbounded growth
        guard.retain(|h| !h.is_finished());
        guard.push(handle);
    }

    fn abort_pending_tasks(&self) {
        let mut guard = self
            .pending_tasks
            .lock()
            .expect("pending task lock poisoned");

        for handle in guard.drain(..) {
            handle.abort();
        }
    }

    /// Populates `order_identities` for an order if not already present.
    ///
    /// Needed for cancel/modify commands on orders loaded via reconciliation
    /// (which bypass `submit_order` and therefore have no identity entry).
    fn ensure_order_identity(
        &self,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
    ) {
        if self
            .ws_dispatch_state
            .order_identities
            .contains_key(&client_order_id)
        {
            return;
        }

        let cache = self.core.cache();
        let (order_side, order_type) = cache
            .order(&client_order_id)
            .map_or((OrderSide::NoOrderSide, OrderType::Market), |o| {
                (o.order_side(), o.order_type())
            });
        drop(cache);

        self.ws_dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
            },
        );
        self.ws_dispatch_state.insert_accepted(client_order_id);
    }

    fn start_deadmans_switch(&mut self) {
        let Some(timeout_secs) = self.config.deadmans_switch_timeout_secs else {
            return;
        };

        let timeout_ms = timeout_secs * 1000;
        let interval_secs = (timeout_secs / 4).max(1);

        log::info!(
            "Starting dead man's switch: timeout={timeout_secs}s, refresh_interval={interval_secs}s",
        );

        self.dms_running.store(true, Ordering::SeqCst);
        let running = self.dms_running.clone();
        let http_client = self.http_client.clone();

        let handle = get_runtime().spawn(async move {
            while running.load(Ordering::SeqCst) {
                if let Err(e) = http_client.cancel_all_after(timeout_ms).await {
                    log::warn!("Dead man's switch heartbeat failed: {e}");
                }
                tokio::time::sleep(Duration::from_secs(interval_secs)).await;
            }
        });

        self.dms_task_handle = Some(handle);
    }

    async fn stop_deadmans_switch(&mut self) {
        if self.config.deadmans_switch_timeout_secs.is_none() {
            return;
        }

        self.dms_running.store(false, Ordering::SeqCst);

        // Abort and await loop shutdown so disconnect does not block on sleep/HTTP timeout.
        if let Some(handle) = self.dms_task_handle.take() {
            handle.abort();
            let _ = handle.await;
        }

        log::info!("Disarming dead man's switch");

        if let Err(e) = self.http_client.cancel_all_after(0).await {
            log::warn!("Failed to disarm dead man's switch: {e}");
        }
    }

    async fn ensure_instruments_initialized_async(&self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }

        let mut instruments: Vec<InstrumentAny> = {
            let cache = self.core.cache();
            cache
                .instruments(&self.core.venue, None)
                .into_iter()
                .cloned()
                .collect()
        };

        if instruments.is_empty() {
            let http = self.http_client.clone();
            instruments = http
                .request_instruments(self.config.active_only)
                .await
                .context("failed to request BitMEX instruments")?;
        } else {
            log::debug!(
                "Reusing {} cached BitMEX instruments for execution client initialization",
                instruments.len()
            );
        }

        instruments.sort_by_key(|instrument| instrument.id());

        self.http_client.cache_instruments(&instruments);
        self.ws_client.cache_instruments(&instruments);
        for instrument in &instruments {
            self._submitter.cache_instrument(instrument);
            self._canceller.cache_instrument(instrument);
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn refresh_account_state(&mut self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request BitMEX account state")?;

        self.apply_account_id(account_state.account_id);
        self.emitter.send_account_state(account_state);
        Ok(())
    }

    fn apply_account_id(&mut self, account_id: AccountId) {
        if self.core.account_id != account_id {
            log::debug!(
                "Discovered BitMEX account ID: account_id={} (was {})",
                account_id,
                self.core.account_id
            );
        }

        self.core.set_account_id(account_id);
        self.emitter.set_account_id(account_id);
        self.ws_client.set_account_id(account_id);
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

    fn start_ws_stream(&mut self) {
        if self.ws_stream_handle.is_some() {
            return;
        }

        let stream = self.ws_client.stream();
        let emitter = self.emitter.clone();
        let state = Arc::clone(&self.ws_dispatch_state);
        let account_id = self.core.account_id;
        let clock = self.clock;

        // Build symbol-keyed instrument map, preferring core cache then HTTP client cache
        let mut instruments_by_symbol: AHashMap<Ustr, InstrumentAny> = self
            .core
            .cache()
            .instruments(&self.core.venue, None)
            .into_iter()
            .map(|inst| (inst.symbol().inner(), inst.clone()))
            .collect();

        if instruments_by_symbol.is_empty() {
            for (key, inst) in self.http_client.instruments_cache.load().iter() {
                instruments_by_symbol.insert(*key, inst.clone());
            }
        }

        let handle = get_runtime().spawn(async move {
            pin_mut!(stream);
            let mut order_type_cache: AHashMap<ClientOrderId, OrderType> = AHashMap::new();
            let mut order_symbol_cache: AHashMap<ClientOrderId, Ustr> = AHashMap::new();
            let mut insts_by_symbol = instruments_by_symbol;

            while let Some(message) = stream.next().await {
                dispatch::dispatch_ws_message(
                    clock.get_time_ns(),
                    message,
                    &emitter,
                    &state,
                    &mut insts_by_symbol,
                    &mut order_type_cache,
                    &mut order_symbol_cache,
                    account_id,
                );
            }
        });

        self.ws_stream_handle = Some(handle);
    }

    fn submit_cached_order(
        &self,
        order: &OrderAny,
        submit_tries: Option<usize>,
        peg_price_type: Option<BitmexPegPriceType>,
        peg_offset_value: Option<f64>,
        task_label: &'static str,
    ) {
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return;
        }

        if let Err(e) = validate_order_for_bitmex_submit(order, peg_price_type, peg_offset_value) {
            self.emitter.emit_order_denied(order, &e.to_string());
            return;
        }

        self.emitter.emit_order_submitted(order);

        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let client_order_id = order.client_order_id();
        let order_side = order.order_side();
        let order_type = order.order_type();

        self.ws_dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
            },
        );

        let use_broadcaster = submit_tries.is_some_and(|n| n > 1);
        let http_client = self.http_client.clone();
        let submitter = self._submitter.clone_for_async();
        let ws_dispatch_state = self.ws_dispatch_state.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let trigger_type = order.trigger_type();
        let trailing_offset = order.trailing_offset().and_then(|d| d.to_f64());
        let trailing_offset_type = order.trailing_offset_type();
        let display_qty = order.display_qty();
        let post_only = order.is_post_only();
        let reduce_only = order.is_reduce_only();
        let order_list_id = order.order_list_id();
        let contingency_type = order.contingency_type();

        self.spawn_task(task_label, async move {
            let result = if use_broadcaster {
                submitter
                    .broadcast_submit(
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        trigger_type,
                        trailing_offset,
                        trailing_offset_type,
                        display_qty,
                        post_only,
                        reduce_only,
                        order_list_id,
                        contingency_type,
                        submit_tries,
                        peg_price_type,
                        peg_offset_value,
                    )
                    .await
            } else {
                http_client
                    .submit_order(
                        instrument_id,
                        client_order_id,
                        order_side,
                        order_type,
                        quantity,
                        time_in_force,
                        price,
                        trigger_price,
                        trigger_type,
                        trailing_offset,
                        trailing_offset_type,
                        display_qty,
                        post_only,
                        reduce_only,
                        order_list_id,
                        contingency_type,
                        peg_price_type,
                        peg_offset_value,
                    )
                    .await
            };

            match result {
                Ok(_report) => {
                    // The WS dispatch handles all lifecycle events for tracked orders.
                    // Forwarding the HTTP response as a report would cause the ExecEngine
                    // to generate inferred fills that conflict with real fills from the
                    // Execution table WS stream.
                }
                Err(e) => handle_submit_failure(&SubmitFailure {
                    err: &e,
                    ws_dispatch_state: &ws_dispatch_state,
                    emitter: &emitter,
                    clock,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    post_only,
                }),
            }
            Ok(())
        });
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BitmexExecutionClient {
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
        self.core.venue
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
            "BitMEX execution client started: client_id={}, account_id={}, environment={}, submitter_pool_size={:?}, canceller_pool_size={:?}, proxy_url={:?}, submitter_proxy_urls={:?}, canceller_proxy_urls={:?}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment,
            self.config.submitter_pool_size,
            self.config.canceller_pool_size,
            self.config.proxy_url,
            self.config.submitter_proxy_urls,
            self.config.canceller_proxy_urls,
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

        if let Some(handle) = self.dms_task_handle.take() {
            handle.abort();
        }
        self.dms_running.store(false, Ordering::SeqCst);
        self.abort_pending_tasks();
        log::info!("BitMEX execution client {} stopped", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        // Reset cancellation token so HTTP requests succeed after reconnect
        self.http_client.reset_cancellation_token();

        self.ensure_instruments_initialized_async().await?;

        self.refresh_account_state().await?;
        self.await_account_registered(30.0).await?;

        self.ws_client.connect().await?;
        self.ws_client.wait_until_active(10.0).await?;

        // Start submitter/canceller after WS connection succeeds
        self._submitter.start().await?;
        self._canceller.start().await?;

        self.ws_client.subscribe_orders().await?;
        self.ws_client.subscribe_executions().await?;
        self.ws_client.subscribe_positions().await?;
        self.ws_client.subscribe_wallet().await?;
        if let Err(e) = self.ws_client.subscribe_margin().await {
            log::debug!("Margin subscription unavailable: {e:?}");
        }

        self.start_ws_stream();

        self.core.set_connected();
        self.start_deadmans_switch();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        // Disarm DMS before cancelling requests (needs working HTTP)
        self.stop_deadmans_switch().await;

        self.http_client.cancel_all_requests();
        self._submitter.stop().await;
        self._canceller.stop().await;

        if let Err(e) = self.ws_client.close().await {
            log::warn!("Error while closing BitMEX execution websocket: {e:?}");
        }

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        self.abort_pending_tasks();
        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let instrument_id = cmd
            .instrument_id
            .context("BitMEX generate_order_status_report requires an instrument identifier")?;

        self.http_client
            .query_order(
                instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id.map(|id| VenueOrderId::from(id.as_str())),
            )
            .await
            .context("failed to query BitMEX order status")
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let start_dt = cmd.start.map(|nanos| nanos.to_datetime_utc());
        let end_dt = cmd.end.map(|nanos| nanos.to_datetime_utc());

        let mut reports = self
            .http_client
            .request_order_status_reports(cmd.instrument_id, cmd.open_only, start_dt, end_dt, None)
            .await
            .context("failed to request BitMEX order status reports")?;

        if let Some(start) = cmd.start {
            reports.retain(|report| report.ts_last >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|report| report.ts_last <= end);
        }

        Self::log_report_receipt(reports.len(), "OrderStatusReport", cmd.log_receipt_level);

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let start_dt = cmd.start.map(|nanos| nanos.to_datetime_utc());
        let end_dt = cmd.end.map(|nanos| nanos.to_datetime_utc());

        let mut reports = self
            .http_client
            .request_fill_reports(cmd.instrument_id, start_dt, end_dt, None)
            .await
            .context("failed to request BitMEX fill reports")?;

        if let Some(order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == order_id.as_str());
        }

        if let Some(start) = cmd.start {
            reports.retain(|report| report.ts_event >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|report| report.ts_event <= end);
        }

        Self::log_report_receipt(reports.len(), "FillReport", cmd.log_receipt_level);

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = self
            .http_client
            .request_position_status_reports()
            .await
            .context("failed to request BitMEX position reports")?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if let Some(start) = cmd.start {
            reports.retain(|report| report.ts_last >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|report| report.ts_last <= end);
        }

        Self::log_report_receipt(reports.len(), "PositionStatusReport", cmd.log_receipt_level);

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins.saturating_mul(60).saturating_mul(1_000_000_000);
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let fill_cmd = GenerateFillReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            self.core.venue,
            ts_now,
            None,
        );
        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;

        self.spawn_task("query_account", async move {
            match http_client.request_account_state(account_id).await {
                Ok(account_state) => emitter.send_account_state(account_state),
                Err(e) => log::error!("BitMEX query account failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = cmd.venue_order_id;
        let emitter = self.emitter.clone();

        self.spawn_task("query_order", async move {
            match http_client
                .request_order_status_report(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(report) => emitter.send_order_status_report(report),
                Err(e) => log::error!("BitMEX query order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let submit_tries = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;

        let peg_price_type = match parse_peg_price_type(cmd.params.as_ref()) {
            Ok(value) => value,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };
        let peg_offset_value = match parse_peg_offset_value(cmd.params.as_ref()) {
            Ok(value) => value,
            Err(e) => {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }
        };

        self.submit_cached_order(
            &order,
            submit_tries,
            peg_price_type,
            peg_offset_value,
            "submit_order",
        );
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        if cmd.order_list.client_order_ids.is_empty() {
            log::debug!("submit_order_list called with empty order list");
            return Ok(());
        }

        let submit_tries = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        let peg_price_type = match parse_peg_price_type(cmd.params.as_ref()) {
            Ok(value) => value,
            Err(e) => {
                for order in &orders {
                    self.emitter.emit_order_denied(order, &e.to_string());
                }
                return Ok(());
            }
        };
        let peg_offset_value = match parse_peg_offset_value(cmd.params.as_ref()) {
            Ok(value) => value,
            Err(e) => {
                for order in &orders {
                    self.emitter.emit_order_denied(order, &e.to_string());
                }
                return Ok(());
            }
        };

        log::debug!(
            "Submitting BitMEX order list: order_list_id={}, count={}",
            cmd.order_list.id,
            orders.len(),
        );

        for order in orders {
            self.submit_cached_order(
                &order,
                submit_tries,
                peg_price_type,
                peg_offset_value,
                "submit_order_list_item",
            );
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let client_order_id_opt = Some(client_order_id);
        let venue_order_id = cmd.venue_order_id;
        let quantity = cmd.quantity;
        let price = cmd.price;
        let trigger_price = cmd.trigger_price;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("modify_order", async move {
            match http_client
                .modify_order(
                    instrument_id,
                    client_order_id_opt,
                    venue_order_id,
                    quantity,
                    price,
                    trigger_price,
                )
                .await
            {
                Ok(_) => {
                    log::debug!(
                        "BitMEX modify accepted by REST, awaiting websocket confirmation: client_order_id={client_order_id}"
                    );
                }
                Err(e) => handle_modify_failure(&ModifyFailure {
                    err: &e,
                    emitter: &emitter,
                    clock,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                }),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.ensure_order_identity(cmd.client_order_id, cmd.strategy_id, cmd.instrument_id);
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
        let dispatch_state = Arc::clone(&self.ws_dispatch_state);
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = cmd.venue_order_id;

        self.spawn_task("cancel_order", async move {
            match canceller
                .broadcast_cancel(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(Some(report)) => {
                    if let Some(cid) = &report.client_order_id {
                        dispatch_state.tombstone_order(cid);
                    }
                    emitter.send_order_status_report(report);
                }
                Ok(None) => {
                    log::debug!("Order already cancelled: {client_order_id:?}");
                }
                Err(e) => log::error!("BitMEX cancel order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
        let dispatch_state = Arc::clone(&self.ws_dispatch_state);
        let instrument_id = cmd.instrument_id;
        let order_side = if cmd.order_side == OrderSide::NoOrderSide {
            log::debug!(
                "BitMEX cancel_all_orders received NoOrderSide for {instrument_id}, using unfiltered cancel-all",
            );
            None
        } else {
            Some(cmd.order_side)
        };

        self.spawn_task("cancel_all_orders", async move {
            match canceller
                .broadcast_cancel_all(instrument_id, order_side)
                .await
            {
                Ok(reports) => {
                    for report in &reports {
                        if let Some(cid) = &report.client_order_id {
                            dispatch_state.tombstone_order(cid);
                        }
                    }

                    for report in reports {
                        emitter.send_order_status_report(report);
                    }
                }
                Err(e) => log::error!("BitMEX cancel all failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
        let dispatch_state = Arc::clone(&self.ws_dispatch_state);
        let instrument_id = cmd.instrument_id;

        let client_ids: Vec<ClientOrderId> = cmd
            .cancels
            .iter()
            .map(|cancel| cancel.client_order_id)
            .collect();

        let venue_ids: Vec<VenueOrderId> = cmd
            .cancels
            .iter()
            .filter_map(|cancel| cancel.venue_order_id)
            .collect();

        let client_ids_opt = if client_ids.is_empty() {
            None
        } else {
            Some(client_ids)
        };

        let venue_ids_opt = if venue_ids.is_empty() {
            None
        } else {
            Some(venue_ids)
        };

        self.spawn_task("batch_cancel_orders", async move {
            match canceller
                .broadcast_batch_cancel(instrument_id, client_ids_opt, venue_ids_opt)
                .await
            {
                Ok(reports) => {
                    for report in &reports {
                        if let Some(cid) = &report.client_order_id {
                            dispatch_state.tombstone_order(cid);
                        }
                    }

                    for report in reports {
                        emitter.send_order_status_report(report);
                    }
                }
                Err(e) => log::error!("BitMEX batch cancel failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }
}

struct SubmitFailure<'a> {
    err: &'a anyhow::Error,
    ws_dispatch_state: &'a Arc<WsDispatchState>,
    emitter: &'a ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    post_only: bool,
}

fn handle_submit_failure(failure: &SubmitFailure<'_>) {
    let error_msg = failure.err.to_string();

    // A duplicate clOrdID can mean the original success response was lost
    if is_bitmex_duplicate_clordid_submit_failure(failure.err) {
        log::warn!(
            "Order {} may exist (duplicate clOrdID), \
             awaiting WebSocket confirmation",
            failure.client_order_id,
        );
        return;
    }

    if is_definitive_bitmex_submit_rejection(failure.err) {
        failure
            .ws_dispatch_state
            .order_identities
            .remove(&failure.client_order_id);
        let ts_event = failure.clock.get_time_ns();
        let rejection_reason = error_msg
            .strip_prefix(DEFINITIVE_SUBMIT_REJECTION)
            .map_or(error_msg.as_str(), |msg| {
                msg.trim_start_matches(':').trim_start()
            });
        failure.emitter.emit_order_rejected_event(
            failure.strategy_id,
            failure.instrument_id,
            failure.client_order_id,
            &format!("submit-order-error: {rejection_reason}"),
            ts_event,
            failure.post_only,
        );
    } else {
        log::warn!(
            "Ambiguous BitMEX submit failure for {}, awaiting reconciliation: {:?}",
            failure.client_order_id,
            failure.err,
        );
    }
}

struct ModifyFailure<'a> {
    err: &'a anyhow::Error,
    emitter: &'a ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
}

fn handle_modify_failure(failure: &ModifyFailure<'_>) {
    if is_definitive_bitmex_modify_rejection(failure.err) {
        let ts_event = failure.clock.get_time_ns();
        failure.emitter.emit_order_modify_rejected_event(
            failure.strategy_id,
            failure.instrument_id,
            failure.client_order_id,
            failure.venue_order_id,
            &format!("modify-order-error: {}", failure.err),
            ts_event,
        );
    } else {
        log::warn!(
            "Ambiguous BitMEX modify failure for {}, awaiting reconciliation: {:?}",
            failure.client_order_id,
            failure.err,
        );
    }
}

fn validate_order_for_bitmex_submit(
    order: &OrderAny,
    peg_price_type: Option<BitmexPegPriceType>,
    peg_offset_value: Option<f64>,
) -> anyhow::Result<()> {
    if order.order_side() == OrderSide::NoOrderSide {
        anyhow::bail!("Order side must be Buy or Sell");
    }

    BitmexOrderType::try_from_order_type(order.order_type())?;
    BitmexTimeInForce::try_from_time_in_force(order.time_in_force())?;

    let is_trailing_stop = matches!(
        order.order_type(),
        OrderType::TrailingStopMarket | OrderType::TrailingStopLimit
    );

    if is_trailing_stop
        && let Some(offset_type) = order.trailing_offset_type()
        && offset_type != TrailingOffsetType::Price
    {
        anyhow::bail!("BitMEX only supports PRICE trailing offset type, was {offset_type:?}");
    }

    if peg_price_type.is_none() && peg_offset_value.is_some() {
        anyhow::bail!("`peg_offset_value` requires `peg_price_type`");
    }

    if peg_price_type.is_some() && order.order_type() != OrderType::Limit {
        let order_type = order.order_type();
        anyhow::bail!("Pegged orders only supported for LIMIT order type, was {order_type:?}");
    }

    if let Some(contingency_type) = order.contingency_type() {
        BitmexContingencyType::try_from(contingency_type)?;
    }

    Ok(())
}

fn is_definitive_bitmex_submit_rejection(err: &anyhow::Error) -> bool {
    if is_bitmex_duplicate_clordid_submit_failure(err) {
        return false;
    }

    if has_bitmex_api_refusal(err) {
        return true;
    }

    let message = err.to_string();
    message.starts_with("Order rejected:") || message.starts_with(DEFINITIVE_SUBMIT_REJECTION)
}

fn is_bitmex_duplicate_clordid_submit_failure(err: &anyhow::Error) -> bool {
    if err.to_string().contains("IDEMPOTENT_DUPLICATE") {
        return true;
    }

    err.chain().any(|cause| {
        cause
            .downcast_ref::<BitmexHttpError>()
            .is_some_and(|e| {
                matches!(e, BitmexHttpError::BitmexError { message, .. } if message.contains("Duplicate clOrdID"))
            })
    })
}

fn is_definitive_bitmex_modify_rejection(err: &anyhow::Error) -> bool {
    if has_bitmex_api_refusal(err) {
        return true;
    }

    err.to_string().starts_with("Order modification rejected:")
}

fn has_bitmex_api_refusal(err: &anyhow::Error) -> bool {
    err.chain().any(|cause| {
        cause
            .downcast_ref::<BitmexHttpError>()
            .is_some_and(|e| matches!(e, BitmexHttpError::BitmexError { .. }))
    })
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        clients::ExecutionClient,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_core::{Params, UUID4};
    use nautilus_model::{
        enums::TimeInForce,
        events::OrderEventAny,
        identifiers::{Symbol, TraderId},
        instruments::crypto_perpetual::CryptoPerpetual,
        orders::builder::OrderTestBuilder,
        types::{Currency, Price, Quantity},
    };
    use nautilus_network::http::StatusCode;
    use rstest::rstest;

    use super::*;
    use crate::{
        common::{
            consts::{BITMEX_CLIENT_ID, BITMEX_VENUE},
            testing::load_test_json,
        },
        websocket::{
            enums::BitmexAction,
            messages::{BitmexExecutionMsg, BitmexTableMessage, BitmexWalletMsg, BitmexWsMessage},
        },
    };

    fn bitmex_api_error() -> anyhow::Error {
        anyhow::Error::new(BitmexHttpError::BitmexError {
            error_name: "HTTPError".to_string(),
            message: "Invalid price".to_string(),
        })
    }

    fn test_execution_client() -> (BitmexExecutionClient, Rc<RefCell<Cache>>) {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *BITMEX_CLIENT_ID,
            *BITMEX_VENUE,
            OmsType::Netting,
            AccountId::from("BITMEX-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let config = BitmexExecClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            base_url_http: Some("http://127.0.0.1:9/api/v1".to_string()),
            base_url_ws: Some("ws://127.0.0.1:9/realtime".to_string()),
            ..Default::default()
        };

        (BitmexExecutionClient::new(core, config).unwrap(), cache)
    }

    fn make_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            TraderId::from("TESTER-001"),
            AccountId::from("BITMEX-001"),
            AccountType::Margin,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn limit_order() -> OrderAny {
        limit_order_with_id(ClientOrderId::from("O-LIMIT"))
    }

    fn limit_order_with_id(client_order_id: ClientOrderId) -> OrderAny {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id(InstrumentId::from("XBTUSD.BITMEX"))
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("100.0"))
            .build()
    }

    fn test_perpetual_instrument() -> InstrumentAny {
        InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
            InstrumentId::from("XBTUSD.BITMEX"),
            Symbol::new("XBTUSD"),
            Currency::BTC(),
            Currency::USD(),
            Currency::BTC(),
            true,
            1,
            0,
            Price::new(0.5, 1),
            Quantity::new(1.0, 0),
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UnixNanos::default(),
            UnixNanos::default(),
        ))
    }

    fn market_order() -> OrderAny {
        let mut builder = OrderTestBuilder::new(OrderType::Market);
        builder
            .instrument_id(InstrumentId::from("XBTUSD.BITMEX"))
            .quantity(Quantity::from("1"))
            .build()
    }

    fn order_identity(order: &OrderAny) -> OrderIdentity {
        OrderIdentity {
            instrument_id: order.instrument_id(),
            strategy_id: order.strategy_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
        }
    }

    fn submit_command(order: &OrderAny, params: Option<Params>) -> SubmitOrder {
        SubmitOrder::new(
            order.trader_id(),
            Some(*BITMEX_CLIENT_ID),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.init_event().clone(),
            None,
            None,
            params,
            UUID4::new(),
            UnixNanos::default(),
            None,
        )
    }

    fn drain_order_events(
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) -> Vec<OrderEventAny> {
        let mut events = Vec::new();

        while let Ok(event) = rx.try_recv() {
            if let ExecutionEvent::Order(event) = event {
                events.push(event);
            }
        }
        events
    }

    fn dispatch_execution_fixture(
        state: &WsDispatchState,
        emitter: &ExecutionEventEmitter,
        account_id: AccountId,
    ) {
        let exec_msg: BitmexExecutionMsg =
            serde_json::from_str(&load_test_json("ws_execution.json")).unwrap();
        let mut instruments_by_symbol = AHashMap::new();
        instruments_by_symbol.insert(Ustr::from("XBTUSD"), test_perpetual_instrument());
        let mut order_type_cache = AHashMap::new();
        let mut order_symbol_cache = AHashMap::new();

        dispatch::dispatch_ws_message(
            UnixNanos::default(),
            BitmexWsMessage::Table(BitmexTableMessage::Execution {
                action: BitmexAction::Insert,
                data: vec![exec_msg],
            }),
            emitter,
            state,
            &mut instruments_by_symbol,
            &mut order_type_cache,
            &mut order_symbol_cache,
            account_id,
        );
    }

    #[rstest]
    fn test_bitmex_api_error_is_definitive_submit_rejection() {
        let err = bitmex_api_error();

        assert!(is_definitive_bitmex_submit_rejection(&err));
    }

    #[rstest]
    fn test_config_account_id_seeds_core_account_id() {
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *BITMEX_CLIENT_ID,
            *BITMEX_VENUE,
            OmsType::Netting,
            AccountId::from("BITMEX-001"),
            AccountType::Margin,
            None,
            cache,
        );
        let config = BitmexExecClientConfig {
            api_key: Some("test_key".to_string()),
            api_secret: Some("test_secret".to_string()),
            account_id: Some(AccountId::from("BITMEX-319111")),
            base_url_http: Some("http://127.0.0.1:9/api/v1".to_string()),
            base_url_ws: Some("ws://127.0.0.1:9/realtime".to_string()),
            ..Default::default()
        };

        let client = BitmexExecutionClient::new(core, config).unwrap();

        assert_eq!(client.account_id(), AccountId::from("BITMEX-319111"));
    }

    #[rstest]
    fn test_apply_account_id_updates_core_emitter_and_websocket_client() {
        let (mut client, _) = test_execution_client();
        let account_id = AccountId::from("BITMEX-319111");

        client.apply_account_id(account_id);

        assert_eq!(client.account_id(), account_id);
        assert_eq!(client.emitter.account_id(), account_id);
        assert_eq!(client.ws_client.account_id(), account_id);
    }

    #[rstest]
    fn test_dispatch_tracked_fill_uses_bitmex_account_id() {
        let (emitter, mut rx) = make_emitter();
        let state = WsDispatchState::default();
        let account_id = AccountId::from("BITMEX-1234567");
        let client_order_id = ClientOrderId::from("mm_bitmex_2b/oemUeQ4CAJZgP3fjHsB");
        state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id: InstrumentId::from("XBTUSD.BITMEX"),
                strategy_id: StrategyId::from("S-001"),
                order_side: OrderSide::Sell,
                order_type: OrderType::Limit,
            },
        );

        dispatch_execution_fixture(&state, &emitter, account_id);

        let events = drain_order_events(&mut rx);
        assert_eq!(events.len(), 2);
        match &events[..] {
            [
                OrderEventAny::Accepted(accepted),
                OrderEventAny::Filled(filled),
            ] => {
                assert_eq!(accepted.account_id, account_id);
                assert_eq!(filled.account_id, account_id);
            }
            events => panic!("expected accepted and filled events, was {events:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_untracked_fill_report_uses_bitmex_account_id() {
        let (emitter, mut rx) = make_emitter();
        let state = WsDispatchState::default();
        let account_id = AccountId::from("BITMEX-1234567");

        dispatch_execution_fixture(&state, &emitter, account_id);

        match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.account_id, account_id);
            }
            event => panic!("expected fill report, was {event:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_dispatch_wallet_account_state_uses_bitmex_account_id() {
        let (emitter, mut rx) = make_emitter();
        let state = WsDispatchState::default();
        let account_id = AccountId::from("BITMEX-1234567");
        let wallet_msg: BitmexWalletMsg =
            serde_json::from_str(&load_test_json("ws_wallet.json")).unwrap();
        let mut instruments_by_symbol = AHashMap::new();
        let mut order_type_cache = AHashMap::new();
        let mut order_symbol_cache = AHashMap::new();

        dispatch::dispatch_ws_message(
            UnixNanos::default(),
            BitmexWsMessage::Table(BitmexTableMessage::Wallet {
                action: BitmexAction::Insert,
                data: vec![wallet_msg],
            }),
            &emitter,
            &state,
            &mut instruments_by_symbol,
            &mut order_type_cache,
            &mut order_symbol_cache,
            account_id,
        );

        match rx.try_recv().unwrap() {
            ExecutionEvent::Account(state) => {
                assert_eq!(state.account_id, account_id);
            }
            event => panic!("expected account state, was {event:?}"),
        }
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_bitmex_api_error_is_definitive_modify_rejection() {
        let err = bitmex_api_error();

        assert!(is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_parsed_submit_reject_is_definitive_submit_rejection() {
        let err = anyhow::anyhow!("Order rejected: Price is invalid");

        assert!(is_definitive_bitmex_submit_rejection(&err));
        assert!(!is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_broadcast_submit_refusal_is_definitive_submit_rejection() {
        let err =
            anyhow::anyhow!("{DEFINITIVE_SUBMIT_REJECTION}: All submit requests were refused");

        assert!(is_definitive_bitmex_submit_rejection(&err));
        assert!(!is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_duplicate_clordid_is_ambiguous_submit_failure() {
        let err = anyhow::Error::new(BitmexHttpError::BitmexError {
            error_name: "HTTPError".to_string(),
            message: "Duplicate clOrdID".to_string(),
        });

        assert!(is_bitmex_duplicate_clordid_submit_failure(&err));
        assert!(!is_definitive_bitmex_submit_rejection(&err));
    }

    #[rstest]
    fn test_parsed_modify_reject_is_definitive_modify_rejection() {
        let err = anyhow::anyhow!("Order modification rejected: Price is invalid");

        assert!(is_definitive_bitmex_modify_rejection(&err));
        assert!(!is_definitive_bitmex_submit_rejection(&err));
    }

    #[rstest]
    fn test_network_error_is_ambiguous_command_failure() {
        let err = anyhow::Error::new(BitmexHttpError::NetworkError("timeout".to_string()));

        assert!(!is_definitive_bitmex_submit_rejection(&err));
        assert!(!is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_canceled_request_is_ambiguous_command_failure() {
        let err = anyhow::Error::new(BitmexHttpError::Canceled("shutdown".to_string()));

        assert!(!is_definitive_bitmex_submit_rejection(&err));
        assert!(!is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_unstructured_http_status_is_ambiguous_command_failure() {
        let err = anyhow::Error::new(BitmexHttpError::UnexpectedStatus {
            status: StatusCode::BAD_GATEWAY,
            body: "bad gateway".to_string(),
        });

        assert!(!is_definitive_bitmex_submit_rejection(&err));
        assert!(!is_definitive_bitmex_modify_rejection(&err));
    }

    #[rstest]
    fn test_validate_order_for_bitmex_submit_requires_peg_type_for_offset() {
        let order = limit_order();
        let err = validate_order_for_bitmex_submit(&order, None, Some(1.0)).unwrap_err();

        assert!(err.to_string().contains("`peg_offset_value` requires"));
    }

    #[rstest]
    fn test_validate_order_for_bitmex_submit_rejects_pegged_market_order() {
        let order = market_order();
        let err = validate_order_for_bitmex_submit(&order, Some(BitmexPegPriceType::LastPeg), None)
            .unwrap_err();

        assert!(err.to_string().contains("Pegged orders only supported"));
    }

    #[rstest]
    fn test_submit_order_invalid_peg_params_emits_denied_without_submitted() {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let order = limit_order_with_id(ClientOrderId::from("O-INVALID-PEG"));
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*BITMEX_CLIENT_ID), false)
            .unwrap();

        let mut params = Params::new();
        params.insert("peg_price_type".to_string(), serde_json::json!("BadPeg"));

        client
            .submit_order(submit_command(&order, Some(params)))
            .unwrap();

        let events = drain_order_events(&mut rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::Denied(denied) => {
                assert_eq!(denied.client_order_id, order.client_order_id());
                assert_eq!(denied.reason.to_string(), "Invalid peg_price_type: BadPeg");
            }
            event => panic!("expected OrderDenied event, was {event:?}"),
        }
        assert!(
            !client
                .ws_dispatch_state
                .order_identities
                .contains_key(&order.client_order_id())
        );
    }

    #[rstest]
    fn test_submit_order_gtd_time_in_force_emits_denied_without_submitted() {
        let (mut client, cache) = test_execution_client();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        client.emitter.set_sender(tx);

        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        let order = builder
            .instrument_id(InstrumentId::from("XBTUSD.BITMEX"))
            .client_order_id(ClientOrderId::from("O-GTD"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1"))
            .price(Price::from("100.0"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(1_000_000_000_u64))
            .build();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(*BITMEX_CLIENT_ID), false)
            .unwrap();

        client.submit_order(submit_command(&order, None)).unwrap();

        let events = drain_order_events(&mut rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::Denied(denied) => {
                assert_eq!(denied.client_order_id, order.client_order_id());
                assert!(
                    denied
                        .reason
                        .to_string()
                        .contains("GTD time in force is not supported")
                );
            }
            event => panic!("expected OrderDenied event, was {event:?}"),
        }
        assert!(
            !client
                .ws_dispatch_state
                .order_identities
                .contains_key(&order.client_order_id())
        );
    }

    #[rstest]
    fn test_submit_failure_definitive_refusal_removes_identity_and_emits_rejected() {
        let (emitter, mut rx) = make_emitter();
        let ws_dispatch_state = Arc::new(WsDispatchState::default());
        let order = limit_order_with_id(ClientOrderId::from("O-SUBMIT-REJECTED"));
        ws_dispatch_state
            .order_identities
            .insert(order.client_order_id(), order_identity(&order));

        let err = anyhow::anyhow!(
            "{DEFINITIVE_SUBMIT_REJECTION}: All submit requests were refused by BitMEX"
        );

        handle_submit_failure(&SubmitFailure {
            err: &err,
            ws_dispatch_state: &ws_dispatch_state,
            emitter: &emitter,
            clock: get_atomic_clock_realtime(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            post_only: false,
        });

        assert!(
            !ws_dispatch_state
                .order_identities
                .contains_key(&order.client_order_id())
        );

        let events = drain_order_events(&mut rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::Rejected(rejected) => {
                assert_eq!(rejected.client_order_id, order.client_order_id());
                assert_eq!(
                    rejected.reason.to_string(),
                    "submit-order-error: All submit requests were refused by BitMEX"
                );
                assert!(!rejected.due_post_only);
            }
            event => panic!("expected OrderRejected event, was {event:?}"),
        }
    }

    #[rstest]
    fn test_submit_failure_duplicate_clordid_keeps_identity_and_emits_no_rejection() {
        let (emitter, mut rx) = make_emitter();
        let ws_dispatch_state = Arc::new(WsDispatchState::default());
        let order = limit_order_with_id(ClientOrderId::from("O-DUPLICATE"));
        ws_dispatch_state
            .order_identities
            .insert(order.client_order_id(), order_identity(&order));
        let err = anyhow::Error::new(BitmexHttpError::BitmexError {
            error_name: "HTTPError".to_string(),
            message: "Duplicate clOrdID".to_string(),
        });

        handle_submit_failure(&SubmitFailure {
            err: &err,
            ws_dispatch_state: &ws_dispatch_state,
            emitter: &emitter,
            clock: get_atomic_clock_realtime(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            post_only: false,
        });

        assert!(
            ws_dispatch_state
                .order_identities
                .contains_key(&order.client_order_id())
        );
        assert!(drain_order_events(&mut rx).is_empty());
    }

    #[rstest]
    fn test_submit_failure_network_error_keeps_identity_and_emits_no_rejection() {
        let (emitter, mut rx) = make_emitter();
        let ws_dispatch_state = Arc::new(WsDispatchState::default());
        let order = limit_order_with_id(ClientOrderId::from("O-SUBMIT-NETWORK"));
        ws_dispatch_state
            .order_identities
            .insert(order.client_order_id(), order_identity(&order));
        let err = anyhow::Error::new(BitmexHttpError::NetworkError("timeout".to_string()));

        handle_submit_failure(&SubmitFailure {
            err: &err,
            ws_dispatch_state: &ws_dispatch_state,
            emitter: &emitter,
            clock: get_atomic_clock_realtime(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            post_only: false,
        });

        assert!(
            ws_dispatch_state
                .order_identities
                .contains_key(&order.client_order_id())
        );
        assert!(drain_order_events(&mut rx).is_empty());
    }

    #[rstest]
    fn test_modify_failure_definitive_refusal_emits_modify_rejected() {
        let (emitter, mut rx) = make_emitter();
        let order = limit_order_with_id(ClientOrderId::from("O-MODIFY-REJECTED"));
        let venue_order_id = Some(VenueOrderId::from("V-001"));
        let err = bitmex_api_error();

        handle_modify_failure(&ModifyFailure {
            err: &err,
            emitter: &emitter,
            clock: get_atomic_clock_realtime(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            venue_order_id,
        });

        let events = drain_order_events(&mut rx);
        assert_eq!(events.len(), 1);
        match &events[0] {
            OrderEventAny::ModifyRejected(rejected) => {
                assert_eq!(rejected.client_order_id, order.client_order_id());
                assert_eq!(rejected.venue_order_id, venue_order_id);
                assert_eq!(
                    rejected.reason.to_string(),
                    "modify-order-error: BitMEX error HTTPError: Invalid price"
                );
            }
            event => panic!("expected OrderModifyRejected event, was {event:?}"),
        }
    }

    #[rstest]
    fn test_modify_failure_network_error_emits_no_modify_rejected() {
        let (emitter, mut rx) = make_emitter();
        let order = limit_order_with_id(ClientOrderId::from("O-MODIFY-NETWORK"));
        let err = anyhow::Error::new(BitmexHttpError::NetworkError("timeout".to_string()));

        handle_modify_failure(&ModifyFailure {
            err: &err,
            emitter: &emitter,
            clock: get_atomic_clock_realtime(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id: order.client_order_id(),
            venue_order_id: None,
        });

        assert!(drain_order_events(&mut rx).is_empty());
    }
}
