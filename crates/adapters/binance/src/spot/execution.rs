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
    sync::{Arc, Mutex},
    time::Duration,
};

use ahash::AHashMap;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::fifo::FifoCache,
    clients::ExecutionClient,
    live::{get_runtime, runner::get_exec_event_sender},
    messages::execution::{
        BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
        GenerateOrderStatusReport, GenerateOrderStatusReports, GenerateOrderStatusReportsBuilder,
        GeneratePositionStatusReports, GeneratePositionStatusReportsBuilder, ModifyOrder,
        QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
    },
};
use nautilus_core::{
    MUTEX_POISONED, UUID4, UnixNanos,
    datetime::mins_to_nanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    enums::{LiquiditySide, OmsType, OrderType},
    events::{
        AccountState, OrderAccepted, OrderCancelRejected, OrderCanceled, OrderEventAny,
        OrderFilled, OrderModifyRejected, OrderRejected, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    instruments::Instrument,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use tokio::task::JoinHandle;
use ustr::Ustr;

use super::websocket::trading::{
    client::BinanceSpotWsTradingClient,
    messages::BinanceSpotWsTradingMessage,
    parse::{
        parse_spot_account_position, parse_spot_exec_report_to_fill,
        parse_spot_exec_report_to_order_status,
    },
    user_data::{BinanceSpotExecutionReport, BinanceSpotExecutionType},
};
use crate::{
    common::{
        consts::{
            BINANCE_GTX_ORDER_REJECT_CODE, BINANCE_NAUTILUS_SPOT_BROKER_ID,
            BINANCE_NEW_ORDER_REJECTED_CODE, BINANCE_SPOT_POST_ONLY_REJECT_MSG, BINANCE_VENUE,
        },
        credential::resolve_credentials,
        dispatch::{
            OrderIdentity, PendingOperation, PendingRequest, WsDispatchState,
            ensure_accepted_emitted,
        },
        encoder::{decode_broker_id, encode_broker_id},
        enums::{BinanceProductType, BinanceSide, BinanceTimeInForce},
    },
    config::BinanceExecClientConfig,
    spot::{
        enums::{
            BinanceCancelReplaceMode, BinanceOrderResponseType, BinanceSpotOrderType,
            order_type_to_binance_spot, time_in_force_to_binance_spot,
        },
        http::{
            client::BinanceSpotHttpClient,
            models::BatchCancelResult,
            query::{BatchCancelItem, CancelOrderParams, CancelReplaceOrderParams, NewOrderParams},
        },
    },
};

/// Live execution client for Binance Spot trading.
///
/// Implements the [`ExecutionClient`] trait for order management on Binance Spot
/// and Spot Margin markets. Uses WebSocket API as the primary transport for order
/// operations (lowest latency), with HTTP API fallback when the WS connection is
/// unavailable. The WebSocket User Data Stream provides real-time execution events.
#[derive(Debug)]
pub struct BinanceSpotExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: BinanceExecClientConfig,
    emitter: ExecutionEventEmitter,
    dispatch_state: Arc<WsDispatchState>,
    http_client: BinanceSpotHttpClient,
    ws_trading_client: Option<BinanceSpotWsTradingClient>,
    ws_trading_handle: Mutex<Option<JoinHandle<()>>>,
    ws_authenticated: Arc<tokio::sync::Notify>,
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

        let clock = get_atomic_clock_realtime();

        let http_client = BinanceSpotHttpClient::new(
            config.environment,
            clock,
            Some(api_key.clone()),
            Some(api_secret.clone()),
            config.base_url_http.clone(),
            None, // recv_window
            None, // timeout_secs
            None, // proxy_url
        )
        .context("failed to construct Binance Spot HTTP client")?;
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        let ws_trading_client = if config.use_ws_trading {
            Some(BinanceSpotWsTradingClient::new(
                config.base_url_ws_trading.clone(),
                api_key,
                api_secret,
                None, // heartbeat
                config.transport_backend,
            ))
        } else {
            None
        };

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            dispatch_state: Arc::new(WsDispatchState::default()),
            http_client,
            ws_trading_client,
            ws_trading_handle: Mutex::new(None),
            ws_authenticated: Arc::new(tokio::sync::Notify::new()),
            pending_tasks: Mutex::new(Vec::new()),
        })
    }

    async fn refresh_account_state(&self) -> anyhow::Result<AccountState> {
        self.http_client
            .request_account_state(self.core.account_id)
            .await
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_account", async move {
            let account_state = http_client.request_account_state(account_id).await?;
            let ts_now = clock.get_time_ns();
            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                ts_now,
            );
            Ok(())
        });
    }

    /// Returns whether the WS trading client is connected and active.
    fn ws_trading_active(&self) -> bool {
        self.ws_trading_client
            .as_ref()
            .is_some_and(|c| c.is_active())
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

        let event_emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
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
        let is_quote_quantity = order.is_quote_quantity();
        let display_qty = order.display_qty();
        let clock = self.clock;
        let ts_init = self.clock.get_time_ns();

        // Register identity for tracked/external dispatch routing
        self.dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id,
                order_side,
                order_type,
                price,
            },
        );

        if self.ws_trading_active() {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();
            let params =
                build_new_order_params(&order, client_order_id, is_post_only, is_quote_quantity)?;

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id,
                    venue_order_id: None,
                    operation: PendingOperation::Place,
                },
            );

            self.spawn_task("submit_order_ws", async move {
                if let Err(e) = ws_client
                    .place_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let rejected = OrderRejected::new(
                        trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        account_id,
                        format!("ws-submit-order-error: {e}").into(),
                        UUID4::new(),
                        ts_init,
                        clock.get_time_ns(),
                        false,
                        false,
                    );
                    event_emitter.send_order_event(OrderEventAny::Rejected(rejected));
                    anyhow::bail!("WS submit order failed: {e}");
                }
                Ok(())
            });
        } else {
            let http_client = self.http_client.clone();
            let dispatch_state = self.dispatch_state.clone();
            log::debug!("WS trading not active, falling back to HTTP for submit_order");

            self.spawn_task("submit_order_http", async move {
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
                        is_quote_quantity,
                        display_qty,
                    )
                    .await;

                match result {
                    Ok(report) => {
                        dispatch_state.insert_accepted(client_order_id);
                        let accepted = OrderAccepted::new(
                            trader_id,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            report.venue_order_id,
                            account_id,
                            UUID4::new(),
                            ts_init,
                            ts_init,
                            false,
                        );
                        event_emitter.send_order_event(OrderEventAny::Accepted(accepted));
                    }
                    Err(e) => {
                        let due_post_only = e
                            .downcast_ref::<crate::spot::http::BinanceSpotHttpError>()
                            .is_some_and(is_spot_post_only_rejection);
                        dispatch_state.cleanup_terminal(client_order_id);
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
                            due_post_only,
                        );
                        event_emitter.send_order_event(OrderEventAny::Rejected(rejected));
                        return Err(e);
                    }
                }
                Ok(())
            });
        }

        Ok(())
    }

    fn cancel_order_internal(&self, cmd: &CancelOrder) {
        let event_emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;
        let command = cmd.clone();

        if self.ws_trading_active() {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();
            let params = build_cancel_order_params(&command);

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id: command.client_order_id,
                    venue_order_id: command.venue_order_id,
                    operation: PendingOperation::Cancel,
                },
            );

            self.spawn_task("cancel_order_ws", async move {
                if let Err(e) = ws_client
                    .cancel_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let ts_now = clock.get_time_ns();
                    let rejected_event = OrderCancelRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("ws-cancel-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );
                    event_emitter.send_order_event(OrderEventAny::CancelRejected(rejected_event));
                    anyhow::bail!("WS cancel order failed: {e}");
                }
                Ok(())
            });
        } else {
            let http_client = self.http_client.clone();
            let dispatch_state = self.dispatch_state.clone();
            log::debug!("WS trading not active, falling back to HTTP for cancel_order");

            self.spawn_task("cancel_order_http", async move {
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
                        dispatch_state.cleanup_terminal(command.client_order_id);
                        let ts_now = clock.get_time_ns();
                        let canceled_event = OrderCanceled::new(
                            trader_id,
                            command.strategy_id,
                            command.instrument_id,
                            command.client_order_id,
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            Some(venue_order_id),
                            Some(account_id),
                        );
                        event_emitter.send_order_event(OrderEventAny::Canceled(canceled_event));
                    }
                    Err(e) => {
                        let ts_now = clock.get_time_ns();
                        let rejected_event = OrderCancelRejected::new(
                            trader_id,
                            command.strategy_id,
                            command.instrument_id,
                            command.client_order_id,
                            format!("cancel-order-error: {e}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            command.venue_order_id,
                            Some(account_id),
                        );
                        event_emitter
                            .send_order_event(OrderEventAny::CancelRejected(rejected_event));
                        return Err(e);
                    }
                }
                Ok(())
            });
        }
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        crate::common::execution::spawn_task(&self.pending_tasks, description, fut);
    }

    fn abort_pending_tasks(&self) {
        crate::common::execution::abort_pending_tasks(&self.pending_tasks);
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BinanceSpotExecutionClient {
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
        *BINANCE_VENUE
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

        // Load instruments if not already done
        if !self.core.instruments_initialized() {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request Binance Spot instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned for Binance Spot");
            } else {
                log::info!("Loaded {} Spot instruments", instruments.len());
                self.http_client.cache_instruments(instruments);
            }

            self.core.set_instruments_initialized();
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

        self.emitter.send_account_state(account_state);

        // Wait for account to be registered in cache before completing connect
        crate::common::execution::await_account_registered(&self.core, self.core.account_id, 30.0)
            .await?;

        // Connect WS trading client (primary order transport)
        if let Some(ref mut ws_trading) = self.ws_trading_client {
            match ws_trading.connect().await {
                Ok(()) => {
                    log::info!("Connected to Binance Spot WS trading API");

                    let ws_trading_clone = ws_trading.clone();
                    let emitter = self.emitter.clone();
                    let account_id = self.core.account_id;
                    let clock = self.clock;
                    let http_client = self.http_client.clone();
                    let dispatch_state = self.dispatch_state.clone();
                    let ws_authenticated = self.ws_authenticated.clone();
                    let seen_trade_ids = std::sync::Arc::new(Mutex::new(FifoCache::new()));

                    let handle = get_runtime().spawn(async move {
                        loop {
                            match ws_trading_clone.recv().await {
                                Some(msg) => {
                                    dispatch_ws_trading_message(
                                        msg,
                                        &emitter,
                                        &http_client,
                                        account_id,
                                        clock,
                                        &dispatch_state,
                                        &ws_authenticated,
                                        &seen_trade_ids,
                                    );
                                }
                                None => {
                                    log::warn!("WS trading dispatch loop ended");
                                    break;
                                }
                            }
                        }
                    });

                    *self.ws_trading_handle.lock().expect(MUTEX_POISONED) = Some(handle);

                    // Block until session is authenticated before signaling connected
                    if let Err(e) = ws_trading.session_logon().await {
                        log::error!("WS session logon failed: {e}");
                    } else {
                        let auth_result = tokio::time::timeout(
                            Duration::from_secs(10),
                            self.ws_authenticated.notified(),
                        )
                        .await;

                        if auth_result.is_err() {
                            log::error!(
                                "WS session authentication timed out, \
                                 order operations will use HTTP fallback"
                            );

                            if let Some(handle) =
                                self.ws_trading_handle.lock().expect(MUTEX_POISONED).take()
                            {
                                handle.abort();
                            }
                            ws_trading.disconnect().await;
                            self.ws_trading_client = None;
                        } else if let Err(e) = ws_trading.subscribe_user_data().await {
                            log::error!("WS user data subscribe failed: {e}");
                        }
                    }
                }
                Err(e) => {
                    log::error!(
                        "Failed to connect WS trading API: {e}. \
                         Order operations will use HTTP fallback"
                    );
                }
            }
        }

        self.core.set_connected();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        // Abort WS trading task and disconnect
        if let Some(handle) = self.ws_trading_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Some(ref mut ws_trading) = self.ws_trading_client {
            ws_trading.disconnect().await;
        }

        self.abort_pending_tasks();

        self.core.set_disconnected();
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        log::debug!("query_order: client_order_id={}", cmd.client_order_id);

        let http_client = self.http_client.clone();
        let command = cmd;
        let event_emitter = self.emitter.clone();
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
                    event_emitter.send_order_status_report(report);
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
        if self.core.is_stopped() {
            return Ok(());
        }

        // Abort WS trading task
        if let Some(handle) = self.ws_trading_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        self.core.set_stopped();
        self.core.set_disconnected();
        self.abort_pending_tasks();
        log::info!("Stopped: client_id={}", self.core.client_id);
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
            let lookback_ns = mins_to_nanos(mins);
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

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .cloned()
            .ok_or_else(|| anyhow::anyhow!("Order not found: {}", cmd.client_order_id))?;

        if order.is_closed() {
            let client_order_id = order.client_order_id();
            log::warn!("Cannot submit closed order {client_order_id}");
            return Ok(());
        }

        log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
        self.emitter.emit_order_submitted(&order);

        self.submit_order_internal(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        log::warn!(
            "submit_order_list not yet implemented for Binance Spot execution client (received {} orders)",
            cmd.order_list.client_order_ids.len()
        );
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        // Binance Spot uses cancel-replace for order modification, which requires
        // the full order specification (side, type, time_in_force). Since ModifyOrder
        // doesn't include these fields, we need to look up the original order from cache.
        let order = self.core.cache().order(&cmd.client_order_id).cloned();

        let Some(order) = order else {
            log::warn!(
                "Cannot modify order {}: not found in cache",
                cmd.client_order_id
            );
            let ts_init = self.clock.get_time_ns();
            let rejected_event = OrderModifyRejected::new(
                self.core.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                "Order not found in cache for modify".into(),
                UUID4::new(),
                ts_init, // no venue timestamp, rejected locally
                ts_init,
                false,
                cmd.venue_order_id,
                Some(self.core.account_id),
            );

            self.emitter
                .send_order_event(OrderEventAny::ModifyRejected(rejected_event));
            return Ok(());
        };

        let event_emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;

        let order_side = order.order_side();
        let order_type = order.order_type();
        let time_in_force = order.time_in_force();
        let quantity = cmd.quantity.unwrap_or_else(|| order.quantity());

        if self.ws_trading_active() {
            let command = cmd;
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let dispatch_state = self.dispatch_state.clone();
            let params = build_cancel_replace_params(&command, &order, quantity)?;

            // Pre-register before sending to avoid response racing the insert
            let request_id = ws_client.next_request_id();
            dispatch_state.pending_requests.insert(
                request_id.clone(),
                PendingRequest {
                    client_order_id: command.client_order_id,
                    venue_order_id: command.venue_order_id,
                    operation: PendingOperation::Modify,
                },
            );

            self.spawn_task("modify_order_ws", async move {
                if let Err(e) = ws_client
                    .cancel_replace_order_with_id(request_id.clone(), params)
                    .await
                {
                    dispatch_state.pending_requests.remove(&request_id);
                    let ts_now = clock.get_time_ns();
                    let rejected_event = OrderModifyRejected::new(
                        trader_id,
                        command.strategy_id,
                        command.instrument_id,
                        command.client_order_id,
                        format!("ws-modify-order-error: {e}").into(),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        command.venue_order_id,
                        Some(account_id),
                    );
                    event_emitter.send_order_event(OrderEventAny::ModifyRejected(rejected_event));
                    anyhow::bail!("WS modify order failed: {e}");
                }
                Ok(())
            });
        } else {
            let command = cmd;
            let http_client = self.http_client.clone();
            log::debug!("WS trading not active, falling back to HTTP for modify_order");

            self.spawn_task("modify_order_http", async move {
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
                        let ts_now = clock.get_time_ns();
                        let updated_event = OrderUpdated::new(
                            trader_id,
                            command.strategy_id,
                            command.instrument_id,
                            command.client_order_id,
                            report.quantity,
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            Some(report.venue_order_id),
                            Some(account_id),
                            report.price,
                            None,  // trigger_price
                            None,  // protection_price
                            false, // is_quote_quantity
                        );
                        event_emitter.send_order_event(OrderEventAny::Updated(updated_event));
                    }
                    Err(e) => {
                        let ts_now = clock.get_time_ns();
                        let rejected_event = OrderModifyRejected::new(
                            trader_id,
                            command.strategy_id,
                            command.instrument_id,
                            command.client_order_id,
                            format!("modify-order-error: {e}").into(),
                            UUID4::new(),
                            ts_now,
                            ts_now,
                            false,
                            command.venue_order_id,
                            Some(account_id),
                        );
                        event_emitter
                            .send_order_event(OrderEventAny::ModifyRejected(rejected_event));
                        return Err(e);
                    }
                }
                Ok(())
            });
        }

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let event_emitter = self.emitter.clone();
        let trader_id = self.core.trader_id;
        let account_id = self.core.account_id;
        let clock = self.clock;

        if self.ws_trading_active() {
            let ws_client = self.ws_trading_client.as_ref().unwrap().clone();
            let symbol = cmd.instrument_id.symbol.to_string();

            self.spawn_task("cancel_all_orders_ws", async move {
                if let Err(e) = ws_client.cancel_all_orders(symbol).await {
                    log::error!("WS cancel_all_orders failed: {e}");
                }
                // Individual cancel confirmations dispatched via WS trading message loop
                Ok(())
            });

            return Ok(());
        }

        log::debug!("WS trading not active, falling back to HTTP for cancel_all_orders");
        let http_client = self.http_client.clone();

        // Build strategy lookup from cache before spawning (cache is not Send)
        let strategy_lookup: AHashMap<ClientOrderId, StrategyId> = {
            let cache = self.core.cache();
            cache
                .orders_open(None, Some(&cmd.instrument_id), None, None, None)
                .into_iter()
                .map(|order| (order.client_order_id(), order.strategy_id()))
                .collect()
        };

        let command = cmd;
        self.spawn_task("cancel_all_orders_http", async move {
            let canceled_orders = http_client.cancel_all_orders(command.instrument_id).await?;

            for (venue_order_id, client_order_id) in canceled_orders {
                let strategy_id = strategy_lookup
                    .get(&client_order_id)
                    .copied()
                    .unwrap_or(command.strategy_id);

                let canceled_event = OrderCanceled::new(
                    trader_id,
                    strategy_id,
                    command.instrument_id,
                    client_order_id,
                    UUID4::new(),
                    command.ts_init,
                    clock.get_time_ns(),
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                );

                event_emitter.send_order_event(OrderEventAny::Canceled(canceled_event));
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        const BATCH_SIZE: usize = 5;

        if cmd.cancels.is_empty() {
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let command = cmd;

        let event_emitter = self.emitter.clone();
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
                                    encode_broker_id(
                                        &cancel.client_order_id,
                                        BINANCE_NAUTILUS_SPOT_BROKER_ID,
                                    ),
                                )
                            }
                        } else {
                            BatchCancelItem::by_client_order_id(
                                command.instrument_id.symbol.to_string(),
                                encode_broker_id(
                                    &cancel.client_order_id,
                                    BINANCE_NAUTILUS_SPOT_BROKER_ID,
                                ),
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

                                    event_emitter
                                        .send_order_event(OrderEventAny::Canceled(canceled_event));
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

                                    event_emitter.send_order_event(OrderEventAny::CancelRejected(
                                        rejected_event,
                                    ));
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

                            event_emitter
                                .send_order_event(OrderEventAny::CancelRejected(rejected_event));
                        }
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_ws_trading_message(
    msg: BinanceSpotWsTradingMessage,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceSpotHttpClient,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    ws_authenticated: &tokio::sync::Notify,
    seen_trade_ids: &std::sync::Arc<Mutex<FifoCache<(Ustr, i64), 10_000>>>,
) {
    match msg {
        BinanceSpotWsTradingMessage::OrderAccepted {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order accepted: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderAccepted event is synthesized from UDS executionReport (New)
        }
        BinanceSpotWsTradingMessage::OrderRejected {
            request_id,
            code,
            msg,
        } => {
            log::debug!("WS order rejected: request_id={request_id}, code={code}, msg={msg}");
            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id) {
                // Clone to drop the DashMap read guard before cleanup_terminal
                let identity = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
                    .map(|r| r.clone());

                if let Some(identity) = identity {
                    let code_i64 = i64::from(code);
                    let due_post_only = code_i64 == BINANCE_GTX_ORDER_REJECT_CODE
                        || (code_i64 == BINANCE_NEW_ORDER_REJECTED_CODE
                            && msg == BINANCE_SPOT_POST_ONLY_REJECT_MSG);
                    let ts_now = clock.get_time_ns();
                    let rejected = OrderRejected::new(
                        emitter.trader_id(),
                        identity.strategy_id,
                        identity.instrument_id,
                        pending.client_order_id,
                        account_id,
                        Ustr::from(&format!("code={code}: {msg}")),
                        UUID4::new(),
                        ts_now,
                        ts_now,
                        false,
                        due_post_only,
                    );
                    dispatch_state.cleanup_terminal(pending.client_order_id);
                    emitter.send_order_event(OrderEventAny::Rejected(rejected));
                } else {
                    log::warn!(
                        "No order identity for {}, cannot emit OrderRejected",
                        pending.client_order_id
                    );
                }
            } else {
                log::warn!("No pending request for {request_id}, cannot emit OrderRejected");
            }
        }
        BinanceSpotWsTradingMessage::OrderCanceled {
            request_id,
            response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS order canceled: request_id={request_id}, order_id={}",
                response.order_id
            );
            // OrderCanceled event is synthesized from UDS executionReport (Canceled)
        }
        BinanceSpotWsTradingMessage::CancelRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!("WS cancel rejected: request_id={request_id}, code={code}, msg={msg}");
            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderCancelRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::CancelRejected(rejected));
            }
        }
        BinanceSpotWsTradingMessage::CancelReplaceAccepted {
            request_id,
            cancel_response,
            new_order_response,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS cancel-replace accepted: request_id={request_id}, \
                 canceled_id={}, new_id={}",
                cancel_response.order_id,
                new_order_response.order_id,
            );
            // OrderUpdated event is synthesized from UDS executionReport (Replaced)
        }
        BinanceSpotWsTradingMessage::CancelReplaceRejected {
            request_id,
            code,
            msg,
        } => {
            log::warn!(
                "WS cancel-replace rejected: request_id={request_id}, code={code}, msg={msg}"
            );

            if let Some((_, pending)) = dispatch_state.pending_requests.remove(&request_id)
                && let Some(identity) = dispatch_state
                    .order_identities
                    .get(&pending.client_order_id)
            {
                let ts_now = clock.get_time_ns();
                let rejected = OrderModifyRejected::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    pending.client_order_id,
                    Ustr::from(&format!("code={code}: {msg}")),
                    UUID4::new(),
                    ts_now,
                    ts_now,
                    false,
                    pending.venue_order_id,
                    Some(account_id),
                );
                emitter.send_order_event(OrderEventAny::ModifyRejected(rejected));
            }
        }
        BinanceSpotWsTradingMessage::AllOrdersCanceled {
            request_id,
            responses,
        } => {
            dispatch_state.pending_requests.remove(&request_id);
            log::debug!(
                "WS all orders canceled: request_id={request_id}, count={}",
                responses.len()
            );
            // Individual OrderCanceled events arrive via UDS executionReport
        }
        BinanceSpotWsTradingMessage::UserDataSubscribed { subscription_id } => {
            log::info!("User data stream subscribed: id={subscription_id}");
        }
        BinanceSpotWsTradingMessage::ExecutionReport(report) => {
            let ts_init = clock.get_time_ns();
            dispatch_execution_report(
                &report,
                emitter,
                http_client,
                account_id,
                dispatch_state,
                seen_trade_ids,
                ts_init,
            );
        }
        BinanceSpotWsTradingMessage::AccountPosition(position) => {
            let ts_init = clock.get_time_ns();
            let state = parse_spot_account_position(&position, account_id, ts_init);
            emitter.send_account_state(state);
        }
        BinanceSpotWsTradingMessage::BalanceUpdate(update) => {
            log::info!(
                "Balance update: asset={}, delta={}",
                update.asset,
                update.delta,
            );
            let http_client = http_client.clone();
            let emitter = emitter.clone();

            get_runtime().spawn(async move {
                match http_client.request_account_state(account_id).await {
                    Ok(state) => emitter.send_account_state(state),
                    Err(e) => {
                        log::error!("Failed to refresh account state after balance update: {e}");
                    }
                }
            });
        }
        BinanceSpotWsTradingMessage::Connected => {
            log::info!("WS trading API connected");
        }
        BinanceSpotWsTradingMessage::Authenticated => {
            log::info!("WS trading API authenticated");
            ws_authenticated.notify_one();
        }
        BinanceSpotWsTradingMessage::Reconnected => {
            log::info!("WS trading API reconnected");
        }
        BinanceSpotWsTradingMessage::Error(err) => {
            log::error!("WS trading API error: {err}");
        }
    }
}

