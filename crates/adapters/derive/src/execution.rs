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

//! Live execution client implementation for the Derive adapter.
//!
//! An [`ExecutionClientCore`] holds identity and connection state, an
//! [`ExecutionEventEmitter`] publishes order/account events back to the live
//! engine, and the venue clients ([`DeriveHttpClient`], [`DeriveWebSocketClient`])
//! handle the wire. All state-changing requests are EIP-712 typed-data signed
//! against the per-action module contracts on the Derive Chain; the
//! `private/order` body in particular is built by [`order_to_derive_payload`].

use std::{
    cell::RefCell,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use ahash::{AHashMap, AHashSet};
use alloy_primitives::U256;
use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::ORDER_NOT_FOUND,
    clients::ExecutionClient,
    enums::LogLevel,
    live::{
        dst::time::{self, Instant},
        runner::{get_exec_event_sender, try_get_data_event_sender, try_get_exec_event_sender},
    },
    messages::{
        DataEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReport, GenerateOrderStatusReports, GeneratePositionStatusReports,
            ModifyOrder, QueryAccount, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
    runner::{TimeEventMessage, TimeEventSender, try_get_time_event_sender},
    timer::{TimeEvent, TimeEventCallback},
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
    data::QuoteTick,
    enums::{OmsType, OrderSide, OrderStatus, OrderType, PositionSide},
    events::{
        OrderAccepted, OrderCanceled, OrderDeniedReason, OrderEventAny, OrderExpired, OrderFilled,
        OrderRejected, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Price, Quantity},
};
use parking_lot::Mutex;
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            DECIMAL_SCALE, DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS, DERIVE_VENUE,
            MIN_SIGNATURE_TTL, TRIGGER_ORDER_SIGNATURE_TTL, WS_REQUEST_TIMEOUT,
        },
        credential::DeriveCredential,
        enums::{DeriveInstrumentType, DeriveOrderSide, DeriveOrderStatus},
        parse::{
            STRATEGY_REASON_MAX_CHARS, derive_order_type_to_nautilus_for_order,
            derive_rejection_due_post_only, derive_status_to_nautilus, format_instrument_id,
            format_venue_symbol, parse_derive_instrument_any, strategy_rejection_reason,
        },
        retry::{http_retry_config, is_write_outcome_ambiguous_ws},
    },
    config::DeriveExecutionClientConfig,
    http::{
        DeriveCredentials, DeriveHttpClient, DeriveHttpError,
        models::{
            DeriveInstrument, DeriveOrder, DeriveOrderResult, DeriveReplaceOutcome,
            DeriveSubaccount, DeriveTrade,
        },
        parse::{
            CommissionError, parse_derive_order_to_report_with_precision,
            parse_derive_position_to_report_with_precision, parse_derive_subaccount_to_balances,
            parse_derive_trade_to_fill_report_with_precision,
        },
        query::{
            DeriveCancelByInstrumentParams, DeriveCancelByLabelParams, DeriveCancelParams,
            DeriveCancelTriggerOrderParams, DeriveGetOpenOrdersParams, DeriveGetOrderHistoryParams,
            DeriveGetOrderParams, DeriveGetPositionsParams, DeriveGetSubaccountParams,
            DeriveGetTradeHistoryParams, DeriveGetTriggerOrdersParams, PaginationCursor,
            order_replace_to_derive_payload, order_to_derive_payload,
            trigger_order_to_derive_payload, validate_order_support,
            validate_trigger_order_support,
        },
    },
    signing::{
        context::{SigningContext, resolve_signing_context},
        encoding::decimal_to_scaled_u256,
        nonce::{NONCE_FUTURE_NS, NonceError, NonceManager},
    },
    websocket::{
        DeriveOrdersSubscriptionData, DeriveTradesSubscriptionData, DeriveWebSocketClient,
        DeriveWsChannel, DeriveWsCredentials, DeriveWsError, DeriveWsExecutionHandle,
        DeriveWsMessage, OrderIdentity, WsDispatchState,
        dispatch::{OrderBinding, ReplaceCompletion},
        parse::parse_ticker_quote_from_rest,
    },
};

const DERIVE_PRIVATE_PAGE_SIZE: u32 = 500;

/// Live execution client for Derive.
///
/// Owns the HTTP and WebSocket clients used to talk to the venue plus an
/// [`ExecutionEventEmitter`] that publishes order/account events back to the
/// live engine. Order operations are signed against the per-environment
/// EIP-712 signing context resolved at construction.
#[derive(Debug)]
pub struct DeriveExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: DeriveExecutionClientConfig,
    credential: DeriveCredential,
    emitter: ExecutionEventEmitter,
    http_client: DeriveHttpClient,
    ws_client: DeriveWebSocketClient,
    ws_exec: DeriveWsExecutionHandle,
    instruments: Arc<AtomicMap<InstrumentId, DeriveInstrument>>,
    nonce_manager: Arc<NonceManager>,
    signing: SigningContext,
    is_connected: Arc<AtomicBool>,
    cancellation_token: CancellationToken,
    session_tasks: TaskGroup,
    pending_tasks: TaskGroup,
    shutdown_errors: Vec<String>,
    dispatch_state: Arc<WsDispatchState>,
    reconciliation_request: Mutex<Option<ReconciliationSnapshotRequest>>,
}

impl DeriveExecutionClient {
    /// Creates a new [`DeriveExecutionClient`].
    ///
    /// Resolves wallet/session-key/subaccount from the supplied config, falling
    /// back to the documented environment variables when fields are unset, and
    /// parses the EIP-712 signing constants (domain separator, action typehash,
    /// trade-module address) from config overrides or the shipped per-environment
    /// defaults.
    ///
    /// # Errors
    ///
    /// Returns an error when:
    /// - `max_fee_per_contract` is missing or not greater than zero.
    /// - Required credentials are not provided via config or environment.
    /// - Signing constants are still placeholders or cannot be parsed as hex.
    /// - The HTTP or WebSocket client cannot be constructed.
    pub fn new(
        core: ExecutionClientCore,
        config: DeriveExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        config.validate()?;

        let credential = DeriveCredential::resolve(
            config.wallet_address.clone(),
            config.session_key.clone().map(SecretString::into_inner),
            config.subaccount_id,
            config.environment,
        )?;

        let http_credentials = DeriveCredentials::new(
            credential.wallet_address().to_string(),
            credential.session_key(),
        )
        .context("failed to build Derive HTTP credentials")?;
        let retry_config = http_retry_config(
            config.max_retries,
            config.retry_delay_initial_ms,
            config.retry_delay_max_ms,
        );
        let proxy_url = config
            .proxy_url
            .as_ref()
            .map(|value| value.expose_secret().to_owned());
        let http_client = DeriveHttpClient::with_credentials(
            config.rest_url(),
            http_credentials,
            Some(config.http_timeout_secs),
            proxy_url.clone(),
            Some(retry_config),
        )
        .context("failed to create Derive HTTP client")?;

        let ws_credentials = DeriveWsCredentials::new(
            credential.wallet_address().to_string(),
            credential.session_key(),
        )
        .context("failed to build Derive WebSocket credentials")?;
        let mut ws_client = DeriveWebSocketClient::with_credentials(
            Some(config.ws_url()),
            config.environment,
            config.transport_backend,
            proxy_url,
            ws_credentials,
            config.max_matching_requests_per_second,
            config.max_per_instrument_matching_requests_per_second,
        )
        .with_socket_control(SocketControl::new(
            core.client_id,
            Some(*DERIVE_VENUE),
            "derive-user-streams",
        ));

        if let Some(secs) = config.ws_timeout_secs {
            ws_client.set_request_timeout(Duration::from_secs(secs));
        }

        // The handle shares the client's command channel, which survives the
        // reconnect swap, so it stays valid for the client's lifetime.
        let ws_exec = ws_client.execution_handle();

        let signing = resolve_signing_context(&credential, &config)?;

        let clock = get_atomic_clock_realtime();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        if let Some(sender) = try_get_exec_event_sender() {
            emitter.set_sender(sender);
        }

        let session_tasks = TaskGroup::new();
        let pending_tasks = TaskGroup::new();

        let dispatch_state = Arc::new(WsDispatchState::new(credential.subaccount_id()));

        Ok(Self {
            core,
            clock,
            config,
            credential,
            emitter,
            http_client,
            ws_client,
            ws_exec,
            instruments: Arc::new(AtomicMap::new()),
            nonce_manager: Arc::new(NonceManager::new()),
            signing,
            is_connected: Arc::new(AtomicBool::new(false)),
            cancellation_token: CancellationToken::new(),
            session_tasks,
            pending_tasks,
            shutdown_errors: Vec::new(),
            dispatch_state,
            reconciliation_request: Mutex::new(None),
        })
    }

    /// Returns the resolved subaccount id.
    #[must_use]
    pub const fn subaccount_id(&self) -> u64 {
        self.credential.subaccount_id()
    }

    /// Returns a reference to the resolved configuration.
    #[must_use]
    pub fn config(&self) -> &DeriveExecutionClientConfig {
        &self.config
    }

    /// Returns a reference to the underlying HTTP client.
    #[must_use]
    pub fn http_client(&self) -> &DeriveHttpClient {
        &self.http_client
    }

    /// Caches a Derive instrument by instrument ID so order submission can
    /// resolve `base_asset_address` and `base_asset_sub_id` without
    /// re-querying the venue.
    ///
    /// # Errors
    ///
    /// Returns an error when instrument identity or price and size increments are invalid.
    pub fn cache_instrument(&self, instrument: DeriveInstrument) -> anyhow::Result<()> {
        let instrument_id = format_instrument_id(instrument.instrument_name)?;
        anyhow::ensure!(
            instrument.tick_size > Decimal::ZERO,
            "Derive tick_size must be positive"
        );
        anyhow::ensure!(
            instrument.amount_step > Decimal::ZERO,
            "Derive amount_step must be positive"
        );
        let price_increment =
            Price::from_decimal(instrument.tick_size).context("invalid Derive tick_size")?;
        let size_increment =
            Quantity::from_decimal(instrument.amount_step).context("invalid Derive amount_step")?;
        self.dispatch_state.register_instrument_precision(
            instrument_id,
            price_increment.precision,
            size_increment.precision,
        );
        self.instruments.insert(instrument_id, instrument);
        Ok(())
    }

    /// Spawns a fire-and-forget task tracked in `pending_tasks` for teardown.
    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let future = async move {
            if let Err(e) = fut.await {
                log::warn!("{description} failed: {e:?}");
            }
        };

