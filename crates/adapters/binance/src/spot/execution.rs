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

//! Live execution client implementation for the Binance Spot adapter.

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
use nautilus_common::{
    clients::ExecutionClient,
    live::{runner::get_exec_event_sender, runtime::get_runtime},
    messages::{
        ExecutionEvent, ExecutionReport as NautilusExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports,
            GenerateOrderStatusReportsBuilder, GeneratePositionStatusReports,
            GeneratePositionStatusReportsBuilder, ModifyOrder, QueryAccount, QueryOrder,
            SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    accounts::AccountAny,
    enums::OmsType,
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny,
        OrderModifyRejected, OrderRejected, OrderSubmitted, OrderUpdated,
    },
    identifiers::{AccountId, ClientId, Venue, VenueOrderId},
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance},
};
use tokio::task::JoinHandle;

use crate::{
    common::{consts::BINANCE_VENUE, credential::resolve_credentials, enums::BinanceProductType},
    config::BinanceExecClientConfig,
    spot::http::{
        client::BinanceSpotHttpClient, models::BatchCancelResult, query::BatchCancelItem,
    },
};

/// Live execution client for Binance Spot trading.
///
/// Implements the [`ExecutionClient`] trait for order management on Binance Spot
/// and Spot Margin markets. Uses HTTP API for all order operations with SBE encoding.
#[derive(Debug)]
pub struct BinanceSpotExecutionClient {
    clock: &'static AtomicTime,
    core: ExecutionClientCore,
    config: BinanceExecClientConfig,
    http_client: BinanceSpotHttpClient,
    exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl BinanceSpotExecutionClient {
    /// Creates a new [`BinanceSpotExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize or credentials are missing.
    pub fn new(core: ExecutionClientCore, config: BinanceExecClientConfig) -> anyhow::Result<Self> {
        let product_type = config
            .product_types
            .first()
            .copied()
            .unwrap_or(BinanceProductType::Spot);

        let (api_key, api_secret) = resolve_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.environment,
            product_type,
        )?;

        let http_client = BinanceSpotHttpClient::new(
            config.environment,
            Some(api_key),
            Some(api_secret),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
        )
        .context("failed to construct Binance Spot HTTP client")?;

        let clock = get_atomic_clock_realtime();
        let exec_sender = get_exec_event_sender();

        Ok(Self {
            clock,
            core,
            config,
            http_client,
            exec_sender,
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    async fn refresh_account_state(&self) -> anyhow::Result<AccountState> {
        self.http_client
            .request_account_state(self.core.account_id)
            .await
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        let account_state = runtime.block_on(self.refresh_account_state())?;

        self.core.generate_account_state(
            account_state.balances.clone(),
            account_state.margins.clone(),
            account_state.is_reported,
            account_state.ts_event,
        )
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        let http_client = self.http_client.clone();

        let exec_sender = self.exec_sender.clone();
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
        let clock = self.clock;

        self.spawn_task("submit_order", async move {
            let result = http_client
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
                    is_post_only,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit order failed: {e}"));

            match result {
                Ok(report) => {
                    let accepted = OrderAccepted::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        report.venue_order_id,
                        account_id,
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                    );

                    if let Err(e) =
                        exec_sender.send(ExecutionEvent::Order(OrderEventAny::Accepted(accepted)))
                    {
                        log::warn!("Failed to send OrderAccepted event: {e}");
                    }
                }
                Err(e) => {
                    let rejected = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        false,
                    );

                    if let Err(send_err) =
                        exec_sender.send(ExecutionEvent::Order(OrderEventAny::Rejected(rejected)))
                    {
                        log::warn!("Failed to send OrderRejected event: {send_err}");
                    }

                    return Err(e);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order_internal(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let command = cmd.clone();

        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            let result = http_client
                .cancel_order(
                    command.instrument_id,
                    command.venue_order_id,
                    Some(command.client_order_id),
                )
                .await
                .map_err(|e| anyhow::anyhow!("Cancel order failed: {e}"));

            match result {
                Ok(venue_order_id) => {
                    // Order canceled - dispatch OrderCanceled event
                    let canceled_event = OrderCanceled::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::Canceled(canceled_event),
                    )) {
                        log::warn!("Failed to send OrderCanceled event: {e}");
                    }
                }
                Err(e) => {
                    let rejected_event = OrderCancelRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("cancel-order-error: {e}").into(),
                        UUID4::new(),
                        clock.get_time_ns(),
                        ts_init,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::CancelRejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderCancelRejected event: {e}");
                    }

                    return Err(e);
                }
            }

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
impl ExecutionClient for BinanceSpotExecutionClient {
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
        *BINANCE_VENUE
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

        // Load instruments if not already done
        if !self.instruments_initialized.load(Ordering::Acquire) {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request Binance Spot instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for Binance Spot");
            } else {
                log::info!("Loaded {} Spot instruments", instruments.len());
                self.http_client.cache_instruments(instruments.clone());

                // Add instruments to Nautilus Cache for reconciliation
                {
                    let mut cache = self.core.cache().borrow_mut();
                    for instrument in &instruments {
                        if let Err(e) = cache.add_instrument(instrument.clone()) {
                            log::debug!("Instrument already in cache: {e}");
                        }
                    }
                }
            }

            self.instruments_initialized.store(true, Ordering::Release);
        }

        // Request initial account state
        let account_state = self
            .refresh_account_state()
            .await
            .context("failed to request Binance account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }

        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Account(account_state))
        {
            log::warn!("Failed to send account state: {e}");
        }

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

        self.connected.store(false, Ordering::Release);
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: &QueryAccount) -> anyhow::Result<()> {
        self.update_account_state()
    }

    fn query_order(&self, cmd: &QueryOrder) -> anyhow::Result<()> {
        log::debug!("query_order: client_order_id={}", cmd.client_order_id);

        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let exec_sender = self.exec_sender.clone();
        let account_id = self.core.account_id;

        self.spawn_task("query_order", async move {
            let result = http_client
                .request_order_status_report(
                    account_id,
                    command.instrument_id,
                    command.venue_order_id,
                    Some(command.client_order_id),
                )
                .await;

            match result {
                Ok(report) => {
                    let exec_report = NautilusExecutionReport::Order(Box::new(report));
                    if let Err(e) = exec_sender.send(ExecutionEvent::Report(exec_report)) {
                        log::warn!("Failed to send order status report: {e}");
                    }
                }
                Err(e) => log::warn!("Failed to query order status: {e}"),
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

        get_runtime().spawn(async move {
            match http_client.request_instruments().await {
                Ok(instruments) => {
                    if instruments.is_empty() {
                        log::warn!("No instruments returned for Binance Spot");
                    } else {
                        http_client.cache_instruments(instruments);
                        log::info!("Instruments initialized");
                    }
                }
                Err(e) => {
                    log::error!("Failed to request Binance Spot instruments: {e}");
                }
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, environment={:?}, product_types={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.environment,
            self.config.product_types,
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
            self.clock.get_time_ns(),
        );
        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Order(OrderEventAny::Submitted(event)))
        {
            log::warn!("Failed to send OrderSubmitted event: {e}");
        }

        self.submit_order_internal(cmd)
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for Binance Spot execution client (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        // Binance Spot uses cancel-replace for order modification, which requires
        // the full order specification (side, type, time_in_force). Since ModifyOrder
        // doesn't include these fields, we need to look up the original order from cache.
        let order = {
            let cache = self.core.cache().borrow();
            cache.order(&cmd.client_order_id).cloned()
        };

        let Some(order) = order else {
            log::warn!(
                "Cannot modify order {}: not found in cache",
                cmd.client_order_id
            );
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Order not found in cache for modify".into(),
                UUID4::new(),
                self.clock.get_time_ns(),
                cmd.ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            if let Err(e) =
                self.exec_sender
                    .send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                        rejected_event,
                    )))
            {
                log::warn!("Failed to send OrderModifyRejected event: {e}");
            }
            return Ok(());
        };

        let http_client = self.http_client.clone();
        let command = cmd.clone();

        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;

        // Get order properties from cached order
        let order_side = order.order_side();
        let order_type = order.order_type();
        let time_in_force = order.time_in_force();
        let quantity = cmd.quantity.unwrap_or_else(|| order.quantity());
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            // Binance uses cancel-replace for order modification
            let result = http_client
                .modify_order(
                    account_id,
                    command.instrument_id,
                    command
                        .venue_order_id
                        .ok_or_else(|| anyhow::anyhow!("venue_order_id required for modify"))?,
                    command.client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    command.price,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Modify order failed: {e}"));

            match result {
                Ok(report) => {
                    // Order modified - dispatch OrderUpdated event
                    let updated_event = OrderUpdated::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        report.quantity,
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        Some(report.venue_order_id),
                        Some(account_id),
                        report.price,
                        None, // trigger_price
                        None, // protection_price
                    );

                    if let Err(e) = exec_sender
                        .send(ExecutionEvent::Order(OrderEventAny::Updated(updated_event)))
                    {
                        log::warn!("Failed to send OrderUpdated event: {e}");
                    }
                }
                Err(e) => {
                    let rejected_event = OrderModifyRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("modify-order-error: {e}").into(),
                        UUID4::new(),
                        clock.get_time_ns(),
                        ts_init,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::ModifyRejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderModifyRejected event: {e}");
                    }

                    return Err(e);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(cmd)
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let command = cmd.clone();

        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;

        self.spawn_task("cancel_all_orders", async move {
            let canceled_orders = http_client.cancel_all_orders(command.instrument_id).await?;

            // Generate OrderCanceled events for each canceled order
            for (venue_order_id, client_order_id) in canceled_orders {
                let canceled_event = OrderCanceled::new(
                    trader_id,
                    command.strategy_id,
                    command.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    command.ts_init,
                    clock.get_time_ns(),
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );

                if let Err(e) = exec_sender.send(ExecutionEvent::Order(OrderEventAny::Canceled(
                    canceled_event,
                ))) {
                    log::warn!("Failed to send OrderCanceled event: {e}");
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        const BATCH_SIZE: usize = 5;

        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let command = cmd.clone();

        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;

        self.spawn_task("batch_cancel_orders", async move {
            for chunk in command.cancels.chunks(BATCH_SIZE) {
                let batch_items: Vec<BatchCancelItem> = chunk
                    .iter()
                    .map(|cancel| {
                        if let Some(venue_order_id) = cancel.venue_order_id {
                            let order_id = venue_order_id.inner().parse::<i64>().unwrap_or(0);
                            if order_id != 0 {
                                BatchCancelItem::by_order_id(
                                    command.instrument_id.symbol.to_string(),
                                    order_id,
                                )
                            } else {
                                BatchCancelItem::by_client_order_id(
                                    command.instrument_id.symbol.to_string(),
                                    cancel.client_order_id.to_string(),
                                )
                            }
                        } else {
                            BatchCancelItem::by_client_order_id(
                                command.instrument_id.symbol.to_string(),
                                cancel.client_order_id.to_string(),
                            )
                        }
                    })
                    .collect();

                match http_client.batch_cancel_orders(&batch_items).await {
                    Ok(results) => {
                        for (i, result) in results.iter().enumerate() {
                            let cancel = &chunk[i];
                            match result {
                                BatchCancelResult::Success(success) => {
                                    let venue_order_id =
                                        VenueOrderId::new(success.order_id.to_string());
                                    let canceled_event = OrderCanceled::new(
                                        trader_id,
                                        cancel.strategy_id,
                                        cancel.instrument_id,
                                        cancel.client_order_id,
                                        UUID4::new(),
                                        cancel.ts_init,
                                        clock.get_time_ns(),
                                        false,
                                        Some(venue_order_id),
                                        Some(account_id),
                                    );

                                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                                        OrderEventAny::Canceled(canceled_event),
                                    )) {
                                        log::warn!("Failed to send OrderCanceled event: {e}");
                                    }
                                }
                                BatchCancelResult::Error(error) => {
                                    let rejected_event = OrderCancelRejected::new(
                                        trader_id,
                                        cancel.strategy_id,
                                        cancel.instrument_id,
                                        cancel.client_order_id,
                                        format!(
                                            "batch-cancel-error: code={}, msg={}",
                                            error.code, error.msg
                                        )
                                        .into(),
                                        UUID4::new(),
                                        clock.get_time_ns(),
                                        cancel.ts_init,
                                        false,
                                        cancel.venue_order_id,
                                        Some(account_id),
                                    );

                                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                                        OrderEventAny::CancelRejected(rejected_event),
                                    )) {
                                        log::warn!("Failed to send OrderCancelRejected event: {e}");
                                    }
                                }
                            }
                        }
                    }
                    Err(e) => {
                        for cancel in chunk {
                            let rejected_event = OrderCancelRejected::new(
                                trader_id,
                                cancel.strategy_id,
                                cancel.instrument_id,
                                cancel.client_order_id,
                                format!("batch-cancel-request-failed: {e}").into(),
                                UUID4::new(),
                                clock.get_time_ns(),
                                cancel.ts_init,
                                false,
                                cancel.venue_order_id,
                                Some(account_id),
                            );

                            if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                                OrderEventAny::CancelRejected(rejected_event),
                            )) {
                                log::warn!("Failed to send OrderCancelRejected event: {e}");
                            }
                        }
                    }
                }
            }

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

        // Convert ClientOrderId to VenueOrderId if provided (API naming quirk)
        let venue_order_id = cmd
            .venue_order_id
            .as_ref()
            .map(|id| VenueOrderId::new(id.inner()));

        let report = self
            .http_client
            .request_order_status_report(
                self.core.account_id,
                instrument_id,
                venue_order_id,
                cmd.client_order_id,
            )
            .await?;

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let start_dt = cmd.start.map(|nanos| nanos.to_datetime_utc());
        let end_dt = cmd.end.map(|nanos| nanos.to_datetime_utc());

        let reports = self
            .http_client
            .request_order_status_reports(
                self.core.account_id,
                cmd.instrument_id,
                start_dt,
                end_dt,
                cmd.open_only,
                None, // limit
            )
            .await?;

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_fill_reports requires instrument_id for Binance Spot");
            return Ok(Vec::new());
        };

        // Convert ClientOrderId to VenueOrderId if provided (API naming quirk)
        let venue_order_id = cmd
            .venue_order_id
            .as_ref()
            .map(|id| VenueOrderId::new(id.inner()));

        let start_dt = cmd.start.map(|nanos| nanos.to_datetime_utc());
        let end_dt = cmd.end.map(|nanos| nanos.to_datetime_utc());

        let reports = self
            .http_client
            .request_fill_reports(
                self.core.account_id,
                instrument_id,
                venue_order_id,
                start_dt,
                end_dt,
                None, // limit
            )
            .await?;

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Spot trading doesn't have positions in the traditional sense
        // Returns empty for spot, could be extended for margin positions
        Ok(Vec::new())
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

        // Binance requires instrument_id for historical orders (open_only=false).
        // Use open_only=true for mass status to get all open orders across instruments.
        let order_cmd = GenerateOrderStatusReportsBuilder::default()
            .ts_init(ts_now)
            .open_only(true)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let position_cmd = GeneratePositionStatusReportsBuilder::default()
            .ts_init(ts_now)
            .start(start)
            .build()
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let (order_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&order_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        // Note: Fill reports require instrument_id for Binance, so we skip them in mass status
        // They would need to be fetched per-instrument if needed

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *BINANCE_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }
}