fn build_new_order_params(
    order: &impl Order,
    client_order_id: ClientOrderId,
    is_post_only: bool,
    is_quote_quantity: bool,
) -> anyhow::Result<NewOrderParams> {
    let binance_side = BinanceSide::try_from(order.order_side())?;
    let binance_order_type = order_type_to_binance_spot(order.order_type(), is_post_only)?;

    let requires_trigger = matches!(
        order.order_type(),
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    );

    if requires_trigger && order.trigger_price().is_none() {
        anyhow::bail!("Conditional orders require a trigger price");
    }

    let supports_tif = matches!(
        binance_order_type,
        BinanceSpotOrderType::Limit
            | BinanceSpotOrderType::StopLossLimit
            | BinanceSpotOrderType::TakeProfitLimit
    );
    let binance_tif = if supports_tif {
        Some(time_in_force_to_binance_spot(order.time_in_force())?)
    } else {
        None
    };

    let qty_str = order.quantity().to_string();
    let (base_qty, quote_qty) = if is_quote_quantity {
        (None, Some(qty_str))
    } else {
        (Some(qty_str), None)
    };

    let client_id_str = encode_broker_id(&client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);

    Ok(NewOrderParams {
        symbol: order.instrument_id().symbol.to_string(),
        side: binance_side,
        order_type: binance_order_type,
        time_in_force: binance_tif,
        quantity: base_qty,
        quote_order_qty: quote_qty,
        price: order.price().map(|p| p.to_string()),
        new_client_order_id: Some(client_id_str),
        stop_price: order.trigger_price().map(|p| p.to_string()),
        trailing_delta: None,
        iceberg_qty: order.display_qty().map(|q| q.to_string()),
        new_order_resp_type: Some(BinanceOrderResponseType::Full),
        self_trade_prevention_mode: None,
        strategy_id: None,
        strategy_type: None,
    })
}

