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
    sync::Arc,
    time::{Duration, Instant},
};

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use async_trait::async_trait;
use dashmap::DashMap;
use futures_util::{StreamExt, pin_mut};
use nautilus_common::{
    clients::{ExecutionClient, ExecutionReportTask},
    enums::LogLevel,
    live::{runner::get_exec_event_sender, sender::EventSender},
    logging::map_log_level_to_filter,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    msgbus::{self, TypedHandler},
};
use nautilus_core::{
    AtomicMap, DurationNanos, Params, UUID4, UnixNanos,
    string::secret::SecretString,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl,
    execution::{failure::CommandFailure, reports::retain_order_status_reports},
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{
        AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, PositionSide,
        TimeInForce,
    },
    events::{
        OrderAccepted, OrderCancelRejected, OrderCanceled, OrderDeniedReason, OrderEventAny,
        OrderExpired, OrderFilled, OrderInitialized, OrderRejected, OrderUpdated,
    },
    fees::MakerTakerFeeRates,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use crate::{
    common::{
        auth::run_auth_token_refresh,
        consts::{
            AX_ACCOUNT_REGISTRATION_TIMEOUT_SECS, AX_AUTH_TOKEN_TTL_SECS, AX_FILL_ORDER_PREFIX,
            AX_FILLS_MAX_LOOKBACK_DAYS, AX_POST_ONLY_REJECT, AX_VENUE,
        },
        credential::Credential,
        enums::{AxCancelRejectionReason, AxOrderSide, AxOrderStatus, AxTimeInForce},
        parse::{
            ax_timestamp_stn_to_unix_nanos, cid_to_client_order_id, client_order_id_to_cid,
            quantity_to_contracts,
        },
    },
    config::AxExecutionClientConfig,
    http::{
        client::AxHttpClient,
        error::AxHttpError,
        models::{AxOrderRejectReason, PreviewAggressiveLimitOrderRequest, ReplaceOrderRequest},
        parse::update_order_report_timestamps,
    },
    websocket::{
        AxOrdersWsMessage, AxWsOrderEvent,
        messages::{AxWsOrder, AxWsTradeExecution, OrderMetadata},
        orders::{
            AxOrdersWebSocketClient, AxOrdersWsClientError, OrdersCaches,
            client::reserve_cid_mapping,
        },
    },
};

/// Live execution client for the AX Exchange.
#[derive(Debug)]
pub struct AxExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: AxExecutionClientConfig,
    emitter: ExecutionEventEmitter,
    http_client: AxHttpClient,
    ws_orders: AxOrdersWebSocketClient,
    order_event_handler: Option<TypedHandler<OrderEventAny>>,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    shutdown_errors: Vec<String>,
}

impl AxExecutionClient {
    /// Creates a new [`AxExecutionClient`].
    ///
    /// # Errors
    ///
    /// Returns an error if the client fails to initialize.
    pub fn new(core: ExecutionClientCore, config: AxExecutionClientConfig) -> anyhow::Result<Self> {
        let http_client = AxHttpClient::with_credentials(
            config
                .api_key
                .clone()
                .map(|value| value.into_inner())
                .unwrap_or_default(),
            config
                .api_secret
                .clone()
                .map(|value| value.into_inner())
                .unwrap_or_default(),
            Some(config.http_base_url()),
            Some(config.orders_base_url()),
            config.http_timeout_secs,
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
            config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_owned()),
        )?;

