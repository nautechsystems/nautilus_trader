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
        Arc, Mutex, RwLock,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use anyhow::Context;
use async_trait::async_trait;
use futures_util::{StreamExt, pin_mut};
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
        AccountState, OrderCancelRejected, OrderCanceled, OrderEventAny, OrderModifyRejected,
        OrderRejected, OrderSubmitted, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    instruments::Instrument,
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::{
    http::{
        client::{BinanceFuturesHttpClient, BinanceFuturesInstrument},
        models::{BatchOrderResult, BinancePositionRisk},
        query::{
            BatchCancelItem, BinanceAllOrdersParamsBuilder, BinanceOpenOrdersParamsBuilder,
            BinanceOrderQueryParamsBuilder, BinancePositionRiskParamsBuilder,
            BinanceUserTradesParamsBuilder,
        },
    },
    websocket::{
        client::BinanceFuturesWebSocketClient,
        handler_exec::BinanceFuturesExecWsFeedHandler,
        messages::{ExecHandlerCommand, NautilusExecWsMessage},
    },
};
use crate::{
    common::{
        consts::BINANCE_VENUE,
        credential::resolve_credentials,
        enums::{BinancePositionSide, BinanceProductType},
    },
    config::BinanceExecClientConfig,
    futures::http::models::BinanceFuturesAccountInfo,
};

/// Listen key keepalive interval (30 minutes).
const LISTEN_KEY_KEEPALIVE_SECS: u64 = 30 * 60;

/// Live execution client for Binance Futures trading.
///
/// Implements the [`ExecutionClient`] trait for order management on Binance
/// USD-M and COIN-M Futures markets. Uses HTTP API for order operations and
/// WebSocket for real-time order updates via user data stream.
///
/// Uses a two-tier architecture with an execution handler that maintains
/// pending order maps for correlating WebSocket updates with order context.
#[derive(Debug)]
pub struct BinanceFuturesExecutionClient {
    clock: &'static AtomicTime,
    core: ExecutionClientCore,
    config: BinanceExecClientConfig,
    product_type: BinanceProductType,
    http_client: BinanceFuturesHttpClient,
    ws_client: Option<BinanceFuturesWebSocketClient>,
    exec_sender: tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    exec_cmd_tx: Option<tokio::sync::mpsc::UnboundedSender<ExecHandlerCommand>>,
    listen_key: Arc<RwLock<Option<String>>>,
    cancellation_token: CancellationToken,
    handler_signal: Arc<AtomicBool>,
    ws_task: Mutex<Option<JoinHandle<()>>>,
    keepalive_task: Mutex<Option<JoinHandle<()>>>,
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
        let product_type = config
            .product_types
            .iter()
            .find(|pt| matches!(pt, BinanceProductType::UsdM | BinanceProductType::CoinM))
            .copied()
            .unwrap_or(BinanceProductType::UsdM);

        let (api_key, api_secret) = resolve_credentials(
            config.api_key.clone(),
            config.api_secret.clone(),
            config.environment,
            product_type,
        )?;

        let http_client = BinanceFuturesHttpClient::new(
            product_type,
            config.environment,
            Some(api_key.clone()),
            Some(api_secret.clone()),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
        )
        .context("failed to construct Binance Futures HTTP client")?;

        let ws_client = BinanceFuturesWebSocketClient::new(
            product_type,
            config.environment,
            Some(api_key),
            Some(api_secret),
            config.base_url_ws.clone(),
            Some(20), // Heartbeat interval
        )
        .context("failed to construct Binance Futures WebSocket client")?;

        let clock = get_atomic_clock_realtime();
        let exec_sender = get_exec_event_sender();