        if let Err(e) = self.pending_tasks.spawn(future) {
            log::warn!("Skipping Derive {description} after shutdown began: {e}");
        }
    }

    fn abort_pending_tasks(&self) {
        self.pending_tasks.begin_shutdown();
    }

    fn abort_session_tasks(&self) {
        let request = self.reconciliation_request.lock().take();
        if let Some(request) = request {
            request.close();
        }

        self.session_tasks.begin_shutdown();
        self.ws_client.begin_shutdown();
    }

    async fn await_pending_tasks(&self) -> anyhow::Result<()> {
        self.pending_tasks.begin_shutdown();
        self.pending_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| anyhow::anyhow!("Failed to terminate Derive execution tasks: {e}"))?;
        Ok(())
    }

    async fn await_session_tasks(&self) -> anyhow::Result<()> {
        self.session_tasks.begin_shutdown();
        self.session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(2))
            .await
            .map_err(|e| {
                anyhow::anyhow!("Failed to terminate Derive execution session tasks: {e}")
            })?;

        Ok(())
    }

    async fn ensure_instruments_initialized(&self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }

        let instruments: Vec<_> = self
            .core
            .cache()
            .instruments(&DERIVE_VENUE, None)
            .into_iter()
            .cloned()
            .collect();

        for instrument in instruments {
            self.cache_native_instrument(&instrument)?;
        }

        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn initialize_portfolio(&self) -> anyhow::Result<()> {
        let snapshot = self
            .http_client
            .get_subaccount(&DeriveGetSubaccountParams::new(self.subaccount_id()))
            .await?;
        self.reconciliation_context()
            .emit_account_snapshot(&snapshot)?;
        let triggers = self
            .http_client
            .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(self.subaccount_id()))
            .await?;
        let mut names: AHashSet<Ustr> = snapshot
            .open_orders
            .iter()
            .chain(triggers.orders.iter())
            .map(|order| order.instrument_name)
            .chain(
                snapshot
                    .positions
                    .iter()
                    .map(|position| position.instrument_name),
            )
            .collect();
        {
            let cache = self.core.cache();
            for order in cache.orders_refs(
                Some(&DERIVE_VENUE),
                None,
                None,
                Some(&self.core.account_id),
                None,
            ) {
                if (order.is_open() || order.is_inflight())
                    && cache.client_id(&order.client_order_id()) == Some(&self.core.client_id)
                {
                    names.insert(format_venue_symbol(&order.instrument_id())?);
                }
            }
        }

        let mut names: Vec<_> = names.into_iter().collect();
        names.sort_unstable();
        let mut required = Vec::with_capacity(names.len());
        for name in names {
            let id = format_instrument_id(name)?;
            required.push(id);
            if self.core.cache().instrument(&id).is_some() {
                continue;
            }

            let raw = cached_or_fetch_instrument(
                &self.http_client,
                &self.instruments,
                &id,
                name.as_str(),
            )
            .await?;
            let instrument = parse_derive_instrument_any(&raw, self.clock.get_time_ns())?
                .context("unsupported required Derive portfolio instrument")?;
            self.cache_instrument(raw)?;
            let sender = try_get_data_event_sender()
                .context("Derive instrument event sender is unavailable")?;
            sender
                .send(DataEvent::Instrument(instrument))
                .map_err(|e| {
                    anyhow::anyhow!("Failed to publish required Derive instrument {id}: {e}")
                })?;
        }

        self.await_instruments_registered(&required).await?;
        self.await_account_registered(DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS)
            .await?;
        Ok(())
    }

    async fn await_instruments_registered(&self, required: &[InstrumentId]) -> anyhow::Result<()> {
        let started = Instant::now();
        let timeout = Duration::from_secs_f64(DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS);

        loop {
            if required
                .iter()
                .all(|id| self.core.cache().instrument(id).is_some())
            {
                return Ok(());
            }

            anyhow::ensure!(
                started.elapsed() < timeout,
                "Timeout waiting for required Derive instruments to be registered"
            );
            time::sleep(Duration::from_millis(10)).await;
        }
    }

    fn cache_native_instrument(&self, instrument: &InstrumentAny) -> anyhow::Result<()> {
        let info = instrument
            .info()
            .context("Derive instrument has no venue metadata")?;
        let definition: DeriveInstrument = serde_json::from_value(serde_json::to_value(info)?)
            .context("invalid Derive instrument metadata")?;
        anyhow::ensure!(
            format_instrument_id(definition.instrument_name)? == instrument.id(),
            "Derive metadata identity does not match {}",
            instrument.id(),
        );
        self.cache_instrument(definition)?;
        Ok(())
    }

    fn reconciliation_context(&self) -> DeriveReconciliationContext {
        let (orders, terminal_revision) = cached_order_bindings(&self.core, &self.dispatch_state);

        DeriveReconciliationContext {
            http_client: self.http_client.clone(),
            emitter: self.emitter.clone(),
            client_id: self.core.client_id,
            account_id: self.core.account_id,
            subaccount_id: self.credential.subaccount_id(),
            clock: self.clock,
            dispatch_state: Arc::clone(&self.dispatch_state),
            instruments: Arc::clone(&self.instruments),
            orders,
            terminal_revision,
        }
    }

    fn validate_order(&self, order: &OrderAny) -> Result<(), OrderDeniedReason> {
        if is_derive_trigger_order_type(order.order_type()) {
            validate_trigger_order_support(order)?;
        } else {
            validate_order_support(order)?;
        }

        let cache = self.core.cache();

        if order.is_reduce_only()
            && matches!(
                cache.instrument(&order.instrument_id()),
                Some(InstrumentAny::CurrencyPair(_))
            )
        {
            return Err(OrderDeniedReason::UnsupportedReduceOnly);
        }

        if order.order_type() == OrderType::Market && cache.quote(&order.instrument_id()).is_none()
        {
            return Err(OrderDeniedReason::MarketPriceUnavailable {
                order_type: order.order_type(),
                instrument_id: order.instrument_id(),
            });
        }

        Ok(())
    }

    fn restore_active_orders(&self) {
        let _delivery = self.dispatch_state.delivery_guard();
        let cache = self.core.cache();
        for order in cache.orders_refs(
            Some(&self.core.venue),
            None,
            None,
            Some(&self.core.account_id),
            None,
        ) {
            if (order.is_open() || order.is_inflight())
                && cache.client_id(&order.client_order_id()) == Some(&self.core.client_id)
            {
                self.dispatch_state.restore_order(&order);
            }
        }
    }

    /// Blocks until the account appears in the cache, or `timeout_secs` elapses.
    ///
    /// The execution engine populates the cache from the [`refresh_account_state`]
    /// event asynchronously; strategies that begin issuing orders before the
    /// account is registered race the portfolio. Connecting blocks here so the
    /// runner can rely on `core.cache().account(account_id)` immediately after
    /// `connect()` returns.
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
            time::sleep(interval).await;

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

    /// Reverses the partial state `connect()` set up before the failing step:
    /// cancels the shared cancellation token, aborts the WS dispatch task,
    /// and closes the WS client. Used when initial account state cannot be
    /// loaded so that the next `connect()` call starts from a clean slate.
    async fn teardown_partial_connect(&mut self) -> anyhow::Result<()> {
        self.cancellation_token.cancel();
        self.abort_session_tasks();
        self.abort_pending_tasks();

        if let Err(e) = self.ws_client.disconnect().await {
            self.shutdown_errors
                .push(format!("Derive WebSocket shutdown failed: {e}"));
        }

        let (session_result, pending_result) =
            tokio::join!(self.await_session_tasks(), self.await_pending_tasks());
        self.core.set_disconnected();
        self.is_connected.store(false, Ordering::Release);

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

    fn start_ws_dispatch(
        &self,
        rx: tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>,
    ) -> anyhow::Result<()> {
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;
        let clock = self.clock;
        let cancellation = self.cancellation_token.clone();
        let dispatch_state = self.dispatch_state.clone();
        let reconciliation = self.reconciliation_context();
        let timeout = self
            .config
            .ws_timeout_secs
            .map_or(WS_REQUEST_TIMEOUT, Duration::from_secs);

        let request = ReconciliationSnapshotRequest::new(
            self.core.clone(),
            reconciliation.clone(),
            cancellation.clone(),
            timeout,
        );
        *self.reconciliation_request.lock() = request.clone();
        let is_connected = Arc::clone(&self.is_connected);
        let session_spawner = self
            .session_tasks
            .spawner()
            .map_err(|e| anyhow::anyhow!("Derive session task admission is closed: {e}"))?;

        self.session_tasks.spawn(async move {
            let mut rx = rx;

            loop {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => break,
                    maybe = rx.recv() => {
                        match maybe {
                            Some(DeriveWsMessage::Reconnected) => {
                                let context = reconciliation.clone();
                                let request = request.clone();
                                let task_cancellation = cancellation.clone();

                                if let Err(e) = session_spawner.spawn(async move {
                                    tokio::select! {
                                        biased;
                                        () = task_cancellation.cancelled() => {}
                                        result = async {
                                            let (context, ts_init) = match request {
                                                Some(request) => request.snapshot().await?,
                                                None => (context, clock.get_time_ns()),
                                            };
                                            context.recover_after_reconnect(ts_init).await
                                        } => {
                                            if let Err(e) = result {
                                                log::warn!("Derive post-reconnect recovery failed: {e:?}");
                                            }
                                        }
                                    }
                                }) {
                                    log::warn!("Skipping Derive reconnect recovery after shutdown began: {e}");
                                }
                            }
                            Some(DeriveWsMessage::SessionRecoveryFailed(reason)) => {
                                is_connected.store(false, Ordering::Release);
                                log::error!("Derive execution WebSocket recovery failed: {reason}");
                            }
                            Some(DeriveWsMessage::Subscription(payload))
                                if payload.channel == DeriveWsChannel::balances(dispatch_state.subaccount_id()).to_string() =>
                            {
                                let context = reconciliation.clone();
                                let task_cancellation = cancellation.clone();

                                if let Err(e) = session_spawner.spawn(async move {
                                    tokio::select! {
                                        biased;
                                        () = task_cancellation.cancelled() => {}
                                        result = context.refresh_account_state() => {
                                            if let Err(e) = result {
                                                log::warn!("Derive balance update refresh failed: {e:?}");
                                            }
                                        }
                                    }
                                }) {
                                    log::warn!("Skipping Derive account refresh after shutdown began: {e}");
                                }
                            }
                            Some(message) => handle_ws_message(
                                message,
                                &emitter,
                                account_id,
                                clock,
                                &dispatch_state,
                            ),
                            None => break,
                        }
                    }
                }
            }

            is_connected.store(false, Ordering::Release);

            if !cancellation.is_cancelled() {
                log::warn!("Derive execution WebSocket stream ended unexpectedly");
            }
        })?;

        Ok(())
    }
    fn select_cancel_orders(
        &self,
        cmd: &CancelAllOrders,
    ) -> Option<Vec<(ClientOrderId, VenueOrderId, bool)>> {
        let side_filter = cmd.order_side;
        let cache = self.core.cache();
        let orders = cache.orders_open_refs(
            Some(&self.core.venue),
            Some(&cmd.instrument_id),
            None,
            Some(&self.core.account_id),
            side_filter,
        );
        let mut cancels = Vec::with_capacity(orders.len());

        for order in orders {
            let client_order_id = order.client_order_id();
            if cache.client_id(&client_order_id) != Some(&self.core.client_id) {
                continue;
            }

            let is_trigger = is_derive_trigger_order_type(order.order_type());
            if side_filter.is_none() && !is_trigger {
                continue;
            }

            let Some(venue_order_id) = self
                .dispatch_state
                .bound_venue_order_id(&client_order_id)
                .or_else(|| order.venue_order_id())
            else {
                log::warn!(
                    "Cannot cancel all orders for {}: order {client_order_id} has no venue_order_id",
                    cmd.instrument_id,
                );
                return None;
            };

            cancels.push((client_order_id, venue_order_id, is_trigger));
        }

        Some(cancels)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for DeriveExecutionClient {
    fn is_connected(&self) -> bool {
        self.is_connected.load(Ordering::Acquire)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *DERIVE_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        self.emitter.set_sender(get_exec_event_sender());
        self.restore_active_orders();
        flush_pending_delivery(
            &self.emitter,
            self.core.account_id,
            self.clock,
            &self.dispatch_state,
        );

        if self.core.is_started() {
            return Ok(());
        }

        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}, subaccount_id={}, environment={:?}, proxy_url={:?}",
            self.core.client_id,
            self.core.account_id,
            self.credential.subaccount_id(),
            self.config.environment,
            self.config.proxy_url,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped()
            && self.cancellation_token.is_cancelled()
            && !self.session_tasks.is_open()
            && !self.pending_tasks.is_open()
        {
            return Ok(());
        }

        log::info!("Stopping Derive execution client");

        self.cancellation_token.cancel();
        self.abort_session_tasks();
        self.abort_pending_tasks();

        self.core.set_stopped();
        self.core.set_disconnected();
        self.is_connected.store(false, Ordering::Release);

        log::info!("Derive execution client stopped");
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.stop()
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected()
            && !self.cancellation_token.is_cancelled()
            && self.session_tasks.is_open()
            && self.pending_tasks.is_open()
        {
            return Ok(());
        }

        log::info!("Connecting Derive execution client");

        if self.cancellation_token.is_cancelled()
            || !self.session_tasks.is_open()
            || !self.pending_tasks.is_open()
        {
            self.teardown_partial_connect().await?;
            self.session_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Derive session generation: {e}"))?;
            self.pending_tasks
                .start_generation()
                .map_err(|e| anyhow::anyhow!("Failed to start Derive task generation: {e}"))?;
            self.cancellation_token = CancellationToken::new();
        }

        let cancellation_token = self.cancellation_token.clone();
        let ws_shutdown = self.ws_client.shutdown_handle();

        let setup_guard =
            TaskGroupGuard::new(&[&self.session_tasks, &self.pending_tasks], move || {
                cancellation_token.cancel();
                ws_shutdown.begin_shutdown();
            });

        self.ensure_instruments_initialized()
            .await
            .context("failed to initialize Derive instruments")?;
        self.initialize_portfolio()
            .await
            .context("failed initial Derive account state refresh")?;
        self.restore_active_orders();

        if self.dispatch_state.has_active_orders() {
            let context = self.reconciliation_context();
            let mut instruments: Vec<_> = context
                .orders
                .iter()
                .filter(|(id, _)| self.dispatch_state.identity(id).is_some())
                .map(|(_, binding)| binding.identity.instrument_id)
                .collect::<AHashSet<_>>()
                .into_iter()
                .collect();
            instruments.sort_unstable();
            for instrument_id in instruments {
                context.prime_order_bindings(instrument_id).await?;
            }
        }

        self.ws_client
            .connect()
            .await
            .context("failed to connect Derive WebSocket")?;

        let Some(rx) = self.ws_client.take_event_receiver() else {
            let e = anyhow::anyhow!("Derive execution WS event receiver not initialized");
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Derive execution startup teardown failed: {teardown_error}"
                )));
            }

            return Err(e);
        };

        let subaccount_id = self.credential.subaccount_id();
        let channels = vec![
            DeriveWsChannel::orders(subaccount_id),
            DeriveWsChannel::private_trades(subaccount_id),
            DeriveWsChannel::balances(subaccount_id),
        ];

        if let Err(e) = self.ws_client.subscribe_channels(channels).await {
            log::warn!("Derive private WS subscriptions failed: {e}; tearing down");

            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(anyhow::Error::new(e).context(format!(
                    "Derive execution startup teardown failed: {teardown_error}"
                )));
            }

            return Err(anyhow::Error::new(e).context("failed Derive private WS subscriptions"));
        }

        self.initialize_portfolio()
            .await
            .context("failed subscribed Derive portfolio refresh")?;

        if let Err(e) = self.start_ws_dispatch(rx) {
            if let Err(teardown_error) = self.teardown_partial_connect().await {
                return Err(e.context(format!(
                    "Derive execution startup teardown failed: {teardown_error}"
                )));
            }

            return Err(e.context("failed to register Derive execution WebSocket dispatch task"));
        }

        self.core.set_connected();
        self.is_connected.store(true, Ordering::Release);
        setup_guard.disarm();
        log::info!(
            "Connected Derive execution client ({:?})",
            self.config.environment
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        log::info!("Disconnecting Derive execution client");
        self.teardown_partial_connect().await?;
        log::info!("Derive execution client disconnected");
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

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        if let Err(e) = self.cache_native_instrument(&instrument) {
            log::warn!("Cannot cache Derive instrument {}: {e:#}", instrument.id());
        }
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.reconciliation_context()
            .generate_order_status_report(cmd)
            .await
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let context = self.reconciliation_context();
        let mut reports = context
            .generate_order_status_reports(cmd, false)
            .await?
            .into_complete("order")?;

        if !cmd.open_only {
            let mut active_cmd = cmd.clone();
            active_cmd.open_only = true;
            let active = context
                .generate_order_status_reports(&active_cmd, false)
                .await?
                .into_complete("active order")?;
            reports.extend(active);
        }

        context.ensure_order_context()?;
        let reports = deduplicate_order_status_reports(reports)?;
        log_report_receipt(reports.len(), "OrderStatusReport", cmd.log_receipt_level);
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let log_level = cmd.log_receipt_level;
        let reports = self
            .reconciliation_context()
            .generate_fill_reports(cmd)
            .await?
            .into_complete("fill")?;
        log_report_receipt(reports.len(), "FillReport", log_level);
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let snapshot = self
            .reconciliation_context()
            .generate_position_status_snapshot(cmd)
            .await?;
        log_report_receipt(
            snapshot.reports.len(),
            "PositionStatusReport",
            cmd.log_receipt_level,
        );
        Ok(snapshot.reports)
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let ts_init = self.clock.get_time_ns();
        Box::pin(
            self.reconciliation_context()
                .generate_mass_status(lookback_mins, ts_init),
        )
        .await
        .map(Some)
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self.core.cache().try_order_owned(&cmd.client_order_id)?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Err(reason) = self.validate_order(&order) {
            log::warn!("Cannot submit order {}: {reason}", order.client_order_id());
            self.emitter.emit_order_denied(&order, &reason.to_string());
            return Ok(());
        }

        let is_trigger_order = is_derive_trigger_order_type(order.order_type());
        let market_quote = (order.order_type() == OrderType::Market).then_some(());

        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let http_client = self.http_client.clone();
        let ws_exec = self.ws_exec.clone();
        let signing = self.signing.clone();
        let nonce_manager = self.nonce_manager.clone();
        let wallet_str = self.credential.wallet_address().to_string();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instruments = self.instruments.clone();
        let instrument_id = cmd.instrument_id;
        let order_for_task = order.clone();
        let account_id = self.core.account_id;

        // Capture identity so the WS dispatch can route subsequent updates
        // for this order to proper events rather than execution reports.
        let identity = OrderIdentity {
            instrument_id: order.instrument_id(),
            strategy_id: order.strategy_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
        };

        self.dispatch_state
            .register_identity(order.client_order_id(), identity);
        self.dispatch_state.record_order_shape(
            order.client_order_id(),
            order.quantity(),
            order.price(),
        );

        self.emitter.emit_order_submitted(&order);

        let slippage_bps = self.signing.market_order_slippage_bps;
        let dispatch_state = self.dispatch_state.clone();

        self.spawn_task("submit_order", async move {
            let instrument = match cached_or_fetch_instrument(
                &http_client,
                &instruments,
                &instrument_id,
                &venue_symbol,
            )
            .await
            {
                Ok(i) => i,
                Err(e) => {
                    log::warn!("Failed to resolve instrument {venue_symbol}: {e}");
                    dispatch_state.forget(&order_for_task.client_order_id());
                    let ts = clock.get_time_ns();
                    emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(format!("instrument resolution failed: {e}")), ts, false);
                    return Ok(());
                }
            };

            // Lazy-resolution net: the synchronous deny is skipped when the
            // cache was empty at submit time. OrderSubmitted already fired, so
            // reject here rather than deny.
            if order_for_task.is_reduce_only()
                && instrument.instrument_type == DeriveInstrumentType::Erc20
            {
                let reason = format!(
                    "reduce-only is not supported for spot instrument {}; Derive spot has no position to reduce",
                    order_for_task.instrument_id(),
                );
                log::warn!("{reason}");
                dispatch_state.forget(&order_for_task.client_order_id());
                let ts = clock.get_time_ns();
                emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, false);
                return Ok(());
            }

            // Avoid signing against a quote captured before instrument resolution
            let explicit_price = if market_quote.is_some() {
                let quote = match refresh_market_order_quote(
                    &http_client,
                    &venue_symbol,
                    &instrument,
                    clock,
                )
                .await
                {
                    Ok(quote) => quote,
                    Err(e) => {
                        let reason = format!(
                            "market-order quote refresh failed for {}: {e}",
                            order_for_task.client_order_id(),
                        );
                        log::warn!("{reason}");
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, false);
                        return Ok(());
                    }
                };

                match market_order_limit_price(
                    &quote,
                    order_for_task.order_side(),
                    slippage_bps,
                    instrument.tick_size,
                ) {
                    Some(p) => Some(p),
                    None => {
                        let reason = format!(
                            "market-order slippage bound is non-positive for {} ({} bps)",
                            order_for_task.client_order_id(),
                            slippage_bps,
                        );
                        log::warn!("{reason}");
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, false);
                        return Ok(());
                    }
                }
            } else if matches!(
                order_for_task.order_type(),
                OrderType::StopMarket | OrderType::MarketIfTouched
            ) {
                let trigger_price = match order_for_task.trigger_price() {
                    Some(price) => price.as_decimal(),
                    None => {
                        let reason = format!(
                            "trigger market order {} is missing trigger_price",
                            order_for_task.client_order_id(),
                        );
                        log::warn!("{reason}");
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, false);
                        return Ok(());
                    }
                };

                match trigger_market_limit_price(
                    trigger_price,
                    order_for_task.order_side(),
                    slippage_bps,
                    instrument.tick_size,
                ) {
                    Some(p) => Some(p),
                    None => {
                        let reason = format!(
                            "trigger market-order slippage bound is non-positive for {} ({} bps)",
                            order_for_task.client_order_id(),
                            slippage_bps,
                        );
                        log::warn!("{reason}");
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, false);
                        return Ok(());
                    }
                }
            } else {
                None
            };

            let matching_reservation = match ws_exec
                .reserve_matching_request(
                    "private/order",
                    &instrument.instrument_name,
                )
                .await
            {
                Ok(reservation) => reservation,
                Err(e) => {
                    let (reason, due_post_only) = ws_rejection_reason(&e);
                    log::warn!(
                        "Cannot reserve Derive order quota for {}: {reason}",
                        order_for_task.client_order_id(),
                    );
                    dispatch_state.forget(&order_for_task.client_order_id());
                    let ts = clock.get_time_ns();
                    emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(&reason), ts, due_post_only);
                    return Ok(());
                }
            };

            let expiry =
                match normal_order_signature_expiry(
                    clock,
                    if is_trigger_order {
                        TRIGGER_ORDER_SIGNATURE_TTL.as_secs()
                    } else {
                        signing.signature_expiry_secs
                    },
                ) {
                    Ok(expiry) => expiry,
                    Err(e) => {
                        log::warn!(
                            "Order expiry validation failed for {}: {e}",
                            order_for_task.client_order_id()
                        );
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(format!("order expiry validation failed: {e}")), ts, false);
                        return Ok(());
                    }
                };

            let nonce = match resolve_submit_nonce(
                nonce_manager.next_nonce(&wallet_str, signing.subaccount_id),
                &emitter,
                &dispatch_state,
                &order_for_task,
                clock,
            ) {
                Some(nonce) => nonce,
                None => return Ok(()),
            };

            let payload_result = if is_trigger_order {
                trigger_order_to_derive_payload(
                    &order_for_task,
                    &instrument,
                    signing.subaccount_id,
                    signing.wallet_address,
                    &signing.signer,
                    nonce,
                    expiry,
                    signing.trade_module_address,
                    signing.domain_separator,
                    signing.action_typehash,
                    signing.max_fee_per_contract,
                    explicit_price,
                    "",
                    "",
                )
                .map(|params| params.order)
            } else {
                order_to_derive_payload(
                    &order_for_task,
                    &instrument,
                    signing.subaccount_id,
                    signing.wallet_address,
                    &signing.signer,
                    nonce,
                    expiry,
                    signing.trade_module_address,
                    signing.domain_separator,
                    signing.action_typehash,
                    signing.max_fee_per_contract,
                    explicit_price,
                )
            };

            let payload = match payload_result {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("Order encode failed for {}: {e}", order_for_task.client_order_id());
                    dispatch_state.forget(&order_for_task.client_order_id());
                    let ts = clock.get_time_ns();
                    emit_submit_failure(&emitter, &order_for_task, CommandFailure::not_sent(format!("order encoding failed: {e}")), ts, false);
                    return Ok(());
                }
            };

            // Pre-flight debug log so a venue 11012-style rejection can be
            // diagnosed without re-running with full payload tracing.
            log::debug!(
                "Derive submit payload client_order_id={} instrument_name={} direction={} order_type={} time_in_force={} amount={} limit_price={}",
                order_for_task.client_order_id(),
                payload.instrument_name.as_str(),
                payload.direction,
                payload.order_type,
                payload.time_in_force,
                payload.amount,
                payload.limit_price,
            );

            match ws_exec
                .submit_order_after_rate_limit(&payload, matching_reservation)
                .await
                .map_err(|e| ws_command_failure(&e))
            {
                Ok(result) => {
                    dispatch_order_result(
                        result,
                        order_for_task.client_order_id(),
                        &emitter,
                        account_id,
                        clock,
                        &dispatch_state,
                    );
                    log::debug!(
                        "Order submitted: client_order_id={}",
                        order_for_task.client_order_id(),
                    );
                }
                // See docs/integrations/derive.md "Order rejection semantics".
                Err((CommandFailure::Ambiguous(reason), _)) => {
                    log::warn!(
                        "Derive submit for {} returned ambiguous WS outcome: {reason}; awaiting reconciliation",
                        order_for_task.client_order_id(),
                    );
                }
                Err((failure, due_post_only)) => {
                    log::debug!(
                        "Derive rejected order {}: {failure:?}",
                        order_for_task.client_order_id(),
                    );

                    if dispatch_state
                        .take_identity(&order_for_task.client_order_id())
                        .is_some()
                    {
                        let ts = clock.get_time_ns();
                        emit_submit_failure(&emitter, &order_for_task, failure, ts, due_post_only);
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;
        let denials: Vec<_> = orders
            .iter()
            .map(|order| self.validate_order(order).err())
            .collect();

        if denials.iter().any(Option::is_some) {
            for (order, denial) in orders.iter().zip(denials) {
                let reason = denial.unwrap_or(OrderDeniedReason::OrderListDenied {
                    order_list_id: cmd.order_list.id,
                });

                self.emitter.emit_order_denied(order, &reason.to_string());
            }

            return Ok(());
        }

        for order in orders {
            let sub = SubmitOrder::from_order(
                &order,
                cmd.trader_id,
                cmd.client_id,
                cmd.position_id,
                UUID4::new(),
                cmd.ts_init,
            );
            self.submit_order(sub)?;
        }

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        if let Err(e) = ensure_order_binding_resolved(&self.dispatch_state, cmd.client_order_id) {
            emit_cancel_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                self.dispatch_state
                    .bound_venue_order_id(&cmd.client_order_id)
                    .or(cmd.venue_order_id),
                CommandFailure::not_sent(e.to_string()),
                self.clock.get_time_ns(),
            );
            return Ok(());
        }

        let http_client = self.http_client.clone();
        let ws_exec = self.ws_exec.clone();
        let subaccount_id = self.credential.subaccount_id();
        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let account_id = self.core.account_id;
        let dispatch_state = self.dispatch_state.clone();
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = dispatch_state
            .bound_venue_order_id(&client_order_id)
            .or(cmd.venue_order_id);
        let is_trigger_order = self
            .core
            .cache()
            .order(&client_order_id)
            .is_some_and(|order| is_derive_trigger_order_type(order.order_type()));

        self.spawn_task("cancel_order", async move {
            let venue_order_id = if venue_order_id.is_none() && is_trigger_order
                && !dispatch_state.trigger_active(&client_order_id)
            {
                match resolve_trigger_cancel_id(&http_client, &dispatch_state, subaccount_id, client_order_id, venue_symbol.as_str()).await {
                    Ok(id) => Some(id),
                    Err(reason) => {
                        log::warn!("Cannot cancel trigger order {client_order_id}: {reason}");
                        emit_cancel_failure(&emitter, strategy_id, instrument_id, client_order_id, None, CommandFailure::not_sent(&reason), clock.get_time_ns());
                        return Ok(());
                    }
                }
            } else { dispatch_state.bound_venue_order_id(&client_order_id).or(venue_order_id) };

            let is_trigger_order = is_trigger_order && !dispatch_state.trigger_active(&client_order_id);
            let expected_cancel_id = venue_order_id;

            let outcome = match venue_order_id {
                Some(venue_order_id) if is_trigger_order => {
                    ws_exec
                        .cancel_trigger_order_current(
                            &DeriveCancelTriggerOrderParams::new(subaccount_id, venue_order_id.as_str()),
                            &DeriveCancelParams::new(subaccount_id, venue_symbol.as_str(), venue_order_id.as_str()),
                            || dispatch_state.trigger_active(&client_order_id),
                        )
                        .await
                }
                Some(venue_order_id) => ws_exec
                    .cancel_order(&DeriveCancelParams::new(
                        subaccount_id,
                        venue_symbol.as_str(),
                        venue_order_id.as_str(),
                    ))
                    .await
                    .map(|()| None),
                None => ws_exec
                    .cancel_by_label(&DeriveCancelByLabelParams::new(
                        subaccount_id,
                        client_order_id.as_str(),
                    ))
                    .await
                    .map(|result| {
                        if result.cancelled_orders == 0 {
                            let reason = "no open order matched the client_order_id label";
                            log::debug!(
                                "Derive rejected cancel for {client_order_id}: {reason}"
                            );
                            let ts = clock.get_time_ns();
                            emit_cancel_failure(&emitter, strategy_id, instrument_id, client_order_id, None, CommandFailure::venue_rejected(reason), ts);
                        }

                        None
                    }),
            };

            match outcome.map_err(|e| ws_command_failure(&e)) {
                Ok(Some(canceled_order)) => {
                    emit_trigger_cancel_result(&canceled_order, expected_cancel_id, &cmd, &emitter, &dispatch_state, account_id, clock)?;
                }
                Ok(None) => {}
                // See docs/integrations/derive.md "Order rejection semantics".
                Err((CommandFailure::Ambiguous(reason), _)) => {
                    log::warn!(
                        "Derive cancel for {client_order_id} returned ambiguous WS outcome: {reason}; awaiting reconciliation",
                    );
                }
                Err((failure, _)) => {
                    log::debug!("Derive rejected cancel for {client_order_id}: {failure:?}");
                    let ts = clock.get_time_ns();
                    emit_cancel_failure(&emitter, strategy_id, instrument_id, client_order_id, venue_order_id, failure, ts);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        ensure_bindings_resolved(&self.dispatch_state, Some(cmd.instrument_id))?;
        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let side_filter = cmd.order_side;

        let Some(cancels) = self.select_cancel_orders(&cmd) else {
            return Ok(());
        };

        if side_filter.is_some() && cancels.is_empty() {
            return Ok(());
        }

        let ws_exec = self.ws_exec.clone();
        let subaccount_id = self.credential.subaccount_id();

        let dispatch_state = Arc::clone(&self.dispatch_state);
        self.spawn_task("cancel_all_orders", async move {
            for (client_order_id, venue_order_id, is_trigger) in cancels {
                let is_trigger = is_trigger && !dispatch_state.trigger_active(&client_order_id);
                if side_filter.is_none() && !is_trigger {
                    continue;
                }

                let outcome = if is_trigger {
                    ws_exec
                        .cancel_trigger_order_current(
                            &DeriveCancelTriggerOrderParams::new(
                                subaccount_id,
                                venue_order_id.as_str(),
                            ),
                            &DeriveCancelParams::new(
                                subaccount_id,
                                venue_symbol.as_str(),
                                venue_order_id.as_str(),
                            ),
                            || dispatch_state.trigger_active(&client_order_id),
                        )
                        .await
                        .map(|_| ())
                } else {
                    ws_exec
                        .cancel_order(&DeriveCancelParams::new(
                            subaccount_id,
                            venue_symbol.as_str(),
                            venue_order_id.as_str(),
                        ))
                        .await
                };

                if let Err(e) = outcome {
                    log::warn!(
                        "Derive cancel_all_orders: cancel for {venue_order_id} failed: {:?}",
                        ws_command_failure(&e).0,
                    );
                }
            }

            if side_filter.is_none() {
                match ws_exec
                    .cancel_by_instrument(&DeriveCancelByInstrumentParams::new(
                        subaccount_id,
                        venue_symbol.as_str(),
                    ))
                    .await
                {
                    Ok(result) if result.cancelled_orders == 0 => {
                        log::debug!("No open orders to cancel for {venue_symbol}");
                    }
                    Ok(_) => {}
                    Err(e) => {
                        log::warn!(
                            "Derive cancel_all_orders failed for {venue_symbol}: {:?}",
                            ws_command_failure(&e).0
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        for cancel in &cmd.cancels {
            ensure_order_binding_resolved(&self.dispatch_state, cancel.client_order_id)?;
        }

        for inner in cmd.cancels {
            self.cancel_order(inner)?;
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let ts_now = self.clock.get_time_ns();

        if let Err(e) = ensure_order_binding_resolved(&self.dispatch_state, cmd.client_order_id) {
            emit_modify_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                self.dispatch_state
                    .bound_venue_order_id(&cmd.client_order_id)
                    .or(cmd.venue_order_id),
                CommandFailure::not_sent(e.to_string()),
                ts_now,
            );
            return Ok(());
        }

        let Some(venue_order_id) = self
            .dispatch_state
            .bound_venue_order_id(&cmd.client_order_id)
            .or(cmd.venue_order_id)
        else {
            let reason = "venue_order_id is required for modify";
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            emit_modify_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                CommandFailure::not_sent(reason),
                ts_now,
            );
            return Ok(());
        };

        let Ok(order) = self.core.cache().try_order_owned(&cmd.client_order_id) else {
            let reason = ORDER_NOT_FOUND;
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            emit_modify_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                CommandFailure::not_sent(reason),
                ts_now,
            );
            return Ok(());
        };

        if is_derive_trigger_order_type(order.order_type()) {
            let reason = "Derive trigger orders cannot be modified; cancel and resubmit";
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            emit_modify_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                CommandFailure::not_sent(reason),
                ts_now,
            );
            return Ok(());
        }

        if let Err(e) =
            normal_order_signature_expiry(self.clock, self.signing.signature_expiry_secs)
        {
            emit_modify_failure(
                &self.emitter,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                CommandFailure::not_sent(format!("replace expiry validation failed: {e}")),
                ts_now,
            );
            return Ok(());
        }

        let (quantity, price) = self
            .dispatch_state
            .order_shape(&cmd.client_order_id)
            .unwrap_or_else(|| (order.quantity(), order.price()));
        let target_quantity = cmd.quantity.unwrap_or(quantity);
        let target_price = cmd.price.or(price);

        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let http_client = self.http_client.clone();
        let ws_exec = self.ws_exec.clone();
        let signing = self.signing.clone();
        let nonce_manager = self.nonce_manager.clone();
        let wallet_str = self.credential.wallet_address().to_string();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let instruments = self.instruments.clone();
        let dispatch_state = self.dispatch_state.clone();
        let order_for_task = order;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let stale_venue_order_id = venue_order_id;
        let account_id = self.core.account_id;
        let voi_str = venue_order_id.to_string();

        {
            let _delivery = dispatch_state.delivery_guard();
            if !dispatch_state.mark_pending_modify(client_order_id, stale_venue_order_id) {
                emit_modify_failure(
                    &emitter,
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Some(stale_venue_order_id),
                    CommandFailure::not_sent("Another modify is pending"),
                    clock.get_time_ns(),
                );
                return Ok(());
            }

            dispatch_state.record_modify_target(client_order_id, target_quantity, target_price);
        }

        self.spawn_task("modify_order", async move {
            let instrument = match cached_or_fetch_instrument(
                &http_client,
                &instruments,
                &instrument_id,
                &venue_symbol,
            )
            .await
            {
                Ok(i) => i,
                Err(e) => {
                    let reason = format!("instrument resolution failed: {e}");
                    log::warn!("Cannot modify order {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), CommandFailure::not_sent(&reason), ts);
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            let matching_reservation = match ws_exec
                .reserve_matching_request("private/replace", &instrument.instrument_name)
                .await
            {
                Ok(reservation) => reservation,
                Err(e) => {
                    let (reason, _) = ws_rejection_reason(&e);
                    log::warn!("Cannot reserve Derive replace quota for {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), CommandFailure::not_sent(&reason), ts);
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            let identity = OrderIdentity {
                instrument_id,
                strategy_id,
                order_side: order_for_task.order_side(),
                order_type: order_for_task.order_type(),
            };

            let (remaining_amount, expected_filled_amount) = match replacement_amounts(&http_client, &dispatch_state, client_order_id, identity, stale_venue_order_id, target_quantity.as_decimal()).await {
                Ok(amounts) => amounts,
                Err(e) => {
                    log::warn!("Cannot prepare Derive replace for {client_order_id}: {e:#}");
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), CommandFailure::not_sent(e.to_string()), clock.get_time_ns());
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            let expiry = match normal_order_signature_expiry(clock, signing.signature_expiry_secs) {
                Ok(expiry) => expiry,
                Err(e) => {
                    let reason = format!("replace expiry validation failed: {e}");
                    log::warn!("Cannot modify order {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), CommandFailure::not_sent(&reason), ts);
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            let nonce = match resolve_modify_nonce(
                nonce_manager.next_nonce(&wallet_str, signing.subaccount_id),
                &emitter,
                strategy_id,
                instrument_id,
                client_order_id,
                stale_venue_order_id,
                clock,
            ) {
                Some(nonce) => nonce,
                None => {
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            let mut payload = match order_replace_to_derive_payload(
                &order_for_task,
                &instrument,
                signing.subaccount_id,
                signing.wallet_address,
                &signing.signer,
                nonce,
                expiry,
                signing.trade_module_address,
                signing.domain_separator,
                signing.action_typehash,
                signing.max_fee_per_contract,
                Some(remaining_amount),
                target_price.map(|p| p.as_decimal()),
                &voi_str,
            ) {
                Ok(p) => p,
                Err(e) => {
                    let reason = format!("replace encoding failed: {e}");
                    log::warn!("Cannot modify order {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), CommandFailure::not_sent(&reason), ts);
                    dispatch_state.clear_pending_modify(&client_order_id);
                    return Ok(());
                }
            };

            payload.expected_filled_amount = Some(expected_filled_amount);

            {
                let _delivery = dispatch_state.delivery_guard();
                dispatch_state.record_modify_nonce(client_order_id, nonce);
            }

            let outcome = ws_exec
                .modify_order_after_rate_limit(&payload, matching_reservation)
                .await;

            let _delivery = dispatch_state.delivery_guard();

            match outcome.map_err(|e| ws_command_failure(&e)) {
                Ok(response) => {
                    dispatch_state.record_replace_completion(client_order_id, ReplaceCompletion {
                        old_venue_order_id: stale_venue_order_id,
                        nonce,
                        identity: OrderIdentity {
                            instrument_id,
                            strategy_id,
                            order_side: order_for_task.order_side(),
                            order_type: order_for_task.order_type(),
                        },
                        result: response.result,
                        trades: response.trades,
                    });

                    flush_pending_delivery(&emitter, account_id, clock, &dispatch_state);
                }
                Err((CommandFailure::Ambiguous(reason), _)) => {
                    log::warn!(
                        "Derive modify for {client_order_id} returned ambiguous WS outcome: {reason}; awaiting reconciliation",
                    );
                }
                Err((failure, _)) => {
                    if !dispatch_state.take_pending_modify(
                        &client_order_id,
                        stale_venue_order_id,
                        None,
                    ) {
                        log::debug!(
                            "Skipping private/replace rejection for {client_order_id}: an incoming terminal frame already resolved the modify",
                        );
                        return Ok(());
                    }

                    dispatch_state.take_modify_target(&client_order_id);
                    log::debug!("Derive rejected modify for {client_order_id}: {failure:?}");
                    let ts = clock.get_time_ns();
                    emit_modify_failure(&emitter, strategy_id, instrument_id, client_order_id, Some(stale_venue_order_id), failure, ts);
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let subaccount_id = self.credential.subaccount_id();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        self.spawn_task("query_account", async move {
            let subaccount = http_client
                .get_subaccount(&DeriveGetSubaccountParams::new(subaccount_id))
                .await?;
            let (balances, margins, info) = parse_derive_subaccount_to_balances(&subaccount)?;
            let ts_event = clock.get_time_ns();
            emitter.emit_account_state(balances, margins, true, ts_event, Some(info));
            Ok(())
        });

        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let context = self.reconciliation_context();

        let report_cmd = GenerateOrderStatusReport::new(
            cmd.command_id,
            cmd.ts_init,
            Some(cmd.instrument_id),
            Some(cmd.client_order_id),
            cmd.venue_order_id,
            cmd.params,
            cmd.correlation_id,
        );

        self.spawn_task("query_order", async move {
            let report = context
                .generate_order_status_report(&report_cmd)
                .await
                .with_context(|| {
                    format!(
                        "failed to query Derive order: client_order_id={}, venue_order_id={:?}",
                        cmd.client_order_id, cmd.venue_order_id,
                    )
                })?;

            if let Some(report) = report {
                context.emitter.send_order_status_report(report);
            } else {
                log::debug!(
                    "Derive order not found: client_order_id={}, venue_order_id={:?}",
                    cmd.client_order_id,
                    cmd.venue_order_id,
                );
            }

            Ok(())
        });

        Ok(())
    }
}

fn deduplicate_order_status_reports(
    reports: Vec<OrderStatusReport>,
) -> anyhow::Result<Vec<OrderStatusReport>> {
    let mut indices = AHashMap::new();
    let mut unique: Vec<OrderReportEvidence> = Vec::with_capacity(reports.len());

    for report in reports {
        let Some(&index) = indices.get(&report.venue_order_id) else {
            indices.insert(report.venue_order_id, unique.len());
            unique.push(OrderReportEvidence {
                client_order_id: report.client_order_id,
                ts_terminal_first: report.order_status.is_closed().then_some(report.ts_last),
                ts_active_last: (!report.order_status.is_closed()).then_some(report.ts_last),
                report,
            });

            continue;
        };

        unique[index].observe(report)?;
    }

    Ok(unique.into_iter().map(|evidence| evidence.report).collect())
}

struct OrderReportEvidence {
    report: OrderStatusReport,
    client_order_id: Option<ClientOrderId>,
    ts_terminal_first: Option<UnixNanos>,
    ts_active_last: Option<UnixNanos>,
}

impl OrderReportEvidence {
    fn observe(&mut self, report: OrderStatusReport) -> anyhow::Result<()> {
        let previous = &self.report;
        anyhow::ensure!(
            previous.account_id == report.account_id
                && previous.instrument_id == report.instrument_id
                && previous.order_side == report.order_side
                && previous.order_type == report.order_type
                && (self.client_order_id.is_none()
                    || report.client_order_id.is_none()
                    || self.client_order_id == report.client_order_id),
            "Conflicting Derive order identity for {}",
            report.venue_order_id,
        );

        let consistent = if report.order_status.is_closed() {
            self.ts_active_last.is_none_or(|ts| ts <= report.ts_last)
        } else {
            self.ts_terminal_first.is_none_or(|ts| report.ts_last <= ts)
        };

        anyhow::ensure!(
            consistent,
            "Conflicting Derive order status for {}",
            report.venue_order_id
        );

        self.client_order_id = self.client_order_id.or(report.client_order_id);
        if report.order_status.is_closed() {
            self.ts_terminal_first = Some(
                self.ts_terminal_first
                    .map_or(report.ts_last, |ts| ts.min(report.ts_last)),
            );
        } else {
            self.ts_active_last = Some(
                self.ts_active_last
                    .map_or(report.ts_last, |ts| ts.max(report.ts_last)),
            );
        }

        if report.ts_last > previous.ts_last
            || (report.ts_last == previous.ts_last
                && (report.order_status.is_closed()
                    || (!previous.order_status.is_closed()
                        && previous.client_order_id.is_none()
                        && report.client_order_id.is_some())))
        {
            self.report = report;
        }

        Ok(())
    }
}

fn log_report_receipt(count: usize, report_type: &str, level: LogLevel) {
    match level {
        LogLevel::Off => {}
        LogLevel::Trace => log::trace!("Received {count} {report_type} reports"),
        LogLevel::Debug => log::debug!("Received {count} {report_type} reports"),
        LogLevel::Info => log::info!("Received {count} {report_type} reports"),
        LogLevel::Warning => log::warn!("Received {count} {report_type} reports"),
        LogLevel::Error => log::error!("Received {count} {report_type} reports"),
    }
}

impl Drop for DeriveExecutionClient {
    fn drop(&mut self) {
        self.cancellation_token.cancel();
        self.abort_session_tasks();
    }
}

#[derive(Debug, Clone)]
struct ReconciliationSnapshotRequest {
    sender: Arc<dyn TimeEventSender>,
    next: Arc<Mutex<Option<PendingReconciliationSnapshot>>>,
    cancellation: CancellationToken,
    timeout: Duration,
}

#[derive(Debug)]
struct PendingReconciliationSnapshot {
    message: TimeEventMessage,
    result: tokio::sync::oneshot::Receiver<(DeriveReconciliationContext, UnixNanos)>,
}

impl ReconciliationSnapshotRequest {
    fn new(
        core: ExecutionClientCore,
        context: DeriveReconciliationContext,
        cancellation: CancellationToken,
        timeout: Duration,
    ) -> Option<Self> {
        let request = Self {
            sender: try_get_time_event_sender()?,
            next: Arc::new(Mutex::new(None)),
            cancellation,
            timeout,
        };

        request.arm(core, context);
        Some(request)
    }

    fn arm(&self, core: ExecutionClientCore, mut template: DeriveReconciliationContext) {
        template.orders = AHashMap::new();
        let next = Arc::downgrade(&self.next);
        let sender = Arc::clone(&self.sender);
        let cancellation = self.cancellation.clone();
        let timeout = self.timeout;
        let ts_init = template.clock.get_time_ns();
        let (response, result) = tokio::sync::oneshot::channel();
        let response = RefCell::new(Some(response));

        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            if cancellation.is_cancelled() {
                return;
            }

            let Some(next) = next.upgrade() else {
                return;
            };

            let ts_init = template.clock.get_time_ns();
            let (orders, terminal_revision) =
                cached_order_bindings(&core, &template.dispatch_state);
            let mut context = template.clone();
            context.orders = orders;
            context.terminal_revision = terminal_revision;

            let request = Self {
                sender: Arc::clone(&sender),
                next,
                cancellation: cancellation.clone(),
                timeout,
            };

            request.arm(core.clone(), template.clone());

            if let Some(response) = response.borrow_mut().take() {
                let _ = response.send((context, ts_init));
            }
        }));

        let message = TimeEventMessage::new(
            TimeEvent::new(
                Ustr::from("derive-reconciliation"),
                UUID4::new(),
                ts_init,
                ts_init,
            ),
            callback,
        );
        *self.next.lock() = Some(PendingReconciliationSnapshot { message, result });
    }

    async fn snapshot(&self) -> anyhow::Result<(DeriveReconciliationContext, UnixNanos)> {
        let pending = self
            .next
            .lock()
            .take()
            .context("Derive cache snapshot refresh is already pending")?;
        self.sender.send(pending.message);
        tokio::select! {
            biased;
            () = self.cancellation.cancelled() => anyhow::bail!("Derive cache snapshot refresh canceled"),
            result = time::timeout(self.timeout, pending.result) => {
                result.context("Derive cache snapshot refresh timed out")?
                    .context("Derive cache snapshot refresh stopped without a result")
            }
        }
    }

    fn close(&self) {
        let pending = self.next.lock().take();
        drop(pending);
    }
}

fn cached_order_bindings(
    core: &ExecutionClientCore,
    dispatch_state: &WsDispatchState,
) -> (AHashMap<ClientOrderId, OrderBinding>, Arc<()>) {
    let _delivery = dispatch_state.delivery_guard();
    let cache = core.cache();
    let orders = cache
        .orders_refs(Some(&core.venue), None, None, Some(&core.account_id), None)
        .into_iter()
        .filter(|order| cache.client_id(&order.client_order_id()) == Some(&core.client_id))
        .map(|order| {
            (
                order.client_order_id(),
                OrderBinding {
                    identity: OrderIdentity {
                        instrument_id: order.instrument_id(),
                        strategy_id: order.strategy_id(),
                        order_side: order.order_side(),
                        order_type: order.order_type(),
                    },
                    venue_order_id: order.venue_order_id(),
                    venue_order_legs: order.venue_order_ids().into_iter().copied().collect(),
                },
            )
        })
        .collect();

    (orders, dispatch_state.terminal_revision())
}

#[derive(Debug, Clone)]
struct DeriveReconciliationContext {
    http_client: DeriveHttpClient,
    emitter: ExecutionEventEmitter,
    client_id: ClientId,
    account_id: AccountId,
    subaccount_id: u64,
    clock: &'static AtomicTime,
    dispatch_state: Arc<WsDispatchState>,
    instruments: Arc<AtomicMap<InstrumentId, DeriveInstrument>>,
    orders: AHashMap<ClientOrderId, OrderBinding>,
    terminal_revision: Arc<()>,
}

impl DeriveReconciliationContext {
    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let value = self
            .http_client
            .get_subaccount(&DeriveGetSubaccountParams::new(self.subaccount_id))
            .await
            .context("failed to fetch Derive subaccount snapshot")?;
        self.emit_account_snapshot(&value)
    }

    fn emit_account_snapshot(&self, snapshot: &DeriveSubaccount) -> anyhow::Result<()> {
        let (balances, margins, info) = parse_derive_subaccount_to_balances(snapshot)
            .context("failed to parse Derive subaccount balances")?;
        let ts_event = self.clock.get_time_ns();
        self.emitter
            .emit_account_state(balances, margins, true, ts_event, Some(info));
        Ok(())
    }

    async fn recover_after_reconnect(&self, ts_init: UnixNanos) -> anyhow::Result<()> {
        self.refresh_account_state().await?;
        let mass_status = Box::pin(self.generate_mass_status(None, ts_init)).await?;
        let order_count = mass_status.order_reports().len();
        let fill_count: usize = mass_status.fill_reports().values().map(Vec::len).sum();
        let position_count = mass_status.position_reports().len();
        self.emitter
            .send_execution_report(ExecutionReport::MassStatus(Box::new(mass_status)));
        log::info!(
            "Derive post-reconnect reconciliation submitted: orders={order_count}, fills={fill_count}, positions={position_count}",
        );
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        self.ensure_order_context()?;
        self.resolve_pending_modifies(cmd.instrument_id).await?;
        if cmd.venue_order_id.is_none() && cmd.client_order_id.is_none() {
            log::warn!(
                "Derive generate_order_status_report requires venue_order_id or client_order_id"
            );
            return Ok(None);
        }

        let order = self.resolve_order_record(cmd).await?;

        let Some(mut order) = order else {
            return Ok(None);
        };

        let client_order_id = ClientOrderId::new_checked(order.label)
            .ok()
            .or(cmd.client_order_id);

        if let Ok(venue_order_id) = VenueOrderId::new_checked(&order.order_id) {
            validate_report_binding(
                &self.dispatch_state,
                client_order_id,
                venue_order_id,
                format_instrument_id(order.instrument_name)?,
            )?;
        }

        if let Some(binding) = client_order_id.and_then(|cid| self.order_binding(cid))
            && binding.identity.instrument_id == format_instrument_id(order.instrument_name)?
            && let Some(current) = binding.venue_order_id
            && order.order_id != current.as_str()
        {
            anyhow::ensure!(
                binding
                    .venue_order_legs
                    .iter()
                    .any(|id| id.as_str() == order.order_id),
                "Unknown native Derive order binding"
            );
            order = fetch_order_record(
                &self.http_client,
                self.subaccount_id,
                current,
                Some(binding.identity.instrument_id),
            )
            .await?;
        }

        if let Some(instrument_id) = cmd.instrument_id
            && format_instrument_id(order.instrument_name)? != instrument_id
        {
            log::warn!(
                "Derive order {} is for {} but report requested {}",
                order.order_id,
                order.instrument_name.as_str(),
                instrument_id,
            );
            return Ok(None);
        }

        let instrument_id = format_instrument_id(order.instrument_name)?;
        let client_order_id = ClientOrderId::new_checked(order.label)
            .ok()
            .or(cmd.client_order_id);

        if let Ok(venue_order_id) = VenueOrderId::new_checked(&order.order_id) {
            validate_report_binding(
                &self.dispatch_state,
                client_order_id,
                venue_order_id,
                instrument_id,
            )?;
        }

        if tracked_order_identity(client_order_id, instrument_id, &self.dispatch_state).is_some() {
            self.prime_order_bindings(instrument_id).await?;
        }

        let (price_precision, size_precision) = self.order_precision(&order)?;
        let ts_init = self.clock.get_time_ns();

        let mut report = parse_derive_order_to_report_with_precision(
            &order,
            self.account_id,
            price_precision,
            size_precision,
            ts_init,
        )?;

        // Prefer the parsed label (the venue's source of truth); only stamp
        // the cmd's id when the venue label is absent or unrepresentable.
        if report.client_order_id.is_none()
            && let Some(client_order_id) = cmd.client_order_id
        {
            report = report.with_client_order_id(client_order_id);
        }

        observe_trigger_order(&self.dispatch_state, &order, &report);
        self.project_order_report(&order, report).await.map(Some)
    }

    async fn prime_order_bindings(&self, instrument_id: InstrumentId) -> anyhow::Result<()> {
        for open_only in [true, false] {
            self.generate_order_status_reports(
                &GenerateOrderStatusReports::new(
                    UUID4::new(),
                    self.clock.get_time_ns(),
                    open_only,
                    Some(instrument_id),
                    None,
                    (!open_only).then(|| self.clock.get_time_ns()),
                    None,
                    None,
                ),
                false,
            )
            .await?;
        }

        Ok(())
    }

    async fn resolve_order_record(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<DeriveOrder>> {
        let subaccount_id = self.subaccount_id;

        if let Some(venue_order_id) = cmd.venue_order_id {
            return match fetch_order_record(
                &self.http_client,
                subaccount_id,
                venue_order_id,
                cmd.instrument_id,
            )
            .await
            {
                Ok(order) => Ok(Some(order)),
                Err(e) => {
                    let triggers = self
                        .http_client
                        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                        .await?
                        .orders;

                    match triggers
                        .into_iter()
                        .find(|order| order.order_id.as_str() == venue_order_id.as_str())
                    {
                        Some(order) => Ok(Some(order)),
                        None => Err(e),
                    }
                }
            };
        }

        let label = cmd
            .client_order_id
            .expect("report target checked before resolution");
        let orders = self
            .http_client
            .get_open_orders(&DeriveGetOpenOrdersParams::new(subaccount_id))
            .await?
            .orders;

        if let Some(order) = orders
            .into_iter()
            .find(|order| order.label == label.as_str())
        {
            return Ok(Some(order));
        }

        let triggers = self
            .http_client
            .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
            .await?
            .orders;

        if let Some(order) = triggers
            .into_iter()
            .find(|order| order.label == label.as_str())
        {
            return Ok(Some(order));
        }

        let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());
        let mut pages = PaginationCursor::new();

        loop {
            let mut params = DeriveGetOrderHistoryParams::new(
                subaccount_id,
                pages.page(),
                DERIVE_PRIVATE_PAGE_SIZE,
            );

            if let Some(name) = instrument_name.as_deref() {
                params = params.with_instrument_name(name);
            }

            let result = self.http_client.get_order_history(&params).await?;
            if pages.restart_if_changed(&result.pagination)? {
                continue;
            }

            let more = pages.advance(
                &result.pagination,
                result.orders.iter().map(|order| order.order_id.as_str()),
            )?;

            if let Some(order) = result
                .orders
                .into_iter()
                .find(|order| order.label == label.as_str())
            {
                return Ok(Some(order));
            }

            if !more {
                return Ok(None);
            }
        }
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
        normalize_history_client_order_ids: bool,
    ) -> anyhow::Result<CollectedReports<OrderStatusReport>> {
        self.ensure_order_context()?;
        self.resolve_pending_modifies(cmd.instrument_id).await?;
        let mut complete = true;
        let mut records_complete = true;
        let orders = self.collect_order_records(cmd).await?;

        let ts_init = self.clock.get_time_ns();

        let orders: Vec<DeriveOrder> = orders
            .into_iter()
            .filter(|order| {
                cmd.instrument_id.is_none_or(|instrument_id| {
                    format_instrument_id(order.instrument_name)
                        .map_or(true, |id| id == instrument_id)
                })
            })
            .collect();

        let ambiguous_client_order_ids = if normalize_history_client_order_ids {
            ambiguous_history_client_order_ids(&orders)
        } else {
            AHashSet::new()
        };

        let mut reports = Vec::with_capacity(orders.len());

        for order in orders {
            observe_order_binding(&self.dispatch_state, &order);
            ensure_bindings_resolved(&self.dispatch_state, cmd.instrument_id)?;
            let (price_precision, size_precision) = self.order_precision(&order)?;
            complete &= price_precision.is_some();

            match parse_derive_order_to_report_with_precision(
                &order,
                self.account_id,
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(mut report) => {
                    validate_report_binding(
                        &self.dispatch_state,
                        report.client_order_id,
                        report.venue_order_id,
                        report.instrument_id,
                    )?;
                    observe_trigger_order(&self.dispatch_state, &order, &report);
                    if report
                        .client_order_id
                        .and_then(|cid| self.order_binding(cid))
                        .is_none()
                        && report.client_order_id.is_some_and(|client_order_id| {
                            ambiguous_client_order_ids.contains(&client_order_id.inner())
                        })
                    {
                        report.client_order_id = None;
                    }

                    reports.push(self.project_order_report(&order, report).await?);
                }
                Err(e) if cmd.open_only => {
                    return Err(e.context(format!(
                        "failed to parse active Derive order {:?} on {}",
                        order.order_id, order.instrument_name,
                    )));
                }
                Err(e) => {
                    records_complete = false;
                    log::warn!(
                        "Skipping Derive order {:?} on {:?} in status report: {:?}",
                        order.order_id,
                        order.instrument_name.as_str(),
                        format!("{e:#}"),
                    );
                }
            }
        }

        self.ensure_order_context()?;
        let mut reports = deduplicate_order_status_reports(reports)?;
        retain_order_status_reports(&mut reports, cmd);
        Ok(CollectedReports {
            reports,
            complete: complete && records_complete,
            records_complete,
        })
    }

    async fn resolve_pending_modifies(
        &self,
        instrument_id: Option<InstrumentId>,
    ) -> anyhow::Result<()> {
        while let Some(client_order_id) = {
            let _delivery = self.dispatch_state.delivery_guard();
            self.dispatch_state
                .unresolved_client_order_id(instrument_id)
        } {
            let identity = self
                .dispatch_state
                .identity(&client_order_id)
                .ok_or(UnresolvedOrderBinding(client_order_id))?;
            let parent = self
                .dispatch_state
                .pending_modify(&client_order_id)
                .ok_or(UnresolvedOrderBinding(client_order_id))?;
            let nonce = self
                .dispatch_state
                .modify_nonce(&client_order_id)
                .ok_or(UnresolvedOrderBinding(client_order_id))?;
            let ts_init = self.clock.get_time_ns();
            let mut candidate = None;

            for open_only in [true, false] {
                let records = self
                    .collect_order_records(&GenerateOrderStatusReports::new(
                        UUID4::new(),
                        ts_init,
                        open_only,
                        Some(identity.instrument_id),
                        Some(UnixNanos::from(nonce.saturating_sub(NONCE_FUTURE_NS))),
                        None,
                        None,
                        None,
                    ))
                    .await?;
                candidate = records.into_iter().find(|order| {
                    order.label == client_order_id.as_str()
                        && order.replaced_order_id.as_deref() == Some(parent.as_str())
                        && order.nonce == nonce
                });

                if candidate.is_some() {
                    break;
                }
            }

            let order = candidate.ok_or(UnresolvedOrderBinding(client_order_id))?;
            let venue_order_id = VenueOrderId::new_checked(&order.order_id)?;
            validate_replacement_order(
                &order,
                client_order_id,
                identity,
                &self.dispatch_state,
                venue_order_id,
                venue_order_id,
            )?;
            let _delivery = self.dispatch_state.delivery_guard();
            self.ensure_order_context()?;
            dispatch_order_row(
                &order,
                &self.emitter,
                self.account_id,
                self.clock,
                &self.dispatch_state,
                ts_init,
            );
            ensure_order_binding_resolved(&self.dispatch_state, client_order_id)?;
        }

        Ok(())
    }

    async fn collect_order_records(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<DeriveOrder>> {
        let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());

        let orders = if cmd.open_only {
            let mut orders = self
                .http_client
                .get_open_orders(&DeriveGetOpenOrdersParams::new(self.subaccount_id))
                .await?
                .orders;
            orders.extend(
                self.http_client
                    .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(self.subaccount_id))
                    .await?
                    .orders,
            );
            orders
        } else {
            let start_ms = cmd.start.map(|t| t.as_millis() as i64);
            let end_ms = cmd.end.map(|t| t.as_millis() as i64);
            let mut pages = PaginationCursor::new();
            let mut collected = Vec::new();

            loop {
                let mut params = DeriveGetOrderHistoryParams::new(
                    self.subaccount_id,
                    pages.page(),
                    DERIVE_PRIVATE_PAGE_SIZE,
                )
                .with_window(start_ms, end_ms);

                if let Some(name) = instrument_name.as_deref() {
                    params = params.with_instrument_name(name);
                }

                let result = self.http_client.get_order_history(&params).await?;
                if pages.restart_if_changed(&result.pagination)? {
                    collected.clear();
                    continue;
                }

                let more = pages.advance(
                    &result.pagination,
                    result.orders.iter().map(|order| order.order_id.as_str()),
                )?;
                collected.extend(result.orders);

                if !more {
                    anyhow::ensure!(pages.is_complete(), "incomplete Derive order history");
                    break;
                }
            }

            collected
        };

        Ok(orders)
    }

    fn ensure_order_context(&self) -> anyhow::Result<()> {
        let _delivery = self.dispatch_state.delivery_guard();
        anyhow::ensure!(
            Arc::ptr_eq(
                &self.terminal_revision,
                &self.dispatch_state.terminal_revision()
            ),
            "Derive order context expired after terminal binding eviction; retry with a fresh cache snapshot",
        );
        Ok(())
    }

    fn order_binding(&self, client_order_id: ClientOrderId) -> Option<OrderBinding> {
        self.dispatch_state
            .order_binding(&client_order_id)
            .or_else(|| self.orders.get(&client_order_id).cloned())
    }

    async fn project_order_report(
        &self,
        order: &DeriveOrder,
        mut report: OrderStatusReport,
    ) -> anyhow::Result<OrderStatusReport> {
        self.ensure_order_context()?;

        let Some(client_order_id) = report.client_order_id else {
            return Ok(report);
        };

        let Some(binding) = self
            .order_binding(client_order_id)
            .filter(|binding| binding.identity.instrument_id == report.instrument_id)
        else {
            return Ok(report);
        };

        let Some(current) = binding.venue_order_id else {
            return Err(UnresolvedOrderBinding(client_order_id).into());
        };

        if report.venue_order_id != current {
            if !binding.venue_order_legs.contains(&report.venue_order_id) {
                return Err(UnresolvedOrderBinding(client_order_id).into());
            }

            validate_replacement_order(
                order,
                client_order_id,
                binding.identity,
                &self.dispatch_state,
                report.venue_order_id,
                current,
            )?;
            return Ok(report);
        }

        if binding.venue_order_legs.len() <= 1 && order.replaced_order_id.is_none() {
            return Ok(report);
        }

        let legs = replacement_orders(
            &self.http_client,
            &self.dispatch_state,
            client_order_id,
            binding.identity,
            order.clone(),
            binding.venue_order_legs.clone(),
        )
        .await?;
        self.project_replacement_history(order, &legs, &mut report)?;
        let _delivery = self.dispatch_state.delivery_guard();
        self.ensure_order_context()?;
        ensure_bindings_resolved(&self.dispatch_state, Some(report.instrument_id))?;

        if self.order_binding(client_order_id).as_ref() != Some(&binding) {
            return Err(UnresolvedOrderBinding(client_order_id).into());
        }

        Ok(report)
    }

    fn project_replacement_history(
        &self,
        order: &DeriveOrder,
        legs: &[DeriveOrder],
        report: &mut OrderStatusReport,
    ) -> anyhow::Result<()> {
        let mut filled = Decimal::ZERO;
        let mut ancestor_filled = Decimal::ZERO;
        let mut notional = Decimal::ZERO;
        let mut average_complete = true;

        for leg in legs {
            filled = filled
                .checked_add(leg.filled_amount)
                .context("native cumulative fills exceed decimal range")?;

            if leg.order_id != order.order_id {
                ancestor_filled = ancestor_filled
                    .checked_add(leg.filled_amount)
                    .context("native ancestor fills exceed decimal range")?;
            }

            if leg.filled_amount > Decimal::ZERO {
                if leg.average_price <= Decimal::ZERO {
                    average_complete = false;
                } else {
                    let value = leg
                        .filled_amount
                        .checked_mul(leg.average_price)
                        .context("native fill notional exceeds decimal range")?;
                    notional = notional
                        .checked_add(value)
                        .context("native cumulative notional exceeds decimal range")?;
                }
            }
        }

        report.ts_accepted = crate::http::parse::ms_to_nanos(
            legs.iter()
                .map(|leg| leg.creation_timestamp)
                .min()
                .unwrap_or(order.creation_timestamp),
        );
        let quantity = order
            .amount
            .checked_add(ancestor_filled)
            .context("native logical quantity exceeds decimal range")?;
        let (_, size_precision) = self.order_precision(order)?;
        report.quantity = quantity_from_report_decimal(quantity, size_precision)?;
        report.filled_qty = quantity_from_report_decimal(filled, size_precision)?;
        report.avg_px = if filled > Decimal::ZERO && average_complete {
            Some(
                notional
                    .checked_div(filled)
                    .context("native cumulative average cannot be represented")?,
            )
        } else {
            None
        };

        if report.order_status == OrderStatus::Accepted && filled > Decimal::ZERO {
            report.order_status = OrderStatus::PartiallyFilled;
        }

        Ok(())
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<CollectedReports<FillReport>> {
        self.resolve_pending_modifies(cmd.instrument_id).await?;
        let mut complete = true;
        let mut records_complete = true;
        let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());
        let mut pages = PaginationCursor::new();
        let mut all_trades: Vec<DeriveTrade> = Vec::new();

        loop {
            let mut params = DeriveGetTradeHistoryParams::new(
                self.subaccount_id,
                pages.page(),
                DERIVE_PRIVATE_PAGE_SIZE,
            )
            .with_window(
                cmd.start.map(|t| t.as_millis() as i64),
                cmd.end.map(|t| t.as_millis() as i64),
            );

            if let Some(name) = instrument_name.as_deref() {
                params = params.with_instrument_name(name);
            }

            let result = self.http_client.get_private_trade_history(&params).await?;
            if pages.restart_if_changed(&result.pagination)? {
                all_trades.clear();
                records_complete = true;
                continue;
            }

            records_complete &= result.records_complete;
            let more = pages.advance(
                &result.pagination,
                result.trades.iter().map(|trade| trade.trade_id.as_str()),
            )?;
            all_trades.extend(result.trades);

            if !more {
                break;
            }
        }

        let ts_init = self.clock.get_time_ns();
        records_complete &= pages.is_complete();
        let venue_order_id_filter = cmd
            .venue_order_id
            .as_ref()
            .map(|id| id.as_str().to_string());

        let mut reports = Vec::with_capacity(all_trades.len());
        let mut seen_trade_ids = AHashSet::new();

        for trade in all_trades {
            if let Some(target) = venue_order_id_filter.as_deref()
                && trade.order_id != target
            {
                continue;
            }

            let (price_precision, size_precision) =
                self.report_precision(trade.instrument_name.as_str(), false)?;
            complete &= price_precision.is_some();

            match parse_derive_trade_to_fill_report_with_precision(
                &trade,
                self.account_id,
                Currency::USDC(),
                price_precision,
                size_precision,
                ts_init,
            ) {
                Ok(Some(report)) => {
                    validate_report_binding(
                        &self.dispatch_state,
                        report.client_order_id,
                        report.venue_order_id,
                        report.instrument_id,
                    )?;
                    observe_trigger_fill(&self.dispatch_state, &report);
                    if self.dispatch_state.contains_trade(&report.trade_id)
                        || !seen_trade_ids.insert(report.trade_id)
                    {
                        log::debug!(
                            "Skipping duplicate Derive fill (trade_id={}) in generate_fill_reports",
                            report.trade_id,
                        );
                        continue;
                    }

                    reports.push(report);
                }
                Ok(None) => {}
                Err(e) if e.is::<CommissionError>() => {
                    return Err(e.context(format!(
                        "failed to construct Derive fill {:?} for order {:?} on {:?}",
                        trade.trade_id,
                        trade.order_id,
                        trade.instrument_name.as_str(),
                    )));
                }
                Err(e) => {
                    records_complete = false;
                    log::warn!(
                        "Skipping Derive trade {:?} for order {:?} on {:?} in fill report: {:?}",
                        trade.trade_id,
                        trade.order_id,
                        trade.instrument_name.as_str(),
                        format!("{e:#}"),
                    );
                }
            }
        }

        Ok(CollectedReports {
            reports,
            complete: complete && records_complete,
            records_complete,
        })
    }

    async fn generate_position_status_snapshot(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<PositionStatusSnapshot> {
        let positions = self
            .http_client
            .get_positions(&DeriveGetPositionsParams::new(self.subaccount_id))
            .await?
            .positions;
        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::with_capacity(positions.len());
        let mut instruments = AHashSet::with_capacity(positions.len());

        for position in positions {
            let instrument_id = format_instrument_id(position.instrument_name)?;

            if let Some(target) = cmd.instrument_id
                && instrument_id != target
            {
                continue;
            }

            instruments.insert(instrument_id);

            let (_, size_precision) =
                self.report_precision(position.instrument_name.as_str(), true)?;
            let report = parse_derive_position_to_report_with_precision(
                &position,
                self.account_id,
                size_precision,
                ts_init,
            )?;
            reports.push(report);
        }

        Ok(PositionStatusSnapshot {
            reports,
            instruments,
        })
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
        ts_now: UnixNanos,
    ) -> anyhow::Result<ExecutionMassStatus> {
        self.ensure_order_context()?;
        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|lookback| ts_now.saturating_sub(lookback));

        let open_order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_now,
            true,
            None,
            None,
            None,
            None,
            None,
        );

        let history_order_cmd = GenerateOrderStatusReports::new(
            UUID4::new(),
            ts_now,
            false,
            None,
            start,
            None,
            None,
            None,
        );
        let fill_cmd =
            GenerateFillReports::new(UUID4::new(), ts_now, None, None, start, None, None, None);
        let position_cmd =
            GeneratePositionStatusReports::new(UUID4::new(), ts_now, None, None, None, None, None);

        let (history, open, fills, positions) = tokio::join!(
            self.generate_order_status_reports(&history_order_cmd, true),
            self.generate_order_status_reports(&open_order_cmd, false),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_snapshot(&position_cmd),
        );
        let history = collect_mass_reports(history, "order history", start.is_some())?;
        let open = collect_mass_reports(open, "open orders", start.is_some())?;

        let fills = match fills {
            Err(e)
                if e.downcast_ref::<DeriveHttpError>().is_some_and(|cause| {
                    matches!(
                        cause,
                        DeriveHttpError::Decode(_) | DeriveHttpError::Serde(_)
                    )
                }) =>
            {
                return Err(e);
            }
            other => collect_mass_reports(other, "fill history", start.is_some())?,
        };

        let positions_complete = positions.is_ok();

        let position_snapshot = match positions {
            Ok(snapshot) => snapshot,
            Err(e) if start.is_some() && e.is::<DeriveHttpError>() => {
                log::warn!("Derive position source failed: {e:#}");

                PositionStatusSnapshot {
                    reports: Vec::new(),
                    instruments: AHashSet::new(),
                }
            }
            Err(e) => return Err(e),
        };

        let mut records_complete = history.records_complete
            && open.records_complete
            && fills.records_complete
            && positions_complete;
        let mut reports_complete =
            history.complete && open.complete && fills.complete && positions_complete;
        let history_order_reports = history.reports;
        let open_order_reports = open.reports;
        let mut fill_reports = fills.reports;
        let companions = self
            .collect_companion_order_reports(
                history_order_reports,
                &open_order_reports,
                &fill_reports,
                ts_now,
            )
            .await?;
        reports_complete &= companions.complete;
        records_complete &= companions.records_complete;
        let history_order_reports = companions.reports;
        let order_evidence: AHashSet<_> = history_order_reports
            .iter()
            .chain(open_order_reports.iter())
            .map(|report| (report.venue_order_id, report.instrument_id))
            .collect();

        for report in &fill_reports {
            if !order_evidence.contains(&(report.venue_order_id, report.instrument_id)) {
                reports_complete = false;
                records_complete = false;
                log::warn!(
                    "Derive fill {} for order {} on {} has no matching order report; coverage is incomplete",
                    report.trade_id,
                    report.venue_order_id,
                    report.instrument_id,
                );
            }
        }

        anyhow::ensure!(
            start.is_some() || records_complete,
            "incomplete unbounded Derive report coverage"
        );

        let detached_history_order_ids: AHashSet<VenueOrderId> = history_order_reports
            .iter()
            .filter(|report| report.client_order_id.is_none())
            .map(|report| report.venue_order_id)
            .collect();

        for report in &mut fill_reports {
            if detached_history_order_ids.contains(&report.venue_order_id) {
                report.client_order_id = None;
            }
        }

        log::info!(
            "Received {} historical OrderStatusReports",
            history_order_reports.len()
        );
        log::info!(
            "Received {} open OrderStatusReports",
            open_order_reports.len()
        );
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!(
            "Received {} PositionReports",
            position_snapshot.reports.len()
        );

        let mut touched_instruments = AHashSet::new();
        for report in history_order_reports
            .iter()
            .chain(open_order_reports.iter())
        {
            touched_instruments.insert(report.instrument_id);
        }

        for report in &fill_reports {
            touched_instruments.insert(report.instrument_id);
        }

        let PositionStatusSnapshot {
            reports: position_reports,
            instruments: position_instruments,
        } = position_snapshot;
        let mut mass_status =
            ExecutionMassStatus::new(self.client_id, self.account_id, *DERIVE_VENUE, ts_now, None);
        mass_status.set_report_window(
            start,
            if start.is_some() {
                reports_complete
            } else {
                records_complete
            },
        );

        mass_status.add_order_reports(deduplicate_order_status_reports(
            history_order_reports
                .into_iter()
                .chain(open_order_reports)
                .collect(),
        )?);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);

        if positions_complete {
            add_missing_flat_position_reports(
                &mut mass_status,
                self.account_id,
                touched_instruments,
                &position_instruments,
                &self.instruments,
                ts_now,
            );
        }

        self.ensure_order_context()?;
        Ok(mass_status)
    }

    async fn collect_companion_order_reports(
        &self,
        mut reports: Vec<OrderStatusReport>,
        open_reports: &[OrderStatusReport],
        fills: &[FillReport],
        ts_init: UnixNanos,
    ) -> anyhow::Result<CollectedReports<OrderStatusReport>> {
        let mut order_evidence = AHashSet::new();
        let mut companion_orders = AHashMap::new();
        let mut complete = true;

        for report in reports.iter().chain(open_reports.iter()) {
            order_evidence.insert((report.venue_order_id, report.instrument_id));

            if let Some(client_order_id) = report.client_order_id
                && let Some(binding) = self.order_binding(client_order_id)
                && binding.identity.instrument_id == report.instrument_id
            {
                for id in binding.venue_order_legs {
                    companion_orders.insert((id, report.instrument_id), client_order_id);
                }
            }
        }

        for fill in fills {
            let key = (fill.venue_order_id, fill.instrument_id);
            if order_evidence.contains(&key) {
                continue;
            }

            let Some(client_order_id) = companion_orders.get(&key).copied() else {
                continue;
            };

            let order = fetch_order_record(
                &self.http_client,
                self.dispatch_state.subaccount_id(),
                fill.venue_order_id,
                Some(fill.instrument_id),
            )
            .await?;
            anyhow::ensure!(
                order_response_matches(
                    &order,
                    client_order_id,
                    fill.instrument_id,
                    &self.dispatch_state
                ) && order.order_id == fill.venue_order_id.as_str(),
                "Native companion order identity does not match",
            );
            let (price_precision, size_precision) = self.order_precision(&order)?;
            complete &= price_precision.is_some();
            let report = parse_derive_order_to_report_with_precision(
                &order,
                self.account_id,
                price_precision,
                size_precision,
                ts_init,
            )?;
            validate_report_binding(
                &self.dispatch_state,
                report.client_order_id,
                report.venue_order_id,
                report.instrument_id,
            )?;
            let report = self.project_order_report(&order, report).await?;
            reports.push(report);
            order_evidence.insert(key);
        }

        reports.sort_by_key(|report| {
            let is_current = report
                .client_order_id
                .and_then(|cid| self.order_binding(cid))
                .is_some_and(|binding| binding.venue_order_id == Some(report.venue_order_id));
            (is_current, report.ts_last)
        });

        Ok(CollectedReports {
            reports,
            complete,
            records_complete: true,
        })
    }

    fn order_precision(&self, order: &DeriveOrder) -> anyhow::Result<(Option<u8>, Option<u8>)> {
        let active =
            derive_status_to_nautilus(order.order_status, order.filled_amount, order.amount)
                .is_ok_and(|status| !status.is_closed());
        self.report_precision(order.instrument_name.as_str(), active)
    }

    fn report_precision(
        &self,
        instrument_name: &str,
        required: bool,
    ) -> anyhow::Result<(Option<u8>, Option<u8>)> {
        let precision = report_precision(&self.dispatch_state, instrument_name);
        if precision.0.is_some() {
            return Ok(precision);
        }

        if required {
            let instrument_id = format_instrument_id(instrument_name)?;
            anyhow::bail!("missing Derive instrument metadata for {instrument_id}");
        }

        log::warn!(
            "Missing Derive instrument metadata for historical record on {instrument_name}; coverage is incomplete"
        );
        Ok(precision)
    }
}

fn ambiguous_history_client_order_ids(orders: &[DeriveOrder]) -> AHashSet<Ustr> {
    let mut orders_by_label: AHashMap<Ustr, AHashMap<&str, Option<&str>>> = AHashMap::new();

    for order in orders {
        if order.label.is_empty() {
            continue;
        }

        orders_by_label
            .entry(order.label)
            .or_default()
            .insert(order.order_id.as_str(), order.replaced_order_id.as_deref());
    }

    let mut ambiguous_client_order_ids = AHashSet::new();

    for (label, orders_by_id) in orders_by_label {
        if orders_by_id.len() < 2 {
            continue;
        }

        let predecessors: AHashMap<&str, &str> = orders_by_id
            .iter()
            .filter_map(|(order_id, replaced_order_id)| {
                let replaced_order_id = (*replaced_order_id)?;
                orders_by_id
                    .contains_key(replaced_order_id)
                    .then_some((*order_id, replaced_order_id))
            })
            .collect();

        let predecessor_ids: AHashSet<&str> = predecessors.values().copied().collect();
        let heads: Vec<&str> = orders_by_id
            .keys()
            .copied()
            .filter(|order_id| !predecessor_ids.contains(order_id))
            .collect();

        // One client order may own several venue IDs only when they form one linear replace chain
        let is_linear_chain = predecessors.len() + 1 == orders_by_id.len()
            && predecessor_ids.len() == predecessors.len()
            && heads.len() == 1
            && {
                let mut visited = AHashSet::new();
                let mut current = Some(heads[0]);
                while let Some(order_id) = current {
                    if !visited.insert(order_id) {
                        break;
                    }

                    current = predecessors.get(order_id).copied();
                }

                visited.len() == orders_by_id.len()
            };

        if !is_linear_chain {
            ambiguous_client_order_ids.insert(label);
        }
    }

    ambiguous_client_order_ids
}

#[derive(Debug, thiserror::Error)]
#[error("unresolved Derive venue binding for {0}")]
struct UnresolvedOrderBinding(ClientOrderId);

fn validate_report_binding(
    dispatch_state: &WsDispatchState,
    client_order_id: Option<ClientOrderId>,
    venue_order_id: VenueOrderId,
    instrument_id: InstrumentId,
) -> anyhow::Result<()> {
    let _delivery = dispatch_state.delivery_guard();
    if let Some((client_order_id, _)) =
        tracked_order_identity(client_order_id, instrument_id, dispatch_state)
    {
        if !dispatch_state.knows_venue_leg(&client_order_id, venue_order_id) {
            dispatch_state.defer_binding(client_order_id, venue_order_id);
        }

        if dispatch_state.binding_unresolved(&client_order_id) {
            return Err(UnresolvedOrderBinding(client_order_id).into());
        }
    }

    Ok(())
}

fn ensure_bindings_resolved(
    dispatch_state: &WsDispatchState,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<()> {
    let _delivery = dispatch_state.delivery_guard();
    if let Some(client_order_id) = dispatch_state.unresolved_client_order_id(instrument_id) {
        return Err(UnresolvedOrderBinding(client_order_id).into());
    }

    Ok(())
}

fn ensure_order_binding_resolved(
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
) -> anyhow::Result<()> {
    let _delivery = dispatch_state.delivery_guard();
    if dispatch_state.binding_unresolved(&client_order_id)
        || dispatch_state.pending_modify(&client_order_id).is_some()
    {
        return Err(UnresolvedOrderBinding(client_order_id).into());
    }

    Ok(())
}

fn emit_trigger_cancel_result(
    canceled_order: &DeriveOrder,
    expected_cancel_id: Option<VenueOrderId>,
    cmd: &CancelOrder,
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    account_id: AccountId,
    clock: &'static AtomicTime,
) -> anyhow::Result<()> {
    let client_order_id = cmd.client_order_id;

    if !order_response_matches(
        canceled_order,
        cmd.client_order_id,
        cmd.instrument_id,
        dispatch_state,
    ) || expected_cancel_id.is_none_or(|id| id.as_str() != canceled_order.order_id)
        || canceled_order.order_status != DeriveOrderStatus::Cancelled
    {
        log::warn!(
            "Derive trigger cancellation response does not match {client_order_id}, awaiting reconciliation"
        );
        return Ok(());
    }

    let Ok(canceled_venue_order_id) = VenueOrderId::new_checked(canceled_order.order_id.as_str())
    else {
        log::warn!(
            "Derive cancel outcome for {:?} on {:?} has an invalid response order_id; awaiting reconciliation",
            cmd.client_order_id.as_str(),
            cmd.instrument_id.to_string(),
        );
        return Ok(());
    };

    let ts = clock.get_time_ns();

    if !ensure_canceled_emitted(
        emitter,
        dispatch_state,
        cmd.client_order_id,
        OrderIdentity {
            instrument_id: cmd.instrument_id,
            strategy_id: cmd.strategy_id,
            order_side: match canceled_order.direction {
                DeriveOrderSide::Buy => OrderSide::Buy,
                DeriveOrderSide::Sell => OrderSide::Sell,
            },
            order_type: derive_order_type_to_nautilus_for_order(
                canceled_order.order_type,
                canceled_order.trigger_type,
            )?,
        },
        canceled_venue_order_id,
        account_id,
        ts,
        ts,
    ) {
        return Ok(());
    }

    dispatch_state.forget(&cmd.client_order_id);
    Ok(())
}

async fn resolve_trigger_cancel_id(
    http_client: &DeriveHttpClient,
    dispatch_state: &WsDispatchState,
    subaccount_id: u64,
    client_order_id: ClientOrderId,
    venue_symbol: &str,
) -> Result<VenueOrderId, String> {
    let result = http_client
        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
        .await;

    if let Some(venue_order_id) = dispatch_state.bound_venue_order_id(&client_order_id) {
        return Ok(venue_order_id);
    }

    let orders = result
        .map_err(|e| format!("failed to resolve trigger order by label: {e}"))?
        .orders;

    let order = orders
        .into_iter()
        .find(|order| {
            order.label == client_order_id.as_str() && order.instrument_name == venue_symbol
        })
        .ok_or_else(|| "trigger order not found for client_order_id".to_string())?;

    VenueOrderId::new_checked(&order.order_id).map_err(|e| e.to_string())
}

fn observe_order_binding(dispatch_state: &WsDispatchState, order: &DeriveOrder) {
    let _delivery = dispatch_state.delivery_guard();

    let (Ok(client_order_id), Ok(instrument_id), Ok(venue_order_id)) = (
        ClientOrderId::new_checked(order.label),
        format_instrument_id(order.instrument_name),
        VenueOrderId::new_checked(&order.order_id),
    ) else {
        return;
    };

    if tracked_order_identity(Some(client_order_id), instrument_id, dispatch_state).is_some()
        && dispatch_state
            .bound_venue_order_id(&client_order_id)
            .is_some()
        && !dispatch_state.knows_venue_leg(&client_order_id, venue_order_id)
    {
        dispatch_state.defer_binding(client_order_id, venue_order_id);
    }
}

fn observe_trigger_order(
    dispatch_state: &WsDispatchState,
    order: &DeriveOrder,
    report: &OrderStatusReport,
) {
    let _delivery = dispatch_state.delivery_guard();
    if !dispatch_state.owns_subaccount(order.subaccount_id)
        || !matches!(
            order.order_status,
            DeriveOrderStatus::Open | DeriveOrderStatus::Filled
        )
    {
        return;
    }

    if let Some((client_order_id, identity)) =
        tracked_order_identity(report.client_order_id, report.instrument_id, dispatch_state)
        && is_derive_trigger_order_type(identity.order_type)
        && report.order_type == identity.order_type
        && report.order_side == Some(identity.order_side)
        && dispatch_state
            .bound_venue_order_id(&client_order_id)
            .is_none_or(|id| id == report.venue_order_id)
    {
        dispatch_state.mark_trigger_active(client_order_id);
    }
}

fn observe_trigger_fill(dispatch_state: &WsDispatchState, report: &FillReport) {
    let _delivery = dispatch_state.delivery_guard();
    if let Some((client_order_id, identity)) =
        tracked_order_identity(report.client_order_id, report.instrument_id, dispatch_state)
        && is_derive_trigger_order_type(identity.order_type)
        && report.order_side == identity.order_side
        && dispatch_state
            .bound_venue_order_id(&client_order_id)
            .is_none_or(|id| id == report.venue_order_id)
    {
        dispatch_state.mark_trigger_active(client_order_id);
    }
}

struct CollectedReports<T> {
    reports: Vec<T>,
    complete: bool,
    records_complete: bool,
}

impl<T> CollectedReports<T> {
    fn into_complete(self, source: &str) -> anyhow::Result<Vec<T>> {
        anyhow::ensure!(self.complete, "incomplete Derive {source} report coverage");
        Ok(self.reports)
    }
}

fn collect_mass_reports<T>(
    result: anyhow::Result<CollectedReports<T>>,
    source: &str,
    allow_partial: bool,
) -> anyhow::Result<CollectedReports<T>> {
    match result {
        Ok(collection) => Ok(collection),
        Err(e)
            if !allow_partial || e.is::<CommissionError>() || e.is::<UnresolvedOrderBinding>() =>
        {
            Err(e)
        }
        Err(e) => {
            log::warn!("Derive {source} source failed: {e:#}");
            Ok(CollectedReports {
                reports: Vec::new(),
                complete: false,
                records_complete: false,
            })
        }
    }
}

struct PositionStatusSnapshot {
    reports: Vec<PositionStatusReport>,
    instruments: AHashSet<InstrumentId>,
}

fn ws_command_failure(error: &DeriveWsError) -> (CommandFailure, bool) {
    let (reason, due_post_only) = ws_rejection_reason(error);
    let failure = if is_write_outcome_ambiguous_ws(error) {
        CommandFailure::ambiguous(reason)
    } else if matches!(error, DeriveWsError::JsonRpc { .. }) {
        CommandFailure::venue_rejected(reason)
    } else {
        CommandFailure::not_sent(reason)
    };

    log::warn!("Derive order command failed: {error}");
    (failure, due_post_only)
}

fn command_rejection_reason(failure: CommandFailure) -> Option<String> {
    match failure {
        CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
            Some(strategy_rejection_reason(&reason))
        }
        CommandFailure::Ambiguous(_) => None,
    }
}

fn emit_submit_failure(
    emitter: &ExecutionEventEmitter,
    order: &OrderAny,
    failure: CommandFailure,
    ts: UnixNanos,
    due_post_only: bool,
) {
    if let Some(reason) = command_rejection_reason(failure) {
        emitter.emit_order_rejected(order, &reason, ts, due_post_only);
    }
}

fn emit_cancel_failure(
    emitter: &ExecutionEventEmitter,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    failure: CommandFailure,
    ts: UnixNanos,
) {
    if let Some(reason) = command_rejection_reason(failure) {
        emitter.emit_order_cancel_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            &reason,
            ts,
        );
    }
}

fn emit_modify_failure(
    emitter: &ExecutionEventEmitter,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: Option<VenueOrderId>,
    failure: CommandFailure,
    ts: UnixNanos,
) {
    if let Some(reason) = command_rejection_reason(failure) {
        emitter.emit_order_modify_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            &reason,
            ts,
        );
    }
}

fn ws_rejection_reason(error: &DeriveWsError) -> (String, bool) {
    match error {
        DeriveWsError::JsonRpc { code, message, .. } => (
            format!("{code}: {}", strategy_rejection_reason(message))
                .chars()
                .take(STRATEGY_REASON_MAX_CHARS)
                .collect(),
            derive_rejection_due_post_only(Some(*code), message),
        ),
        DeriveWsError::NotConnected => ("WebSocket client is not connected".to_string(), false),
        DeriveWsError::Auth(_) | DeriveWsError::Authentication { .. } => {
            ("Session authentication failed".to_string(), false)
        }
        DeriveWsError::MissingCredentials { .. } => {
            ("Session credentials are missing".to_string(), false)
        }
        DeriveWsError::Subscription { .. } => ("Session subscription failed".to_string(), false),
        DeriveWsError::RateLimited { .. } => {
            ("Local pacing reservation expired".to_string(), false)
        }
        DeriveWsError::Transport(_) => ("Transport outcome is unconfirmed".to_string(), false),
        DeriveWsError::Serde(_) => ("Order response could not be decoded".to_string(), false),
        DeriveWsError::Timeout { .. } | DeriveWsError::RequestCancelled { .. } => {
            ("Order response is unconfirmed".to_string(), false)
        }
    }
}

fn add_missing_flat_position_reports(
    mass_status: &mut ExecutionMassStatus,
    account_id: AccountId,
    touched_instruments: AHashSet<InstrumentId>,
    position_instruments: &AHashSet<InstrumentId>,
    instruments: &AtomicMap<InstrumentId, DeriveInstrument>,
    ts_init: UnixNanos,
) {
    let mut flat_reports = Vec::new();

    let mut touched_instruments = touched_instruments.into_iter().collect::<Vec<_>>();
    touched_instruments.sort();
    for instrument_id in touched_instruments {
        if position_instruments.contains(&instrument_id)
            || !instruments
                .get_cloned(&instrument_id)
                .is_some_and(|instrument| {
                    matches!(
                        instrument.instrument_type,
                        DeriveInstrumentType::Perp | DeriveInstrumentType::Option,
                    )
                })
        {
            continue;
        }

        flat_reports.push(PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSide::Flat,
            Quantity::from("0"),
            ts_init,
            ts_init,
            Some(UUID4::new()),
            None,
            None,
        ));
    }

    if !flat_reports.is_empty() {
        log::info!(
            "Added {} flat PositionReports for Derive instruments absent from current positions",
            flat_reports.len()
        );
        mass_status.add_position_reports(flat_reports);
    }
}

fn report_precision(
    dispatch_state: &WsDispatchState,
    instrument_name: &str,
) -> (Option<u8>, Option<u8>) {
    let Ok(instrument_id) = format_instrument_id(instrument_name) else {
        return (None, None);
    };

    dispatch_state
        .instrument_precision(&instrument_id)
        .map_or((None, None), |(price, size)| (Some(price), Some(size)))
}

fn handle_ws_message(
    message: DeriveWsMessage,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    flush_pending_delivery(emitter, account_id, clock, dispatch_state);

    let payload = match message {
        DeriveWsMessage::Subscription(payload) => payload,
        DeriveWsMessage::Authenticated
        | DeriveWsMessage::Reconnected
        | DeriveWsMessage::SessionRecoveryFailed(_) => return,
    };

    let channel = DeriveWsChannel::from(payload.channel.as_str());

    if let DeriveWsChannel::Orders { subaccount_id }
    | DeriveWsChannel::PrivateTrades { subaccount_id }
    | DeriveWsChannel::Balances { subaccount_id } = &channel
        && *subaccount_id != dispatch_state.subaccount_id()
    {
        log::warn!("Ignoring Derive private frame for an unexpected subaccount");
        return;
    }

    let is_orders_channel = matches!(channel, DeriveWsChannel::Orders { .. });
    let is_trades_channel = matches!(channel, DeriveWsChannel::PrivateTrades { .. });

    if is_orders_channel {
        let data = match serde_json::from_str::<DeriveOrdersSubscriptionData>(payload.data.get()) {
            Ok(data) => data,
            Err(e) => {
                log::warn!(
                    "Failed to decode Derive orders frame on channel {}: {e}",
                    payload.channel,
                );
                return;
            }
        };

        dispatch_orders_payload(data, emitter, account_id, clock, dispatch_state);
    } else if is_trades_channel {
        let data = match serde_json::from_str::<DeriveTradesSubscriptionData>(payload.data.get()) {
            Ok(data) => data,
            Err(e) => {
                log::warn!(
                    "Failed to decode Derive trades frame on channel {}: {e}",
                    payload.channel,
                );
                return;
            }
        };

        dispatch_trades_payload(data, emitter, account_id, clock, dispatch_state);
    }
}

/// Dispatches a parsed `{subaccount_id}.orders` payload to the execution event
/// emitter.
///
/// Emits tracked order events when an order's client order id resolves to a
/// registered identity in `dispatch_state`, and forwards a raw status report
/// otherwise.
pub fn dispatch_orders_payload(
    data: DeriveOrdersSubscriptionData,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let _delivery = dispatch_state.delivery_guard();
    let ts_init = clock.get_time_ns();
    for order in data.orders {
        dispatch_order_row(&order, emitter, account_id, clock, dispatch_state, ts_init);
    }
}

fn dispatch_order_row(
    order: &DeriveOrder,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if !dispatch_state.owns_subaccount(order.subaccount_id) {
        log::warn!("Ignoring Derive order row for an unexpected subaccount");
        return;
    }

    observe_order_binding(dispatch_state, order);

    let (price_precision, size_precision) =
        report_precision(dispatch_state, order.instrument_name.as_str());

    let mut report = match parse_derive_order_to_report_with_precision(
        order,
        account_id,
        price_precision,
        size_precision,
        ts_init,
    ) {
        Ok(report) => report,
        Err(e) => {
            log::warn!(
                "Failed to parse Derive order {:?} on {:?} in WS update: {:?}",
                order.order_id,
                order.instrument_name.as_str(),
                format!("{e:#}"),
            );
            return;
        }
    };

    observe_trigger_order(dispatch_state, order, &report);
    if report
        .client_order_id
        .is_some_and(|cid| dispatch_state.is_terminal(&cid, report.instrument_id))
    {
        return;
    }

    let identity =
        tracked_order_identity(report.client_order_id, report.instrument_id, dispatch_state);

    match identity {
        Some((client_order_id, identity)) => {
            dispatch_tracked_order_report(
                order,
                &report,
                client_order_id,
                identity,
                emitter,
                account_id,
                clock,
                dispatch_state,
                ts_init,
            );
        }
        None => {
            if report.client_order_id.is_some_and(|cid| {
                dispatch_state
                    .known_instrument(&cid)
                    .is_some_and(|id| id != report.instrument_id)
            }) {
                report.client_order_id = None;
            }

            emitter.send_order_status_report(report);
        }
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "native tracked rows retain the frame identity and delivery context"
)]
fn dispatch_tracked_order_report(
    order: &DeriveOrder,
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    ts_init: UnixNanos,
) {
    if dispatch_state
        .bound_venue_order_id(&client_order_id)
        .is_some_and(|bound| bound != report.venue_order_id)
    {
        if !dispatch_state.knows_venue_leg(&client_order_id, report.venue_order_id) {
            dispatch_state.defer_binding(client_order_id, report.venue_order_id);
        }

        let Some(old_venue_order_id) = dispatch_state.pending_modify(&client_order_id) else {
            return;
        };

        if order.replaced_order_id.as_deref() != Some(old_venue_order_id.as_str())
            || dispatch_state.modify_nonce(&client_order_id) != Some(order.nonce)
            || !matches!(
                order.order_status,
                DeriveOrderStatus::Open
                    | DeriveOrderStatus::Filled
                    | DeriveOrderStatus::Cancelled
                    | DeriveOrderStatus::Expired
            )
        {
            log::debug!(
                "Deferring uncorrelated Derive order leg {} for {client_order_id}",
                report.venue_order_id
            );
            return;
        }

        if !emit_modify_target(
            emitter,
            dispatch_state,
            client_order_id,
            identity,
            report.venue_order_id,
            account_id,
            report.ts_last,
            ts_init,
        ) {
            return;
        }

        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: dispatch_state.take_deferred_trades(&client_order_id),
            },
            emitter,
            account_id,
            clock,
            dispatch_state,
        );
    }

    emit_tracked_order_event(
        emitter,
        dispatch_state,
        client_order_id,
        identity,
        report,
        account_id,
        ts_init,
    );

    if dispatch_state.identity(&client_order_id).is_none() {
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: dispatch_state.take_deferred_trades(&client_order_id),
            },
            emitter,
            account_id,
            clock,
            dispatch_state,
        );
    }
}

/// Dispatches a parsed `{subaccount_id}.trades` payload to the execution event
/// emitter.
///
/// Deduplicates by trade id, then emits a tracked fill when the trade's client
/// order id resolves to a registered identity in `dispatch_state`, and forwards
/// a raw fill report otherwise.
pub fn dispatch_trades_payload(
    data: DeriveTradesSubscriptionData,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let _delivery = dispatch_state.delivery_guard();
    let fee_currency = Currency::USDC();
    let ts_init = clock.get_time_ns();

    for trade in data.trades {
        dispatch_trade_row(
            trade,
            emitter,
            account_id,
            clock,
            dispatch_state,
            ts_init,
            fee_currency,
        );
    }
}

fn dispatch_trade_row(
    trade: DeriveTrade,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
    ts_init: UnixNanos,
    fee_currency: Currency,
) {
    if !dispatch_state.owns_subaccount(trade.subaccount_id) {
        log::warn!("Ignoring Derive trade row for an unexpected subaccount");
        return;
    }

    let (price_precision, size_precision) =
        report_precision(dispatch_state, trade.instrument_name.as_str());

    let mut report = match parse_derive_trade_to_fill_report_with_precision(
        &trade,
        account_id,
        fee_currency,
        price_precision,
        size_precision,
        ts_init,
    ) {
        Ok(Some(report)) => report,
        Ok(None) => return,
        Err(e) => {
            log::warn!(
                "Failed to parse Derive trade {:?} for order {:?} on {:?} in WS update: {:?}",
                trade.trade_id,
                trade.order_id,
                trade.instrument_name.as_str(),
                format!("{e:#}")
            );
            return;
        }
    };

    if dispatch_state.contains_trade(&report.trade_id) {
        log::debug!(
            "Skipping duplicate Derive fill (trade_id={}) on WS dispatch",
            report.trade_id,
        );
        return;
    }

    let identity =
        tracked_order_identity(report.client_order_id, report.instrument_id, dispatch_state);

    let known_side = identity
        .map(|(_, identity)| identity.order_side)
        .or_else(|| {
            report
                .client_order_id
                .and_then(|cid| dispatch_state.order_binding(&cid))
                .filter(|binding| binding.identity.instrument_id == report.instrument_id)
                .map(|binding| binding.identity.order_side)
        });

    if known_side.is_some_and(|side| side != report.order_side) {
        log::warn!(
            "Deferring Derive fill {} with a conflicting tracked order side",
            report.trade_id
        );
        dispatch_state.retain_trade(trade);
        return;
    }

    if let Some((client_order_id, _)) = identity
        && dispatch_state
            .bound_venue_order_id(&client_order_id)
            .is_some()
        && !dispatch_state.knows_venue_leg(&client_order_id, report.venue_order_id)
    {
        dispatch_state.defer_binding(client_order_id, report.venue_order_id);
        dispatch_state.defer_trade(client_order_id, trade);
        return;
    }

    if let Some((client_order_id, _)) = identity
        && dispatch_state
            .bound_venue_order_id(&client_order_id)
            .is_none()
    {
        dispatch_state.record_venue_order_id(client_order_id, report.venue_order_id);
    }

    observe_trigger_fill(dispatch_state, &report);

    let delivered = match identity {
        Some((client_order_id, identity)) => emit_tracked_fill(
            emitter,
            dispatch_state,
            client_order_id,
            identity,
            &report,
            account_id,
            ts_init,
        ),
        None => {
            if report.client_order_id.is_some_and(|cid| {
                dispatch_state
                    .known_instrument(&cid)
                    .is_some_and(|id| id != report.instrument_id)
            }) {
                report.client_order_id = None;
            }

            match emitter.try_send_execution_report(ExecutionReport::Fill(Box::new(report.clone())))
            {
                Ok(()) => true,
                Err(e) => {
                    log::warn!(
                        "Failed to deliver Derive fill report {}: {e}",
                        report.trade_id
                    );
                    false
                }
            }
        }
    };

    if delivered {
        dispatch_state.check_and_insert_trade(report.trade_id);

        if let Some((client_order_id, _)) = identity
            && dispatch_state.identity(&client_order_id).is_none()
        {
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: dispatch_state.take_deferred_trades(&client_order_id),
                },
                emitter,
                account_id,
                clock,
                dispatch_state,
            );
        }
    } else {
        dispatch_state.retain_trade(trade);
    }
}

async fn replacement_amounts(
    http_client: &DeriveHttpClient,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    current_venue_order_id: VenueOrderId,
    target_quantity: Decimal,
) -> anyhow::Result<(Decimal, Decimal)> {
    let current = fetch_order_record(
        http_client,
        dispatch_state.subaccount_id(),
        current_venue_order_id,
        Some(identity.instrument_id),
    )
    .await?;
    anyhow::ensure!(
        current.order_id == current_venue_order_id.as_str(),
        "Native replacement order identity does not match"
    );
    anyhow::ensure!(
        current.order_status == DeriveOrderStatus::Open,
        "Native replacement target is not open; current binding requires reconciliation"
    );
    let current_filled = current.filled_amount.normalize();
    let orders = replacement_orders(
        http_client,
        dispatch_state,
        client_order_id,
        identity,
        current,
        dispatch_state.venue_order_legs(&client_order_id),
    )
    .await?;
    let mut filled = U256::ZERO;

    for order in orders {
        let leg_filled = decimal_to_scaled_u256(order.filled_amount.normalize())
            .map_err(|e| anyhow::anyhow!(e))?;
        filled = filled
            .checked_add(leg_filled)
            .context("native cumulative fills exceed exact integer range")?;
    }

    let target =
        decimal_to_scaled_u256(target_quantity.normalize()).map_err(|e| anyhow::anyhow!(e))?;
    let remaining = target
        .checked_sub(filled)
        .context("requested quantity is below native cumulative fills")?;
    anyhow::ensure!(
        remaining > U256::ZERO,
        "Requested quantity does not leave a positive native replacement amount"
    );

    let remaining = if filled == U256::ZERO {
        target_quantity
    } else {
        decimal_from_scaled(remaining)?
    };

    Ok((remaining, current_filled))
}

async fn replacement_orders(
    http_client: &DeriveHttpClient,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    current: DeriveOrder,
    mut proven_legs: Vec<VenueOrderId>,
) -> anyhow::Result<Vec<DeriveOrder>> {
    let current_venue_order_id = VenueOrderId::new_checked(&current.order_id)?;
    proven_legs.retain(|id| *id != current_venue_order_id);
    proven_legs.insert(0, current_venue_order_id);
    let mut visited = AHashSet::new();
    let mut orders = Vec::new();

    for mut venue_order_id in proven_legs {
        let mut ancestry = AHashSet::new();

        loop {
            anyhow::ensure!(
                ancestry.insert(venue_order_id),
                "Cyclic native replacement ancestry"
            );

            if !visited.insert(venue_order_id) {
                break;
            }

            let order = if venue_order_id == current_venue_order_id {
                current.clone()
            } else {
                fetch_order_record(
                    http_client,
                    dispatch_state.subaccount_id(),
                    venue_order_id,
                    Some(identity.instrument_id),
                )
                .await?
            };

            validate_replacement_order(
                &order,
                client_order_id,
                identity,
                dispatch_state,
                venue_order_id,
                current_venue_order_id,
            )?;

            let previous = order.replaced_order_id.clone();
            orders.push(order);

            let Some(previous) = previous else {
                break;
            };

            venue_order_id = VenueOrderId::new_checked(previous)?;
        }
    }

    Ok(orders)
}

fn validate_replacement_order(
    order: &DeriveOrder,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    dispatch_state: &WsDispatchState,
    venue_order_id: VenueOrderId,
    current_venue_order_id: VenueOrderId,
) -> anyhow::Result<()> {
    anyhow::ensure!(
        order_response_matches(
            order,
            client_order_id,
            identity.instrument_id,
            dispatch_state
        ) && order.order_id == venue_order_id.as_str(),
        "Native replacement order identity does not match"
    );

    let side = match order.direction {
        DeriveOrderSide::Buy => OrderSide::Buy,
        DeriveOrderSide::Sell => OrderSide::Sell,
    };

    anyhow::ensure!(
        side == identity.order_side,
        "Native replacement order side does not match"
    );
    anyhow::ensure!(
        derive_order_type_to_nautilus_for_order(order.order_type, order.trigger_type)?
            == identity.order_type,
        "Native replacement order type does not match"
    );
    anyhow::ensure!(
        order.amount > Decimal::ZERO
            && order.filled_amount >= Decimal::ZERO
            && order.filled_amount <= order.amount,
        "Invalid native replacement amount or filled amount"
    );

    if venue_order_id != current_venue_order_id {
        anyhow::ensure!(
            matches!(
                order.order_status,
                DeriveOrderStatus::Cancelled
                    | DeriveOrderStatus::Filled
                    | DeriveOrderStatus::Expired
            ),
            "Native replacement ancestor is not closed"
        );
    }

    Ok(())
}

async fn fetch_order_record(
    http_client: &DeriveHttpClient,
    subaccount_id: u64,
    venue_order_id: VenueOrderId,
    instrument_id: Option<InstrumentId>,
) -> anyhow::Result<DeriveOrder> {
    let missing = match http_client
        .get_order(&DeriveGetOrderParams::new(
            subaccount_id,
            venue_order_id.as_str(),
        ))
        .await
    {
        Ok(order) => return Ok(order),
        Err(e) if matches!(e, DeriveHttpError::JsonRpc { code: 11006, .. }) => e,
        Err(e) => return Err(e.into()),
    };

    let mut pages = PaginationCursor::new();

    loop {
        let mut params =
            DeriveGetOrderHistoryParams::new(subaccount_id, pages.page(), DERIVE_PRIVATE_PAGE_SIZE);

        if let Some(instrument_id) = instrument_id {
            params = params.with_instrument_name(format_venue_symbol(&instrument_id)?);
        }

        let result = http_client.get_order_history(&params).await?;
        if pages.restart_if_changed(&result.pagination)? {
            continue;
        }

        let more = pages.advance(
            &result.pagination,
            result.orders.iter().map(|order| order.order_id.as_str()),
        )?;

        if let Some(order) = result
            .orders
            .into_iter()
            .find(|order| order.order_id == venue_order_id.as_str())
        {
            return Ok(order);
        }

        if !more {
            return Err(missing.into());
        }
    }
}

fn quantity_from_report_decimal(value: Decimal, precision: Option<u8>) -> anyhow::Result<Quantity> {
    match precision {
        Some(precision) => Quantity::from_decimal_dp(value, precision),
        None => Quantity::from_decimal(value.normalize()),
    }
    .context("native cumulative report quantity cannot be represented")
}

fn decimal_from_scaled(mut value: U256) -> anyhow::Result<Decimal> {
    let mut scale = DECIMAL_SCALE.ilog10();
    let ten = U256::from(10_u8);
    while scale > 0 && value % ten == U256::ZERO {
        value /= ten;
        scale -= 1;
    }

    let coefficient =
        u128::try_from(value).context("native amount exceeds exact decimal coefficient range")?;
    let coefficient = i128::try_from(coefficient)
        .context("native amount exceeds signed decimal coefficient range")?;
    Decimal::try_from_i128_with_scale(coefficient, scale)
        .context("native amount cannot be represented exactly")
}

fn flush_pending_delivery(
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let _delivery = dispatch_state.delivery_guard();
    for (client_order_id, completion) in dispatch_state.replace_completions() {
        if dispatch_replace_completion(
            client_order_id,
            &completion,
            emitter,
            account_id,
            clock,
            dispatch_state,
        ) {
            dispatch_state.clear_replace_completion(&client_order_id);
        }
    }

    dispatch_trades_payload(
        DeriveTradesSubscriptionData {
            trades: dispatch_state.take_undelivered_trades(),
        },
        emitter,
        account_id,
        clock,
        dispatch_state,
    );
}

fn dispatch_replace_completion(
    client_order_id: ClientOrderId,
    completion: &ReplaceCompletion,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) -> bool {
    let identity = completion.identity;
    let old_venue_order_id = completion.old_venue_order_id;
    let ts_init = clock.get_time_ns();
    dispatch_trades_payload(
        DeriveTradesSubscriptionData {
            trades: completion.trades.clone(),
        },
        emitter,
        account_id,
        clock,
        dispatch_state,
    );

    if dispatch_state.pending_modify(&client_order_id) != Some(old_venue_order_id)
        && (dispatch_state.identity(&client_order_id).is_none()
            || dispatch_state
                .bound_venue_order_id(&client_order_id)
                .is_some_and(|bound| bound != old_venue_order_id))
    {
        return true;
    }

    let Some(outcome) = decode_replace_completion(client_order_id, completion, dispatch_state)
    else {
        return false;
    };

    match &outcome {
        DeriveReplaceOutcome::Replaced(order) => {
            if order.nonce != completion.nonce {
                log::warn!(
                    "Derive replacement nonce does not match {client_order_id}; awaiting reconciliation"
                );
                return false;
            }

            let Ok(new_venue_order_id) = VenueOrderId::new_checked(&order.order_id) else {
                log::warn!(
                    "Derive replacement response has an invalid order ID for {client_order_id}, awaiting reconciliation"
                );
                return false;
            };

            if dispatch_state.pending_modify(&client_order_id) == Some(old_venue_order_id)
                && !emit_modify_target(
                    emitter,
                    dispatch_state,
                    client_order_id,
                    identity,
                    new_venue_order_id,
                    account_id,
                    ts_init,
                    ts_init,
                )
            {
                return false;
            }

            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: dispatch_state.take_deferred_trades(&client_order_id),
                },
                emitter,
                account_id,
                clock,
                dispatch_state,
            );

            dispatch_order_result(
                DeriveOrderResult {
                    order: order.clone(),
                    trades: vec![],
                },
                client_order_id,
                emitter,
                account_id,
                clock,
                dispatch_state,
            );

            true
        }
        DeriveReplaceOutcome::Canceled {
            cancelled_order,
            create_order_error,
        } => {
            if dispatch_state.pending_modify(&client_order_id) != Some(old_venue_order_id) {
                return true;
            }

            if !ensure_canceled_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                old_venue_order_id,
                account_id,
                ts_init,
                ts_init,
            ) {
                return false;
            }

            log::warn!(
                "Derive cancels {client_order_id} ({}) without creating its replacement: JSON-RPC {}: {}",
                cancelled_order.order_id,
                create_order_error.code,
                create_order_error.message
            );
            dispatch_state.forget(&client_order_id);
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: dispatch_state.take_deferred_trades(&client_order_id),
                },
                emitter,
                account_id,
                clock,
                dispatch_state,
            );

            true
        }
    }
}

fn decode_replace_completion(
    client_order_id: ClientOrderId,
    completion: &ReplaceCompletion,
    dispatch_state: &WsDispatchState,
) -> Option<DeriveReplaceOutcome> {
    let identity = completion.identity;
    let old_venue_order_id = completion.old_venue_order_id;

    let Ok(expected_instrument) = format_venue_symbol(&identity.instrument_id) else {
        log::warn!("Cannot correlate Derive replacement instrument for {client_order_id}");
        return None;
    };

    let result = match &completion.result {
        Ok(result) => result.clone(),
        Err(e) => {
            log::warn!(
                "Cannot decode Derive replacement authority for {client_order_id}: {e}; awaiting reconciliation"
            );
            return None;
        }
    };

    match result.into_outcome(
        old_venue_order_id.as_str(),
        client_order_id.as_str(),
        dispatch_state.subaccount_id(),
        expected_instrument.as_str(),
    ) {
        Ok(outcome) => Some(outcome),
        Err(e) => {
            log::warn!(
                "Derive replacement response does not match {client_order_id}: {e}, awaiting reconciliation"
            );
            None
        }
    }
}

fn dispatch_order_result(
    result: DeriveOrderResult,
    client_order_id: ClientOrderId,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let _delivery = dispatch_state.delivery_guard();
    let mut is_current = false;

    if let Some(identity) = dispatch_state.identity(&client_order_id) {
        if !order_response_matches(
            &result.order,
            client_order_id,
            identity.instrument_id,
            dispatch_state,
        ) {
            log::warn!(
                "Derive order response does not match {client_order_id}, awaiting reconciliation"
            );
        } else if let Ok(venue_order_id) = VenueOrderId::new_checked(&result.order.order_id) {
            is_current = dispatch_state
                .bound_venue_order_id(&client_order_id)
                .is_none_or(|bound| bound == venue_order_id);
            if is_current
                && !matches!(
                    result.order.order_status,
                    DeriveOrderStatus::Rejected | DeriveOrderStatus::Unknown
                )
            {
                dispatch_state.record_venue_order_id(client_order_id, venue_order_id);
                let ts = clock.get_time_ns();
                ensure_accepted_emitted(
                    emitter,
                    dispatch_state,
                    client_order_id,
                    identity,
                    venue_order_id,
                    account_id,
                    ts,
                    ts,
                );
            }
        } else {
            log::warn!(
                "Derive submit outcome for {:?} on {:?} has an invalid response order_id; awaiting reconciliation",
                client_order_id.as_str(),
                identity.instrument_id.to_string(),
            );
        }
    }

    dispatch_trades_payload(
        DeriveTradesSubscriptionData {
            trades: result.trades,
        },
        emitter,
        account_id,
        clock,
        dispatch_state,
    );

    if is_current {
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![result.order],
            },
            emitter,
            account_id,
            clock,
            dispatch_state,
        );
    }
}

