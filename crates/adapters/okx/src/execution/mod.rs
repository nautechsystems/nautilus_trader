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
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use chrono::{DateTime, Utc};
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent, ExecutionReport as NautilusExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateFillReportsBuilder, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderType},
    events::{
        AccountState, OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected,
        OrderSubmitted,
    },
    identifiers::{AccountId, ClientId, InstrumentId, Venue},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;

use crate::{
    common::{
        consts::{OKX_CONDITIONAL_ORDER_TYPES, OKX_VENUE},
        enums::{OKXInstrumentType, OKXMarginMode, OKXTradeMode},
    },
    config::OKXExecClientConfig,
    http::client::OKXHttpClient,
    websocket::{
        client::OKXWebSocketClient,
        messages::{ExecutionReport, NautilusWsMessage},
    },
};

#[derive(Debug)]
pub struct OKXExecutionClient {
    core: ExecutionClientCore,
    config: OKXExecClientConfig,
    http_client: OKXHttpClient,
    ws_private: OKXWebSocketClient,
    ws_business: OKXWebSocketClient,
    trade_mode: OKXTradeMode,
    exec_event_sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    ws_stream_handle: Option<JoinHandle<()>>,
    ws_business_stream_handle: Option<JoinHandle<()>>,
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

        Ok(Self {
            core,
            config,
            http_client,
            ws_private,
            ws_business,
            trade_mode,
            exec_event_sender: None,
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            ws_stream_handle: None,
            ws_business_stream_handle: None,
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

        self.core.generate_account_state(
            account_state.balances.clone(),
            account_state.margins.clone(),
            account_state.is_reported,
            account_state.ts_event,
        )
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }

    fn is_conditional_order(&self, order_type: OrderType) -> bool {
        OKX_CONDITIONAL_ORDER_TYPES.contains(&order_type)
    }

    fn submit_regular_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let ws_private = self.ws_private.clone();
        let trade_mode = self.trade_mode;

        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
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
                let rejected_event = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    format!("submit-order-error: {e}").into(),
                    UUID4::new(),
                    ts_init,
                    get_atomic_clock_realtime().get_time_ns(),
                    false,
                    false,
                );

