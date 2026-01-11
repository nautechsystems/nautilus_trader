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

//! Live execution client implementation for the Binance Futures adapter.

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
    enums::{OmsType, OrderSide, PositionSideSpecified},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny,
        OrderModifyRejected, OrderRejected, OrderSubmitted, OrderUpdated,
    },
    identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId},
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use super::http::{
    client::BinanceFuturesHttpClient,
    models::{BatchOrderResult, BinancePositionRisk},
    query::{
        BatchCancelItem, BinanceAllOrdersParamsBuilder, BinanceOpenOrdersParamsBuilder,
        BinanceOrderQueryParamsBuilder, BinancePositionRiskParamsBuilder,
        BinanceUserTradesParamsBuilder,
    },
};
use crate::{
    common::{
        consts::BINANCE_VENUE,
        enums::{BinancePositionSide, BinanceProductType},
    },
    config::BinanceExecClientConfig,
    futures::http::models::BinanceFuturesAccountInfo,
};

/// Live execution client for Binance Futures trading.
///
/// Implements the [`ExecutionClient`] trait for order management on Binance
/// USD-M and COIN-M Futures markets. Uses HTTP API for order operations and
/// supports position management, leverage configuration, and margin types.
#[derive(Debug)]
pub struct BinanceFuturesExecutionClient {
    core: ExecutionClientCore,
    config: BinanceExecClientConfig,
    http_client: BinanceFuturesHttpClient,
    clock: &'static AtomicTime,
    exec_event_sender: Option<tokio::sync::mpsc::UnboundedSender<ExecutionEvent>>,
    started: bool,
    connected: AtomicBool,
    instruments_initialized: AtomicBool,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    is_hedge_mode: AtomicBool,
}

impl BinanceFuturesExecutionClient {
    /// Creates a new [`BinanceFuturesExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the HTTP client fails to initialize or credentials are missing.
    pub fn new(core: ExecutionClientCore, config: BinanceExecClientConfig) -> anyhow::Result<Self> {
        let api_key = config
            .api_key
            .clone()
            .or_else(|| std::env::var("BINANCE_FUTURES_API_KEY").ok())
            .or_else(|| std::env::var("BINANCE_API_KEY").ok())
            .ok_or_else(|| {
                anyhow::anyhow!("BINANCE_FUTURES_API_KEY or BINANCE_API_KEY not found")
            })?;

        let api_secret = config
            .api_secret
            .clone()
            .or_else(|| std::env::var("BINANCE_FUTURES_API_SECRET").ok())
            .or_else(|| std::env::var("BINANCE_API_SECRET").ok())
            .ok_or_else(|| {
                anyhow::anyhow!("BINANCE_FUTURES_API_SECRET or BINANCE_API_SECRET not found")
            })?;

        let product_type = config
            .product_types
            .iter()
            .find(|pt| matches!(pt, BinanceProductType::UsdM | BinanceProductType::CoinM))
            .copied()
            .unwrap_or(BinanceProductType::UsdM);

        let http_client = BinanceFuturesHttpClient::new(
            product_type,
            config.environment,
            Some(api_key),
            Some(api_secret),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
        )
        .context("failed to construct Binance Futures HTTP client")?;

        Ok(Self {
            core,
            config,
            http_client,
            clock: get_atomic_clock_realtime(),
            exec_event_sender: None,
            started: false,
            connected: AtomicBool::new(false),
            instruments_initialized: AtomicBool::new(false),
            pending_tasks: Mutex::new(Vec::new()),
            is_hedge_mode: AtomicBool::new(false),
        })
    }

    /// Returns whether the account is in hedge mode (dual side position).
    #[must_use]
    pub fn is_hedge_mode(&self) -> bool {
        self.is_hedge_mode.load(Ordering::Acquire)
    }

    /// Determines the position side for hedge mode based on order direction.
    fn determine_position_side(
        &self,
        order_side: OrderSide,
        reduce_only: bool,
    ) -> Option<BinancePositionSide> {
        if !self.is_hedge_mode() {
            return None;
        }

        // In hedge mode, position side depends on whether we're opening or closing
        Some(if reduce_only {
            // Closing: Buy closes Short, Sell closes Long
            match order_side {
                OrderSide::Buy => BinancePositionSide::Short,
                OrderSide::Sell => BinancePositionSide::Long,
                _ => BinancePositionSide::Both,
            }
        } else {
            // Opening: Buy opens Long, Sell opens Short
            match order_side {
                OrderSide::Buy => BinancePositionSide::Long,
                OrderSide::Sell => BinancePositionSide::Short,
                _ => BinancePositionSide::Both,
            }
        })
    }

