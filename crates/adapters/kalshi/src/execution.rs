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

//! Live Kalshi execution client.

pub(crate) mod settlement;

mod account;
mod batch;
mod context;
mod mutation;
mod reports;
mod snapshot;
mod stream;
mod submit;

use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    num::NonZeroU64,
    rc::Rc,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::{
        runner::{try_get_data_event_sender, try_get_exec_event_sender},
        timer::LiveTimer,
    },
    messages::{DataEvent, execution::*},
    runner::try_get_time_event_sender,
    timer::TimeEventCallback,
};
use nautilus_core::{
    DurationNanos, Params, UUID4, UnixNanos,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter,
    task::{TaskGroup, TaskGroupGuard},
};
use nautilus_model::{
    accounts::AccountAny,
    enums::{AccountType, LiquiditySide, OmsType, OrderStatus},
    events::{OrderDeniedReason, OrderInitialized},
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, PositionId, Venue, VenueOrderId,
    },
    instruments::{BinaryOption, Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Money, Price, Quantity},
};
use nautilus_network::dst::time::Instant;
use rust_decimal::Decimal;

use self::{
    context::RecoveryContextError,
    mutation::{ADMISSION_TIMEOUT, Command, Mutation, PendingMutation},
    settlement::Registration,
    snapshot::{
        collect_fills, collect_orders, collect_positions_current, collect_recovery_snapshot,
        collect_snapshot_at, load_definition, timestamp, validate_definition, validate_order_scope,
    },
    stream::PrivateStream,
};
pub use self::{
    context::{OrderPlan, PrivateState},
    reports::{fill as fill_report, order as order_report},
    snapshot::{KalshiExecutionSnapshot, collect_snapshot},
    submit::request,
};
use crate::{
    common::{
        consts::KALSHI_VENUE,
        parse::{parse_instrument_id, parse_ticker},
    },
    config::{KalshiDataClientConfig, KalshiExecutionClientConfig},
    http::{
        client::KalshiHttpClient,
        models::{Fill, HistoricalCutoff, Order as VenueOrder, OrderStatus as VenueOrderStatus},
        query::FillsQuery,
    },
    websocket::{
        client::KalshiWebSocketClient,
        messages::{KalshiChannel, KalshiSubscription},
    },
};

/// Submits GTC, IOC, FOK, and GTD limits, amends and cancels orders, recovers Kalshi execution
/// and cash, and provides shard-scoped reports.
///
/// Aggregate reports omit validated unfilled external orders without command metadata and mark mass
/// coverage incomplete. Targeted reports remain strict. Report collection does not authorize
/// submission. Account bootstrap and reconciliation precede order flow.
#[derive(Debug)]
pub struct KalshiExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    config: KalshiExecutionClientConfig,
    http: KalshiHttpClient,
    emitter: ExecutionEventEmitter,
    definitions: RefCell<HashMap<InstrumentId, BinaryOption>>,
    transport: KalshiDataClientConfig,
    disposed: bool,
    private: Arc<Mutex<PrivateState>>,
    ready: Arc<AtomicBool>,
    session_tasks: TaskGroup,
    command_tasks: TaskGroup,
    ws_client: Option<KalshiWebSocketClient>,
    refresh: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    settlement_registration: Option<Registration>,
    settlement_timer: Option<LiveTimer>,
}

impl KalshiExecutionClient {
    /// Creates a disconnected execution client using primary-account credentials.
    ///
    /// # Errors
    ///
    /// Returns an error if identity/account/OMS settings or credentials are invalid, or transport
    /// setup fails.
    pub fn new(
        core: ExecutionClientCore,
        config: KalshiExecutionClientConfig,
    ) -> anyhow::Result<Self> {
        config.validate()?;
        anyhow::ensure!(
            core.venue == *KALSHI_VENUE && core.account_id == config.account_id,
            "Kalshi execution identity mismatch"
        );
        anyhow::ensure!(
            core.account_type == AccountType::Cash
                && core.oms_type == OmsType::Netting
                && core.base_currency == Some(Currency::USD()),
            "Kalshi requires a USD Cash account with netting OMS"
        );

        let transport = KalshiDataClientConfig {
            api_key_id: config.api_key_id.clone(),
            private_key: config.private_key.clone(),
            environment: config.environment,
            base_url_http: config.base_url_http.clone(),
            base_url_ws: config.base_url_ws.clone(),
            proxy_url: config.proxy_url.clone(),
            http_timeout_secs: config.http_timeout_secs,
            ws_timeout_secs: config.ws_timeout_secs,
            transport_backend: config.transport_backend,
            ..Default::default()
        };

        let http = KalshiHttpClient::new_authenticated(&transport)?;
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

        core.set_disconnected();
        Ok(Self {
            core,
            clock,
            config,
            http,
            emitter,
            definitions: RefCell::new(HashMap::new()),
            transport,
            disposed: false,
            private: Arc::new(Mutex::new(PrivateState::new())),
            ready: Arc::new(AtomicBool::new(false)),
            session_tasks: TaskGroup::new(),
            command_tasks: TaskGroup::new(),
            ws_client: None,
            refresh: None,
            settlement_registration: None,
            settlement_timer: None,
        })
    }

    #[cfg(all(test, not(all(feature = "simulation", madsim))))]
    pub(crate) fn config(&self) -> &KalshiExecutionClientConfig {
        &self.config
    }

    #[cfg(all(test, not(all(feature = "simulation", madsim))))]
    pub(crate) fn core(&self) -> &ExecutionClientCore {
        &self.core
    }

    async fn finish_shutdown(&self) -> anyhow::Result<()> {
        let (session, commands) = tokio::join!(
            self.session_tasks
                .finish_shutdown(Duration::from_millis(100), Duration::from_secs(1)),
            self.command_tasks
                .finish_shutdown(Duration::from_millis(100), Duration::from_secs(1)),
        );
        session?;
        commands?;
        Ok(())
    }

    async fn definition(&self, ticker: &str, ts_init: UnixNanos) -> anyhow::Result<BinaryOption> {
        let id = parse_instrument_id(ticker)?;
        let known = self.definitions.borrow().get(&id).cloned();
        if let Some(instrument) = known {
            return Ok(instrument);
        }

        let cached = self.core.cache().instrument(&id).cloned();
        let unloaded = cached.is_none();

        let instrument = if let Some(instrument) = cached {
            let InstrumentAny::BinaryOption(instrument) = instrument else {
                anyhow::bail!("Kalshi report requires a binary instrument");
            };

            instrument
        } else {
            load_definition(&self.http, ticker, ts_init).await?
        };

        validate_definition(&instrument, self.config.exchange_index)?;
        if unloaded && let Some(sender) = try_get_data_event_sender() {
            let data = InstrumentAny::BinaryOption(instrument.clone()).into();
            sender
                .send(DataEvent::Data(data))
                .map_err(|_| anyhow::anyhow!("instrument receiver closed"))?;
        }

        self.definitions.borrow_mut().insert(id, instrument.clone());
        Ok(instrument)
    }

    fn client_order_id(&self, value: &VenueOrder) -> anyhow::Result<Option<ClientOrderId>> {
        let venue_order_id = VenueOrderId::new_checked(&value.order_id)?;

        let wire_id = if value.client_order_id.is_empty() {
            None
        } else {
            Some(ClientOrderId::new_checked(&value.client_order_id)?)
        };

        let mapped_id = self.core.cache().client_order_id(&venue_order_id).copied();
        anyhow::ensure!(
            wire_id
                .zip(mapped_id)
                .is_none_or(|(wire, mapped)| wire == mapped),
            "cached client order identity mismatch"
        );
        Ok(wire_id.or(mapped_id))
    }

