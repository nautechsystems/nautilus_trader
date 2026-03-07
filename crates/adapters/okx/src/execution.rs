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

//! Live execution client implementation for the OKX adapter.

use std::{
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use dashmap::DashSet;
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
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderStatus, OrderType, TrailingOffsetType},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::{OKX_CONDITIONAL_ORDER_TYPES, OKX_VENUE},
        enums::{OKXInstrumentType, OKXMarginMode, OKXTradeMode, is_advance_algo_order},
    },
    config::OKXExecClientConfig,
    http::{client::OKXHttpClient, models::OKXCancelAlgoOrderRequest},
    websocket::{
        client::OKXWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage},
    },
};

/// Maximum entries in the dedup sets before they are cleared.
const DEDUP_CAPACITY: usize = 10_000;

/// Shared state for cross-stream event deduplication between the private
/// and business WebSocket dispatch loops.
#[doc(hidden)]
#[derive(Debug)]
pub struct WsDispatchState {
    pub filled_orders: DashSet<ClientOrderId>,
    pub triggered_orders: DashSet<ClientOrderId>,
    clearing: AtomicBool,
}

impl Default for WsDispatchState {
    fn default() -> Self {
        Self {
            filled_orders: DashSet::default(),
            triggered_orders: DashSet::default(),
            clearing: AtomicBool::new(false),
        }
    }
}

impl WsDispatchState {
    fn evict_if_full(&self, set: &DashSet<ClientOrderId>) {
        if set.len() >= DEDUP_CAPACITY
            && self
                .clearing
                .compare_exchange(false, true, Ordering::AcqRel, Ordering::Relaxed)
                .is_ok()
        {
            set.clear();
            self.clearing.store(false, Ordering::Release);
        }
    }

    fn insert_filled(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.filled_orders);
        self.filled_orders.insert(cid);
    }

    fn insert_triggered(&self, cid: ClientOrderId) {
        self.evict_if_full(&self.triggered_orders);
        self.triggered_orders.insert(cid);
    }
}