fn build_cancel_order_params(cmd: &CancelOrder) -> CancelOrderParams {
    let order_id = cmd
        .venue_order_id
        .and_then(|id| id.inner().parse::<i64>().ok());

    if let Some(order_id) = order_id {
        CancelOrderParams::by_order_id(cmd.instrument_id.symbol.to_string(), order_id)
    } else {
        let client_id_str = encode_broker_id(&cmd.client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);
        CancelOrderParams::by_client_order_id(cmd.instrument_id.symbol.to_string(), client_id_str)
    }
}

fn build_cancel_replace_params(
    cmd: &ModifyOrder,
    order: &impl Order,
    quantity: Quantity,
) -> anyhow::Result<CancelReplaceOrderParams> {
    let binance_side = BinanceSide::try_from(order.order_side())?;
    let binance_order_type = order_type_to_binance_spot(order.order_type(), false)?;
    let binance_tif = time_in_force_to_binance_spot(order.time_in_force())?;

    let cancel_order_id: Option<i64> = cmd
        .venue_order_id
        .map(|id| {
            id.inner()
                .parse::<i64>()
                .map_err(|_| anyhow::anyhow!("Invalid venue order ID: {id}"))
        })
        .transpose()?;

    let client_id_str = encode_broker_id(&cmd.client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);

    Ok(CancelReplaceOrderParams {
        symbol: cmd.instrument_id.symbol.to_string(),
        side: binance_side,
        order_type: binance_order_type,
        cancel_replace_mode: BinanceCancelReplaceMode::StopOnFailure,
        time_in_force: Some(binance_tif),
        quantity: Some(quantity.to_string()),
        quote_order_qty: None,
        price: cmd.price.map(|p| p.to_string()),
        cancel_order_id,
        cancel_orig_client_order_id: if cancel_order_id.is_none() {
            Some(client_id_str.clone())
        } else {
            None
        },
        new_client_order_id: Some(client_id_str),
        stop_price: None,
        trailing_delta: None,
        iceberg_qty: None,
        new_order_resp_type: Some(BinanceOrderResponseType::Full),
        self_trade_prevention_mode: None,
    })
}