    fn cached_order(&self, value: &VenueOrder) -> anyhow::Result<Option<OrderAny>> {
        let client_order_id = self.client_order_id(value)?;
        let cache = self.core.cache();
        let cached =
            client_order_id.and_then(|id| cache.order_ref(&id).map(|order| order.cloned()));

        if let Some(order) = &cached {
            let venue_order_id = VenueOrderId::new_checked(&value.order_id)?;
            anyhow::ensure!(
                order.venue_order_id().is_none_or(|id| id == venue_order_id)
                    && cache
                        .venue_order_id(&order.client_order_id())
                        .is_none_or(|id| *id == venue_order_id),
                "cached venue order identity mismatch"
            );
        }

        Ok(cached)
    }

    async fn order_report(
        &self,
        value: &VenueOrder,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let definition = self.definition(&value.ticker, ts_init).await?;
        let cached = self.cached_order(value)?;
        let report = reports::order(
            value,
            &definition,
            self.core.account_id,
            cached.as_ref(),
            ts_init,
        )?;
        // Standalone status reports cannot carry the fills needed to import an unknown order
        Ok(report.filter(|_| cached.is_some()))
    }

    async fn order_report_aggregate(
        &self,
        value: &VenueOrder,
        ts_init: UnixNanos,
        max_pages: u32,
        cutoff: Option<&HistoricalCutoff>,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let recovered = self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .source_order(value)?;
        let report = self.order_report(value, ts_init).await?;
        if report.is_some() {
            return Ok(report);
        }

        if let Some(cached) = recovered
            .as_ref()
            .filter(|order| order.status() == OrderStatus::Rejected)
        {
            let definition = self.definition(&value.ticker, ts_init).await?;
            let report = reports::order_aggregate(
                value,
                &definition,
                self.core.account_id,
                cached,
                ts_init,
            )?;

            if report.is_some() {
                return Ok(report);
            }
        }

        let venue_order_id = VenueOrderId::new_checked(&value.order_id)?;
        let missing_context = self.cached_order(value)?.is_none()
            && self.core.cache().client_order_id(&venue_order_id).is_none();

        if missing_context && !value.fill_count_fp.0.is_zero() {
            return Err(RecoveryContextError::Order.into());
        }

        let unfilled_external = recovered
            .as_ref()
            .is_some_and(|order| order.strategy_id().is_external());
        anyhow::ensure!(
            missing_context && (unfilled_external || recovered.is_none()),
            "order command metadata or canceled total is not authoritative"
        );

        let cutoff = match cutoff {
            Some(cutoff) => cutoff.clone(),
            None => self.http.get_historical_cutoff().await?,
        };

        let query = FillsQuery {
            ticker: Some(value.ticker.clone()),
            order_id: Some(value.order_id.clone()),
            exchange_index: self.config.exchange_index,
            ..Default::default()
        };

        let fills = collect_fills(&self.http, &query, None, ts_init, &cutoff, max_pages).await?;
        anyhow::ensure!(
            fills.iter().all(|fill| fill.order_id != value.order_id),
            "external order recovery lacks command metadata for collected fills"
        );
        Ok(None)
    }

    async fn fill_report(
        &self,
        value: &Fill,
        source: &VenueOrder,
        ts_init: UnixNanos,
    ) -> anyhow::Result<FillReport> {
        let definition = self.definition(&value.ticker, ts_init).await?;
        let mut report = reports::fill(value, &definition, self.core.account_id, source, ts_init)?;
        let cached = self.cached_order(source)?;
        if let Some(cached) = cached {
            anyhow::ensure!(
                cached.instrument_id() == report.instrument_id
                    && cached.order_side() == report.order_side
                    && cached
                        .account_id()
                        .is_none_or(|account| account == self.core.account_id),
                "cached fill identity mismatch"
            );
            report.client_order_id = Some(cached.client_order_id());
        }

        Ok(report)
    }

    fn admit_reports(
        &self,
        orders: &[OrderStatusReport],
        fills: &[FillReport],
    ) -> anyhow::Result<()> {
        let result = self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .admit_reports(&self.core.cache(), orders, fills);

        if result.is_err() {
            self.ready.store(false, Ordering::Release);
        }

        result
    }

    fn observe_settlement(&self) {
        let private = self
            .private
            .lock()
            .expect("private state lock is not poisoned");
        private
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .observe(&self.core.cache(), self.core.account_id);
    }

    fn start_settlement_timer(&mut self) {
        if self.settlement_timer.is_some() {
            return;
        }

        let Some(sender) = try_get_time_event_sender() else {
            return;
        };

        let core = self.core.clone();
        let private = Arc::downgrade(&self.private);
        let callback = TimeEventCallback::RustLocal(Rc::new(move |_| {
            if let Some(private) = private.upgrade() {
                let private = private.lock().expect("private state lock is not poisoned");
                private
                    .barrier
                    .lock()
                    .expect("settlement barrier lock")
                    .observe(&core.cache(), core.account_id);
            }
        }));

        let mut timer = LiveTimer::new(
            ustr::Ustr::from(format!("kalshi-settlement-{}", self.core.client_id).as_str()),
            NonZeroU64::new(100_000_000).expect("nonzero interval"),
            self.clock.get_time_ns(),
            None,
            callback,
            false,
            Some(sender),
        );

        timer.start();
        self.settlement_timer = Some(timer);
    }

    fn restore_cache(&self) -> anyhow::Result<()> {
        let cache = self.core.cache();
        if let Some(account) = cache.account_owned(&self.core.account_id) {
            anyhow::ensure!(
                matches!(account, AccountAny::Cash(ref account) if !account.base.calculate_account_state && !account.allow_borrowing),
                "Kalshi bootstrap requires reported Cash without borrowing"
            );
        }

        let mut private = self
            .private
            .lock()
            .expect("private state lock is not poisoned");
        private.invalidate();

        let orders: Vec<_> = cache
            .orders(Some(&KALSHI_VENUE), None, None, None, None)
            .into_iter()
            .filter(|order| {
                cache.client_id(&order.client_order_id()) == Some(&self.core.client_id)
                    && order
                        .account_id()
                        .is_none_or(|id| id == self.core.account_id)
            })
            .collect();

        if orders
            .iter()
            .any(|order| private.requires_rebase(&order.cloned()))
        {
            private.rebase();
        }

        for order in orders {
            if let Some(InstrumentAny::BinaryOption(definition)) =
                cache.instrument(&order.instrument_id())
            {
                self.definitions
                    .borrow_mut()
                    .insert(definition.id, definition.clone());
            }

            private.restore(order.cloned());
        }

        private.seed_positions(
            cache
                .positions_open(
                    Some(&KALSHI_VENUE),
                    None,
                    None,
                    Some(&self.core.account_id),
                    None,
                )
                .into_iter()
                .map(|position| {
                    (
                        position.instrument_id,
                        position.signed_decimal_qty(),
                        position.ts_last,
                    )
                }),
        )?;

        private
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .observe(&cache, self.core.account_id);

        Ok(())
    }
}

