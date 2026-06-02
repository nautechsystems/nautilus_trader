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
//! Mirrors the Hyperliquid adapter's structural pattern: an
//! [`ExecutionClientCore`] holds identity and connection state, an
//! [`ExecutionEventEmitter`] publishes order/account events back to the live
//! engine, and the venue clients ([`DeriveHttpClient`], [`DeriveWebSocketClient`])
//! handle the wire. All state-changing requests are EIP-712 typed-data signed
//! against the per-action module contracts on the Derive Chain; the
//! `private/order` body in particular is built by [`order_to_derive_payload`].

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use ahash::AHashSet;
use anyhow::Context;
use async_trait::async_trait;
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
    AtomicMap, MUTEX_POISONED, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{ExecutionClientCore, ExecutionEventEmitter};
use nautilus_model::{
    accounts::AccountAny,
    data::QuoteTick,
    enums::{OmsType, OrderSide, OrderStatus, OrderType, PositionSideSpecified},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
    },
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, Symbol, Venue, VenueOrderId},
    instruments::InstrumentAny,
    orders::Order,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS, DERIVE_VENUE, TRIGGER_ORDER_SIGNATURE_TTL,
        },
        credential::DeriveCredential,
        enums::{DeriveInstrumentType, DeriveOrderSide},
        parse::{derive_rejection_due_post_only, format_instrument_id, format_venue_symbol},
        retry::{http_retry_config, is_write_outcome_ambiguous_ws},
    },
    config::DeriveExecClientConfig,
    http::{
        DeriveCredentials, DeriveHttpClient,
        models::{DeriveInstrument, DeriveOrder, DeriveTrade},
        parse::{
            parse_derive_order_to_report, parse_derive_position_to_report,
            parse_derive_subaccount_to_balances, parse_derive_trade_to_fill_report,
        },
        query::{
            DeriveCancelAllParams, DeriveCancelParams, DeriveCancelTriggerOrderParams,
            DeriveGetOpenOrdersParams, DeriveGetOrderHistoryParams, DeriveGetOrderParams,
            DeriveGetPositionsParams, DeriveGetSubaccountParams, DeriveGetTradeHistoryParams,
            DeriveGetTriggerOrdersParams, order_replace_to_derive_payload, order_to_derive_payload,
            trigger_order_to_derive_payload,
        },
    },
    signing::{
        context::{SigningContext, resolve_signing_context},
        nonce::NonceManager,
    },
    websocket::{
        DeriveOrdersSubscriptionData, DeriveTradesSubscriptionData, DeriveWebSocketClient,
        DeriveWsChannel, DeriveWsCredentials, DeriveWsError, DeriveWsExecutionHandle,
        DeriveWsMessage, OrderIdentity, WsDispatchState, parse::parse_ticker_quote_from_rest,
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
    config: DeriveExecClientConfig,
    credential: DeriveCredential,
    emitter: ExecutionEventEmitter,
    http_client: DeriveHttpClient,
    ws_client: DeriveWebSocketClient,
    ws_exec: DeriveWsExecutionHandle,
    instruments: Arc<AtomicMap<InstrumentId, DeriveInstrument>>,
    nonce_manager: Arc<NonceManager>,
    signing: SigningContext,
    is_connected: AtomicBool,
    cancellation_token: CancellationToken,
    pending_tasks: Mutex<Vec<JoinHandle<()>>>,
    ws_stream_handle: Mutex<Option<JoinHandle<()>>>,
    dispatch_state: Arc<WsDispatchState>,
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
    /// - Required credentials are not provided via config or environment.
    /// - Signing constants are still placeholders or cannot be parsed as hex.
    /// - The HTTP or WebSocket client cannot be constructed.
    pub fn new(core: ExecutionClientCore, config: DeriveExecClientConfig) -> anyhow::Result<Self> {
        let credential = DeriveCredential::resolve(
            config.wallet_address.clone(),
            config.session_key.clone(),
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
        let http_client = DeriveHttpClient::with_credentials(
            config.rest_url(),
            http_credentials,
            Some(config.http_timeout_secs),
            config.proxy_url.clone(),
            Some(retry_config),
        )
        .context("failed to create Derive HTTP client")?;

        let ws_credentials = DeriveWsCredentials::new(
            credential.wallet_address().to_string(),
            credential.session_key(),
        )
        .context("failed to build Derive WebSocket credentials")?;
        let ws_client = DeriveWebSocketClient::with_credentials(
            Some(config.ws_url()),
            config.environment,
            config.transport_backend,
            config.proxy_url.clone(),
            ws_credentials,
        );
        // The handle shares the client's command channel, which survives the
        // reconnect swap, so it stays valid for the client's lifetime.
        let ws_exec = ws_client.execution_handle();

        let signing = resolve_signing_context(&credential, &config)?;

        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

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
            is_connected: AtomicBool::new(false),
            cancellation_token: CancellationToken::new(),
            pending_tasks: Mutex::new(Vec::new()),
            ws_stream_handle: Mutex::new(None),
            dispatch_state: Arc::new(WsDispatchState::new()),
        })
    }

    /// Returns the resolved subaccount id.
    #[must_use]
    pub const fn subaccount_id(&self) -> u64 {
        self.credential.subaccount_id()
    }

    /// Returns a reference to the resolved configuration.
    #[must_use]
    pub fn config(&self) -> &DeriveExecClientConfig {
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
    pub fn cache_instrument(&self, instrument: DeriveInstrument) {
        let instrument_id = format_instrument_id(instrument.instrument_name);
        self.instruments.insert(instrument_id, instrument);
    }

    /// Spawns a fire-and-forget task tracked in `pending_tasks` for teardown.
    fn spawn_task<F>(&self, description: &'static str, fut: F)
    where
        F: std::future::Future<Output = anyhow::Result<()>> + Send + 'static,
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

    async fn ensure_instruments_initialized(&self) -> anyhow::Result<()> {
        if self.core.instruments_initialized() {
            return Ok(());
        }
        // Lazy bootstrap: exec-side fetches per-instrument on first reference.
        // Marking the flag prevents duplicate work across reconnect cycles.
        self.core.set_instruments_initialized();
        Ok(())
    }

    async fn refresh_account_state(&self) -> anyhow::Result<()> {
        let value = self
            .http_client
            .get_subaccount(&DeriveGetSubaccountParams::new(
                self.credential.subaccount_id(),
            ))
            .await
            .context("failed to fetch Derive subaccount snapshot")?;
        let (balances, margins) = parse_derive_subaccount_to_balances(&value)
            .context("failed to parse Derive subaccount balances")?;
        let ts_event = self.clock.get_time_ns();
        self.emitter
            .emit_account_state(balances, margins, true, ts_event);
        Ok(())
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

    /// Reverses the partial state `connect()` set up before the failing step:
    /// cancels the shared cancellation token, aborts the WS dispatch task,
    /// and closes the WS client. Used when initial account state cannot be
    /// loaded so that the next `connect()` call starts from a clean slate.
    async fn teardown_partial_connect(&mut self) {
        self.cancellation_token.cancel();

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }

        if let Err(e) = self.ws_client.disconnect().await {
            log::warn!("Error tearing down Derive WebSocket after connect failure: {e}");
        }
        self.abort_pending_tasks();
    }

    fn start_ws_dispatch(&self, rx: tokio::sync::mpsc::UnboundedReceiver<DeriveWsMessage>) {
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;
        let clock = self.clock;
        let cancellation = self.cancellation_token.clone();
        let dispatch_state = self.dispatch_state.clone();

        let handle = get_runtime().spawn(async move {
            let mut rx = rx;

            loop {
                tokio::select! {
                    biased;
                    () = cancellation.cancelled() => break,
                    maybe = rx.recv() => {
                        match maybe {
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
        });
        *self.ws_stream_handle.lock().expect(MUTEX_POISONED) = Some(handle);
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
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
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
        if self.core.is_stopped() {
            return Ok(());
        }

        log::info!("Stopping Derive execution client");

        self.cancellation_token.cancel();

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }
        self.abort_pending_tasks();

        self.core.set_disconnected();
        self.core.set_stopped();
        self.is_connected.store(false, Ordering::Release);

        log::info!("Derive execution client stopped");
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.is_connected() {
            return Ok(());
        }

        log::info!("Connecting Derive execution client");

        if self.cancellation_token.is_cancelled() {
            self.cancellation_token = CancellationToken::new();
        }

        self.ensure_instruments_initialized()
            .await
            .context("failed to initialize Derive instruments")?;

        self.ws_client
            .connect()
            .await
            .context("failed to connect Derive WebSocket")?;
        let rx = self
            .ws_client
            .take_event_receiver()
            .context("Derive execution WS event receiver not initialized")?;

        let subaccount_id = self.credential.subaccount_id();
        let channels = vec![
            DeriveWsChannel::orders(subaccount_id),
            DeriveWsChannel::private_trades(subaccount_id),
            DeriveWsChannel::balances(subaccount_id),
        ];

        if let Err(e) = self.ws_client.subscribe_channels(channels).await {
            log::warn!("Derive private WS subscriptions failed: {e}");
        }

        self.start_ws_dispatch(rx);

        // Fail-fast if the initial account snapshot cannot load: without it,
        // `await_account_registered` would block the full timeout window and
        // surface a misleading registration timeout. Tear down the WS we
        // already started so the caller does not leak the dispatch task.
        if let Err(e) = self.refresh_account_state().await {
            log::warn!("Initial Derive account state refresh failed: {e}; tearing down");
            self.teardown_partial_connect().await;
            return Err(e.context("failed initial Derive account state refresh"));
        }

        if let Err(e) = self
            .await_account_registered(DERIVE_ACCOUNT_REGISTRATION_TIMEOUT_SECS)
            .await
        {
            log::warn!("Derive account did not register in time: {e}; tearing down");
            self.teardown_partial_connect().await;
            return Err(e.context("failed waiting for Derive account registration"));
        }

        self.core.set_connected();
        self.is_connected.store(true, Ordering::Release);
        log::info!(
            "Connected Derive execution client ({:?})",
            self.config.environment
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if !self.is_connected() {
            return Ok(());
        }

        log::info!("Disconnecting Derive execution client");
        self.cancellation_token.cancel();

        if let Err(e) = self.ws_client.disconnect().await {
            log::warn!("Error while disconnecting Derive execution WebSocket: {e}");
        }

        if let Some(handle) = self.ws_stream_handle.lock().expect(MUTEX_POISONED).take() {
            handle.abort();
        }
        self.abort_pending_tasks();

        self.core.set_disconnected();
        self.is_connected.store(false, Ordering::Release);
        log::info!("Derive execution client disconnected");
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

    fn on_instrument(&mut self, _instrument: InstrumentAny) {
        // The exec-side instrument cache holds `DeriveInstrument` records so
        // signing can pull `base_asset_address` / `base_asset_sub_id`; the
        // generic `InstrumentAny` shape published on the bus does not carry
        // those, so the data client populates the cache via
        // [`Self::cache_instrument`] from its bootstrap pass instead.
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        if cmd.venue_order_id.is_none() && cmd.client_order_id.is_none() {
            log::warn!(
                "Derive generate_order_status_report requires venue_order_id or client_order_id"
            );
            return Ok(None);
        }

        let subaccount_id = self.credential.subaccount_id();
        let order = if let Some(venue_order_id) = cmd.venue_order_id {
            match self
                .http_client
                .get_order(&DeriveGetOrderParams::new(
                    subaccount_id,
                    venue_order_id.as_str(),
                ))
                .await
            {
                Ok(order) => Some(order),
                Err(e) => {
                    let trigger_orders = self
                        .http_client
                        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                        .await?
                        .orders;

                    match trigger_orders
                        .into_iter()
                        .find(|o| o.order_id.as_str() == venue_order_id.as_str())
                    {
                        Some(order) => Some(order),
                        None => return Err(e.into()),
                    }
                }
            }
        } else {
            // Derive has no by-label lookup endpoint; scan open orders first,
            // then trigger orders, then fall through to paginated history so
            // terminal orders resolve for reconcilers that only carry the
            // client_order_id.
            let label = cmd.client_order_id.expect("guarded above");
            let open_orders = self
                .http_client
                .get_open_orders(&DeriveGetOpenOrdersParams::new(subaccount_id))
                .await?
                .orders;
            let mut found = open_orders
                .into_iter()
                .find(|o| o.label.as_str() == label.as_str());

            if found.is_none() {
                let trigger_orders = self
                    .http_client
                    .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                    .await?
                    .orders;
                found = trigger_orders
                    .into_iter()
                    .find(|o| o.label.as_str() == label.as_str());
            }

            if found.is_none() {
                let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());
                let mut page: u32 = 1;

                'history: loop {
                    let mut params = DeriveGetOrderHistoryParams::new(
                        subaccount_id,
                        page,
                        DERIVE_PRIVATE_PAGE_SIZE,
                    );

                    if let Some(name) = instrument_name.as_deref() {
                        params = params.with_instrument_name(name);
                    }

                    let result = self.http_client.get_order_history(&params).await?;
                    let total_pages = result.pagination.num_pages;

                    for order in result.orders {
                        if order.label.as_str() == label.as_str() {
                            found = Some(order);
                            break 'history;
                        }
                    }

                    if (page as i64) >= total_pages || total_pages == 0 {
                        break;
                    }
                    page += 1;
                }
            }
            found
        };

        let Some(order) = order else {
            return Ok(None);
        };

        if let Some(instrument_id) = cmd.instrument_id
            && InstrumentId::new(Symbol::new(order.instrument_name.as_str()), *DERIVE_VENUE)
                != instrument_id
        {
            log::warn!(
                "Derive order {} is for {} but report requested {}",
                order.order_id,
                order.instrument_name.as_str(),
                instrument_id,
            );
            return Ok(None);
        }

        let ts_init = self.clock.get_time_ns();
        let mut report = parse_derive_order_to_report(&order, self.core.account_id, ts_init)?;
        // Prefer the parsed label (the venue's source of truth); only stamp
        // the cmd's id when the venue order has no label at all.
        if report.client_order_id.is_none()
            && let Some(client_order_id) = cmd.client_order_id
        {
            report = report.with_client_order_id(client_order_id);
        }
        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let subaccount_id = self.credential.subaccount_id();
        let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());

        // open_only routes to private/get_open_orders and
        // private/get_trigger_orders regardless of window; the venue
        // endpoints have no time bound but the caller's start/end is applied
        // below. For full history we walk private/get_order_history pages,
        // scoped to the optional window.
        let orders: Vec<DeriveOrder> = if cmd.open_only {
            let mut orders = self
                .http_client
                .get_open_orders(&DeriveGetOpenOrdersParams::new(subaccount_id))
                .await?
                .orders;
            orders.extend(
                self.http_client
                    .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                    .await?
                    .orders,
            );
            orders
        } else {
            let start_ms = cmd.start.map(|t| t.as_millis() as i64);
            let end_ms = cmd.end.map(|t| t.as_millis() as i64);
            let mut page: u32 = 1;
            let mut collected: Vec<DeriveOrder> = Vec::new();

            loop {
                let mut params =
                    DeriveGetOrderHistoryParams::new(subaccount_id, page, DERIVE_PRIVATE_PAGE_SIZE)
                        .with_window(start_ms, end_ms);

                if let Some(name) = instrument_name.as_deref() {
                    params = params.with_instrument_name(name);
                }

                let result = self.http_client.get_order_history(&params).await?;
                let total_pages = result.pagination.num_pages;
                collected.extend(result.orders);

                if (page as i64) >= total_pages || total_pages == 0 {
                    break;
                }
                page += 1;
            }
            collected
        };

        let ts_init = self.clock.get_time_ns();
        let start_ms = cmd.start.map(|t| t.as_millis() as i64);
        let end_ms = cmd.end.map(|t| t.as_millis() as i64);
        let mut reports = Vec::with_capacity(orders.len());
        for order in orders {
            if let Some(instrument_id) = cmd.instrument_id
                && InstrumentId::new(Symbol::new(order.instrument_name.as_str()), *DERIVE_VENUE)
                    != instrument_id
            {
                continue;
            }
            // open_only routed via private/get_open_orders ignores time bounds
            // at the venue level; apply the command's window here so callers
            // asking for "open orders since X" get exactly that.
            if let Some(start) = start_ms
                && order.last_update_timestamp < start
            {
                continue;
            }

            if let Some(end) = end_ms
                && order.last_update_timestamp > end
            {
                continue;
            }

            match parse_derive_order_to_report(&order, self.core.account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Skipping order in status report: {e}"),
            }
        }
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let instrument_name = cmd.instrument_id.map(|id| id.symbol.as_str().to_string());
        let mut page: u32 = 1;
        let mut all_trades: Vec<DeriveTrade> = Vec::new();

        loop {
            let mut params = DeriveGetTradeHistoryParams::new(
                self.credential.subaccount_id(),
                page,
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
            let total_pages = result.pagination.num_pages;
            all_trades.extend(result.trades);

            if (page as i64) >= total_pages || total_pages == 0 {
                break;
            }
            page += 1;
        }

        let ts_init = self.clock.get_time_ns();
        let fee_currency = Currency::USDC();
        let venue_order_id_filter = cmd
            .venue_order_id
            .as_ref()
            .map(|id| id.as_str().to_string());
        let mut reports = Vec::with_capacity(all_trades.len());
        for trade in all_trades {
            if let Some(target) = venue_order_id_filter.as_deref()
                && trade.order_id != target
            {
                continue;
            }

            match parse_derive_trade_to_fill_report(
                &trade,
                self.core.account_id,
                fee_currency,
                ts_init,
            ) {
                Ok(Some(report)) => {
                    // Cross-source dedup against the WS dispatch path: if the
                    // live stream already emitted this trade, the reconciler
                    // should not see it again.
                    if self.dispatch_state.check_and_insert_trade(report.trade_id) {
                        log::debug!(
                            "Skipping duplicate Derive fill (trade_id={}) in generate_fill_reports",
                            report.trade_id,
                        );
                        continue;
                    }
                    reports.push(report);
                }
                Ok(None) => {}
                Err(e) => log::warn!("Skipping trade in fill report: {e}"),
            }
        }
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let positions = self
            .http_client
            .get_positions(&DeriveGetPositionsParams::new(
                self.credential.subaccount_id(),
            ))
            .await?
            .positions;
        let ts_init = self.clock.get_time_ns();
        let mut reports = Vec::with_capacity(positions.len());
        for position in positions {
            if let Some(target) = cmd.instrument_id
                && InstrumentId::new(
                    Symbol::new(position.instrument_name.as_str()),
                    *DERIVE_VENUE,
                ) != target
            {
                continue;
            }

            match parse_derive_position_to_report(&position, self.core.account_id, ts_init) {
                Ok(report) => reports.push(report),
                Err(e) => log::warn!("Skipping position in status report: {e}"),
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
            let lookback_ns = mins.saturating_mul(60).saturating_mul(1_000_000_000);
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

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

        let (history_order_reports, open_order_reports, fill_reports, position_reports) = tokio::try_join!(
            self.generate_order_status_reports(&history_order_cmd),
            self.generate_order_status_reports(&open_order_cmd),
            self.generate_fill_reports(fill_cmd),
            self.generate_position_status_reports(&position_cmd),
        )?;

        log::info!(
            "Received {} historical OrderStatusReports",
            history_order_reports.len()
        );
        log::info!(
            "Received {} open OrderStatusReports",
            open_order_reports.len()
        );
        log::info!("Received {} FillReports", fill_reports.len());
        log::info!("Received {} PositionReports", position_reports.len());

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

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *DERIVE_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(history_order_reports);
        mass_status.add_order_reports(open_order_reports);
        mass_status.add_fill_reports(fill_reports);
        mass_status.add_position_reports(position_reports);
        add_missing_flat_position_reports(
            &mut mass_status,
            self.core.account_id,
            touched_instruments,
            ts_now,
        );

        Ok(Some(mass_status))
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let order = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|o| o.clone())
            .ok_or_else(|| {
                anyhow::anyhow!("Order not found in cache for {}", cmd.client_order_id)
            })?;

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        // Spot has no position to reduce; the venue rejects reduce-only
        // unconditionally (11025), so deny locally. Perp/option reduce-only is
        // position-conditional and must still reach the venue.
        if order.is_reduce_only()
            && matches!(
                self.core.cache().instrument(&cmd.instrument_id),
                Some(InstrumentAny::CurrencyPair(_))
            )
        {
            let reason = format!(
                "reduce-only is not supported for spot instrument {}; Derive spot has no position to reduce",
                cmd.instrument_id,
            );
            log::warn!("{reason}");
            self.emitter.emit_order_denied(&order, &reason);
            return Ok(());
        }

        // Keep the existing OrderDenied path here, then refresh before signing
        let is_trigger_order = is_derive_trigger_order_type(order.order_type());
        let market_quote = if order.order_type() == OrderType::Market {
            match self.core.cache().quote(&cmd.instrument_id) {
                Some(_) => Some(()),
                None => {
                    let reason = format!(
                        "no cached quote for {}; subscribe to quote data before submitting market orders",
                        cmd.instrument_id,
                    );
                    log::warn!("{reason}");
                    self.emitter.emit_order_denied(&order, &reason);
                    return Ok(());
                }
            }
        } else {
            None
        };

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
                    emitter.emit_order_rejected(
                        &order_for_task,
                        &format!("instrument resolution failed: {e}"),
                        ts,
                        false,
                    );
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
                emitter.emit_order_rejected(&order_for_task, &reason, ts, false);
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
                        emitter.emit_order_rejected(&order_for_task, &reason, ts, false);
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
                        emitter.emit_order_rejected(&order_for_task, &reason, ts, false);
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
                        emitter.emit_order_rejected(&order_for_task, &reason, ts, false);
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
                        emitter.emit_order_rejected(&order_for_task, &reason, ts, false);
                        return Ok(());
                    }
                }
            } else {
                None
            };

            let nonce = nonce_manager.next_nonce(&wallet_str, signing.subaccount_id)?;
            let expiry =
                (clock.get_time_ns().as_u64() / 1_000_000_000) as i64 + signing.signature_expiry_secs as i64;

            if is_trigger_order {
                let expiry = trigger_order_signature_expiry(clock);
                let payload = match trigger_order_to_derive_payload(
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
                    ws_exec.conn_id(),
                    UUID4::new().to_string(),
                ) {
                    Ok(p) => p,
                    Err(e) => {
                        log::warn!(
                            "Trigger order encode failed for {}: {e}",
                            order_for_task.client_order_id()
                        );
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emitter.emit_order_rejected(
                            &order_for_task,
                            &format!("order encoding failed: {e}"),
                            ts,
                            false,
                        );
                        return Ok(());
                    }
                };

                log::debug!(
                    "Derive trigger submit payload client_order_id={} instrument_name={} direction={} order_type={} time_in_force={} amount={} limit_price={} trigger_price={:?} trigger_price_type={:?} trigger_type={:?}",
                    order_for_task.client_order_id(),
                    payload.order.instrument_name.as_str(),
                    payload.order.direction,
                    payload.order.order_type,
                    payload.order.time_in_force,
                    payload.order.amount,
                    payload.order.limit_price,
                    payload.order.trigger_price,
                    payload.order.trigger_price_type,
                    payload.order.trigger_type,
                );

                match ws_exec.submit_trigger_order(&payload).await {
                    Ok(order) => {
                        let venue_order_id = VenueOrderId::new(order.order_id.as_str());
                        dispatch_state.record_venue_order_id(
                            order_for_task.client_order_id(),
                            venue_order_id,
                        );
                        let ts_now = clock.get_time_ns();
                        ensure_accepted_emitted(
                            &emitter,
                            &dispatch_state,
                            order_for_task.client_order_id(),
                            identity,
                            venue_order_id,
                            account_id,
                            ts_now,
                            ts_now,
                        );
                        log::debug!(
                            "Trigger order submitted: client_order_id={} venue_order_id={venue_order_id}",
                            order_for_task.client_order_id(),
                        );
                    }
                    Err(e) if is_write_outcome_ambiguous_ws(&e) => {
                        log::warn!(
                            "Derive trigger submit for {} returned ambiguous WS outcome: {e}; awaiting reconciliation",
                            order_for_task.client_order_id(),
                        );
                    }
                    Err(e) => {
                        let (reason, due_post_only) = ws_rejection_reason(&e);
                        log::debug!(
                            "Derive rejected trigger order {}: {reason}",
                            order_for_task.client_order_id(),
                        );
                        dispatch_state.forget(&order_for_task.client_order_id());
                        let ts = clock.get_time_ns();
                        emitter.emit_order_rejected(
                            &order_for_task,
                            &reason,
                            ts,
                            due_post_only,
                        );
                    }
                }
                return Ok(());
            }

            let payload = match order_to_derive_payload(
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
            ) {
                Ok(p) => p,
                Err(e) => {
                    log::warn!("Order encode failed for {}: {e}", order_for_task.client_order_id());
                    dispatch_state.forget(&order_for_task.client_order_id());
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(
                        &order_for_task,
                        &format!("order encoding failed: {e}"),
                        ts,
                        false,
                    );
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

            // Discard the result (and any `trades` it carries): fills arrive on
            // the `.trades` channel and are deduped by trade id.
            match ws_exec.submit_order(&payload).await {
                Ok(_) => {
                    log::debug!(
                        "Order submitted: client_order_id={}",
                        order_for_task.client_order_id(),
                    );
                }
                // See docs/integrations/derive.md "Order rejection semantics".
                Err(e) if is_write_outcome_ambiguous_ws(&e) => {
                    log::warn!(
                        "Derive submit for {} returned ambiguous WS outcome: {e}; awaiting reconciliation",
                        order_for_task.client_order_id(),
                    );
                }
                Err(e) => {
                    let (reason, due_post_only) = ws_rejection_reason(&e);
                    log::debug!(
                        "Derive rejected order {}: {reason}",
                        order_for_task.client_order_id(),
                    );
                    dispatch_state.forget(&order_for_task.client_order_id());
                    let ts = clock.get_time_ns();
                    emitter.emit_order_rejected(&order_for_task, &reason, ts, due_post_only);
                }
            }
            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        let orders = self.core.get_orders_for_list(&cmd.order_list)?;
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
        let Some(venue_order_id) = cmd.venue_order_id else {
            log::warn!(
                "Derive cancel_order requires venue_order_id (client_order_id={})",
                cmd.client_order_id,
            );
            return Ok(());
        };
        let ws_exec = self.ws_exec.clone();
        let subaccount_id = self.credential.subaccount_id();
        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let voi = venue_order_id.to_string();
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let strategy_id = cmd.strategy_id;
        let instrument_id = cmd.instrument_id;
        let client_order_id = cmd.client_order_id;
        let stale_venue_order_id = venue_order_id;
        let is_trigger_order = self
            .core
            .cache()
            .order(&client_order_id)
            .is_some_and(|order| is_derive_trigger_order_type(order.order_type()));

        self.spawn_task("cancel_order", async move {
            let outcome = if is_trigger_order {
                ws_exec
                    .cancel_trigger_order(&DeriveCancelTriggerOrderParams::new(
                        subaccount_id,
                        voi.as_str(),
                    ))
                    .await
                    .map(|_| ())
            } else {
                ws_exec
                    .cancel_order(&DeriveCancelParams::new(
                        subaccount_id,
                        venue_symbol.as_str(),
                        voi.as_str(),
                    ))
                    .await
            };

            match outcome {
                Ok(()) => {}
                // See docs/integrations/derive.md "Order rejection semantics".
                Err(e) if is_write_outcome_ambiguous_ws(&e) => {
                    log::warn!(
                        "Derive cancel for {client_order_id} returned ambiguous WS outcome: {e}; awaiting reconciliation",
                    );
                }
                Err(e) => {
                    let (reason, _) = ws_rejection_reason(&e);
                    log::debug!("Derive rejected cancel for {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(stale_venue_order_id),
                        &reason,
                        ts,
                    );
                }
            }
            Ok(())
        });
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let http_client = self.http_client.clone();
        let ws_exec = self.ws_exec.clone();
        let subaccount_id = self.credential.subaccount_id();
        let venue_symbol = format_venue_symbol(&cmd.instrument_id)?.to_string();
        let side_filter = cmd.order_side;

        self.spawn_task("cancel_all_orders", async move {
            // The venue endpoint scopes by instrument only, so when the
            // caller asks for a single side we list open orders (an idempotent
            // private read kept on HTTP), filter by side, and cancel each one
            // over the WebSocket. Calling `cancel_all` directly would drop both
            // sides and violate the command's filter.
            if matches!(side_filter, OrderSide::Buy | OrderSide::Sell) {
                let open_params = DeriveGetOpenOrdersParams::new(subaccount_id);
                let mut orders = match http_client.get_open_orders(&open_params).await {
                    Ok(v) => v,
                    Err(e) => {
                        log::warn!(
                            "Derive cancel_all_orders: failed to list open orders for side filter {side_filter:?}: {e}",
                        );
                        return Ok(());
                    }
                }
                .orders;

                match http_client
                    .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                    .await
                {
                    Ok(result) => orders.extend(result.orders),
                    Err(e) => {
                        log::warn!(
                            "Derive cancel_all_orders: failed to list trigger orders for side filter {side_filter:?}: {e}",
                        );
                    }
                }

                for order in orders {
                    if order.instrument_name.as_str() != venue_symbol {
                        continue;
                    }
                    let order_side = match order.direction {
                        DeriveOrderSide::Buy => OrderSide::Buy,
                        DeriveOrderSide::Sell => OrderSide::Sell,
                    };

                    if order_side != side_filter {
                        continue;
                    }

                    let outcome = if order.trigger_type.is_some() {
                        ws_exec
                            .cancel_trigger_order(&DeriveCancelTriggerOrderParams::new(
                                subaccount_id,
                                order.order_id.as_str(),
                            ))
                            .await
                            .map(|_| ())
                    } else {
                        ws_exec
                            .cancel_order(&DeriveCancelParams::new(
                                subaccount_id,
                                venue_symbol.as_str(),
                                order.order_id.as_str(),
                            ))
                            .await
                    };

                    if let Err(e) = outcome {
                        log::warn!(
                            "Derive cancel_all_orders: cancel for {} failed: {e}",
                            order.order_id,
                        );
                    }
                }
            } else if let Err(e) = ws_exec
                .cancel_all_orders(
                    &DeriveCancelAllParams::new(subaccount_id)
                        .with_instrument_name(venue_symbol.as_str()),
                )
                .await
            {
                log::warn!("Derive cancel_all_orders failed for {venue_symbol}: {e}");
            }

            if !matches!(side_filter, OrderSide::Buy | OrderSide::Sell) {
                let trigger_orders = match http_client
                    .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                    .await
                {
                    Ok(result) => result.orders,
                    Err(e) => {
                        log::warn!(
                            "Derive cancel_all_orders: failed to list trigger orders for {venue_symbol}: {e}",
                        );
                        return Ok(());
                    }
                };

                for order in trigger_orders {
                    if order.instrument_name.as_str() != venue_symbol {
                        continue;
                    }

                    if let Err(e) = ws_exec
                        .cancel_trigger_order(&DeriveCancelTriggerOrderParams::new(
                            subaccount_id,
                            order.order_id.as_str(),
                        ))
                        .await
                    {
                        log::warn!(
                            "Derive cancel_all_orders: trigger cancel for {} failed: {e}",
                            order.order_id,
                        );
                    }
                }
            }
            Ok(())
        });
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        for inner in cmd.cancels {
            self.cancel_order(inner)?;
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let ts_now = self.clock.get_time_ns();

        let Some(venue_order_id) = cmd.venue_order_id else {
            let reason = "venue_order_id is required for modify";
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                None,
                reason,
                ts_now,
            );
            return Ok(());
        };

        let Some(order) = self
            .core
            .cache()
            .order(&cmd.client_order_id)
            .map(|o| o.clone())
        else {
            let reason = "order not found in cache";
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                reason,
                ts_now,
            );
            return Ok(());
        };

        if is_derive_trigger_order_type(order.order_type()) {
            let reason = "Derive trigger orders cannot be modified; cancel and resubmit";
            log::warn!("Cannot modify order {}: {reason}", cmd.client_order_id);
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                reason,
                ts_now,
            );
            return Ok(());
        }

        let target_quantity = cmd.quantity.unwrap_or_else(|| order.quantity());
        let target_price = cmd.price.or_else(|| order.price());

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
        let voi_str = venue_order_id.to_string();

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
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(stale_venue_order_id),
                        &reason,
                        ts,
                    );
                    return Ok(());
                }
            };

            let nonce = nonce_manager.next_nonce(&wallet_str, signing.subaccount_id)?;
            let expiry = (clock.get_time_ns().as_u64() / 1_000_000_000) as i64
                + signing.signature_expiry_secs as i64;

            let payload = match order_replace_to_derive_payload(
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
                Some(target_quantity.as_decimal()),
                target_price.map(|p| p.as_decimal()),
                &voi_str,
            ) {
                Ok(p) => p,
                Err(e) => {
                    let reason = format!("replace encoding failed: {e}");
                    log::warn!("Cannot modify order {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(stale_venue_order_id),
                        &reason,
                        ts,
                    );
                    return Ok(());
                }
            };

            // Mark before sending so the cancel-of-old leg is suppressed even if
            // it arrives before this response.
            dispatch_state.mark_pending_modify(client_order_id, stale_venue_order_id);

            match ws_exec.modify_order(&payload).await {
                Ok(order) => {
                    let new_voi = VenueOrderId::new(order.order_id.as_str());
                    log::debug!(
                        "Order replaced: client_order_id={client_order_id}, new venue_order_id={new_voi}",
                    );
                    // Rebind before clearing the marker so a later cancel-of-old
                    // stays suppressed by the bound-id check.
                    dispatch_state.record_venue_order_id(client_order_id, new_voi);
                    dispatch_state.clear_pending_modify(&client_order_id);
                    let ts = clock.get_time_ns();
                    emitter.emit_order_updated(
                        &order_for_task,
                        new_voi,
                        target_quantity,
                        target_price,
                        None,
                        None,
                        ts,
                    );
                }
                // See docs/integrations/derive.md "Order rejection semantics".
                Err(e) if is_write_outcome_ambiguous_ws(&e) => {
                    dispatch_state.clear_pending_modify(&client_order_id);
                    log::warn!(
                        "Derive modify for {client_order_id} returned ambiguous WS outcome: {e}; awaiting reconciliation",
                    );
                }
                Err(e) => {
                    dispatch_state.clear_pending_modify(&client_order_id);
                    let (reason, _) = ws_rejection_reason(&e);
                    log::debug!("Derive rejected modify for {client_order_id}: {reason}");
                    let ts = clock.get_time_ns();
                    emitter.emit_order_modify_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(stale_venue_order_id),
                        &reason,
                        ts,
                    );
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
            let (balances, margins) = parse_derive_subaccount_to_balances(&subaccount)?;
            let ts_event = clock.get_time_ns();
            emitter.emit_account_state(balances, margins, true, ts_event);
            Ok(())
        });
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let Some(venue_order_id) = cmd.venue_order_id else {
            log::warn!(
                "Derive query_order requires venue_order_id (client_order_id={})",
                cmd.client_order_id,
            );
            return Ok(());
        };
        let http_client = self.http_client.clone();
        let subaccount_id = self.credential.subaccount_id();
        let account_id = self.core.account_id;
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let voi = venue_order_id.to_string();

        self.spawn_task("query_order", async move {
            let order = match http_client
                .get_order(&DeriveGetOrderParams::new(subaccount_id, voi.as_str()))
                .await
            {
                Ok(o) => o,
                Err(e) => {
                    let trigger_orders = match http_client
                        .get_trigger_orders(&DeriveGetTriggerOrdersParams::new(subaccount_id))
                        .await
                    {
                        Ok(result) => result.orders,
                        Err(trigger_err) => {
                            log::warn!(
                                "Failed to fetch Derive order {voi}: {e}; trigger lookup also failed: {trigger_err}",
                            );
                            return Ok(());
                        }
                    };

                    match trigger_orders
                        .into_iter()
                        .find(|o| o.order_id.as_str() == voi.as_str())
                    {
                        Some(order) => order,
                        None => {
                            log::warn!("Failed to fetch Derive order {voi}: {e}");
                            return Ok(());
                        }
                    }
                }
            };
            let ts_init = clock.get_time_ns();
            let report = parse_derive_order_to_report(&order, account_id, ts_init)?;
            emitter.send_order_status_report(report);
            Ok(())
        });
        Ok(())
    }
}