/// Dispatches a Spot execution report with tracked/untracked routing.
///
/// Tracked orders (with registered identity) produce proper order events.
/// Untracked orders fall back to execution reports for reconciliation.
fn dispatch_execution_report(
    report: &BinanceSpotExecutionReport,
    emitter: &ExecutionEventEmitter,
    http_client: &BinanceSpotHttpClient,
    account_id: AccountId,
    dispatch_state: &WsDispatchState,
    seen_trade_ids: &std::sync::Arc<Mutex<FifoCache<(Ustr, i64), 10_000>>>,
    ts_init: UnixNanos,
) {
    let symbol = report.symbol;
    let instrument_id = InstrumentId::new(symbol.into(), *BINANCE_VENUE);
    let (price_precision, size_precision) = http_client
        .get_instrument(&symbol)
        .map_or((8, 8), |i| (i.price_precision(), i.size_precision()));

    let client_order_id = ClientOrderId::new(decode_broker_id(
        &report.client_order_id,
        BINANCE_NAUTILUS_SPOT_BROKER_ID,
    ));

    let identity = dispatch_state
        .order_identities
        .get(&client_order_id)
        .map(|r| r.clone());

    if let Some(identity) = identity {
        dispatch_tracked_execution_report(
            report,
            emitter,
            account_id,
            dispatch_state,
            seen_trade_ids,
            client_order_id,
            &identity,
            instrument_id,
            price_precision,
            size_precision,
            ts_init,
        );
    } else {
        dispatch_untracked_execution_report(
            report,
            emitter,
            http_client,
            account_id,
            seen_trade_ids,
            instrument_id,
            price_precision,
            size_precision,
            ts_init,
        );
    }
}