#[async_trait(?Send)]
impl ExecutionClient for KalshiExecutionClient {
    fn is_connected(&self) -> bool {
        self.observe_settlement();

        if self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .is_blocked()
        {
            return false;
        }

        if !self.core.is_connected()
            || !self.ready.load(Ordering::Acquire)
            || !self
                .ws_client
                .as_ref()
                .is_some_and(KalshiWebSocketClient::is_active)
        {
            return false;
        }

        let private = self
            .private
            .lock()
            .expect("private state lock is not poisoned");

        if private.epoch.is_none()
            || private.epoch
                != self
                    .ws_client
                    .as_ref()
                    .and_then(KalshiWebSocketClient::connection_epoch)
        {
            return false;
        }

        let session = private.session.clone();
        drop(private);
        self.core
            .cache()
            .account(&self.core.account_id)
            .and_then(|account| account.last_event())
            .and_then(|event| event.info)
            .and_then(|info| {
                info.get("kalshi_session")
                    .and_then(serde_json::Value::as_str)
                    .map(String::from)
            })
            .is_some_and(|value| value == session)
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *KALSHI_VENUE
    }

    fn oms_type(&self) -> OmsType {
        OmsType::Netting
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
    }

    fn retain_unresolved_submissions(&self) -> bool {
        true
    }

    fn provides_bulk_position_coverage(&self, _instrument_id: InstrumentId) -> bool {
        false
    }

    fn calculate_commission(
        &self,
        _instrument: &InstrumentAny,
        _last_qty: Quantity,
        _last_px: Price,
        _liquidity_side: LiquiditySide,
    ) -> anyhow::Result<Option<Money>> {
        anyhow::bail!("Kalshi reconciliation requires an authoritative fill fee")
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
            .try_emit_account_state(balances, margins, reported, ts_event, info)
    }

    fn start(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi execution client is disposed");
        let sender = try_get_exec_event_sender().context("execution sender is not installed")?;
        self.emitter.set_sender(sender);

        if self.settlement_registration.is_none()
            && let Some(sender) = try_get_data_event_sender()
        {
            let private = self
                .private
                .lock()
                .expect("private state lock is not poisoned");
            self.settlement_registration = Some(Registration::new(sender, &private.barrier));
        }

        self.start_settlement_timer();
        self.core.set_started();
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        self.core.set_stopped();
        self.core.set_disconnected();
        self.ready.store(false, Ordering::Release);
        self.private
            .lock()
            .expect("private state lock is not poisoned")
            .invalidate();

        if let Some(mut timer) = self.settlement_timer.take() {
            timer.cancel();
        }

        self.session_tasks.begin_shutdown();
        self.command_tasks.begin_shutdown();
        self.refresh = None;
        self.ws_client = None;
        Ok(())
    }

    fn reset(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        self.definitions.borrow_mut().clear();
        Ok(())
    }

    fn dispose(&mut self) -> anyhow::Result<()> {
        self.reset()?;
        self.settlement_registration = None;
        self.disposed = true;
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        anyhow::ensure!(!self.disposed, "Kalshi execution client is disposed");
        anyhow::ensure!(
            self.emitter.is_initialized(),
            "Kalshi bootstrap requires an execution sender"
        );

        if self.core.is_connected()
            && self
                .ws_client
                .as_ref()
                .is_some_and(KalshiWebSocketClient::is_active)
        {
            return Ok(());
        }

        self.stop()?;
        self.finish_shutdown().await?;
        self.session_tasks.start_generation()?;
        self.command_tasks.start_generation()?;
        self.restore_cache()?;
        self.start_settlement_timer();

        let (ws_client, receiver) = KalshiWebSocketClient::new(self.transport.clone())?;
        for channel in [
            KalshiChannel::UserOrders,
            KalshiChannel::Fill,
            KalshiChannel::MarketPositions,
        ] {
            ws_client.subscribe(KalshiSubscription {
                channel,
                market_tickers: Vec::new(),
            })?;
        }

        let (requests, pending) = tokio::sync::mpsc::unbounded_channel();
        let (startup, initialized) = tokio::sync::oneshot::channel();
        let ready = Arc::clone(&self.ready);
        let guard = TaskGroupGuard::new(&[&self.session_tasks, &self.command_tasks], move || {
            ready.store(false, Ordering::Release);
        });

        let stream = PrivateStream {
            config: self.config.clone(),
            http: self.http.clone(),
            emitter: self.emitter.clone(),
            data_sender: try_get_data_event_sender(),
            state: Arc::clone(&self.private),
            definitions: self.definitions.borrow().clone(),
            instruments: self
                .core
                .cache()
                .instruments(&KALSHI_VENUE, None)
                .iter()
                .map(|instrument| instrument.id())
                .collect(),
            ready: Arc::clone(&self.ready),
            ws_client: ws_client.clone(),
        };

        self.session_tasks.spawn(stream.run(
            receiver,
            pending,
            self.session_tasks.cancellation_token(),
            startup,
        ))?;
        self.refresh = Some(requests);
        self.ws_client = Some(ws_client.clone());
        let result = async {
            ws_client.connect().await?;
            nautilus_common::live::dst::time::timeout(
                Duration::from_secs(self.config.ws_timeout_secs),
                initialized,
            )
            .await
            .context("Kalshi bootstrap timed out")?
            .context("Kalshi bootstrap stopped")??;
            nautilus_common::live::dst::time::timeout(
                Duration::from_secs(self.config.ws_timeout_secs),
                async {
                    loop {
                        let session = self
                            .private
                            .lock()
                            .expect("private state lock is not poisoned")
                            .session
                            .clone();

                        if let Some(AccountAny::Cash(account)) = self.get_account() {
                            anyhow::ensure!(
                                !account.base.calculate_account_state && !account.allow_borrowing,
                                "Kalshi bootstrap account policy mismatch"
                            );

                            if account
                                .base
                                .base_last_event()
                                .and_then(|event| event.info)
                                .is_some_and(|info| {
                                    info.get("kalshi_session")
                                        .and_then(serde_json::Value::as_str)
                                        == Some(session.as_str())
                                })
                            {
                                break;
                            }
                        }

                        nautilus_common::live::dst::time::sleep(Duration::from_millis(10)).await;
                    }

                    Ok::<_, anyhow::Error>(())
                },
            )
            .await
            .context("Kalshi bootstrap account observation timed out")??;

            Ok::<_, anyhow::Error>(())
        }
        .await;

        if let Err(e) = result {
            drop(guard);
            self.stop()?;
            self.finish_shutdown().await?;
            return Err(e);
        }

        self.core.set_started();
        self.core.set_connected();
        guard.disarm();
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        self.stop()?;
        self.finish_shutdown().await?;
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        let mut order = self.submission_order(&cmd)?;
        if order.order_list_id().is_some() {
            self.private
                .lock()
                .expect("private state lock is not poisoned")
                .deny_submission(
                    &mut order,
                    "VALIDATION_FAILED: unsupported Kalshi limit instruction combination",
                    &self.emitter,
                )?;
            return Ok(());
        }

        if let Some(submission) = self.submission(order, &cmd)?
            && let Err(e) = self.command_tasks.spawn(submission.run())
        {
            log::warn!("Kalshi submission task was not registered: {e}");
        }

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        cmd.order_list.validate()?;
        anyhow::ensure!(
            cmd.order_list.strategy_id == cmd.strategy_id
                && cmd.instrument_id == cmd.order_list.instrument_id
                && cmd.exec_algorithm_id.is_none()
                && cmd.order_inits.len() == cmd.order_list.client_order_ids.len(),
            "Kalshi order-list identity mismatch"
        );

        if !self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .batches
            .insert(cmd.command_id)
        {
            return Ok(());
        }

        let mut children = Vec::new();

        for (id, init) in cmd.order_list.client_order_ids.iter().zip(&cmd.order_inits) {
            anyhow::ensure!(
                *id == init.client_order_id,
                "Kalshi order-list child identity mismatch"
            );

            let child = SubmitOrder::new(
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                init.instrument_id,
                *id,
                init.clone(),
                None,
                cmd.position_id,
                cmd.params.clone(),
                cmd.command_id,
                cmd.ts_init,
                cmd.correlation_id,
            );

            let order = self.submission_order(&child)?;
            children.push((order, child));
        }

        let mut submissions = Vec::new();

        let prepared = (|| -> anyhow::Result<()> {
            for (order, child) in &children {
                submissions.push(
                    self.submission(order.clone(), child)?
                        .context("Kalshi order-list child failed validation")?,
                );
            }

            Ok(())
        })();

        if let Err(e) = prepared {
            let reason = format!("VALIDATION_FAILED: {e}");
            for child in &mut submissions {
                child.deny(&reason);
            }

            drop(submissions);
            let mut state = self
                .private
                .lock()
                .expect("private state lock is not poisoned");

            for (mut order, _) in children {
                if !state.orders.contains_key(&order.client_order_id()) {
                    state.deny_submission(&mut order, &reason, &self.emitter)?;
                }
            }

            return Ok(());
        }

        if let Err(e) = self
            .command_tasks
            .spawn(batch::SubmissionBatch { submissions }.run())
        {
            log::warn!("Kalshi order-list task was not registered: {e}");
        }

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.mutate(&Command::Modify(cmd))
    }