#[derive(Debug)]
pub struct OKXExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: OKXExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: OKXHttpClient,
    ws_private: OKXWebSocketClient,
    ws_business: OKXWebSocketClient,
    trade_mode: OKXTradeMode,
    ws_stream_handle: Option<JoinHandle<()>>,
    ws_business_stream_handle: Option<JoinHandle<()>>,
    ws_dispatch_state: Arc<WsDispatchState>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl OKXExecutionClient {
    /// Creates a new [`OKXExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: OKXExecClientConfig) -> anyhow::Result<Self> {
        // Always use with_credentials which loads from env vars when config values are None
        let http_client = OKXHttpClient::with_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            config.base_url_http.clone(),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.is_demo,
            config.http_proxy_url.clone(),
        )?;

        let account_id = core.account_id;

        let ws_private = OKXWebSocketClient::with_credentials(
            Some(config.ws_private_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            Some(account_id),
            Some(20), // Heartbeat
        )
        .context("failed to construct OKX private websocket client")?;

        let ws_business = OKXWebSocketClient::with_credentials(
            Some(config.ws_business_url()),
            config.api_key.clone(),
            config.api_secret.clone(),
            config.api_passphrase.clone(),
            Some(account_id),
            Some(20), // Heartbeat
        )
        .context("failed to construct OKX business websocket client")?;

        let trade_mode = Self::derive_trade_mode(core.account_type, &config);
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            None,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_private,
            ws_business,
            trade_mode,
            ws_stream_handle: None,
            ws_business_stream_handle: None,
            ws_dispatch_state: Arc::new(WsDispatchState::default()),
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    fn derive_trade_mode(account_type: AccountType, config: &OKXExecClientConfig) -> OKXTradeMode {
        let is_cross_margin = config.margin_mode == Some(OKXMarginMode::Cross);

        if account_type == AccountType::Cash {
            if !config.use_spot_margin {
                return OKXTradeMode::Cash;
            }
            return if is_cross_margin {
                OKXTradeMode::Cross
            } else {
                OKXTradeMode::Isolated
            };
        }

        if is_cross_margin {
            OKXTradeMode::Cross
        } else {
            OKXTradeMode::Isolated
        }
    }

    fn instrument_types(&self) -> Vec<OKXInstrumentType> {
        if self.config.instrument_types.is_empty() {
            vec![OKXInstrumentType::Spot]
        } else {
            self.config.instrument_types.clone()
        }
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request OKX account state")?;

        self.emitter.send_account_state(account_state);
        Ok(())
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }

    fn is_conditional_order(&self, order_type: OrderType) -> bool {
        OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
    }

    fn submit_regular_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache
                .order(&cmd.client_order_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?
        };
        let ws_private = self.ws_private.clone();
        let trade_mode = self.trade_mode;

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let trader_id = self.core.trader_id;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let time_in_force = order.time_in_force();
        let price = order.price();
        let trigger_price = order.trigger_price();
        let is_post_only = order.is_post_only();
        let is_reduce_only = order.is_reduce_only();
        let is_quote_quantity = order.is_quote_quantity();

        self.spawn_task("submit_order", async move {
            let result = ws_private
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    trade_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    Some(time_in_force),
                    price,
                    trigger_price,
                    Some(is_post_only),
                    Some(is_reduce_only),
                    Some(is_quote_quantity),
                    None,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit order failed: {e}"));

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
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_conditional_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = {
            let cache = self.core.cache();
            cache
                .order(&cmd.client_order_id)
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?
        };
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode;

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let trigger_type = order.trigger_type();
        let trigger_price = order.trigger_price();
        let price = order.price();
        let is_reduce_only = order.is_reduce_only();

        let trailing_offset = order.trailing_offset();
        let trailing_offset_type = order.trailing_offset_type();
        let activation_price = order.activation_price();

        let (callback_ratio, callback_spread) = if order_type == OrderType::TrailingStopMarket {
            let offset = trailing_offset
                .ok_or_else(|| anyhow::anyhow!("TrailingStopMarket requires trailing_offset"))?;
            let offset_type = trailing_offset_type.ok_or_else(|| {
                anyhow::anyhow!("TrailingStopMarket requires trailing_offset_type")
            })?;
            match offset_type {
                TrailingOffsetType::BasisPoints => {
                    // Convert basis points to ratio (e.g., 100 bps = 0.01)
                    let ratio = offset / Decimal::from(10000);
                    (Some(ratio.to_string()), None)
                }
                TrailingOffsetType::Price => (None, Some(offset.to_string())),
                _ => {
                    anyhow::bail!("Unsupported trailing_offset_type for OKX: {offset_type:?}");
                }
            }
        } else {
            (None, None)
        };

        self.spawn_task("submit_algo_order", async move {
            let result = http_client
                .place_algo_order_with_domain_types(
                    instrument_id,
                    trade_mode,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    trigger_price,
                    trigger_type,
                    price,
                    Some(is_reduce_only),
                    callback_ratio,
                    callback_spread,
                    activation_price,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit algo order failed: {e}"));

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
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_ws_order(&self, cmd: &CancelOrder) {
        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            let result = ws_private
                .cancel_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.venue_order_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Cancel order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
            }

            Ok(())
        });
    }

    fn cancel_algo_order(&self, cmd: &CancelOrder) {
        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let cache = self.core.cache();
        let is_advance = cache
            .order(&cmd.client_order_id)
            .is_some_and(|o| is_advance_algo_order(o.order_type()));
        drop(cache);

        let request = OKXCancelAlgoOrderRequest {
            inst_id: cmd.instrument_id.symbol.to_string(),
            inst_id_code: None,
            algo_id: cmd.venue_order_id.map(|id| id.to_string()),
            algo_cl_ord_id: if cmd.venue_order_id.is_none() {
                Some(cmd.client_order_id.to_string())
            } else {
                None
            },
        };

        self.spawn_task("cancel_algo_order", async move {
            let responses = if is_advance {
                http_client
                    .cancel_advance_algo_orders(vec![request])
                    .await
                    .map_err(|e| anyhow::anyhow!("Cancel advance algo order failed: {e}"))
            } else {
                http_client
                    .cancel_algo_orders(vec![request])
                    .await
                    .map_err(|e| anyhow::anyhow!("Cancel algo order failed: {e}"))
            };

            let reject_reason = match &responses {
                Err(e) => Some(format!("cancel-algo-order-error: {e}")),
                Ok(resps) => {
                    // Check per-order business status code
                    resps.first().and_then(|r| {
                        r.s_code.as_deref().and_then(|code| {
                            if code == "0" {
                                None
                            } else {
                                let msg = r.s_msg.as_deref().unwrap_or("unknown");
                                Some(format!(
                                    "cancel-algo-order-rejected: s_code={code}, s_msg={msg}"
                                ))
                            }
                        })
                    })
                }
            };

            if let Some(reason) = reject_reason {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &reason,
                    ts_event,
                );
                anyhow::bail!("{reason}");
            }

            Ok(())
        });
    }

    fn mass_cancel_instrument(&self, instrument_id: InstrumentId) {
        let ws_private = self.ws_private.clone();
        self.spawn_task("mass_cancel_orders", async move {
            ws_private.mass_cancel_orders(instrument_id).await?;
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
impl ExecutionClient for OKXExecutionClient {
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
        *OKX_VENUE
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

        let instrument_types = self.instrument_types();

        if !self.core.instruments_initialized() {
            let mut all_instruments = Vec::new();
            let mut all_inst_id_codes = Vec::new();

            for instrument_type in &instrument_types {
                let (instruments, inst_id_codes) = self
                    .http_client
                    .request_instruments(*instrument_type, None)
                    .await
                    .with_context(|| {
                        format!("failed to request OKX instruments for {instrument_type:?}")
                    })?;

                if instruments.is_empty() {
                    log::warn!("No instruments returned for {instrument_type:?}");
                    continue;
                }

                log::info!(
                    "Loaded {} {instrument_type:?} instruments",
                    instruments.len()
                );

                self.http_client.cache_instruments(instruments.clone());
                all_instruments.extend(instruments);
                all_inst_id_codes.extend(inst_id_codes);
            }

            if all_instruments.is_empty() {
                anyhow::bail!(
                    "No instruments loaded for configured types {instrument_types:?}, \
                     cannot initialize execution client"
                );
            }

            self.ws_private.cache_instruments(all_instruments.clone());
            self.ws_private
                .cache_inst_id_codes(all_inst_id_codes.clone());
            self.ws_business.cache_instruments(all_instruments);
            self.ws_business.cache_inst_id_codes(all_inst_id_codes);
            self.core.set_instruments_initialized();
        }

        self.ws_private.connect().await?;
        self.ws_private.wait_until_active(10.0).await?;
        log::info!("Connected to private WebSocket");

        if self.ws_stream_handle.is_none() {
            let stream = self.ws_private.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &emitter, &state);
                }
            });
            self.ws_stream_handle = Some(handle);
        }

        self.ws_business.connect().await?;
        self.ws_business.wait_until_active(10.0).await?;
        log::info!("Connected to business WebSocket");

        if self.ws_business_stream_handle.is_none() {
            let stream = self.ws_business.stream();
            let emitter = self.emitter.clone();
            let state = Arc::clone(&self.ws_dispatch_state);
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &emitter, &state);
                }
            });
            self.ws_business_stream_handle = Some(handle);
        }

        for inst_type in &instrument_types {
            log::info!("Subscribing to orders channel for {inst_type:?}");
            self.ws_private.subscribe_orders(*inst_type).await?;

            if self.config.use_fills_channel {
                log::info!("Subscribing to fills channel for {inst_type:?}");
                if let Err(e) = self.ws_private.subscribe_fills(*inst_type).await {
                    log::warn!("Failed to subscribe to fills channel ({inst_type:?}): {e}");
                }
            }
        }

        self.ws_private.subscribe_account().await?;

        // Subscribe to algo orders on business WebSocket (OKX requires this endpoint)
        for inst_type in &instrument_types {
            if *inst_type != OKXInstrumentType::Option {
                self.ws_business.subscribe_orders_algo(*inst_type).await?;
                self.ws_business.subscribe_algo_advance(*inst_type).await?;
            }
        }

        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request OKX account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

        // Wait for account to be registered in cache before completing connect
        self.await_account_registered(30.0).await?;

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

        if let Err(e) = self.ws_private.close().await {
            log::warn!("Error closing private websocket: {e:?}");
        }

        if let Err(e) = self.ws_business.close().await {
            log::warn!("Error closing business websocket: {e:?}");
        }

        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.ws_business_stream_handle.take() {
            handle.abort();
        }

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        self.update_account_state()
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log::debug!(
            "query_order not implemented for OKX execution client (client_order_id={})",
            cmd.client_order_id
        );
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

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        // Spawn instrument bootstrap task
        let http_client = self.http_client.clone();
        let ws_private = self.ws_private.clone();
        let ws_business = self.ws_business.clone();
        let instrument_types = self.config.instrument_types.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            let mut all_inst_id_codes = Vec::new();

            for instrument_type in instrument_types {
                match http_client.request_instruments(instrument_type, None).await {
                    Ok((instruments, inst_id_codes)) => {
                        if instruments.is_empty() {
                            log::warn!("No instruments returned for {instrument_type:?}");
                            continue;
                        }
                        http_client.cache_instruments(instruments.clone());
                        all_instruments.extend(instruments);
                        all_inst_id_codes.extend(inst_id_codes);
                    }
                    Err(e) => {
                        log::error!("Failed to request instruments for {instrument_type:?}: {e}");
                    }
                }
            }

            if all_instruments.is_empty() {
                log::warn!(
                    "Instrument bootstrap yielded no instruments; WebSocket submissions may fail"
                );
            } else {
                ws_private.cache_instruments(all_instruments.clone());
                ws_private.cache_inst_id_codes(all_inst_id_codes.clone());
                ws_business.cache_instruments(all_instruments);
                ws_business.cache_inst_id_codes(all_inst_id_codes);
                log::info!("Instruments initialized");
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, trade_mode={:?}, instrument_types={:?}, use_fills_channel={}, is_demo={}, http_proxy_url={:?}, ws_proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.trade_mode,
            self.config.instrument_types,
            self.config.use_fills_channel,
            self.config.is_demo,
            self.config.http_proxy_url,
            self.config.ws_proxy_url,
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

        if let Some(handle) = self.ws_business_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order_type = {
            let cache = self.core.cache();
            let order = cache
                .order(&cmd.client_order_id)
                .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);

            order.order_type()
        };

        if self.is_conditional_order(order_type) {
            self.submit_conditional_order(cmd)
        } else {
            self.submit_regular_order(cmd)
        }
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        anyhow::bail!(
            "submit_order_list not implemented for OKX execution client (got {} orders)",
            cmd.order_list.client_order_ids.len()
        );
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            let result = ws_private
                .modify_order(
                    command.trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    Some(command.client_order_id),
                    command.price,
                    command.quantity,
                    command.venue_order_id,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Modify order failed: {e}"));

            if let Err(e) = result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_modify_rejected_event(
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    command.venue_order_id,
                    &format!("modify-order-error: {e}"),
                    ts_event,
                );
                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let cache = self.core.cache();
        let is_pending_algo = cache.order(&cmd.client_order_id).is_some_and(|o| {
            self.is_conditional_order(o.order_type()) && o.is_triggered() != Some(true)
        });
        drop(cache);

        if is_pending_algo {
            self.cancel_algo_order(cmd);
        } else {
            self.cancel_ws_order(cmd);
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if self.config.use_mm_mass_cancel {
            // Use OKX's mass-cancel endpoint (requires market maker permissions)
            self.mass_cancel_instrument(cmd.instrument_id);
            Ok(())
        } else {
            // Cancel orders via batch cancel (works for all users)
            let cache = self.core.cache();
            let open_orders = cache.orders_open(None, Some(&cmd.instrument_id), None, None, None);

            if open_orders.is_empty() {
                log::debug!("No open orders to cancel for {}", cmd.instrument_id);
                return Ok(());
            }

            let mut regular_payload = Vec::new();
            let mut algo_orders: Vec<(
                InstrumentId,
                ClientOrderId,
                Option<VenueOrderId>,
                TraderId,
                StrategyId,
            )> = Vec::new();

            for order in &open_orders {
                // Triggered stop orders become regular orders on OKX
                let is_pending_algo = self.is_conditional_order(order.order_type())
                    && order.is_triggered() != Some(true);

                if is_pending_algo {
                    algo_orders.push((
                        order.instrument_id(),
                        order.client_order_id(),
                        order.venue_order_id(),
                        order.trader_id(),
                        order.strategy_id(),
                    ));
                } else {
                    regular_payload.push((
                        order.instrument_id(),
                        Some(order.client_order_id()),
                        order.venue_order_id(),
                    ));
                }
            }
            drop(cache);

            log::debug!(
                "Canceling {} regular orders and {} algo orders for {}",
                regular_payload.len(),
                algo_orders.len(),
                cmd.instrument_id
            );

            if !regular_payload.is_empty() {
                let ws_private = self.ws_private.clone();
                self.spawn_task("batch_cancel_orders", async move {
                    ws_private.batch_cancel_orders(regular_payload).await?;
                    Ok(())
                });
            }

            // OKX doesn't support algo cancel via private WebSocket, must use HTTP
            if !algo_orders.is_empty() {
                let http_client = self.http_client.clone();
                let mut regular_algo_requests = Vec::new();
                let mut advance_algo_requests = Vec::new();

                for (instrument_id, client_order_id, venue_order_id, _trader_id, _strategy_id) in
                    algo_orders
                {
                    let request = OKXCancelAlgoOrderRequest {
                        inst_id: instrument_id.symbol.to_string(),
                        inst_id_code: None,
                        algo_id: venue_order_id.map(|id| id.to_string()),
                        algo_cl_ord_id: if venue_order_id.is_none() {
                            Some(client_order_id.to_string())
                        } else {
                            None
                        },
                    };

                    let cache = self.core.cache();
                    let is_advance = cache
                        .order(&client_order_id)
                        .is_some_and(|o| is_advance_algo_order(o.order_type()));
                    drop(cache);

                    if is_advance {
                        advance_algo_requests.push(request);
                    } else {
                        regular_algo_requests.push(request);
                    }
                }

                if !regular_algo_requests.is_empty() {
                    let client = http_client.clone();
                    self.spawn_task("cancel_algo_orders", async move {
                        client.cancel_algo_orders(regular_algo_requests).await?;
                        Ok(())
                    });
                }

                if !advance_algo_requests.is_empty() {
                    self.spawn_task("cancel_advance_algo_orders", async move {
                        http_client
                            .cancel_advance_algo_orders(advance_algo_requests)
                            .await?;
                        Ok(())
                    });
                }
            }

            Ok(())
        }
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();

        let mut regular_payload = Vec::new();
        let mut algo_orders = Vec::new();

        for cancel in &cmd.cancels {
            // Triggered stop orders become regular orders on OKX
            let is_pending_algo = cache.order(&cancel.client_order_id).is_some_and(|o| {
                self.is_conditional_order(o.order_type()) && o.is_triggered() != Some(true)
            });

            if is_pending_algo {
                algo_orders.push(cancel.clone());
            } else {
                regular_payload.push((
                    cancel.instrument_id,
                    Some(cancel.client_order_id),
                    cancel.venue_order_id,
                ));
            }
        }
        drop(cache);

        if !regular_payload.is_empty() {
            let ws_private = self.ws_private.clone();
            self.spawn_task("batch_cancel_orders", async move {
                ws_private.batch_cancel_orders(regular_payload).await?;
                Ok(())
            });
        }

        // OKX doesn't support algo cancel via private WebSocket, must use HTTP
        if !algo_orders.is_empty() {
            let http_client = self.http_client.clone();
            let mut regular_algo_requests = Vec::new();
            let mut advance_algo_requests = Vec::new();

            let cache = self.core.cache();
            for cancel in algo_orders {
                let request = OKXCancelAlgoOrderRequest {
                    inst_id: cancel.instrument_id.symbol.to_string(),
                    inst_id_code: None,
                    algo_id: cancel.venue_order_id.map(|id| id.to_string()),
                    algo_cl_ord_id: if cancel.venue_order_id.is_none() {
                        Some(cancel.client_order_id.to_string())
                    } else {
                        None
                    },
                };

                let is_advance = cache
                    .order(&cancel.client_order_id)
                    .is_some_and(|o| is_advance_algo_order(o.order_type()));

                if is_advance {
                    advance_algo_requests.push(request);
                } else {
                    regular_algo_requests.push(request);
                }
            }
            drop(cache);

            if !regular_algo_requests.is_empty() {
                let client = http_client.clone();
                self.spawn_task("cancel_algo_orders", async move {
                    client.cancel_algo_orders(regular_algo_requests).await?;
                    Ok(())
                });
            }

            if !advance_algo_requests.is_empty() {
                self.spawn_task("cancel_advance_algo_orders", async move {
                    http_client
                        .cancel_advance_algo_orders(advance_algo_requests)
                        .await?;
                    Ok(())
                });
            }
        }

        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_order_status_report requires instrument_id: {cmd:?}");
            return Ok(None);
        };

        let mut reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                None,
                Some(instrument_id),
                None,
                None,
                false,
                None,
            )
            .await?;

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
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_order_status_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    None,
                    None,
                    false,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_order_status_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        None,
                        None,
                        false,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
        }

        // Filter by open_only if specified
        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

        // Filter by time range if specified
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
        let start_dt = nanos_to_datetime(cmd.start);
        let end_dt = nanos_to_datetime(cmd.end);
        let mut reports = Vec::new();

        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_fill_reports(
                    self.core.account_id,
                    None,
                    Some(instrument_id),
                    start_dt,
                    end_dt,
                    None,
                )
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                let mut fetched = self
                    .http_client
                    .request_fill_reports(
                        self.core.account_id,
                        Some(inst_type),
                        None,
                        start_dt,
                        end_dt,
                        None,
                    )
                    .await?;
                reports.append(&mut fetched);
            }
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
        let mut reports = Vec::new();

        // Query derivative positions (SWAP/FUTURES/OPTION) from /api/v5/account/positions
        // Note: The positions endpoint does not support Spot or Margin - those are handled separately
        if let Some(instrument_id) = cmd.instrument_id {
            let mut fetched = self
                .http_client
                .request_position_status_reports(self.core.account_id, None, Some(instrument_id))
                .await?;
            reports.append(&mut fetched);
        } else {
            for inst_type in self.instrument_types() {
                // Skip Spot and Margin - positions API only supports derivatives
                if inst_type == OKXInstrumentType::Spot || inst_type == OKXInstrumentType::Margin {
                    continue;
                }
                let mut fetched = self
                    .http_client
                    .request_position_status_reports(self.core.account_id, Some(inst_type), None)
                    .await?;
                reports.append(&mut fetched);
            }
        }

        // Query spot margin positions from /api/v5/account/balance
        // Spot margin positions appear as balance sheet items (liab/spotInUseAmt fields)
        let mut margin_reports = self
            .http_client
            .request_spot_margin_position_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            margin_reports.retain(|report| report.instrument_id == instrument_id);
        }

        reports.append(&mut margin_reports);

        if let Some(start) = cmd.start {
            reports.retain(|r| r.ts_last >= start);
        }

        if let Some(end) = cmd.end {
            reports.retain(|r| r.ts_last <= end);
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

        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(false) // get all orders for mass status
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

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *OKX_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }
}