fn order_response_matches(
    order: &DeriveOrder,
    client_order_id: ClientOrderId,
    instrument_id: InstrumentId,
    dispatch_state: &WsDispatchState,
) -> bool {
    dispatch_state.owns_subaccount(order.subaccount_id)
        && order.label == client_order_id.as_str()
        && format_instrument_id(order.instrument_name).is_ok_and(|id| id == instrument_id)
}

fn tracked_order_identity(
    client_order_id: Option<ClientOrderId>,
    instrument_id: InstrumentId,
    dispatch_state: &WsDispatchState,
) -> Option<(ClientOrderId, OrderIdentity)> {
    client_order_id.and_then(|cid| {
        dispatch_state
            .identity(&cid)
            .filter(|identity| identity.instrument_id == instrument_id)
            .map(|identity| (cid, identity))
    })
}

/// Synthesizes and emits `OrderAccepted` when one has not yet been emitted
/// for the order. Used to guarantee the `Submitted -> Accepted -> ...`
/// lifecycle when a fill or terminal event arrives before (or instead of)
/// the venue's `Open` notice.
#[expect(clippy::too_many_arguments)]
fn ensure_accepted_emitted(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> bool {
    let _delivery = dispatch_state.delivery_guard();
    if dispatch_state.contains_accepted(&client_order_id) {
        return true;
    }

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

    if let Err(e) = emitter.try_send_order_event(OrderEventAny::Accepted(accepted)) {
        log::warn!("Failed to deliver Derive accepted for {client_order_id}: {e}");
        return false;
    }

    dispatch_state.mark_accepted(client_order_id);
    true
}

#[expect(clippy::too_many_arguments)]
fn ensure_canceled_emitted(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> bool {
    let _delivery = dispatch_state.delivery_guard();
    if dispatch_state.contains_canceled(&client_order_id) {
        return true;
    }

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
        None,
    );

    if let Err(e) = emitter.try_send_order_event(OrderEventAny::Canceled(canceled)) {
        log::warn!("Failed to deliver Derive canceled for {client_order_id}: {e}");
        return false;
    }

    dispatch_state.mark_canceled(client_order_id);
    true
}

fn emit_tracked_order_event(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    report: &OrderStatusReport,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let venue_order_id = report.venue_order_id;
    let ts_accepted = report.ts_accepted;
    let ts_event = report.ts_last;

    // A `private/replace` cancels the old order and opens a new one under the
    // same label; suppress events for the superseded old venue order id so they
    // don't terminate the order that `modify_order` rebinds via `OrderUpdated`.
    // `pending_modify` covers the in-flight window; the bound-id check covers
    // after the rebind.
    if report.order_status == OrderStatus::Canceled
        && dispatch_state.pending_modify(&client_order_id) == Some(venue_order_id)
    {
        log::debug!(
            "Skipping cancel-replace leg for {client_order_id}: stale venue_order_id={venue_order_id}",
        );
        return;
    }

    if let Some(bound) = dispatch_state.bound_venue_order_id(&client_order_id)
        && bound != venue_order_id
    {
        log::debug!(
            "Skipping stale {:?} for {client_order_id}: venue_order_id={venue_order_id} superseded by {bound}",
            report.order_status
        );
        return;
    }

    if dispatch_state.binding_unresolved(&client_order_id) && report.order_status.is_closed() {
        log::debug!(
            "Deferring terminal Derive status for {client_order_id} while venue binding is unresolved"
        );
        return;
    }

    if matches!(
        report.order_status,
        OrderStatus::Accepted | OrderStatus::PartiallyFilled
    ) && dispatch_state.contains_filled(&client_order_id)
    {
        log::debug!("Skipping stale Accepted for {client_order_id} (already filled)");
        return;
    }

    if matches!(
        report.order_status,
        OrderStatus::Accepted | OrderStatus::PartiallyFilled | OrderStatus::Filled
    ) {
        dispatch_state.record_venue_order_id(client_order_id, venue_order_id);
    }

    if matches!(
        report.order_status,
        OrderStatus::Accepted
            | OrderStatus::PartiallyFilled
            | OrderStatus::Filled
            | OrderStatus::Canceled
            | OrderStatus::Expired
    ) && !ensure_accepted_emitted(
        emitter,
        dispatch_state,
        client_order_id,
        identity,
        venue_order_id,
        account_id,
        ts_accepted,
        ts_init,
    ) {
        return;
    }

    match report.order_status {
        OrderStatus::Accepted | OrderStatus::PartiallyFilled => {}
        OrderStatus::Filled => {
            // The final trades frame can follow the terminal order notice.
            dispatch_state.mark_filled(client_order_id);
        }
        OrderStatus::Canceled => {
            if !ensure_canceled_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                ts_event,
                ts_init,
            ) {
                return;
            }

            dispatch_state.forget(&client_order_id);
        }
        OrderStatus::Expired | OrderStatus::Rejected => {
            emit_expiry_or_rejection(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                report,
                account_id,
                ts_init,
            );
        }
        other => {
            log::debug!(
                "Unhandled tracked order status {other:?} for {client_order_id}, sending as report",
            );
            emitter.send_order_status_report(report.clone());
        }
    }
}