    fn batch_modify_orders(&self, _cmd: BatchModifyOrders) -> anyhow::Result<()> {
        anyhow::bail!("Kalshi batch modification is not available")
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.mutate(&Command::Cancel(cmd))
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let command_id = cmd.causation_id.unwrap_or(cmd.command_id);
        anyhow::ensure!(
            cmd.trader_id == self.core.trader_id
                && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                && cmd.params.as_ref().is_none_or(|params| params.is_empty()),
            "Kalshi cancel-all identity or parameters mismatch"
        );
        let cache = self.core.cache();
        let state = self
            .private
            .lock()
            .expect("private state lock is not poisoned");

        if state.batches.contains(&command_id) {
            return Ok(());
        }

        let mut cancels = Vec::new();

        for cached in cache.orders(
            Some(&KALSHI_VENUE),
            Some(&cmd.instrument_id),
            Some(&cmd.strategy_id),
            None,
            cmd.order_side,
        ) {
            let id = cached.client_order_id();
            let cached = cached.cloned();
            let order = Self::claimed_order(&state, &cached);

            if order.is_closed()
                || order.is_emulated()
                || order.exec_algorithm_id().is_some()
                || cache.client_id(&id) != Some(&self.core.client_id)
                || order
                    .account_id()
                    .is_some_and(|id| id != self.core.account_id)
                || order.trader_id() != cmd.trader_id
                || order.strategy_id() != cmd.strategy_id
                || order.instrument_id() != cmd.instrument_id
                || cmd
                    .order_side
                    .is_some_and(|side| side != order.order_side())
                || !state.orders.contains_key(&id) && !state.submissions.contains(&id)
            {
                continue;
            }

            let cancel = CancelOrder::new(
                cmd.trader_id,
                Some(self.core.client_id),
                cmd.strategy_id,
                order.instrument_id(),
                id,
                order.venue_order_id(),
                UUID4::new(),
                cmd.ts_init,
                cmd.params.clone(),
                cmd.correlation_id.or(Some(cmd.command_id)),
            );

            cancels.push(cancel);
        }

        drop(state);
        drop(cache);
        cancels.sort_by_key(|child| child.client_order_id);
        self.cancel_batch(cancels, command_id)
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.trader_id == self.core.trader_id
                && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                && cmd.params.as_ref().is_none_or(|params| params.is_empty())
                && cmd
                    .cancels
                    .iter()
                    .all(|child| child.trader_id == cmd.trader_id
                        && child.strategy_id == cmd.strategy_id
                        && child.instrument_id == cmd.instrument_id),
            "Kalshi batch cancellation identity or parameters mismatch"
        );
        self.cancel_batch(cmd.cancels, cmd.command_id)
    }

    fn query_account(&self, cmd: QueryAccount) -> anyhow::Result<()> {
        anyhow::ensure!(
            cmd.account_id == self.core.account_id
                && cmd.trader_id == self.core.trader_id
                && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                && cmd.params.is_none(),
            "Kalshi account query identity or parameters mismatch"
        );
        self.refresh
            .as_ref()
            .context("Kalshi account bootstrap is not active")?
            .send(())
            .map_err(|_| anyhow::anyhow!("Kalshi account refresh stopped"))
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        anyhow::ensure!(
            cmd.trader_id == self.core.trader_id
                && cmd.strategy_id == order.strategy_id()
                && cmd.instrument_id == order.instrument_id()
                && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                && cmd
                    .venue_order_id
                    .zip(order.venue_order_id())
                    .is_none_or(|(requested, actual)| requested == actual)
                && order
                    .account_id()
                    .is_none_or(|id| id == self.core.account_id)
                && cmd.params.is_none()
                && self.core.cache().client_id(&cmd.client_order_id) == Some(&self.core.client_id),
            "Kalshi order query identity or parameters mismatch"
        );
        let pending = !self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .mutations
            .is_empty();

        if !pending {
            self.ready.store(false, Ordering::Release);
            self.restore_cache()?;
        }

        self.refresh
            .as_ref()
            .context("Kalshi account bootstrap is not active")?
            .send(())
            .map_err(|_| anyhow::anyhow!("Kalshi private recovery stopped"))
    }

    async fn generate_order_status_report(
        &self,
        cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        let max_pages = validate_request(cmd.params.as_ref(), None, None)?;
        anyhow::ensure!(
            cmd.client_order_id.is_some() || cmd.venue_order_id.is_some(),
            "targeted order report requires an order identity"
        );

        if let Some(id) = cmd.instrument_id {
            parse_ticker(id)?;
        }

        let ts_init = self.clock.get_time_ns();
        let records =
            collect_orders(&self.http, self.config.exchange_index, None, max_pages).await?;

        let mut matches = Vec::new();

        for value in records.values() {
            let client_order_id = self.client_order_id(value)?;
            if cmd
                .venue_order_id
                .is_some_and(|id| id.as_str() == value.order_id)
                || cmd
                    .client_order_id
                    .is_some_and(|id| Some(id) == client_order_id)
            {
                matches.push((value, client_order_id));
            }
        }

        if matches.is_empty() {
            return Ok(None);
        }

        anyhow::ensure!(
            matches.len() == 1,
            "targeted order identities are ambiguous"
        );
        let (value, client_order_id) = matches[0];
        anyhow::ensure!(
            cmd.venue_order_id
                .is_none_or(|id| id.as_str() == value.order_id)
                && cmd
                    .client_order_id
                    .is_none_or(|id| Some(id) == client_order_id)
                && cmd.instrument_id.is_none_or(
                    |id| parse_instrument_id(&value.ticker).is_ok_and(|actual| actual == id)
                ),
            "targeted order identity mismatch"
        );
        let report = self
            .order_report(value, ts_init)
            .await?
            .context("order command metadata or canceled total is not authoritative")?;
        self.admit_reports(std::slice::from_ref(&report), &[])?;
        Ok(Some(report))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        let max_pages = validate_request(cmd.params.as_ref(), cmd.start, cmd.end)?;
        if let Some(id) = cmd.instrument_id {
            parse_ticker(id)?;
        }

        let ts_init = self.clock.get_time_ns();
        let records = collect_orders(
            &self.http,
            self.config.exchange_index,
            cmd.instrument_id.as_ref().map(|id| id.symbol.as_str()),
            max_pages,
        )
        .await?;
        let mut reports = Vec::new();
        let mut omitted = 0;

        for value in records.values() {
            if cmd
                .instrument_id
                .is_some_and(|id| id.symbol.as_str() != value.ticker)
                || cmd.open_only && value.status != VenueOrderStatus::Resting
            {
                continue;
            }

            if value.status != VenueOrderStatus::Resting
                && !order_in_window(value, cmd.start, cmd.end)?
            {
                continue;
            }

            match self
                .order_report_aggregate(value, ts_init, max_pages, None)
                .await?
            {
                Some(report) => reports.push(report),
                None => omitted += 1,
            }
        }

        if omitted > 0 {
            log::warn!(
                "Kalshi bulk order reports omit {omitted} unfilled external order(s), command metadata unavailable"
            );
        }

        reports.sort_by_key(|report| report.venue_order_id);
        self.admit_reports(&reports, &[])?;
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        let max_pages = validate_request(cmd.params.as_ref(), cmd.start, cmd.end)?;
        if let Some(id) = cmd.instrument_id {
            parse_ticker(id)?;
        }

        let ts_init = self.clock.get_time_ns();
        let cutoff = self.http.get_historical_cutoff().await?;
        let ticker = cmd.instrument_id.as_ref().map(|id| id.symbol.as_str());
        let orders =
            collect_orders(&self.http, self.config.exchange_index, ticker, max_pages).await?;

        let query = FillsQuery {
            ticker: ticker.map(String::from),
            order_id: cmd.venue_order_id.map(|id| id.to_string()),
            exchange_index: self.config.exchange_index,
            ..Default::default()
        };

        let fills =
            collect_fills(&self.http, &query, cmd.start, ts_init, &cutoff, max_pages).await?;
        let mut reports = Vec::new();

        for value in &fills {
            if cmd
                .instrument_id
                .is_some_and(|id| id.symbol.as_str() != value.ticker)
                || cmd
                    .venue_order_id
                    .is_some_and(|id| id.as_str() != value.order_id)
            {
                continue;
            }

            let ts_event = timestamp(&value.created_time)?;

            if cmd.end.is_some_and(|end| ts_event > end) {
                continue;
            }

            let order = orders
                .get(value.order_id.as_str())
                .context("fill lacks order evidence")?;
            reports.push(self.fill_report(value, order, ts_init).await?);
        }

        let selected_orders = orders.values().filter(|order| {
            cmd.instrument_id
                .is_none_or(|id| id.symbol.as_str() == order.ticker)
                && cmd
                    .venue_order_id
                    .is_none_or(|id| id.as_str() == order.order_id)
        });

        validate_fill_coverage(selected_orders, &reports, cmd.start, cmd.end)?;
        self.admit_reports(&[], &reports)?;
        Ok(reports)
    }

    async fn generate_position_status_reports(
        &self,
        cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        let max_pages = validate_request(cmd.params.as_ref(), cmd.start, cmd.end)?;
        if let Some(id) = cmd.instrument_id {
            parse_ticker(id)?;
        }

        let ts_init = self.clock.get_time_ns();
        let positions = collect_positions_current(
            &self.http,
            self.config.exchange_index,
            cmd.instrument_id.as_ref().map(|id| id.symbol.as_str()),
            max_pages,
        )
        .await?;
        let mut reports = Vec::new();

        for value in &positions {
            if cmd
                .instrument_id
                .is_some_and(|id| id.symbol.as_str() != value.ticker)
            {
                continue;
            }

            let definition = self.definition(&value.ticker, ts_init).await?;
            let report = reports::position(value, &definition, self.core.account_id, ts_init)?;
            if cmd.start.is_none_or(|start| report.ts_last >= start)
                && cmd.end.is_none_or(|end| report.ts_last <= end)
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
        let requested_at = self.clock.get_time_ns();
        let start = lookback_mins
            .map(DurationNanos::try_from_mins)
            .transpose()?
            .map(|duration| requested_at.saturating_sub(duration));
        let max_pages = 100;
        let snapshot = collect_snapshot_at(
            &self.http,
            self.config.exchange_index,
            start,
            max_pages,
            requested_at,
        )
        .await?;
        let mut orders = Vec::new();
        let mut omitted = 0;

        for value in &snapshot.orders {
            anyhow::ensure!(
                timestamp(&value.last_update_time)? <= snapshot.ts_init,
                "order changed after mass collection began"
            );

            match self
                .order_report_aggregate(value, snapshot.ts_init, max_pages, Some(&snapshot.cutoff))
                .await?
            {
                Some(report) => orders.push(report),
                None => omitted += 1,
            }
        }

        let source_orders: HashMap<_, _> = snapshot
            .orders
            .iter()
            .map(|order| (order.order_id.as_str(), order))
            .collect();
        let mut fills = Vec::new();

        for value in &snapshot.fills {
            let source = source_orders
                .get(value.order_id.as_str())
                .context("fill lacks order evidence")?;
            let report = self.fill_report(value, source, snapshot.ts_init).await?;
            fills.push(report);
        }

        validate_fill_coverage(
            snapshot.orders.iter(),
            &fills,
            start,
            Some(snapshot.ts_init),
        )?;

        let mut positions = Vec::new();

        for value in &snapshot.positions {
            let definition = self.definition(&value.ticker, snapshot.ts_init).await?;
            let report =
                reports::position(value, &definition, self.core.account_id, snapshot.ts_init)?;
            anyhow::ensure!(
                report.ts_last <= snapshot.ts_init,
                "position changed after mass collection began"
            );
            positions.push(report);
        }

        let mut mass = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *KALSHI_VENUE,
            snapshot.ts_init,
            None,
        );

        self.admit_reports(&orders, &fills)?;

        if omitted > 0 {
            log::warn!(
                "Kalshi mass status omits {omitted} unfilled external order(s), reports are incomplete"
            );
        }

        mass.set_report_window(start, omitted == 0);
        mass.add_order_reports(orders);
        mass.add_fill_reports(fills);
        mass.add_position_reports(positions);
        Ok(Some(mass))
    }
}