        Ok(Self {
            clock,
            core,
            config,
            product_type,
            http_client,
            ws_client: Some(ws_client),
            exec_sender,
            exec_cmd_tx: None,
            listen_key: Arc::new(RwLock::new(None)),
            cancellation_token: CancellationToken::new(),
            handler_signal: Arc::new(AtomicBool::new(false)),
            ws_task: Mutex::new(None),
            keepalive_task: Mutex::new(None),
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
                let wallet_balance: f64 = b.wallet_balance.parse().unwrap_or(0.0);
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

    /// Handles execution events from the handler.
    ///
    /// The handler has already correlated WebSocket updates with order context
    /// (strategy_id, etc.) and emits normalized Nautilus events.
    fn handle_exec_event(
        message: NautilusExecWsMessage,
        exec_sender: &tokio::sync::mpsc::UnboundedSender<ExecutionEvent>,
    ) {
        match message {
            NautilusExecWsMessage::OrderAccepted(event) => {
                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Accepted(event)))
                {
                    log::warn!("Failed to send OrderAccepted event: {e}");
                }
            }
            NautilusExecWsMessage::OrderCanceled(event) => {
                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Canceled(event)))
                {
                    log::warn!("Failed to send OrderCanceled event: {e}");
                }
            }
            NautilusExecWsMessage::OrderRejected(event) => {
                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Rejected(event)))
                {
                    log::warn!("Failed to send OrderRejected event: {e}");
                }
            }
            NautilusExecWsMessage::OrderFilled(event) => {
                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Filled(event)))
                {
                    log::warn!("Failed to send OrderFilled event: {e}");
                }
            }
            NautilusExecWsMessage::OrderUpdated(event) => {
                if let Err(e) =
                    exec_sender.send(ExecutionEvent::Order(OrderEventAny::Updated(event)))
                {
                    log::warn!("Failed to send OrderUpdated event: {e}");
                }
            }
            NautilusExecWsMessage::AccountUpdate(event) => {
                if let Err(e) = exec_sender.send(ExecutionEvent::Account(event)) {
                    log::warn!("Failed to send AccountState event: {e}");
                }
            }
            NautilusExecWsMessage::ListenKeyExpired => {
                log::warn!("Listen key expired - reconnection required");
            }
            NautilusExecWsMessage::Reconnected => {
                log::info!("User data stream WebSocket reconnected");
            }
        }
    }

    /// Registers an order with the execution handler for context tracking.
    fn register_order(&self, order: &OrderAny) {
        if let Some(ref cmd_tx) = self.exec_cmd_tx {
            let cmd = ExecHandlerCommand::RegisterOrder {
                client_order_id: order.client_order_id(),
                trader_id: order.trader_id(),
                strategy_id: order.strategy_id(),
                instrument_id: order.instrument_id(),
            };
            if let Err(e) = cmd_tx.send(cmd) {
                log::error!("Failed to register order with handler: {e}");
            }
        }
    }

    /// Registers a cancel request with the execution handler for context tracking.
    fn register_cancel(
        &self,
        client_order_id: ClientOrderId,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        venue_order_id: Option<VenueOrderId>,
    ) {
        if let Some(ref cmd_tx) = self.exec_cmd_tx {
            let cmd = ExecHandlerCommand::RegisterCancel {
                client_order_id,
                trader_id,
                strategy_id,
                instrument_id,
                venue_order_id,
            };
            if let Err(e) = cmd_tx.send(cmd) {
                log::error!("Failed to register cancel with handler: {e}");
            }
        }
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();

        let order = self.core.get_order(&cmd.client_order_id)?;

        // Register order with handler for context tracking before HTTP request
        self.register_order(&order);

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
        let reduce_only = order.is_reduce_only();
        let position_side = self.determine_position_side(order_side, reduce_only);
        let clock = self.clock;

        // HTTP only generates OrderRejected on failure.
        // OrderAccepted comes from WebSocket user data stream ORDER_TRADE_UPDATE.
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
                    log::debug!(
                        "Order submit accepted: client_order_id={}, venue_order_id={}",
                        client_order_id,
                        report.venue_order_id
                    );
                }
                Err(e) => {
                    // Keep order registered - if HTTP failed due to timeout but order
                    // reached Binance, WebSocket updates will still arrive. The order
                    // will be cleaned up via WebSocket rejection or reconciliation.
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

                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                        OrderEventAny::Rejected(rejected_event),
                    )) {
                        log::warn!("Failed to send OrderRejected event: {e}");
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

        // Register cancel with handler for context tracking before HTTP request
        self.register_cancel(
            command.client_order_id,
            self.core.trader_id,
            command.strategy_id,
            command.instrument_id,
            command.venue_order_id,
        );

        let exec_sender = self.exec_sender.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let ts_init = cmd.ts_init;
        let instrument_id = command.instrument_id;
        let venue_order_id = command.venue_order_id;
        let client_order_id = Some(command.client_order_id);
        let clock = self.clock;

        // HTTP only generates OrderCancelRejected on failure.
        // OrderCanceled comes from WebSocket user data stream ORDER_TRADE_UPDATE.
        self.spawn_task("cancel_order", async move {
            let result = http_client
                .cancel_order(instrument_id, venue_order_id, client_order_id)
                .await;

            match result {
                Ok(venue_order_id) => {
                    log::debug!(
                        "Cancel request accepted: client_order_id={}, venue_order_id={}",
                        command.client_order_id,
                        venue_order_id
                    );
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

        // Reinitialize cancellation token in case of reconnection
        self.cancellation_token = CancellationToken::new();

        // Check hedge mode
        let is_hedge_mode = self
            .init_hedge_mode()
            .await
            .context("failed to query hedge mode")?;
        self.is_hedge_mode.store(is_hedge_mode, Ordering::Release);
        log::info!("Hedge mode (dual side position): {is_hedge_mode}");

        // Load instruments if not already done
        let _instruments = if self.instruments_initialized.load(Ordering::Acquire) {
            Vec::new()
        } else {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request Binance Futures instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for Binance Futures");
            } else {
                log::info!("Loaded {} Futures instruments", instruments.len());

                let cache = self.core.cache();
                for instrument in &instruments {
                    if let Err(e) = cache.borrow_mut().add_instrument(instrument.clone()) {
                        log::debug!("Instrument already in cache: {e}");
                    }
                }
            }

            self.instruments_initialized.store(true, Ordering::Release);
            instruments
        };

        // Create listen key for user data stream
        log::info!("Creating listen key for user data stream...");
        let listen_key_response = self
            .http_client
            .create_listen_key()
            .await
            .context("failed to create listen key")?;
        let listen_key = listen_key_response.listen_key;
        log::info!("Listen key created successfully");

        {
            let mut key_guard = self.listen_key.write().expect(MUTEX_POISONED);
            *key_guard = Some(listen_key.clone());
        }

        // Connect WebSocket and set up execution handler
        if let Some(ref mut ws_client) = self.ws_client {
            log::info!("Connecting to Binance Futures user data stream WebSocket...");
            ws_client.connect().await.map_err(|e| {
                log::error!("Binance Futures WebSocket connection failed: {e:?}");
                anyhow::anyhow!("failed to connect Binance Futures WebSocket: {e}")
            })?;
            log::info!("Binance Futures WebSocket connected");

            // Subscribe to user data stream using listen key
            log::info!("Subscribing to user data stream...");
            ws_client
                .subscribe(vec![listen_key.clone()])
                .await
                .map_err(|e| anyhow::anyhow!("failed to subscribe to user data stream: {e}"))?;
            log::info!("Subscribed to user data stream");

            // Create channels for the execution handler
            let (cmd_tx, cmd_rx) = tokio::sync::mpsc::unbounded_channel();
            let (raw_tx, raw_rx) = tokio::sync::mpsc::unbounded_channel();

            // Store command channel for order registration
            self.exec_cmd_tx = Some(cmd_tx.clone());

            // Create and initialize the execution handler
            let mut handler = BinanceFuturesExecWsFeedHandler::new(
                self.clock,
                self.core.trader_id,
                self.core.account_id,
                self.core.account_type,
                self.product_type,
                self.handler_signal.clone(),
                cmd_rx,
                raw_rx,
            );

            // Initialize handler with instruments
            let instruments_for_handler: Vec<BinanceFuturesInstrument> = self
                .http_client
                .instruments_cache()
                .iter()
                .map(|r| r.value().clone())
                .collect();
            if let Err(e) = cmd_tx.send(ExecHandlerCommand::InitializeInstruments(
                instruments_for_handler,
            )) {
                log::error!("Failed to send instruments to handler: {e}");
            }

            // Set up raw message forwarding from WebSocket to handler
            let stream = ws_client.stream();
            let cancel = self.cancellation_token.clone();
            let raw_forward_task = get_runtime().spawn(async move {
                pin_mut!(stream);
                loop {
                    tokio::select! {
                        Some(message) = stream.next() => {
                            if let Err(e) = raw_tx.send(message) {
                                log::error!("Failed to forward raw message to handler: {e}");
                                break;
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Raw message forwarding task cancelled");
                            break;
                        }
                    }
                }
            });

            // Start handler processing task
            let exec_sender = self.exec_sender.clone();
            let handler_cancel = self.cancellation_token.clone();
            let ws_task = get_runtime().spawn(async move {
                loop {
                    tokio::select! {
                        msg = handler.next() => {
                            match msg {
                                Some(event) => {
                                    Self::handle_exec_event(event, &exec_sender);
                                }
                                None => break,
                            }
                        }
                        () = handler_cancel.cancelled() => {
                            log::debug!("Handler task cancelled");
                            break;
                        }
                    }
                }

                // Clean up raw forwarding task
                raw_forward_task.abort();
            });
            *self.ws_task.lock().expect(MUTEX_POISONED) = Some(ws_task);

            // Start listen key keepalive task
            let http_client = self.http_client.clone();
            let listen_key_ref = self.listen_key.clone();
            let cancel = self.cancellation_token.clone();

            let keepalive_task = get_runtime().spawn(async move {
                let mut interval =
                    tokio::time::interval(Duration::from_secs(LISTEN_KEY_KEEPALIVE_SECS));
                loop {
                    tokio::select! {
                        _ = interval.tick() => {
                            let key = {
                                let guard = listen_key_ref.read().expect(MUTEX_POISONED);
                                guard.clone()
                            };
                            if let Some(ref key) = key {
                                match http_client.keepalive_listen_key(key).await {
                                    Ok(()) => {
                                        log::debug!("Listen key keepalive sent successfully");
                                    }
                                    Err(e) => {
                                        log::warn!("Listen key keepalive failed: {e}");
                                    }
                                }
                            }
                        }
                        () = cancel.cancelled() => {
                            log::debug!("Listen key keepalive task cancelled");
                            break;
                        }
                    }
                }
            });
            *self.keepalive_task.lock().expect(MUTEX_POISONED) = Some(keepalive_task);
        }

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

        if let Err(e) = self
            .exec_sender
            .send(ExecutionEvent::Account(account_state))
        {
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

        // Cancel all background tasks
        self.cancellation_token.cancel();

        // Wait for WebSocket task to complete
        let ws_task = self.ws_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = ws_task {
            let _ = task.await;
        }

        // Wait for keepalive task to complete
        let keepalive_task = self.keepalive_task.lock().expect(MUTEX_POISONED).take();
        if let Some(task) = keepalive_task {
            let _ = task.await;
        }

        // Close WebSocket
        if let Some(ref mut ws_client) = self.ws_client {
            let _ = ws_client.close().await;
        }

        // Close listen key
        let listen_key = self.listen_key.read().expect(MUTEX_POISONED).clone();
        if let Some(ref key) = listen_key
            && let Err(e) = self.http_client.close_listen_key(key).await
        {
            log::warn!("Failed to close listen key: {e}");
        }
        *self.listen_key.write().expect(MUTEX_POISONED) = None;

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
                        format!("modify-order-failed: {e}").into(),
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

        // HTTP cancel_all_orders only confirms the request was accepted.
        // Actual OrderCanceled events come from WebSocket user data stream.
        self.spawn_task("cancel_all_orders", async move {
            match http_client.cancel_all_orders(instrument_id).await {
                Ok(_) => {
                    log::info!("Cancel all orders request accepted for {instrument_id}");
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

                                    if let Err(e) = exec_sender.send(ExecutionEvent::Order(
                                        OrderEventAny::Canceled(canceled_event),
                                    )) {
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