/// Dispatches a tracked execution report as proper order events.
#[expect(clippy::too_many_arguments)]
fn dispatch_tracked_execution_report(
    report: &BinanceSpotExecutionReport,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    state: &WsDispatchState,
    seen_trade_ids: &std::sync::Arc<Mutex<FifoCache<(Ustr, i64), 10_000>>>,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) {
    let venue_order_id = VenueOrderId::new(report.order_id.to_string());
    let ts_event = UnixNanos::from_millis(report.event_time as u64);

    match report.execution_type {
        BinanceSpotExecutionType::New => {
            if state.has_filled(&client_order_id) {
                log::debug!("Skipping New for already-filled {client_order_id}");
                return;
            }

            if state.has_emitted_accepted(&client_order_id) {
                // Already accepted: this New is a cancel-replace result
                let price: f64 = report.price.parse().unwrap_or(0.0);
                let quantity: f64 = report.original_qty.parse().unwrap_or(0.0);
                let trigger_price: f64 = report.stop_price.parse().unwrap_or(0.0);
                let trigger = if trigger_price > 0.0 {
                    Some(Price::new(trigger_price, price_precision))
                } else {
                    None
                };
                let updated = OrderUpdated::new(
                    emitter.trader_id(),
                    identity.strategy_id,
                    identity.instrument_id,
                    client_order_id,
                    Quantity::new(quantity, size_precision),
                    UUID4::new(),
                    ts_event,
                    ts_init,
                    false,
                    Some(venue_order_id),
                    Some(account_id),
                    Some(Price::new(price, price_precision)),
                    trigger,
                    None,  // protection_price
                    false, // is_quote_quantity
                );
                emitter.send_order_event(OrderEventAny::Updated(updated));
                return;
            }
            state.insert_accepted(client_order_id);
            let accepted = OrderAccepted::new(
                emitter.trader_id(),
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
            );
            emitter.send_order_event(OrderEventAny::Accepted(accepted));
        }
        BinanceSpotExecutionType::Trade => {
            let dedup_key = (report.symbol, report.trade_id);
            let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
            let is_duplicate = guard.contains(&dedup_key);
            guard.add(dedup_key);
            drop(guard);

            if is_duplicate {
                log::debug!(
                    "Duplicate trade_id={} for {}, skipping",
                    report.trade_id,
                    report.symbol
                );
                return;
            }

            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );

            let last_qty: f64 = report.last_filled_qty.parse().unwrap_or(0.0);
            let last_px: f64 = report.last_filled_price.parse().unwrap_or(0.0);
            let commission: f64 = report.commission.parse().unwrap_or(0.0);
            let commission_currency = report
                .commission_asset
                .as_ref()
                .map_or_else(Currency::USDT, |a| {
                    Currency::get_or_create_crypto(a.as_str())
                });

            let liquidity_side = if report.is_maker {
                LiquiditySide::Maker
            } else {
                LiquiditySide::Taker
            };

            let filled = OrderFilled::new(
                emitter.trader_id(),
                identity.strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                TradeId::new(report.trade_id.to_string()),
                identity.order_side,
                identity.order_type,
                Quantity::new(last_qty, size_precision),
                Price::new(last_px, price_precision),
                commission_currency,
                liquidity_side,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                None,
                Some(Money::new(commission, commission_currency)),
            );

            state.insert_filled(client_order_id);
            emitter.send_order_event(OrderEventAny::Filled(filled));

            let cum_qty: f64 = report.cumulative_filled_qty.parse().unwrap_or(0.0);
            let orig_qty: f64 = report.original_qty.parse().unwrap_or(0.0);
            if (orig_qty - cum_qty) <= 0.0 {
                state.cleanup_terminal(client_order_id);
            }
        }
        BinanceSpotExecutionType::Replaced => {
            // Cancel-replace succeeded: the old order is being replaced.
            // The replacement NEW event follows with the new price/qty.
            log::debug!(
                "Order replaced: client_order_id={client_order_id}, venue_order_id={venue_order_id}"
            );
        }
        BinanceSpotExecutionType::Canceled
        | BinanceSpotExecutionType::Expired
        | BinanceSpotExecutionType::TradePrevention => {
            ensure_accepted_emitted(
                client_order_id,
                account_id,
                venue_order_id,
                identity,
                emitter,
                state,
                ts_init,
            );
            let canceled = OrderCanceled::new(
                emitter.trader_id(),
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(venue_order_id),
                Some(account_id),
            );
            state.cleanup_terminal(client_order_id);
            emitter.send_order_event(OrderEventAny::Canceled(canceled));
        }
        BinanceSpotExecutionType::Rejected => {
            let reason = if report.reject_reason.is_empty() {
                Ustr::from("Order rejected by venue")
            } else {
                Ustr::from(&report.reject_reason)
            };
            let due_post_only = report.time_in_force == BinanceTimeInForce::Gtx
                || (report.order_type == "LIMIT_MAKER"
                    && (report.reject_reason.is_empty() || report.reject_reason == "NONE"));
            state.cleanup_terminal(client_order_id);
            emitter.emit_order_rejected_event(
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                reason.as_str(),
                ts_init,
                due_post_only,
            );
        }
    }
}