fn position_qty(
    cache: &Cache,
    order: &OrderAny,
    requested: Option<PositionId>,
    account_id: AccountId,
) -> Result<Option<Decimal>, OrderDeniedReason> {
    let expected = PositionId::new(format!("{}-{}", order.instrument_id(), order.strategy_id()));

    let validated = (|| -> anyhow::Result<_> {
        anyhow::ensure!(
            requested.is_none_or(|id| id == expected)
                && order.position_id().is_none_or(|id| id == expected)
                && cache
                    .position_id(&order.client_order_id())
                    .is_none_or(|id| *id == expected),
            "Kalshi NETTING position binding must match the submitting strategy"
        );

        if !order.is_reduce_only() {
            return Ok(None);
        }

        let position = cache
            .position_ref(&expected)
            .filter(|position| position.is_open());

        let Some(position) = position else {
            return Ok(None);
        };

        anyhow::ensure!(
            position.instrument_id == order.instrument_id()
                && position.account_id == account_id
                && position.strategy_id == order.strategy_id()
                && cache.oms_type(&expected) == Some(OmsType::Netting),
            "Kalshi canonical position scope mismatch"
        );
        Ok(Some(position.signed_decimal_qty()))
    })();

    validated.map_err(|e| OrderDeniedReason::ValidationFailed {
        detail: e.to_string(),
    })
}

