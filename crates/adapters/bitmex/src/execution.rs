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
    enums::{AccountType, OmsType, OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{AccountId, ClientId, ClientOrderId, Venue, VenueOrderId},
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
        submitter::{SubmitBroadcaster, SubmitBroadcasterConfig},
    },
    common::{
        enums::{BitmexExecType, BitmexOrderType, BitmexPegPriceType},
        parse::{parse_peg_offset_value, parse_peg_price_type},
    },
    config::BitmexExecClientConfig,
    http::{
        client::BitmexHttpClient,
        parse::{InstrumentParseResult, parse_instrument_any},
    },
    websocket::{
        client::BitmexWebSocketClient,
        enums::BitmexAction,
        messages::{BitmexTableMessage, BitmexWsMessage, OrderData},
        parse::{
            parse_execution_msg, parse_order_msg, parse_order_update_msg, parse_position_msg,
            parse_wallet_msg,
        },
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
    pub fn new(core: ExecutionClientCore, config: BitmexExecClientConfig) -> anyhow::Result<Self> {
        if !config.has_api_credentials() {
            anyhow::bail!("BitMEX execution client requires API key and secret");
        }

        let trader_id = core.trader_id;
        let account_id = config.account_id.unwrap_or(core.account_id);
        let clock = get_atomic_clock_realtime();
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let http_client = BitmexHttpClient::new(
            Some(config.http_base_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.use_testnet,
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.recv_window_ms,
            config.max_requests_per_second,
            config.max_requests_per_minute,
            config.http_proxy_url.clone(),
        )
        .context("failed to construct BitMEX HTTP client")?;
        let ws_client = BitmexWebSocketClient::new_with_env(
            Some(config.ws_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            Some(account_id),
            config.heartbeat_interval_secs,
            config.use_testnet,
        )
        .context("failed to construct BitMEX execution websocket client")?;

        let pool_size = config.submitter_pool_size.unwrap_or(1);
        let submitter_proxy_urls = match &config.submitter_proxy_urls {
            Some(urls) => urls.iter().map(|url| Some(url.clone())).collect(),
            None => vec![config.http_proxy_url.clone(); pool_size],
        };

        let submitter_config = SubmitBroadcasterConfig {
            pool_size,
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            base_url: config.base_url_http.clone(),
            testnet: config.use_testnet,
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
            None => vec![config.http_proxy_url.clone(); canceller_pool_size],
        };

        let canceller_config = CancelBroadcasterConfig {
            pool_size: canceller_pool_size,
            api_key: config.api_key.clone(),
            api_secret: config.api_secret.clone(),
            base_url: config.base_url_http.clone(),
            testnet: config.use_testnet,
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

    async fn ensure_instruments_initialized_async(&mut self) -> anyhow::Result<()> {
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

        for instrument in &instruments {
            self.http_client.cache_instrument(instrument.clone());
            self._submitter.cache_instrument(instrument.clone());
            self._canceller.cache_instrument(instrument.clone());
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request BitMEX account state")?;

        self.emitter.send_account_state(account_state);
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

    fn start_ws_stream(&mut self) {
        if self.ws_stream_handle.is_some() {
            return;
        }

        let stream = self.ws_client.stream();
        let emitter = self.emitter.clone();
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
            for entry in self.http_client.instruments_cache.iter() {
                instruments_by_symbol.insert(*entry.key(), entry.value().clone());
            }
        }

        let handle = get_runtime().spawn(async move {
            pin_mut!(stream);
            let mut order_type_cache: AHashMap<ClientOrderId, OrderType> = AHashMap::new();
            let mut order_symbol_cache: AHashMap<ClientOrderId, Ustr> = AHashMap::new();
            let mut insts_by_symbol = instruments_by_symbol;

            while let Some(message) = stream.next().await {
                dispatch_ws_message(
                    clock.get_time_ns(),
                    message,
                    &emitter,
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
        order: OrderAny,
        submit_tries: Option<usize>,
        peg_price_type: Option<BitmexPegPriceType>,
        peg_offset_value: Option<f64>,
        task_label: &'static str,
    ) {
        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return;
        }

        self.emitter.emit_order_submitted(&order);

        let use_broadcaster = submit_tries.is_some_and(|n| n > 1);
        let http_client = self.http_client.clone();
        let submitter = self._submitter.clone_for_async();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let client_order_id = order.client_order_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
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
                Ok(report) => emitter.send_order_status_report(report),
                Err(e) => {
                    let error_msg = e.to_string();

                    // If all transports returned "Duplicate clOrdID", the order likely exists
                    // but the success response was lost. Wait for WebSocket confirmation.
                    if error_msg.contains("IDEMPOTENT_DUPLICATE") {
                        log::warn!(
                            "Order {client_order_id} may exist (duplicate clOrdID from all transports), \
                             awaiting WebSocket confirmation",
                        );
                        return Ok(());
                    }

                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        &format!("submit-order-error: {error_msg}"),
                        ts_event,
                        post_only,
                    );
                }
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
            "BitMEX execution client started: client_id={}, account_id={}, use_testnet={}, submitter_pool_size={:?}, canceller_pool_size={:?}, http_proxy_url={:?}, ws_proxy_url={:?}, submitter_proxy_urls={:?}, canceller_proxy_urls={:?}",
            self.core.client_id,
            self.core.account_id,
            self.config.use_testnet,
            self.config.submitter_pool_size,
            self.config.canceller_pool_size,
            self.config.http_proxy_url,
            self.config.ws_proxy_url,
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
        self.refresh_account_state().await?;
        self.await_account_registered(30.0).await?;

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

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
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

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
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

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let submit_tries = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        let peg_price_type = parse_peg_price_type(cmd.params.as_ref())?;
        let peg_offset_value = parse_peg_offset_value(cmd.params.as_ref())?;

        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        self.submit_cached_order(
            order,
            submit_tries,
            peg_price_type,
            peg_offset_value,
            "submit_order",
        );
        Ok(())
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        if cmd.order_list.client_order_ids.is_empty() {
            log::debug!("submit_order_list called with empty order list");
            return Ok(());
        }

        let submit_tries = cmd
            .params
            .as_ref()
            .and_then(|p| p.get_usize("submit_tries"))
            .filter(|&n| n > 0);

        let peg_price_type = parse_peg_price_type(cmd.params.as_ref())?;
        let peg_offset_value = parse_peg_offset_value(cmd.params.as_ref())?;

        let orders = self.core.get_orders_for_list(&cmd.order_list)?;

        log::info!(
            "Submitting BitMEX order list: order_list_id={}, count={}",
            cmd.order_list.id,
            orders.len(),
        );

        for order in orders {
            self.submit_cached_order(
                order,
                submit_tries,
                peg_price_type,
                peg_offset_value,
                "submit_order_list_item",
            );
        }

        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = cmd.venue_order_id;
        let quantity = cmd.quantity;
        let price = cmd.price;
        let trigger_price = cmd.trigger_price;

        self.spawn_task("modify_order", async move {
            match http_client
                .modify_order(
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    quantity,
                    price,
                    trigger_price,
                )
                .await
            {
                Ok(report) => emitter.send_order_status_report(report),
                Err(e) => log::error!("BitMEX modify order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
        let instrument_id = cmd.instrument_id;
        let client_order_id = Some(cmd.client_order_id);
        let venue_order_id = cmd.venue_order_id;

        self.spawn_task("cancel_order", async move {
            match canceller
                .broadcast_cancel(instrument_id, client_order_id, venue_order_id)
                .await
            {
                Ok(Some(report)) => emitter.send_order_status_report(report),
                Ok(None) => {
                    // Idempotent success - order already cancelled
                    log::debug!("Order already cancelled: {client_order_id:?}");
                }
                Err(e) => log::error!("BitMEX cancel order failed: {e:?}"),
            }
            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
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

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        let canceller = self._canceller.clone_for_async();
        let emitter = self.emitter.clone();
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

/// Dispatches a venue WebSocket message by parsing it and routing to the event emitter.
#[allow(clippy::too_many_arguments)]
fn dispatch_ws_message(
    ts_init: UnixNanos,
    message: BitmexWsMessage,
    emitter: &ExecutionEventEmitter,
    instruments_by_symbol: &mut AHashMap<Ustr, InstrumentAny>,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    order_symbol_cache: &mut AHashMap<ClientOrderId, Ustr>,
    account_id: AccountId,
) {
    match message {
        BitmexWsMessage::Table(table_msg) => {
            match table_msg {
                BitmexTableMessage::Order { data, .. } => {
                    handle_order_messages(
                        data,
                        emitter,
                        instruments_by_symbol,
                        order_type_cache,
                        order_symbol_cache,
                        account_id,
                        ts_init,
                    );
                }
                BitmexTableMessage::Execution { data, .. } => {
                    handle_execution_messages(
                        data,
                        emitter,
                        instruments_by_symbol,
                        order_symbol_cache,
                        ts_init,
                    );
                }
                BitmexTableMessage::Position { data, .. } => {
                    for pos_msg in data {
                        let Some(instrument) = instruments_by_symbol.get(&pos_msg.symbol) else {
                            log::error!(
                                "Instrument cache miss: position dropped for symbol={}, account={}",
                                pos_msg.symbol,
                                pos_msg.account,
                            );
                            continue;
                        };
                        let report = parse_position_msg(pos_msg, instrument, ts_init);
                        emitter.send_position_report(report);
                    }
                }
                BitmexTableMessage::Wallet { data, .. } => {
                    for wallet_msg in data {
                        let state = parse_wallet_msg(wallet_msg, ts_init);
                        emitter.send_account_state(state);
                    }
                }
                BitmexTableMessage::Margin { .. } => {
                    // Skip margin messages - BitMEX uses account-level cross-margin
                    // which doesn't map well to Nautilus's per-instrument margin model
                }
                BitmexTableMessage::Instrument { action, data } => {
                    if matches!(action, BitmexAction::Partial | BitmexAction::Insert) {
                        for msg in data {
                            match msg.try_into() {
                                Ok(http_inst) => match parse_instrument_any(&http_inst, ts_init) {
                                    InstrumentParseResult::Ok(boxed) => {
                                        let inst = *boxed;
                                        let symbol = inst.symbol().inner();
                                        instruments_by_symbol.insert(symbol, inst);
                                    }
                                    InstrumentParseResult::Unsupported { .. }
                                    | InstrumentParseResult::Inactive { .. } => {}
                                    InstrumentParseResult::Failed { symbol, error, .. } => {
                                        log::warn!("Failed to parse instrument {symbol}: {error}");
                                    }
                                },
                                Err(e) => {
                                    log::debug!(
                                        "Skipping instrument (missing required fields): {e}"
                                    );
                                }
                            }
                        }
                    }
                }
                // Ignore data-only tables on execution client
                BitmexTableMessage::OrderBookL2 { .. }
                | BitmexTableMessage::OrderBookL2_25 { .. }
                | BitmexTableMessage::OrderBook10 { .. }
                | BitmexTableMessage::Quote { .. }
                | BitmexTableMessage::Trade { .. }
                | BitmexTableMessage::TradeBin1m { .. }
                | BitmexTableMessage::TradeBin5m { .. }
                | BitmexTableMessage::TradeBin1h { .. }
                | BitmexTableMessage::TradeBin1d { .. }
                | BitmexTableMessage::Funding { .. } => {
                    log::debug!("Ignoring BitMEX data message on execution stream");
                }
                _ => {
                    log::warn!("Unhandled table message type on execution stream");
                }
            }
        }
        BitmexWsMessage::Reconnected => {
            order_type_cache.clear();
            order_symbol_cache.clear();
            log::info!("BitMEX execution websocket reconnected");
        }
        BitmexWsMessage::Authenticated => {
            log::debug!("BitMEX execution websocket authenticated");
        }
    }
}

fn handle_order_messages(
    data: Vec<OrderData>,
    emitter: &ExecutionEventEmitter,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    order_type_cache: &mut AHashMap<ClientOrderId, OrderType>,
    order_symbol_cache: &mut AHashMap<ClientOrderId, Ustr>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    for order_data in data {
        match order_data {
            OrderData::Full(order_msg) => {
                let Some(instrument) = instruments_by_symbol.get(&order_msg.symbol) else {
                    log::error!(
                        "Instrument cache miss: order dropped for symbol={}, order_id={}",
                        order_msg.symbol,
                        order_msg.order_id,
                    );
                    continue;
                };

                match parse_order_msg(&order_msg, instrument, order_type_cache, ts_init) {
                    Ok(report) => {
                        if let Some(client_order_id) = &order_msg.cl_ord_id {
                            let client_order_id = ClientOrderId::new(client_order_id);

                            if let Some(ord_type) = &order_msg.ord_type {
                                let order_type: OrderType = if *ord_type == BitmexOrderType::Pegged
                                    && order_msg.peg_price_type
                                        == Some(BitmexPegPriceType::TrailingStopPeg)
                                {
                                    if order_msg.price.is_some() {
                                        OrderType::TrailingStopLimit
                                    } else {
                                        OrderType::TrailingStopMarket
                                    }
                                } else {
                                    (*ord_type).into()
                                };
                                order_type_cache.insert(client_order_id, order_type);
                            }

                            order_symbol_cache.insert(client_order_id, order_msg.symbol);
                        }

                        if report.order_status.is_closed()
                            && let Some(client_id) = report.client_order_id
                        {
                            order_type_cache.remove(&client_id);
                            order_symbol_cache.remove(&client_id);
                        }

                        emitter.send_order_status_report(report);
                    }
                    Err(e) => {
                        log::error!(
                            "Failed to parse full order message: \
                            error={e}, symbol={}, order_id={}, time_in_force={:?}",
                            order_msg.symbol,
                            order_msg.order_id,
                            order_msg.time_in_force,
                        );
                    }
                }
            }
            OrderData::Update(msg) => {
                let Some(instrument) = instruments_by_symbol.get(&msg.symbol) else {
                    log::error!(
                        "Instrument cache miss: order update dropped for symbol={}, order_id={}",
                        msg.symbol,
                        msg.order_id,
                    );
                    continue;
                };

                // Populate cache for execution message routing
                if let Some(cl_ord_id) = &msg.cl_ord_id {
                    let client_order_id = ClientOrderId::new(cl_ord_id);
                    order_symbol_cache.insert(client_order_id, msg.symbol);
                }

                if let Some(event) = parse_order_update_msg(&msg, instrument, account_id, ts_init) {
                    emitter.send_order_event(OrderEventAny::Updated(event));
                } else {
                    log::warn!(
                        "Skipped order update (insufficient data): order_id={}, price={:?}",
                        msg.order_id,
                        msg.price,
                    );
                }
            }
        }
    }
}

fn handle_execution_messages(
    data: Vec<crate::websocket::messages::BitmexExecutionMsg>,
    emitter: &ExecutionEventEmitter,
    instruments_by_symbol: &AHashMap<Ustr, InstrumentAny>,
    order_symbol_cache: &AHashMap<ClientOrderId, Ustr>,
    ts_init: UnixNanos,
) {
    for exec_msg in data {
        let symbol_opt = if let Some(sym) = &exec_msg.symbol {
            Some(*sym)
        } else if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
            let client_order_id = ClientOrderId::new(cl_ord_id);
            order_symbol_cache.get(&client_order_id).copied()
        } else {
            None
        };

        let Some(symbol) = symbol_opt else {
            if let Some(cl_ord_id) = &exec_msg.cl_ord_id {
                if exec_msg.exec_type == Some(BitmexExecType::Trade) {
                    log::warn!(
                        "Execution missing symbol and not in cache: \
                        cl_ord_id={cl_ord_id}, exec_id={:?}",
                        exec_msg.exec_id,
                    );
                } else {
                    log::debug!(
                        "Execution missing symbol and not in cache: \
                        cl_ord_id={cl_ord_id}, exec_type={:?}",
                        exec_msg.exec_type,
                    );
                }
            } else if exec_msg.exec_type == Some(BitmexExecType::CancelReject) {
                log::debug!(
                    "CancelReject missing symbol/clOrdID (expected with redundant cancels): \
                    exec_id={:?}, order_id={:?}",
                    exec_msg.exec_id,
                    exec_msg.order_id,
                );
            } else {
                log::warn!(
                    "Execution missing both symbol and clOrdID: \
                    exec_id={:?}, order_id={:?}, exec_type={:?}",
                    exec_msg.exec_id,
                    exec_msg.order_id,
                    exec_msg.exec_type,
                );
            }
            continue;
        };

        let Some(instrument) = instruments_by_symbol.get(&symbol) else {
            log::error!(
                "Instrument cache miss: execution dropped for symbol={}, exec_id={:?}, exec_type={:?}",
                symbol,
                exec_msg.exec_id,
                exec_msg.exec_type,
            );
            continue;
        };

        if let Some(fill) = parse_execution_msg(exec_msg, instrument, ts_init) {
            emitter.send_fill_report(fill);
        }
    }
}