/// Dispatches an untracked execution report as execution reports for reconciliation.
#[expect(clippy::too_many_arguments)]
fn dispatch_untracked_execution_report(
    report: &BinanceSpotExecutionReport,
    emitter: &ExecutionEventEmitter,
    _http_client: &BinanceSpotHttpClient,
    account_id: AccountId,
    seen_trade_ids: &std::sync::Arc<Mutex<FifoCache<(Ustr, i64), 10_000>>>,
    instrument_id: InstrumentId,
    price_precision: u8,
    size_precision: u8,
    ts_init: UnixNanos,
) {
    match report.execution_type {
        BinanceSpotExecutionType::Trade => {
            let dedup_key = (report.symbol, report.trade_id);
            let mut guard = seen_trade_ids.lock().expect(MUTEX_POISONED);
            let is_duplicate = guard.contains(&dedup_key);
            guard.add(dedup_key);
            drop(guard);

            if is_duplicate {
                log::debug!(
                    "Duplicate trade_id={} for {}, skipping",
                    report.trade_id,
                    report.symbol
                );
                return;
            }

            match parse_spot_exec_report_to_order_status(
                report,
                instrument_id,
                price_precision,
                size_precision,
                account_id,
                ts_init,
            ) {
                Ok(status) => emitter.send_order_status_report(status),
                Err(e) => log::error!("Failed to parse order status report: {e}"),
            }

            match parse_spot_exec_report_to_fill(
                report,
                instrument_id,
                price_precision,
                size_precision,
                account_id,
                ts_init,
            ) {
                Ok(fill) => emitter.send_fill_report(fill),
                Err(e) => log::error!("Failed to parse fill report: {e}"),
            }
        }
        BinanceSpotExecutionType::New
        | BinanceSpotExecutionType::Canceled
        | BinanceSpotExecutionType::Replaced
        | BinanceSpotExecutionType::Rejected
        | BinanceSpotExecutionType::Expired
        | BinanceSpotExecutionType::TradePrevention => {
            match parse_spot_exec_report_to_order_status(
                report,
                instrument_id,
                price_precision,
                size_precision,
                account_id,
                ts_init,
            ) {
                Ok(status) => emitter.send_order_status_report(status),
                Err(e) => log::error!("Failed to parse order status report: {e}"),
            }
        }
    }
}