fn validate_request(
    params: Option<&Params>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> anyhow::Result<u32> {
    anyhow::ensure!(
        start.zip(end).is_none_or(|(start, end)| start <= end),
        "invalid report time window"
    );

    let Some(params) = params else {
        return Ok(100);
    };

    anyhow::ensure!(
        params.keys().all(|key| key == "max_pages"),
        "unsupported Kalshi report parameter"
    );
    let pages = params
        .get("max_pages")
        .map(|value| value.as_u64().context("invalid report page budget"))
        .transpose()?
        .unwrap_or(100);
    let pages = u32::try_from(pages).context("invalid report page budget")?;
    anyhow::ensure!(pages > 0, "invalid report page budget");
    Ok(pages)
}

fn order_in_window(
    value: &VenueOrder,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> anyhow::Result<bool> {
    let last = timestamp(&value.last_update_time)?;
    Ok(start.is_none_or(|start| last >= start) && end.is_none_or(|end| last <= end))
}

fn validate_fill_coverage<'a>(
    orders: impl Iterator<Item = &'a VenueOrder>,
    fills: &[FillReport],
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> anyhow::Result<()> {
    let mut fill_counts = HashMap::new();
    for report in fills {
        let count = fill_counts
            .entry(report.venue_order_id)
            .or_insert(Decimal::ZERO);
        *count = count
            .checked_add(report.last_qty.as_decimal())
            .context("fill count overflow")?;
    }

    for order in orders {
        let id = VenueOrderId::new_checked(&order.order_id)?;
        let count = fill_counts.get(&id).copied().unwrap_or(Decimal::ZERO);
        anyhow::ensure!(
            count <= order.fill_count_fp.0,
            "collected fills exceed order filled count"
        );
        let created = timestamp(&order.created_time)?;
        let updated = timestamp(&order.last_update_time)?;
        anyhow::ensure!(created <= updated, "order timestamps out of sequence");

        if start.is_none_or(|start| created >= start) && end.is_none_or(|end| updated <= end) {
            anyhow::ensure!(
                count == order.fill_count_fp.0,
                "order fills are not yet completely visible"
            );
        }
    }

    Ok(())
}

impl KalshiExecutionClient {
    fn submission_order(&self, cmd: &SubmitOrder) -> anyhow::Result<OrderAny> {
        let order = self.core.get_order(&cmd.client_order_id)?;
        anyhow::ensure!(
            cmd.trader_id == self.core.trader_id
                && cmd.trader_id == order.trader_id()
                && cmd.strategy_id == order.strategy_id()
                && cmd.instrument_id == order.instrument_id()
                && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                && cmd.order_init == OrderInitialized::from(&order)
                && cmd.exec_algorithm_id == order.exec_algorithm_id()
                && order
                    .account_id()
                    .is_none_or(|id| id == self.core.account_id)
                && self.core.cache().client_id(&cmd.client_order_id) == Some(&self.core.client_id)
                && order.status() == OrderStatus::Initialized,
            "Kalshi submission command or cached identity mismatch"
        );

        Ok(order)
    }

    fn submission(
        &self,
        mut order: OrderAny,
        cmd: &SubmitOrder,
    ) -> anyhow::Result<Option<submit::Submission>> {
        let session = {
            let mut state = self
                .private
                .lock()
                .expect("private state lock is not poisoned");
            anyhow::ensure!(
                !state.orders.contains_key(&cmd.client_order_id)
                    && state.submissions.insert(cmd.client_order_id),
                "Kalshi submission already claimed"
            );
            state.session.clone()
        };

        let request = match submit::request(&order, cmd.params.as_ref(), self.config.exchange_index)
        {
            Ok(request) => request,
            Err(reason) => {
                self.private
                    .lock()
                    .expect("private state lock is not poisoned")
                    .deny_submission(&mut order, &reason.to_string(), &self.emitter)?;
                return Ok(None);
            }
        };

        // Admission's session check fences this cache snapshot across async preparation
        let position_qty = match position_qty(
            &self.core.cache(),
            &order,
            cmd.position_id,
            self.core.account_id,
        ) {
            Ok(qty) => qty,
            Err(reason) => {
                self.private
                    .lock()
                    .expect("private state lock is not poisoned")
                    .deny_submission(&mut order, &reason.to_string(), &self.emitter)?;
                return Ok(None);
            }
        };

        anyhow::ensure!(
            self.is_connected(),
            "Kalshi submission requires current-session readiness"
        );

        let definition = match self
            .core
            .cache()
            .instrument(&order.instrument_id())
            .cloned()
        {
            Some(InstrumentAny::BinaryOption(definition)) => definition,
            _ => anyhow::bail!("Kalshi submission requires a cached binary instrument"),
        };

        validate_definition(&definition, self.config.exchange_index)?;
        let ws_client = self
            .ws_client
            .clone()
            .context("Kalshi private transport missing")?;
        let refresh = self
            .refresh
            .clone()
            .context("Kalshi private recovery missing")?;

        Ok(Some(submit::Submission {
            order,
            position_qty,
            definition,
            request,
            http: self.http.clone(),
            state: Arc::clone(&self.private),
            emitter: self.emitter.clone(),
            ready: Arc::clone(&self.ready),
            ws_client,
            refresh,
            session,
            cancellation: self.command_tasks.cancellation_token(),
        }))
    }

    fn cancel_batch(&self, cancels: Vec<CancelOrder>, command_id: UUID4) -> anyhow::Result<()> {
        if self
            .private
            .lock()
            .expect("private state lock is not poisoned")
            .batches
            .contains(&command_id)
        {
            return Ok(());
        }

        let mut selected = Vec::new();
        let mut identities = HashSet::new();

        for cmd in cancels {
            anyhow::ensure!(
                identities.insert(cmd.client_order_id),
                "duplicate Kalshi cancellation identity"
            );
            let cached = self.core.get_order(&cmd.client_order_id)?;
            anyhow::ensure!(
                cmd.trader_id == self.core.trader_id
                    && cmd.trader_id == cached.trader_id()
                    && cmd.strategy_id == cached.strategy_id()
                    && cmd.instrument_id == cached.instrument_id()
                    && cmd.client_id.is_none_or(|id| id == self.core.client_id)
                    && cached
                        .account_id()
                        .is_none_or(|id| id == self.core.account_id)
                    && self.core.cache().client_id(&cmd.client_order_id)
                        == Some(&self.core.client_id)
                    && cmd.params.as_ref().is_none_or(|params| params.is_empty())
                    && cmd
                        .venue_order_id
                        .zip(cached.venue_order_id())
                        .is_none_or(|(requested, actual)| requested == actual)
                    && !cached.is_emulated()
                    && cached.exec_algorithm_id().is_none(),
                "Kalshi batch cancellation command or cached identity mismatch"
            );
            selected.push((Command::Cancel(cmd), cached));
        }

        self.private
            .lock()
            .expect("private state lock is not poisoned")
            .batches
            .insert(command_id);
        let mut mutations = Vec::new();

        for (command, cached) in selected {
            {
                let state = self
                    .private
                    .lock()
                    .expect("private state lock is not poisoned");

                if state.mutations_dispatched.contains(&command.id()) {
                    continue;
                }

                if let Some(pending) = state.mutations.get(&command.client_order_id()) {
                    if pending.command_id == command.id() {
                        continue;
                    }

                    let event = command.outcome(
                        &cached,
                        "VALIDATION_FAILED: Kalshi order change is already in flight",
                        true,
                        &self.emitter,
                    );
                    self.emitter.try_send_order_event(event)?;
                    continue;
                }
            }

            mutations.push(self.prepare_change(&command, &cached)?);
        }

        if !mutations.is_empty() {
            let epoch = Some(mutations[0].epoch);

            if let Err(e) = self
                .command_tasks
                .spawn(batch::CancellationBatch { mutations, epoch }.run())
            {
                log::warn!("Kalshi batch cancellation task was not registered: {e}");
            }
        }

        Ok(())
    }

    fn mutate(&self, command: &Command) -> anyhow::Result<()> {
        let id = command.client_order_id();
        let cached = self.core.get_order(&id)?;

        let (trader_id, client_id, strategy_id, instrument_id, venue_order_id) = match &command {
            Command::Modify(cmd) => (
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.venue_order_id,
            ),
            Command::Cancel(cmd) => (
                cmd.trader_id,
                cmd.client_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.venue_order_id,
            ),
        };

        anyhow::ensure!(
            trader_id == cached.trader_id()
                && strategy_id == cached.strategy_id()
                && instrument_id == cached.instrument_id()
                && client_id.is_none_or(|id| id == self.core.client_id)
                && cached.trader_id() == self.core.trader_id
                && cached.account_id() == Some(self.core.account_id)
                && self.core.cache().client_id(&id) == Some(&self.core.client_id),
            "Kalshi change command or cached identity mismatch"
        );

        let pending = {
            let state = self
                .private
                .lock()
                .expect("private state lock is not poisoned");

            if state.mutations_dispatched.contains(&command.id()) {
                return Ok(());
            }

            state.mutations.get(&id).map(|pending| pending.command_id)
        };

        if pending == Some(command.id()) {
            return Ok(());
        }

        let prepared = if pending.is_some() {
            Err(anyhow::anyhow!("Kalshi order change is already in flight"))
        } else if venue_order_id.is_some_and(|id| Some(id) != cached.venue_order_id()) {
            Err(anyhow::anyhow!("Kalshi change venue identity mismatch"))
        } else if cached.is_closed() {
            Err(anyhow::anyhow!("Kalshi order is already closed"))
        } else {
            self.prepare_change(command, &cached)
        };

        let mutation = match prepared {
            Ok(mutation) => mutation,
            Err(e) => {
                let event = command.outcome(
                    &cached,
                    &format!("VALIDATION_FAILED: {e}"),
                    true,
                    &self.emitter,
                );
                self.emitter.try_send_order_event(event)?;
                return Ok(());
            }
        };

        if let Err(e) = self.command_tasks.spawn(mutation.run()) {
            log::warn!("Kalshi change task was not registered: {e}");
        }

        Ok(())
    }

    fn prepare_change(&self, command: &Command, cached: &OrderAny) -> anyhow::Result<Mutation> {
        let deadline = Instant::now() + ADMISSION_TIMEOUT;
        self.observe_settlement();
        let id = command.client_order_id();

        let definition = match self
            .core
            .cache()
            .instrument(&cached.instrument_id())
            .cloned()
        {
            Some(InstrumentAny::BinaryOption(definition)) => definition,
            _ => anyhow::bail!("Kalshi change requires a cached binary instrument"),
        };

        validate_definition(&definition, self.config.exchange_index)?;
        let ws_client = self
            .ws_client
            .clone()
            .context("Kalshi private transport missing")?;
        let refresh = self
            .refresh
            .clone()
            .context("Kalshi private recovery missing")?;
        let epoch = ws_client
            .connection_epoch()
            .context("Kalshi private transport is not active")?;

        anyhow::ensure!(
            self.core.is_connected() && ws_client.is_active(),
            "Kalshi change requires an active execution connection"
        );

        let (order, session) = {
            let mut state = self
                .private
                .lock()
                .expect("private state lock is not poisoned");

            let order = if !state.orders.contains_key(&id) && state.submissions.contains(&id) {
                cached.clone()
            } else {
                anyhow::ensure!(
                    state.orders.contains_key(&id),
                    "Kalshi change requires recovered order context"
                );
                Self::claimed_order(&state, cached)
            };

            anyhow::ensure!(
                order.strategy_id() == cached.strategy_id()
                    && order.instrument_id() == cached.instrument_id()
                    && (order.venue_order_id() == cached.venue_order_id()
                        || cached.venue_order_id().is_none()),
                "Kalshi change private identity mismatch"
            );
            state.mutations.insert(
                id,
                PendingMutation {
                    command_id: command.id(),
                    started: self.clock.get_time_ns(),
                    dispatched: false,
                    acknowledgement: None,
                    terms: None,
                },
            );

            (order, state.session.clone())
        };

        let mutation = Mutation {
            command: command.clone(),
            order,
            definition,
            shard: self.config.exchange_index,
            http: self.http.clone(),
            state: Arc::clone(&self.private),
            emitter: self.emitter.clone(),
            ready: Arc::clone(&self.ready),
            ws_client,
            epoch,
            deadline,
            refresh,
            session,
            cancellation: self.command_tasks.cancellation_token(),
        };

        Ok(mutation)
    }

    fn claimed_order(state: &PrivateState, cached: &OrderAny) -> OrderAny {
        let order = state
            .orders
            .get(&cached.client_order_id())
            .map_or_else(|| cached.clone(), |context| context.order.clone());
        if order.strategy_id().is_external() && order.strategy_id() != cached.strategy_id() {
            return cached.clone();
        }

        order
    }
}

#[cfg(test)]
mod tests {
    #[cfg(not(all(feature = "simulation", madsim)))]
    use futures_util::StreamExt;
    use nautilus_common::{cache::Cache, messages::ExecutionEvent};
    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_model::events::AccountState;
    use nautilus_model::{
        enums::{OrderType, TimeInForce},
        events::OrderEventAny,
        orders::builder::OrderTestBuilder,
    };

    use super::*;

    #[allow(dead_code, reason = "shared key fixtures serve multiple suites")]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    #[rstest::rstest]
    fn reset_preserves_unconsumed_submission_identity_and_funds() {
        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        nautilus_common::live::runner::set_exec_event_sender(sender);
        let cache = Rc::new(RefCell::new(Cache::default()));

        let config = KalshiExecutionClientConfig {
            api_key_id: Some("reset-test".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            ..Default::default()
        };

        let core = ExecutionClientCore::new(
            "TRADER-001".into(),
            "KALSHI".into(),
            *KALSHI_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            Some(Currency::USD()),
            Rc::clone(&cache),
        );

        let mut client = KalshiExecutionClient::new(core, config).unwrap();
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("RESET.KALSHI".into())
            .client_order_id("reset-order".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let id = order.client_order_id();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client.client_id()), false)
            .unwrap();
        {
            let mut state = client.private.lock().unwrap();
            let ticket = state.ledger.begin_refresh();
            state
                .ledger
                .apply_refresh(ticket, Decimal::from(3), HashMap::new())
                .unwrap();
            state
                .submit(&mut order, Decimal::new(7845, 4), None, &client.emitter)
                .unwrap();
        }

        let balance = client.private.lock().unwrap().ledger.balance().unwrap();
        client.reset().unwrap();
        assert_eq!(
            cache.borrow().order(&id).unwrap().status(),
            OrderStatus::Initialized
        );
        let mut state = client.private.lock().unwrap();
        assert_eq!(state.orders[&id].order.status(), OrderStatus::Submitted);
        assert_eq!(state.orders[&id].order.client_order_id(), id);
        assert_eq!(state.ledger.balance().unwrap(), balance);
        assert_eq!(
            state
                .ledger
                .reserve(id, Decimal::new(7845, 4))
                .unwrap_err()
                .to_string(),
            "Kalshi command already owns funds"
        );
        assert!(matches!(
            output.try_recv().unwrap(),
            ExecutionEvent::Account(_)
        ));
        assert!(matches!(
            output.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Submitted(_))
        ));
        assert!(output.try_recv().is_err());
    }

    #[rstest::rstest]
    #[case::ordinary(None, false)]
    #[case::unsent(Some(false), false)]
    #[case::unsent_other_order(Some(false), true)]
    #[case::dispatched(Some(true), false)]
    fn order_query_preserves_recovery_for_pending_native_changes(
        #[case] dispatched: Option<bool>,
        #[case] other_order: bool,
    ) {
        let cache = Rc::new(RefCell::new(Cache::default()));

        let config = KalshiExecutionClientConfig {
            api_key_id: Some("query-test".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            ..Default::default()
        };

        let core = ExecutionClientCore::new(
            "TRADER-001".into(),
            "KALSHI".into(),
            *KALSHI_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Cash,
            Some(Currency::USD()),
            Rc::clone(&cache),
        );

        let mut client = KalshiExecutionClient::new(core, config).unwrap();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("QUERY.KALSHI".into())
            .client_order_id("query-order".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let other = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("QUERY.KALSHI".into())
            .client_order_id("other-order".into())
            .quantity(Quantity::from("2.50"))
            .price(Price::from("0.700000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let id = order.client_order_id();
        cache
            .borrow_mut()
            .add_order(order.clone(), None, Some(client.client_id()), false)
            .unwrap();
        cache
            .borrow_mut()
            .add_order(other.clone(), None, Some(client.client_id()), false)
            .unwrap();
        let (refresh, mut requests) = tokio::sync::mpsc::unbounded_channel();
        client.refresh = Some(refresh);
        client
            .ready
            .store(dispatched != Some(true), Ordering::Release);

        let (ticket, session) = {
            let mut state = client.private.lock().unwrap();
            state.restore(order.clone());
            let other_id = other.client_order_id();
            state.restore(other);
            state.epoch = Some((3, 4));
            if let Some(dispatched) = dispatched {
                state.mutations.insert(
                    if other_order { other_id } else { id },
                    PendingMutation {
                        command_id: UUID4::new(),
                        terms: None,
                        acknowledgement: None,
                        started: order.ts_last(),
                        dispatched,
                    },
                );
            }

            (state.ledger.begin_refresh(), state.session.clone())
        };

        client
            .query_order(QueryOrder::new(
                client.core.trader_id,
                Some(client.client_id()),
                order.strategy_id(),
                order.instrument_id(),
                id,
                None,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();

        let state = client.private.lock().unwrap();
        let pending = dispatched.is_some();
        assert_eq!(state.ledger.check_refresh(ticket).is_ok(), pending);
        assert_eq!(state.session == session, pending);
        assert_eq!(state.epoch, pending.then_some((3, 4)));
        assert_eq!(
            client.ready.load(Ordering::Acquire),
            dispatched == Some(false)
        );
        assert_eq!(requests.try_recv(), Ok(()));
        assert!(requests.try_recv().is_err());
        assert_eq!(state.orders[&id].order.quantity(), Quantity::from("1.25"));
        assert_eq!(
            state.orders[&id].order.price(),
            Some(Price::from("0.600000"))
        );
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn connected_requires_the_transport_epoch_covered_by_account_recovery() {
        tokio::time::timeout(Duration::from_secs(10), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let address = listener.local_addr().unwrap();
            let peer = tokio::spawn(async move {
                let (stream, _) = listener.accept().await.unwrap();
                let mut ws_client = tokio_tungstenite::accept_async(stream).await.unwrap();
                while ws_client.next().await.is_some() {}
            });

            let config = KalshiExecutionClientConfig {
                api_key_id: Some("epoch-test".into()),
                private_key: Some(keys::rsa_pkcs8().into()),
                base_url_ws: Some(format!("ws://{address}/ws").into()),
                ..Default::default()
            };

            let cache = Rc::new(RefCell::new(Cache::default()));

            let core = ExecutionClientCore::new(
                "TRADER-001".into(),
                "KALSHI".into(),
                *KALSHI_VENUE,
                OmsType::Netting,
                config.account_id,
                AccountType::Cash,
                Some(Currency::USD()),
                Rc::clone(&cache),
            );

            let mut client = KalshiExecutionClient::new(core, config).unwrap();
            let (ws_client, _events) =
                KalshiWebSocketClient::new(client.transport.clone()).unwrap();
            ws_client.connect().await.unwrap();
            let epoch = ws_client.connection_epoch().unwrap();
            client.ws_client = Some(ws_client.clone());
            client.ready.store(true, Ordering::Release);
            client.core.set_connected();

            let session = {
                let mut state = client.private.lock().unwrap();
                state.epoch = Some(epoch);
                state.session.clone()
            };

            let info =
                Params::new([("kalshi_session".into(), serde_json::Value::String(session))].into());
            let zero = Money::zero(Currency::USD());
            let balance = AccountBalance::new(zero, zero, zero);
            let event = AccountState::new(
                client.account_id(),
                AccountType::Cash,
                vec![balance],
                Vec::new(),
                true,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(2),
                Some(Currency::USD()),
            )
            .with_info(Some(info));
            cache
                .borrow_mut()
                .add_account(AccountAny::from(event.clone()))
                .unwrap();
            assert!(client.is_connected());
            client.private.lock().unwrap().epoch = Some((epoch.0.wrapping_add(1), epoch.1));
            assert!(!client.is_connected());
            client.private.lock().unwrap().epoch = Some((epoch.0, epoch.1.wrapping_add(1)));
            assert!(!client.is_connected());
            client.private.lock().unwrap().epoch = None;
            assert!(!client.is_connected());
            client.private.lock().unwrap().epoch = Some(epoch);
            assert!(client.is_connected());
            let mut stale = event.clone();
            stale.info = Some(Params::new(
                [(
                    "kalshi_session".into(),
                    serde_json::Value::String("stale-session".into()),
                )]
                .into(),
            ));
            cache
                .borrow_mut()
                .add_account(AccountAny::from(stale))
                .unwrap();
            assert!(!client.is_connected());
            cache
                .borrow_mut()
                .add_account(AccountAny::from(event))
                .unwrap();
            assert!(client.is_connected());
            ws_client.stop().await.unwrap();
            assert!(!client.is_connected());
            peer.await.unwrap();
        })
        .await
        .unwrap();
    }
}