        let clock = get_atomic_clock_realtime();
        let trader_id = core.trader_id;
        let account_id = core.account_id;
        let emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        let mut ws_url = config.ws_private_url();
        if config.cancel_on_disconnect {
            let separator = if ws_url.contains('?') { "&" } else { "?" };
            ws_url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        }
        let ws_orders = AxOrdersWebSocketClient::new(
            ws_url,
            account_id,
            trader_id,
            config.heartbeat_interval_secs,
            config.transport_backend,
            config
                .proxy_url
                .as_ref()
                .map(|url| url.expose_secret().to_owned()),
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*AX_VENUE),
            "architect-ax-user-streams",
        ));

        let session_tasks = TaskGroup::new();
        let pending_tasks = TaskGroup::new();

        Ok(Self {
            core,
            clock,
            config,
            emitter,
            http_client,
            ws_orders,
            order_event_handler: None,
            session_tasks,
            pending_tasks,
            shutdown_errors: Vec::new(),
        })
    }

    pub(crate) fn set_event_sender(&mut self, sender: EventSender<ExecutionEvent>) {
        self.emitter.set_sender(sender);
    }

    fn ensure_order_event_subscription(&mut self) {
        if self.order_event_handler.is_some() {
            return;
        }

        let caches = self.ws_orders.caches().clone();
        let account_id = self.core.account_id;

        let handler = TypedHandler::from(move |event: &OrderEventAny| {
            let OrderEventAny::Accepted(accepted) = event else {
                return;
            };

            if accepted.account_id != account_id {
                return;
            }

            let Some(mut metadata) = caches.orders_metadata.get_mut(&accepted.client_order_id)
            else {
                return;
            };

            if metadata.trader_id != accepted.trader_id
                || metadata.strategy_id != accepted.strategy_id
                || metadata.instrument_id != accepted.instrument_id
            {
                return;
            }

            metadata.is_accepted = true;
            if metadata.venue_order_id.is_none() {
                metadata.venue_order_id = Some(accepted.venue_order_id);
                drop(metadata);
                caches
                    .venue_to_client_id
                    .insert(accepted.venue_order_id, accepted.client_order_id);
            }
        });

        msgbus::subscribe_order_events("events.order.*".into(), handler.clone(), Some(10));
        self.order_event_handler = Some(handler);
    }

    fn clear_order_event_subscription(&mut self) {
        if let Some(handler) = self.order_event_handler.take() {
            msgbus::unsubscribe_order_events("events.order.*".into(), &handler);
        }
    }

    async fn authenticate(&self, credential: &Credential) -> anyhow::Result<SecretString> {
        self.http_client
            .authenticate(
                credential.api_key(),
                credential.api_secret(),
                AX_AUTH_TOKEN_TTL_SECS,
            )
            .await
            .map_err(|e| anyhow::anyhow!("Authentication failed: {e}"))
    }

    fn update_account_state(&self) {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("query_account", async move {
            let account_state = http_client
                .request_account_state(account_id)
                .await
                .context("failed to request AX account state")?;
            let ts_event = clock.get_time_ns();
            emitter.emit_account_state(
                account_state.balances.clone(),
                account_state.margins.clone(),
                account_state.is_reported,
                ts_event,
                account_state.info,
            );
            Ok(())
        });
    }

    fn submit_order_internal(&self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let (
            order_for_task,
            client_order_id,
            instrument_id,
            order_side,
            order_type,
            quantity,
            time_in_force,
            is_post_only,
            limit_price,
        ) = {
            let cache = self.core.cache();
            let order = cache.try_order(&cmd.client_order_id)?;
            (
                order.clone(),
                order.client_order_id(),
                order.instrument_id(),
                order.order_side(),
                order.order_type(),
                order.quantity(),
                order.time_in_force(),
                order.is_post_only(),
                order.price(),
            )
        };

        let order_init = cmd.order_init.clone();
        let ws_orders = self.ws_orders.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;

        let http_client = self.http_client.clone();

        self.spawn_task("submit_order", async move {
            // AX emulates market orders with preview-priced IOC limits, so book moves
            // between preview and submission can produce partial fills.
            let (price, submit_time_in_force, submit_post_only) = if order_type
                == OrderType::Market
            {
                let preview_result: anyhow::Result<Price> = async {
                    let symbol = instrument_id.symbol.inner();
                    let ax_side = AxOrderSide::from(order_side);
                    let qty_contracts = quantity_to_contracts(quantity)?;

                    let instrument = http_client.get_instrument(&symbol).ok_or_else(|| {
                        anyhow::anyhow!("Instrument {instrument_id} not found in cache")
                    })?;

                    let request =
                        PreviewAggressiveLimitOrderRequest::new(symbol, qty_contracts, ax_side);
                    let response = http_client
                        .inner
                        .preview_aggressive_limit_order(&request)
                        .await
                        .map_err(|e| {
                            anyhow::anyhow!("Failed to preview aggressive limit order: {e}")
                        })?;

                    if response.remaining_quantity > 0 {
                        log::warn!(
                            "Market order book depth insufficient: \
                             filled_qty={} remaining_qty={} for {instrument_id}",
                            response.filled_quantity,
                            response.remaining_quantity,
                        );
                    }

                    let limit_price_decimal = response.limit_price.ok_or_else(|| {
                        anyhow::anyhow!(
                            "No liquidity available for market order on {instrument_id}"
                        )
                    })?;

                    let price =
                        Price::from_decimal_dp(limit_price_decimal, instrument.price_precision())
                            .with_context(|| {
                                format!(
                                    "Failed to convert AX take-through price {limit_price_decimal} for {instrument_id}"
                                )
                            })?;
                    log::debug!("Market order take-through price: {price} for {instrument_id}",);
                    Ok(price)
                }
                .await;

                let price = match preview_result {
                    Ok(price) => price,
                    Err(e) => {
                        let reason = e.to_string();
                        log::warn!(
                            "AX market order preview failed for {client_order_id}: {reason}"
                        );
                        emitter.emit_order_rejected(
                            &order_for_task,
                            &reason,
                            clock.get_time_ns(),
                            false,
                        );
                        return Ok(());
                    }
                };

                (price, TimeInForce::Ioc, false)
            } else {
                (
                    limit_price.context("AX limit order is missing a price")?,
                    time_in_force,
                    is_post_only,
                )
            };

            let result = ws_orders
                .submit_order(
                    &order_init,
                    quantity,
                    submit_time_in_force,
                    price,
                    submit_post_only,
                )
                .await;

            if let Err(e) = result {
                match classify_ax_ws_failure(&e) {
                    // AX classifies no send failure as a venue rejection (see
                    // `classify_ax_ws_failure`); both terminal-valid classes reject the same way.
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        log::warn!("AX submit failed for {client_order_id}: {reason}");
                        emitter.emit_order_rejected(
                            &order_for_task,
                            &reason,
                            clock.get_time_ns(),
                            false,
                        );
                    }
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Ambiguous AX submit failure for {client_order_id}, awaiting reconciliation: {reason}"
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order_internal(&self, cmd: &CancelOrder) {
        let ws_orders = self.ws_orders.clone();
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;

        // `OrderCancelRejected` comes only from the WS CancelRejected event;
        // local send failures leave the outcome to reconciliation.
        self.spawn_task("cancel_order", async move {
            if let Err(e) = ws_orders.cancel_order(client_order_id, venue_order_id).await {
                match classify_ax_ws_failure(&e) {
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        log::warn!("Cancel command failed for {client_order_id}: {reason}");
                    }
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Ambiguous AX cancel failure for {client_order_id}, awaiting reconciliation: {reason}"
                        );
                    }
                }
            }

            Ok(())
        });
    }

    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let future = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e}");
            }
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            log::warn!("Skipping AX {description} after shutdown began: {e}");
        }
    }

    fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    fn abort_session_tasks(&self) {
        self.session_tasks.begin_shutdown();
        self.ws_orders.begin_shutdown();
    }

    async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        self.pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate AX execution tasks: {e}"))?;
        Ok(())
    }

    async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate AX execution session tasks: {e}"))?;
        Ok(())
    }

    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.clear_order_event_subscription();
        self.abort_session_tasks();
        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();

        if let Err(e) = self.ws_orders.close().await {
            self.shutdown_errors
                .push(format!("AX orders WebSocket shutdown failed: {e}"));
        }

        let (session_result, pending_result) =
            tokio::join!(self.await_session_tasks(), self.await_pending_tasks());
        cleanup_deferred_order_tracking(self.ws_orders.caches());
        self.core.set_disconnected();

        if let Err(e) = session_result {
            self.shutdown_errors.push(e.to_string());
        }

        if let Err(e) = pending_result {
            self.shutdown_errors.push(e.to_string());
        }

        if !self.shutdown_errors.is_empty() {
            anyhow::bail!(std::mem::take(&mut self.shutdown_errors).join("; "));
        }
        Ok(())
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
        self.core.cache().account_owned(&self.core.account_id)
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() && self.pending_tasks.is_open() && self.session_tasks.is_open()
        {
            return Ok(());
        }

        if !self.pending_tasks.is_open() || !self.session_tasks.is_open() {
            self.teardown_partial_connect().await?;
            self.pending_tasks.start_generation().map_err(|e| {
                anyhow::anyhow!("Failed to start AX execution task generation: {e}")
            })?;
            self.session_tasks.start_generation().map_err(|e| {
                anyhow::anyhow!("Failed to start AX execution session generation: {e}")
            })?;
        }
        let http_client = self.http_client.clone();
        let ws_orders = self.ws_orders.clone();
        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                http_client.cancel_all_requests();
                ws_orders.begin_shutdown();
            });

        // Reset so requests work after a previous disconnect
        self.http_client.reset_cancellation_token();

        let credential = Credential::resolve(
            self.config.api_key.clone().map(|value| value.into_inner()),
            self.config
                .api_secret
                .clone()
                .map(|value| value.into_inner()),
        )
        .context("API credentials not configured")?;
        let token = self.authenticate(&credential).await?;

        self.http_client
            .request_account_fees()
            .await
            .context("failed to resolve AX account fee rates")?;

        if !self.core.instruments_initialized() {
            let instruments = self
                .http_client
                .request_instruments()
                .await
                .context("failed to request AX instruments")?;

            if instruments.is_empty() {
                log::warn!("No instruments returned from AX");
            } else {
                log::debug!("Loaded {} instruments", instruments.len());
                self.http_client.cache_instruments(&instruments);
                self.ws_orders.cache_instruments(&instruments);
            }
            self.core.set_instruments_initialized();
        }

        let orders = {
            let cache = self.core.cache();
            let mut orders = cache.orders_refs(Some(&AX_VENUE), None, None, None, None);
            orders.retain(|order| {
                (order.is_open() || order.is_inflight())
                    && order.trader_id() == self.core.trader_id
                    && order
                        .account_id()
                        .is_none_or(|account_id| account_id == self.core.account_id)
                    && cache
                        .client_id(&order.client_order_id())
                        .is_none_or(|client_id| *client_id == self.core.client_id)
            });

            orders
                .into_iter()
                .map(|order| order.cloned())
                .collect::<Vec<_>>()
        };

        for order in orders {
            let symbol = order.instrument_id().symbol.inner();
            let needs_instrument = self.http_client.get_instrument(&symbol).is_none()
                && self
                    .core
                    .cache()
                    .instrument(&order.instrument_id())
                    .is_none();

            if needs_instrument {
                let instrument = self
                    .http_client
                    .request_instrument(symbol)
                    .await
                    .with_context(|| format!("failed to restore AX instrument {symbol}"))?;
                self.http_client.cache_instrument(instrument);
            }

            self.restore_order_tracking(&order, None)?;
        }

        self.ensure_order_event_subscription();
        self.ws_orders.connect(token.expose_secret()).await?;
        log::debug!("Connected to orders WebSocket");

        let stream = self.ws_orders.stream();
        let emitter = self.emitter.clone();
        let caches = self.ws_orders.caches().clone();
        let account_id = self.core.account_id;
        let instruments_cache = self.ws_orders.instruments_cache();
        let account_fee_rates = self.http_client.account_fee_rates.clone();
        let clock = self.clock;

        if let Err(e) = self.session_tasks.spawn(async move {
            pin_mut!(stream);
            while let Some(message) = stream.next().await {
                dispatch_ws_message(
                    message,
                    &emitter,
                    &caches,
                    account_id,
                    &instruments_cache,
                    &account_fee_rates,
                    clock,
                );
            }
        }) {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(anyhow::Error::new(e).context(format!(
                    "AX execution startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e.into());
        }

        let session_result = async {
            let account_state = self
                .http_client
                .request_account_state(self.core.account_id)
                .await
                .context("failed to request AX account state")?;

            if !account_state.balances.is_empty() {
                log::debug!(
                    "Received account state with {} balance(s)",
                    account_state.balances.len()
                );
            }
            self.emitter.send_account_state(account_state);

            self.await_account_registered(AX_ACCOUNT_REGISTRATION_TIMEOUT_SECS)
                .await?;

            let ws_orders = self.ws_orders.clone();
            self.session_tasks.spawn(run_auth_token_refresh(
                self.http_client.clone(),
                credential,
                move |token| ws_orders.update_auth_token(token.expose_secret()),
            ))?;
            Ok::<(), anyhow::Error>(())
        }
        .await;

        if let Err(e) = session_result {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "AX execution startup teardown failed: {teardown_error}"
                )));
            }
            return Err(e);
        }

        self.core.set_connected();
        setup_guard.disarm();
        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.clear_order_event_subscription();
        self.abort_session_tasks();
        self.abort_pending_tasks();
        self.http_client.cancel_all_requests();

        let ws_result = self.ws_orders.close().await;
        let (session_result, pending_result) =
            tokio::join!(self.await_session_tasks(), self.await_pending_tasks());

        cleanup_deferred_order_tracking(self.ws_orders.caches());
        self.core.set_disconnected();
        ws_result?;
        session_result?;
        pending_result?;
        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        self.update_account_state();
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let account_id = self.core.account_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = self
            .ws_orders
            .orders_metadata()
            .get(&client_order_id)
            .and_then(|metadata| metadata.venue_order_id)
            .or(cmd.venue_order_id);
        let instrument_id = cmd.instrument_id;
        let emitter = self.emitter.clone();
        let caches = self.ws_orders.caches().clone();

        // Read immutable order fields from cache before spawning
        let (order_side, order_type, time_in_force) = {
            let cache = self.core.cache();
            match cache.order(&client_order_id) {
                Some(order) => (
                    Some(order.order_side()),
                    order.order_type(),
                    order.time_in_force(),
                ),
                None => (None, OrderType::Limit, TimeInForce::Gtc),
            }
        };

        self.spawn_task("query_order", async move {
            match http_client
                .request_order_status(
                    account_id,
                    instrument_id,
                    Some(client_order_id),
                    venue_order_id,
                    order_side,
                    order_type,
                    time_in_force,
                )
                .await
            {
                Ok(report) => {
                    if let Some(report) =
                        resolve_order_status_report(&http_client, &caches, client_order_id, report)
                            .await?
                    {
                        cleanup_resolved_order_status_report(&report, &caches);
                        emitter.send_order_status_report(report);
                    }
                }
                Err(e) => log::warn!("AX query order failed: {e}"),
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
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        self.emitter
            .emit_account_state(balances, margins, reported, ts_event, info);
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.ensure_order_event_subscription();

        if self.core.is_started() {
            return Ok(());
        }

        self.emitter.set_sender(get_exec_event_sender());
        self.core.set_started();
        log::info!(
            "Started: client_id={}, account_id={}, environment={}",
            self.core.client_id,
            self.core.account_id,
            self.config.environment,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.clear_order_event_subscription();

        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();

        self.abort_session_tasks();
        self.abort_pending_tasks();
        cleanup_deferred_order_tracking(self.ws_orders.caches());
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.clear_order_event_subscription();
        self.abort_session_tasks();
        self.abort_pending_tasks();
        cleanup_deferred_order_tracking(self.ws_orders.caches());
        self.core.set_disconnected();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.clear_order_event_subscription();
        self.abort_session_tasks();
        self.abort_pending_tasks();
        cleanup_deferred_order_tracking(self.ws_orders.caches());
        self.core.set_disconnected();
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        {
            let cache = self.core.cache();
            let order = cache.try_order(&cmd.client_order_id)?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            if let Err(e) = validate_order_for_ax_submit(&order)
                .and_then(|()| validate_order_init_instructions(&cmd.order_init))
            {
                self.emitter.emit_order_denied(&order, &e.to_string());
                return Ok(());
            }

            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(&order);
        }

        self.submit_order_internal(&cmd)
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        for (client_order_id, order_init) in cmd
            .order_list
            .client_order_ids
            .iter()
            .zip(cmd.order_inits.iter())
        {
            let submit_cmd = SubmitOrder::new(
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                *client_order_id,
                order_init.clone(),
                cmd.exec_algorithm_id,
                cmd.position_id,
                cmd.params.clone(),
                UUID4::new(),
                cmd.ts_init,
                cmd.correlation_id,
            );
            self.submit_order(submit_cmd)?;
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        if cmd.trigger_price.is_some() {
            emit_ax_modify_rejected(
                &self.emitter,
                self.clock,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                "AX does not support venue-native trigger prices",
            );
            return Ok(());
        }

        let tracked_venue_order_id = self
            .ws_orders
            .orders_metadata()
            .get(&cmd.client_order_id)
            .and_then(|metadata| metadata.venue_order_id);

        let venue_order_id = match tracked_venue_order_id.or(cmd.venue_order_id) {
            Some(voi) => voi,
            None => {
                emit_ax_modify_rejected(
                    &self.emitter,
                    self.clock,
                    cmd.strategy_id,
                    cmd.instrument_id,
                    cmd.client_order_id,
                    None,
                    "missing venue_order_id",
                );
                return Ok(());
            }
        };

        let quantity = match cmd.quantity {
            Some(quantity) => match quantity_to_contracts(quantity) {
                Ok(contracts) => Some(contracts),
                Err(e) => {
                    emit_ax_modify_rejected(
                        &self.emitter,
                        self.clock,
                        cmd.strategy_id,
                        cmd.instrument_id,
                        cmd.client_order_id,
                        Some(venue_order_id),
                        &e.to_string(),
                    );
                    return Ok(());
                }
            },
            None => None,
        };

        if !self.core.is_connected() {
            emit_ax_modify_rejected(
                &self.emitter,
                self.clock,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                "AX execution client is not connected",
            );
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let caches = self.ws_orders.caches().clone();
        let client_order_id = cmd.client_order_id;
        let price = cmd.price;

        caches
            .pending_modifies
            .entry(venue_order_id)
            .and_modify(|(_, count, _)| *count += 1)
            .or_insert((client_order_id, 1, false));

        self.spawn_task("modify_order", async move {
            let mut request = ReplaceOrderRequest::new(venue_order_id.as_str());

            if let Some(price) = price {
                request = request.with_price(price.as_decimal());
            }

            if let Some(contracts) = quantity {
                request = request.with_quantity(contracts);
            }

            match http_client.inner.replace_order(&request).await {
                Ok(resp) => {
                    let new_venue_order_id = match VenueOrderId::new_checked(&resp.oid) {
                        Ok(venue_order_id) => venue_order_id,
                        Err(e) => {
                            log::warn!(
                                "AX replace returned invalid venue order ID for {client_order_id}, awaiting reconciliation: {e}"
                            );
                            return Ok(());
                        }
                    };

                    record_replacement_proposal(&caches, client_order_id, venue_order_id, new_venue_order_id);
                    log::debug!(
                        "AX replace received: old={} proposed={new_venue_order_id}",
                        request.oid,
                    );
                }
                // No replace failure is an unambiguous rejection (see
                // `classify_ax_http_failure`); leave the order pending.
                Err(e) => match classify_ax_http_failure(&e) {
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        complete_pending_modify(&caches, venue_order_id);
                        emit_ax_modify_rejected(
                            &emitter,
                            clock,
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            Some(venue_order_id),
                            &reason,
                        );
                    }
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Ambiguous AX modify failure for {client_order_id}, awaiting reconciliation: {reason}"
                        );
                    }
                },
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.cancel_order_internal(&cmd);
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        if cmd.order_side.is_some() {
            // AX cancel-all has no side parameter, so select matching open
            // orders and cancel their explicit IDs through the batch path.
            let cancels: Vec<CancelOrder> = {
                let cache = self.core.cache();
                cache
                    .orders_open(None, Some(&cmd.instrument_id), None, None, cmd.order_side)
                    .iter()
                    .map(|order| CancelOrder {
                        trader_id: order.trader_id(),
                        client_id: cmd.client_id,
                        strategy_id: order.strategy_id(),
                        instrument_id: order.instrument_id(),
                        client_order_id: order.client_order_id(),
                        venue_order_id: order.venue_order_id(),
                        command_id: cmd.command_id,
                        ts_init: cmd.ts_init,
                        params: cmd.params.clone(),
                        correlation_id: cmd.correlation_id,
                        causation_id: cmd.causation_id,
                    })
                    .collect()
            };

            if cancels.is_empty() {
                log::debug!("No open orders to cancel for {}", cmd.instrument_id);
                return Ok(());
            }

            return self.batch_cancel_orders(BatchCancelOrders {
                trader_id: cmd.trader_id,
                client_id: cmd.client_id,
                strategy_id: cmd.strategy_id,
                instrument_id: cmd.instrument_id,
                cancels,
                command_id: cmd.command_id,
                ts_init: cmd.ts_init,
                params: cmd.params,
                correlation_id: cmd.correlation_id,
                causation_id: cmd.causation_id,
            });
        }

        let http_client = self.http_client.clone();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instrument_id = cmd.instrument_id;
        let account_id = self.core.account_id;
        let trader_id = self.core.trader_id;

        // Snapshot open orders so we can emit cancel events after the HTTP request
        let open_orders: Vec<(ClientOrderId, Option<VenueOrderId>, StrategyId)> = {
            let cache = self.core.cache();
            cache
                .orders_open(None, Some(&instrument_id), None, None, None)
                .iter()
                .map(|o| (o.client_order_id(), o.venue_order_id(), o.strategy_id()))
                .collect()
        };

        let caches = self.ws_orders.caches().clone();

        self.spawn_task("cancel_all_orders", async move {
            match http_client.cancel_all_orders(instrument_id).await {
                Ok(()) => {
                    log::debug!("Canceled all orders for {instrument_id}");

                    // AX does not push WS cancel confirmations for HTTP-initiated
                    // cancels, so emit OrderCanceled events locally and clean up
                    // tracking state to prevent duplicates if WS events arrive
                    let ts_event = clock.get_time_ns();

                    for (client_order_id, venue_order_id, strategy_id) in &open_orders {
                        let event = OrderCanceled::new(
                            trader_id,
                            *strategy_id,
                            instrument_id,
                            *client_order_id,
                            UUID4::new(),
                            ts_event,
                            clock.get_time_ns(),
                            false,
                            *venue_order_id,
                            Some(account_id),
                            None,
                        );
                        emitter.send_order_event(OrderEventAny::Canceled(event));

                        cleanup_terminal_order_tracking_ids(
                            &caches,
                            None,
                            None,
                            Some(*client_order_id),
                        );
                    }
                }
                // A whole-request failure has no per-order venue results and
                // must not fan out per-order rejections.
                Err(e) => match classify_ax_http_failure(&e) {
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                        log::warn!("Cancel-all for {instrument_id} failed: {reason}");
                    }
                    CommandFailure::Ambiguous(reason) => {
                        log::warn!(
                            "Ambiguous AX cancel-all failure for {instrument_id}, awaiting reconciliation: {reason}"
                        );
                    }
                },
            }
            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        for cancel in &cmd.cancels {
            self.cancel_order_internal(cancel);
        }
        Ok(())
    }

    fn generate_order_status_report_task(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> Option<ExecutionReportTask<Option<OrderStatusReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        let caches = self.ws_orders.caches().clone();
        Some(ExecutionReportTask::new(
            async move { client.generate_order_status_report(&command).await },
            move |report| {
                if let Some(report) = &report {
                    cleanup_closed_order_status_report(report, &caches);
                }

                Ok(report)
            },
        ))
    }

    fn generate_order_status_reports_task(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> Option<ExecutionReportTask<Vec<OrderStatusReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        let caches = self.ws_orders.caches().clone();
        Some(ExecutionReportTask::new(
            async move { client.generate_order_status_reports(&command).await },
            move |reports| {
                for report in &reports {
                    cleanup_closed_order_status_report(report, &caches);
                }

                Ok(reports)
            },
        ))
    }

    fn generate_fill_reports_task(
        &self,
        cmd: &GenerateFillReports,
    ) -> Option<ExecutionReportTask<Vec<FillReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move { client.generate_fill_reports(command).await },
            Ok,
        ))
    }

    fn generate_position_status_reports_task(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> Option<ExecutionReportTask<Vec<PositionStatusReport>>> {
        let client = self.report_client();
        let command = cmd.clone();
        Some(ExecutionReportTask::new(
            async move { client.generate_position_status_reports(&command).await },
            Ok,
        ))
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let report = self
            .report_client()
            .generate_order_status_report(cmd)
            .await?;

        if let Some(report) = &report {
            cleanup_closed_order_status_report(report, self.ws_orders.caches());
        }

        Ok(report)
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let reports = self
            .report_client()
            .generate_order_status_reports(cmd)
            .await?;

        for report in &reports {
            cleanup_closed_order_status_report(report, self.ws_orders.caches());
        }

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.report_client().generate_fill_reports(cmd).await
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        self.report_client()
            .generate_position_status_reports(cmd)
            .await
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();

        let positions_cached = self
            .core
            .cache()
            .positions_open(
                Some(&AX_VENUE),
                None,
                None,
                Some(&self.core.account_id),
                None,
            )
            .into_iter()
            .map(|position| (position.instrument_id, position.quantity.precision))
            .collect();

        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|lookback| ts_now.saturating_sub(lookback));

        // Floor the declared window at the /fills span cap so it does not overstate coverage
        let fills_span = DurationNanos::try_from_days(u64::try_from(AX_FILLS_MAX_LOOKBACK_DAYS)?)?;
        let fills_floor = ts_now.saturating_sub(fills_span);

        // AX rejects zero-width fill windows, including a zero-minute lookback
        let fills_latest_start = ts_now.saturating_sub(DurationNanos::new(1));
        let declared_start = Some(
            start
                .unwrap_or(fills_floor)
                .max(fills_floor)
                .min(fills_latest_start),
        );

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

        let mut fill_cmd = GenerateFillReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            None, // venue_order_id
            declared_start,
            Some(ts_now), // end
            None,         // params
            None,         // correlation_id
        );

        fill_cmd.log_receipt_level = LogLevel::Off;

        let mut position_cmd = GeneratePositionStatusReports::new(
            UUID4::new(),
            ts_now,
            None, // instrument_id
            start,
            None, // end
            None, // params
            None, // correlation_id
        );

        position_cmd.log_receipt_level = LogLevel::Off;

        let (mut order_reports, fill_reports, mut position_reports) = tokio::try_join!(
            async {
                let cid_resolver = |cid| {
                    self.ws_orders
                        .caches()
                        .cid_to_client_order_id
                        .get(&cid)
                        .map(|v| *v)
                };
                let mut reports = self
                    .http_client
                    .request_order_history(self.core.account_id, start, None, Some(&cid_resolver))
                    .await?;
                let active = self
                    .http_client
                    .request_order_status_reports(self.core.account_id, Some(&cid_resolver))
                    .await?;
                reports = merge_active_order_reports(reports, active);
                retain_order_status_reports(&mut reports, &order_cmd);

                for report in &reports {
                    cleanup_closed_order_status_report(report, self.ws_orders.caches());
                }

                Ok::<_, anyhow::Error>(reports)
            },
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        self.add_flat_position_reports(positions_cached, &mut position_reports, ts_now);

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

        // Declare the window so these fills are bounded history, not live fills
        mass_status.set_report_window(declared_start, true);

        update_order_report_timestamps(&mut order_reports, &fill_reports);
        add_special_fill_order_reports(&mut order_reports, &fill_reports);

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
        _ts_init: UnixNanos,
    ) {
        let order = self.core.cache().order_owned(&client_order_id);
        if let Some(order) = order {
            if let Err(e) = self.restore_order_tracking(&order, Some(venue_order_id)) {
                log::warn!("Cannot restore AX order {client_order_id}: {e}");
            }

            return;
        }

        self.ws_orders.register_external_order(
            client_order_id,
            venue_order_id,
            instrument_id,
            strategy_id,
        );
    }
}

impl AxExecutionClient {
    fn restore_order_tracking(
        &self,
        order: &OrderAny,
        venue_order_id: Option<VenueOrderId>,
    ) -> anyhow::Result<()> {
        let client_order_id = order.client_order_id();
        let caches = self.ws_orders.caches();

        let instrument = self
            .http_client
            .get_instrument(&order.instrument_id().symbol.inner())
            .or_else(|| {
                self.core
                    .cache()
                    .instrument(&order.instrument_id())
                    .cloned()
            })
            .context("cannot restore AX order without its instrument")?;

        self.http_client.cache_instrument(instrument.clone());
        self.ws_orders.cache_instrument(instrument.clone());
        let cid = client_order_id_to_cid(&client_order_id);
        let cid_owner = caches.cid_to_client_order_id.get(&cid).map(|entry| *entry);
        if cid_owner != Some(client_order_id) {
            reserve_cid_mapping(caches, cid, client_order_id)?;
        }

        let metadata = OrderMetadata {
            trader_id: order.trader_id(),
            strategy_id: order.strategy_id(),
            instrument_id: order.instrument_id(),
            client_order_id,
            order_init: order
                .events()
                .into_iter()
                .rev()
                .find_map(|event| match event {
                    OrderEventAny::Initialized(init) => Some(init.clone()),
                    _ => None,
                }),
            is_accepted: order.status() != OrderStatus::Submitted,
            venue_order_id: order.venue_order_id().or(venue_order_id),
            ts_init: order.init_event().ts_init,
            size_precision: instrument.size_precision(),
            price_precision: instrument.price_precision(),
            quote_currency: instrument.quote_currency(),
        };

        let venue_order_id = caches
            .orders_metadata
            .entry(client_order_id)
            .or_insert(metadata)
            .venue_order_id;

        if let Some(venue_order_id) = venue_order_id {
            caches
                .venue_to_client_id
                .insert(venue_order_id, client_order_id);
        }

        Ok(())
    }

    fn report_client(&self) -> AxReportClient {
        AxReportClient {
            http_client: self.http_client.clone(),
            account_id: self.core.account_id,
            cid_to_client_order_id: self.ws_orders.caches().cid_to_client_order_id.clone(),
        }
    }

    fn add_flat_position_reports(
        &self,
        mut positions_expected: AHashMap<InstrumentId, u8>,
        position_reports: &mut Vec<PositionStatusReport>,
        ts_init: UnixNanos,
    ) {
        for report in position_reports.iter() {
            positions_expected.remove(&report.instrument_id);
        }

        let mut positions_expected: Vec<_> = positions_expected.into_iter().collect();
        positions_expected.sort_unstable_by_key(|(instrument_id, _)| *instrument_id);

        // The successful complete AX positions snapshot establishes absent instruments as flat
        for (instrument_id, precision) in positions_expected {
            position_reports.push(PositionStatusReport::new(
                self.core.account_id,
                instrument_id,
                PositionSide::Flat,
                Quantity::zero(precision),
                ts_init,
                ts_init,
                None,
                None,
                None,
            ));
        }
    }
}

struct AxReportClient {
    http_client: AxHttpClient,
    account_id: AccountId,
    cid_to_client_order_id: Arc<DashMap<u64, ClientOrderId>>,
}

impl AxReportClient {
    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let cid_map = self.cid_to_client_order_id.clone();
        let cid_resolver = move |cid: u64| cid_map.get(&cid).map(|v| *v);

        let Some(report) = self
            .http_client
            .request_order_report(
                self.account_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                Some(cid_resolver),
            )
            .await?
        else {
            return Ok(None);
        };

        anyhow::ensure!(
            cmd.instrument_id
                .is_none_or(|id| report.instrument_id == id),
            "AX order report instrument does not match requested instrument"
        );
        anyhow::ensure!(
            cmd.venue_order_id
                .is_none_or(|id| report.venue_order_id == id),
            "AX order report venue identity does not match requested order"
        );
        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let cid_map = self.cid_to_client_order_id.clone();
        let cid_resolver = move |cid: u64| cid_map.get(&cid).map(|v| *v);

        let mut reports = if cmd.open_only {
            self.http_client
                .request_order_status_reports(self.account_id, Some(&cid_resolver))
                .await?
        } else {
            let reports = self
                .http_client
                .request_historical_order_status_reports(
                    self.account_id,
                    cmd.start,
                    cmd.end,
                    Some(&cid_resolver),
                )
                .await?;
            let active = self
                .http_client
                .request_order_status_reports(self.account_id, Some(&cid_resolver))
                .await?;
            merge_active_order_reports(reports, active)
        };

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        retain_order_status_reports(&mut reports, cmd);
        log_report_receipt(reports.len(), "OrderStatusReport", cmd.log_receipt_level);

        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let log_level = cmd.log_receipt_level;
        let mut reports = self
            .http_client
            .request_fill_reports(self.account_id, cmd.start, cmd.end)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        if let Some(venue_order_id) = cmd.venue_order_id {
            reports.retain(|report| report.venue_order_id.as_str() == venue_order_id.as_str());
        }

        log_report_receipt(reports.len(), "FillReport", log_level);
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let mut reports = self
            .http_client
            .request_position_reports(self.account_id)
            .await?;

        if let Some(instrument_id) = cmd.instrument_id {
            reports.retain(|report| report.instrument_id == instrument_id);
        }

        log_report_receipt(reports.len(), "PositionStatusReport", cmd.log_receipt_level);
        Ok(reports)
    }
}

fn merge_active_order_reports(
    mut reports: Vec<OrderStatusReport>,
    active: Vec<OrderStatusReport>,
) -> Vec<OrderStatusReport> {
    let active_ids: AHashSet<_> = active.iter().map(|report| report.venue_order_id).collect();
    reports.retain(|report| !active_ids.contains(&report.venue_order_id));
    reports.extend(active);
    reports
}

fn log_report_receipt(count: usize, report_type: &str, level: LogLevel) {
    if let Some(level) = map_log_level_to_filter(level).to_level() {
        log::log!(level, "Received {count} {report_type} reports");
    }
}

fn add_special_fill_order_reports(
    order_reports: &mut Vec<OrderStatusReport>,
    fill_reports: &[FillReport],
) {
    for fill in fill_reports {
        if fill
            .venue_order_id
            .as_str()
            .strip_prefix(AX_FILL_ORDER_PREFIX)
            != Some(fill.trade_id.as_str())
        {
            continue;
        }

        let report = OrderStatusReport::new(
            fill.account_id,
            fill.instrument_id,
            fill.client_order_id,
            fill.venue_order_id,
            Some(fill.order_side),
            OrderType::Market,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            fill.last_qty,
            fill.last_qty,
            fill.ts_event,
            fill.ts_event,
            fill.ts_init,
            None,
        )
        .with_avg_px(fill.last_px.as_decimal());
        order_reports.push(report);
    }
}

fn emit_ax_modify_rejected(
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    reason: &str,
) {
    log::warn!("AX modify rejected for {client_order_id}: {reason}");
    emitter.emit_order_modify_rejected_event(
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        reason,
        clock.get_time_ns(),
    );
}

/// Dispatches a WebSocket message using the event emitter.
fn dispatch_ws_message(
    message: AxOrdersWsMessage,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_fee_rates: &AtomicMap<Option<Ustr>, MakerTakerFeeRates>,
    clock: &'static AtomicTime,
) {
    match message {
        AxOrdersWsMessage::Event(event) => {
            dispatch_order_event(
                *event,
                emitter,
                caches,
                account_id,
                instruments,
                account_fee_rates,
                clock,
            );
        }
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
            log::debug!("Open orders response: {} orders", resp.res.orders.len());
        }
        AxOrdersWsMessage::Error(err) => {
            log::debug!("WebSocket error: {}", err.message);
        }
        AxOrdersWsMessage::Reconnected => {
            log::info!("WebSocket reconnected");
        }
        AxOrdersWsMessage::Authenticated => {
            log::debug!("WebSocket authenticated");
        }
    }
}