// Checks for GTX (-5022) and spot LIMIT_MAKER (-2010 + specific message)
fn is_spot_post_only_rejection(error: &crate::spot::http::BinanceSpotHttpError) -> bool {
    match error {
        crate::spot::http::BinanceSpotHttpError::BinanceError { code, message } => {
            *code == BINANCE_GTX_ORDER_REJECT_CODE
                || (*code == BINANCE_NEW_ORDER_REJECTED_CODE
                    && message == BINANCE_SPOT_POST_ONLY_REJECT_MSG)
        }
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{AccountType, LiquiditySide, OrderSide},
        identifiers::{StrategyId, TraderId},
    };
    use rstest::rstest;

    use super::*;
    use crate::common::enums::BinanceEnvironment;

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_cancel_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT.BINANCE"),
        );
        let ws_authenticated = tokio::sync::Notify::new();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_state.pending_requests.insert(
            "req-cancel".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Cancel,
            },
        );

        dispatch_ws_trading_message(
            BinanceSpotWsTradingMessage::CancelRejected {
                request_id: "req-cancel".to_string(),
                code: -2011,
                msg: "Unknown order sent".to_string(),
            },
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
            &ws_authenticated,
            &seen_trade_ids,
        );

        assert!(dispatch_state.pending_requests.get("req-cancel").is_none());

        match rx
            .try_recv()
            .expect("Cancel rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-2011"));
            }
            other => panic!("Expected CancelRejected event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_ws_trading_message_emits_modify_rejected_and_clears_pending_request() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("TEST"),
            InstrumentId::from("BTCUSDT.BINANCE"),
        );
        let ws_authenticated = tokio::sync::Notify::new();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        dispatch_state.pending_requests.insert(
            "req-modify".to_string(),
            PendingRequest {
                client_order_id: ClientOrderId::from("TEST"),
                venue_order_id: Some(VenueOrderId::from("12345")),
                operation: PendingOperation::Modify,
            },
        );

        dispatch_ws_trading_message(
            BinanceSpotWsTradingMessage::CancelReplaceRejected {
                request_id: "req-modify".to_string(),
                code: -2021,
                msg: "Order cancel-replace partially failed".to_string(),
            },
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
            &ws_authenticated,
            &seen_trade_ids,
        );

        assert!(dispatch_state.pending_requests.get("req-modify").is_none());

        match rx
            .try_recv()
            .expect("Modify rejection event should be emitted")
        {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(event)) => {
                assert_eq!(event.client_order_id, ClientOrderId::from("TEST"));
                assert_eq!(event.account_id, Some(AccountId::from("BINANCE-001")));
                assert!(event.reason.as_str().contains("code=-2021"));
            }
            other => panic!("Expected ModifyRejected event, was {other:?}"),
        }
    }

    fn create_test_emitter(
        clock: &'static AtomicTime,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            AccountId::from("BINANCE-001"),
            AccountType::Cash,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    fn create_test_http_client(clock: &'static AtomicTime) -> BinanceSpotHttpClient {
        BinanceSpotHttpClient::new(
            BinanceEnvironment::Mainnet,
            clock,
            None,
            None,
            None,
            None,
            None,
            None,
        )
        .expect("Test HTTP client should be created")
    }

    fn create_tracked_dispatch_state(
        client_order_id: ClientOrderId,
        instrument_id: InstrumentId,
    ) -> WsDispatchState {
        let dispatch_state = WsDispatchState::default();
        dispatch_state.order_identities.insert(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("TEST-STRATEGY"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
                price: None,
            },
        );
        dispatch_state
    }

    #[rstest]
    #[case::gtx(
        crate::spot::http::BinanceSpotHttpError::BinanceError {
            code: BINANCE_GTX_ORDER_REJECT_CODE,
            message: "Order would immediately trigger.".to_string(),
        },
        true,
    )]
    #[case::spot_post_only(
        crate::spot::http::BinanceSpotHttpError::BinanceError {
            code: BINANCE_NEW_ORDER_REJECTED_CODE,
            message: BINANCE_SPOT_POST_ONLY_REJECT_MSG.to_string(),
        },
        true,
    )]
    #[case::new_order_rejected_other_message(
        crate::spot::http::BinanceSpotHttpError::BinanceError {
            code: BINANCE_NEW_ORDER_REJECTED_CODE,
            message: "Insufficient balance.".to_string(),
        },
        false,
    )]
    #[case::unrelated_code(
        crate::spot::http::BinanceSpotHttpError::BinanceError {
            code: -2011,
            message: "Unknown order sent.".to_string(),
        },
        false,
    )]
    #[case::non_binance_error(
        crate::spot::http::BinanceSpotHttpError::NetworkError("connection reset".to_string()),
        false,
    )]
    fn test_is_spot_post_only_rejection(
        #[case] error: crate::spot::http::BinanceSpotHttpError,
        #[case] expected: bool,
    ) {
        assert_eq!(is_spot_post_only_rejection(&error), expected);
    }

    #[rstest]
    fn test_dispatch_tracked_execution_report_trade_dedup() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("x-TD67BGP9-T0000000000000");
        let dispatch_state = create_tracked_dispatch_state(
            ClientOrderId::from("O-20200101-000000-000-000-0"),
            InstrumentId::from("ETHUSDT.BINANCE"),
        );
        let ws_authenticated = tokio::sync::Notify::new();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        let trade_json = crate::common::testing::load_fixture_string(
            "spot/user_data_json/execution_report_trade.json",
        );
        let report: BinanceSpotExecutionReport = serde_json::from_str(&trade_json).unwrap();

        dispatch_ws_trading_message(
            BinanceSpotWsTradingMessage::ExecutionReport(Box::new(report.clone())),
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
            &ws_authenticated,
            &seen_trade_ids,
        );
        dispatch_ws_trading_message(
            BinanceSpotWsTradingMessage::ExecutionReport(Box::new(report)),
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
            &ws_authenticated,
            &seen_trade_ids,
        );

        let mut events = Vec::new();
        while let Ok(event) = rx.try_recv() {
            events.push(event);
        }

        let fills: Vec<_> = events
            .iter()
            .filter(|e| matches!(e, ExecutionEvent::Order(OrderEventAny::Filled(_))))
            .collect();
        assert_eq!(fills.len(), 1, "duplicate trade should be deduped");

        match fills[0] {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
                assert_eq!(
                    fill.client_order_id,
                    ClientOrderId::from("O-20200101-000000-000-000-0"),
                );
                assert_eq!(fill.trade_id, TradeId::new("98765432"));
                assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
            }
            _ => unreachable!(),
        }
        let _ = client_order_id;
    }

    #[rstest]
    fn test_dispatch_tracked_execution_report_rejected_gtx_sets_post_only() {
        let clock = get_atomic_clock_realtime();
        let (emitter, mut rx) = create_test_emitter(clock);
        let http_client = create_test_http_client(clock);
        let client_order_id = ClientOrderId::from("O-20200101-000000-000-000-1");
        let dispatch_state =
            create_tracked_dispatch_state(client_order_id, InstrumentId::from("ETHUSDT.BINANCE"));
        let ws_authenticated = tokio::sync::Notify::new();
        let seen_trade_ids = Arc::new(Mutex::new(FifoCache::new()));

        let encoded = encode_broker_id(&client_order_id, BINANCE_NAUTILUS_SPOT_BROKER_ID);
        let report_json = format!(
            r#"{{
                "e":"executionReport","E":1709654400000,"s":"ETHUSDT",
                "c":"{encoded}","S":"BUY","o":"LIMIT","f":"GTX",
                "q":"1.00000000","p":"2500.00000000","P":"0.00000000",
                "x":"REJECTED","X":"REJECTED","r":"NONE","i":12345678,
                "l":"0.00000000","z":"0.00000000","L":"0.00000000",
                "n":"0","N":null,"T":1709654400000,"t":-1,"w":false,"m":false,
                "O":1709654400000,"Z":"0.00000000","C":""
            }}"#,
        );
        let report: BinanceSpotExecutionReport = serde_json::from_str(&report_json).unwrap();

        dispatch_ws_trading_message(
            BinanceSpotWsTradingMessage::ExecutionReport(Box::new(report)),
            &emitter,
            &http_client,
            AccountId::from("BINANCE-001"),
            clock,
            &dispatch_state,
            &ws_authenticated,
            &seen_trade_ids,
        );

        match rx.try_recv().expect("OrderRejected event expected") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(event.client_order_id, client_order_id);
                assert_eq!(event.account_id, AccountId::from("BINANCE-001"));
                assert_ne!(event.due_post_only, 0);
            }
            other => panic!("Expected OrderRejected event, was {other:?}"),
        }
    }
}