// Reason text and post-only classification for a definitive WS write failure.
// Non-JSON-RPC errors carry no venue code and are never post-only crossings.
fn ws_rejection_reason(error: &DeriveWsError) -> (String, bool) {
    match error {
        DeriveWsError::JsonRpc { code, message, .. } => (
            format!("JSON-RPC {code}: {message}"),
            derive_rejection_due_post_only(Some(*code), message),
        ),
        other => (other.to_string(), false),
    }
}

fn add_missing_flat_position_reports(
    mass_status: &mut ExecutionMassStatus,
    account_id: AccountId,
    touched_instruments: AHashSet<InstrumentId>,
    ts_init: UnixNanos,
) {
    let active_position_instruments: AHashSet<InstrumentId> =
        mass_status.position_reports().keys().copied().collect();
    let mut flat_reports = Vec::new();

    for instrument_id in touched_instruments {
        if active_position_instruments.contains(&instrument_id) {
            continue;
        }

        flat_reports.push(PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSideSpecified::Flat,
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

fn handle_ws_message(
    message: DeriveWsMessage,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    clock: &'static AtomicTime,
    dispatch_state: &WsDispatchState,
) {
    let payload = match message {
        DeriveWsMessage::Subscription(payload) => payload,
        DeriveWsMessage::Authenticated | DeriveWsMessage::Reconnected => return,
    };

    let is_orders_channel = payload.channel.as_str().ends_with(".orders");
    let is_trades_channel = payload.channel.as_str().ends_with(".trades");

    if is_orders_channel {
        let data = match serde_json::from_str::<DeriveOrdersSubscriptionData>(payload.data.get()) {
            Ok(data) => data,
            Err(_) => return,
        };
        dispatch_orders_payload(data, emitter, account_id, clock, dispatch_state);
    } else if is_trades_channel {
        let data = match serde_json::from_str::<DeriveTradesSubscriptionData>(payload.data.get()) {
            Ok(data) => data,
            Err(_) => return,
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
    let ts_init = clock.get_time_ns();
    for order in data.orders {
        let report = match parse_derive_order_to_report(&order, account_id, ts_init) {
            Ok(report) => report,
            Err(e) => {
                log::warn!("Failed to parse Derive order WS update: {e}");
                continue;
            }
        };

        let identity = report
            .client_order_id
            .and_then(|cid| dispatch_state.identity(&cid).map(|ident| (cid, ident)));

        match identity {
            Some((client_order_id, identity)) => emit_tracked_order_event(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                &report,
                account_id,
                ts_init,
            ),
            None => emitter.send_order_status_report(report),
        }
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
    let ts_init = clock.get_time_ns();
    let fee_currency = Currency::USDC();
    for trade in data.trades {
        match parse_derive_trade_to_fill_report(&trade, account_id, fee_currency, ts_init) {
            Ok(Some(report)) => {
                if dispatch_state.check_and_insert_trade(report.trade_id) {
                    log::debug!(
                        "Skipping duplicate Derive fill (trade_id={}) on WS dispatch",
                        report.trade_id,
                    );
                    continue;
                }

                let identity = report
                    .client_order_id
                    .and_then(|cid| dispatch_state.identity(&cid).map(|ident| (cid, ident)));

                match identity {
                    Some((client_order_id, identity)) => emit_tracked_fill(
                        emitter,
                        dispatch_state,
                        client_order_id,
                        identity,
                        &report,
                        account_id,
                        ts_init,
                    ),
                    None => emitter.send_fill_report(report),
                }
            }
            Ok(None) => {}
            Err(e) => log::warn!("Failed to parse Derive trade WS update: {e}"),
        }
    }
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
) {
    if dispatch_state.mark_accepted(client_order_id) {
        return;
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
    emitter.send_order_event(OrderEventAny::Accepted(accepted));
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
    if dispatch_state.pending_modify(&client_order_id) == Some(venue_order_id) {
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
            report.order_status,
        );
        return;
    }

    match report.order_status {
        OrderStatus::Accepted | OrderStatus::PartiallyFilled => {
            if dispatch_state.contains_filled(&client_order_id) {
                log::debug!("Skipping stale Accepted for {client_order_id} (already filled)",);
                return;
            }
            dispatch_state.record_venue_order_id(client_order_id, venue_order_id);
            ensure_accepted_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                ts_accepted,
                ts_init,
            );
        }
        OrderStatus::Filled => {
            dispatch_state.record_venue_order_id(client_order_id, venue_order_id);
            ensure_accepted_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                ts_accepted,
                ts_init,
            );
            // Mark the order terminal so replayed Accepted frames are
            // suppressed, but keep its identity alive: the matching
            // `.trades` frame may arrive after this `.orders` Filled
            // notice and still needs the tracked path to emit a proper
            // `OrderFilled`. Identity is retired by Canceled/Expired/
            // Rejected paths; full-fill leaks are bounded by submission
            // throughput.
            dispatch_state.mark_filled(client_order_id);
        }
        OrderStatus::Canceled => {
            ensure_accepted_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                ts_accepted,
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
            emitter.send_order_event(OrderEventAny::Canceled(canceled));
            dispatch_state.forget(&client_order_id);
        }
        OrderStatus::Expired => {
            ensure_accepted_emitted(
                emitter,
                dispatch_state,
                client_order_id,
                identity,
                venue_order_id,
                account_id,
                ts_accepted,
                ts_init,
            );
            let expired = OrderExpired::new(
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
            emitter.send_order_event(OrderEventAny::Expired(expired));
            dispatch_state.forget(&client_order_id);
        }
        OrderStatus::Rejected => {
            let reason = report
                .cancel_reason
                .as_deref()
                .unwrap_or("Order rejected by Derive");
            let due_post_only = derive_rejection_due_post_only(None, reason);
            let rejected = OrderRejected::new(
                emitter.trader_id(),
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                account_id,
                Ustr::from(reason),
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                due_post_only,
            );
            emitter.send_order_event(OrderEventAny::Rejected(rejected));
            dispatch_state.forget(&client_order_id);
        }
        other => {
            log::debug!(
                "Unhandled tracked order status {other:?} for {client_order_id}, sending as report",
            );
            emitter.send_order_status_report(report.clone());
        }
    }
}

fn emit_tracked_fill(
    emitter: &ExecutionEventEmitter,
    dispatch_state: &WsDispatchState,
    client_order_id: ClientOrderId,
    identity: OrderIdentity,
    report: &FillReport,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    ensure_accepted_emitted(
        emitter,
        dispatch_state,
        client_order_id,
        identity,
        report.venue_order_id,
        account_id,
        report.ts_event,
        ts_init,
    );

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
    );
    emitter.send_order_event(OrderEventAny::Filled(filled));
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
        // NoOrderSide is rejected upstream by `order_side_to_derive`.
        OrderSide::NoOrderSide => return None,
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
        OrderSide::NoOrderSide => return None,
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

fn trigger_order_signature_expiry(clock: &'static AtomicTime) -> i64 {
    let now_secs = (clock.get_time_ns().as_u64() / 1_000_000_000) as i64;
    now_secs + TRIGGER_ORDER_SIGNATURE_TTL.as_secs() as i64
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
        OrderSide::NoOrderSide => ratio.round(),
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

    use nautilus_common::{cache::Cache, messages::ExecutionEvent};
    use nautilus_core::UnixNanos;
    use nautilus_live::ExecutionClientCore;
    use nautilus_model::{
        data::QuoteTick,
        enums::{AccountType, OmsType, TimeInForce},
        identifiers::{AccountId, ClientId, InstrumentId, StrategyId, TraderId},
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::common::{consts::DERIVE, enums::DeriveEnvironment};

    const TEST_WALLET: &str = "0x0000000000000000000000000000000000001234";
    const TEST_SESSION_KEY: &str =
        "0x2ae8be44db8a590d20bffbe3b6872df9b569147d3bf6801a35a28281a4816bbd";
    const TEST_SUBACCOUNT: u64 = 30769;

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

    fn test_config() -> DeriveExecClientConfig {
        DeriveExecClientConfig {
            wallet_address: Some(TEST_WALLET.to_string()),
            session_key: Some(TEST_SESSION_KEY.to_string()),
            subaccount_id: Some(TEST_SUBACCOUNT),
            environment: DeriveEnvironment::Testnet,
            domain_separator: Some(
                "0x2222222222222222222222222222222222222222222222222222222222222222".to_string(),
            ),
            action_typehash: Some(
                "0x1111111111111111111111111111111111111111111111111111111111111111".to_string(),
            ),
            trade_module_address: Some("0x000000000000000000000000000000000000bbbb".to_string()),
            ..DeriveExecClientConfig::default()
        }
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
            OrderSide::Buy,
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

        let new_emitter = || {
            let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
            let mut emitter = ExecutionEventEmitter::new(
                clock,
                TraderId::from("TRADER-001"),
                account_id,
                AccountType::Margin,
                Some(Currency::USDC()),
            );
            emitter.set_sender(tx);
            (emitter, rx)
        };

        // Marker targets the cancel's venue order id and no bound id is
        // recorded, so suppression can only come from the in-flight branch.
        let (emitter, mut rx) = new_emitter();
        let state = WsDispatchState::new();
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
        let (emitter, mut rx) = new_emitter();
        let state = WsDispatchState::new();
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
}