    /// Converts Binance futures account info to Nautilus account state.
    fn create_account_state(&self, account_info: &BinanceFuturesAccountInfo) -> AccountState {
        let ts_now = self.clock.get_time_ns();

        let balances: Vec<AccountBalance> = account_info
            .assets
            .iter()
            .filter_map(|b| {
                let wallet_balance: f64 = b.balance.parse().unwrap_or(0.0);
                let available_balance: f64 = b.available_balance.parse().unwrap_or(0.0);
                let locked = wallet_balance - available_balance;

                if wallet_balance == 0.0 {
                    return None;
                }

                let currency = Currency::from(&b.asset);
                Some(AccountBalance::new(
                    Money::new(wallet_balance, currency),
                    Money::new(locked.max(0.0), currency),
                    Money::new(available_balance, currency),
                ))
            })
            .collect();

        // Margin balances in futures are position-specific, not account-level,
        // so we don't create MarginBalance entries here as they require instrument_id.
        let margins: Vec<MarginBalance> = Vec::new();

        AccountState::new(
            self.core.account_id,
            self.core.account_type,
            balances,
            margins,
            true, // reported
            UUID4::new(),
            ts_now,
            ts_now,
            None, // base currency
        )
    }

    async fn refresh_account_state(&self) -> anyhow::Result<AccountState> {
        let account_info = match self.http_client.query_account().await {
            Ok(info) => info,
            Err(e) => {
                log::error!("Binance Futures account state request failed: {e}");
                anyhow::bail!("Binance Futures account state request failed: {e}");
            }
        };

        Ok(self.create_account_state(&account_info))
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

    async fn init_hedge_mode(&self) -> anyhow::Result<bool> {
        let response = self.http_client.query_hedge_mode().await?;
        Ok(response.dual_side_position)
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();

        let order = self.core.get_order(&cmd.client_order_id)?;
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
        let reduce_only = order.is_reduce_only();
        let position_side = self.determine_position_side(order_side, reduce_only);
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
                    reduce_only,
                    position_side,
                )
                .await;

            match result {
                Ok(report) => {
                    let venue_order_id = report.venue_order_id;
                    let accepted_event = OrderAccepted::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        venue_order_id,
                        account_id,
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                    );

                    if let Some(sender) = &exec_event_sender
                        && let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::Accepted(
                            accepted_event,
                        )))
                    {
                        log::warn!("Failed to send OrderAccepted event: {e}");
                    }
                }
                Err(e) => {
                    let rejected_event = OrderRejected::new(
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

                    if let Some(sender) = &exec_event_sender
                        && let Err(send_err) = sender.send(ExecutionEvent::Order(
                            OrderEventAny::Rejected(rejected_event),
                        ))
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
        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let instrument_id = command.instrument_id;
        let venue_order_id = command.venue_order_id;
        let client_order_id = Some(command.client_order_id);
        let clock = self.clock;

        self.spawn_task("cancel_order", async move {
            let result = http_client
                .cancel_order(instrument_id, venue_order_id, client_order_id)
                .await;

            match result {
                Ok(venue_order_id) => {
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

                    if let Some(sender) = &exec_event_sender
                        && let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::Canceled(
                            canceled_event,
                        )))
                    {
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

                    if let Some(sender) = &exec_event_sender
                        && let Err(send_err) = sender.send(ExecutionEvent::Order(
                            OrderEventAny::CancelRejected(rejected_event),
                        ))
                    {
                        log::warn!("Failed to send OrderCancelRejected event: {send_err}");
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

    /// Returns the (price_precision, size_precision) for an instrument.
    fn get_instrument_precision(&self, instrument_id: InstrumentId) -> (u8, u8) {
        let cache = self.core.cache().borrow();
        cache
            .instrument(&instrument_id)
            .map_or((8, 8), |i| (i.price_precision(), i.size_precision()))
    }

    /// Creates a position status report from Binance position risk data.
    fn create_position_report(
        &self,
        position: &BinancePositionRisk,
        instrument_id: InstrumentId,
        size_precision: u8,
    ) -> anyhow::Result<PositionStatusReport> {
        let position_amount: Decimal = position
            .position_amt
            .parse()
            .context("invalid position_amt")?;

        if position_amount.is_zero() {
            anyhow::bail!("Position is flat");
        }

        let entry_price: Decimal = position
            .entry_price
            .parse()
            .context("invalid entry_price")?;

        let position_side = if position_amount > Decimal::ZERO {
            PositionSideSpecified::Long
        } else {
            PositionSideSpecified::Short
        };

        let ts_now = self.clock.get_time_ns();

        Ok(PositionStatusReport::new(
            self.core.account_id,
            instrument_id,
            position_side,
            Quantity::new(position_amount.abs().to_string().parse()?, size_precision),
            ts_now,
            ts_now,
            Some(UUID4::new()),
            None, // venue_position_id
            Some(entry_price),
        ))
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BinanceFuturesExecutionClient {
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

        if self.exec_event_sender.is_none() {
            self.exec_event_sender = Some(get_exec_event_sender());
        }

        // Check hedge mode
        let is_hedge_mode = self
            .init_hedge_mode()
            .await
            .context("failed to query hedge mode")?;
        self.is_hedge_mode.store(is_hedge_mode, Ordering::Release);
        log::info!("Hedge mode (dual side position): {is_hedge_mode}");

        // Load instruments if not already done
        if !self.instruments_initialized.load(Ordering::Acquire) {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request Binance Futures instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for Binance Futures");
            } else {
                log::info!("Loaded {} Futures instruments", instruments.len());

                let mut cache = self.core.cache().borrow_mut();
                for instrument in &instruments {
                    if let Err(e) = cache.add_instrument(instrument.clone()) {
                        log::debug!("Instrument already in cache: {e}");
                    }
                }
            }

            self.instruments_initialized.store(true, Ordering::Release);
        }

        let Some(sender) = self.exec_event_sender.as_ref() else {
            log::error!("Execution event sender not initialized");
            anyhow::bail!("Execution event sender not initialized");
        };

        // Request initial account state
        let account_state = self
            .refresh_account_state()
            .await
            .context("failed to request Binance Futures account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s) and {} margin(s)",
                account_state.balances.len(),
                account_state.margins.len()
            );
        }

        if let Err(e) = sender.send(ExecutionEvent::Account(account_state)) {
            log::warn!("Failed to send account state: {e}");
        }

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
        let exec_event_sender = self.exec_event_sender.clone();
        let account_id = self.core.account_id;

        let symbol = command.instrument_id.symbol.to_string();
        let order_id = command.venue_order_id.map(|id| {
            id.inner()
                .parse::<i64>()
                .expect("venue_order_id should be numeric")
        });
        let orig_client_order_id = Some(command.client_order_id.to_string());
        let (_, size_precision) = self.get_instrument_precision(command.instrument_id);

        self.spawn_task("query_order", async move {
            let mut builder = BinanceOrderQueryParamsBuilder::default();
            builder.symbol(symbol.clone());
            if let Some(oid) = order_id {
                builder.order_id(oid);
            }
            if let Some(coid) = orig_client_order_id {
                builder.orig_client_order_id(coid);
            }
            let params = builder.build().expect("order query params");

            let result = http_client.query_order(&params).await;

            match result {
                Ok(order) => {
                    let report = order.to_order_status_report(
                        account_id,
                        command.instrument_id,
                        size_precision,
                    )?;

                    if let Some(sender) = &exec_event_sender {
                        let exec_report = NautilusExecutionReport::Order(Box::new(report));
                        if let Err(e) = sender.send(ExecutionEvent::Report(exec_report)) {
                            log::warn!("Failed to send order status report: {e}");
                        }
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

        let http_client = self.http_client.clone();

        get_runtime().spawn(async move {
            match http_client.request_instruments().await {
                Ok(instruments) => {
                    if instruments.is_empty() {
                        log::warn!("No instruments returned for Binance Futures");
                    } else {
                        log::info!("Loaded {} Futures instruments", instruments.len());
                    }
                }
                Err(e) => {
                    log::error!("Failed to request Binance Futures instruments: {e}");
                }
            }
        });

        log::info!(
            "Started: client_id={}, account_id={}, account_type={:?}, environment={:?}",
            self.core.client_id,
            self.core.account_id,
            self.core.account_type,
            self.config.environment,
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

        if let Some(sender) = &self.exec_event_sender {
            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            if let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::Submitted(event))) {
                log::warn!("Failed to send OrderSubmitted event: {e}");
            }
        } else {
            log::warn!("Cannot send OrderSubmitted: exec_event_sender not initialized");
        }

        self.submit_order_internal(cmd)
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for Binance Futures (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
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

            if let Some(sender) = &self.exec_event_sender
                && let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                    rejected_event,
                )))
            {
                log::warn!("Failed to send OrderModifyRejected event: {e}");
            }
            return Ok(());
        };

        let http_client = self.http_client.clone();
        let command = cmd.clone();
        let exec_event_sender = self.exec_event_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let instrument_id = command.instrument_id;
        let venue_order_id = command.venue_order_id;
        let client_order_id = Some(command.client_order_id);
        let order_side = order.order_side();
        let quantity = command.quantity.unwrap_or_else(|| order.quantity());
        let price = command.price.or_else(|| order.price());

        let Some(price) = price else {
            log::warn!(
                "Cannot modify order {}: price required",
                cmd.client_order_id
            );
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Price required for order modification".into(),
                UUID4::new(),
                self.clock.get_time_ns(),
                cmd.ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            if let Some(sender) = &self.exec_event_sender
                && let Err(e) = sender.send(ExecutionEvent::Order(OrderEventAny::ModifyRejected(
                    rejected_event,
                )))
            {
                log::warn!("Failed to send OrderModifyRejected event: {e}");
            }
            return Ok(());
        };
        let clock = self.clock;

        self.spawn_task("modify_order", async move {
            let result = http_client
                .modify_order(
                    account_id,
                    instrument_id,
                    venue_order_id,
                    client_order_id,
                    order_side,
                    quantity,
                    price,
                )
                .await;

            match result {
                Ok(report) => {
                    let updated_event = OrderUpdated::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        quantity,
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        Some(report.venue_order_id),
                        Some(account_id),
                        Some(price),
                        None,
                        None,
                    );

                    if let Some(sender) = &exec_event_sender
                        && let Err(e) = sender
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
                        format!("modify-order-failed: {e}").into(),
                        UUID4::new(),
                        clock.get_time_ns(),
                        ts_init,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );

                    if let Some(sender) = &exec_event_sender
                        && let Err(send_err) = sender.send(ExecutionEvent::Order(
                            OrderEventAny::ModifyRejected(rejected_event),
                        ))
                    {
                        log::warn!("Failed to send OrderModifyRejected event: {send_err}");
                    }

                    anyhow::bail!("Modify order failed: {e}");
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
        let instrument_id = cmd.instrument_id;

        self.spawn_task("cancel_all_orders", async move {
            match http_client.cancel_all_orders(instrument_id).await {
                Ok(_) => {
                    log::info!("Cancelled all orders for {instrument_id}");
                }
                Err(e) => {
                    log::error!("Failed to cancel all orders for {instrument_id}: {e}");
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

        let exec_event_sender = self.exec_event_sender.clone();
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
                                BatchOrderResult::Success(response) => {
                                    let venue_order_id =
                                        VenueOrderId::new(response.order_id.to_string());
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

                                    if let Some(sender) = &exec_event_sender
                                        && let Err(e) = sender.send(ExecutionEvent::Order(
                                            OrderEventAny::Canceled(canceled_event),
                                        ))
                                    {
                                        log::warn!("Failed to send OrderCanceled event: {e}");
                                    }
                                }
                                BatchOrderResult::Error(error) => {
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

                                    if let Some(sender) = &exec_event_sender
                                        && let Err(e) = sender.send(ExecutionEvent::Order(
                                            OrderEventAny::CancelRejected(rejected_event),
                                        ))
                                    {
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

                            if let Some(sender) = &exec_event_sender
                                && let Err(send_err) = sender.send(ExecutionEvent::Order(
                                    OrderEventAny::CancelRejected(rejected_event),
                                ))
                            {
                                log::warn!("Failed to send OrderCancelRejected event: {send_err}");
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

        let symbol = instrument_id.symbol.to_string();
        let order_id = cmd.venue_order_id.as_ref().map(|id| {
            id.inner()
                .parse::<i64>()
                .expect("venue_order_id should be numeric")
        });
        let orig_client_order_id = cmd.client_order_id.map(|id| id.to_string());

        let mut builder = BinanceOrderQueryParamsBuilder::default();
        builder.symbol(symbol);
        if let Some(oid) = order_id {
            builder.order_id(oid);
        }
        if let Some(coid) = orig_client_order_id {
            builder.orig_client_order_id(coid);
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let order = self.http_client.query_order(&params).await?;
        let (_, size_precision) = self.get_instrument_precision(instrument_id);
        let report =
            order.to_order_status_report(self.core.account_id, instrument_id, size_precision)?;

        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let mut reports = Vec::new();

        if cmd.open_only {
            let symbol = cmd.instrument_id.map(|id| id.symbol.to_string());
            let mut builder = BinanceOpenOrdersParamsBuilder::default();
            if let Some(s) = symbol {
                builder.symbol(s);
            }
            let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

            let orders = self.http_client.query_open_orders(&params).await?;

            for order in orders {
                if let Some(instrument_id) = cmd.instrument_id {
                    let (_, size_precision) = self.get_instrument_precision(instrument_id);
                    if let Ok(report) = order.to_order_status_report(
                        self.core.account_id,
                        instrument_id,
                        size_precision,
                    ) {
                        reports.push(report);
                    }
                } else {
                    let cache = self.core.cache().borrow();
                    if let Some(instrument) = cache
                        .instruments(&BINANCE_VENUE, None)
                        .into_iter()
                        .find(|i| i.symbol().as_str() == order.symbol.as_str())
                        && let Ok(report) = order.to_order_status_report(
                            self.core.account_id,
                            instrument.id(),
                            instrument.size_precision(),
                        )
                    {
                        reports.push(report);
                    }
                }
            }
        } else if let Some(instrument_id) = cmd.instrument_id {
            let symbol = instrument_id.symbol.to_string();
            let start_time = cmd.start.map(|t| t.as_i64() / 1_000_000); // ns to ms
            let end_time = cmd.end.map(|t| t.as_i64() / 1_000_000);

            let mut builder = BinanceAllOrdersParamsBuilder::default();
            builder.symbol(symbol);
            if let Some(st) = start_time {
                builder.start_time(st);
            }
            if let Some(et) = end_time {
                builder.end_time(et);
            }
            let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

            let orders = self.http_client.query_all_orders(&params).await?;
            let (_, size_precision) = self.get_instrument_precision(instrument_id);

            for order in orders {
                if let Ok(report) = order.to_order_status_report(
                    self.core.account_id,
                    instrument_id,
                    size_precision,
                ) {
                    reports.push(report);
                }
            }
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let Some(instrument_id) = cmd.instrument_id else {
            log::warn!("generate_fill_reports requires instrument_id for Binance Futures");
            return Ok(Vec::new());
        };

        let symbol = instrument_id.symbol.to_string();
        let start_time = cmd.start.map(|t| t.as_i64() / 1_000_000);
        let end_time = cmd.end.map(|t| t.as_i64() / 1_000_000);

        let mut builder = BinanceUserTradesParamsBuilder::default();
        builder.symbol(symbol);
        if let Some(st) = start_time {
            builder.start_time(st);
        }
        if let Some(et) = end_time {
            builder.end_time(et);
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let trades = self.http_client.query_user_trades(&params).await?;
        let (price_precision, size_precision) = self.get_instrument_precision(instrument_id);

        let mut reports = Vec::new();
        for trade in trades {
            if let Ok(report) = trade.to_fill_report(
                self.core.account_id,
                instrument_id,
                price_precision,
                size_precision,
            ) {
                reports.push(report);
            }
        }

        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let symbol = cmd.instrument_id.map(|id| id.symbol.to_string());

        let mut builder = BinancePositionRiskParamsBuilder::default();
        if let Some(s) = symbol {
            builder.symbol(s);
        }
        let params = builder.build().map_err(|e| anyhow::anyhow!("{e}"))?;

        let positions = self.http_client.query_positions(&params).await?;

        let mut reports = Vec::new();
        for position in positions {
            let position_amt: f64 = position.position_amt.parse().unwrap_or(0.0);
            if position_amt == 0.0 {
                continue;
            }

            let cache = self.core.cache().borrow();
            if let Some(instrument) = cache
                .instruments(&BINANCE_VENUE, None)
                .into_iter()
                .find(|i| i.symbol().as_str() == position.symbol.as_str())
                && let Ok(report) = self.create_position_report(
                    &position,
                    instrument.id(),
                    instrument.size_precision(),
                )
            {
                reports.push(report);
            }
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