                if let Some(sender) = &exec_event_sender {
                    if let Err(send_err) = sender.send(ExecutionEvent::Order(
                        OrderEventAny::Rejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderRejected event: {send_err}");
                    }
                } else {
                    log::warn!("Cannot send OrderRejected: exec_event_sender not initialized");
                }

                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_conditional_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let trigger_price = order
            .trigger_price()
            .ok_or_else(|| anyhow::anyhow!("conditional order requires a trigger price"))?;
        let http_client = self.http_client.clone();
        let trade_mode = self.trade_mode;

        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();
        let instrument_id = order.instrument_id();
        let order_side = order.order_side();
        let order_type = order.order_type();
        let quantity = order.quantity();
        let trigger_type = order.trigger_type();
        let price = order.price();
        let is_reduce_only = order.is_reduce_only();

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
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit algo order failed: {e}"));

            if let Err(e) = result {
                let rejected_event = OrderRejected::new(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    account_id,
                    format!("submit-order-error: {e}").into(),
                    UUID4::new(),
                    ts_init,
                    get_atomic_clock_realtime().get_time_ns(),
                    false,
                    false,
                );

                if let Some(sender) = &exec_event_sender {
                    if let Err(send_err) = sender.send(ExecutionEvent::Order(
                        OrderEventAny::Rejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderRejected event: {send_err}");
                    }
                } else {
                    log::warn!("Cannot send OrderRejected: exec_event_sender not initialized");
                }

                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_ws_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;

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
                let rejected_event = OrderCancelRejected::new(
                    trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    format!("cancel-order-error: {e}").into(),
                    UUID4::new(),
                    get_atomic_clock_realtime().get_time_ns(),
                    ts_init,
                    false,
                    command.venue_order_id,
                    Some(account_id),
                );

                if let Some(sender) = &exec_event_sender {
                    if let Err(send_err) = sender.send(ExecutionEvent::Order(
                        OrderEventAny::CancelRejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderCancelRejected event: {send_err}");
                    }
                } else {
                    log::warn!(
                        "Cannot send OrderCancelRejected: exec_event_sender not initialized"
                    );
                }

                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn mass_cancel_instrument(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        let ws_private = self.ws_private.clone();
        self.spawn_task("mass_cancel_orders", async move {
            ws_private.mass_cancel_orders(instrument_id).await?;
            Ok(())
        });
        Ok(())
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

        if self.core.cache().borrow().account(&account_id).is_some() {
            log::info!("Account {account_id} registered");
            return Ok(());
        }

        let start = Instant::now();
        let timeout = Duration::from_secs_f64(timeout_secs);
        let interval = Duration::from_millis(10);

        loop {
            tokio::time::sleep(interval).await;

            if self.core.cache().borrow().account(&account_id).is_some() {
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
        self.connected.load(Ordering::Acquire)
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
        self.core.get_account()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.connected.load(Ordering::Acquire) {
            return Ok(());
        }

        // Initialize exec event sender (must be done in async context after runner is set up)
        if self.exec_event_sender.is_none() {
            self.exec_event_sender = Some(get_exec_event_sender());
        }

        let instrument_types = self.instrument_types();

        if !self.instruments_initialized.load(Ordering::Acquire) {
            let mut all_instruments = Vec::new();
            for instrument_type in &instrument_types {
                let instruments = self
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
            }

            // Add instruments to Nautilus Cache for reconciliation
            {
                let mut cache = self.core.cache().borrow_mut();
                for instrument in &all_instruments {
                    if let Err(e) = cache.add_instrument(instrument.clone()) {
                        log::debug!("Instrument already in cache: {e}");
                    }
                }
            }

            if !all_instruments.is_empty() {
                self.ws_private.cache_instruments(all_instruments);
            }
            self.instruments_initialized.store(true, Ordering::Release);
        }

        let Some(sender) = self.exec_event_sender.as_ref() else {
            log::error!("Execution event sender not initialized");
            anyhow::bail!("Execution event sender not initialized");
        };

        self.ws_private.connect().await?;
        self.ws_private.wait_until_active(10.0).await?;
        log::info!("Connected to private WebSocket");

        if self.ws_stream_handle.is_none() {
            let stream = self.ws_private.stream();
            let sender = sender.clone();
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &sender);
                }
            });
            self.ws_stream_handle = Some(handle);
        }

        self.ws_business.connect().await?;
        self.ws_business.wait_until_active(10.0).await?;
        log::info!("Connected to business WebSocket");

        if self.ws_business_stream_handle.is_none() {
            let stream = self.ws_business.stream();
            let sender = sender.clone();
            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &sender);
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
        dispatch_account_state(account_state, sender);

        // Wait for account to be registered in cache before completing connect
        self.await_account_registered(30.0).await?;

        self.connected.store(true, Ordering::Release);
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.connected.load(Ordering::Acquire) {
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

        self.connected.store(false, Ordering::Release);
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
        self.core
            .generate_account_state(balances, margins, reported, ts_event)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.started {
            return Ok(());
        }

        self.started = true;

        // Spawn instrument bootstrap task
        let http_client = self.http_client.clone();
        let ws_private = self.ws_private.clone();
        let instrument_types = self.config.instrument_types.clone();

        get_runtime().spawn(async move {
            let mut all_instruments = Vec::new();
            for instrument_type in instrument_types {
                match http_client.request_instruments(instrument_type, None).await {
                    Ok(instruments) => {
                        if instruments.is_empty() {
                            log::warn!("No instruments returned for {instrument_type:?}");
                            continue;
                        }
                        http_client.cache_instruments(instruments.clone());
                        all_instruments.extend(instruments);
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
                ws_private.cache_instruments(all_instruments);
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
        if !self.started {
            return Ok(());
        }

        self.started = false;
        self.connected.store(false, Ordering::Release);
        if let Some(handle) = self.ws_stream_handle.take() {
            handle.abort();
        }
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;

        if order.is_closed() {
            let client_order_id = order.client_order_id();
            log::warn!("Cannot submit closed order {client_order_id}");
            return Ok(());
        }

        let event = OrderSubmitted::new(
            self.core.trader_id,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            self.core.account_id,
            UUID4::new(),
            cmd.ts_init,
            get_atomic_clock_realtime().get_time_ns(),
        );
        if let Some(sender) = &self.exec_event_sender {
            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            if let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::Submitted(event))) {
                log::warn!("Failed to send OrderSubmitted event: {e}");
            }
        } else {
            log::warn!("Cannot send OrderSubmitted: exec_event_sender not initialized");
        }

        if self.is_conditional_order(order.order_type()) {
            self.submit_conditional_order(cmd)
        } else {
            self.submit_regular_order(cmd)
        }
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for OKX execution client (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        let ws_private = self.ws_private.clone();
        let command = cmd.clone();

        // Capture for error handling
        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;

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
                let rejected_event = OrderModifyRejected::new(
                    trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    command.client_order_id,
                    format!("modify-order-error: {e}").into(),
                    UUID4::new(),
                    get_atomic_clock_realtime().get_time_ns(),
                    ts_init,
                    false,
                    command.venue_order_id,
                    Some(account_id),
                );

                if let Some(sender) = &exec_event_sender {
                    if let Err(send_err) = sender.send(ExecutionEvent::Order(
                        OrderEventAny::ModifyRejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderModifyRejected event: {send_err}");
                    }
                } else {
                    log::warn!(
                        "Cannot send OrderModifyRejected: exec_event_sender not initialized"
                    );
                }

                return Err(e);
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        self.cancel_ws_order(cmd)
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        if self.config.use_mm_mass_cancel {
            // Use OKX's mass-cancel endpoint (requires market maker permissions)
            self.mass_cancel_instrument(cmd.instrument_id)
        } else {
            // Cancel orders individually via batch cancel (works for all users)
            let cache = self.core.cache().borrow();
            let open_orders = cache.orders_open(None, Some(&cmd.instrument_id), None, None, None);

            if open_orders.is_empty() {
                log::debug!("No open orders to cancel for {}", cmd.instrument_id);
                return Ok(());
            }

            let mut payload = Vec::with_capacity(open_orders.len());
            for order in open_orders {
                payload.push((
                    order.instrument_id(),
                    Some(order.client_order_id()),
                    order.venue_order_id(),
                ));
            }
            drop(cache);

            log::debug!(
                "Canceling {} open orders for {} via batch cancel",
                payload.len(),
                cmd.instrument_id
            );

            let ws_private = self.ws_private.clone();
            self.spawn_task("batch_cancel_orders", async move {
                ws_private.batch_cancel_orders(payload).await?;
                Ok(())
            });

            Ok(())
        }
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        let mut payload = Vec::with_capacity(cmd.cancels.len());

        for cancel in &cmd.cancels {
            payload.push((
                cancel.instrument_id,
                Some(cancel.client_order_id),
                cancel.venue_order_id,
            ));
        }

        let ws_private = self.ws_private.clone();
        self.spawn_task("batch_cancel_orders", async move {
            ws_private.batch_cancel_orders(payload).await?;
            Ok(())
        });

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

        let _ = nanos_to_datetime(cmd.start);
        let _ = nanos_to_datetime(cmd.end);

        Ok(reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = get_atomic_clock_realtime().get_time_ns();

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

fn dispatch_ws_message(
    message: NautilusWsMessage,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    match message {
        NautilusWsMessage::AccountUpdate(state) => dispatch_account_state(state, sender),
        NautilusWsMessage::PositionUpdate(report) => {
            dispatch_position_status_report(report, sender);
        }
        NautilusWsMessage::ExecutionReports(reports) => {
            log::debug!("Processing {} execution report(s)", reports.len());
            for report in reports {
                dispatch_execution_report(report, sender);
            }
        }
        NautilusWsMessage::OrderAccepted(event) => {
            dispatch_order_event(OrderEventAny::Accepted(event), sender);
        }
        NautilusWsMessage::OrderCanceled(event) => {
            dispatch_order_event(OrderEventAny::Canceled(event), sender);
        }
        NautilusWsMessage::OrderExpired(event) => {
            dispatch_order_event(OrderEventAny::Expired(event), sender);
        }
        NautilusWsMessage::OrderRejected(event) => {
            dispatch_order_event(OrderEventAny::Rejected(event), sender);
        }
        NautilusWsMessage::OrderCancelRejected(event) => {
            dispatch_order_event(OrderEventAny::CancelRejected(event), sender);
        }
        NautilusWsMessage::OrderModifyRejected(event) => {
            dispatch_order_event(OrderEventAny::ModifyRejected(event), sender);
        }
        NautilusWsMessage::OrderTriggered(event) => {
            dispatch_order_event(OrderEventAny::Triggered(event), sender);
        }
        NautilusWsMessage::OrderUpdated(event) => {
            dispatch_order_event(OrderEventAny::Updated(event), sender);
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
        | NautilusWsMessage::Instrument(_) => {
            log::debug!("Ignoring websocket data message");
        }
    }
}

fn dispatch_account_state(
    state: AccountState,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    if let Err(e) = sender.send(ExecutionEvent::Account(state)) {
        log::warn!("Failed to send account state: {e}");
    }
}

fn dispatch_position_status_report(
    report: PositionStatusReport,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    let exec_report = NautilusExecutionReport::Position(Box::new(report));
    if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
        log::warn!("Failed to send position status report: {e}");
    }
}

fn dispatch_execution_report(
    report: ExecutionReport,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    match report {
        ExecutionReport::Order(order_report) => {
            let exec_report = NautilusExecutionReport::Order(Box::new(order_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send order status report: {e}");
            }
        }
        ExecutionReport::Fill(fill_report) => {
            let exec_report = NautilusExecutionReport::Fill(Box::new(fill_report));
            if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                log::warn!("Failed to send fill report: {e}");
            }
        }
    }
}

fn dispatch_order_event(
    event: OrderEventAny,
    sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
) {
    if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
        log::warn!("Failed to send order event: {e}");
    }
}

fn nanos_to_datetime(value: Option<UnixNanos>) -> Option<DateTime<Utc>> {
    value.map(|nanos| nanos.to_datetime_utc())
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::execution::{BatchCancelOrders, CancelOrder};
    use nautilus_core::UnixNanos;
    use nautilus_model::identifiers::{
        ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId,
    };
    use rstest::rstest;

    #[rstest]
    fn test_batch_cancel_orders_builds_payload() {
        let trader_id = TraderId::from("TRADER-001");
        let strategy_id = StrategyId::from("STRATEGY-001");
        let client_id = Some(ClientId::from("OKX"));
        let instrument_id = InstrumentId::from("BTC-USDT.OKX");
        let client_order_id1 = ClientOrderId::new("order1");
        let client_order_id2 = ClientOrderId::new("order2");
        let venue_order_id1 = VenueOrderId::new("venue1");
        let venue_order_id2 = VenueOrderId::new("venue2");

        let cmd = BatchCancelOrders {
            trader_id,
            client_id,
            strategy_id,
            instrument_id,
            cancels: vec![
                CancelOrder {
                    trader_id,
                    client_id,
                    strategy_id,
                    instrument_id,
                    client_order_id: client_order_id1,
                    venue_order_id: Some(venue_order_id1),
                    command_id: Default::default(),
                    ts_init: UnixNanos::default(),
                    params: None,
                },
                CancelOrder {
                    trader_id,
                    client_id,
                    strategy_id,
                    instrument_id,
                    client_order_id: client_order_id2,
                    venue_order_id: Some(venue_order_id2),
                    command_id: Default::default(),
                    ts_init: UnixNanos::default(),
                    params: None,
                },
            ],
            command_id: Default::default(),
            ts_init: UnixNanos::default(),
            params: None,
        };

        // Verify we can build the payload structure
        let mut payload = Vec::with_capacity(cmd.cancels.len());
        for cancel in &cmd.cancels {
            payload.push((
                cancel.instrument_id,
                Some(cancel.client_order_id),
                cancel.venue_order_id,
            ));
        }

        assert_eq!(payload.len(), 2);
        assert_eq!(payload[0].0, instrument_id);
        assert_eq!(payload[0].1, Some(client_order_id1));
        assert_eq!(payload[0].2, Some(venue_order_id1));
        assert_eq!(payload[1].0, instrument_id);
        assert_eq!(payload[1].1, Some(client_order_id2));
        assert_eq!(payload[1].2, Some(venue_order_id2));
    }

    #[rstest]
    fn test_batch_cancel_orders_with_empty_cancels() {
        let cmd = BatchCancelOrders {
            trader_id: TraderId::from("TRADER-001"),
            client_id: Some(ClientId::from("OKX")),
            strategy_id: StrategyId::from("STRATEGY-001"),
            instrument_id: InstrumentId::from("BTC-USDT.OKX"),
            cancels: vec![],
            command_id: Default::default(),
            ts_init: UnixNanos::default(),
            params: None,
        };

        let payload: Vec<(InstrumentId, Option<ClientOrderId>, Option<VenueOrderId>)> =
            Vec::with_capacity(cmd.cancels.len());
        assert_eq!(payload.len(), 0);
    }
}