/// Dispatches a WebSocket message with cross-stream deduplication.
#[doc(hidden)]
pub fn dispatch_ws_message(
    message: NautilusWsMessage,
    emitter: &ExecutionEventEmitter,
    state: &WsDispatchState,
) {
    match message {
        NautilusWsMessage::AccountUpdate(account_state) => {
            emitter.send_account_state(account_state);
        }
        NautilusWsMessage::PositionUpdate(report) => {
            emitter.send_position_report(report);
        }
        NautilusWsMessage::ExecutionReports(reports) => {
            log::debug!("Processing {} execution report(s)", reports.len());
            for report in reports {
                match report {
                    ExecutionReport::Order(order_report) => {
                        if let Some(cid) = order_report.client_order_id {
                            match order_report.order_status {
                                OrderStatus::Accepted => {
                                    if state.filled_orders.contains(&cid)
                                        || state.triggered_orders.contains(&cid)
                                    {
                                        log::debug!(
                                            "Skipping stale OrderStatusReport(Accepted) \
                                             for {cid} (already triggered/filled)"
                                        );
                                        continue;
                                    }
                                }
                                OrderStatus::Triggered => {
                                    if state.filled_orders.contains(&cid) {
                                        log::debug!(
                                            "Skipping stale OrderStatusReport(Triggered) \
                                             for {cid} (already filled)"
                                        );
                                        continue;
                                    }
                                    state.insert_triggered(cid);
                                }
                                OrderStatus::Filled => {
                                    state.insert_filled(cid);
                                    state.triggered_orders.remove(&cid);
                                }
                                OrderStatus::Canceled
                                | OrderStatus::Expired
                                | OrderStatus::Rejected => {
                                    state.triggered_orders.remove(&cid);
                                    state.filled_orders.remove(&cid);
                                }
                                _ => {}
                            }
                        }
                        emitter.send_order_status_report(order_report);
                    }
                    ExecutionReport::Fill(fill_report) => {
                        if let Some(cid) = fill_report.client_order_id {
                            state.insert_filled(cid);
                            state.triggered_orders.remove(&cid);
                        }
                        emitter.send_fill_report(fill_report);
                    }
                }
            }
        }
        NautilusWsMessage::OrderAccepted(event) => {
            let cid = event.client_order_id;
            if state.filled_orders.contains(&cid) || state.triggered_orders.contains(&cid) {
                log::debug!("Skipping stale OrderAccepted for {cid} (already triggered/filled)");
                return;
            }
            emitter.send_order_event(OrderEventAny::Accepted(event));
        }
        NautilusWsMessage::OrderCanceled(event) => {
            let cid = event.client_order_id;
            state.triggered_orders.remove(&cid);
            state.filled_orders.remove(&cid);
            emitter.send_order_event(OrderEventAny::Canceled(event));
        }
        NautilusWsMessage::OrderExpired(event) => {
            let cid = event.client_order_id;
            state.triggered_orders.remove(&cid);
            state.filled_orders.remove(&cid);
            emitter.send_order_event(OrderEventAny::Expired(event));
        }
        NautilusWsMessage::OrderRejected(event) => {
            let cid = event.client_order_id;
            state.triggered_orders.remove(&cid);
            state.filled_orders.remove(&cid);
            emitter.send_order_event(OrderEventAny::Rejected(event));
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            emitter.send_order_event(OrderEventAny::CancelRejected(event));
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            emitter.send_order_event(OrderEventAny::ModifyRejected(event));
        }
        NautilusWsMessage::OrderTriggered(event) => {
            let cid = event.client_order_id;
            if state.filled_orders.contains(&cid) {
                log::debug!("Skipping stale OrderTriggered for {cid} (already filled)");
                return;
            }
            state.insert_triggered(cid);
            emitter.send_order_event(OrderEventAny::Triggered(event));
        }
        NautilusWsMessage::OrderUpdated(event) => {
            emitter.send_order_event(OrderEventAny::Updated(event));
        }
        NautilusWsMessage::Error(e) => {
            log::warn!(
                "Websocket error: code={} message={} conn_id={:?}",
                e.code,
                e.message,
                e.conn_id
            );
        }
        NautilusWsMessage::Reconnected => {
            log::info!("Websocket reconnected");
        }
        NautilusWsMessage::Authenticated => {
            log::debug!("Websocket authenticated");
        }
        NautilusWsMessage::Deltas(_)
        | NautilusWsMessage::Raw(_)
        | NautilusWsMessage::Data(_)
        | NautilusWsMessage::FundingRates(_)
        | NautilusWsMessage::Instrument(_, _)
        | NautilusWsMessage::InstrumentStatus(_) => {
            log::debug!("Ignoring websocket data message");
        }
    }
}

fn nanos_to_datetime(value: Option<UnixNanos>) -> Option<DateTime<Utc>> {
    value.map(|nanos| nanos.to_datetime_utc())
}