fn dispatch_order_event(
    event: AxWsOrderEvent,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_fee_rates: &AtomicMap<Option<Ustr>, MakerTakerFeeRates>,
    clock: &'static AtomicTime,
) {
    let (order, replacement_id) = match &event {
        AxWsOrderEvent::Acknowledged(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::PartiallyFilled(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Filled(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Canceled(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Rejected(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Expired(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Replaced(msg) => {
            (Some(msg.ro.as_ref()), replacement_venue_order_id(msg).ok())
        }
        AxWsOrderEvent::DoneForDay(msg) => (Some(&msg.o), None),
        AxWsOrderEvent::Heartbeat | AxWsOrderEvent::CancelRejected(_) => (None, None),
    };

    let allow_previous = matches!(
        &event,
        AxWsOrderEvent::PartiallyFilled(_) | AxWsOrderEvent::Filled(_)
    );

    if let Some(order) = order
        && !is_valid_order_update(order, caches, replacement_id, allow_previous)
    {
        log::warn!(
            "Deferring stale or malformed AX order update, order_id={}",
            order.oid
        );
        return;
    }

    match event {
        AxWsOrderEvent::Heartbeat => {}
        AxWsOrderEvent::Acknowledged(msg) => {
            if let Some(event) =
                create_order_accepted(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Accepted(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Accepted,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            }
        }
        AxWsOrderEvent::PartiallyFilled(msg) => {
            dispatch_fill_event(
                &msg.o,
                &msg.xs,
                msg.ts,
                msg.tn,
                emitter,
                caches,
                account_id,
                instruments,
                account_fee_rates,
                false,
                clock,
            );
        }
        AxWsOrderEvent::Filled(msg) => {
            dispatch_fill_event(
                &msg.o,
                &msg.xs,
                msg.ts,
                msg.tn,
                emitter,
                caches,
                account_id,
                instruments,
                account_fee_rates,
                true,
                clock,
            );
        }
        AxWsOrderEvent::Canceled(msg) => {
            if let Some(event) =
                create_order_canceled(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Canceled(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Canceled,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            } else {
                return;
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Rejected(msg) => {
            let reason = AxOrderRejectReason::reason_str(msg.r, msg.txt.as_deref())
                .unwrap_or_else(|| "UNKNOWN".to_string());

            if let Some(event) =
                create_order_rejected(&msg.o, &reason, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Rejected(event));
            } else {
                return;
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Expired(msg) => {
            // AX reports an unfilled IOC/FOK as EXPIRED; Nautilus models those as canceled
            let as_canceled = matches!(msg.o.tif, AxTimeInForce::Ioc | AxTimeInForce::Fok);
            let event = if as_canceled {
                create_order_canceled(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
                    .map(OrderEventAny::Canceled)
            } else {
                create_order_expired(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
                    .map(OrderEventAny::Expired)
            };

            if let Some(event) = event {
                emitter.send_order_event(event);
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                if as_canceled {
                    OrderStatus::Canceled
                } else {
                    OrderStatus::Expired
                },
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            } else {
                return;
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::Replaced(msg) => {
            let replacement_venue_order_id = match replacement_venue_order_id(&msg) {
                Ok(venue_order_id) => venue_order_id,
                Err(e) => {
                    log::warn!("Invalid AX replace event, awaiting reconciliation: {e}");
                    return;
                }
            };

            let order = msg.no.as_deref().unwrap_or(&msg.ro);

            if let Some(event) = create_order_updated(
                order,
                &msg.ro,
                replacement_venue_order_id,
                (msg.ts, msg.tn),
                caches,
                account_id,
                clock,
            ) {
                emitter.send_order_event(OrderEventAny::Updated(event));
            } else if lookup_order_metadata(&msg.ro, caches).is_none()
                && let Some(report) = create_order_status_report(
                    order,
                    OrderStatus::Accepted,
                    msg.ts,
                    msg.tn,
                    caches,
                    account_id,
                    instruments,
                    clock,
                )
            {
                emitter.send_order_status_report(report);
            }
        }
        AxWsOrderEvent::DoneForDay(msg) => {
            if let Some(event) =
                create_order_expired(&msg.o, msg.ts, msg.tn, caches, account_id, clock)
            {
                emitter.send_order_event(OrderEventAny::Expired(event));
            } else if let Some(report) = create_order_status_report(
                &msg.o,
                OrderStatus::Expired,
                msg.ts,
                msg.tn,
                caches,
                account_id,
                instruments,
                clock,
            ) {
                emitter.send_order_status_report(report);
            } else {
                return;
            }
            cleanup_terminal_order_tracking(&msg.o, caches);
        }
        AxWsOrderEvent::CancelRejected(msg) => {
            let Ok(venue_order_id) = VenueOrderId::new_checked(&msg.oid) else {
                log::warn!("Invalid AX cancel rejection order ID");
                return;
            };

            let reason = match msg.r {
                AxCancelRejectionReason::Unknown => msg
                    .txt
                    .as_deref()
                    .filter(|text| !text.trim().is_empty())
                    .unwrap_or(msg.r.as_ref()),
                _ => msg.r.as_ref(),
            };

            let modify_rejected = reason == AX_POST_ONLY_REJECT;

            let client_order_id = caches
                .venue_to_client_id
                .get(&venue_order_id)
                .map(|entry| *entry)
                .or_else(|| {
                    modify_rejected
                        .then(|| {
                            caches
                                .pending_modifies
                                .get(&venue_order_id)
                                .map(|entry| entry.0)
                        })
                        .flatten()
                });

            if let Some(client_order_id) = client_order_id
                && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
                && (modify_rejected
                    || metadata
                        .venue_order_id
                        .is_none_or(|current| current == venue_order_id))
            {
                let strategy_id = metadata.strategy_id;
                let instrument_id = metadata.instrument_id;
                if modify_rejected {
                    let active_venue_order_id = metadata.venue_order_id.unwrap_or(venue_order_id);
                    drop(metadata);
                    complete_pending_modify(caches, venue_order_id);
                    emit_ax_modify_rejected(
                        emitter,
                        clock,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(active_venue_order_id),
                        reason,
                    );

                    if let Some(order) = &msg.o
                        && matches!(
                            order.o,
                            AxOrderStatus::Filled
                                | AxOrderStatus::Canceled
                                | AxOrderStatus::Expired
                        )
                    {
                        cleanup_terminal_order_tracking(order, caches);
                    }
                } else {
                    let event = OrderCancelRejected::new(
                        metadata.trader_id,
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Ustr::from(reason),
                        UUID4::new(),
                        clock.get_time_ns(),
                        metadata.ts_init,
                        false,
                        Some(venue_order_id),
                        Some(account_id),
                    );
                    emitter.send_order_event(OrderEventAny::CancelRejected(event));
                }
            } else {
                log::debug!(
                    "Could not find metadata for cancel rejected order {}",
                    msg.oid
                );
            }
        }
    }
}

#[expect(clippy::too_many_arguments)]
fn dispatch_fill_event(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    ts: i64,
    tn: i64,
    emitter: &ExecutionEventEmitter,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_fee_rates: &AtomicMap<Option<Ustr>, MakerTakerFeeRates>,
    complete: bool,
    clock: &'static AtomicTime,
) {
    if order.o == AxOrderStatus::Unknown || order.tif == AxTimeInForce::Unknown {
        log::warn!(
            "Unknown AX order classification in fill, order_id={}, instrument={}, trade_id={}, state={}, time_in_force={}",
            order.oid,
            order.s,
            execution.tid,
            order.o,
            order.tif,
        );
    }

    if execution.s != order.s {
        log::warn!("Deferring inconsistent AX fill, trade_id={}", execution.tid);
        return;
    }

    let commission =
        match calculate_fill_commission(order, execution, instruments, account_fee_rates) {
            Ok(commission) => commission,
            Err(e) => {
                log::warn!(
                    "Cannot calculate AX commission for trade_id={}: {e}",
                    execution.tid
                );
                return;
            }
        };

    if let Some(event) = create_order_filled(
        order, execution, ts, tn, caches, account_id, commission, clock,
    ) {
        let current = caches
            .orders_metadata
            .get(&event.client_order_id)
            .and_then(|metadata| metadata.venue_order_id);

        if let Some(parent) = current
            && parent != event.venue_order_id
            && caches.pending_modifies.contains_key(&parent)
            && !caches.pending_modifies.contains_key(&event.venue_order_id)
        {
            record_replacement_venue_id(
                caches,
                event.client_order_id,
                parent,
                event.venue_order_id,
                false,
            );
        }

        let needs_acceptance =
            lookup_order_metadata(order, caches).is_some_and(|metadata| !metadata.is_accepted);

        if needs_acceptance
            && let Some(accepted) = create_order_accepted(order, ts, tn, caches, account_id, clock)
        {
            emitter.send_order_event(OrderEventAny::Accepted(accepted));
        }

        emitter.send_order_event(OrderEventAny::Filled(event));
    } else if let Some(report) = create_fill_report(
        order,
        execution,
        ts,
        tn,
        caches,
        account_id,
        instruments,
        commission,
        clock,
    ) {
        emitter.send_fill_report(report);
    } else {
        return;
    }

    if complete {
        cleanup_terminal_order_tracking(order, caches);
    }
}

fn calculate_fill_commission(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_fee_rates: &AtomicMap<Option<Ustr>, MakerTakerFeeRates>,
) -> anyhow::Result<Money> {
    let instruments = instruments.load();
    let instrument = instruments
        .get(&order.s)
        .context("AX fill instrument is not cached")?;
    let rates = account_fee_rates.load();
    let rates = rates
        .get(&execution.aid.or(order.aid))
        .context("AX fill account fee rates are not cached")?;

    let liquidity_side = if execution.agg {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let rate = rates.rate_for(liquidity_side)?;

    // Keep notional in Decimal so sub-cent rounding happens only after applying the rate
    let commission = Decimal::from(execution.q)
        .checked_mul(execution.p.abs())
        .and_then(|notional| notional.checked_mul(instrument.multiplier().as_decimal()))
        .and_then(|notional| notional.checked_mul(rate))
        .context("AX fill commission calculation overflow")?;

    Money::from_decimal(commission, Currency::USD())
        .context("failed to convert AX fill commission to Money")
}

pub(crate) fn lookup_order_metadata<'a>(
    order: &AxWsOrder,
    caches: &'a OrdersCaches,
) -> Option<dashmap::mapref::one::Ref<'a, ClientOrderId, OrderMetadata>> {
    let client_order_id = VenueOrderId::new_checked(&order.oid)
        .ok()
        .and_then(|venue_order_id| {
            caches
                .venue_to_client_id
                .get(&venue_order_id)
                .map(|entry| *entry)
        });

    if let Some(client_order_id) = client_order_id
        && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
    {
        return Some(metadata);
    }

    if let Some(cid) = order.cid
        && let Some(client_order_id) = caches.cid_to_client_order_id.get(&cid).map(|entry| *entry)
        && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
    {
        return Some(metadata);
    }

    None
}

fn is_valid_order_update(
    order: &AxWsOrder,
    caches: &OrdersCaches,
    replacement_id: Option<VenueOrderId>,
    allow_previous: bool,
) -> bool {
    let Ok(venue_order_id) = VenueOrderId::new_checked(&order.oid) else {
        return false;
    };

    let Some(metadata) = lookup_order_metadata(order, caches) else {
        return true;
    };

    if metadata.instrument_id.symbol.inner() != order.s {
        return false;
    }

    if let Some(order_init) = &metadata.order_init
        && order_init.order_side != OrderSide::from(order.d)
    {
        return false;
    }

    let is_current = metadata
        .venue_order_id
        .is_none_or(|current| current == venue_order_id || replacement_id == Some(current));
    let client_order_id = metadata.client_order_id;
    drop(metadata);

    is_current
        || (allow_previous
            && caches
                .venue_to_client_id
                .get(&venue_order_id)
                .is_some_and(|mapped| *mapped == client_order_id))
}

pub(crate) fn replacement_venue_order_id(
    message: &crate::websocket::messages::AxWsOrderReplaced,
) -> anyhow::Result<VenueOrderId> {
    match (&message.noid, &message.no) {
        (Some(noid), Some(order)) if noid == &order.oid => {
            VenueOrderId::new_checked(noid).map_err(anyhow::Error::from)
        }
        (None, None) => VenueOrderId::new_checked(&message.ro.oid).map_err(anyhow::Error::from),
        _ => anyhow::bail!("Inconsistent AX replacement order identity"),
    }
}

pub(crate) fn create_order_accepted(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderAccepted> {
    if !is_valid_order_update(order, caches, None, false) {
        return None;
    }

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let metadata = lookup_order_metadata(order, caches)?;

    if metadata.is_accepted {
        return None;
    }

    let client_order_id = metadata.client_order_id;
    let trader_id = metadata.trader_id;
    let strategy_id = metadata.strategy_id;
    let instrument_id = metadata.instrument_id;
    drop(metadata);

    let mut entry = caches.orders_metadata.get_mut(&client_order_id)?;
    if entry.is_accepted
        || entry
            .venue_order_id
            .is_some_and(|current| current != venue_order_id)
    {
        return None;
    }

    entry.venue_order_id = Some(venue_order_id);
    entry.is_accepted = true;
    drop(entry);
    caches
        .venue_to_client_id
        .insert(venue_order_id, client_order_id);

    Some(OrderAccepted::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        account_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
    ))
}

pub(crate) fn create_order_updated(
    order: &AxWsOrder,
    replaced_order: &AxWsOrder,
    replacement_venue_order_id: VenueOrderId,
    event_timestamp: (i64, i64),
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderUpdated> {
    let metadata = lookup_order_metadata(order, caches)
        .or_else(|| lookup_order_metadata(replaced_order, caches))?;

    if metadata.instrument_id.symbol.inner() != order.s
        || metadata
            .order_init
            .as_ref()
            .is_some_and(|order_init| order_init.order_side != OrderSide::from(order.d))
    {
        log::warn!(
            "Deferring inconsistent AX replacement, order_id={}",
            order.oid
        );
        return None;
    }

    let client_order_id = metadata.client_order_id;
    let trader_id = metadata.trader_id;
    let strategy_id = metadata.strategy_id;
    let instrument_id = metadata.instrument_id;
    let price_precision = metadata.price_precision;
    let size_precision = metadata.size_precision;
    drop(metadata);

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_timestamp.0, event_timestamp.1)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    let quantity = Quantity::from_decimal_dp(Decimal::from(order.q), size_precision).ok()?;
    let price = Some(Price::from_decimal_dp(order.p, price_precision).ok()?);
    let previous_venue_order_id = VenueOrderId::new_checked(&replaced_order.oid).ok()?;

    if !record_replacement_venue_id(
        caches,
        client_order_id,
        previous_venue_order_id,
        replacement_venue_order_id,
        true,
    ) {
        return None;
    }

    complete_pending_modify(caches, previous_venue_order_id);

    Some(OrderUpdated::new(
        trader_id,
        strategy_id,
        instrument_id,
        client_order_id,
        quantity,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(replacement_venue_order_id),
        Some(account_id),
        price,
        None, // trigger_price
        None, // protection_price
        false,
    ))
}

#[expect(clippy::too_many_arguments)]
pub(crate) fn create_order_filled(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    commission: Money,
    clock: &'static AtomicTime,
) -> Option<OrderFilled> {
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    let last_qty =
        Quantity::from_decimal_dp(Decimal::from(execution.q), metadata.size_precision).ok()?;
    let last_px = Price::from_decimal_dp(execution.p, metadata.price_precision).ok()?;

    let order_side = metadata.order_init.as_ref().map_or_else(
        || OrderSide::from(order.d),
        |order_init| order_init.order_side,
    );
    let order_type = metadata
        .order_init
        .as_ref()
        .map_or(OrderType::Limit, |order_init| order_init.order_type);
    let trade_id = TradeId::new_checked(&execution.tid).ok()?;

    let liquidity_side = if execution.agg {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    Some(OrderFilled::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        venue_order_id,
        account_id,
        trade_id,
        order_side,
        order_type,
        last_qty,
        last_px,
        metadata.quote_currency,
        liquidity_side,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        None,
        Some(commission),
        None,
    ))
}

pub(crate) fn create_order_canceled(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderCanceled> {
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    Some(OrderCanceled::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(account_id),
        None,
    ))
}

pub(crate) fn create_order_expired(
    order: &AxWsOrder,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderExpired> {
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;

    Some(OrderExpired::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(account_id),
    ))
}

pub(crate) fn create_order_rejected(
    order: &AxWsOrder,
    reason: &str,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> Option<OrderRejected> {
    let metadata = lookup_order_metadata(order, caches)?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let due_post_only = reason.contains(AX_POST_ONLY_REJECT);

    Some(OrderRejected::new(
        metadata.trader_id,
        metadata.strategy_id,
        metadata.instrument_id,
        metadata.client_order_id,
        account_id,
        Ustr::from(reason),
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        due_post_only,
    ))
}

fn cleanup_closed_order_status_report(report: &OrderStatusReport, caches: &OrdersCaches) {
    if caches.pending_modifies.contains_key(&report.venue_order_id) {
        return;
    }

    cleanup_resolved_order_status_report(report, caches);
}

fn cleanup_resolved_order_status_report(report: &OrderStatusReport, caches: &OrdersCaches) {
    if !report.order_status.is_closed() {
        return;
    }

    let client_order_id = report.client_order_id.or_else(|| {
        caches
            .venue_to_client_id
            .get(&report.venue_order_id)
            .map(|entry| *entry)
    });

    if let Some(client_order_id) = client_order_id
        && let Some(metadata) = caches.orders_metadata.get(&client_order_id)
        && metadata
            .venue_order_id
            .is_some_and(|current| current != report.venue_order_id)
    {
        return;
    }

    cleanup_terminal_order_tracking_ids(
        caches,
        Some(&report.venue_order_id),
        report.client_order_id.as_ref().map(client_order_id_to_cid),
        report.client_order_id,
    );
}

pub(crate) fn cleanup_terminal_order_tracking(order: &AxWsOrder, caches: &OrdersCaches) {
    if !is_valid_order_update(order, caches, None, false) {
        return;
    }

    let Ok(venue_order_id) = VenueOrderId::new_checked(&order.oid) else {
        return;
    };

    cleanup_terminal_order_tracking_ids(caches, Some(&venue_order_id), order.cid, None);
}

fn cleanup_terminal_order_tracking_ids(
    caches: &OrdersCaches,
    venue_order_id: Option<&VenueOrderId>,
    cid: Option<u64>,
    known_client_order_id: Option<ClientOrderId>,
) {
    // A terminal parent can precede the replacement result on the stream
    if let Some(id) = venue_order_id
        && let Some(mut pending) = caches.pending_modifies.get_mut(id)
    {
        pending.2 = true;
        return;
    }

    let client_order_id = venue_order_id
        .and_then(|venue_order_id| {
            caches
                .venue_to_client_id
                .remove(venue_order_id)
                .map(|(_, v)| v)
        })
        .or_else(|| cid.and_then(|cid| caches.cid_to_client_order_id.remove(&cid).map(|(_, v)| v)))
        .or(known_client_order_id);

    if let Some(client_order_id) = client_order_id {
        caches.orders_metadata.remove(&client_order_id);
        caches
            .pending_modifies
            .retain(|_, (owner, _, _)| *owner != client_order_id);
        caches
            .venue_to_client_id
            .retain(|_, mapped_client_order_id| *mapped_client_order_id != client_order_id);
        caches
            .cid_to_client_order_id
            .retain(|_, mapped_client_order_id| *mapped_client_order_id != client_order_id);
    }

    if let Some(cid) = cid {
        caches.cid_to_client_order_id.remove(&cid);
    }
}

fn cleanup_deferred_order_tracking(caches: &OrdersCaches) {
    let terminal: Vec<_> = caches
        .pending_modifies
        .iter()
        .filter_map(|entry| entry.2.then_some((*entry.key(), entry.0)))
        .collect();

    for (venue_order_id, client_order_id) in terminal {
        cleanup_deferred_order_tracking_ids(caches, venue_order_id, client_order_id);
    }
}

fn complete_pending_modify(caches: &OrdersCaches, venue_order_id: VenueOrderId) {
    let completed = caches
        .pending_modifies
        .remove_if_mut(&venue_order_id, |_, (_, count, _)| {
            *count -= 1;
            *count == 0
        });

    if let Some((_, (client_order_id, _, terminal))) = completed {
        if !caches
            .pending_modifies
            .iter()
            .any(|entry| entry.0 == client_order_id)
        {
            let current = caches
                .orders_metadata
                .get(&client_order_id)
                .and_then(|metadata| metadata.venue_order_id);
            caches
                .venue_to_client_id
                .retain(|venue_id, owner| *owner != client_order_id || Some(*venue_id) == current);
        }

        if terminal {
            cleanup_deferred_order_tracking_ids(caches, venue_order_id, client_order_id);
        }
    }
}

fn cleanup_deferred_order_tracking_ids(
    caches: &OrdersCaches,
    venue_order_id: VenueOrderId,
    client_order_id: ClientOrderId,
) {
    let is_current = caches
        .orders_metadata
        .get(&client_order_id)
        .is_some_and(|metadata| metadata.venue_order_id == Some(venue_order_id));
    if is_current {
        caches.pending_modifies.remove(&venue_order_id);
        cleanup_terminal_order_tracking_ids(
            caches,
            Some(&venue_order_id),
            None,
            Some(client_order_id),
        );
    }
}

async fn resolve_order_status_report(
    http_client: &AxHttpClient,
    caches: &OrdersCaches,
    client_order_id: ClientOrderId,
    report: OrderStatusReport,
) -> anyhow::Result<Option<OrderStatusReport>> {
    let parent = report.venue_order_id;
    let stale = caches
        .orders_metadata
        .get(&client_order_id)
        .and_then(|metadata| metadata.venue_order_id)
        .is_some_and(|current| current != parent);

    if stale {
        return Ok(None);
    }

    if !report.order_status.is_closed() {
        return Ok(Some(report));
    }

    let candidates: Vec<_> = caches
        .venue_to_client_id
        .iter()
        .filter_map(|entry| {
            (*entry.value() == client_order_id && *entry.key() != parent).then_some(*entry.key())
        })
        .collect();

    if candidates.is_empty() && !caches.pending_modifies.contains_key(&parent) {
        return Ok(Some(report));
    }

    let status = http_client
        .inner
        .get_order_status_by_id(parent.as_str())
        .await?;

    if status.status.state != AxOrderStatus::Replaced {
        return Ok(Some(report));
    }

    for candidate in candidates {
        let Some(candidate_report) = http_client
            .request_order_report(
                report.account_id,
                Some(client_order_id),
                Some(candidate),
                None::<fn(u64) -> Option<ClientOrderId>>,
            )
            .await?
        else {
            continue;
        };

        anyhow::ensure!(
            candidate_report.venue_order_id == candidate
                && candidate_report.instrument_id == report.instrument_id,
            "AX replacement report identity does not match the tracked order"
        );

        if candidate_report.order_status == OrderStatus::Rejected {
            continue;
        }

        if candidate_report.order_status == OrderStatus::Canceled {
            let status = http_client
                .inner
                .get_order_status_by_id(candidate.as_str())
                .await?;

            if status.status.state == AxOrderStatus::Replaced {
                continue;
            }
        }

        if record_replacement_venue_id(caches, client_order_id, parent, candidate, false) {
            return Ok(Some(candidate_report));
        }

        return Ok(None);
    }

    log::warn!(
        "AX replaced parent {parent} has no confirmed working replacement, awaiting reconciliation"
    );
    Ok(None)
}

fn record_replacement_proposal(
    caches: &OrdersCaches,
    client_order_id: ClientOrderId,
    parent: VenueOrderId,
    proposed: VenueOrderId,
) {
    let Some(pending) = caches.pending_modifies.get(&parent) else {
        return;
    };

    let current = caches
        .orders_metadata
        .get(&client_order_id)
        .and_then(|metadata| metadata.venue_order_id);

    if pending.0 == client_order_id && current == Some(parent) {
        caches.venue_to_client_id.insert(proposed, client_order_id);
    }
}

fn record_replacement_venue_id(
    caches: &OrdersCaches,
    client_order_id: ClientOrderId,
    previous_venue_order_id: VenueOrderId,
    venue_order_id: VenueOrderId,
    remove_previous_venue_ids: bool,
) -> bool {
    let Some(mut entry) = caches.orders_metadata.get_mut(&client_order_id) else {
        return false;
    };

    if entry
        .venue_order_id
        .is_some_and(|current| current != previous_venue_order_id && current != venue_order_id)
    {
        log::warn!("Deferring stale AX replacement for {client_order_id}");
        return false;
    }

    entry.venue_order_id = Some(venue_order_id);
    drop(entry);
    caches
        .venue_to_client_id
        .insert(venue_order_id, client_order_id);

    if remove_previous_venue_ids {
        caches
            .venue_to_client_id
            .retain(|mapped_venue_order_id, mapped_client_order_id| {
                *mapped_client_order_id != client_order_id
                    || *mapped_venue_order_id == venue_order_id
            });
    }

    true
}

#[expect(clippy::too_many_arguments)]
fn create_order_status_report(
    order: &AxWsOrder,
    order_status: OrderStatus,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    clock: &'static AtomicTime,
) -> Option<OrderStatusReport> {
    if lookup_order_metadata(order, caches).is_some() {
        return None;
    }

    let instruments_snap = instruments.load();
    let instrument = instruments_snap.get(&order.s)?;
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let instrument_id = instrument.id();
    let order_side = OrderSide::from(order.d);

    let time_in_force = TimeInForce::try_from(order.tif)
        .map_err(|e| {
            log::warn!(
                "Cannot map AX order time in force, order_id={}, instrument={}, error={e}",
                order.oid,
                order.s
            );
        })
        .ok()?;

    let quantity =
        Quantity::from_decimal_dp(Decimal::from(order.q), instrument.size_precision()).ok()?;
    let filled_qty =
        Quantity::from_decimal_dp(Decimal::from(order.xq), instrument.size_precision()).ok()?;

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let ts_init = clock.get_time_ns();

    let client_order_id = order.cid.map(|cid| {
        caches
            .cid_to_client_order_id
            .get(&cid)
            .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
    });

    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        client_order_id,
        venue_order_id,
        order_side.into(),
        OrderType::Limit,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    );

    if let Ok(price) = Price::from_decimal_dp(order.p, instrument.price_precision()) {
        report = report.with_price(price);
    }

    report = report.with_post_only(order.po);

    Some(report)
}

#[expect(clippy::too_many_arguments)]
fn create_fill_report(
    order: &AxWsOrder,
    execution: &AxWsTradeExecution,
    event_ts: i64,
    event_tn: i64,
    caches: &OrdersCaches,
    account_id: AccountId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    commission: Money,
    clock: &'static AtomicTime,
) -> Option<FillReport> {
    if lookup_order_metadata(order, caches).is_some() {
        return None;
    }

    let instruments_snap = instruments.load();
    let instrument = instruments_snap.get(&order.s)?;
    let venue_order_id = VenueOrderId::new_checked(&order.oid).ok()?;
    let instrument_id = instrument.id();
    let order_side = order.d.into();

    let last_qty =
        Quantity::from_decimal_dp(Decimal::from(execution.q), instrument.size_precision()).ok()?;
    let last_px = Price::from_decimal_dp(execution.p, instrument.price_precision()).ok()?;

    let liquidity_side = if execution.agg {
        LiquiditySide::Taker
    } else {
        LiquiditySide::Maker
    };

    let ts_event = ax_timestamp_stn_to_unix_nanos(event_ts, event_tn)
        .map_err(|e| log::error!("{e}"))
        .ok()?;
    let ts_init = clock.get_time_ns();

    let client_order_id = order.cid.map(|cid| {
        caches
            .cid_to_client_order_id
            .get(&cid)
            .map_or_else(|| cid_to_client_order_id(cid), |v| *v)
    });

    Some(FillReport::new(
        account_id,
        instrument_id,
        venue_order_id,
        TradeId::new(&execution.tid),
        order_side,
        last_qty,
        last_px,
        commission,
        liquidity_side,
        client_order_id,
        None,
        ts_event,
        ts_init,
        Some(UUID4::new()),
    ))
}

fn validate_order_for_ax_submit(order: &OrderAny) -> anyhow::Result<()> {
    if !matches!(order.order_type(), OrderType::Market | OrderType::Limit) {
        anyhow::bail!(
            "Unsupported order type: {:?}, the Architect AX adapter accepts Nautilus MARKET and LIMIT orders",
            order.order_type(),
        );
    }

    // AX accepts only GTC, IOC, and DAY; deny others locally to avoid an opaque venue error
    if !matches!(
        order.time_in_force(),
        TimeInForce::Gtc | TimeInForce::Ioc | TimeInForce::Day
    ) {
        anyhow::bail!(
            "Unsupported time in force: {:?}, AX supports GTC, IOC, and DAY",
            order.time_in_force(),
        );
    }

    validate_order_instructions(
        order.is_reduce_only(),
        order.is_quote_quantity(),
        order.display_qty().is_some(),
    )?;

    quantity_to_contracts(order.quantity())?;

    Ok(())
}

fn validate_order_init_instructions(order_init: &OrderInitialized) -> anyhow::Result<()> {
    validate_order_instructions(
        order_init.reduce_only,
        order_init.quote_quantity,
        order_init.display_qty.is_some(),
    )
}

fn validate_order_instructions(
    reduce_only: bool,
    quote_quantity: bool,
    has_display_qty: bool,
) -> anyhow::Result<()> {
    if reduce_only {
        anyhow::bail!("AX does not support reduce-only orders");
    }

    if quote_quantity {
        anyhow::bail!(OrderDeniedReason::unsupported_quote_quantity());
    }

    if has_display_qty {
        anyhow::bail!("Architect AX adapter cannot encode display_qty iceberg instructions");
    }

    Ok(())
}

// The AX HTTP API documents only a bare 400 with no error schema, so no venue
// failure can be allowlisted as an unambiguous rejection; those arrive via the
// orders WS `Rejected` and `CancelRejected` events instead. AX therefore never
// classifies a send failure as `CommandFailure::VenueRejected`.
fn classify_ax_http_failure(error: &AxHttpError) -> CommandFailure {
    let message = error.to_string();
    match error {
        AxHttpError::MissingCredentials
        | AxHttpError::MissingSessionToken
        | AxHttpError::ValidationError(_)
        | AxHttpError::BuildError(_) => CommandFailure::NotSent(message),
        AxHttpError::ApiError { .. }
        | AxHttpError::JsonError(_)
        | AxHttpError::Canceled(_)
        | AxHttpError::NetworkError(_)
        | AxHttpError::UnexpectedStatus { .. } => CommandFailure::Ambiguous(message),
    }
}

fn classify_ax_ws_failure(error: &AxOrdersWsClientError) -> CommandFailure {
    match error {
        AxOrdersWsClientError::ClientError(message) => CommandFailure::NotSent(message.clone()),
        AxOrdersWsClientError::Transport(_)
        | AxOrdersWsClientError::ChannelError(_)
        | AxOrdersWsClientError::AuthenticationError(_) => {
            CommandFailure::Ambiguous(error.to_string())
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, net::SocketAddr, rc::Rc, sync::Arc, time::Duration};

    use dashmap::DashMap;
    use nautilus_common::{
        cache::Cache,
        live::runner::replace_exec_event_sender,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_live::{ExecutionClientCore, testing::ExecutionHarness};
    use nautilus_model::{
        enums::AssetClass,
        events::order::spec::OrderSubmittedSpec,
        identifiers::{
            AccountId, ClientOrderId, InstrumentId, StrategyId, Symbol, TraderId, VenueOrderId,
        },
        instruments::{InstrumentAny, PerpetualContract},
        orders::{builder::OrderTestBuilder, stubs::TestOrderEventStubs},
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::{
            consts::{AX_CLIENT_ID, AX_VENUE},
            enums::{AxEnvironment, AxOrderSide, AxOrderStatus, AxTimeInForce},
        },
        config::AxExecutionClientConfig,
        http::{error::AxBuildError, models::AxFillsResponse},
        websocket::{
            messages::{AxWsOrderExpired, AxWsOrderFilled, AxWsTradeExecution, OrderMetadata},
            orders::OrdersCaches,
        },
    };

    #[rstest]
    fn test_start_replaces_sender_for_emitter_clone() {
        std::thread::spawn(|| {
            let core = ExecutionClientCore::new(
                TraderId::from("TESTER-002"),
                *AX_CLIENT_ID,
                *AX_VENUE,
                OmsType::Netting,
                AccountId::from("AX-002"),
                AccountType::Margin,
                None,
                Rc::new(RefCell::new(Cache::default())),
            );
            let mut client =
                AxExecutionClient::new(core, AxExecutionClientConfig::default()).unwrap();
            let emitter = client.emitter.clone();
            let (sender_initial, mut receiver_initial) = tokio::sync::mpsc::unbounded_channel();
            let (sender_current, mut receiver_current) = tokio::sync::mpsc::unbounded_channel();
            let event = OrderEventAny::Submitted(
                OrderSubmittedSpec::builder()
                    .trader_id(TraderId::from("TESTER-002"))
                    .account_id(AccountId::from("AX-002"))
                    .client_order_id(ClientOrderId::from("O-SENDER-002"))
                    .build(),
            );

            client.set_event_sender(sender_initial.into());
            emitter.send_order_event(event.clone());

            let ExecutionEvent::Order(received) = receiver_initial.try_recv().unwrap() else {
                panic!("Expected order event");
            };
            assert_eq!(received, event);

            replace_exec_event_sender(sender_current);
            client.start().unwrap();
            emitter.send_order_event(event.clone());

            let ExecutionEvent::Order(received) = receiver_current.try_recv().unwrap() else {
                panic!("Expected order event");
            };
            assert_eq!(received, event);
            assert_eq!(
                receiver_initial.try_recv().unwrap_err(),
                tokio::sync::mpsc::error::TryRecvError::Disconnected
            );
            assert_eq!(
                receiver_current.try_recv().unwrap_err(),
                tokio::sync::mpsc::error::TryRecvError::Empty
            );
        })
        .join()
        .unwrap();
    }

    fn test_caches() -> OrdersCaches {
        OrdersCaches {
            orders_metadata: Arc::new(DashMap::new()),
            venue_to_client_id: Arc::new(DashMap::new()),
            cid_to_client_order_id: Arc::new(DashMap::new()),
            pending_modifies: Arc::new(DashMap::new()),
        }
    }

    fn test_ws_order(oid: &str, price: Decimal, qty: u64) -> AxWsOrder {
        AxWsOrder {
            aid: None,
            po: false,
            rb: None,
            r: None,
            oid: oid.to_string(),
            u: Ustr::from("user"),
            s: Ustr::from("BTC-PERP"),
            p: price,
            q: qty,
            xq: 0,
            rq: qty,
            o: AxOrderStatus::Accepted,
            d: AxOrderSide::Buy,
            tif: AxTimeInForce::Gtc,
            ts: 1609459200,
            tn: 0,
            cid: None,
            tag: None,
            txt: None,
        }
    }

    #[rstest]
    fn test_create_order_updated_uses_ws_replacement_id_before_http_response() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-WS-FIRST");
        let old_venue_order_id = VenueOrderId::new("OLD-OID");
        let new_venue_order_id = VenueOrderId::new("NEW-OID");

        let mut metadata = test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX"));
        metadata.venue_order_id = Some(old_venue_order_id);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches
            .venue_to_client_id
            .insert(old_venue_order_id, client_order_id);

        let old_order = test_ws_order(old_venue_order_id.as_str(), dec!(50000.00), 100);
        let new_order = test_ws_order(new_venue_order_id.as_str(), dec!(50001.00), 100);
        let event = create_order_updated(
            &new_order,
            &old_order,
            new_venue_order_id,
            (1609459200, 0),
            &caches,
            account_id,
            clock,
        )
        .expect("should produce OrderUpdated");

        assert_eq!(event.venue_order_id, Some(new_venue_order_id));
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(new_venue_order_id),
        );
        assert!(!caches.venue_to_client_id.contains_key(&old_venue_order_id));
        assert_eq!(
            *caches.venue_to_client_id.get(&new_venue_order_id).unwrap(),
            client_order_id,
        );
    }

    #[rstest]
    fn test_pending_replacement_retains_parent_until_ws_event() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-HTTP-FIRST");
        let old_venue_order_id = VenueOrderId::new("OLD-OID");
        let new_venue_order_id = VenueOrderId::new("NEW-OID");
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX"));
        metadata.venue_order_id = Some(old_venue_order_id);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches
            .venue_to_client_id
            .insert(old_venue_order_id, client_order_id);

        caches
            .pending_modifies
            .insert(old_venue_order_id, (client_order_id, 1, false));
        let mut parent = test_ws_order(old_venue_order_id.as_str(), dec!(50000), 100);
        parent.o = AxOrderStatus::Filled;
        parent.xq = 100;
        parent.rq = 0;
        cleanup_terminal_order_tracking(&parent, &caches);

        assert!(caches.venue_to_client_id.contains_key(&old_venue_order_id));
        assert!(!caches.venue_to_client_id.contains_key(&new_venue_order_id));
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(old_venue_order_id),
        );

        record_replacement_venue_id(
            &caches,
            client_order_id,
            old_venue_order_id,
            new_venue_order_id,
            true,
        );

        assert!(!caches.venue_to_client_id.contains_key(&old_venue_order_id));
        assert!(caches.venue_to_client_id.contains_key(&new_venue_order_id));
    }

    #[rstest]
    fn test_late_replacement_cannot_regress_active_leg() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CHAIN");
        let parent = VenueOrderId::from("OID-A");
        let child = VenueOrderId::from("OID-B");
        let current = VenueOrderId::from("OID-C");
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX"));
        metadata.venue_order_id = Some(parent);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.venue_to_client_id.insert(parent, client_order_id);
        assert!(record_replacement_venue_id(
            &caches,
            client_order_id,
            parent,
            child,
            true
        ));
        assert!(record_replacement_venue_id(
            &caches,
            client_order_id,
            child,
            current,
            true
        ));
        let late_replacement =
            record_replacement_venue_id(&caches, client_order_id, parent, child, true);
        let old_order = test_ws_order(parent.as_str(), dec!(50000), 100);
        let new_order = test_ws_order(child.as_str(), dec!(50001), 100);
        caches.cid_to_client_order_id.insert(127, client_order_id);
        let mut old_order = old_order;
        old_order.cid = Some(127);
        let late_event = create_order_updated(
            &new_order,
            &old_order,
            child,
            (1609459200, 0),
            &caches,
            AccountId::from("AX-001"),
            get_atomic_clock_realtime(),
        );

        assert!(!late_replacement);
        assert!(late_event.is_none());
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(current)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&current).unwrap(),
            client_order_id
        );
        assert_eq!(caches.venue_to_client_id.len(), 1);
    }

    #[rstest]
    #[case::timestamp(0)]
    #[case::price(1)]
    #[case::instrument(2)]
    #[case::side(3)]
    fn test_malformed_replacement_does_not_promote(#[case] invalid: u8) {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-BAD-REPLACE");
        let parent = VenueOrderId::from("OID-PARENT");
        let child = VenueOrderId::from("OID-CHILD");
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX"));
        metadata.venue_order_id = Some(parent);
        let submitted = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("BTC-PERP.AX"))
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .price(Price::from("50000.00"))
            .build();
        metadata.order_init = Some(submitted.init_event().clone());
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.venue_to_client_id.insert(parent, client_order_id);
        let old_order = test_ws_order(parent.as_str(), dec!(50000), 100);

        let mut new_order = test_ws_order(
            child.as_str(),
            if invalid == 1 {
                Decimal::MAX
            } else {
                dec!(50001)
            },
            101,
        );

        if invalid == 2 {
            new_order.s = Ustr::from("WRONG-PERP");
        }

        if invalid == 3 {
            new_order.d = AxOrderSide::Sell;
        }

        let event = create_order_updated(
            &new_order,
            &old_order,
            child,
            (if invalid == 0 { -1 } else { 1609459200 }, 0),
            &caches,
            AccountId::from("AX-001"),
            get_atomic_clock_realtime(),
        );

        let mut message: crate::websocket::messages::AxWsOrderReplaced =
            serde_json::from_str(include_str!("../test_data/ws_order_replaced_live.json")).unwrap();
        message.ro = Box::new(old_order);
        message.no = Some(Box::new(new_order));
        message.noid = Some(child.to_string());
        if invalid == 0 {
            message.ts = -1;
        }

        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from("BTC-PERP"), test_perp_instrument("BTC-PERP"));
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            AxWsOrderEvent::Replaced(message),
            &emitter,
            &caches,
            account_id,
            &instruments,
            &AtomicMap::new(),
            clock,
        );

        assert!(rx.try_recv().is_err());
        assert!(event.is_none());
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(parent)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&parent).unwrap(),
            client_order_id
        );
        assert_eq!(caches.venue_to_client_id.len(), 1);
    }

    fn test_metadata(client_order_id: ClientOrderId, instrument_id: InstrumentId) -> OrderMetadata {
        OrderMetadata {
            trader_id: TraderId::from("TRADER-001"),
            strategy_id: StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            order_init: None,
            is_accepted: true,
            venue_order_id: None,
            ts_init: 0.into(),
            size_precision: 0,
            price_precision: 2,
            quote_currency: Currency::USD(),
        }
    }

    fn test_execution(tid: &str, price: Decimal, qty: u64, agg: bool) -> AxWsTradeExecution {
        AxWsTradeExecution {
            aid: None,
            tid: tid.to_string(),
            s: Ustr::from("BTC-PERP"),
            q: qty,
            p: price,
            d: AxOrderSide::Buy,
            agg,
        }
    }

    fn seed_terminal_tracking(
        caches: &OrdersCaches,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        stale_venue_order_id: VenueOrderId,
        cid_value: u64,
    ) {
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX")),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches
            .venue_to_client_id
            .insert(stale_venue_order_id, client_order_id);
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);
    }

    fn test_perp_instrument(symbol: &str) -> InstrumentAny {
        let symbol = Symbol::new(symbol);
        let instrument = PerpetualContract::builder()
            .instrument_id(InstrumentId::new(symbol, *AX_VENUE))
            .raw_symbol(symbol)
            .underlying(Ustr::from("BTC"))
            .asset_class(AssetClass::Cryptocurrency)
            .quote_currency(Currency::USD())
            .settlement_currency(Currency::USD())
            .is_inverse(false)
            .price_precision(2)
            .size_precision(0)
            .price_increment(Price::from("0.01"))
            .size_increment(Quantity::from("1"))
            .margin_init(Decimal::new(1, 2))
            .margin_maint(Decimal::new(5, 3))
            .ts_event(0.into())
            .ts_init(0.into())
            .build()
            .unwrap();
        InstrumentAny::PerpetualContract(instrument)
    }

    async fn start_orders_report_server(body: serde_json::Value) -> SocketAddr {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind report server");
        let addr = listener.local_addr().expect("report server addr");

        tokio::spawn(async move {
            let mut open_body = body.clone();
            open_body["total_count"] = serde_json::json!(body["orders"].as_array().unwrap().len());
            open_body["limit"] = serde_json::json!(100);
            open_body["offset"] = serde_json::json!(0);
            let order = &body["orders"][0];
            let status_body = serde_json::json!({"status": {
                "order_id": order["oid"], "state": order["o"], "symbol": order["s"],
                "clord_id": order["cid"], "filled_quantity": order["xq"], "remaining_quantity": order["rq"]
            }});

            let app = axum::Router::new()
                .route(
                    "/order-status",
                    axum::routing::get(move || async { axum::Json(status_body) }),
                )
                .route(
                    "/orders",
                    axum::routing::get(move || async { axum::Json(body) }),
                )
                .route(
                    "/open-orders",
                    axum::routing::get(move || async { axum::Json(open_body) }),
                )
                .route(
                    "/fills",
                    axum::routing::get(|| async {
                        axum::Json(AxFillsResponse {
                            fills: Vec::new(),
                            total_count: Some(0),
                            limit: Some(100),
                            next_cursor: None,
                        })
                    }),
                );
            axum::serve(listener, app).await.unwrap();
        });

        for _ in 0..50 {
            if tokio::net::TcpStream::connect(addr).await.is_ok() {
                break;
            }

            tokio::time::sleep(Duration::from_millis(10)).await;
        }

        addr
    }

    fn test_status_report(
        client_order_id: Option<ClientOrderId>,
        venue_order_id: VenueOrderId,
        order_status: OrderStatus,
    ) -> OrderStatusReport {
        OrderStatusReport::new(
            AccountId::from("AX-001"),
            InstrumentId::from("BTC-PERP.AX"),
            client_order_id,
            venue_order_id,
            Some(OrderSide::Buy),
            OrderType::Limit,
            TimeInForce::Gtc,
            order_status,
            Quantity::from("1"),
            Quantity::from("1"),
            0.into(),
            0.into(),
            0.into(),
            None,
        )
    }

    #[rstest]
    fn test_create_order_accepted_populates_cache_and_event() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-ACK");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-ACK");

        let mut metadata = test_metadata(client_order_id, instrument_id);
        metadata.is_accepted = false;
        caches.orders_metadata.insert(client_order_id, metadata);
        let cid_value = 7u64;
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        let mut ws_order = test_ws_order(venue_order_id.as_str(), dec!(50500.00), 100);
        ws_order.cid = Some(cid_value);

        let event = create_order_accepted(&ws_order, 1609459200, 500, &caches, account_id, clock)
            .expect("should produce OrderAccepted");

        assert_eq!(event.venue_order_id, venue_order_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.account_id, account_id);
        assert_eq!(event.instrument_id, instrument_id);
        assert_eq!(event.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(event.strategy_id, StrategyId::from("S-001"));
        assert_eq!(
            event.ts_event,
            UnixNanos::from(1_609_459_200_000_000_500u64)
        );

        // Side effects on caches
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id,
        );
        let meta = caches.orders_metadata.get(&client_order_id).unwrap();
        assert_eq!(meta.venue_order_id, Some(venue_order_id));
    }

    #[rstest]
    #[case::unbound_partial(false, false)]
    #[case::response_before_partial(true, false)]
    #[case::unbound_complete(false, true)]
    #[case::response_before_complete(true, true)]
    fn test_market_fill_before_acknowledgement(#[case] bound: bool, #[case] complete: bool) {
        let raw = if complete {
            include_str!("../test_data/ws_order_filled.json")
        } else {
            include_str!("../test_data/ws_order_partially_filled.json")
        };

        let mut event: AxWsOrderEvent = serde_json::from_str(raw).unwrap();

        let order = match &mut event {
            AxWsOrderEvent::Filled(msg) => &mut msg.o,
            AxWsOrderEvent::PartiallyFilled(msg) => &mut msg.o,
            _ => unreachable!(),
        };

        order.cid = Some(117);
        let venue_order_id = VenueOrderId::from(order.oid.as_str());
        let client_order_id = ClientOrderId::from("O-ORIGINAL-MARKET");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let local_order = OrderTestBuilder::new(OrderType::Market)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("100"))
            .time_in_force(TimeInForce::Ioc)
            .build();
        let mut metadata = test_metadata(client_order_id, instrument_id);
        metadata.is_accepted = false;
        metadata.order_init = Some(local_order.init_event().clone());
        metadata.venue_order_id = bound.then_some(venue_order_id);
        let caches = test_caches();
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.cid_to_client_order_id.insert(117, client_order_id);
        if bound {
            caches
                .venue_to_client_id
                .insert(venue_order_id, client_order_id);
        }

        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = rx.try_recv().unwrap()
        else {
            panic!("expected acceptance before fill");
        };

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected tracked fill");
        };

        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.venue_order_id, venue_order_id);
        assert_eq!(accepted.instrument_id, instrument_id);
        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.trader_id, local_order.trader_id());
        assert_eq!(fill.strategy_id, local_order.strategy_id());
        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.account_id, account_id);
        assert_eq!(fill.venue_order_id, venue_order_id);
        assert_eq!(fill.order_type, OrderType::Market);
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(
            fill.last_qty.as_decimal(),
            if complete { dec!(100) } else { dec!(50) }
        );
        assert_eq!(fill.last_px.as_decimal(), dec!(50000));
        assert_eq!(
            fill.commission,
            Some(Money::from(if complete {
                "1000.00 USD"
            } else {
                "500.00 USD"
            }))
        );
        assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
        assert!(rx.try_recv().is_err());

        if complete {
            assert!(caches.orders_metadata.is_empty());
            assert!(caches.cid_to_client_order_id.is_empty());
        } else {
            let metadata = caches.orders_metadata.get(&client_order_id).unwrap();
            assert_eq!(metadata.venue_order_id, Some(venue_order_id));
            assert!(metadata.is_accepted);
            assert_eq!(metadata.order_init.as_ref(), Some(local_order.init_event()));
        }
    }

    #[rstest]
    #[case::partial_first(false, false)]
    #[case::complete_first(true, false)]
    #[case::partial_second(false, true)]
    #[case::complete_second(true, true)]
    fn test_parent_fill_before_stream_replacement(#[case] complete: bool, #[case] second: bool) {
        let mut event: AxWsOrderEvent = serde_json::from_str(if complete {
            include_str!("../test_data/ws_order_filled.json")
        } else {
            include_str!("../test_data/ws_order_partially_filled.json")
        })
        .unwrap();

        let order = match &mut event {
            AxWsOrderEvent::Filled(msg) => &mut msg.o,
            AxWsOrderEvent::PartiallyFilled(msg) => &mut msg.o,
            _ => unreachable!(),
        };

        order.cid = Some(131);
        let first_id = VenueOrderId::from("OID-FIRST");
        let parent_id = VenueOrderId::from(order.oid.as_str());
        let child_id = VenueOrderId::from("OID-REPLACEMENT");
        let client_order_id = ClientOrderId::from("O-PARENT-FILL");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, instrument_id);
        metadata.venue_order_id = Some(if second { first_id } else { parent_id });
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.cid_to_client_order_id.insert(131, client_order_id);
        if second {
            caches.venue_to_client_id.insert(first_id, client_order_id);
            assert!(record_replacement_venue_id(
                &caches,
                client_order_id,
                first_id,
                parent_id,
                true
            ));
        }

        caches.venue_to_client_id.insert(parent_id, client_order_id);
        caches
            .pending_modifies
            .insert(parent_id, (client_order_id, 1, false));
        record_replacement_proposal(&caches, client_order_id, parent_id, child_id);
        let parent_order = order.clone();
        let mut child_order = parent_order.clone();
        child_order.oid = child_id.to_string();
        child_order.p = dec!(50001);
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event.clone(),
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected tracked parent fill");
        };

        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(fill.strategy_id, StrategyId::from("S-001"));
        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.account_id, account_id);
        assert_eq!(fill.venue_order_id, parent_id);
        assert_eq!(fill.order_type, OrderType::Limit);
        assert_eq!(fill.order_side, OrderSide::Buy);
        assert_eq!(
            fill.last_qty.as_decimal(),
            if complete { dec!(100) } else { dec!(50) }
        );
        assert_eq!(fill.last_px.as_decimal(), dec!(50000));
        assert_eq!(
            fill.commission,
            Some(Money::from(if complete {
                "1000.00 USD"
            } else {
                "500.00 USD"
            }))
        );
        assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
        assert_eq!(fill.ts_event, UnixNanos::from(1_609_459_200_123_456_789u64));
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(parent_id)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&parent_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&child_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&131).unwrap(),
            client_order_id
        );

        let updated = create_order_updated(
            &child_order,
            &parent_order,
            child_id,
            (1609459201, 0),
            &caches,
            account_id,
            clock,
        )
        .unwrap();
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );
        assert_eq!(updated.venue_order_id, Some(child_id));
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        assert!(!caches.venue_to_client_id.contains_key(&parent_id));
        assert_eq!(caches.venue_to_client_id.len(), 1);
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(child_id)
        );
    }

    #[rstest]
    fn test_reconciled_acceptance_suppresses_duplicate_stream_acceptance() {
        let mut event: AxWsOrderEvent =
            serde_json::from_str(include_str!("../test_data/ws_order_partially_filled.json"))
                .unwrap();

        let AxWsOrderEvent::PartiallyFilled(msg) = &mut event else {
            unreachable!()
        };

        msg.o.cid = Some(133);
        let venue_order_id = VenueOrderId::from(msg.o.oid.as_str());
        let client_order_id = ClientOrderId::from("O-RECONCILED-ACCEPTANCE");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            *AX_CLIENT_ID,
            *AX_VENUE,
            OmsType::Netting,
            AccountId::from("AX-001"),
            AccountType::Margin,
            None,
            Rc::new(RefCell::new(Cache::default())),
        );
        let mut client = AxExecutionClient::new(core, AxExecutionClientConfig::default()).unwrap();
        client.ensure_order_event_subscription();
        client.ensure_order_event_subscription();
        let caches = client.ws_orders.caches().clone();
        let mut metadata = test_metadata(client_order_id, instrument_id);
        metadata.is_accepted = false;
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.cid_to_client_order_id.insert(133, client_order_id);
        let accepted = OrderEventAny::Accepted(OrderAccepted::new(
            TraderId::from("TRADER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            client_order_id,
            venue_order_id,
            AccountId::from("AX-001"),
            UUID4::new(),
            1.into(),
            2.into(),
            true,
        ));
        msgbus::publish_order_event("events.order.S-001".into(), &accepted);
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected fill without duplicate acceptance");
        };

        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.venue_order_id, venue_order_id);
        assert_eq!(fill.last_qty, Quantity::from("50"));
        assert_eq!(
            rx.try_recv().unwrap_err(),
            tokio::sync::mpsc::error::TryRecvError::Empty
        );
        let metadata = caches.orders_metadata.get(&client_order_id).unwrap();
        assert!(metadata.is_accepted);
        assert_eq!(metadata.venue_order_id, Some(venue_order_id));
        drop(metadata);
        client.stop().unwrap();
        caches
            .orders_metadata
            .get_mut(&client_order_id)
            .unwrap()
            .is_accepted = false;
        msgbus::publish_order_event("events.order.S-001".into(), &accepted);
        assert!(
            !caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .is_accepted
        );
        assert!(client.order_event_handler.is_none());
    }

    #[rstest]
    #[case::acknowledgement(false, false)]
    #[case::terminal_parent(true, false)]
    #[case::pending_acknowledgement(false, true)]
    #[case::pending_terminal_parent(true, true)]
    fn test_stale_parent_preserves_replacement(#[case] terminal: bool, #[case] mapped: bool) {
        let mut event: AxWsOrderEvent = serde_json::from_str(if terminal {
            include_str!("../test_data/ws_order_canceled.json")
        } else {
            include_str!("../test_data/ws_order_acknowledged.json")
        })
        .unwrap();

        let order = match &mut event {
            AxWsOrderEvent::Acknowledged(msg) => &mut msg.o,
            AxWsOrderEvent::Canceled(msg) => &mut msg.o,
            _ => unreachable!(),
        };

        order.cid = Some(119);
        let parent_id = VenueOrderId::from(order.oid.as_str());
        let child_id = VenueOrderId::from("OID-ACTIVE-CHILD");
        let client_order_id = ClientOrderId::from("O-STALE-PARENT");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(child_id);
        metadata.is_accepted = true;
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.cid_to_client_order_id.insert(119, client_order_id);
        caches.venue_to_client_id.insert(child_id, client_order_id);
        if mapped {
            caches.venue_to_client_id.insert(parent_id, client_order_id);
        }

        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &AtomicMap::new(),
            clock,
        );
        let report = test_status_report(Some(client_order_id), parent_id, OrderStatus::Canceled);
        cleanup_closed_order_status_report(&report, &caches);

        assert!(rx.try_recv().is_err());
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(child_id)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&child_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&119).unwrap(),
            client_order_id
        );
        assert_eq!(caches.venue_to_client_id.len(), if mapped { 2 } else { 1 });
    }

    #[rstest]
    #[case::cancel(false)]
    #[case::modify(true)]
    fn test_stale_parent_rejection_preserves_active_leg(#[case] modify: bool) {
        let mut event: AxWsOrderEvent =
            serde_json::from_str(include_str!("../test_data/ws_cancel_rejected.json")).unwrap();

        if modify {
            let AxWsOrderEvent::CancelRejected(msg) = &mut event else {
                unreachable!()
            };

            msg.r = AxCancelRejectionReason::Unknown;
            msg.txt = Some("post-only order would cross the book".to_string());
        }

        let parent = VenueOrderId::from("O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let child = VenueOrderId::from("OID-CHILD");
        let client_order_id = ClientOrderId::from("O-STALE-CANCEL");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(child);
        caches.orders_metadata.insert(client_order_id, metadata);
        if !modify {
            caches.venue_to_client_id.insert(parent, client_order_id);
        }

        caches
            .pending_modifies
            .insert(parent, (client_order_id, 1, false));
        caches.venue_to_client_id.insert(child, client_order_id);
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &AtomicMap::new(),
            &AtomicMap::new(),
            clock,
        );

        if modify {
            let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) =
                rx.try_recv().unwrap()
            else {
                panic!("expected stale parent modify rejection");
            };

            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.venue_order_id, Some(child));
            assert_eq!(rejected.reason, "post-only order would cross the book");
        }

        assert!(rx.try_recv().is_err());
        assert_eq!(caches.pending_modifies.contains_key(&parent), !modify);
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(child)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&child).unwrap(),
            client_order_id
        );
    }

    #[rstest]
    #[case::trade_id(0)]
    #[case::timestamp(1)]
    #[case::venue_id(2)]
    #[case::instrument(3)]
    fn test_malformed_tracked_fill_preserves_context(#[case] invalid: u8) {
        let mut fill: AxWsOrderFilled =
            serde_json::from_str(include_str!("../test_data/ws_order_filled.json")).unwrap();
        fill.o.cid = Some(121);
        let venue_order_id = VenueOrderId::from(fill.o.oid.as_str());
        let client_order_id = ClientOrderId::from("O-MALFORMED");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(venue_order_id);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.cid_to_client_order_id.insert(121, client_order_id);
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        match invalid {
            0 => fill.xs.tid.clear(),
            1 => fill.ts = -1,
            2 => fill.o.oid.clear(),
            3 => fill.o.s = Ustr::from("WRONG-PERP"),
            _ => unreachable!(),
        }

        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            AxWsOrderEvent::Filled(fill),
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        assert!(rx.try_recv().is_err());
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(venue_order_id)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&121).unwrap(),
            client_order_id
        );
    }

    #[rstest]
    fn test_create_order_accepted_returns_none_without_metadata() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let ws_order = test_ws_order("OID-UNKNOWN", dec!(100.00), 10);

        let result = create_order_accepted(&ws_order, 1609459200, 0, &caches, account_id, clock);
        assert!(result.is_none());
        assert!(caches.venue_to_client_id.is_empty());
    }

    #[rstest]
    fn test_lookup_order_metadata_cid_fallback() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CID");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches.cid_to_client_order_id.insert(99, client_order_id);

        let mut ws_order = test_ws_order("UNKNOWN-OID", dec!(0), 0);
        ws_order.cid = Some(99);

        let found = lookup_order_metadata(&ws_order, &caches).expect("cid fallback should find");
        assert_eq!(found.client_order_id, client_order_id);
    }

    #[rstest]
    fn test_lookup_order_metadata_returns_none_when_unknown() {
        let caches = test_caches();
        let ws_order = test_ws_order("UNKNOWN-OID", dec!(0), 0);
        assert!(lookup_order_metadata(&ws_order, &caches).is_none());
    }

    #[rstest]
    #[case(true, LiquiditySide::Taker)]
    #[case(false, LiquiditySide::Maker)]
    fn test_create_order_filled_maps_liquidity_side(
        #[case] agg: bool,
        #[case] expected: LiquiditySide,
    ) {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-FILL");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-FILL");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(50500.00), 100);
        let execution = test_execution("TID-1", dec!(50500.00), 25, agg);

        let event = create_order_filled(
            &order,
            &execution,
            1609459200,
            0,
            &caches,
            account_id,
            Money::from("12.34 USD"),
            clock,
        )
        .expect("should produce OrderFilled");

        assert_eq!(event.venue_order_id, venue_order_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.trade_id, TradeId::new("TID-1"));
        assert_eq!(event.last_qty, Quantity::new(25.0, 0));
        assert_eq!(event.last_px, Price::from("50500.00"));
        assert_eq!(event.liquidity_side, expected);
        assert_eq!(event.commission, Some(Money::from("12.34 USD")));
    }

    #[rstest]
    #[case::maker(false, dec!(0.0002), dec!(0.0025), dec!(100.25), 20, dec!(2.5), "1.00 USD")]
    #[case::taker(true, dec!(0.0002), dec!(0.0025), dec!(100.25), 20, dec!(2.5), "12.53 USD")]
    #[case::zero(false, dec!(0), dec!(0.0025), dec!(100.25), 20, dec!(2.5), "0.00 USD")]
    #[case::rebate(false, dec!(-0.0002), dec!(0.0025), dec!(100.25), 20, dec!(2.5), "-1.00 USD")]
    #[case::negative_price(false, dec!(0.0002), dec!(0.0025), dec!(-100.25), 20, dec!(2.5), "1.00 USD")]
    #[case::sub_cent_notional(false, dec!(0.0002), dec!(0.0025), dec!(0.007499), 10_001, dec!(1), "0.01 USD")]
    fn test_calculate_fill_commission(
        #[case] agg: bool,
        #[case] maker: Decimal,
        #[case] taker: Decimal,
        #[case] price: Decimal,
        #[case] quantity: u64,
        #[case] multiplier: Decimal,
        #[case] expected: &str,
    ) {
        let mut instrument = test_perp_instrument("BTC-PERP");
        if let InstrumentAny::PerpetualContract(ref mut instrument) = instrument {
            instrument.multiplier = Quantity::from_decimal(multiplier).unwrap();
        }

        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from("BTC-PERP"), instrument);
        let rates = AtomicMap::new();
        rates.insert(None, MakerTakerFeeRates::new(maker, taker));
        let order = test_ws_order("OID-FILL", price, quantity);
        let execution = test_execution("TID-FILL", price, quantity, agg);

        let commission =
            calculate_fill_commission(&order, &execution, &instruments, &rates).unwrap();

        assert_eq!(commission, Money::from(expected));
    }

    #[rstest]
    #[case::default(None, None, "0.05 USD")]
    #[case::order_account(Some("order-account"), None, "0.10 USD")]
    #[case::execution_account(None, Some("execution-account"), "0.15 USD")]
    #[case::execution_account_precedence(
        Some("order-account"),
        Some("execution-account"),
        "0.15 USD"
    )]
    fn test_calculate_fill_commission_resolves_account(
        #[case] order_account: Option<&str>,
        #[case] execution_account: Option<&str>,
        #[case] expected: &str,
    ) {
        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from("BTC-PERP"), test_perp_instrument("BTC-PERP"));
        let rates = AtomicMap::new();
        rates.insert(None, MakerTakerFeeRates::new(dec!(0.0001), dec!(0.0002)));
        rates.insert(
            Some(Ustr::from("order-account")),
            MakerTakerFeeRates::new(dec!(0.0003), dec!(0.0004)),
        );
        rates.insert(
            Some(Ustr::from("execution-account")),
            MakerTakerFeeRates::new(dec!(0.0005), dec!(0.0006)),
        );
        let mut order = test_ws_order("OID-FILL", dec!(10), 25);
        order.aid = order_account.map(Ustr::from);
        let mut execution = test_execution("TID-FILL", dec!(10), 25, true);
        execution.aid = execution_account.map(Ustr::from);

        let commission =
            calculate_fill_commission(&order, &execution, &instruments, &rates).unwrap();

        assert_eq!(commission, Money::from(expected));
    }

    #[rstest]
    #[case::tracked_partial(true, false, "500.00 USD")]
    #[case::tracked_full(true, true, "1000.00 USD")]
    #[case::untracked_partial(false, false, "500.00 USD")]
    #[case::untracked_full(false, true, "1000.00 USD")]
    fn test_dispatch_stream_fill_carries_commission(
        #[case] tracked: bool,
        #[case] complete: bool,
        #[case] expected: &str,
    ) {
        let raw = if complete {
            include_str!("../test_data/ws_order_filled.json")
        } else {
            include_str!("../test_data/ws_order_partially_filled.json")
        };

        let event: AxWsOrderEvent = serde_json::from_str(raw).unwrap();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-FILL");
        let venue_order_id = VenueOrderId::new("O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let caches = test_caches();
        if tracked {
            caches.orders_metadata.insert(
                client_order_id,
                test_metadata(client_order_id, instrument_id),
            );
            caches
                .venue_to_client_id
                .insert(venue_order_id, client_order_id);
        }

        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);

        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        let expected_qty = if complete { dec!(100) } else { dec!(50) };

        match rx.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
                assert!(tracked);
                assert_eq!(fill.commission, Some(Money::from(expected)));
                assert_eq!(fill.client_order_id, client_order_id);
                assert_eq!(fill.venue_order_id, venue_order_id);
                assert_eq!(fill.trade_id.as_str(), "T-01ARZ3NDEKTSV4RRFFQ69G5FAV");
                assert_eq!(fill.last_qty.as_decimal(), expected_qty);
                assert_eq!(fill.last_px.as_decimal(), dec!(50000));
                assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
            }
            ExecutionEvent::Report(report) => {
                let ExecutionReport::Fill(fill) = report else {
                    panic!("expected fill report");
                };

                assert!(!tracked);
                assert_eq!(fill.commission, Money::from(expected));
                assert_eq!(fill.instrument_id, instrument_id);
                assert_eq!(fill.venue_order_id, venue_order_id);
                assert_eq!(fill.last_qty.as_decimal(), expected_qty);
                assert_eq!(fill.last_px.as_decimal(), dec!(50000));
                assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
            }
            other => panic!("unexpected fill event: {other:?}"),
        }

        assert!(rx.try_recv().is_err());
        assert_eq!(
            caches.orders_metadata.len(),
            usize::from(tracked && !complete)
        );
        assert_eq!(
            caches.venue_to_client_id.len(),
            usize::from(tracked && !complete)
        );
    }

    #[rstest]
    #[case::missing_account(Some("unknown"), dec!(100), "AX fill account fee rates are not cached")]
    #[case::overflow(None, Decimal::MAX, "AX fill commission calculation overflow")]
    fn test_dispatch_fill_fee_error_retains_tracking(
        #[case] account: Option<&str>,
        #[case] price: Decimal,
        #[case] expected: &str,
    ) {
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-FILL");
        let venue_order_id = VenueOrderId::new("OID-FILL");
        let caches = test_caches();
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX")),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from("BTC-PERP"), test_perp_instrument("BTC-PERP"));
        let rates = AtomicMap::new();
        rates.insert(None, MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)));
        let mut order = test_ws_order("OID-FILL", dec!(100), 25);
        order.aid = account.map(Ustr::from);
        let execution = test_execution("TID-FILL", price, 25, true);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);

        let event = AxWsOrderEvent::Filled(AxWsOrderFilled {
            ts: 1609459200,
            tn: 0,
            eid: "E-FILL".to_owned(),
            o: order.clone(),
            xs: execution.clone(),
        });

        let error =
            calculate_fill_commission(&order, &execution, &instruments, &rates).unwrap_err();
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        assert_eq!(error.to_string(), expected);
        assert!(rx.try_recv().is_err());
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .client_order_id,
            client_order_id
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id
        );
    }

    #[rstest]
    #[case::unaccepted_partial(true, false, false, false)]
    #[case::unaccepted_terminal(true, false, true, false)]
    #[case::partial_order(true, true, false, false)]
    #[case::partial_order_terminal(true, true, true, false)]
    #[case::untracked_partial(false, true, false, false)]
    #[case::untracked_terminal(false, true, true, false)]
    #[case::rest_partial(true, true, false, true)]
    #[case::rest_terminal(true, true, true, true)]
    fn test_commission_failure_corrected_replay_applies_once(
        #[case] tracked: bool,
        #[case] prior_fill: bool,
        #[case] complete: bool,
        #[case] rest_recovery: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let trader_id = TraderId::from("TRADER-001");
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-FEE-REPLAY");
        let venue_order_id = VenueOrderId::from("OID-FEE-REPLAY");
        let instrument = test_perp_instrument("BTC-PERP");
        let instrument_id = instrument.id();
        let harness =
            ExecutionHarness::new(trader_id, *AX_CLIENT_ID, account_id, instrument.clone());

        let prior_qty = if prior_fill { 10 } else { 0 };
        let total_qty = prior_qty + if complete { 25 } else { 50 };
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .strategy_id(StrategyId::from("S-001"))
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from(total_qty))
            .price(Price::from("100.00"))
            .build();
        initialize_commission_replay_order(
            &harness,
            &order,
            &instrument,
            venue_order_id,
            prior_fill,
        );

        let before = harness
            .cache()
            .borrow()
            .order(&client_order_id)
            .unwrap()
            .clone();
        assert_eq!(
            before.venue_order_id(),
            prior_fill.then_some(venue_order_id)
        );
        let caches = test_caches();

        if tracked {
            let mut metadata = test_metadata(client_order_id, instrument_id);
            metadata.order_init = Some(order.init_event().clone());
            metadata.is_accepted = prior_fill;
            metadata.venue_order_id = prior_fill.then_some(venue_order_id);
            caches.orders_metadata.insert(client_order_id, metadata);
            if prior_fill {
                caches
                    .venue_to_client_id
                    .insert(venue_order_id, client_order_id);
            }

            caches.cid_to_client_order_id.insert(42, client_order_id);
        }

        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from("BTC-PERP"), instrument.clone());
        let rates = AtomicMap::new();
        rates.insert(
            None,
            MakerTakerFeeRates::new(dec!(1000000000000000000000), dec!(0.0025)),
        );
        let mut wire_order = test_ws_order(venue_order_id.as_str(), dec!(100), total_qty);
        wire_order.cid = tracked.then_some(42);
        wire_order.xq = prior_qty + 25;
        wire_order.rq = total_qty - wire_order.xq;

        let execution = test_execution("T-FEE-REPLAY", dec!(80), 25, false);
        let event = commission_replay_event(wire_order.clone(), execution.clone(), complete);

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut emitter =
            ExecutionEventEmitter::new(clock, trader_id, account_id, AccountType::Margin, None);
        emitter.set_sender(tx);

        let error =
            calculate_fill_commission(&wire_order, &execution, &instruments, &rates).unwrap_err();
        dispatch_order_event(
            event.clone(),
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        assert_eq!(
            error.to_string(),
            "failed to convert AX fill commission to Money"
        );
        assert!(rx.try_recv().is_err());
        assert_eq!(caches.orders_metadata.len(), usize::from(tracked));
        assert_eq!(
            caches.venue_to_client_id.len(),
            usize::from(tracked && prior_fill)
        );
        assert_eq!(caches.cid_to_client_order_id.len(), usize::from(tracked));
        assert!(caches.pending_modifies.is_empty());

        if tracked {
            let metadata = caches.orders_metadata.get(&client_order_id).unwrap();
            assert_eq!(metadata.is_accepted, prior_fill);
            assert_eq!(
                metadata.venue_order_id,
                prior_fill.then_some(venue_order_id)
            );
            assert_eq!(
                caches
                    .venue_to_client_id
                    .get(&venue_order_id)
                    .map(|owner| *owner),
                prior_fill.then_some(client_order_id)
            );
            assert_eq!(
                *caches.cid_to_client_order_id.get(&42).unwrap(),
                client_order_id
            );
        }

        assert_eq!(
            harness
                .cache()
                .borrow()
                .order(&client_order_id)
                .unwrap()
                .events(),
            before.events()
        );
        rates.insert(None, MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)));

        let retained = usize::from(tracked && !complete);

        for replay in 0..2 {
            if rest_recovery {
                let mut response: AxFillsResponse =
                    serde_json::from_str(include_str!("../test_data/http_get_fills.json")).unwrap();
                let fill = &mut response.fills[0];
                fill.trade_id = execution.tid.clone();
                fill.order_id = Some(wire_order.oid.clone());
                fill.symbol = wire_order.s;
                fill.quantity = execution.q;
                fill.price = execution.p;
                fill.fee = dec!(0.40);
                fill.is_taker = false;
                let report = crate::http::parse::parse_fill_report(
                    fill,
                    account_id,
                    &instrument,
                    clock.get_time_ns(),
                )
                .unwrap();
                emitter.send_fill_report(report);
            } else {
                dispatch_order_event(
                    event.clone(),
                    &emitter,
                    &caches,
                    account_id,
                    &instruments,
                    &rates,
                    clock,
                );
                assert_eq!(caches.orders_metadata.len(), retained);
                assert_eq!(caches.venue_to_client_id.len(), retained);
                assert_eq!(caches.cid_to_client_order_id.len(), retained);
            }

            apply_commission_replay(
                &harness,
                &mut rx,
                client_order_id,
                venue_order_id,
                usize::from(!rest_recovery && tracked && !prior_fill && replay == 0),
            );

            assert_commission_replay_state(
                &harness,
                client_order_id,
                venue_order_id,
                prior_fill,
                complete,
            );
        }
    }

    fn initialize_commission_replay_order(
        harness: &ExecutionHarness,
        order: &OrderAny,
        instrument: &InstrumentAny,
        venue_order_id: VenueOrderId,
        prior_fill: bool,
    ) {
        let account_id = harness.account_id();
        let client_order_id = order.client_order_id();
        harness
            .cache()
            .borrow_mut()
            .add_order(order.clone(), None, Some(*AX_CLIENT_ID), false)
            .unwrap();
        let submitted = TestOrderEventStubs::submitted(order, account_id);
        harness.exec_engine().borrow_mut().process(&submitted);

        if prior_fill {
            let accepted = TestOrderEventStubs::accepted(order, account_id, venue_order_id);
            harness.exec_engine().borrow_mut().process(&accepted);
            let accepted_order = harness
                .cache()
                .borrow()
                .order_owned(&client_order_id)
                .unwrap();
            let prior = TestOrderEventStubs::filled(
                &accepted_order,
                instrument,
                Some(TradeId::from("T-PRIOR")),
                None,
                Some(Price::from("90.00")),
                Some(Quantity::from(10)),
                Some(LiquiditySide::Taker),
                Some(Money::from("0.09 USD")),
                Some(UnixNanos::from(1_609_459_199_000_000_000)),
                Some(account_id),
            );
            harness.exec_engine().borrow_mut().process(&prior);
        }
    }

    fn commission_replay_event(
        mut order: AxWsOrder,
        execution: AxWsTradeExecution,
        complete: bool,
    ) -> AxWsOrderEvent {
        order.o = if complete {
            AxOrderStatus::Filled
        } else {
            AxOrderStatus::PartiallyFilled
        };

        let mut event: AxWsOrderEvent = serde_json::from_str(if complete {
            include_str!("../test_data/ws_order_filled.json")
        } else {
            include_str!("../test_data/ws_order_partially_filled.json")
        })
        .unwrap();

        match &mut event {
            AxWsOrderEvent::Filled(message) => {
                message.o = order;
                message.xs = execution;
            }
            AxWsOrderEvent::PartiallyFilled(message) => {
                message.o = order;
                message.xs = execution;
            }
            _ => unreachable!(),
        }

        event
    }

    fn apply_commission_replay(
        harness: &ExecutionHarness,
        rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        expected_accepted: usize,
    ) {
        let mut accepted = 0;
        let mut filled = 0;

        while let Ok(message) = rx.try_recv() {
            match message {
                ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
                    accepted += 1;
                    harness
                        .exec_engine()
                        .borrow_mut()
                        .process(&OrderEventAny::Accepted(event));
                }
                ExecutionEvent::Order(OrderEventAny::Filled(event)) => {
                    assert_eq!(event.client_order_id, client_order_id);
                    assert_eq!(event.venue_order_id, venue_order_id);
                    assert_eq!(event.account_id, harness.account_id());
                    assert_eq!(event.instrument_id, harness.instrument_id());
                    assert_eq!(event.trade_id, TradeId::from("T-FEE-REPLAY"));
                    assert_eq!(event.order_side, OrderSide::Buy);
                    assert_eq!(event.last_qty.as_decimal(), dec!(25));
                    assert_eq!(event.last_px.as_decimal(), dec!(80));
                    assert_eq!(event.currency, Currency::USD());
                    assert_eq!(event.commission, Some(Money::from("0.40 USD")));
                    assert_eq!(event.liquidity_side, LiquiditySide::Maker);
                    filled += 1;
                    harness
                        .exec_engine()
                        .borrow_mut()
                        .process(&OrderEventAny::Filled(event));
                }
                ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                    assert_eq!(report.trade_id, TradeId::from("T-FEE-REPLAY"));
                    assert_eq!(report.venue_order_id, venue_order_id);
                    assert_eq!(report.account_id, harness.account_id());
                    assert_eq!(report.instrument_id, harness.instrument_id());
                    assert_eq!(report.order_side, OrderSide::Buy);
                    assert_eq!(report.last_qty.as_decimal(), dec!(25));
                    assert_eq!(report.last_px.as_decimal(), dec!(80));
                    assert_eq!(report.commission, Money::from("0.40 USD"));
                    assert_eq!(report.liquidity_side, LiquiditySide::Maker);
                    filled += 1;
                    harness
                        .exec_engine()
                        .borrow_mut()
                        .reconcile_fill_report(&report);
                }
                other => panic!("unexpected commission replay event: {other:?}"),
            }
        }

        assert_eq!(accepted, expected_accepted);
        assert_eq!(filled, 1);
    }

    fn assert_commission_replay_state(
        harness: &ExecutionHarness,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        prior_fill: bool,
        complete: bool,
    ) {
        let cache = harness.cache().borrow();
        let cached = cache.order(&client_order_id).unwrap();
        assert_eq!(cached.venue_order_id(), Some(venue_order_id));
        assert_eq!(
            cached.status(),
            if complete {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            }
        );
        assert_eq!(
            cached.filled_qty(),
            Quantity::from(if prior_fill { 35 } else { 25 })
        );
        assert_eq!(
            cached.leaves_qty(),
            Quantity::from(if complete { 0 } else { 25 })
        );
        assert_eq!(
            cached.commissions().values().copied().collect::<Vec<_>>(),
            vec![Money::from(if prior_fill {
                "0.49 USD"
            } else {
                "0.40 USD"
            })]
        );

        let expected_trades = if prior_fill {
            vec![TradeId::from("T-PRIOR"), TradeId::from("T-FEE-REPLAY")]
        } else {
            vec![TradeId::from("T-FEE-REPLAY")]
        };

        assert_eq!(
            cached.trade_ids().into_iter().copied().collect::<Vec<_>>(),
            expected_trades
        );
        assert_eq!(
            cached
                .events()
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Filled(_)))
                .count(),
            usize::from(prior_fill) + 1
        );
    }

    #[rstest]
    fn test_create_order_canceled_populates_identifiers() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-CXL");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-CXL");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        let event = create_order_canceled(&order, 1609459200, 0, &caches, account_id, clock)
            .expect("should produce OrderCanceled");

        assert_eq!(event.venue_order_id, Some(venue_order_id));
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.account_id, Some(account_id));
        assert_eq!(event.instrument_id, instrument_id);
    }

    #[rstest]
    fn test_create_order_expired_populates_identifiers() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-EXP");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-EXP");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);

        let order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        let event = create_order_expired(&order, 1609459200, 0, &caches, account_id, clock)
            .expect("should produce OrderExpired");

        assert_eq!(event.venue_order_id, Some(venue_order_id));
        assert_eq!(event.client_order_id, client_order_id);
    }

    #[rstest]
    #[case(AxTimeInForce::Ioc, true)]
    #[case(AxTimeInForce::Fok, true)]
    #[case(AxTimeInForce::Day, false)]
    #[case(AxTimeInForce::Gtc, false)]
    fn test_dispatch_expired_maps_ioc_fok_to_canceled(
        #[case] tif: AxTimeInForce,
        #[case] expect_canceled: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);

        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-EXP");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(VenueOrderId::new("OID-EXP"), client_order_id);

        let mut order = test_ws_order("OID-EXP", dec!(100.00), 10);
        order.tif = tif;
        let event = AxWsOrderEvent::Expired(AxWsOrderExpired {
            ts: 1609459200,
            tn: 0,
            eid: "E-EXP".to_string(),
            o: order,
        });

        let instruments: AtomicMap<Ustr, InstrumentAny> = AtomicMap::new();
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &AtomicMap::new(),
            clock,
        );

        match rx.try_recv().expect("an order event should be emitted") {
            ExecutionEvent::Order(OrderEventAny::Canceled(_)) => assert!(expect_canceled),
            ExecutionEvent::Order(OrderEventAny::Expired(_)) => assert!(!expect_canceled),
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[rstest]
    fn test_create_order_rejected_sets_due_post_only_when_reason_matches() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-REJ");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(VenueOrderId::new("OID-REJ"), client_order_id);

        let order = test_ws_order("OID-REJ", dec!(100.00), 10);
        // Use the literal venue reason text so this guards the AX_POST_ONLY_REJECT constant
        let reason = "post-only order would cross the book";
        let event =
            create_order_rejected(&order, reason, 1609459200, 0, &caches, account_id, clock)
                .expect("should produce OrderRejected");

        assert!(event.due_post_only, "post-only reason should set flag");
        assert_eq!(event.reason, Ustr::from(reason));
    }

    #[rstest]
    fn test_create_order_rejected_clears_due_post_only_for_other_reasons() {
        let caches = test_caches();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-REJ-2");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(VenueOrderId::new("OID-REJ-2"), client_order_id);

        let order = test_ws_order("OID-REJ-2", dec!(100.00), 10);
        let event = create_order_rejected(
            &order,
            "INSUFFICIENT_MARGIN",
            1609459200,
            0,
            &caches,
            account_id,
            clock,
        )
        .expect("should produce OrderRejected");

        assert!(!event.due_post_only);
        assert_eq!(event.reason, Ustr::from("INSUFFICIENT_MARGIN"));
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_removes_all_caches() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CLEAN");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let venue_order_id = VenueOrderId::new("OID-CLEAN");
        let stale_venue_order_id = VenueOrderId::new("OID-CLEAN-OLD");
        let cid_value = 123u64;

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches
            .venue_to_client_id
            .insert(stale_venue_order_id, client_order_id);
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        let mut order = test_ws_order(venue_order_id.as_str(), dec!(100.00), 10);
        order.cid = Some(cid_value);

        cleanup_terminal_order_tracking(&order, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_via_cid_when_venue_missing() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CLEAN-CID");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        let cid_value = 321u64;

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, instrument_id),
        );
        caches
            .cid_to_client_order_id
            .insert(cid_value, client_order_id);

        // Venue id missing from cache
        let mut order = test_ws_order("OID-UNKNOWN", dec!(100.00), 10);
        order.cid = Some(cid_value);

        cleanup_terminal_order_tracking(&order, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_noop_when_unknown() {
        let caches = test_caches();
        let other = ClientOrderId::from("OTHER");
        let instrument_id = InstrumentId::from("BTC-PERP.AX");
        caches
            .orders_metadata
            .insert(other, test_metadata(other, instrument_id));

        let order = test_ws_order("OID-NOT-TRACKED", dec!(100.00), 10);
        cleanup_terminal_order_tracking(&order, &caches);

        // Unrelated metadata still present
        assert_eq!(caches.orders_metadata.len(), 1);
    }

    #[rstest]
    fn test_cleanup_terminal_order_tracking_by_client_clears_pending_replacements() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-CANCEL-ALL");
        let parent = VenueOrderId::from("OID-CANCEL-ALL-PARENT");
        let child = VenueOrderId::from("OID-CANCEL-ALL-CHILD");
        seed_terminal_tracking(&caches, client_order_id, parent, child, 891);
        caches
            .pending_modifies
            .insert(parent, (client_order_id, 2, false));
        caches
            .pending_modifies
            .insert(child, (client_order_id, 1, true));

        cleanup_terminal_order_tracking_ids(&caches, None, None, Some(client_order_id));

        assert!(caches.pending_modifies.is_empty());
        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_closed_order_status_report_removes_all_caches() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-RECON-FILL");
        let venue_order_id = VenueOrderId::new("OID-RECON-FILL");
        let stale_venue_order_id = VenueOrderId::new("OID-RECON-FILL-OLD");
        let cid_value = client_order_id_to_cid(&client_order_id);

        seed_terminal_tracking(
            &caches,
            client_order_id,
            venue_order_id,
            stale_venue_order_id,
            cid_value,
        );

        let report = test_status_report(Some(client_order_id), venue_order_id, OrderStatus::Filled);
        cleanup_closed_order_status_report(&report, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_closed_order_status_report_via_venue_when_client_id_absent() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-RECON-CANCEL");
        let venue_order_id = VenueOrderId::new("OID-RECON-CANCEL");
        let stale_venue_order_id = VenueOrderId::new("OID-RECON-CANCEL-OLD");
        let cid_value = 654u64;

        seed_terminal_tracking(
            &caches,
            client_order_id,
            venue_order_id,
            stale_venue_order_id,
            cid_value,
        );

        let report = test_status_report(None, venue_order_id, OrderStatus::Canceled);
        cleanup_closed_order_status_report(&report, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_closed_order_status_report_prefers_venue_identity() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-EXT-RECON");
        let venue_order_id = VenueOrderId::new("OID-EXT-RECON");
        let stale_venue_order_id = VenueOrderId::new("OID-EXT-RECON-OLD");

        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, InstrumentId::from("BTC-PERP.AX")),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches
            .venue_to_client_id
            .insert(stale_venue_order_id, client_order_id);

        let report = test_status_report(
            Some(cid_to_client_order_id(99)),
            venue_order_id,
            OrderStatus::Filled,
        );
        cleanup_closed_order_status_report(&report, &caches);

        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    fn test_cleanup_open_order_status_report_retains_caches() {
        let caches = test_caches();
        let client_order_id = ClientOrderId::from("O-RECON-OPEN");
        let venue_order_id = VenueOrderId::new("OID-RECON-OPEN");
        let stale_venue_order_id = VenueOrderId::new("OID-RECON-OPEN-OLD");
        let cid_value = client_order_id_to_cid(&client_order_id);

        seed_terminal_tracking(
            &caches,
            client_order_id,
            venue_order_id,
            stale_venue_order_id,
            cid_value,
        );

        let report =
            test_status_report(Some(client_order_id), venue_order_id, OrderStatus::Accepted);
        cleanup_closed_order_status_report(&report, &caches);

        assert!(caches.orders_metadata.contains_key(&client_order_id));
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&cid_value).unwrap(),
            client_order_id
        );
    }

    #[rstest]
    fn test_cleanup_closed_order_status_reports_only_cleans_terminal() {
        let caches = test_caches();
        let filled_client_order_id = ClientOrderId::from("O-RECON-BATCH-FILL");
        let filled_venue_order_id = VenueOrderId::new("OID-RECON-BATCH-FILL");
        let filled_stale_venue_order_id = VenueOrderId::new("OID-RECON-BATCH-FILL-OLD");
        let filled_cid = client_order_id_to_cid(&filled_client_order_id);
        let open_client_order_id = ClientOrderId::from("O-RECON-BATCH-OPEN");
        let open_venue_order_id = VenueOrderId::new("OID-RECON-BATCH-OPEN");
        let open_stale_venue_order_id = VenueOrderId::new("OID-RECON-BATCH-OPEN-OLD");
        let open_cid = client_order_id_to_cid(&open_client_order_id);

        seed_terminal_tracking(
            &caches,
            filled_client_order_id,
            filled_venue_order_id,
            filled_stale_venue_order_id,
            filled_cid,
        );
        seed_terminal_tracking(
            &caches,
            open_client_order_id,
            open_venue_order_id,
            open_stale_venue_order_id,
            open_cid,
        );

        let reports = vec![
            test_status_report(
                Some(filled_client_order_id),
                filled_venue_order_id,
                OrderStatus::Filled,
            ),
            test_status_report(
                Some(open_client_order_id),
                open_venue_order_id,
                OrderStatus::Accepted,
            ),
        ];

        for report in &reports {
            cleanup_closed_order_status_report(report, &caches);
        }

        assert!(!caches.orders_metadata.contains_key(&filled_client_order_id));
        assert!(
            !caches
                .venue_to_client_id
                .contains_key(&filled_venue_order_id)
        );
        assert!(!caches.cid_to_client_order_id.contains_key(&filled_cid));
        assert!(caches.orders_metadata.contains_key(&open_client_order_id));
        assert_eq!(
            *caches.venue_to_client_id.get(&open_venue_order_id).unwrap(),
            open_client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&open_cid).unwrap(),
            open_client_order_id
        );
    }

    #[rstest]
    #[case::inline_bulk(false, false, false)]
    #[case::owned_bulk(true, false, false)]
    #[case::inline_single(false, true, false)]
    #[case::owned_single(true, true, false)]
    #[case::canceled_bulk(true, false, true)]
    #[case::canceled_single(true, true, true)]
    #[tokio::test]
    async fn test_generate_order_status_reports_cleans_terminal_tracking(
        #[case] owned: bool,
        #[case] single: bool,
        #[case] canceled: bool,
    ) {
        let filled_client_order_id = ClientOrderId::from("O-RECON-HTTP-FILL");
        let filled_venue_order_id = VenueOrderId::new("OID-RECON-HTTP-FILL");
        let filled_stale_venue_order_id = VenueOrderId::new("OID-RECON-HTTP-FILL-OLD");
        let filled_cid = client_order_id_to_cid(&filled_client_order_id);
        let open_client_order_id = ClientOrderId::from("O-RECON-HTTP-OPEN");
        let open_venue_order_id = VenueOrderId::new("OID-RECON-HTTP-OPEN");
        let open_stale_venue_order_id = VenueOrderId::new("OID-RECON-HTTP-OPEN-OLD");
        let open_cid = client_order_id_to_cid(&open_client_order_id);

        let addr = start_orders_report_server(serde_json::json!({
            "orders": [
                {
                    "ts": 1_704_067_200,
                    "tn": 500_000_000,
                    "oid": filled_venue_order_id.as_str(),
                    "u": "u",
                    "s": "BTC-PERP",
                    "p": "100.00",
                    "q": 10,
                    "xq": 10,
                    "rq": 0,
                    "o": "FILLED",
                    "d": "B",
                    "tif": "GTC",
                    "cid": filled_cid,
                    "po": false
                },
                {
                    "ts": 1_704_067_201,
                    "tn": 500_000_000,
                    "oid": open_venue_order_id.as_str(),
                    "u": "u",
                    "s": "BTC-PERP",
                    "p": "100.00",
                    "q": 10,
                    "xq": 0,
                    "rq": 10,
                    "o": "ACCEPTED",
                    "d": "B",
                    "tif": "GTC",
                    "cid": open_cid,
                    "po": false
                }
            ]
        }))
        .await;

        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *AX_CLIENT_ID,
            *AX_VENUE,
            OmsType::Netting,
            AccountId::from("AX-001"),
            AccountType::Margin,
            None,
            cache,
        );

        let config = AxExecutionClientConfig {
            api_key: Some("test_api_key".into()),
            api_secret: Some("test_api_secret".into()),
            environment: AxEnvironment::Sandbox,
            base_url_http: Some(format!("http://{addr}")),
            base_url_orders: Some(format!("http://{addr}")),
            base_url_ws_private: Some(format!("ws://{addr}/orders/ws")),
            http_timeout_secs: 5,
            max_retries: 1,
            retry_delay_initial_ms: 10,
            retry_delay_max_ms: 10,
            ..Default::default()
        };

        let client = AxExecutionClient::new(core, config).expect("create exec client");
        client.http_client.set_session_token("test-token".into());
        client
            .http_client
            .cache_instrument(test_perp_instrument("BTC-PERP"));

        seed_terminal_tracking(
            client.ws_orders.caches(),
            filled_client_order_id,
            filled_venue_order_id,
            filled_stale_venue_order_id,
            filled_cid,
        );
        seed_terminal_tracking(
            client.ws_orders.caches(),
            open_client_order_id,
            open_venue_order_id,
            open_stale_venue_order_id,
            open_cid,
        );

        let cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            UnixNanos::default(),
            false,
            None,
            None,
            None,
            None,
            None,
        );

        let single_cmd = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::from(123),
            Some(InstrumentId::from("BTC-PERP.AX")),
            Some(filled_client_order_id),
            Some(filled_venue_order_id),
            None,
            None,
        );

        let reports = if owned {
            client
                .ws_orders
                .caches()
                .cid_to_client_order_id
                .remove(&filled_cid);

            let task = if single {
                let task = client
                    .generate_order_status_report_task(&single_cmd)
                    .unwrap();

                ExecutionReportTask {
                    collection: task.collection,
                    finish: Box::new(move || Ok((task.finish)()?.into_iter().collect::<Vec<_>>())),
                }
            } else {
                client.generate_order_status_reports_task(&cmd).unwrap()
            };

            client
                .ws_orders
                .caches()
                .cid_to_client_order_id
                .insert(filled_cid, filled_client_order_id);

            let worker = tokio::spawn(task.collection);
            if canceled {
                worker.abort();
                assert!(worker.await.unwrap_err().is_cancelled());
            } else {
                worker.await.unwrap();
            }

            let caches = client.ws_orders.caches();
            assert!(caches.orders_metadata.contains_key(&filled_client_order_id));
            assert!(
                caches
                    .venue_to_client_id
                    .contains_key(&filled_venue_order_id)
            );
            assert!(caches.cid_to_client_order_id.contains_key(&filled_cid));

            if canceled {
                let error = (task.finish)().unwrap_err();
                assert_eq!(
                    error.to_string(),
                    "report collection stopped without a result"
                );
                assert_eq!(caches.orders_metadata.len(), 2);
                assert_eq!(caches.venue_to_client_id.len(), 4);
                assert_eq!(caches.cid_to_client_order_id.len(), 2);
                return;
            }

            (task.finish)().unwrap()
        } else if single {
            client
                .generate_order_status_report(&single_cmd)
                .await
                .unwrap()
                .into_iter()
                .collect()
        } else {
            client.generate_order_status_reports(&cmd).await.unwrap()
        };

        assert_eq!(reports.len(), if single { 1 } else { 2 });
        let filled = reports
            .iter()
            .find(|report| report.venue_order_id == filled_venue_order_id)
            .expect("filled report");
        assert_eq!(filled.order_status, OrderStatus::Filled);
        assert_eq!(filled.client_order_id, Some(filled_client_order_id));

        if !single {
            let open = reports
                .iter()
                .find(|report| report.venue_order_id == open_venue_order_id)
                .unwrap();
            assert_eq!(open.order_status, OrderStatus::Accepted);
            assert_eq!(open.client_order_id, Some(open_client_order_id));
        }

        let caches = client.ws_orders.caches();
        assert!(!caches.orders_metadata.contains_key(&filled_client_order_id));
        assert!(
            !caches
                .venue_to_client_id
                .contains_key(&filled_venue_order_id)
        );
        assert!(
            !caches
                .venue_to_client_id
                .contains_key(&filled_stale_venue_order_id)
        );
        assert!(!caches.cid_to_client_order_id.contains_key(&filled_cid));
        assert!(caches.orders_metadata.contains_key(&open_client_order_id));
        assert_eq!(
            *caches.venue_to_client_id.get(&open_venue_order_id).unwrap(),
            open_client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&open_cid).unwrap(),
            open_client_order_id
        );
    }

    #[tokio::test]
    #[ignore = "requires AX sandbox credentials and network access"]
    async fn test_live_report_tasks_sandbox() {
        let credential = Credential::resolve(None, None).expect("AX sandbox credentials");

        let core = ExecutionClientCore::new(
            TraderId::from("TESTER-001"),
            *AX_CLIENT_ID,
            *AX_VENUE,
            OmsType::Netting,
            AccountId::from("AX-001"),
            AccountType::Margin,
            None,
            Rc::new(RefCell::new(Cache::default())),
        );

        let config = AxExecutionClientConfig {
            api_key: Some(credential.api_key().to_owned().into()),
            api_secret: Some(credential.api_secret().to_owned().into()),
            environment: AxEnvironment::Sandbox,
            ..Default::default()
        };

        let client = AxExecutionClient::new(core, config).unwrap();
        client
            .authenticate(&credential)
            .await
            .expect("sandbox authentication");
        let now = client.clock.get_time_ns();

        let single = GenerateOrderStatusReport::new(
            UUID4::new(),
            now,
            None,
            None,
            Some(VenueOrderId::from("O-01ARZ3NDEKTSV4RRFFQ69G5FAV")),
            None,
            None,
        );
        let orders =
            GenerateOrderStatusReports::new(UUID4::new(), now, false, None, None, None, None, None);
        let fills = GenerateFillReports::new(UUID4::new(), now, None, None, None, None, None, None);
        let positions =
            GeneratePositionStatusReports::new(UUID4::new(), now, None, None, None, None, None);
        let task = client.generate_order_status_report_task(&single).unwrap();
        tokio::spawn(task.collection).await.unwrap();
        let single = (task.finish)().expect("sandbox single order reports");
        let task = client.generate_order_status_reports_task(&orders).unwrap();
        tokio::spawn(task.collection).await.unwrap();
        let orders = (task.finish)().expect("sandbox bulk order reports");
        let task = client.generate_fill_reports_task(&fills).unwrap();
        tokio::spawn(task.collection).await.unwrap();
        let fills = (task.finish)().expect("sandbox fill reports");
        let task = client
            .generate_position_status_reports_task(&positions)
            .unwrap();

        tokio::spawn(task.collection).await.unwrap();
        let positions = (task.finish)().expect("sandbox position reports");

        for account_id in single
            .iter()
            .map(|r| r.account_id)
            .chain(orders.iter().map(|r| r.account_id))
            .chain(fills.iter().map(|r| r.account_id))
            .chain(positions.iter().map(|r| r.account_id))
        {
            assert_eq!(account_id, client.account_id());
        }

        println!(
            "AX sandbox reports passed: single={}, orders={}, fills={}, positions={}",
            usize::from(single.is_some()),
            orders.len(),
            fills.len(),
            positions.len()
        );
    }

    #[rstest]
    fn test_cancel_on_disconnect_url_no_existing_query() {
        let mut url = "wss://example.com/orders/ws".to_string();
        let separator = if url.contains('?') { "&" } else { "?" };
        url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        assert_eq!(url, "wss://example.com/orders/ws?cancel_on_disconnect=true");
    }

    #[rstest]
    fn test_cancel_on_disconnect_url_with_existing_query() {
        let mut url = "wss://example.com/orders/ws?token=abc".to_string();
        let separator = if url.contains('?') { "&" } else { "?" };
        url.push_str(&format!("{separator}cancel_on_disconnect=true"));
        assert_eq!(
            url,
            "wss://example.com/orders/ws?token=abc&cancel_on_disconnect=true"
        );
    }

    fn limit_order_for_validation(quantity: Quantity) -> OrderAny {
        OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(quantity)
            .price(Price::from("1.10"))
            .build()
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_accepts_supported_limit_order() {
        let order = limit_order_for_validation(Quantity::from("10"));

        assert!(validate_order_for_ax_submit(&order).is_ok());
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_unsupported_order_type() {
        let order = OrderTestBuilder::new(OrderType::StopMarket)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .trigger_price(Price::from("1.10"))
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("Unsupported order type"));
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_gtd_time_in_force() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("1.10"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(2_000_000_000_000_000_000u64))
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("Unsupported time in force"));
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_stop_limit_order() {
        let order = OrderTestBuilder::new(OrderType::StopLimit)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("1.11"))
            .trigger_price(Price::from("1.10"))
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("Unsupported order type"));
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_fok_time_in_force() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("1.10"))
            .time_in_force(TimeInForce::Fok)
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("Unsupported time in force"));
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_fractional_quantity() {
        let order = limit_order_for_validation(Quantity::from("10.5"));

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("whole contract"));
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_quote_quantity() {
        let order = OrderTestBuilder::new(OrderType::Market)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .time_in_force(TimeInForce::Ioc)
            .quote_quantity(true)
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert_eq!(err.to_string(), "UNSUPPORTED_QUOTE_QUANTITY");
    }

    #[rstest]
    fn test_validate_order_for_ax_submit_denies_display_quantity() {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(InstrumentId::from("EURUSD-PERP.AX"))
            .side(OrderSide::Buy)
            .quantity(Quantity::from("10"))
            .price(Price::from("1.10"))
            .display_qty(Quantity::from("5"))
            .build();

        let err = validate_order_for_ax_submit(&order).unwrap_err();

        assert!(err.to_string().contains("display_qty"));
    }

    #[rstest]
    #[case(AxHttpError::MissingCredentials, true)]
    #[case(AxHttpError::MissingSessionToken, true)]
    #[case(AxHttpError::ValidationError("bad param".to_string()), true)]
    #[case(AxHttpError::BuildError(AxBuildError::MissingOrderId), true)]
    #[case(AxHttpError::ApiError { message: "invalid modification".to_string() }, false)]
    #[case(AxHttpError::JsonError("parse failure".to_string()), false)]
    #[case(AxHttpError::Canceled("shutdown".to_string()), false)]
    #[case(AxHttpError::NetworkError("timeout".to_string()), false)]
    #[case(AxHttpError::UnexpectedStatus { status: 400, body: "invalid".to_string() }, false)]
    #[case(AxHttpError::UnexpectedStatus { status: 503, body: String::new() }, false)]
    fn test_classify_ax_http_failure(#[case] error: AxHttpError, #[case] expect_not_sent: bool) {
        // Asserting the exact variant also pins that AX never classifies a venue rejection
        let expected = if expect_not_sent {
            CommandFailure::NotSent(error.to_string())
        } else {
            CommandFailure::Ambiguous(error.to_string())
        };

        assert_eq!(classify_ax_http_failure(&error), expected);
    }

    #[rstest]
    #[case(
        AxOrdersWsClientError::ClientError("missing venue_order_id".to_string()),
        Some("missing venue_order_id")
    )]
    #[case(AxOrdersWsClientError::ChannelError("handler closed".to_string()), None)]
    #[case(AxOrdersWsClientError::Transport("connection reset".to_string()), None)]
    #[case(AxOrdersWsClientError::AuthenticationError("token expired".to_string()), None)]
    fn test_classify_ax_ws_failure(
        #[case] error: AxOrdersWsClientError,
        #[case] not_sent_reason: Option<&str>,
    ) {
        // Asserting the exact variant also pins that AX never classifies a venue rejection
        let expected = match not_sent_reason {
            Some(reason) => CommandFailure::NotSent(reason.to_string()),
            None => CommandFailure::Ambiguous(error.to_string()),
        };

        assert_eq!(classify_ax_ws_failure(&error), expected);
    }

    #[rstest]
    #[case(include_str!("../test_data/ws_order_filled_unknown_state.json"))]
    #[case(include_str!("../test_data/ws_order_filled_unknown_tif.json"))]
    fn test_dispatch_fill_with_unknown_sibling_classification(#[case] raw: &str) {
        let event: AxWsOrderEvent = serde_json::from_str(raw).unwrap();
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let client_order_id = ClientOrderId::from("O-UNKNOWN-STATE");
        let venue_order_id = VenueOrderId::new("O-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        let caches = test_caches();
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX")),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let account_fee_rates = AtomicMap::new();
        account_fee_rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );

        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &account_fee_rates,
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected fill")
        };

        assert_eq!(fill.venue_order_id, venue_order_id);
        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.trade_id.as_str(), "T-01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(fill.last_qty.as_decimal(), dec!(100));
        assert_eq!(fill.last_px.as_decimal(), dec!(50000));
        assert_eq!(fill.ts_event, UnixNanos::from(1609459200123456789));
        assert!(rx.try_recv().is_err());
        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
    }

    #[rstest]
    fn test_amendment_keeps_venue_identity_and_updates_quantity() {
        let message: crate::websocket::messages::AxWsOrderReplaced =
            serde_json::from_str(include_str!("../test_data/ws_order_amended.json")).unwrap();
        let venue_order_id = VenueOrderId::new(&message.ro.oid);
        let client_order_id = ClientOrderId::from("O-AMEND");
        let caches = test_caches();
        caches.orders_metadata.insert(
            client_order_id,
            test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX")),
        );
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        let instruments = AtomicMap::new();

        dispatch_order_event(
            AxWsOrderEvent::Replaced(message),
            &emitter,
            &caches,
            account_id,
            &instruments,
            &AtomicMap::new(),
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Updated(update)) = rx.try_recv().unwrap() else {
            panic!("expected update")
        };

        assert_eq!(update.venue_order_id, Some(venue_order_id));
        assert_eq!(update.client_order_id, client_order_id);
        assert_eq!(update.quantity.as_decimal(), dec!(150));
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id
        );
        assert_eq!(caches.venue_to_client_id.len(), 1);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case(true, false)]
    #[case(false, true)]
    #[case(true, true)]
    fn test_replacement_rejects_inconsistent_identity(
        #[case] has_id: bool,
        #[case] has_order: bool,
    ) {
        let mut message: crate::websocket::messages::AxWsOrderReplaced =
            serde_json::from_str(include_str!("../test_data/ws_order_replaced_live.json")).unwrap();
        message.noid = has_id.then(|| "MISMATCHED".to_owned());

        if !has_order {
            message.no = None;
        }

        assert_eq!(
            replacement_venue_order_id(&message)
                .unwrap_err()
                .to_string(),
            "Inconsistent AX replacement order identity"
        );
    }

    #[rstest]
    #[case::known(
        AxCancelRejectionReason::OrderNotFound,
        Some("ignored"),
        "ORDER_NOT_FOUND",
        false
    )]
    #[case::unknown(
        AxCancelRejectionReason::Unknown,
        Some("cancel denied by venue"),
        "cancel denied by venue",
        false
    )]
    #[case::empty(AxCancelRejectionReason::Unknown, Some("  "), "UNKNOWN", false)]
    #[case::post_only(
        AxCancelRejectionReason::Unknown,
        Some("post-only order would cross the book"),
        "post-only order would cross the book",
        true
    )]
    fn test_pending_modify_does_not_misclassify_cancel_rejection(
        #[case] reason: AxCancelRejectionReason,
        #[case] text: Option<&str>,
        #[case] expected_reason: &str,
        #[case] modify: bool,
    ) {
        let mut event: AxWsOrderEvent =
            serde_json::from_str(include_str!("../test_data/ws_cancel_rejected.json")).unwrap();

        let AxWsOrderEvent::CancelRejected(msg) = &mut event else {
            unreachable!()
        };

        msg.r = reason;
        msg.txt = text.map(str::to_owned);
        let venue_order_id = VenueOrderId::from(msg.oid.as_str());
        let client_order_id = ClientOrderId::from("O-PENDING-MODIFY");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(venue_order_id);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches
            .pending_modifies
            .insert(venue_order_id, (client_order_id, 1, false));
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &AtomicMap::new(),
            &AtomicMap::new(),
            clock,
        );

        let ExecutionEvent::Order(event) = rx.try_recv().unwrap() else {
            panic!("expected order event")
        };

        let actual_reason = match &event {
            OrderEventAny::ModifyRejected(rejected) if modify => rejected.reason,
            OrderEventAny::CancelRejected(rejected) if !modify => rejected.reason,
            _ => panic!("unexpected event {event:?}"),
        };

        assert_eq!(actual_reason.as_str(), expected_reason);
        assert_eq!(event.client_order_id(), client_order_id);
        assert_eq!(event.venue_order_id(), Some(venue_order_id));
        assert_eq!(
            caches.pending_modifies.contains_key(&venue_order_id),
            !modify
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_pending_modify_completes_deferred_terminal_cleanup() {
        let caches = test_caches();
        let venue_order_id = VenueOrderId::from("OID-DEFERRED");
        let client_order_id = ClientOrderId::from("O-DEFERRED");
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(venue_order_id);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches
            .venue_to_client_id
            .insert(venue_order_id, client_order_id);
        caches.cid_to_client_order_id.insert(765, client_order_id);
        caches
            .pending_modifies
            .insert(venue_order_id, (client_order_id, 2, false));
        cleanup_terminal_order_tracking_ids(&caches, Some(&venue_order_id), Some(765), None);
        complete_pending_modify(&caches, venue_order_id);
        assert_eq!(
            *caches.pending_modifies.get(&venue_order_id).unwrap(),
            (client_order_id, 1, true)
        );
        assert_eq!(caches.orders_metadata.len(), 1);
        complete_pending_modify(&caches, venue_order_id);
        assert!(caches.pending_modifies.is_empty());
        assert!(caches.orders_metadata.is_empty());
        assert!(caches.venue_to_client_id.is_empty());
        assert!(caches.cid_to_client_order_id.is_empty());
    }

    #[rstest]
    #[case::without_stream_terminal(false, false)]
    #[case::after_stream_terminal(true, false)]
    #[case::resolved_terminal(false, true)]
    #[case::resolved_after_stream_terminal(true, true)]
    fn test_closed_parent_report_retains_pending_replacement(
        #[case] terminal: bool,
        #[case] resolved: bool,
    ) {
        let caches = test_caches();
        let venue_order_id = VenueOrderId::from("OID-AMBIGUOUS");
        let client_order_id = ClientOrderId::from("O-AMBIGUOUS");
        let old_venue_order_id = VenueOrderId::from("OID-AMBIGUOUS-OLD");
        seed_terminal_tracking(
            &caches,
            client_order_id,
            venue_order_id,
            old_venue_order_id,
            987,
        );
        caches
            .orders_metadata
            .get_mut(&client_order_id)
            .unwrap()
            .venue_order_id = Some(venue_order_id);
        caches
            .pending_modifies
            .insert(venue_order_id, (client_order_id, 1, terminal));
        let report =
            test_status_report(Some(client_order_id), venue_order_id, OrderStatus::Canceled);

        if resolved {
            cleanup_resolved_order_status_report(&report, &caches);
        } else {
            cleanup_closed_order_status_report(&report, &caches);
        }

        let terminal = terminal || resolved;
        assert_eq!(
            *caches.pending_modifies.get(&venue_order_id).unwrap(),
            (client_order_id, 1, terminal)
        );
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(venue_order_id)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&venue_order_id).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.cid_to_client_order_id.get(&987).unwrap(),
            client_order_id
        );
        cleanup_deferred_order_tracking(&caches);
        assert_eq!(
            caches.orders_metadata.contains_key(&client_order_id),
            !terminal
        );
        assert_eq!(
            caches.pending_modifies.contains_key(&venue_order_id),
            !terminal
        );
        assert_eq!(
            caches.venue_to_client_id.contains_key(&venue_order_id),
            !terminal
        );
        assert_eq!(caches.cid_to_client_order_id.contains_key(&987), !terminal);
    }

    #[rstest]
    #[case::terminal(true, false)]
    #[case::working(false, false)]
    #[case::replaced(true, true)]
    fn test_cleanup_deferred_order_tracking_preserves_working_legs(
        #[case] terminal: bool,
        #[case] replaced: bool,
    ) {
        let caches = test_caches();
        let parent = VenueOrderId::from("OID-TEARDOWN-PARENT");
        let child = VenueOrderId::from("OID-TEARDOWN-CHILD");
        let client_order_id = ClientOrderId::from("O-TEARDOWN");

        let active = if replaced { child } else { parent };
        let mut metadata = test_metadata(client_order_id, InstrumentId::from("EURUSD-PERP.AX"));
        metadata.venue_order_id = Some(active);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.venue_to_client_id.insert(active, client_order_id);
        caches.cid_to_client_order_id.insert(678, client_order_id);
        caches
            .pending_modifies
            .insert(parent, (client_order_id, 1, terminal));
        if !terminal {
            record_replacement_proposal(&caches, client_order_id, parent, child);
        }

        cleanup_deferred_order_tracking(&caches);
        let cleared = terminal && !replaced;
        assert_eq!(caches.pending_modifies.contains_key(&parent), !cleared);

        if !cleared {
            assert_eq!(
                *caches.pending_modifies.get(&parent).unwrap(),
                (client_order_id, 1, terminal)
            );
        }

        if cleared {
            assert!(caches.orders_metadata.is_empty());
            assert!(caches.venue_to_client_id.is_empty());
            assert!(caches.cid_to_client_order_id.is_empty());
        } else {
            assert_eq!(
                caches
                    .orders_metadata
                    .get(&client_order_id)
                    .unwrap()
                    .venue_order_id,
                Some(active)
            );
            assert_eq!(
                *caches.venue_to_client_id.get(&active).unwrap(),
                client_order_id
            );
            assert_eq!(
                *caches.cid_to_client_order_id.get(&678).unwrap(),
                client_order_id
            );
            assert_eq!(caches.orders_metadata.len(), 1);
            assert_eq!(
                caches.venue_to_client_id.len(),
                if terminal { 1 } else { 2 }
            );

            if !terminal {
                assert_eq!(
                    *caches.venue_to_client_id.get(&child).unwrap(),
                    client_order_id
                );
            }

            assert_eq!(caches.cid_to_client_order_id.len(), 1);
        }
    }

    #[rstest]
    #[case::child(true, false)]
    #[case::child_after_reconnect(true, true)]
    #[case::parent(false, false)]
    #[case::parent_after_reconnect(false, true)]
    fn test_fill_confirms_child_without_regressing_parent(
        #[case] child_fill: bool,
        #[case] reconnect: bool,
    ) {
        let mut event: AxWsOrderEvent =
            serde_json::from_str(include_str!("../test_data/ws_order_partially_filled.json"))
                .unwrap();

        let AxWsOrderEvent::PartiallyFilled(msg) = &mut event else {
            unreachable!()
        };

        let parent = VenueOrderId::from(msg.o.oid.as_str());
        let child = VenueOrderId::from("OID-FILLED-CHILD");
        let client_order_id = ClientOrderId::from("O-FILL-CONFIRMATION");
        let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
        let caches = test_caches();
        let mut metadata = test_metadata(client_order_id, instrument_id);
        metadata.venue_order_id = Some(parent);
        caches.orders_metadata.insert(client_order_id, metadata);
        caches.venue_to_client_id.insert(parent, client_order_id);
        caches
            .pending_modifies
            .insert(parent, (client_order_id, 1, false));
        record_replacement_proposal(&caches, client_order_id, parent, child);
        if child_fill {
            msg.o.oid = child.to_string();
        } else {
            assert!(record_replacement_venue_id(
                &caches,
                client_order_id,
                parent,
                child,
                false
            ));
            caches
                .pending_modifies
                .insert(child, (client_order_id, 1, false));
        }

        if reconnect {
            cleanup_deferred_order_tracking(&caches);
        }

        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from("EURUSD-PERP"),
            test_perp_instrument("EURUSD-PERP"),
        );
        let rates = AtomicMap::new();
        rates.insert(
            Some(Ustr::from("fixture-account")),
            MakerTakerFeeRates::new(dec!(0.0002), dec!(0.0025)),
        );
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("AX-001");
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            account_id,
            AccountType::Margin,
            None,
        );
        emitter.set_sender(tx);
        dispatch_order_event(
            event,
            &emitter,
            &caches,
            account_id,
            &instruments,
            &rates,
            clock,
        );

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected correlated fill")
        };

        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.account_id, account_id);
        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.venue_order_id, if child_fill { child } else { parent });
        assert_eq!(fill.last_qty, Quantity::from("50"));
        assert_eq!(fill.last_px, Price::from("50000"));
        assert_eq!(
            caches
                .orders_metadata
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(child)
        );
        assert_eq!(
            *caches.venue_to_client_id.get(&child).unwrap(),
            client_order_id
        );
        assert_eq!(
            *caches.pending_modifies.get(&parent).unwrap(),
            (client_order_id, 1, false)
        );
        assert!(rx.try_recv().is_err());
    }
}
