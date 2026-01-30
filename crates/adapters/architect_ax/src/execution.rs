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

//! Live execution client implementation for the AX Exchange adapter.

use std::{
    future::Future,
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
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
    MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, OmsType, OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Price},
};
use tokio::task::JoinHandle;
use totp_rs::{Algorithm, Secret, TOTP};

use crate::{
    common::consts::AX_VENUE,
    config::AxExecClientConfig,
    http::client::AxHttpClient,
    websocket::{AxOrdersWsMessage, NautilusExecWsMessage, orders::AxOrdersWebSocketClient},
};

/// Live execution client for the AX Exchange.
#[derive(Debug)]
pub struct AxExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: AxExecClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: AxHttpClient,
    ws_orders: AxOrdersWebSocketClient,
    ws_stream_handle: Option<JoinHandle<()>>,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
}

impl AxExecutionClient {
    /// Creates a new [`AxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: AxExecClientConfig) -> anyhow::Result<Self> {
        let http_client = AxHttpClient::with_credentials(
            config.api_key.clone().unwrap_or_default(),
            config.api_secret.clone().unwrap_or_default(),
            Some(config.http_base_url()),
            Some(config.orders_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config.http_proxy_url.clone(),
        )?;

        let clock = get_atomic_clock_realtime();
        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let ws_orders = AxOrdersWebSocketClient::new(
            config.ws_private_url(),
            account_id,
            trader_id,
            config.heartbeat_interval_secs,
        );

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_orders,
            ws_stream_handle: None,
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    async fn authenticate(&self) -> anyhow::Result<String> {
        let api_key = self
            .config
            .api_key
            .clone()
            .or_else(|| std::env::var("AX_API_KEY").ok())
            .context("AX_API_KEY not configured")?;

        let api_secret = self
            .config
            .api_secret
            .clone()
            .or_else(|| std::env::var("AX_API_SECRET").ok())
            .context("AX_API_SECRET not configured")?;

        match self
            .http_client
            .authenticate(&api_key, &api_secret, 3600)
            .await
        {
            Ok(token) => Ok(token),
            Err(e) => {
                let totp_secret = self
                    .config
                    .totp_secret
                    .clone()
                    .or_else(|| std::env::var("AX_TOTP_SECRET").ok());

                if let Some(secret) = totp_secret {
                    log::info!("2FA required, generating TOTP code...");
                    let code = self.generate_totp(&secret)?;
                    self.http_client
                        .authenticate_with_totp(&api_key, &api_secret, 3600, Some(&code))
                        .await
                        .map_err(|e| anyhow::anyhow!("Authentication with 2FA failed: {e}"))
                } else {
                    Err(anyhow::anyhow!("Authentication failed: {e}"))
                }
            }
        }
    }

    fn generate_totp(&self, secret: &str) -> anyhow::Result<String> {
        let secret_bytes = Secret::Encoded(secret.to_string())
            .to_bytes()
            .map_err(|e| anyhow::anyhow!("Invalid TOTP secret: {e}"))?;

        let totp = TOTP::new(Algorithm::SHA1, 6, 1, 30, secret_bytes)
            .map_err(|e| anyhow::anyhow!("Invalid TOTP configuration: {e}"))?;

        totp.generate_current()
            .map_err(|e| anyhow::anyhow!("Failed to generate TOTP: {e}"))
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request AX account state")?;

        let ts_event = self.clock.get_time_ns();
        self.emitter.emit_account_state(
            account_state.balances.clone(),
            account_state.margins.clone(),
            account_state.is_reported,
            ts_event,
        );
        Ok(())
    }

    fn update_account_state(&self) -> anyhow::Result<()> {
        let runtime = get_runtime();
        runtime.block_on(self.refresh_account_state())
    }

    /// Calculates an aggressive limit price for market order simulation.
    ///
    /// Uses the best bid/ask from cached quote data with a conservative price band
    /// buffer to ensure the order fills immediately while staying within AX price bounds.
    fn calculate_market_order_price(
        &self,
        instrument_id: InstrumentId,
        order_side: OrderSide,
    ) -> anyhow::Result<Option<Price>> {
        // Use 3% band (conservative, as AX typically allows ~5%)
        const PRICE_BAND_PCT: f64 = 0.03;

        let cache = self.core.cache();

        let quote = cache.quote(&instrument_id).ok_or_else(|| {
            anyhow::anyhow!("Market order simulation requires cached quote for {instrument_id}")
        })?;

        let aggressive_price = match order_side {
            OrderSide::Buy => {
                // For BUY: use ask price + buffer to ensure fill
                let ask = quote.ask_price.as_f64();
                let price_value = ask * (1.0 + PRICE_BAND_PCT);
                Price::new(price_value, quote.ask_price.precision)
            }
            OrderSide::Sell => {
                // For SELL: use bid price - buffer to ensure fill
                let bid = quote.bid_price.as_f64();
                let price_value = bid * (1.0 - PRICE_BAND_PCT);
                Price::new(price_value, quote.bid_price.precision)
            }
            _ => {
                anyhow::bail!("Invalid order side for market simulation: {order_side:?}");
            }
        };

        log::debug!(
            "Market order simulation: {order_side:?} {instrument_id} aggressive_price={aggressive_price}"
        );

        Ok(Some(aggressive_price))
    }

    fn submit_order_impl(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        // Extract all needed fields in a single borrow scope
        let (
            client_order_id,
            strategy_id,
            instrument_id,
            order_side,
            order_type,
            quantity,
            trigger_price,
            time_in_force,
            is_post_only,
            limit_price,
        ) = {
            let cache = self.core.cache();
            let order = cache.order(&cmd.client_order_id).ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;
            (
                order.client_order_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.order_side(),
                order.order_type(),
                order.quantity(),
                order.trigger_price(),
                order.time_in_force(),
                order.is_post_only(),
                order.price(),
            )
        };

        // For market orders, calculate aggressive price from cached quote
        let price = if order_type == OrderType::Market {
            self.calculate_market_order_price(instrument_id, order_side)?
        } else {
            limit_price
        };

        let ws_orders = self.ws_orders.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let trader_id = self.core.trader_id;
        let ts_init = self.clock.get_time_ns();

        self.spawn_task("submit_order", async move {
            let result = ws_orders
                .submit_order(
                    trader_id,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    order_side,
                    order_type,
                    quantity,
                    time_in_force,
                    price,
                    trigger_price,
                    is_post_only,
                    ts_init,
                )
                .await
                .map_err(|e| anyhow::anyhow!("Submit order failed: {e}"));

            if let Err(e) = &result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    &format!("submit-order-error: {e}"),
                    ts_event,
                    false,
                );
                anyhow::bail!("{e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order_impl(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        let ws_orders = self.ws_orders.clone();

        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let strategy_id = cmd.strategy_id;

        self.spawn_task("cancel_order", async move {
            let result = ws_orders
                .cancel_order_command(instrument_id, client_order_id, venue_order_id)
                .await
                .map_err(|e| anyhow::anyhow!("Cancel order failed: {e}"));

            if let Err(e) = &result {
                let ts_event = clock.get_time_ns();
                emitter.emit_order_cancel_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                    &format!("cancel-order-error: {e}"),
                    ts_event,
                );
                anyhow::bail!("{e}");
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
impl ExecutionClient for AxExecutionClient {
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
        *AX_VENUE
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

        if !self.core.instruments_initialized() {
            let instruments = self
                .http_client
                .request_instruments(None, None)
                .await
                .context("failed to request AX instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned from AX");
            } else {
                log::info!("Loaded {} instruments", instruments.len());
                self.http_client.cache_instruments(instruments.clone());

                for instrument in instruments {
                    self.ws_orders.cache_instrument(instrument);
                }
            }
            self.core.set_instruments_initialized();
        }

        let token = self.authenticate().await?;
        self.ws_orders.connect(&token).await?;
        log::info!("Connected to orders WebSocket");

        if self.ws_stream_handle.is_none() {
            let stream = self.ws_orders.stream();
            let emitter = self.emitter.clone();

            let handle = get_runtime().spawn(async move {
                pin_mut!(stream);
                while let Some(message) = stream.next().await {
                    dispatch_ws_message(message, &emitter);
                }
            });
            self.ws_stream_handle = Some(handle);
        }

        let account_state = self
            .http_client
            .request_account_state(self.core.account_id)
            .await
            .context("failed to request AX account state")?;

        if !account_state.balances.is_empty() {
            log::info!(
                "Received account state with {} balance(s)",
                account_state.balances.len()
            );
        }
        self.emitter.send_account_state(account_state);

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

        self.ws_orders.close().await;

        if let Some(handle) = self.ws_stream_handle.take() {
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
            "query_order not implemented for AX execution client (client_order_id={})",
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

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();
        log::info!(
            "Started: client_id={}, account_id={}, is_sandbox={}",
            self.core.client_id,
            self.core.account_id,
            self.config.is_sandbox,
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
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn submit_order(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        // Hold single borrow for all cache access
        {
            let cache = self.core.cache();
            let order = cache.order(&cmd.client_order_id).ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            // For market orders, validate quote is cached before emitting OrderSubmitted
            if order.order_type() == OrderType::Market {
                let instrument_id = order.instrument_id();
                if cache.quote(&instrument_id).is_none() {
                    anyhow::bail!(
                        "Market order requires cached quote for {instrument_id} (quote not yet received)"
                    );
                }
            }

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        self.submit_order_impl(cmd)
    }

    fn submit_order_list(&self, cmd: &SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for AX execution client (got {} orders)",
            cmd.order_list.orders.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: &ModifyOrder) -> anyhow::Result<()> {
        log::warn!(
            "modify_order not yet implemented for AX execution client (client_order_id={})",
            cmd.client_order_id
        );
        Ok(())
    }

    fn cancel_order(&self, cmd: &CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_impl(cmd)
    }

    fn cancel_all_orders(&self, cmd: &CancelAllOrders) -> anyhow::Result<()> {
        let cache = self.core.cache();
        let open_orders = cache.orders_open(None, Some(&cmd.instrument_id), None, None, None);

        if open_orders.is_empty() {
            log::debug!("No open orders to cancel for {}", cmd.instrument_id);
            return Ok(());
        }

        log::debug!(
            "Canceling {} open orders for {}",
            open_orders.len(),
            cmd.instrument_id
        );

        let ts_init = self.clock.get_time_ns();

        for order in open_orders {
            let cancel_cmd = CancelOrder {
                trader_id: cmd.trader_id,
                client_id: cmd.client_id,
                strategy_id: cmd.strategy_id,
                instrument_id: order.instrument_id(),
                client_order_id: order.client_order_id(),
                venue_order_id: order.venue_order_id(),
                command_id: UUID4::new(),
                ts_init,
                params: None,
            };
            self.cancel_order_impl(&cancel_cmd)?;
        }

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: &BatchCancelOrders) -> anyhow::Result<()> {
        for cancel in &cmd.cancels {
            self.cancel_order_impl(cancel)?;
        }
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let mut reports = self
            .http_client
            .request_order_status_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

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
        let mut reports = self
            .http_client
            .request_order_status_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if cmd.open_only {
            reports.retain(|r| r.order_status.is_open());
        }

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
        let mut reports = self
            .http_client
            .request_fill_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
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
        let mut reports = self
            .http_client
            .request_position_reports(self.core.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

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

        let order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_now,
            false, // open_only
            None,  // instrument_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        let fill_cmd = GenerateFillReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            None, // venue_order_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        let position_cmd = GeneratePositionStatusReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

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
            *AX_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        Ok(Some(mass_status))
    }

    fn register_external_order(
        &self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        strategy_id: StrategyId,
        ts_init: UnixNanos,
    ) {
        self.ws_orders.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
            ts_init,
        );
    }
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(message: AxOrdersWsMessage, emitter: &ExecutionEventEmitter) {
    match message {
        AxOrdersWsMessage::Nautilus(message) => match message {
            NautilusExecWsMessage::OrderAccepted(event) => {
                log::debug!(
                    "Order accepted: {} {}",
                    event.client_order_id,
                    event.venue_order_id
                );
                emitter.send_order_event(OrderEventAny::Accepted(event));
            }
            NautilusExecWsMessage::OrderFilled(event) => {
                log::debug!(
                    "Order filled: {} {} @ {}",
                    event.client_order_id,
                    event.last_qty,
                    event.last_px
                );
                emitter.send_order_event(OrderEventAny::Filled(*event));
            }
            NautilusExecWsMessage::OrderCanceled(event) => {
                log::debug!("Order canceled: {}", event.client_order_id);
                emitter.send_order_event(OrderEventAny::Canceled(event));
            }
            NautilusExecWsMessage::OrderExpired(event) => {
                log::debug!("Order expired: {}", event.client_order_id);
                emitter.send_order_event(OrderEventAny::Expired(event));
            }
            NautilusExecWsMessage::OrderRejected(event) => {
                log::warn!("Order rejected: {}", event.client_order_id);
                emitter.send_order_event(OrderEventAny::Rejected(event));
            }
            NautilusExecWsMessage::OrderCancelRejected(event) => {
                log::warn!("Cancel rejected: {}", event.client_order_id);
                emitter.send_order_event(OrderEventAny::CancelRejected(event));
            }
            NautilusExecWsMessage::OrderStatusReports(reports) => {
                log::debug!("Order status reports: {}", reports.len());
                for report in reports {
                    emitter.send_order_status_report(report);
                }
            }
            NautilusExecWsMessage::FillReports(reports) => {
                log::debug!("Fill reports: {}", reports.len());
                for report in reports {
                    emitter.send_fill_report(report);
                }
            }
        },
        AxOrdersWsMessage::PlaceOrderResponse(resp) => {
            log::debug!(
                "Place order response: rid={} oid={}",
                resp.rid,
                resp.res.oid
            );
        }
        AxOrdersWsMessage::CancelOrderResponse(resp) => {
            log::debug!(
                "Cancel order response: rid={} accepted={}",
                resp.rid,
                resp.res.cxl_rx
            );
        }
        AxOrdersWsMessage::OpenOrdersResponse(resp) => {
            log::debug!("Open orders response: {} orders", resp.res.len());
        }
        AxOrdersWsMessage::Error(err) => {
            log::error!("WebSocket error: {}", err.message);
        }
        AxOrdersWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        AxOrdersWsMessage::Authenticated => {
            log::debug!("WebSocket authenticated");
        }
    }
}