fn emit_expiry_or_rejection(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    report: &OrderStatusReport,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let venue_order_id = report.venue_order_id;
    let ts_event = report.ts_last;

    let (event, status) = if report.order_status == OrderStatus::Rejected {
        if dispatch_state.identity(&client_order_id).is_none() {
            return;
        }

        let reason = report
            .cancel_reason
            .as_deref()
            .unwrap_or("Order rejected by Derive");
        let due_post_only = derive_rejection_due_post_only(None, reason);

        let event = OrderRejected::new(
            emitter.trader_id(),
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            account_id,
            Ustr::from(&strategy_rejection_reason(reason)),
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            due_post_only,
        );
        (OrderEventAny::Rejected(event), "rejected")
    } else {
        let event = OrderExpired::new(
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
        (OrderEventAny::Expired(event), "expired")
    };

    if let Err(e) = emitter.try_send_order_event(event) {
        log::warn!("Failed to deliver Derive {status} for {client_order_id}: {e}");
        return;
    }

    dispatch_state.forget(&client_order_id);
}

#[expect(clippy::too_many_arguments)]
fn emit_modify_target(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    venue_order_id: VenueOrderId,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> bool {
    if let Some((quantity, price)) = dispatch_state.modify_target(&client_order_id) {
        if let Err(e) = emitter.try_send_order_event(OrderEventAny::Updated(OrderUpdated::new(
            emitter.trader_id(),
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            quantity,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            price,
            None,
            None,
            false,
        ))) {
            log::warn!("Failed to deliver Derive modify for {client_order_id}: {e}");
            return false;
        }

        dispatch_state.record_order_shape(client_order_id, quantity, price);
        dispatch_state.take_modify_target(&client_order_id);
    }

    if let Some(old_venue_order_id) = dispatch_state.pending_modify(&client_order_id) {
        dispatch_state.take_pending_modify(
            &client_order_id,
            old_venue_order_id,
            Some(venue_order_id),
        );
    }

    true
}

fn emit_tracked_fill(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    report: &FillReport,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> bool {
    if !ensure_accepted_emitted(
        emitter,
        dispatch_state,
        client_order_id,
        identity,
        report.venue_order_id,
        account_id,
        report.ts_event,
        ts_init,
    ) {
        return false;
    }

    let filled = OrderFilled::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        report.venue_order_id,
        account_id,
        report.trade_id,
        identity.order_side,
        identity.order_type,
        report.last_qty,
        report.last_px,
        report.commission.currency,
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        ts_init,
        false,
        report.venue_position_id,
        Some(report.commission),
        None,
    );

    if let Err(e) = emitter.try_send_order_event(OrderEventAny::Filled(filled)) {
        log::warn!("Failed to deliver Derive fill for {client_order_id}: {e}");
        return false;
    }

    dispatch_state.record_fill(client_order_id, report.last_qty);
    true
}

/// Derives the worst-acceptable limit price for a market order from the
/// top-of-book quote and a slippage bound in basis points, rounded to the
/// instrument's `tick_size`.
///
/// Buys lift the ask by `slippage_bps` then round up to the next tick; sells
/// drop the bid by the same and round down. The result is the signed
/// `limit_price` slot in the EIP-712 trade module data; the venue uses it
/// as a worst-case bound while the order sweeps. A non-positive sell bound
/// is rejected (`None`) so the caller can deny the order rather than sign
/// an invalid zero limit.
fn market_order_limit_price(
    quote: &QuoteTick,
    side: OrderSide,
    slippage_bps: u32,
    tick_size: Decimal,
) -> Option<Decimal> {
    let bps = Decimal::from(slippage_bps);
    let scale = Decimal::from(10_000_u32);
    let one = Decimal::ONE;
    let raw = match side {
        OrderSide::Buy => quote.ask_price.as_decimal() * (one + bps / scale),
        OrderSide::Sell => quote.bid_price.as_decimal() * (one - bps / scale),
    };

    let rounded = round_to_tick(raw, tick_size, side);
    if rounded <= Decimal::ZERO {
        return None;
    }

    Some(rounded)
}

fn trigger_market_limit_price(
    trigger_price: Decimal,
    side: OrderSide,
    slippage_bps: u32,
    tick_size: Decimal,
) -> Option<Decimal> {
    let bps = Decimal::from(slippage_bps);
    let scale = Decimal::from(10_000_u32);
    let one = Decimal::ONE;
    let raw = match side {
        OrderSide::Buy => trigger_price * (one + bps / scale),
        OrderSide::Sell => trigger_price * (one - bps / scale),
    };

    let rounded = round_to_tick(raw, tick_size, side);
    if rounded <= Decimal::ZERO {
        return None;
    }

    Some(rounded)
}

fn is_derive_trigger_order_type(order_type: OrderType) -> bool {
    matches!(
        order_type,
        OrderType::StopMarket
            | OrderType::StopLimit
            | OrderType::MarketIfTouched
            | OrderType::LimitIfTouched
    )
}

fn resolve_submit_nonce(
    nonce: Result<u64, NonceError>,
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    order: &OrderAny,
    clock: &'static AtomicTime,
) -> Option<u64> {
    match nonce {
        Ok(nonce) => Some(nonce),
        Err(e) => {
            let reason = format!("nonce allocation failed: {e}");
            log::warn!("Cannot submit order {}: {reason}", order.client_order_id());
            dispatch_state.forget(&order.client_order_id());
            emit_submit_failure(
                emitter,
                order,
                CommandFailure::not_sent(&reason),
                clock.get_time_ns(),
                false,
            );
            None
        }
    }
}

fn resolve_modify_nonce(
    nonce: Result<u64, NonceError>,
    emitter: &ExecutionEventEmitter,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    clock: &'static AtomicTime,
) -> Option<u64> {
    match nonce {
        Ok(nonce) => Some(nonce),
        Err(e) => {
            let reason = format!("nonce allocation failed: {e}");
            log::warn!("Cannot modify order {client_order_id}: {reason}");
            emit_modify_failure(
                emitter,
                strategy_id,
                instrument_id,
                client_order_id,
                Some(venue_order_id),
                CommandFailure::not_sent(&reason),
                clock.get_time_ns(),
            );
            None
        }
    }
}

fn normal_order_signature_expiry(
    clock: &'static AtomicTime,
    signature_expiry_secs: u64,
) -> anyhow::Result<i64> {
    let min_ttl_secs = MIN_SIGNATURE_TTL.as_secs();
    if signature_expiry_secs <= min_ttl_secs {
        anyhow::bail!(
            "signature_expiry_secs {signature_expiry_secs}s must be greater than the Derive minimum {min_ttl_secs}s"
        );
    }

    if signature_expiry_secs > 120 * 24 * 60 * 60 {
        anyhow::bail!("signature_expiry_secs exceeds the Derive maximum of 120 days");
    }

    let now_secs_u64 = clock.get_time_ns().as_u64() / 1_000_000_000;

    let now_secs = i64::try_from(now_secs_u64).with_context(|| {
        format!("current UNIX time {now_secs_u64}s cannot fit in Derive signature_expiry_sec")
    })?;

    let ttl_secs = i64::try_from(signature_expiry_secs).with_context(|| {
        format!(
            "signature_expiry_secs {signature_expiry_secs}s cannot fit in Derive signature_expiry_sec"
        )
    })?;

    now_secs.checked_add(ttl_secs).ok_or_else(|| {
        anyhow::anyhow!(
            "signature expiry overflows Derive signature_expiry_sec: now {now_secs}s plus TTL {ttl_secs}s"
        )
    })
}

async fn refresh_market_order_quote(
    http_client: &DeriveHttpClient,
    venue_symbol: &str,
    instrument: &DeriveInstrument,
    clock: &'static AtomicTime,
) -> anyhow::Result<QuoteTick> {
    let ticker = http_client.get_ticker(venue_symbol).await?;
    let price_precision = Price::from_decimal(instrument.tick_size)
        .with_context(|| format!("invalid Derive tick_size for {venue_symbol}"))?
        .precision;
    let size_precision = Quantity::from_decimal(instrument.amount_step)
        .with_context(|| format!("invalid Derive amount_step for {venue_symbol}"))?
        .precision;

    parse_ticker_quote_from_rest(
        &ticker,
        price_precision,
        size_precision,
        clock.get_time_ns(),
    )
}

/// Rounds `value` to the nearest multiple of `tick_size`. Buys round up so
/// the signed bound remains acceptable to the venue; sells round down so the
/// caller does not accidentally tighten the floor. A non-positive `tick_size`
/// is treated as a no-op.
fn round_to_tick(value: Decimal, tick_size: Decimal, side: OrderSide) -> Decimal {
    if tick_size <= Decimal::ZERO {
        return value;
    }

    let ratio = value / tick_size;
    let ticks = match side {
        OrderSide::Buy => ratio.ceil(),
        OrderSide::Sell => ratio.floor(),
    };

    ticks * tick_size
}

async fn cached_or_fetch_instrument(
    http_client: &DeriveHttpClient,
    instruments: &Arc<AtomicMap<InstrumentId, DeriveInstrument>>,
    instrument_id: &InstrumentId,
    venue_symbol: &str,
) -> anyhow::Result<DeriveInstrument> {
    if let Some(cached) = instruments.get_cloned(instrument_id) {
        return Ok(cached);
    }

    let instrument = http_client
        .get_instrument(venue_symbol)
        .await
        .with_context(|| format!("failed to fetch instrument {venue_symbol}"))?;
    instruments.insert(*instrument_id, instrument.clone());
    Ok(instrument)
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc};

    use nautilus_common::{
        cache::Cache,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_core::UnixNanos;
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        data::QuoteTick,
        enums::{AccountType, LiquiditySide, OmsType, TimeInForce},
        events::OrderFillVoided,
        identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TradeId, TraderId},
        orders::OrderTestBuilder,
        types::{Money, Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::{
            consts::DERIVE,
            enums::{DeriveEnvironment, DeriveOrderStatus, DeriveOrderType},
        },
        websocket::WsSubscriptionPayload,
    };

    #[derive(Debug)]
    struct ReconciliationTimeSender(tokio::sync::mpsc::UnboundedSender<TimeEventMessage>);

    impl TimeEventSender for ReconciliationTimeSender {
        fn send(&self, message: TimeEventMessage) {
            let _ = self.0.send(message);
        }
    }

    #[rstest]
    #[case::markup("<b>Insufficient</b>\n\tmargin\u{0}", "11008: Insufficient margin")]
    #[case::blank("<p>\u{0}\n</p>", "11008: Order command rejected")]
    #[case::unicode("  Échec   de\r budget  ", "11008: Échec de budget")]
    fn test_ws_rejection_reason_is_bounded_venue_text(
        #[case] message: &str,
        #[case] expected: &str,
    ) {
        let error = DeriveWsError::JsonRpc {
            code: 11008,
            message: message.to_string(),
            data: None,
        };

        let (reason, due_post_only) = ws_rejection_reason(&error);
        assert_eq!(reason, expected);
        assert!(due_post_only);
        assert_eq!(
            match error {
                DeriveWsError::JsonRpc { message, .. } => message,
                _ => unreachable!(),
            },
            message
        );
    }

    #[rstest]
    fn test_ws_rejection_reason_limits_unicode_characters() {
        let message = "é".repeat(300);

        let error = DeriveWsError::JsonRpc {
            code: 11009,
            message,
            data: None,
        };

        let (reason, due_post_only) = ws_rejection_reason(&error);
        assert_eq!(reason, format!("11009: {}", "é".repeat(249)));
        assert_eq!(reason.chars().count(), 256);
        assert!(!due_post_only);
    }

    #[rstest]
    #[case::not_connected(
        DeriveWsError::NotConnected,
        CommandFailure::not_sent("WebSocket client is not connected"),
        false
    )]
    #[case::local_pacing(
        DeriveWsError::RateLimited { retry_after: Duration::from_millis(125) },
        CommandFailure::not_sent("Local pacing reservation expired"),
        false
    )]
    #[case::credentials(DeriveWsError::MissingCredentials { operation: "private/order".into() }, CommandFailure::not_sent("Session credentials are missing"), false)]
    #[case::authentication(DeriveWsError::Authentication { operation: "private/order".into(), reason: "opaque sensitive detail".into() }, CommandFailure::not_sent("Session authentication failed"), false)]
    #[case::transport(
        DeriveWsError::transport("opaque sensitive URL"),
        CommandFailure::ambiguous("Transport outcome is unconfirmed"),
        false
    )]
    #[case::timeout(DeriveWsError::Timeout { method: "private/order".into() }, CommandFailure::ambiguous("Order response is unconfirmed"), false)]
    #[case::cancelled(DeriveWsError::RequestCancelled { method: "private/order".into() }, CommandFailure::ambiguous("Order response is unconfirmed"), false)]
    #[case::venue(DeriveWsError::JsonRpc { code: 11008, message: "Post only order cannot cross the market".into(), data: None }, CommandFailure::venue_rejected("11008: Post only order cannot cross the market"), true)]
    #[case::internal(DeriveWsError::JsonRpc { code: -32603, message: "Internal error".into(), data: None }, CommandFailure::ambiguous("-32603: Internal error"), false)]
    fn test_ws_command_failure_keeps_evidence_separate_from_retry(
        #[case] error: DeriveWsError,
        #[case] expected: CommandFailure,
        #[case] due_post_only: bool,
    ) {
        assert_eq!(ws_command_failure(&error), (expected, due_post_only));
    }

    const TEST_WALLET: &str = "0x0000000000000000000000000000000000001234";
    const TEST_SESSION_KEY: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
    const TEST_SUBACCOUNT: u64 = 30769;

    #[rstest]
    #[case::newer(200, OrderStatus::Accepted, OrderStatus::Accepted, true)]
    #[case::older(50, OrderStatus::Canceled, OrderStatus::Accepted, false)]
    #[case::same_time_active(100, OrderStatus::Canceled, OrderStatus::Accepted, false)]
    #[case::same_time_terminal(100, OrderStatus::Accepted, OrderStatus::Canceled, true)]
    fn test_duplicate_reports_keep_latest_and_terminal_at_equal_time(
        #[case] timestamp: u64,
        #[case] previous_status: OrderStatus,
        #[case] status: OrderStatus,
        #[case] choose_next: bool,
    ) {
        let native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let mut previous = parse_derive_order_to_report_with_precision(
            &native,
            AccountId::from("DERIVE-001"),
            Some(2),
            Some(3),
            UnixNanos::from(77),
        )
        .unwrap();
        previous.ts_last = UnixNanos::from(100);
        previous.order_status = previous_status;
        let mut next = previous.clone();
        next.ts_last = UnixNanos::from(timestamp);
        next.order_status = status;
        next.report_id = UUID4::new();
        let expected = if choose_next {
            next.clone()
        } else {
            previous.clone()
        };

        let actual = deduplicate_order_status_reports(vec![previous, next]).unwrap();

        assert_eq!(actual, vec![expected]);
    }

    #[rstest]
    fn test_duplicate_reports_reject_reopened_terminal_id(#[values(false, true)] reversed: bool) {
        let native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let mut terminal = parse_derive_order_to_report_with_precision(
            &native,
            AccountId::from("DERIVE-001"),
            Some(2),
            Some(3),
            UnixNanos::from(77),
        )
        .unwrap();
        terminal.order_status = OrderStatus::Canceled;
        terminal.ts_last = UnixNanos::from(100);
        let mut active = terminal.clone();
        active.order_status = OrderStatus::Accepted;
        active.ts_last = UnixNanos::from(200);

        let reports = if reversed {
            vec![active, terminal]
        } else {
            vec![terminal, active]
        };

        let result = deduplicate_order_status_reports(reports);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("Conflicting Derive order status for {}", native.order_id)
        );
    }

    #[rstest]
    #[case::account(0)]
    #[case::instrument(1)]
    #[case::side(2)]
    #[case::order_type(3)]
    #[case::client_order(4)]
    fn test_duplicate_reports_reject_conflicting_identity(#[case] field: u8) {
        let native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let previous = parse_derive_order_to_report_with_precision(
            &native,
            AccountId::from("DERIVE-001"),
            Some(2),
            Some(3),
            UnixNanos::from(77),
        )
        .unwrap();
        let mut next = previous.clone();
        next.ts_last = UnixNanos::from(previous.ts_last.as_u64() + 1);
        match field {
            0 => next.account_id = AccountId::from("DERIVE-002"),
            1 => next.instrument_id = InstrumentId::from("BTC-PERP.DERIVE"),
            2 => next.order_side = Some(OrderSide::Sell),
            3 => next.order_type = OrderType::Limit,
            4 => next.client_order_id = Some(ClientOrderId::from("OTHER-ORDER")),
            _ => unreachable!(),
        }

        let result = deduplicate_order_status_reports(vec![previous, next]);

        assert_eq!(
            result.unwrap_err().to_string(),
            format!("Conflicting Derive order identity for {}", native.order_id)
        );
    }

    #[rstest]
    fn test_duplicate_reports_preserve_known_active_attribution(
        #[values(false, true)] reversed: bool,
    ) {
        let native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let mut known = parse_derive_order_to_report_with_precision(
            &native,
            AccountId::from("DERIVE-001"),
            Some(2),
            Some(3),
            UnixNanos::from(77),
        )
        .unwrap();
        known.order_status = OrderStatus::Accepted;
        known.client_order_id = Some(ClientOrderId::from("KNOWN-ORDER"));
        let mut unattributed = known.clone();
        unattributed.client_order_id = None;
        let expected = known.clone();

        let reports = if reversed {
            vec![known, unattributed]
        } else {
            vec![unattributed, known]
        };

        let actual = deduplicate_order_status_reports(reports).unwrap();

        assert_eq!(actual, vec![expected]);
    }

    #[rstest]
    #[case([0, 1, 2])]
    #[case([0, 2, 1])]
    #[case([1, 0, 2])]
    #[case([1, 2, 0])]
    #[case([2, 0, 1])]
    #[case([2, 1, 0])]
    fn test_duplicate_reports_retain_all_conflict_evidence(
        #[case] permutation: [usize; 3],
        #[values(false, true)] identity_conflict: bool,
    ) {
        let native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let report = parse_derive_order_to_report_with_precision(
            &native,
            AccountId::from("DERIVE-001"),
            Some(2),
            Some(3),
            UnixNanos::from(77),
        )
        .unwrap();
        let mut observations = [report.clone(), report.clone(), report];

        let timestamps = if identity_conflict {
            [100, 200, 300]
        } else {
            [100, 300, 200]
        };

        for (index, observation) in observations.iter_mut().enumerate() {
            observation.ts_last = UnixNanos::from(timestamps[index]);
            observation.order_status = if identity_conflict || index == 2 {
                OrderStatus::Accepted
            } else {
                OrderStatus::Canceled
            };
        }

        if identity_conflict {
            observations[0].client_order_id = Some(ClientOrderId::from("FIRST-ORDER"));
            observations[1].client_order_id = None;
            observations[2].client_order_id = Some(ClientOrderId::from("OTHER-ORDER"));
        }

        let reports = permutation
            .into_iter()
            .map(|index| observations[index].clone())
            .collect();
        let result = deduplicate_order_status_reports(reports);

        let conflict = if identity_conflict {
            "identity"
        } else {
            "status"
        };

        assert_eq!(
            result.unwrap_err().to_string(),
            format!(
                "Conflicting Derive order {conflict} for {}",
                native.order_id
            )
        );
    }

    fn test_core() -> ExecutionClientCore {
        let cache = Rc::new(RefCell::new(Cache::default()));

        ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from(DERIVE),
            *DERIVE_VENUE,
            OmsType::Netting,
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            None,
            cache,
        )
    }

    fn test_config() -> DeriveExecutionClientConfig {
        DeriveExecutionClientConfig {
            wallet_address: Some(TEST_WALLET.to_string()),
            session_key: Some(TEST_SESSION_KEY.into()),
            subaccount_id: Some(TEST_SUBACCOUNT),
            environment: DeriveEnvironment::Testnet,
            domain_separator: Some(
                "0x2222222222222222222222222222222222222222222222222222222222222222".to_string(),
            ),
            action_typehash: Some(
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            ),
            trade_module_address: Some("0x000000000000000000000000000000000000bbbb".to_string()),
            max_fee_per_contract: Some(dec!(1000)),
            ..DeriveExecutionClientConfig::default()
        }
    }

    #[tokio::test]
    async fn test_private_stream_closure_marks_disconnected_without_terminal_events() {
        let (sender, mut events) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::live::runner::replace_exec_event_sender(sender);
        let client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        client.is_connected.store(true, Ordering::Release);
        client.start_ws_dispatch(rx).unwrap();
        assert!(client.is_connected());
        drop(tx);
        client.session_tasks.begin_shutdown();
        client
            .session_tasks
            .finish_shutdown(Duration::from_secs(1), Duration::from_secs(1))
            .await
            .unwrap();
        assert!(!client.is_connected());
        assert!(events.try_recv().is_err());
    }

    #[tokio::test]
    async fn test_reconnect_context_rejects_evicted_replacement_proof() {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from(DERIVE),
            *DERIVE_VENUE,
            OmsType::Netting,
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let client = DeriveExecutionClient::new(core, test_config()).unwrap();
        let stale = client.reconciliation_context();
        let (time_sender, mut time_events) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::runner::set_time_event_sender(Arc::new(ReconciliationTimeSender(
            time_sender,
        )));
        let request = ReconciliationSnapshotRequest::new(
            client.core.clone(),
            stale.clone(),
            client.cancellation_token.clone(),
            Duration::from_secs(1),
        )
        .unwrap();
        let cid = ClientOrderId::from("AFTER-CONNECT-REPLACEMENT");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let parent = VenueOrderId::from("expired-proof-parent");
        let current = VenueOrderId::from("expired-proof-current");
        let account_id = AccountId::from("DERIVE-001");
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(client.core.trader_id)
            .strategy_id(StrategyId::from("S-1"))
            .instrument_id(instrument_id)
            .client_order_id(cid)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("3500.00"))
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument_id,
                cid,
                parent,
                account_id,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(1),
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Updated(OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                instrument_id,
                cid,
                Quantity::from("1.500"),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                Some(current),
                Some(account_id),
                Some(Price::from("3505.00")),
                None,
                None,
                false,
            )))
            .unwrap();
        client.dispatch_state.restore_order(&order);
        client.dispatch_state.forget(&cid);
        order
            .apply(OrderEventAny::Canceled(OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                instrument_id,
                cid,
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                Some(current),
                Some(account_id),
                None,
            )))
            .unwrap();
        cache
            .borrow_mut()
            .add_order(order, None, Some(client.core.client_id), false)
            .unwrap();

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        for index in 0..crate::websocket::dispatch::ORDER_DEDUP_CAPACITY {
            let other = ClientOrderId::from(format!("EVICT-{index}").as_str());
            client.dispatch_state.register_identity(other, identity);
            client.dispatch_state.record_venue_order_id(
                other,
                VenueOrderId::from(format!("eviction-{index}").as_str()),
            );
            client.dispatch_state.forget(&other);
        }

        let mut native: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        native.order_id = current.to_string();
        native.label = cid.inner();
        native.amount = dec!(1.2);
        native.filled_amount = dec!(0.2);
        native.average_price = dec!(3505);
        native.order_status = DeriveOrderStatus::Open;
        native.order_type = DeriveOrderType::Limit;
        native.time_in_force = crate::common::enums::DeriveTimeInForce::Gtc;
        native.replaced_order_id = None;
        let report = parse_derive_order_to_report_with_precision(
            &native,
            account_id,
            Some(2),
            Some(3),
            UnixNanos::from(4),
        )
        .unwrap();
        let result = stale.project_order_report(&native, report).await;
        let fresh = client.reconciliation_context();
        let binding = fresh.order_binding(cid).unwrap();
        let mut legs = binding.venue_order_legs.clone();
        legs.sort_unstable();
        assert_eq!(stale.order_binding(cid), None);
        assert_eq!(binding.identity, identity);
        assert_eq!(binding.venue_order_id, Some(current));
        assert_eq!(legs, vec![current, parent]);
        assert_eq!(
            result.unwrap_err().to_string(),
            "Derive order context expired after terminal binding eviction; retry with a fresh cache snapshot"
        );

        let worker_request = request.clone();
        let snapshot_task = tokio::spawn(async move { worker_request.snapshot().await });
        assert!(time_events.recv().await.unwrap().dispatch());
        let (snapshot, _) = snapshot_task.await.unwrap().unwrap();
        let refreshed = snapshot.order_binding(cid).unwrap();
        let mut refreshed_legs = refreshed.venue_order_legs.clone();
        refreshed_legs.sort_unstable();

        assert_eq!(refreshed.identity, identity);
        assert_eq!(refreshed.venue_order_id, Some(current));
        assert_eq!(refreshed_legs, vec![current, parent]);
        snapshot.ensure_order_context().unwrap();
        request.close();

        let empty_query = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::from(5),
            None,
            None,
            None,
            None,
            None,
        );
        assert!(
            fresh
                .generate_order_status_report(&empty_query)
                .await
                .unwrap()
                .is_none()
        );
    }

    #[tokio::test]
    async fn test_canceled_reconciliation_callback_never_reads_or_rearms_cache() {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from(DERIVE),
            *DERIVE_VENUE,
            OmsType::Netting,
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let client = DeriveExecutionClient::new(core, test_config()).unwrap();
        let (sender, _events) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::runner::set_time_event_sender(Arc::new(ReconciliationTimeSender(sender)));
        let request = ReconciliationSnapshotRequest::new(
            client.core.clone(),
            client.reconciliation_context(),
            client.cancellation_token.clone(),
            Duration::from_secs(1),
        )
        .unwrap();
        let pending = request.next.lock().take().unwrap();
        client.cancellation_token.cancel();
        request.close();
        let cache_borrow = cache.borrow_mut();
        let dispatched = pending.message.dispatch();
        drop(cache_borrow);

        assert!(dispatched);
        assert!(pending.result.await.is_err());
        assert!(request.next.lock().is_none());
        assert_eq!(Rc::strong_count(&cache), 2);
    }

    #[rstest]
    fn test_market_order_limit_price_buy_lifts_ask_and_rounds_up_to_tick() {
        let quote = QuoteTick::new(
            InstrumentId::from("ETH-PERP.DERIVE"),
            Price::from("3500.00"),
            Price::from("3501.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );
        // 50 bps; raw = 3501 * 1.005 = 3518.505; tick 0.01 rounds up to 3518.51.
        let price = market_order_limit_price(&quote, OrderSide::Buy, 50, dec!(0.01)).unwrap();
        assert_eq!(price, dec!(3518.51));
    }

    #[rstest]
    fn test_market_order_limit_price_sell_drops_bid_rounds_down_and_denies_non_positive() {
        let quote = QuoteTick::new(
            InstrumentId::from("ETH-PERP.DERIVE"),
            Price::from("3500.00"),
            Price::from("3501.00"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );
        // 50 bps; raw = 3500 * 0.995 = 3482.5; tick 0.01 stays at 3482.5.
        let price = market_order_limit_price(&quote, OrderSide::Sell, 50, dec!(0.01)).unwrap();
        assert_eq!(price, dec!(3482.5));

        // 20_000 bps = 200% slippage drives the rounded bound below zero; deny.
        let zero = market_order_limit_price(&quote, OrderSide::Sell, 20_000, dec!(0.01));
        assert!(zero.is_none());
    }

    #[rstest]
    fn test_trigger_market_limit_price_uses_trigger_price_bound() {
        let buy = trigger_market_limit_price(dec!(3600), OrderSide::Buy, 50, dec!(0.01)).unwrap();
        let sell = trigger_market_limit_price(dec!(3600), OrderSide::Sell, 50, dec!(0.01)).unwrap();
        let zero = trigger_market_limit_price(dec!(1), OrderSide::Sell, 20_000, dec!(0.01));

        assert_eq!(buy, dec!(3618));
        assert_eq!(sell, dec!(3582));
        assert!(zero.is_none());
    }

    #[rstest]
    fn test_normal_order_signature_expiry_accepts_ttl_above_minimum() {
        let clock = get_atomic_clock_realtime();
        let start_secs = (clock.get_time_ns().as_u64() / 1_000_000_000) as i64;
        let ttl_secs = MIN_SIGNATURE_TTL.as_secs() + 1;

        let expiry = normal_order_signature_expiry(clock, ttl_secs).expect("expiry is valid");

        assert!(expiry >= start_secs + ttl_secs as i64);
    }

    #[rstest]
    #[case(MIN_SIGNATURE_TTL.as_secs(), "must be greater than the Derive minimum")]
    #[case(MIN_SIGNATURE_TTL.as_secs() - 1, "must be greater than the Derive minimum")]
    fn test_normal_order_signature_expiry_rejects_minimum_or_lower_ttl(
        #[case] ttl_secs: u64,
        #[case] reason_fragment: &str,
    ) {
        let clock = get_atomic_clock_realtime();

        let err = normal_order_signature_expiry(clock, ttl_secs).expect_err("TTL is too short");

        assert!(
            err.to_string().contains(reason_fragment),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    #[case(i64::MAX as u64, "exceeds the Derive maximum")]
    #[case(u64::MAX, "exceeds the Derive maximum")]
    fn test_normal_order_signature_expiry_rejects_extreme_ttl(
        #[case] ttl_secs: u64,
        #[case] reason_fragment: &str,
    ) {
        let clock = get_atomic_clock_realtime();

        let err = normal_order_signature_expiry(clock, ttl_secs).expect_err("TTL is invalid");

        assert!(
            err.to_string().contains(reason_fragment),
            "unexpected error: {err}",
        );
    }

    #[rstest]
    #[case(None, "max_fee_per_contract is required")]
    #[case(Some(dec!(0)), "max_fee_per_contract must be greater than zero")]
    #[case(Some(dec!(-1)), "max_fee_per_contract must be greater than zero")]
    fn test_new_rejects_invalid_max_fee_per_contract(
        #[case] max_fee_per_contract: Option<Decimal>,
        #[case] expected: &str,
    ) {
        let mut config = test_config();
        config.max_fee_per_contract = max_fee_per_contract;

        let err = DeriveExecutionClient::new(test_core(), config).expect_err("must reject");

        assert_eq!(err.to_string(), expected);
    }

    #[rstest]
    #[case(OrderType::StopMarket, true)]
    #[case(OrderType::StopLimit, true)]
    #[case(OrderType::MarketIfTouched, true)]
    #[case(OrderType::LimitIfTouched, true)]
    #[case(OrderType::Market, false)]
    #[case(OrderType::Limit, false)]
    #[case(OrderType::MarketToLimit, false)]
    #[case(OrderType::TrailingStopMarket, false)]
    fn test_is_derive_trigger_order_type(#[case] order_type: OrderType, #[case] expected: bool) {
        assert_eq!(is_derive_trigger_order_type(order_type), expected);
    }

    #[rstest]
    fn test_resolve_submit_nonce_emits_rejection_and_forgets_identity() {
        let clock = get_atomic_clock_realtime();
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let strategy_id = StrategyId::from("S-1");
        let client_order_id = ClientOrderId::from("NONCE-SUBMIT-1");
        let order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(TraderId::from("TRADER-001"))
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.000"))
            .price(Price::from("3500.00"))
            .build();

        let identity = OrderIdentity {
            instrument_id,
            strategy_id,
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        let state = WsDispatchState::new(42);
        state.register_identity(client_order_id, identity);
        let (emitter, mut rx) = test_emitter(clock);

        let nonce = resolve_submit_nonce(
            Err(NonceError::ClockBeforeEpoch),
            &emitter,
            &state,
            &order,
            clock,
        );
        let event = rx.try_recv().expect("OrderRejected event");

        assert!(nonce.is_none());
        assert!(state.identity(&client_order_id).is_none());

        if let ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) = event {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(
                rejected.reason,
                "nonce allocation failed: system clock is before UNIX epoch",
            );
        } else {
            panic!("expected OrderRejected, event was {event:?}");
        }
    }

    #[rstest]
    fn test_resolve_modify_nonce_emits_modify_rejection() {
        let clock = get_atomic_clock_realtime();
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let strategy_id = StrategyId::from("S-1");
        let client_order_id = ClientOrderId::from("NONCE-MODIFY-1");
        let venue_order_id = VenueOrderId::from("ord-nonce-modify-1");
        let (emitter, mut rx) = test_emitter(clock);

        let nonce = resolve_modify_nonce(
            Err(NonceError::ClockBeforeEpoch),
            &emitter,
            strategy_id,
            instrument_id,
            client_order_id,
            venue_order_id,
            clock,
        );
        let event = rx.try_recv().expect("OrderModifyRejected event");

        assert!(nonce.is_none());

        if let ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) = event {
            assert_eq!(rejected.client_order_id, client_order_id);
            assert_eq!(rejected.venue_order_id, Some(venue_order_id));
            assert_eq!(
                rejected.reason,
                "nonce allocation failed: system clock is before UNIX epoch",
            );
        } else {
            panic!("expected OrderModifyRejected, event was {event:?}");
        }
    }

    #[rstest]
    #[case(dec!(0))]
    #[case(dec!(-1))]
    fn test_round_to_tick_treats_non_positive_tick_as_no_op(#[case] tick: Decimal) {
        // Non-positive tick must pass through both sides untouched so the
        // signing path does not divide by zero or amplify garbage tick data.
        assert_eq!(
            round_to_tick(dec!(3501.55), tick, OrderSide::Buy),
            dec!(3501.55)
        );
        assert_eq!(
            round_to_tick(dec!(3501.55), tick, OrderSide::Sell),
            dec!(3501.55)
        );
    }

    #[rstest]
    fn test_resolve_signing_context_rejects_placeholder_domain_separator() {
        // The shipped mainnet defaults are real Protocol Constants, so force
        // an explicit placeholder via the config override to verify the
        // placeholder-detection path still refuses to construct.
        let mut config = test_config();
        config.environment = DeriveEnvironment::Mainnet;
        config.domain_separator =
            Some("0x<paste_from_docs.derive.xyz_protocol_constants>".to_string());
        let err = DeriveExecutionClient::new(test_core(), config).expect_err("must reject");
        let msg = err.to_string();
        assert!(msg.contains("placeholder"), "unexpected error: {msg}",);
    }

    #[rstest]
    fn test_resolve_signing_context_uses_mainnet_defaults() {
        let mut config = test_config();
        config.environment = DeriveEnvironment::Mainnet;
        config.domain_separator = None;
        config.action_typehash = None;
        config.trade_module_address = None;

        DeriveExecutionClient::new(test_core(), config).expect("mainnet defaults should parse");
    }

    #[rstest]
    fn test_resolve_signing_context_uses_testnet_defaults() {
        let mut config = test_config();
        config.environment = DeriveEnvironment::Testnet;
        config.domain_separator = None;
        config.action_typehash = None;
        config.trade_module_address = None;

        DeriveExecutionClient::new(test_core(), config).expect("testnet defaults should parse");
    }

    #[rstest]
    fn test_market_order_limit_price_rounds_to_coarse_tick() {
        // Coarse tick = 1.0 (e.g. weekly option strikes); raw 3518.505 rounds
        // up to 3519, raw 3482.5 rounds down to 3482.
        let quote = QuoteTick::new(
            InstrumentId::from("ETH-20260627-3500-C.DERIVE"),
            Price::from("3500"),
            Price::from("3501"),
            Quantity::from("1.000"),
            Quantity::from("1.000"),
            UnixNanos::from(0),
            UnixNanos::from(0),
        );
        let buy = market_order_limit_price(&quote, OrderSide::Buy, 50, dec!(1)).unwrap();
        assert_eq!(buy, dec!(3519));
        let sell = market_order_limit_price(&quote, OrderSide::Sell, 50, dec!(1)).unwrap();
        assert_eq!(sell, dec!(3482));
    }

    #[rstest]
    fn test_new_populates_identity() {
        let core = test_core();
        let client = DeriveExecutionClient::new(core, test_config()).unwrap();

        assert_eq!(client.client_id(), ClientId::from(DERIVE));
        assert_eq!(client.account_id(), AccountId::from("DERIVE-001"));
        assert_eq!(client.venue(), *DERIVE_VENUE);
        assert_eq!(client.oms_type(), OmsType::Netting);
        assert_eq!(client.subaccount_id(), TEST_SUBACCOUNT);
        assert!(!client.is_connected());
    }

    #[rstest]
    fn test_cache_instrument_registers_report_precision() {
        let client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        let instrument = sample_derive_instrument();
        let instrument_id = format_instrument_id(instrument.instrument_name).unwrap();

        client.cache_instrument(instrument).unwrap();

        assert_eq!(
            client.dispatch_state.instrument_precision(&instrument_id),
            Some((2, 3)),
        );
    }

    #[rstest]
    fn test_native_instrument_retains_signing_metadata() {
        let client = test_client_with_instrument();
        let expected = sample_derive_instrument();
        let instrument_id = format_instrument_id(expected.instrument_name).unwrap();
        let actual = client.instruments.get_cloned(&instrument_id).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            client.dispatch_state.instrument_precision(&instrument_id),
            Some((2, 3))
        );
    }

    #[rstest]
    #[tokio::test]
    async fn test_bootstrap_restores_cached_instrument_context() {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from(DERIVE),
            *DERIVE_VENUE,
            OmsType::Netting,
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let client = DeriveExecutionClient::new(core, test_config()).unwrap();
        let expected = sample_derive_instrument();
        let instrument_id = format_instrument_id(expected.instrument_name).unwrap();
        let native = parse_derive_instrument_any(&expected, UnixNanos::default())
            .unwrap()
            .unwrap();
        cache.borrow_mut().add_instrument(native).unwrap();
        client.ensure_instruments_initialized().await.unwrap();
        let actual = client.instruments.get_cloned(&instrument_id).unwrap();
        assert_eq!(
            serde_json::to_value(actual).unwrap(),
            serde_json::to_value(expected).unwrap()
        );
        assert_eq!(
            client.dispatch_state.instrument_precision(&instrument_id),
            Some((2, 3))
        );
        assert!(client.core.instruments_initialized());
    }

    #[rstest]
    fn test_order_dispatch_uses_registered_instrument_precision() {
        let clock = get_atomic_clock_realtime();
        let client = test_client_with_instrument();

        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.subaccount_id = client.subaccount_id() as i64;
        order.amount = Decimal::from_str_exact("25.000").unwrap();
        order.filled_amount = Decimal::from_str_exact("5.000").unwrap();
        order.limit_price = Decimal::from_str_exact("25.000").unwrap();
        order.order_status = DeriveOrderStatus::Open;
        order.order_type = DeriveOrderType::Limit;

        let (emitter, mut rx) = test_emitter(clock);
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![order],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &client.dispatch_state,
        );

        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("Expected OrderStatusReport");
        };

        assert_eq!(report.price, Some(Price::from("25.00")));
        assert_eq!(report.price.unwrap().precision, 2);
        assert_eq!(report.quantity, Quantity::from("25.000"));
        assert_eq!(report.quantity.precision, 3);
        assert_eq!(report.filled_qty, Quantity::from("5.000"));
        assert_eq!(report.filled_qty.precision, 3);
    }

    #[rstest]
    fn test_trade_dispatch_uses_registered_instrument_precision() {
        let clock = get_atomic_clock_realtime();
        let client = test_client_with_instrument();
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.subaccount_id = client.subaccount_id() as i64;
        trade.trade_amount = Decimal::from_str_exact("25.000").unwrap();
        trade.trade_price = Decimal::from_str_exact("25.000").unwrap();

        let (emitter, mut rx) = test_emitter(clock);
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &client.dispatch_state,
        );

        let event = rx.try_recv().unwrap();

        let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event else {
            panic!("Expected FillReport");
        };

        assert_eq!(report.last_px, Price::from("25.00"));
        assert_eq!(report.last_px.precision, 2);
        assert_eq!(report.last_qty, Quantity::from("25.000"));
        assert_eq!(report.last_qty.precision, 3);
    }

    #[rstest]
    fn test_emit_tracked_event_suppresses_in_flight_replace_cancel_leg() {
        // Derive's `private/replace` cancels the old order; the `.orders`
        // cancel-of-old leg can arrive before `modify_order` rebinds the order,
        // i.e. while the replace is in flight. In that window only the
        // `pending_modify` marker (not the bound-id check) can suppress it. The
        // integration suite covers the post-rebind bound-id branch; this covers
        // the in-flight branch, which is otherwise unexercised end to end.
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("DERIVE-001");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let cid = ClientOrderId::from("STRAT-MOD-INFLIGHT");
        let stale_voi = VenueOrderId::from("ord-stale-1");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        // A `cancelled` report for the stale leg, identical across both cases:
        // only the dispatch-state marker differs.
        let report = OrderStatusReport::new(
            account_id,
            instrument_id,
            Some(cid),
            stale_voi,
            OrderSide::Buy.into(),
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Canceled,
            Quantity::from("1.000"),
            Quantity::from("0.000"),
            UnixNanos::from(1_000),
            UnixNanos::from(2_000),
            UnixNanos::from(3_000),
            None,
        );

        // Marker targets the cancel's venue order id and no bound id is
        // recorded, so suppression can only come from the in-flight branch.
        let (emitter, mut rx) = test_emitter(clock);
        let state = WsDispatchState::new(42);
        state.mark_pending_modify(cid, stale_voi);
        emit_tracked_order_event(
            &emitter,
            &state,
            cid,
            identity,
            &report,
            account_id,
            UnixNanos::from(0),
        );
        let suppressed = rx.try_recv().is_err();

        // A marker for a different venue order id must not suppress: the guard
        // keys on the specific id, so the cancel-of-old still terminates.
        let (emitter, mut rx) = test_emitter(clock);
        let state = WsDispatchState::new(42);
        state.mark_pending_modify(cid, VenueOrderId::from("ord-other"));
        emit_tracked_order_event(
            &emitter,
            &state,
            cid,
            identity,
            &report,
            account_id,
            UnixNanos::from(0),
        );
        let mut saw_canceled = false;

        while let Ok(event) = rx.try_recv() {
            if matches!(event, ExecutionEvent::Order(OrderEventAny::Canceled(_))) {
                saw_canceled = true;
            }
        }

        assert!(
            suppressed,
            "in-flight cancel-of-old leg must be suppressed by the pending-modify marker",
        );
        assert!(
            saw_canceled,
            "a pending-modify marker for a different venue order id must not suppress",
        );
    }

    #[rstest]
    fn test_ensure_canceled_emitted_is_idempotent() {
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("DERIVE-001");
        let client_order_id = ClientOrderId::from("TRIGGER-CANCEL-1");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::StopMarket,
        };

        let venue_order_id = VenueOrderId::from("trigger-cancel-1");
        let state = WsDispatchState::new(42);
        let (emitter, mut rx) = test_emitter(clock);

        for _ in 0..2 {
            ensure_canceled_emitted(
                &emitter,
                &state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                UnixNanos::from(1_000),
                UnixNanos::from(1_000),
            );
        }

        assert!(matches!(
            rx.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Canceled(_)))
        ));
        assert!(rx.try_recv().is_err(), "duplicate OrderCanceled emitted");
    }

    fn test_emitter(
        clock: &'static AtomicTime,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TRADER-001"),
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            Some(Currency::USDC()),
        );
        emitter.set_sender(tx);
        (emitter, rx)
    }

    #[rstest]
    #[case(DeriveOrderStatus::Cancelled)]
    #[case(DeriveOrderStatus::Expired)]
    #[case(DeriveOrderStatus::Rejected)]
    fn test_terminal_delivery_failure_preserves_identity(#[case] status: DeriveOrderStatus) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("TERMINAL-DELIVERY-1");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = client_order_id.as_str().into();
        order.order_status = status;
        let (emitter, rx) = test_emitter(clock);
        drop(rx);
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![order],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        assert_eq!(state.identity(&client_order_id), Some(identity));
        assert!(!state.contains_accepted(&client_order_id));
        assert!(!state.contains_canceled(&client_order_id));
    }

    #[rstest]
    #[case::stop(0)]
    #[case::reset(1)]
    #[case::dispose(2)]
    #[tokio::test]
    async fn test_lifecycle_shutdown_closes_owned_generations(#[case] operation: u8) {
        let mut client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        client.core.set_connected();
        client.is_connected.store(true, Ordering::Release);
        let session_spawner = client.session_tasks.spawner().unwrap();
        let pending_spawner = client.pending_tasks.spawner().unwrap();
        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();

        for group in [&client.session_tasks, &client.pending_tasks] {
            let cancellation = group.cancellation_token();
            let tx = tx.clone();
            group
                .spawn(async move {
                    tx.send(()).unwrap();
                    cancellation.cancelled().await;
                })
                .unwrap();
        }

        for _ in 0..2 {
            tokio::time::timeout(Duration::from_secs(1), rx.recv())
                .await
                .unwrap()
                .unwrap();
        }

        for _ in 0..2 {
            match operation {
                0 => client.stop().unwrap(),
                1 => client.reset().unwrap(),
                2 => client.dispose().unwrap(),
                _ => unreachable!(),
            }
        }

        let closed = (
            !client.session_tasks.is_open(),
            !client.pending_tasks.is_open(),
            client.cancellation_token.is_cancelled(),
            client.core.is_disconnected(),
            !client.is_connected(),
        );

        let stale_session_rejected = session_spawner.spawn(async {}).is_err();

        let stale_pending_rejected = pending_spawner.spawn(async {}).is_err();
        client.disconnect().await.unwrap();

        assert_eq!(closed, (true, true, true, true, true));
        assert!(stale_session_rejected);
        assert!(stale_pending_rejected);
        assert!(client.session_tasks.is_empty());
        assert!(client.pending_tasks.is_empty());
    }

    #[rstest]
    #[case(Some(ClientId::from(DERIVE)), "DERIVE-001", false, true)]
    #[case(Some(ClientId::from("OTHER")), "DERIVE-001", false, false)]
    #[case(None, "DERIVE-001", false, false)]
    #[case(Some(ClientId::from(DERIVE)), "DERIVE-OTHER", false, false)]
    #[case(Some(ClientId::from(DERIVE)), "DERIVE-001", true, false)]
    fn test_start_restores_only_owned_active_orders(
        #[case] assigned_client: Option<ClientId>,
        #[case] account: &str,
        #[case] closed: bool,
        #[case] restored: bool,
        #[values(false, true)] voided: bool,
    ) {
        use nautilus_common::live::runner::replace_exec_event_sender;

        let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(tx);
        let cache = Rc::new(RefCell::new(Cache::default()));

        let core = ExecutionClientCore::new(
            TraderId::from("TRADER-001"),
            ClientId::from(DERIVE),
            *DERIVE_VENUE,
            OmsType::Netting,
            AccountId::from("DERIVE-001"),
            AccountType::Margin,
            None,
            cache.clone(),
        );
        let cid = ClientOrderId::from("RESTORED-1");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let account_id = AccountId::from(account);
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(core.trader_id)
            .strategy_id(StrategyId::from("RESTORED-STRATEGY"))
            .instrument_id(instrument_id)
            .client_order_id(cid)
            .side(OrderSide::Buy)
            .quantity(Quantity::from("1.500"))
            .price(Price::from("3499.00"))
            .build();
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.subaccount_id = TEST_SUBACCOUNT as i64;
        trade.label = cid.as_str().into();

        trade.trade_amount = if closed { dec!(1.5) } else { dec!(0.3) };
        let report = parse_derive_trade_to_fill_report_with_precision(
            &trade,
            account_id,
            Currency::USDC(),
            Some(2),
            Some(3),
            UnixNanos::from(3),
        )
        .unwrap()
        .unwrap();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument_id,
                cid,
                report.venue_order_id,
                account_id,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Filled(OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                instrument_id,
                cid,
                report.venue_order_id,
                account_id,
                report.trade_id,
                order.order_side(),
                order.order_type(),
                report.last_qty,
                report.last_px,
                Currency::USDC(),
                report.liquidity_side,
                UUID4::new(),
                report.ts_event,
                report.ts_init,
                false,
                None,
                Some(report.commission),
                None,
            )))
            .unwrap();

        if voided {
            order
                .apply(OrderEventAny::FillVoided(OrderFillVoided::new(
                    order.trader_id(),
                    order.strategy_id(),
                    instrument_id,
                    cid,
                    report.venue_order_id,
                    account_id,
                    Ustr::from("restore-correction"),
                    report.trade_id,
                    report.last_qty,
                    Some(report.commission),
                    order.order_side(),
                    order.order_type(),
                    report.last_px,
                    Currency::USDC(),
                    report.liquidity_side,
                    None,
                    None,
                    None,
                    UUID4::new(),
                    UnixNanos::from(4),
                    UnixNanos::from(5),
                    false,
                    false,
                )))
                .unwrap();
            assert!(order.trade_ids().is_empty());
        }

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: order.strategy_id(),
            order_side: order.order_side(),
            order_type: order.order_type(),
        };

        cache
            .borrow_mut()
            .add_order(order, None, assigned_client, false)
            .unwrap();
        let mut client = DeriveExecutionClient::new(core, test_config()).unwrap();
        client.start().unwrap();

        assert_eq!(
            client.dispatch_state.identity(&cid),
            restored.then_some(identity)
        );
        assert_eq!(
            client.dispatch_state.bound_venue_order_id(&cid),
            restored.then_some(report.venue_order_id)
        );
        assert_eq!(client.dispatch_state.contains_accepted(&cid), restored);
        assert_eq!(
            client.dispatch_state.contains_trade(&report.trade_id),
            restored
        );
        assert!(rx.try_recv().is_err());

        if restored {
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade.clone()],
                },
                &client.emitter,
                account_id,
                client.clock,
                &client.dispatch_state,
            );

            assert!(rx.try_recv().is_err());
            let current_venue_order_id = VenueOrderId::from("restored-current-child");
            client
                .dispatch_state
                .record_venue_order_id(cid, current_venue_order_id);
            client.start().unwrap();
            assert_eq!(
                client.dispatch_state.bound_venue_order_id(&cid),
                Some(current_venue_order_id)
            );
            trade.order_id = current_venue_order_id.as_str().into();
            trade.trade_id = "restored-final-fill".into();
            trade.trade_amount = dec!(1.2);
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade],
                },
                &client.emitter,
                account_id,
                client.clock,
                &client.dispatch_state,
            );

            let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
                panic!("Expected only the unapplied fill");
            };

            assert_eq!(fill.client_order_id, cid);
            assert_eq!(fill.instrument_id, instrument_id);
            assert_eq!(fill.strategy_id, identity.strategy_id);
            assert_eq!(fill.venue_order_id, current_venue_order_id);
            assert_eq!(fill.last_qty, Quantity::from("1.200"));
            assert_eq!(fill.last_px, Price::from("3499.00"));
            assert!(rx.try_recv().is_err());
            assert_eq!(client.dispatch_state.identity(&cid), None);
            assert!(client.dispatch_state.is_terminal(&cid, instrument_id));
        }
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn test_authoritative_sender_installed_before_start_and_reinstalled(#[case] restart: bool) {
        use nautilus_common::live::runner::replace_exec_event_sender;

        let (first_tx, mut first_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(first_tx);
        let mut client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        let (second_tx, mut second_rx) = tokio::sync::mpsc::unbounded_channel();

        if restart {
            client.start().unwrap();
            replace_exec_event_sender(second_tx);
            client.start().unwrap();
        }

        let trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        let report = parse_derive_trade_to_fill_report_with_precision(
            &trade,
            client.account_id(),
            Currency::USDC(),
            None,
            None,
            UnixNanos::from(23),
        )
        .unwrap()
        .unwrap();
        client
            .emitter
            .try_send_execution_report(ExecutionReport::Fill(Box::new(report.clone())))
            .unwrap();

        let event = if restart {
            second_rx.try_recv().unwrap()
        } else {
            first_rx.try_recv().unwrap()
        };

        let ExecutionEvent::Report(ExecutionReport::Fill(actual)) = event else {
            panic!("authoritative sender must receive the report");
        };

        assert_eq!(*actual, report);
        assert!(first_rx.try_recv().is_err());
        assert!(second_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_acceptance_delivery_failure_preserves_replay() {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("DELIVERY-1");
        let venue_order_id = VenueOrderId::from("venue-delivery-1");
        let account_id = AccountId::from("DERIVE-001");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        let (emitter, rx) = test_emitter(clock);
        drop(rx);
        ensure_accepted_emitted(
            &emitter,
            &state,
            client_order_id,
            identity,
            venue_order_id,
            account_id,
            UnixNanos::from(11),
            UnixNanos::from(17),
        );
        assert!(!state.contains_accepted(&client_order_id));
        let (emitter, mut rx) = test_emitter(clock);
        ensure_accepted_emitted(
            &emitter,
            &state,
            client_order_id,
            identity,
            venue_order_id,
            account_id,
            UnixNanos::from(11),
            UnixNanos::from(17),
        );

        let ExecutionEvent::Order(OrderEventAny::Accepted(event)) = rx.try_recv().unwrap() else {
            panic!("acceptance must remain deliverable");
        };

        assert_eq!(event.trader_id, TraderId::from("TRADER-001"));
        assert_eq!(event.strategy_id, identity.strategy_id);
        assert_eq!(event.instrument_id, identity.instrument_id);
        assert_eq!(event.client_order_id, client_order_id);
        assert_eq!(event.venue_order_id, venue_order_id);
        assert_eq!(event.account_id, account_id);
        assert_eq!(event.ts_event, UnixNanos::from(11));
        assert_eq!(event.ts_init, UnixNanos::from(17));
        assert!(!event.reconciliation);
        assert!(state.contains_accepted(&client_order_id));
        assert_eq!(state.identity(&client_order_id), Some(identity));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("valid")]
    #[case("foreign_account")]
    #[case("foreign_instrument")]
    #[case("wrong_side")]
    #[case("unknown_child")]
    #[case("old_leg")]
    #[case("terminal")]
    fn test_trigger_activation_requires_current_native_ownership(
        #[case] scope: &str,
        #[values(false, true)] fill: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("ACTIVATION-SCOPE");
        let native_id = VenueOrderId::from("activation-current");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::StopLimit,
        };

        state.register_identity(cid, identity);
        state.record_venue_order_id(cid, native_id);
        state.record_order_shape(cid, Quantity::from("2.000"), None);
        if scope == "old_leg" {
            state.record_venue_leg(cid, VenueOrderId::from("activation-old"));
        }

        if scope == "terminal" {
            state.forget(&cid);
        }

        let (emitter, rx) = test_emitter(clock);
        drop(rx);

        if fill {
            let mut trade: DeriveTrade = serde_json::from_str(include_str!(
                "../test_data/perps/http_private_trade_eth.json"
            ))
            .unwrap();
            trade.label = cid.to_string().into();
            trade.order_id = native_id.to_string();
            match scope {
                "foreign_account" => trade.subaccount_id = 43,
                "foreign_instrument" => trade.instrument_name = "BTC-PERP".into(),
                "wrong_side" => trade.direction = DeriveOrderSide::Sell,
                "unknown_child" => trade.order_id = "activation-unknown".into(),
                "old_leg" => trade.order_id = "activation-old".into(),
                _ => {}
            }

            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade],
                },
                &emitter,
                AccountId::from("DERIVE-001"),
                clock,
                &state,
            );
        } else {
            let mut order: DeriveOrder = serde_json::from_str(include_str!(
                "../test_data/perps/http_order_eth_partially_filled.json"
            ))
            .unwrap();
            order.label = cid.to_string().into();
            order.order_id = native_id.to_string();
            order.order_type = crate::common::enums::DeriveOrderType::Limit;
            order.order_status = DeriveOrderStatus::Open;
            order.trigger_type = Some(crate::common::enums::DeriveTriggerType::Stoploss);
            match scope {
                "foreign_account" => order.subaccount_id = 43,
                "foreign_instrument" => order.instrument_name = "BTC-PERP".into(),
                "wrong_side" => order.direction = DeriveOrderSide::Sell,
                "unknown_child" => order.order_id = "activation-unknown".into(),
                "old_leg" => order.order_id = "activation-old".into(),
                _ => {}
            }

            dispatch_orders_payload(
                DeriveOrdersSubscriptionData {
                    orders: vec![order],
                },
                &emitter,
                AccountId::from("DERIVE-001"),
                clock,
                &state,
            );
        }

        assert_eq!(state.trigger_active(&cid), scope == "valid");
        assert_eq!(
            state.bound_venue_order_id(&cid),
            (scope != "terminal").then_some(native_id)
        );
        state.forget(&cid);
        assert!(!state.trigger_active(&cid));
    }

    #[rstest]
    fn test_completed_tracked_fill_retires_identity_and_preserves_late_fill_report() {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("FILL-RETIRE-1");
        let account_id = AccountId::from("DERIVE-001");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        state.record_order_shape(client_order_id, Quantity::from("0.500"), None);
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = client_order_id.as_str().into();
        let (emitter, mut rx) = test_emitter(clock);
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade.clone()],
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        assert!(matches!(
            rx.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Accepted(_)))
        ));
        assert!(matches!(
            rx.try_recv(),
            Ok(ExecutionEvent::Order(OrderEventAny::Filled(_)))
        ));
        assert_eq!(state.identity(&client_order_id), None);
        assert!(state.is_terminal(&client_order_id, identity.instrument_id));
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade.clone()],
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        assert!(rx.try_recv().is_err());
        trade.trade_id = "late-fill-2".to_string();
        trade.trade_amount = dec!(0.2);
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade],
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        let ExecutionEvent::Report(ExecutionReport::Fill(report)) = rx.try_recv().unwrap() else {
            panic!("a distinct late fill must reach reconciliation");
        };

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, identity.instrument_id);
        assert_eq!(report.client_order_id, Some(client_order_id));
        assert_eq!(report.venue_order_id, VenueOrderId::from("order-abc"));
        assert_eq!(report.trade_id.as_str(), "late-fill-2");
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty.as_decimal(), dec!(0.2));
        assert_eq!(report.last_px.as_decimal(), dec!(3499));
        assert_eq!(
            report.commission,
            Money::from_decimal(dec!(0.02), Currency::USDC()).unwrap()
        );
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(report.avg_px, None);
        assert_eq!(report.venue_position_id, None);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case(false, false)]
    #[case(false, true)]
    #[case(true, false)]
    #[case(true, true)]
    fn test_foreign_instrument_label_preserves_external_identity(
        #[case] terminal: bool,
        #[case] fill: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("FOREIGN-LABEL-1");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(cid, identity);
        if terminal {
            state.forget(&cid);
        }

        let (emitter, mut rx) = test_emitter(clock);
        let account_id = AccountId::from("DERIVE-001");
        let instrument_id = InstrumentId::from("BTC-PERP.DERIVE");

        if fill {
            let mut trade: DeriveTrade = serde_json::from_str(include_str!(
                "../test_data/perps/http_private_trade_eth.json"
            ))
            .unwrap();
            trade.label = cid.as_str().into();
            trade.instrument_name = "BTC-PERP".into();
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade],
                },
                &emitter,
                account_id,
                clock,
                &state,
            );

            let ExecutionEvent::Report(ExecutionReport::Fill(report)) = rx.try_recv().unwrap()
            else {
                panic!("Expected external fill report");
            };

            assert_eq!(report.client_order_id, None);
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.account_id, account_id);
            assert_eq!(report.venue_order_id, VenueOrderId::from("order-abc"));
            assert_eq!(report.trade_id.as_str(), "trade-xyz");
            assert_eq!(report.last_qty.as_decimal(), dec!(0.5));
            assert_eq!(report.last_px.as_decimal(), dec!(3499));
            assert_eq!(report.commission.as_decimal(), dec!(0.02));
        } else {
            let mut order: DeriveOrder = serde_json::from_str(include_str!(
                "../test_data/perps/http_order_eth_partially_filled.json"
            ))
            .unwrap();
            order.label = cid.as_str().into();
            order.instrument_name = "BTC-PERP".into();
            let venue_order_id = VenueOrderId::from(order.order_id.as_str());
            dispatch_orders_payload(
                DeriveOrdersSubscriptionData {
                    orders: vec![order],
                },
                &emitter,
                account_id,
                clock,
                &state,
            );

            let ExecutionEvent::Report(ExecutionReport::Order(report)) = rx.try_recv().unwrap()
            else {
                panic!("Expected external order report");
            };

            assert_eq!(report.client_order_id, None);
            assert_eq!(report.instrument_id, instrument_id);
            assert_eq!(report.account_id, account_id);
            assert_eq!(report.venue_order_id, venue_order_id);
        }

        assert_eq!(state.identity(&cid), (!terminal).then_some(identity));
        assert_eq!(state.is_terminal(&cid, identity.instrument_id), terminal);
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_order_result_retired_identity_preserves_distinct_trades() {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let account_id = AccountId::from("DERIVE-001");
        let client_order_id = ClientOrderId::from("STRAT-RETIRED-RPC");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        state.register_identity(
            client_order_id,
            OrderIdentity {
                instrument_id,
                strategy_id: StrategyId::from("S-1"),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );

        state.forget(&client_order_id);
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = client_order_id.as_str().into();
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = client_order_id.as_str().into();
        trade.trade_id = "rpc-distinct-late-fill".to_string();
        trade.trade_amount = dec!(0.23);

        let result = DeriveOrderResult {
            order,
            trades: vec![trade],
        };

        let (emitter, mut rx) = test_emitter(clock);

        dispatch_order_result(
            result.clone(),
            client_order_id,
            &emitter,
            account_id,
            clock,
            &state,
        );
        dispatch_order_result(result, client_order_id, &emitter, account_id, clock, &state);

        let ExecutionEvent::Report(ExecutionReport::Fill(report)) = rx.try_recv().unwrap() else {
            panic!("a distinct late RPC fill must reach reconciliation");
        };

        assert_eq!(report.account_id, account_id);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.client_order_id, Some(client_order_id));
        assert_eq!(report.venue_order_id, VenueOrderId::from("order-abc"));
        assert_eq!(report.trade_id.as_str(), "rpc-distinct-late-fill");
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.last_qty.as_decimal(), dec!(0.23));
        assert_eq!(report.last_px.as_decimal(), dec!(3499));
        assert_eq!(
            report.commission,
            Money::from_decimal(dec!(0.02), Currency::USDC()).unwrap()
        );
        assert_eq!(report.liquidity_side, LiquiditySide::Maker);
        assert_eq!(report.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
        assert_eq!(report.avg_px, None);
        assert_eq!(report.venue_position_id, None);
        assert_eq!(state.identity(&client_order_id), None);
        assert!(state.is_terminal(&client_order_id, instrument_id));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("orders")]
    #[case("trades")]
    #[case("rpc")]
    fn test_private_rows_require_native_account_identity(#[case] source: &str) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let account_id = AccountId::from("DERIVE-001");
        let client_order_id = ClientOrderId::from("STRAT-O-1");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        let order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        let trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        let trade_id = TradeId::new(&trade.trade_id);
        let mut foreign_order = order.clone();
        foreign_order.subaccount_id = 43;
        let mut foreign_trade = trade.clone();
        foreign_trade.subaccount_id = 43;
        let (emitter, mut rx) = test_emitter(clock);

        match source {
            "orders" => dispatch_orders_payload(
                DeriveOrdersSubscriptionData {
                    orders: vec![foreign_order],
                },
                &emitter,
                account_id,
                clock,
                &state,
            ),
            "trades" => dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![foreign_trade],
                },
                &emitter,
                account_id,
                clock,
                &state,
            ),
            "rpc" => dispatch_order_result(
                DeriveOrderResult {
                    order: foreign_order,
                    trades: vec![foreign_trade],
                },
                client_order_id,
                &emitter,
                account_id,
                clock,
                &state,
            ),
            _ => unreachable!(),
        }

        assert!(
            rx.try_recv().is_err(),
            "foreign-account evidence must emit nothing"
        );
        assert_eq!(state.bound_venue_order_id(&client_order_id), None);
        assert_eq!(state.identity(&client_order_id), Some(identity));
        assert!(!state.contains_trade(&trade_id));

        match source {
            "orders" => dispatch_orders_payload(
                DeriveOrdersSubscriptionData {
                    orders: vec![order],
                },
                &emitter,
                account_id,
                clock,
                &state,
            ),
            "trades" => dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade],
                },
                &emitter,
                account_id,
                clock,
                &state,
            ),
            "rpc" => dispatch_order_result(
                DeriveOrderResult {
                    order,
                    trades: vec![trade],
                },
                client_order_id,
                &emitter,
                account_id,
                clock,
                &state,
            ),
            _ => unreachable!(),
        }

        assert!(rx.try_recv().is_ok(), "matching native evidence must emit");
    }

    #[rstest]
    #[case("orders")]
    #[case("trades")]
    fn test_private_channel_requires_native_account_identity(#[case] channel: &str) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);

        let data = if channel == "orders" {
            serde_json::json!([serde_json::from_str::<serde_json::Value>(include_str!(
                "../test_data/perps/http_order_eth_partially_filled.json"
            ))
            .unwrap()])
        } else {
            serde_json::json!([serde_json::from_str::<serde_json::Value>(include_str!(
                "../test_data/perps/http_private_trade_eth.json"
            ))
            .unwrap()])
        };

        let payload = WsSubscriptionPayload {
            channel: format!("43.{channel}").into(),
            data: serde_json::value::to_raw_value(&data).unwrap(),
        };

        let (emitter, mut rx) = test_emitter(clock);
        handle_ws_message(
            DeriveWsMessage::Subscription(payload.clone()),
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );
        assert!(
            rx.try_recv().is_err(),
            "foreign channel must emit nothing even with own rows"
        );

        let payload = WsSubscriptionPayload {
            channel: format!("42.{channel}").into(),
            ..payload
        };

        handle_ws_message(
            DeriveWsMessage::Subscription(payload),
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );
        assert!(rx.try_recv().is_ok(), "matching channel must emit");
    }

    #[rstest]
    #[case(None)]
    #[case(Some("unrelated-leg"))]
    #[case(Some("original-leg"))]
    fn test_replacement_order_requires_native_link_before_binding(
        #[case] replaced_id: Option<&str>,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("REPLACE-AUTHORITY");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let old_id = VenueOrderId::from("original-leg");
        let new_id = VenueOrderId::from("replacement-leg");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(cid, identity);
        state.record_venue_order_id(cid, old_id);
        state.mark_accepted(cid);
        state.mark_pending_modify(cid, old_id);
        state.record_modify_target(cid, Quantity::from("2.000"), Some(Price::from("3505.00")));
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = cid.as_str().into();
        order.order_id = new_id.to_string();
        order.order_status = DeriveOrderStatus::Open;
        order.replaced_order_id = replaced_id.map(str::to_owned);
        state.record_modify_nonce(cid, order.nonce);
        let (emitter, mut rx) = test_emitter(clock);
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![order],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        if replaced_id == Some(old_id.as_str()) {
            let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = rx.try_recv().unwrap()
            else {
                panic!("expected authoritative update");
            };

            assert_eq!(updated.client_order_id, cid);
            assert_eq!(updated.instrument_id, instrument_id);
            assert_eq!(updated.venue_order_id, Some(new_id));
            assert_eq!(updated.quantity, Quantity::from("2.000"));
            assert_eq!(updated.price, Some(Price::from("3505.00")));
            assert_eq!(state.bound_venue_order_id(&cid), Some(new_id));
        } else {
            assert!(
                rx.try_recv().is_err(),
                "a label without a matching replaced_order_id is not replacement authority"
            );
            assert_eq!(state.bound_venue_order_id(&cid), Some(old_id));
            assert_eq!(state.pending_modify(&cid), Some(old_id));
            assert_eq!(
                state.modify_target(&cid),
                Some((Quantity::from("2.000"), Some(Price::from("3505.00"))))
            );
        }

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case(None)]
    #[case(Some(101))]
    #[case(Some(202))]
    fn test_replacement_requires_prepared_request_nonce(#[case] prepared_nonce: Option<u64>) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("REPLACE-NOT-PREPARED");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let old_id = VenueOrderId::from("original-leg");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(cid, identity);
        state.record_venue_order_id(cid, old_id);
        state.mark_accepted(cid);
        state.mark_pending_modify(cid, old_id);
        state.record_modify_target(cid, Quantity::from("2.000"), Some(Price::from("3505.00")));
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = cid.as_str().into();
        order.order_id = "prior-replacement-leg".to_string();
        order.nonce = 202;

        if let Some(nonce) = prepared_nonce {
            state.record_modify_nonce(cid, nonce);
        }

        order.order_status = DeriveOrderStatus::Open;
        order.replaced_order_id = Some(old_id.to_string());
        let (emitter, mut rx) = test_emitter(clock);
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![order],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        if prepared_nonce == Some(202) {
            let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = rx.try_recv().unwrap()
            else {
                panic!("expected matching command update");
            };

            assert_eq!(updated.client_order_id, cid);
            assert_eq!(updated.instrument_id, instrument_id);
            assert_eq!(updated.quantity, Quantity::from("2.000"));
            assert_eq!(updated.price, Some(Price::from("3505.00")));
            assert_eq!(
                updated.venue_order_id,
                Some(VenueOrderId::from("prior-replacement-leg"))
            );
            assert_eq!(state.bound_venue_order_id(&cid), updated.venue_order_id);
            assert_eq!(state.pending_modify(&cid), None);
            assert_eq!(state.modify_target(&cid), None);
            assert_eq!(state.modify_nonce(&cid), None);
        } else {
            assert!(
                rx.try_recv().is_err(),
                "a linked child cannot claim an unprepared or different request"
            );
            assert_eq!(state.bound_venue_order_id(&cid), Some(old_id));
            assert_eq!(state.pending_modify(&cid), Some(old_id));
            assert_eq!(
                state.modify_target(&cid),
                Some((Quantity::from("2.000"), Some(Price::from("3505.00"))))
            );
            assert_eq!(state.modify_nonce(&cid), prepared_nonce);
        }

        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_replacement_trade_defers_until_native_order_authority() {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let cid = ClientOrderId::from("REPLACE-DEFERRED-FILL");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let old_id = VenueOrderId::from("original-leg");
        let new_id = VenueOrderId::from("replacement-leg");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(cid, identity);
        state.record_venue_order_id(cid, old_id);
        state.mark_accepted(cid);
        state.record_order_shape(cid, Quantity::from("2.000"), None);
        state.mark_pending_modify(cid, old_id);
        state.record_modify_target(cid, Quantity::from("2.000"), Some(Price::from("3505.00")));
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = cid.as_str().into();
        trade.order_id = new_id.to_string();
        trade.trade_id = "deferred-fill".to_owned();
        trade.trade_amount = dec!(0.3);
        let (emitter, mut rx) = test_emitter(clock);
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        assert!(
            rx.try_recv().is_err(),
            "trade label cannot authorize Updated or Filled"
        );
        assert_eq!(state.bound_venue_order_id(&cid), Some(old_id));
        assert_eq!(state.pending_modify(&cid), Some(old_id));
        assert!(!state.contains_trade(&TradeId::from("deferred-fill")));
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = cid.as_str().into();
        order.order_id = new_id.to_string();
        order.order_status = DeriveOrderStatus::Open;
        order.replaced_order_id = Some(old_id.to_string());
        state.record_modify_nonce(cid, order.nonce);
        dispatch_orders_payload(
            DeriveOrdersSubscriptionData {
                orders: vec![order],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = rx.try_recv().unwrap() else {
            panic!("expected update before fill");
        };

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("expected deferred fill");
        };

        assert_eq!(updated.venue_order_id, Some(new_id));
        assert_eq!(fill.client_order_id, cid);
        assert_eq!(fill.venue_order_id, new_id);
        assert_eq!(fill.trade_id, TradeId::from("deferred-fill"));
        assert_eq!(fill.last_qty, Quantity::from("0.300000"));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    #[case("rpc")]
    #[case("stream")]
    fn test_undelivered_trade_survives_authoritative_sender_reinstallation(#[case] source: &str) {
        use nautilus_common::live::runner::replace_exec_event_sender;
        let (first_tx, first_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(first_tx);
        let mut client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        drop(first_rx);
        let cid = ClientOrderId::from("UNDELIVERED-FINANCIAL");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let venue_order_id = VenueOrderId::from("owned-financial-leg");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        client.dispatch_state.register_identity(cid, identity);
        client
            .dispatch_state
            .register_instrument_precision(instrument_id, 2, 3);
        client
            .dispatch_state
            .record_order_shape(cid, Quantity::from("2.000"), None);
        client
            .dispatch_state
            .record_venue_order_id(cid, venue_order_id);
        client.dispatch_state.mark_accepted(cid);
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.subaccount_id = client.subaccount_id() as i64;
        trade.label = cid.as_str().into();
        trade.order_id = venue_order_id.to_string();
        trade.trade_id = "undelivered-financial".to_owned();

        if source == "rpc" {
            let mut order: DeriveOrder = serde_json::from_str(include_str!(
                "../test_data/perps/http_order_eth_partially_filled.json"
            ))
            .unwrap();
            order.subaccount_id = client.subaccount_id() as i64;
            order.label = cid.as_str().into();
            order.order_id = venue_order_id.to_string();
            order.order_status = DeriveOrderStatus::Open;
            dispatch_order_result(
                DeriveOrderResult {
                    order,
                    trades: vec![trade],
                },
                cid,
                &client.emitter,
                client.account_id(),
                client.clock,
                &client.dispatch_state,
            );
        } else {
            dispatch_trades_payload(
                DeriveTradesSubscriptionData {
                    trades: vec![trade],
                },
                &client.emitter,
                client.account_id(),
                client.clock,
                &client.dispatch_state,
            );
        }

        let (second_tx, mut rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(second_tx);
        client.start().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx
            .try_recv()
            .expect("undelivered raw trade must survive without venue replay")
        else {
            panic!("expected retained fill");
        };

        assert_eq!(fill.client_order_id, cid);
        assert_eq!(fill.instrument_id, instrument_id);
        assert_eq!(fill.strategy_id, identity.strategy_id);
        assert_eq!(fill.venue_order_id, venue_order_id);
        assert_eq!(fill.trade_id, TradeId::from("undelivered-financial"));
        assert_eq!(fill.last_qty, Quantity::from("0.500"));
        assert_eq!(fill.last_px, Price::from("3499.00"));
        assert_eq!(fill.commission, Some(Money::from("0.02 USDC")));
        client.start().unwrap();
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_first_native_partial_fill_binds_before_delivery(
        #[values(false, true)] failed_delivery: bool,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("NATIVE-FIRST-FILL");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        state.record_order_shape(
            client_order_id,
            Quantity::from("1.0"),
            Some(Price::from("3500")),
        );
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = client_order_id.to_string().into();
        let venue_order_id = VenueOrderId::from(trade.order_id.as_str());
        let trade_id = TradeId::from(trade.trade_id.as_str());
        let account_id = AccountId::from("DERIVE-001");
        let (emitter, rx) = test_emitter(clock);
        let mut rx = if failed_delivery {
            drop(rx);
            None
        } else {
            Some(rx)
        };

        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade],
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        assert_eq!(
            state.bound_venue_order_id(&client_order_id),
            Some(venue_order_id)
        );
        assert_eq!(state.identity(&client_order_id), Some(identity));
        assert_eq!(state.contains_trade(&trade_id), !failed_delivery);
        assert_eq!(state.contains_accepted(&client_order_id), !failed_delivery);

        if let Some(rx) = rx.as_mut() {
            let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = rx.try_recv().unwrap()
            else {
                panic!("Expected acceptance before fill");
            };

            let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
                panic!("Expected native partial fill");
            };

            assert_eq!(accepted.trader_id, TraderId::from("TRADER-001"));
            assert_eq!(accepted.strategy_id, identity.strategy_id);
            assert_eq!(accepted.instrument_id, identity.instrument_id);
            assert_eq!(accepted.client_order_id, client_order_id);
            assert_eq!(accepted.venue_order_id, venue_order_id);
            assert_eq!(accepted.account_id, account_id);
            assert_eq!(fill.trader_id, TraderId::from("TRADER-001"));
            assert_eq!(fill.strategy_id, identity.strategy_id);
            assert_eq!(fill.instrument_id, identity.instrument_id);
            assert_eq!(fill.client_order_id, client_order_id);
            assert_eq!(fill.venue_order_id, venue_order_id);
            assert_eq!(fill.account_id, account_id);
            assert_eq!(fill.trade_id, trade_id);
            assert_eq!(fill.order_side, OrderSide::Buy);
            assert_eq!(fill.order_type, OrderType::Limit);
            assert_eq!(fill.last_qty.as_decimal(), dec!(0.5));
            assert_eq!(fill.last_px.as_decimal(), dec!(3499));
            assert_eq!(fill.currency, Currency::USDC());
            assert_eq!(
                fill.commission,
                Some(Money::from_decimal(dec!(0.02), Currency::USDC()).unwrap())
            );
            assert_eq!(fill.liquidity_side, LiquiditySide::Maker);
            assert_eq!(fill.ts_event, UnixNanos::from(1_700_000_000_000_000_000));
            assert_eq!(fill.position_id, None);
            assert!(rx.try_recv().is_err());
        }
    }

    #[rstest]
    fn test_native_fill_side_conflict_preserves_authority(
        #[values(false, true)] failed_delivery: bool,
        #[values(false, true)] bound: bool,
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("NATIVE-SIDE-CONFLICT");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: side,
            order_type: OrderType::Limit,
        };

        let venue_order_id = VenueOrderId::from("native-owned");
        state.register_identity(client_order_id, identity);
        if bound {
            state.record_venue_order_id(client_order_id, venue_order_id);
        }

        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = client_order_id.to_string().into();
        trade.order_id = "native-contradictory".into();

        trade.direction = match side {
            OrderSide::Buy => DeriveOrderSide::Sell,
            OrderSide::Sell => DeriveOrderSide::Buy,
        };

        let trade_id = TradeId::from(trade.trade_id.as_str());
        let (emitter, rx) = test_emitter(clock);
        let mut rx = if failed_delivery {
            drop(rx);
            None
        } else {
            Some(rx)
        };

        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade.clone()],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        assert_eq!(
            state.bound_venue_order_id(&client_order_id),
            bound.then_some(venue_order_id)
        );
        assert_eq!(state.identity(&client_order_id), Some(identity));
        assert!(!state.knows_venue_leg(
            &client_order_id,
            VenueOrderId::from(trade.order_id.as_str())
        ));
        assert!(!state.contains_trade(&trade_id));
        assert!(!state.contains_accepted(&client_order_id));
        assert_eq!(
            serde_json::to_value(state.take_undelivered_trades()).unwrap(),
            serde_json::to_value(vec![trade]).unwrap()
        );

        if let Some(rx) = rx.as_mut() {
            assert!(rx.try_recv().is_err());
        }
    }

    #[rstest]
    fn test_native_fill_side_conflict_stays_deferred_after_terminal(
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    ) {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let client_order_id = ClientOrderId::from("NATIVE-TERMINAL-SIDE-CONFLICT");

        let identity = OrderIdentity {
            instrument_id: InstrumentId::from("ETH-PERP.DERIVE"),
            strategy_id: StrategyId::from("S-1"),
            order_side: side,
            order_type: OrderType::Limit,
        };

        state.register_identity(client_order_id, identity);
        state.record_order_shape(client_order_id, Quantity::from("0.5"), None);
        let mut trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        trade.label = client_order_id.to_string().into();

        trade.direction = match side {
            OrderSide::Buy => DeriveOrderSide::Sell,
            OrderSide::Sell => DeriveOrderSide::Buy,
        };

        let mut complete = trade.clone();
        complete.order_id = "native-complete".into();
        complete.trade_id = "native-complete-trade".into();

        complete.direction = match side {
            OrderSide::Buy => DeriveOrderSide::Buy,
            OrderSide::Sell => DeriveOrderSide::Sell,
        };

        let (emitter, mut rx) = test_emitter(clock);
        let account_id = AccountId::from("DERIVE-001");
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade.clone(), complete],
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: state.take_undelivered_trades(),
            },
            &emitter,
            account_id,
            clock,
            &state,
        );

        let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = rx.try_recv().unwrap()
        else {
            panic!("Expected acceptance before the completing fill");
        };

        let ExecutionEvent::Order(OrderEventAny::Filled(fill)) = rx.try_recv().unwrap() else {
            panic!("Expected the native completing fill");
        };

        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(
            accepted.venue_order_id,
            VenueOrderId::from("native-complete")
        );
        assert_eq!(fill.client_order_id, client_order_id);
        assert_eq!(fill.venue_order_id, VenueOrderId::from("native-complete"));
        assert_eq!(fill.order_side, side);
        assert_eq!(fill.last_qty.as_decimal(), dec!(0.5));
        assert_eq!(state.identity(&client_order_id), None);
        assert!(state.is_terminal(&client_order_id, identity.instrument_id));
        assert!(!state.contains_trade(&TradeId::from(trade.trade_id.as_str())));
        assert_eq!(
            serde_json::to_value(state.take_undelivered_trades()).unwrap(),
            serde_json::to_value(vec![trade]).unwrap()
        );
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_trade_delivery_failure_preserves_replay() {
        let clock = get_atomic_clock_realtime();
        let state = WsDispatchState::new(42);
        let trade: DeriveTrade = serde_json::from_str(include_str!(
            "../test_data/perps/http_private_trade_eth.json"
        ))
        .unwrap();
        let trade_id = TradeId::new(&trade.trade_id);
        let (emitter, rx) = test_emitter(clock);
        drop(rx);
        dispatch_trades_payload(
            DeriveTradesSubscriptionData {
                trades: vec![trade],
            },
            &emitter,
            AccountId::from("DERIVE-001"),
            clock,
            &state,
        );

        assert!(!state.contains_trade(&trade_id));
    }

    fn sample_derive_instrument() -> DeriveInstrument {
        serde_json::from_str(include_str!("../test_data/perps/instrument_eth.json")).unwrap()
    }

    fn test_client_with_instrument() -> DeriveExecutionClient {
        let mut client = DeriveExecutionClient::new(test_core(), test_config()).unwrap();
        let instrument =
            parse_derive_instrument_any(&sample_derive_instrument(), UnixNanos::default())
                .unwrap()
                .unwrap();
        client.on_instrument(instrument);
        client
    }

    #[rstest]
    #[case(" ")]
    #[case("external-\u{03bb}")]
    fn test_order_result_invalid_id_preserves_identity_without_binding_or_events(
        #[case] invalid_id: &str,
    ) {
        let clock = get_atomic_clock_realtime();
        let account_id = AccountId::from("DERIVE-001");
        let client_order_id = ClientOrderId::from("STRAT-SUBMIT-BAD-ID");
        let instrument_id = InstrumentId::from("ETH-PERP.DERIVE");
        let strategy_id = StrategyId::from("S-1");

        let identity = OrderIdentity {
            instrument_id,
            strategy_id,
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
        };

        let state = WsDispatchState::new(42);
        state.register_identity(client_order_id, identity);
        let (emitter, mut rx) = test_emitter(clock);
        let mut order: DeriveOrder = serde_json::from_str(include_str!(
            "../test_data/perps/http_order_eth_partially_filled.json"
        ))
        .unwrap();
        order.label = client_order_id.as_str().into();
        order.order_id = invalid_id.to_string();
        order.direction = crate::common::enums::DeriveOrderSide::Buy;
        order.order_status = DeriveOrderStatus::Open;
        order.filled_amount = Decimal::ZERO;
        dispatch_order_result(
            DeriveOrderResult {
                order: order.clone(),
                trades: vec![],
            },
            client_order_id,
            &emitter,
            account_id,
            clock,
            &state,
        );

        assert!(
            rx.try_recv().is_err(),
            "invalid response must emit no lifecycle event"
        );
        assert_eq!(state.bound_venue_order_id(&client_order_id), None);
        let retained = state
            .identity(&client_order_id)
            .expect("identity retained for reconciliation");
        assert_eq!(retained.instrument_id, instrument_id);
        assert_eq!(retained.strategy_id, strategy_id);
        assert_eq!(retained.order_side, OrderSide::Buy);
        assert_eq!(retained.order_type, OrderType::Limit);
        let venue_order_id = VenueOrderId::from("ord-valid-submit-result");
        order.order_id = venue_order_id.to_string();
        dispatch_order_result(
            DeriveOrderResult {
                order,
                trades: vec![],
            },
            client_order_id,
            &emitter,
            account_id,
            clock,
            &state,
        );

        let event = rx
            .try_recv()
            .expect("valid response accepts retained identity");

        let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = event else {
            panic!("expected tracked acceptance, was {event:?}");
        };

        assert_eq!(accepted.client_order_id, client_order_id);
        assert_eq!(accepted.instrument_id, instrument_id);
        assert_eq!(accepted.strategy_id, strategy_id);
        assert_eq!(accepted.account_id, account_id);
        assert_eq!(accepted.venue_order_id, venue_order_id);
        assert_eq!(
            state.bound_venue_order_id(&client_order_id),
            Some(venue_order_id)
        );
        assert!(rx.try_recv().is_err(), "valid response must accept once");
    }
}
