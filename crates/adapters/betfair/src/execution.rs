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

//! Live execution client for the Betfair adapter.
//!
//! # Submission gate
//!
//! The client halts new-order submissions whenever the execution stream is unavailable or
//! recovering. `submit_order` and `submit_order_list` emit `OrderDenied` with
//! `STREAM_RECONCILING`, while cancel and modify commands remain available. Transport availability
//! alone does not reopen the gate: an active replacement socket remains halted until Betfair sends
//! a `Connection` message and the matching recovery task dispatches mass status.
//!
//! # Reconnect recovery
//!
//! A transport loss or server `connectionClosed` status advances the reconciliation generation
//! immediately. Each replacement `Connection` message advances it again and raises
//! `pending_resync`; subsequent OCMs remain buffered until `process_pending_resync` runs on the
//! engine thread. Connectivity polling and command or report entry points invoke it to synchronize
//! OCM state from the cache and drain the buffer.
//!
//! The recovery task attempts to refresh the session, requests account state on a best-effort basis,
//! and builds a mass status from `list_current_orders`. The mass status recovers fills that
//! completed and rolled off the unmatched book during the gap.
//!
//! After dispatching mass status, the task clears only the generation it handled; it does not wait
//! for a separate cache-application acknowledgement. A newer transport loss or replacement
//! connection therefore keeps submissions halted when an older task finishes. A failed full
//! re-login or mass-status request leaves the gate closed until a later reconnect succeeds or the
//! client disconnects. A keep-alive failure other than explicit `LoginFailed` continues recovery
//! with the retained session; the gate still reopens only after mass status is dispatched.
//!
//! # Modify reconciliation
//!
//! An ambiguous replace or quantity reduction remains pending until OCM or a fully paginated
//! `listCurrentOrders` response confirms it. Reconciliation emits the resulting `OrderUpdated`
//! directly and withholds reports that would reapply or contradict that update. For reductions,
//! active quantity is matched plus remaining, and the confirmed quantity overrides Betfair's
//! original stake in later reports, including terminal reports.

use std::{
    fmt,
    future::Future,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, Ordering},
    },
};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    clients::ExecutionClient,
    live::{
        get_runtime,
        runner::{get_data_event_sender, get_exec_event_sender},
        task::TaskHandles,
    },
    messages::{
        DataEvent, ExecutionReport,
        execution::{
            BatchCancelOrders, CancelAllOrders, CancelOrder, GenerateFillReports,
            GenerateOrderStatusReports, ModifyOrder, QueryOrder, SubmitOrder, SubmitOrderList,
        },
    },
};
use nautilus_core::{
    MUTEX_POISONED, Params, UUID4, UnixNanos,
    datetime::NANOSECONDS_IN_SECOND,
    time::{AtomicTime, get_atomic_clock_realtime},
};
use nautilus_live::{
    ExecutionClientCore, ExecutionEventEmitter, SocketControl, execution::failure::CommandFailure,
};
use nautilus_model::{
    accounts::AccountAny,
    data::Data,
    enums::{AccountType, LiquiditySide, OmsType, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderDeniedReason, OrderEventAny, OrderFillVoided,
        OrderFilled, OrderUpdated,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TradeId, Venue, VenueOrderId,
    },
    instruments::InstrumentAny,
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport},
    types::{AccountBalance, Currency, MarginBalance, Price, Quantity},
};
use nautilus_network::{SocketState, SocketStateSink};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;
use ustr::Ustr;

use crate::{
    common::{
        consts::{
            BETFAIR_VENUE, METHOD_CANCEL_ORDERS, METHOD_GET_ACCOUNT_FUNDS,
            METHOD_LIST_CURRENT_ORDERS, METHOD_PLACE_ORDERS, METHOD_REPLACE_ORDERS,
        },
        credential::BetfairCredential,
        enums::{
            BetfairOrderStatus, BetfairOrderType, BetfairSide, BetfairTimeInForce,
            ExecutionReportErrorCode, ExecutionReportStatus, InstructionReportErrorCode,
            InstructionReportStatus, OrderProjection, PersistenceType, StreamingOrderStatus,
            StreamingSide,
        },
        parse::{
            extract_market_id, extract_selection_id, make_customer_order_ref,
            make_customer_order_ref_legacy, make_instrument_id, parse_account_state,
            parse_betfair_price, parse_betfair_quantity, parse_betfair_timestamp,
            parse_millis_timestamp,
        },
        types::{BetId, OrderSyncEntry, SelectionId},
    },
    config::BetfairExecConfig,
    data::custom_data_with_instrument,
    data_types::{BetfairOrderVoided, register_betfair_custom_data},
    http::{
        client::BetfairHttpClient,
        error::BetfairHttpError,
        models::{
            AccountFundsResponse, CancelExecutionReport, CancelInstruction, CancelOrdersParams,
            CurrentOrderSummary, CurrentOrderSummaryReport, LimitOnCloseOrder, LimitOrder,
            ListCurrentOrdersParams, MarketOnCloseOrder, MarketVersion, PlaceExecutionReport,
            PlaceInstruction, PlaceInstructionReport, PlaceOrdersParams, ReplaceExecutionReport,
            ReplaceInstruction, ReplaceInstructionReport, ReplaceOrdersParams, TimeRange,
        },
        parse::{parse_current_order_fill_report, parse_current_order_report},
    },
    stream::{
        USER_STREAMS_ENDPOINT,
        client::{BetfairStreamClient, HeartbeatTimeoutSource, StreamMessageHandler},
        config::BetfairStreamConfig,
        messages::{OCM, OrderMarketChange, OrderRunnerChange, StreamMessage, UnmatchedOrder},
        ocm::OcmState,
        parse::{FillVoidAllocation, has_cancel_quantity, parse_order_status_report},
    },
};

/// Betfair live execution client.
#[derive(Debug)]
pub struct BetfairExecutionClient {
    core: ExecutionClientCore,
    clock: &'static AtomicTime,
    emitter: ExecutionEventEmitter,
    http_client: Arc<BetfairHttpClient>,
    stream_client: Option<Arc<BetfairStreamClient>>,
    socket_control: Option<SocketControl>,
    credential: BetfairCredential,
    stream_config: BetfairStreamConfig,
    config: BetfairExecConfig,
    currency: Currency,
    ocm_state: Arc<Mutex<OcmState>>,
    pending_resync: Arc<AtomicBool>,
    reconciliation_gate: Arc<ReconciliationGate>,
    replay_buffer: Arc<Mutex<Vec<ReceivedOcm>>>,
    pending_tasks: TaskHandles,
    keep_alive_handle: Option<JoinHandle<()>>,
    account_refresh_tx: Option<tokio::sync::mpsc::UnboundedSender<()>>,
    account_state_handle: Option<JoinHandle<()>>,
    reconnect_handle: Option<JoinHandle<()>>,
}

impl BetfairExecutionClient {
    /// Creates a new [`BetfairExecutionClient`] instance.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        http_client: BetfairHttpClient,
        credential: BetfairCredential,
        stream_config: BetfairStreamConfig,
        config: BetfairExecConfig,
        currency: Currency,
    ) -> Self {
        let clock = get_atomic_clock_realtime();
        let emitter = ExecutionEventEmitter::new(
            clock,
            core.trader_id,
            core.account_id,
            AccountType::Betting,
            None,
        );
        let socket_control = Some(SocketControl::new(
            core.client_id,
            Some(*BETFAIR_VENUE),
            USER_STREAMS_ENDPOINT,
        ));

        Self {
            core,
            clock,
            emitter,
            http_client: Arc::new(http_client),
            stream_client: None,
            socket_control,
            credential,
            stream_config,
            config,
            currency,
            ocm_state: Arc::new(Mutex::new(OcmState::default())),
            pending_resync: Arc::new(AtomicBool::new(false)),
            reconciliation_gate: Arc::new(ReconciliationGate::default()),
            replay_buffer: Arc::new(Mutex::new(Vec::new())),
            pending_tasks: TaskHandles::default(),
            keep_alive_handle: None,
            account_refresh_tx: None,
            account_state_handle: None,
            reconnect_handle: None,
        }
    }

    /// Returns true while new-order submissions are halted for stream recovery.
    #[must_use]
    pub fn is_reconciling(&self) -> bool {
        self.reconciliation_gate.is_halted()
    }

    fn submissions_halted(&self) -> bool {
        !self.core.is_connected()
            || self.reconciliation_gate.is_halted()
            || self
                .stream_client
                .as_ref()
                .is_none_or(|client| !client.is_active())
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

        self.pending_tasks.push(handle);
    }

    fn reconcile_market_ids(&self) -> Option<Vec<String>> {
        if self.config.reconcile_market_ids_only
            && let Some(ids) = &self.config.reconcile_market_ids
        {
            return Some(ids.clone());
        }
        self.config.stream_market_ids_filter.clone()
    }

    /// Returns the market version for price protection on order placement.
    ///
    /// When `use_market_version` is enabled, reads the `version` field from
    /// the instrument's `info` metadata. Betfair lapses orders submitted with
    /// a stale version rather than matching against a moved book.
    fn get_market_version(&self, instrument_id: &InstrumentId) -> Option<MarketVersion> {
        if !self.config.use_market_version {
            return None;
        }

        let cache = self.core.cache();
        let instrument = cache.instrument(instrument_id)?;

        if let InstrumentAny::Betting(betting) = instrument {
            let version = betting.info.as_ref()?.get_i64("version")?;
            return Some(MarketVersion {
                version: Some(version),
            });
        }

        None
    }

    /// Pre-populates OCM state from cached orders to prevent duplicate fills
    /// and terminal events after reconnect.
    fn sync_ocm_state_from_cache(&self) {
        let cache = self.core.cache();
        let venue = *BETFAIR_VENUE;
        let orders = cache.orders(Some(&venue), None, None, None, None);

        let order_data = orders
            .iter()
            .filter_map(|order| Self::order_sync_entry(order))
            .collect::<Vec<_>>();

        let mut state = self.ocm_state.lock().expect(MUTEX_POISONED);
        state.sync_from_orders(&order_data);
        Self::sync_cached_fills(&mut state, orders.iter().map(|order| &**order));

        log::debug!("Synced OCM state from {} cached orders", order_data.len());
    }

    fn order_sync_entry(order: &OrderAny) -> Option<OrderSyncEntry> {
        let venue_order_id = order.venue_order_id()?;
        let (filled_qty, avg_px) = Self::current_order_fill_state(order, venue_order_id);
        let trade_ids = order
            .trade_ids()
            .iter()
            .map(|trade_id| trade_id.to_string())
            .collect();
        Some(OrderSyncEntry {
            bet_id: venue_order_id.to_string(),
            client_order_id: order.client_order_id(),
            filled_qty,
            avg_px,
            is_closed: order.is_closed(),
            trade_ids,
        })
    }

    fn current_order_fill_state(
        order: &OrderAny,
        venue_order_id: VenueOrderId,
    ) -> (Decimal, Decimal) {
        let events = order.events();
        let current_fills = events.iter().filter_map(|event| match event {
            OrderEventAny::Filled(fill) if fill.venue_order_id == venue_order_id => Some(fill),
            _ => None,
        });
        let (filled_qty, notional) = current_fills.fold(
            (Decimal::ZERO, Decimal::ZERO),
            |(filled_qty, notional), fill| {
                let quantity = fill.last_qty.as_decimal();
                (
                    filled_qty + quantity,
                    notional + quantity * fill.last_px.as_decimal(),
                )
            },
        );

        if filled_qty > Decimal::ZERO {
            return (filled_qty, notional / filled_qty);
        }

        if events
            .iter()
            .any(|event| matches!(event, OrderEventAny::Filled(_)))
        {
            return (Decimal::ZERO, Decimal::ZERO);
        }

        (
            order.filled_qty().as_decimal(),
            order.avg_px().unwrap_or(Decimal::ZERO),
        )
    }

    fn sync_cached_fills<'a>(state: &mut OcmState, orders: impl IntoIterator<Item = &'a OrderAny>) {
        let mut replay_fills = Vec::new();
        let mut voided_by_trade = AHashMap::new();

        for order in orders {
            for event in order.events() {
                match event {
                    OrderEventAny::Filled(fill) => replay_fills.push((
                        fill.venue_order_id.to_string(),
                        fill.trade_id,
                        fill.last_qty.as_decimal(),
                        fill.last_px,
                    )),
                    OrderEventAny::FillVoided(voided) => {
                        voided_by_trade.insert(
                            (voided.venue_order_id.to_string(), voided.trade_id),
                            voided.voided_qty.as_decimal(),
                        );
                    }
                    _ => {}
                }
            }
        }

        for (bet_id, trade_id, quantity, price) in replay_fills {
            let voided_qty = voided_by_trade
                .get(&(bet_id.clone(), trade_id))
                .copied()
                .unwrap_or(Decimal::ZERO);
            state
                .fill_tracker
                .sync_fill_lot(&bet_id, trade_id, quantity, price, voided_qty);
        }
        let mut voided_by_bet = AHashMap::<String, Decimal>::new();

        for ((bet_id, _), voided_qty) in voided_by_trade {
            *voided_by_bet.entry(bet_id).or_default() += voided_qty;
        }

        for (bet_id, voided_qty) in voided_by_bet {
            state.fill_tracker.sync_voided_qty(&bet_id, voided_qty);
        }
    }

    /// Resyncs OCM state from cache and drains any OCMs the network handler
    /// buffered while waiting (cache is `!Send` so this must run on the
    /// engine thread).
    fn process_pending_resync(&self) {
        if !self.core.is_connected() {
            return;
        }

        if !self.pending_resync.load(Ordering::Acquire) {
            return;
        }

        let data_sender = get_data_event_sender();
        let market_ids_filter = self
            .config
            .stream_market_ids_filter
            .as_ref()
            .map(|ids| ids.iter().cloned().collect::<ahash::AHashSet<String>>());

        // Sync only at the start: per-iteration re-sync would clobber
        // tracker updates `process_ocm` just made with a staler cache view.
        self.sync_ocm_state_from_cache();

        loop {
            let mut buf = self.replay_buffer.lock().expect(MUTEX_POISONED);
            if buf.is_empty() {
                self.pending_resync.store(false, Ordering::Release);
                return;
            }
            let drained: Vec<ReceivedOcm> = std::mem::take(&mut *buf);
            drop(buf);

            for received in drained {
                Self::process_ocm(
                    &received,
                    self.core.account_id,
                    self.currency,
                    &self.emitter,
                    &self.ocm_state,
                    &data_sender,
                    market_ids_filter.as_ref(),
                    self.config.ignore_external_orders,
                    self.account_refresh_tx.as_ref(),
                );
            }
        }
    }

    /// Clears the resync flag and replay buffer so post-shutdown
    /// `is_connected()` polls don't dispatch buffered OCMs.
    fn clear_resync_state(&self) {
        let mut buf = self.replay_buffer.lock().expect(MUTEX_POISONED);
        buf.clear();
        self.pending_resync.store(false, Ordering::Release);

        self.reconciliation_gate.clear();
    }

    fn abort_pending_tasks(&self) {
        self.pending_tasks.abort_all();
    }

    fn abort_background_tasks(&mut self) {
        if let Some(handle) = self.keep_alive_handle.take() {
            handle.abort();
        }

        if let Some(handle) = self.account_state_handle.take() {
            handle.abort();
        }
        self.account_refresh_tx = None;

        if let Some(handle) = self.reconnect_handle.take() {
            handle.abort();
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn create_ocm_handler(
        emitter: ExecutionEventEmitter,
        account_id: AccountId,
        currency: Currency,
        ocm_state: Arc<Mutex<OcmState>>,
        data_sender: tokio::sync::mpsc::UnboundedSender<DataEvent>,
        market_ids_filter: Option<ahash::AHashSet<String>>,
        ignore_external_orders: bool,
        reconnect_tx: tokio::sync::mpsc::UnboundedSender<u64>,
        pending_resync: Arc<AtomicBool>,
        reconciliation_gate: Arc<ReconciliationGate>,
        replay_buffer: Arc<Mutex<Vec<ReceivedOcm>>>,
        account_refresh_tx: tokio::sync::mpsc::UnboundedSender<()>,
        clock: &'static AtomicTime,
    ) -> StreamMessageHandler {
        let has_initial_connection = Arc::new(AtomicBool::new(false));

        Arc::new(move |msg: StreamMessage| {
            let ts_init = clock.get_time_ns();

            match msg {
                StreamMessage::OrderChange(ocm) => {
                    if ocm.is_heartbeat() {
                        return;
                    }

                    let received = ReceivedOcm {
                        message: ocm,
                        ts_init,
                    };

                    // Lock spans the flag check so the drainer's clear-flag
                    // step cannot race a producer push
                    let mut buf = replay_buffer.lock().expect(MUTEX_POISONED);
                    if pending_resync.load(Ordering::Acquire) {
                        buf.push(received);
                        return;
                    }
                    drop(buf);

                    Self::process_ocm(
                        &received,
                        account_id,
                        currency,
                        &emitter,
                        &ocm_state,
                        &data_sender,
                        market_ids_filter.as_ref(),
                        ignore_external_orders,
                        Some(&account_refresh_tx),
                    );
                }
                StreamMessage::Connection(_) => {
                    let initial = !has_initial_connection.swap(true, Ordering::SeqCst)
                        && !reconciliation_gate.is_halted();

                    if initial {
                        log::debug!("Betfair execution stream connected");
                    } else {
                        log::info!("Betfair execution stream reconnected");
                        let generation = reconciliation_gate.halt();
                        pending_resync.store(true, Ordering::Release);

                        if reconnect_tx.send(generation).is_err() {
                            log::warn!("Failed to schedule Betfair reconnect reconciliation");
                        }
                    }
                }
                StreamMessage::Status(status) => {
                    if status.connection_closed {
                        reconciliation_gate.halt();
                        log::warn!(
                            "Betfair execution stream closed: {:?} - {:?}",
                            status.error_code,
                            status.error_message,
                        );
                    }
                }
                StreamMessage::MarketChange(_)
                | StreamMessage::RaceChange(_)
                | StreamMessage::CricketChange(_) => {}
            }
        })
    }

    #[expect(clippy::too_many_arguments)]
    fn process_ocm(
        received: &ReceivedOcm,
        account_id: AccountId,
        currency: Currency,
        emitter: &ExecutionEventEmitter,
        ocm_state: &Arc<Mutex<OcmState>>,
        data_sender: &tokio::sync::mpsc::UnboundedSender<DataEvent>,
        market_ids_filter: Option<&ahash::AHashSet<String>>,
        ignore_external_orders: bool,
        account_refresh_tx: Option<&tokio::sync::mpsc::UnboundedSender<()>>,
    ) {
        let ocm = &received.message;
        let Some(order_changes) = &ocm.oc else {
            return;
        };

        let ts_event = parse_millis_timestamp(ocm.pt);
        let ts_init = received.ts_init;

        let context = OcmProcessingContext {
            account_id,
            currency,
            emitter,
            ocm_state,
            data_sender,
            ignore_external_orders,
            account_refresh_tx,
            ts_event,
            ts_init,
        };

        for omc in order_changes {
            Self::process_order_market_change(omc, market_ids_filter, &context);
        }
    }

    fn process_order_market_change(
        omc: &OrderMarketChange,
        market_ids_filter: Option<&ahash::AHashSet<String>>,
        context: &OcmProcessingContext<'_>,
    ) {
        if market_ids_filter.is_some_and(|filter| !filter.contains(&omc.id)) {
            return;
        }

        for runner_change in omc.orc.as_deref().unwrap_or_default() {
            Self::process_order_runner_change(&omc.id, runner_change, context);
        }
    }

    fn process_order_runner_change(
        market_id: &str,
        runner_change: &OrderRunnerChange,
        context: &OcmProcessingContext<'_>,
    ) {
        let handicap = runner_change.hc.unwrap_or(Decimal::ZERO);
        let instrument_id = make_instrument_id(market_id, runner_change.id, handicap);

        for order in runner_change.uo.as_deref().unwrap_or_default() {
            let has_customer_ref = order
                .rfo
                .as_deref()
                .is_some_and(|customer_ref| !customer_ref.is_empty());

            if context.ignore_external_orders && !has_customer_ref {
                continue;
            }

            let processed = Self::process_unmatched_order(
                order,
                instrument_id,
                context.account_id,
                context.currency,
                context.emitter,
                context.ocm_state,
                context.ts_event,
                context.ts_init,
            );

            if processed
                && order.sv.is_some_and(|size| size > Decimal::ZERO)
                && let Some(tx) = context.account_refresh_tx
            {
                let _ = tx.send(());
            }

            Self::publish_legacy_order_voided(order, instrument_id, context);
        }
    }

    fn publish_legacy_order_voided(
        order: &UnmatchedOrder,
        instrument_id: InstrumentId,
        context: &OcmProcessingContext<'_>,
    ) {
        if order.status != StreamingOrderStatus::ExecutionComplete {
            return;
        }
        let Some(size_voided) = order.sv.filter(|size| *size > Decimal::ZERO) else {
            return;
        };
        let side = match order.side {
            StreamingSide::Back => "BACK",
            StreamingSide::Lay => "LAY",
        };
        let voided = BetfairOrderVoided::new(
            instrument_id,
            order.rfo.as_deref().unwrap_or("").to_string(),
            order.id.clone(),
            size_voided,
            order.p,
            order.s,
            side.to_string(),
            order.avp,
            order.sm,
            String::new(),
            context.ts_event,
            context.ts_init,
        );

        log::debug!(
            "Order voided: bet_id={}, size_voided={size_voided}",
            order.id
        );
        let custom = custom_data_with_instrument(Arc::new(voided), instrument_id);

        if let Err(e) = context
            .data_sender
            .send(DataEvent::Data(Data::Custom(custom)))
        {
            log::warn!("Failed to send voided event: {e}");
        }
    }

    #[expect(clippy::too_many_arguments)]
    fn process_unmatched_order(
        uo: &UnmatchedOrder,
        instrument_id: InstrumentId,
        account_id: AccountId,
        currency: Currency,
        emitter: &ExecutionEventEmitter,
        ocm_state: &Arc<Mutex<OcmState>>,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
    ) -> bool {
        let context = UnmatchedOrderContext {
            order: uo,
            instrument_id,
            account_id,
            currency,
            emitter,
            ts_event,
            ts_init,
        };
        let mut report =
            match parse_order_status_report(uo, instrument_id, account_id, ts_event, ts_init) {
                Ok(report) => report,
                Err(e) => {
                    log::warn!("Failed to parse order status report for {instrument_id}: {e}");
                    return false;
                }
            };

        let Ok(mut state) = ocm_state.lock() else {
            log::error!("OcmState mutex poisoned");
            return false;
        };

        if state.terminal_orders.contains(&uo.id)
            && !state.is_canceled_replace(&uo.id)
            && !state.fill_tracker.has_unseen_fill_void(uo)
        {
            return false;
        }

        let resolved_client_order_id = state.resolve_client_order_id(uo.rfo.as_deref());

        // Patch the truncated rfo-derived client_order_id with the full
        // resolved value so downstream reconciliation matches correctly.
        if resolved_client_order_id.is_some() {
            report.client_order_id = resolved_client_order_id;
        }

        let cancel_action =
            resolved_client_order_id
                .as_ref()
                .map_or(CancelAction::Emit, |client_order_id| {
                    if state.is_canceled_replace(&uo.id) {
                        CancelAction::RecloseAfterFill
                    } else if uo.status == StreamingOrderStatus::ExecutionComplete
                        && has_cancel_quantity(uo)
                        && state.should_suppress_cancel(client_order_id, &uo.id)
                    {
                        CancelAction::Suppress
                    } else {
                        CancelAction::Emit
                    }
                });

        if cancel_action == CancelAction::Suppress {
            log::debug!(
                "Suppressing cancel for bet_id={} (pending replace or already replaced)",
                uo.id,
            );

            if let Some(client_order_id) = resolved_client_order_id {
                state.mark_pending_replace_terminal(client_order_id, &uo.id);
            }
        }

        if let Some(client_oid) = resolved_client_order_id {
            state.mark_stream_reported(client_oid);
        }

        // Tracked orders take the direct-event path, not reports (routing contract,
        // developer_guide/adapters.md): direct terminal events stay authoritative instead
        // of being deferred by reconciliation while the order is locally `PendingCancel`.
        let tracked = resolved_client_order_id.and_then(|client_oid| {
            state
                .order_strategy_id(&client_oid)
                .map(|strategy_id| (client_oid, strategy_id))
        });

        if cancel_action == CancelAction::Suppress && tracked.is_none() {
            return false;
        }

        if let Some((client_order_id, strategy_id)) = tracked
            && let Some(total_quantity) =
                state.promote_pending_replace(&client_order_id, &uo.id, report.quantity)
        {
            let updated = OrderUpdated::new(
                context.emitter.trader_id(),
                strategy_id,
                context.instrument_id,
                client_order_id,
                total_quantity,
                UUID4::new(),
                report.ts_last,
                context.ts_init,
                false,
                Some(report.venue_order_id),
                Some(context.account_id),
                report.price,
                None,
                None,
                false,
            );
            context
                .emitter
                .send_order_event(OrderEventAny::Updated(updated));
        }

        if let Some((client_order_id, strategy_id, quantity)) =
            Self::resolve_pending_reduction_from_stream(&mut state, tracked, uo)
        {
            let updated = OrderUpdated::new(
                context.emitter.trader_id(),
                strategy_id,
                context.instrument_id,
                client_order_id,
                quantity,
                UUID4::new(),
                report.ts_last,
                context.ts_init,
                false,
                Some(report.venue_order_id),
                Some(context.account_id),
                None,
                None,
                None,
                false,
            );
            context
                .emitter
                .send_order_event(OrderEventAny::Updated(updated));
        }

        let (fill, fill_voids) = Self::derive_fill_changes(&context, &mut state);

        if report.order_status == OrderStatus::Canceled
            && let Some(reason) = report.cancel_reason.as_deref()
        {
            log::debug!(
                "Betfair order {} ({}) canceled: reason={}, matched={}, canceled={}, lapsed={}, voided={}",
                report
                    .client_order_id
                    .unwrap_or_else(|| ClientOrderId::from(uo.id.as_str())),
                uo.id,
                reason,
                uo.sm.unwrap_or(Decimal::ZERO),
                uo.sc.unwrap_or(Decimal::ZERO),
                uo.sl.unwrap_or(Decimal::ZERO),
                uo.sv.unwrap_or(Decimal::ZERO),
            );
        }

        let emitted = if let Some((client_order_id, strategy_id)) = tracked {
            Self::emit_tracked_order_events(
                &context,
                &mut state,
                &report,
                client_order_id,
                strategy_id,
                fill,
                fill_voids,
                cancel_action,
            )
        } else {
            Self::emit_untracked_order_reports(&context, report, resolved_client_order_id, fill);
            true
        };

        if !emitted {
            return false;
        }

        if uo.status == StreamingOrderStatus::ExecutionComplete {
            state.mark_terminal_order(uo.id.clone());

            if cancel_action != CancelAction::Suppress {
                if let Some(ref client_oid) = resolved_client_order_id {
                    state.cleanup_terminal_order(client_oid);
                }
                state.clear_canceled_replace(&uo.id);
            }
        }

        true
    }

    fn resolve_pending_reduction_from_stream(
        state: &mut OcmState,
        tracked: Option<(ClientOrderId, StrategyId)>,
        order: &UnmatchedOrder,
    ) -> Option<(ClientOrderId, StrategyId, Quantity)> {
        let (client_order_id, strategy_id) = tracked?;
        let active_quantity = stream_active_quantity(order)?;
        let quantity =
            state.confirm_pending_reduction(&client_order_id, &order.id, active_quantity)?;
        Some((client_order_id, strategy_id, quantity))
    }

    fn derive_fill_changes(
        context: &UnmatchedOrderContext<'_>,
        state: &mut OcmState,
    ) -> (Option<FillReport>, Vec<FillVoidAllocation>) {
        let order = context.order;
        let has_applied_fill_lots = state.fill_tracker.has_fill_lots(&order.id);
        let size_matched = order.sm.unwrap_or(Decimal::ZERO);
        let size_voided = order.sv.unwrap_or(Decimal::ZERO);
        let gross_matched = size_matched + size_voided;
        let mut cumulative = order.clone();
        cumulative.sm = Some(if has_applied_fill_lots {
            gross_matched
        } else {
            size_matched
        });
        let fill = state.fill_tracker.maybe_fill_report(
            &cumulative,
            order.s,
            context.instrument_id,
            context.account_id,
            context.currency,
            context.ts_event,
            context.ts_init,
        );

        if has_applied_fill_lots {
            return (fill, state.fill_tracker.maybe_fill_voids(order));
        }

        // A first-seen snapshot has no proof that Nautilus applied the voided portion.
        // Only anchor gross lifecycle quantity after emitting a surviving fill lot.
        if fill.is_some() {
            state.fill_tracker.sync_order(
                &order.id,
                gross_matched,
                order.avp.unwrap_or(Decimal::ZERO),
            );
        }
        state.fill_tracker.sync_voided_qty(&order.id, size_voided);
        (fill, Vec::new())
    }

    #[allow(
        clippy::too_many_arguments,
        reason = "tracked OCM routing needs resolved order, fill, and replace state"
    )]
    fn emit_tracked_order_events(
        context: &UnmatchedOrderContext<'_>,
        state: &mut OcmState,
        report: &OrderStatusReport,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        fill: Option<FillReport>,
        fill_voids: Vec<FillVoidAllocation>,
        cancel_action: CancelAction,
    ) -> bool {
        if state.mark_accepted(client_order_id) {
            let accepted = OrderAccepted::new(
                context.emitter.trader_id(),
                strategy_id,
                context.instrument_id,
                client_order_id,
                report.venue_order_id,
                context.account_id,
                UUID4::new(),
                report.ts_accepted,
                context.ts_init,
                false,
            );
            context
                .emitter
                .send_order_event(OrderEventAny::Accepted(accepted));
        }

        let has_fill = fill.is_some();
        let causation_id = fill.map(|fill_report| {
            Self::emit_tracked_fill(context, report, client_order_id, strategy_id, &fill_report)
        });

        if !Self::emit_fill_voids(
            context,
            report,
            client_order_id,
            strategy_id,
            fill_voids,
            causation_id,
        ) {
            return false;
        }

        let emit_cancel = match cancel_action {
            CancelAction::Emit => report.order_status == OrderStatus::Canceled,
            CancelAction::Suppress => false,
            CancelAction::RecloseAfterFill => {
                has_fill && report.order_status != OrderStatus::Filled
            }
        };

        if emit_cancel {
            let canceled = OrderCanceled::new(
                context.emitter.trader_id(),
                strategy_id,
                context.instrument_id,
                client_order_id,
                UUID4::new(),
                report.ts_last,
                context.ts_init,
                false,
                Some(report.venue_order_id),
                Some(context.account_id),
            );
            context
                .emitter
                .send_order_event(OrderEventAny::Canceled(canceled));
        }

        true
    }

    fn emit_tracked_fill(
        context: &UnmatchedOrderContext<'_>,
        report: &OrderStatusReport,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        fill_report: &FillReport,
    ) -> UUID4 {
        log::debug!(
            "Fill: bet_id={}, last_qty={}, last_px={}",
            context.order.id,
            fill_report.last_qty,
            fill_report.last_px,
        );
        let filled = OrderFilled::new(
            context.emitter.trader_id(),
            strategy_id,
            context.instrument_id,
            client_order_id,
            fill_report.venue_order_id,
            context.account_id,
            fill_report.trade_id,
            fill_report.order_side,
            report.order_type,
            fill_report.last_qty,
            fill_report.last_px,
            context.currency,
            fill_report.liquidity_side,
            UUID4::new(),
            fill_report.ts_event,
            context.ts_init,
            false,
            fill_report.venue_position_id,
            Some(fill_report.commission),
            None,
        );
        let event_id = filled.event_id;
        context
            .emitter
            .send_order_event(OrderEventAny::Filled(filled));
        event_id
    }

    fn emit_fill_voids(
        context: &UnmatchedOrderContext<'_>,
        report: &OrderStatusReport,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        fill_voids: Vec<FillVoidAllocation>,
        mut causation_id: Option<UUID4>,
    ) -> bool {
        if fill_voids.is_empty() {
            return Self::emit_terminal_fill_void(
                context,
                report,
                client_order_id,
                strategy_id,
                causation_id,
            );
        }

        for allocation in fill_voids {
            let correction_id = Ustr::from(&format!(
                "{}-sv-{}-{}",
                context.order.id,
                context.order.sv.unwrap_or(Decimal::ZERO).normalize(),
                allocation.trade_id,
            ));
            let mut fill_voided = OrderFillVoided::new(
                context.emitter.trader_id(),
                strategy_id,
                context.instrument_id,
                client_order_id,
                report.venue_order_id,
                context.account_id,
                correction_id,
                allocation.trade_id,
                allocation.voided_qty,
                None,
                OrderSide::from(context.order.side),
                report.order_type,
                allocation.last_px,
                context.currency,
                LiquiditySide::NoLiquiditySide,
                None,
                context.order.rc.as_deref().map(Ustr::from),
                None,
                UUID4::new(),
                report.ts_last,
                context.ts_init,
                false,
                false,
            );
            fill_voided.causation_id = causation_id;
            causation_id = Some(fill_voided.event_id);
            context
                .emitter
                .send_order_event(OrderEventAny::FillVoided(fill_voided));
        }

        true
    }

    fn emit_terminal_fill_void(
        context: &UnmatchedOrderContext<'_>,
        report: &OrderStatusReport,
        client_order_id: ClientOrderId,
        strategy_id: StrategyId,
        causation_id: Option<UUID4>,
    ) -> bool {
        if report.order_status != OrderStatus::Voided {
            return true;
        }
        let order = context.order;
        let Ok(voided_qty) = parse_betfair_quantity(order.sv.unwrap_or(Decimal::ZERO)) else {
            log::warn!("Cannot parse voided quantity for bet_id={}", order.id);
            return false;
        };
        let Ok(last_px) = parse_betfair_price(order.avp.unwrap_or(order.p)) else {
            log::warn!("Cannot parse voided price for bet_id={}", order.id);
            return false;
        };
        let mut voided = OrderFillVoided::new(
            context.emitter.trader_id(),
            strategy_id,
            context.instrument_id,
            client_order_id,
            report.venue_order_id,
            context.account_id,
            Ustr::from(&format!("{}-sv", order.id)),
            TradeId::new(format!("VOID-{}", order.id)),
            voided_qty,
            None,
            OrderSide::from(order.side),
            report.order_type,
            last_px,
            context.currency,
            LiquiditySide::NoLiquiditySide,
            None,
            order.rc.as_deref().map(Ustr::from),
            None,
            UUID4::new(),
            report.ts_last,
            context.ts_init,
            false,
            false,
        );
        voided.causation_id = causation_id;
        context
            .emitter
            .send_order_event(OrderEventAny::FillVoided(voided));
        true
    }

    fn emit_untracked_order_reports(
        context: &UnmatchedOrderContext<'_>,
        report: OrderStatusReport,
        resolved_client_order_id: Option<ClientOrderId>,
        fill: Option<FillReport>,
    ) {
        // The fill must precede the cumulative status report to avoid an inferred duplicate.
        if let Some(mut fill_report) = fill {
            if resolved_client_order_id.is_some() {
                fill_report.client_order_id = resolved_client_order_id;
            }

            log::debug!(
                "Fill: bet_id={}, last_qty={}, last_px={}",
                context.order.id,
                fill_report.last_qty,
                fill_report.last_px,
            );
            context.emitter.send_fill_report(fill_report);
        }

        context.emitter.send_order_status_report(report);
    }
}

#[async_trait(?Send)]
impl ExecutionClient for BetfairExecutionClient {
    fn is_connected(&self) -> bool {
        // Drain any OCMs the network handler buffered during reconnect
        self.process_pending_resync();
        self.core.is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.account_id
    }

    fn venue(&self) -> Venue {
        *BETFAIR_VENUE
    }

    fn oms_type(&self) -> OmsType {
        self.core.oms_type
    }

    fn get_account(&self) -> Option<AccountAny> {
        self.core.cache().account_owned(&self.core.account_id)
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
        if self.core.is_started() {
            return Ok(());
        }

        let sender = get_exec_event_sender();
        self.emitter.set_sender(sender);
        self.core.set_started();

        log::info!(
            "Started: client_id={}, account_id={}",
            self.core.client_id,
            self.core.account_id,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.is_stopped() {
            return Ok(());
        }

        self.core.set_stopped();
        self.core.set_disconnected();
        self.abort_background_tasks();
        self.abort_pending_tasks();

        if let Some(control) = &self.socket_control {
            control.deregister();
        }
        self.clear_resync_state();
        log::info!("Stopped: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.is_connected() {
            return Ok(());
        }

        register_betfair_custom_data();

        self.http_client
            .connect()
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let funds: AccountFundsResponse = self
            .http_client
            .send_accounts(METHOD_GET_ACCOUNT_FUNDS, serde_json::json!({}))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let ts_init = self.clock.get_time_ns();
        let account_state = parse_account_state(
            &funds,
            self.core.account_id,
            self.currency,
            ts_init,
            ts_init,
        )?;
        self.emitter.send_account_state(account_state);

        let session_token = self
            .http_client
            .session_token()
            .await
            .ok_or_else(|| anyhow::anyhow!("No session token after login"))?;

        // Sync OCM state from cached orders before stream connects
        self.sync_ocm_state_from_cache();

        let market_ids_filter = self
            .config
            .stream_market_ids_filter
            .as_ref()
            .map(|ids| ids.iter().cloned().collect::<ahash::AHashSet<String>>());

        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel();
        let (account_refresh_tx, mut account_refresh_rx) = tokio::sync::mpsc::unbounded_channel();
        self.account_refresh_tx = Some(account_refresh_tx.clone());

        let handler = Self::create_ocm_handler(
            self.emitter.clone(),
            self.core.account_id,
            self.currency,
            Arc::clone(&self.ocm_state),
            get_data_event_sender(),
            market_ids_filter,
            self.config.ignore_external_orders,
            reconnect_tx,
            Arc::clone(&self.pending_resync),
            Arc::clone(&self.reconciliation_gate),
            Arc::clone(&self.replay_buffer),
            account_refresh_tx,
            self.clock,
        );

        let transport_gate = Arc::clone(&self.reconciliation_gate);
        let halt_on_disconnect = move |state| {
            if state == SocketState::Disconnected {
                transport_gate.halt();
            }
        };
        let state_sink = match self.socket_control.as_ref() {
            Some(control) => control.sink_with(halt_on_disconnect),
            None => SocketStateSink::new(halt_on_disconnect),
        };

        let stream_client = BetfairStreamClient::connect_with_state_sink(
            &self.credential,
            session_token,
            handler,
            self.stream_config.clone(),
            HeartbeatTimeoutSource::Server,
            Some(state_sink),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

        let stream_client = Arc::new(stream_client);

        stream_client
            .subscribe_orders(None, None)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        if let Some(control) = &self.socket_control {
            let reconnect_stream = Arc::clone(&stream_client);
            control.register(move || reconnect_stream.request_reconnect_outcome());
        }
        self.stream_client = Some(stream_client);

        // Spawn periodic keep-alive to prevent session expiry
        let keep_alive_client = Arc::clone(&self.http_client);
        let keep_alive_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let keep_alive_app_key = self.credential.app_key().to_string();

        self.keep_alive_handle = Some(get_runtime().spawn(async move {
            const KEEP_ALIVE_INTERVAL_SECS: u64 = 36_000;
            let interval = tokio::time::Duration::from_secs(KEEP_ALIVE_INTERVAL_SECS);
            loop {
                tokio::time::sleep(interval).await;

                let (_, session_replaced) = match keep_alive_client.keep_alive_with_token().await {
                    Ok(token) => (token, false),
                    Err(ref e) if e.is_login_failed() => {
                        log::warn!("Betfair execution session expired, attempting re-login: {e}");

                        match keep_alive_client.reconnect_with_token().await {
                            Ok(token) => (token, true),
                            Err(e) => {
                                log::warn!("Betfair execution re-login failed: {e}");
                                continue;
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("Betfair execution keep-alive failed (transient): {e}");
                        continue;
                    }
                };
                apply_stream_session_refresh(
                    keep_alive_client.as_ref(),
                    Some(&keep_alive_stream),
                    &keep_alive_app_key,
                    SessionRefresh {
                        refreshed: true,
                        replaced: session_replaced,
                    },
                )
                .await;
                log::debug!("Betfair execution session keep-alive sent");
            }
        }));

        let acct_client = Arc::clone(&self.http_client);
        let acct_emitter = self.emitter.clone();
        let acct_id = self.core.account_id;
        let acct_currency = self.currency;
        let acct_clock = self.clock;
        let periodic_interval = (self.config.calculate_account_state
            && self.config.request_account_state_secs > 0)
            .then(|| tokio::time::Duration::from_secs(self.config.request_account_state_secs));

        self.account_state_handle = Some(get_runtime().spawn(async move {
            loop {
                let should_refresh = if let Some(interval) = periodic_interval {
                    tokio::select! {
                        request = account_refresh_rx.recv() => request.is_some(),
                        () = tokio::time::sleep(interval) => true,
                    }
                } else {
                    account_refresh_rx.recv().await.is_some()
                };

                if !should_refresh {
                    return;
                }

                while account_refresh_rx.try_recv().is_ok() {}

                match acct_client
                    .send_accounts::<AccountFundsResponse, _>(
                        METHOD_GET_ACCOUNT_FUNDS,
                        serde_json::json!({}),
                    )
                    .await
                {
                    Ok(funds) => {
                        let ts_init = acct_clock.get_time_ns();

                        match parse_account_state(&funds, acct_id, acct_currency, ts_init, ts_init)
                        {
                            Ok(state) => acct_emitter.send_account_state(state),
                            Err(e) => log::warn!("Failed to parse account state: {e}"),
                        }
                    }
                    Err(e) => log::warn!("Failed to fetch account state: {e}"),
                }
            }
        }));

        let reconnect_http = Arc::clone(&self.http_client);
        let reconnect_stream = Arc::clone(self.stream_client.as_ref().unwrap());
        let reconnect_emitter = self.emitter.clone();
        let reconnect_app_key = self.credential.app_key().to_string();
        let reconnect_clock = self.clock;
        let reconnect_client_id = self.core.client_id;
        let reconnect_acct_id = self.core.account_id;
        let reconnect_currency = self.currency;
        let reconnect_market_ids = self.reconcile_market_ids();
        let reconnect_lookback_mins = self.config.stream_gap_recovery_lookback_mins;
        let reconnect_ocm_state = Arc::clone(&self.ocm_state);
        let reconnect_gate = Arc::clone(&self.reconciliation_gate);

        self.reconnect_handle = Some(get_runtime().spawn(async move {
            while let Some(generation) = reconnect_rx.recv().await {
                log::info!("Handling execution stream reconnection");

                let mut session_refresh = SessionRefresh::default();

                let reconciled = async {
                    match reconnect_http.keep_alive_with_token().await {
                        Ok(_) => session_refresh.refreshed = true,
                        Err(ref e) if e.is_login_failed() => {
                            log::warn!("Session expired on reconnect, attempting re-login: {e}",);

                            match reconnect_http.reconnect_with_token().await {
                                Ok(_) => {
                                    session_refresh.refreshed = true;
                                    session_refresh.replaced = true;
                                }
                                Err(e) => {
                                    log::warn!("Re-login failed on reconnect: {e}");
                                    return false;
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!(
                                "Keep-alive failed on reconnect; continuing recovery with the \
                                 retained session: {e}",
                            );
                        }
                    }

                    let stream_session = StreamSession {
                        client: Some(&reconnect_stream),
                        app_key: &reconnect_app_key,
                    };

                    if session_refresh.refreshed {
                        stream_session.publish(&reconnect_http).await;
                    }

                    match reconnect_http
                        .send_accounts::<AccountFundsResponse, _>(
                            METHOD_GET_ACCOUNT_FUNDS,
                            serde_json::json!({}),
                        )
                        .await
                    {
                        Ok(funds) => {
                            let ts_init = reconnect_clock.get_time_ns();

                            match parse_account_state(
                                &funds,
                                reconnect_acct_id,
                                reconnect_currency,
                                ts_init,
                                ts_init,
                            ) {
                                Ok(state) => reconnect_emitter.send_account_state(state),
                                Err(e) => {
                                    log::warn!("Failed to parse account state on reconnect: {e}");
                                }
                            }
                        }
                        Err(e) => {
                            log::warn!("Failed to fetch account state on reconnect: {e}");
                        }
                    }

                    match fetch_post_reconnect_mass_status(
                        &reconnect_http,
                        reconnect_client_id,
                        reconnect_acct_id,
                        reconnect_currency,
                        reconnect_clock,
                        reconnect_market_ids.clone(),
                        reconnect_lookback_mins,
                        &reconnect_ocm_state,
                        &reconnect_emitter,
                        stream_session,
                        &mut session_refresh,
                    )
                    .await
                    {
                        Ok(mass_status) => {
                            let order_count = mass_status.order_reports().len();
                            let fill_count: usize = mass_status
                                .fill_reports()
                                .values()
                                .map(|fills| fills.len())
                                .sum();
                            reconnect_emitter.send_execution_report(ExecutionReport::MassStatus(
                                Box::new(mass_status),
                            ));
                            log::info!(
                                "Post-reconnect reconciliation submitted: \
                                 orders={order_count}, fills={fill_count}",
                            );
                            true
                        }
                        Err(e) => {
                            log::warn!("Post-reconnect reconciliation failed: {e}");
                            false
                        }
                    }
                }
                .await;

                apply_stream_session_refresh(
                    reconnect_http.as_ref(),
                    Some(&reconnect_stream),
                    &reconnect_app_key,
                    session_refresh,
                )
                .await;

                if reconciled && !reconnect_gate.try_resume(generation) {
                    log::info!("A newer execution stream reconnect remains unreconciled");
                }
            }
        }));

        self.core.set_connected();

        log::info!("Connected: client_id={}", self.core.client_id);
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.is_disconnected() {
            return Ok(());
        }

        self.abort_background_tasks();
        self.abort_pending_tasks();

        if let Some(control) = &self.socket_control {
            control.deregister();
        }

        if let Some(client) = &self.stream_client {
            client.close().await;
        }

        self.http_client.disconnect().await;
        self.core.set_disconnected();
        self.clear_resync_state();

        log::info!("Disconnected: client_id={}", self.core.client_id);
        Ok(())
    }

    fn query_order(&self, cmd: QueryOrder) -> anyhow::Result<()> {
        self.process_pending_resync();

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let account_id = self.core.account_id;
        let ocm_state = Arc::clone(&self.ocm_state);
        let clock = self.clock;
        let client_order_id = cmd.client_order_id;
        let venue_order_id = cmd.venue_order_id;
        let instrument_id = cmd.instrument_id;
        let stream_client = self.stream_client.as_ref().map(Arc::clone);
        let app_key = self.credential.app_key().to_string();

        self.spawn_task("query_order", async move {
            let mut session_refresh = SessionRefresh::default();
            let stream_session = StreamSession {
                client: stream_client.as_ref(),
                app_key: &app_key,
            };
            let mut candidates: Vec<CurrentOrderSummary> = Vec::new();
            let mut seen_bet_ids: AHashSet<String> = AHashSet::new();

            // Customer_order_ref lookup: Betfair reuses the ref across a
            // replace (old bet cancelled + new bet live), so this returns the
            // live replacement even when the cached bet_id is stale.
            let rfo = make_customer_order_ref(client_order_id.as_str());
            let rfo_params = list_current_orders_filter_ref(rfo.clone());

            match list_current_orders_with_retry(
                &http_client,
                &rfo_params,
                stream_session,
                &mut session_refresh,
            )
            .await
            {
                Ok(r) => extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders),
                Err(e) => log::warn!("Betfair query_order ref lookup failed: {e}"),
            }

            if candidates.is_empty() {
                let rfo_legacy = make_customer_order_ref_legacy(client_order_id.as_str());
                if rfo_legacy != rfo {
                    let legacy_params = list_current_orders_filter_ref(rfo_legacy);

                    match list_current_orders_with_retry(
                        &http_client,
                        &legacy_params,
                        stream_session,
                        &mut session_refresh,
                    )
                    .await
                    {
                        Ok(r) => {
                            extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders);
                        }
                        Err(e) => log::warn!("Betfair query_order legacy lookup failed: {e}"),
                    }
                }
            }

            // Always also query by bet_id when known. This rescues
            // pre-existing orders without a recognizable ref and orders whose
            // ref-based results came back as foreign-market collisions only.
            if let Some(ref bet_id) = venue_order_id {
                let params = list_current_orders_filter_bet_id(bet_id.to_string());

                match list_current_orders_with_retry(
                    &http_client,
                    &params,
                    stream_session,
                    &mut session_refresh,
                )
                .await
                {
                    Ok(r) => extend_unique(&mut candidates, &mut seen_bet_ids, r.current_orders),
                    Err(e) => log::warn!("Betfair query_order bet_id lookup failed: {e}"),
                }
            }

            let order = if candidates.is_empty() {
                log::warn!(
                    "Betfair query_order found no order for client_order_id={client_order_id}, venue_order_id={venue_order_id:?}",
                );
                None
            } else {
                select_order_for_query(
                    &candidates,
                    instrument_id,
                    client_order_id,
                    venue_order_id,
                )
            };

            if let Some(order) = order {
                let ts_init = clock.get_time_ns();
                match parse_current_order_report(order, account_id, ts_init) {
                    Ok(mut report) => {
                        if report.client_order_id.is_none()
                            && let Some(rfo) = order.customer_order_ref.as_deref()
                            && let Ok(state) = ocm_state.lock()
                            && let Some(full_id) = state.resolve_client_order_id(Some(rfo))
                        {
                            report.client_order_id = Some(full_id);
                        }

                        if report.client_order_id.is_none() {
                            report.client_order_id = Some(client_order_id);
                        }

                        emitter.send_order_status_report(report);
                    }
                    Err(e) => {
                        log::warn!("Failed to parse order report for {}: {e}", order.bet_id);
                    }
                }
            }

            apply_stream_session_refresh(
                http_client.as_ref(),
                stream_client.as_ref(),
                &app_key,
                session_refresh,
            )
            .await;
            Ok(())
        });

        Ok(())
    }

    async fn generate_mass_status(
        &self,
        lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        self.process_pending_resync();

        log::info!("Generating ExecutionMassStatus (lookback_mins={lookback_mins:?})");

        let ts_now = self.clock.get_time_ns();
        let start = lookback_mins.map(|mins| {
            let lookback_ns = mins
                .saturating_mul(60)
                .saturating_mul(NANOSECONDS_IN_SECOND);
            UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns))
        });

        let date_range = start.map(|start| TimeRange {
            from: Some(start.to_rfc3339()),
            to: None,
        });
        let market_ids = self.reconcile_market_ids();
        let mut order_refresh = SessionRefresh::default();
        let mut fill_refresh = SessionRefresh::default();
        let stream_session = StreamSession {
            client: self.stream_client.as_ref(),
            app_key: self.credential.app_key(),
        };
        let (order_reports, fill_reports) = tokio::join!(
            fetch_order_status_reports_http(
                &self.http_client,
                self.core.account_id,
                self.clock.get_time_ns(),
                market_ids.clone(),
                false,
                &self.ocm_state,
                &self.emitter,
                stream_session,
                &mut order_refresh,
            ),
            fetch_fill_reports_http(
                &self.http_client,
                self.core.account_id,
                self.currency,
                self.clock.get_time_ns(),
                market_ids,
                date_range,
                &self.ocm_state,
                stream_session,
                &mut fill_refresh,
            ),
        );

        let mut session_refresh = order_refresh;
        session_refresh.merge(&fill_refresh);
        apply_stream_session_refresh(
            self.http_client.as_ref(),
            self.stream_client.as_ref(),
            self.credential.app_key(),
            session_refresh,
        )
        .await;

        let order_reports = order_reports?;
        let fill_reports = fill_reports?;

        log::info!("Received {} OrderStatusReports", order_reports.len());
        log::info!("Received {} FillReports", fill_reports.len());

        let mut mass_status = ExecutionMassStatus::new(
            self.core.client_id,
            self.core.account_id,
            *BETFAIR_VENUE,
            ts_now,
            None,
        );

        mass_status.add_order_reports(order_reports);
        mass_status.add_fill_reports(fill_reports);

        Ok(Some(mass_status))
    }

    async fn generate_order_status_reports(
        &self,
        cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        self.process_pending_resync();

        let mut session_refresh = SessionRefresh::default();
        let stream_session = StreamSession {
            client: self.stream_client.as_ref(),
            app_key: self.credential.app_key(),
        };
        let result = fetch_order_status_reports_http(
            &self.http_client,
            self.core.account_id,
            self.clock.get_time_ns(),
            self.reconcile_market_ids(),
            cmd.open_only,
            &self.ocm_state,
            &self.emitter,
            stream_session,
            &mut session_refresh,
        )
        .await;

        apply_stream_session_refresh(
            self.http_client.as_ref(),
            self.stream_client.as_ref(),
            self.credential.app_key(),
            session_refresh,
        )
        .await;
        let reports = result?;

        log::debug!("Generated {} order status reports", reports.len());
        Ok(reports)
    }

    async fn generate_fill_reports(
        &self,
        cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        self.process_pending_resync();

        let date_range = match (cmd.start, cmd.end) {
            (Some(start), Some(end)) => Some(TimeRange {
                from: Some(start.to_rfc3339()),
                to: Some(end.to_rfc3339()),
            }),
            (Some(start), None) => Some(TimeRange {
                from: Some(start.to_rfc3339()),
                to: None,
            }),
            (None, Some(end)) => Some(TimeRange {
                from: None,
                to: Some(end.to_rfc3339()),
            }),
            (None, None) => None,
        };

        let mut session_refresh = SessionRefresh::default();
        let stream_session = StreamSession {
            client: self.stream_client.as_ref(),
            app_key: self.credential.app_key(),
        };
        let result = fetch_fill_reports_http(
            &self.http_client,
            self.core.account_id,
            self.currency,
            self.clock.get_time_ns(),
            self.reconcile_market_ids(),
            date_range,
            &self.ocm_state,
            stream_session,
            &mut session_refresh,
        )
        .await;

        apply_stream_session_refresh(
            self.http_client.as_ref(),
            self.stream_client.as_ref(),
            self.credential.app_key(),
            session_refresh,
        )
        .await;
        let reports = result?;

        log::debug!("Generated {} fill reports", reports.len());
        Ok(reports)
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        self.process_pending_resync();

        let order = self.core.get_order(&cmd.client_order_id)?;

        if self.submissions_halted() {
            log::warn!(
                "Halting submit for {} while the execution stream is unavailable or reconciling",
                order.client_order_id(),
            );
            self.emitter
                .emit_order_denied(&order, &OrderDeniedReason::StreamReconciling.to_string());
            return Ok(());
        }

        if order.is_closed() {
            log::warn!("Cannot submit closed order {}", order.client_order_id());
            return Ok(());
        }

        if let Ok(mut state) = self.ocm_state.lock() {
            state.register_customer_order_ref(order.client_order_id());
            state.register_order_identity(order.client_order_id(), order.strategy_id());
        }

        let instrument_id = order.instrument_id();
        let market_id = extract_market_id(&instrument_id)?;
        let (selection_id, handicap) = extract_selection_id(&instrument_id)?;

        let instruction = create_place_instruction(&order, selection_id, handicap)?;

        let market_version = self.get_market_version(&instrument_id);

        let params = PlaceOrdersParams {
            market_id,
            instructions: vec![instruction],
            customer_ref: Some(order_customer_ref()),
            market_version,
            customer_strategy_ref: None,
        };

        let client_order_id = order.client_order_id();
        let strategy_id = order.strategy_id();

        log::debug!("OrderSubmitted client_order_id={client_order_id}");
        self.emitter.emit_order_submitted(&order);

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ocm_state = Arc::clone(&self.ocm_state);

        self.spawn_task("submit-order", async move {
            let report: PlaceExecutionReport = match http_client
                .send_betting_order(METHOD_PLACE_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    match classify_http_error(&e) {
                        CommandFailure::Ambiguous(_) => log::warn!(
                            "Ambiguous submit response for {client_order_id}: {e}. \
                             Order may be live, awaiting OCM reconciliation",
                        ),
                        CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                            let reason = format!("submit-order error: {e}");
                            if should_emit_http_reject(&ocm_state, &client_order_id, &reason) {
                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    &reason,
                                    ts_event,
                                    false,
                                );
                            }
                        }
                    }
                    return Ok(());
                }
            };

            let instruction_report =
                single_instruction_report(report.instruction_reports.as_deref());
            let instruction_result = instruction_report.map(|ir| {
                classify_instruction_report(ir.status, ir.error_code, false, || {
                    format_place_instruction_reason(ir, &report)
                })
            });
            let result = classify_execution_report(
                report.status,
                report.error_code,
                instruction_result,
                || format_betfair_reason(report.error_code, None, "unknown error"),
            );

            match result {
                Err(CommandFailure::Ambiguous(_)) => {
                    log::warn!(
                        "Ambiguous submit report for {client_order_id}. \
                     Order may be live, awaiting OCM reconciliation",
                    );
                }
                Err(CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason)) => {
                    if should_emit_http_reject(&ocm_state, &client_order_id, &reason) {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_rejected_event(
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            &reason,
                            ts_event,
                            false,
                        );
                    }
                }
                Ok(()) => {
                    if let Some(bet_id) = instruction_report.and_then(|ir| ir.bet_id.as_ref()) {
                        let venue_order_id = VenueOrderId::from(bet_id.as_str());
                        let ts_event = clock.get_time_ns();

                        emit_http_accept_if_claimed(&ocm_state, &client_order_id, || {
                            emitter.emit_order_accepted(&order, venue_order_id, ts_event);
                        });
                    } else {
                        log::warn!(
                            "Submit succeeded without a bet ID for {client_order_id}; \
                             awaiting OCM reconciliation",
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.process_pending_resync();

        let instrument_id = cmd.instrument_id;
        let venue_order_id = match cmd.venue_order_id {
            Some(venue_order_id) => venue_order_id,
            None => {
                log::warn!(
                    "Cannot cancel order {}: no venue_order_id",
                    cmd.client_order_id
                );
                return Ok(());
            }
        };
        let market_id = match extract_market_id(&instrument_id) {
            Ok(market_id) => market_id,
            Err(e) => {
                log::warn!("Cannot cancel order {}: {e}", cmd.client_order_id);
                return Ok(());
            }
        };
        let bet_id: BetId = venue_order_id.to_string();

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: Some(vec![CancelInstruction {
                bet_id,
                size_reduction: None,
            }]),
            customer_ref: Some(order_customer_ref()),
        };

        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let ocm_state = Arc::clone(&self.ocm_state);
        let clock = self.clock;

        self.spawn_task("cancel-order", async move {
            let result: Result<CancelExecutionReport, _> = http_client
                .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                .await;

            let report = match result {
                Ok(r) => r,
                Err(e) => {
                    match classify_http_error(&e) {
                        CommandFailure::Ambiguous(_) => log::warn!(
                            "Ambiguous cancel response for {client_order_id}, awaiting OCM reconciliation: {e}",
                        ),
                        CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                            let reason = format!("cancel-order error: {e}");
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_cancel_rejected_event(
                                strategy_id,
                                instrument_id,
                                client_order_id,
                                Some(venue_order_id),
                                &reason,
                                ts_event,
                            );
                        }
                    }
                    return Ok(());
                }
            };

            let instruction_report =
                single_instruction_report(report.instruction_reports.as_deref());
            let instruction_result = instruction_report.map(|ir| {
                classify_instruction_report(ir.status, ir.error_code, true, || {
                    format_cancel_instruction_reason(ir.error_code, report.error_code)
                })
            });
            let result = classify_execution_report(
                report.status,
                report.error_code,
                instruction_result,
                || {
                    format_betfair_reason(report.error_code, None, "unknown error")
                },
            );

            match result {
                Ok(()) => {
                    let bet_taken_or_lapsed = instruction_report.is_some_and(|ir| {
                        ir.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed)
                    });

                    if bet_taken_or_lapsed {
                        log::debug!(
                            "Cancel {client_order_id}: BetTakenOrLapsed, treating as success",
                        );
                    }

                    let old_terminal = ocm_state
                        .lock()
                        .ok()
                        .and_then(|mut state| {
                            let pending = state
                                .take_pending_replace(client_order_id, venue_order_id.as_str());
                            if pending.as_ref().is_some_and(|pending| pending.old_terminal) {
                                state.cleanup_terminal_order(&client_order_id);
                            }
                            pending
                        })
                        .is_some_and(|pending| pending.old_terminal);
                    if old_terminal {
                        let ts_event = clock.get_time_ns();
                        let canceled = OrderCanceled::new(
                            emitter.trader_id(),
                            strategy_id,
                            instrument_id,
                            client_order_id,
                            UUID4::new(),
                            ts_event,
                            ts_event,
                            false,
                            Some(venue_order_id),
                            Some(emitter.account_id()),
                        );
                        emitter.send_order_event(OrderEventAny::Canceled(canceled));
                    }
                }
                Err(CommandFailure::Ambiguous(_)) => log::warn!(
                    "Ambiguous cancel report for {client_order_id}, awaiting OCM reconciliation",
                ),
                Err(
                    CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason),
                ) => {
                    let ts_event = clock.get_time_ns();
                    emitter.emit_order_cancel_rejected_event(
                        strategy_id,
                        instrument_id,
                        client_order_id,
                        Some(venue_order_id),
                        &reason,
                        ts_event,
                    );
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.process_pending_resync();

        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;

        let venue_order_id = cmd
            .venue_order_id
            .ok_or_else(|| anyhow::anyhow!("Cannot modify order without venue_order_id"))?;
        let bet_id: BetId = venue_order_id.to_string();

        // Compare against existing order to determine actual changes
        let existing_order = self.core.get_order(&cmd.client_order_id);
        let has_price_change = match (&cmd.price, &existing_order) {
            (Some(new_price), Ok(order)) => order.price() != Some(*new_price),
            (Some(_), Err(_)) => true,
            (None, _) => false,
        };
        let has_quantity_change = match (&cmd.quantity, &existing_order) {
            (Some(new_qty), Ok(order)) => order.quantity() != *new_qty,
            (Some(_), Err(_)) => true,
            (None, _) => false,
        };

        // Betfair does not support atomic price+quantity modification
        if has_price_change && has_quantity_change {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected_event(
                cmd.strategy_id,
                instrument_id,
                cmd.client_order_id,
                Some(venue_order_id),
                "cannot modify price and quantity simultaneously on Betfair",
                ts_event,
            );
            return Ok(());
        }

        let client_order_id = cmd.client_order_id;
        let strategy_id = cmd.strategy_id;
        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        if has_price_change {
            let new_price = cmd.price.unwrap().as_decimal();
            let old_bet_id = bet_id.clone();
            let update_price = cmd.price;
            let update_qty = existing_order.as_ref().ok().map(|order| order.quantity());

            // Track pending replace so the OCM handler suppresses the
            // cancel event for the old bet that Betfair emits as part
            // of the replace operation.
            if let Ok(mut state) = self.ocm_state.lock() {
                state.register_pending_replace(client_order_id, old_bet_id.clone(), update_qty);
            }

            let market_version = self.get_market_version(&instrument_id);

            let params = ReplaceOrdersParams {
                market_id,
                instructions: vec![ReplaceInstruction { bet_id, new_price }],
                customer_ref: Some(order_customer_ref()),
                market_version,
            };

            let ocm_state = Arc::clone(&self.ocm_state);

            self.spawn_task("modify-order-price", async move {
                let result: Result<ReplaceExecutionReport, _> = http_client
                    .send_betting_order(METHOD_REPLACE_ORDERS, &params)
                    .await;

                match result {
                    Ok(report) => {
                        let instruction_report =
                            single_instruction_report(report.instruction_reports.as_deref());
                        let instruction_result = instruction_report.map(|ir| {
                            classify_replace_instruction(
                                ir,
                                format_replace_instruction_reason(ir, &report),
                            )
                        });
                        let result = classify_execution_report(
                            report.status,
                            report.error_code,
                            instruction_result,
                            || {
                                format_betfair_reason(report.error_code, None, "unknown error")
                            },
                        );

                        match result {
                            Ok(()) => {
                                let new_bet_id = instruction_report
                                    .and_then(|ir| ir.place_instruction_report.as_ref())
                                    .and_then(|ir| ir.bet_id.as_ref());
                                let Some(new_bet_id) = new_bet_id else {
                                    log::warn!(
                                        "Replace succeeded without a new bet ID for {client_order_id}; \
                                         awaiting reconciliation",
                                    );
                                    return Ok(());
                                };

                                let replace_was_pending = if let Ok(mut state) = ocm_state.lock() {
                                    let replace_was_pending = state
                                        .take_pending_replace(client_order_id, &old_bet_id)
                                        .is_some();
                                    state.replaced_venue_order_ids.insert(old_bet_id);
                                    replace_was_pending
                                } else {
                                    true
                                };

                                if replace_was_pending
                                    && let Some(quantity) = update_qty
                                {
                                    let ts_event = clock.get_time_ns();
                                    let updated = OrderUpdated::new(
                                        emitter.trader_id(),
                                        strategy_id,
                                        instrument_id,
                                        client_order_id,
                                        quantity,
                                        UUID4::new(),
                                        ts_event,
                                        ts_event,
                                        false,
                                        Some(VenueOrderId::from(new_bet_id.as_str())),
                                        Some(emitter.account_id()),
                                        update_price,
                                        None,
                                        None,
                                        false,
                                    );
                                    emitter.send_order_event(OrderEventAny::Updated(updated));
                                }
                            }
                            Err(CommandFailure::Ambiguous(_)) => log::warn!(
                                "Ambiguous replace report for {client_order_id}, awaiting reconciliation",
                            ),
                            Err(CommandFailure::VenueRejected(reason))
                                if replace_cancelled_without_replacement(instruction_report) =>
                            {
                                if let Ok(mut state) = ocm_state.lock() {
                                    state.take_pending_replace(client_order_id, &old_bet_id);
                                    state.mark_canceled_replace(old_bet_id);
                                }
                                log::warn!(
                                    "Replace canceled {client_order_id} without placing its replacement: {reason}",
                                );
                                let ts_event = clock.get_time_ns();
                                let canceled = OrderCanceled::new(
                                    emitter.trader_id(),
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    UUID4::new(),
                                    ts_event,
                                    ts_event,
                                    false,
                                    Some(venue_order_id),
                                    Some(emitter.account_id()),
                                );
                                emitter.send_order_event(OrderEventAny::Canceled(canceled));
                            }
                            Err(
                                CommandFailure::NotSent(reason)
                                | CommandFailure::VenueRejected(reason),
                            ) => {
                                emit_replace_failure(
                                    &ocm_state,
                                    &emitter,
                                    clock,
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    venue_order_id,
                                    &old_bet_id,
                                    &reason,
                                );
                            }
                        }
                    }
                    Err(e) => {
                        match classify_http_error(&e) {
                            CommandFailure::Ambiguous(_) => log::warn!(
                                "Ambiguous replace response for {client_order_id}, awaiting reconciliation: {e}",
                            ),
                            CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                                emit_replace_failure(
                                    &ocm_state,
                                    &emitter,
                                    clock,
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    venue_order_id,
                                    &old_bet_id,
                                    &format!("modify-order error: {e}"),
                                );
                            }
                        }
                    }
                }

                Ok(())
            });
        } else if has_quantity_change {
            // Quantity reduction via partial cancel
            let order = self.core.get_order(&client_order_id)?;
            let original_quantity = order.quantity();
            let requested_quantity = cmd.quantity.unwrap();
            let existing_qty = original_quantity.as_decimal();
            let new_qty = requested_quantity.as_decimal();

            if new_qty >= existing_qty {
                let ts_event = self.clock.get_time_ns();
                self.emitter.emit_order_modify_rejected_event(
                    strategy_id,
                    instrument_id,
                    client_order_id,
                    Some(venue_order_id),
                    "can only reduce quantity on Betfair",
                    ts_event,
                );
                return Ok(());
            }

            let size_reduction = existing_qty - new_qty;
            let reduction_bet_id = bet_id.clone();
            let params = CancelOrdersParams {
                market_id: Some(market_id),
                instructions: Some(vec![CancelInstruction {
                    bet_id,
                    size_reduction: Some(size_reduction),
                }]),
                customer_ref: Some(order_customer_ref()),
            };

            // Register before sending so OCM can resolve before REST
            if let Ok(mut state) = self.ocm_state.lock() {
                state.register_pending_reduction(
                    client_order_id,
                    reduction_bet_id.clone(),
                    original_quantity,
                    requested_quantity,
                );
            }

            let ocm_state = Arc::clone(&self.ocm_state);

            self.spawn_task("modify-order-quantity", async move {
                let result: Result<CancelExecutionReport, _> = http_client
                    .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                    .await;

                match result {
                    Err(e) => {
                        match classify_http_error(&e) {
                            CommandFailure::Ambiguous(_) => log::warn!(
                                "Ambiguous quantity reduction for {client_order_id}, awaiting reconciliation: {e}",
                            ),
                            CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                                if let Ok(mut state) = ocm_state.lock() {
                                    state.clear_pending_reduction(
                                        &client_order_id,
                                        &reduction_bet_id,
                                    );
                                }

                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_modify_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    Some(venue_order_id),
                                    &format!("modify-order error: {e}"),
                                    ts_event,
                                );
                            }
                        }
                    }
                    Ok(report) => {
                        let instruction_report =
                            single_instruction_report(report.instruction_reports.as_deref());
                        let instruction_result = instruction_report.map(|ir| {
                            classify_instruction_report(ir.status, ir.error_code, false, || {
                                format_cancel_instruction_reason(ir.error_code, report.error_code)
                            })
                        });
                        let result = classify_execution_report(
                            report.status,
                            report.error_code,
                            instruction_result,
                            || {
                                format_betfair_reason(report.error_code, None, "unknown error")
                            },
                        );

                        match result {
                            Err(CommandFailure::Ambiguous(_)) => log::warn!(
                                "Ambiguous quantity reduction report for {client_order_id}, awaiting reconciliation",
                            ),
                            Err(
                                CommandFailure::NotSent(reason)
                                | CommandFailure::VenueRejected(reason),
                            ) => {
                                if let Ok(mut state) = ocm_state.lock() {
                                    state.clear_pending_reduction(
                                        &client_order_id,
                                        &reduction_bet_id,
                                    );
                                }

                                let ts_event = clock.get_time_ns();
                                emitter.emit_order_modify_rejected_event(
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    Some(venue_order_id),
                                    &reason,
                                    ts_event,
                                );
                            }
                            Ok(()) => {
                                let Some(updated_quantity) = instruction_report
                                    .and_then(|ir| ir.size_cancelled)
                                    .and_then(|cancelled| {
                                        parse_betfair_quantity(existing_qty - cancelled).ok()
                                    })
                                else {
                                    log::warn!(
                                        "Quantity reduction succeeded without a valid cancelled size for {client_order_id}; \
                                         awaiting reconciliation",
                                    );
                                    return Ok(());
                                };

                                let newly_resolved = if let Ok(mut state) = ocm_state.lock() {
                                    state.complete_pending_reduction(
                                        &client_order_id,
                                        &reduction_bet_id,
                                        updated_quantity,
                                    )
                                } else {
                                    true
                                };

                                if !newly_resolved {
                                    log::debug!(
                                        "Suppressing late reduction update for {client_order_id}: \
                                         already resolved from another channel",
                                    );
                                    return Ok(());
                                }

                                let ts_event = clock.get_time_ns();
                                let updated = OrderUpdated::new(
                                    emitter.trader_id(),
                                    strategy_id,
                                    instrument_id,
                                    client_order_id,
                                    updated_quantity,
                                    UUID4::new(),
                                    ts_event,
                                    ts_event,
                                    false,
                                    Some(venue_order_id),
                                    Some(emitter.account_id()),
                                    None,
                                    None,
                                    None,
                                    false,
                                );
                                emitter.send_order_event(OrderEventAny::Updated(updated));
                            }
                        }
                    }
                }

                Ok(())
            });
        } else {
            let ts_event = self.clock.get_time_ns();
            self.emitter.emit_order_modify_rejected_event(
                strategy_id,
                instrument_id,
                client_order_id,
                Some(venue_order_id),
                "no effective change in price or quantity",
                ts_event,
            );
        }

        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        self.process_pending_resync();

        let instrument_id = cmd.instrument_id;
        let market_id = match extract_market_id(&instrument_id) {
            Ok(market_id) => market_id,
            Err(e) => {
                log::warn!("Cannot cancel all orders for {instrument_id}: {e}");
                return Ok(());
            }
        };

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: None,
            customer_ref: Some(order_customer_ref()),
        };

        let http_client = Arc::clone(&self.http_client);

        self.spawn_task("cancel-all-orders", async move {
            let result = http_client
                .send_betting_order::<serde_json::Value, _>(METHOD_CANCEL_ORDERS, &params)
                .await;

            if let Err(e) = result {
                log::warn!("Failed to cancel all orders: {e}");
            }

            Ok(())
        });

        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        self.process_pending_resync();

        let instrument_id = cmd.instrument_id;
        let market_id = match extract_market_id(&instrument_id) {
            Ok(market_id) => market_id,
            Err(e) => {
                log::warn!("Cannot batch cancel orders for {instrument_id}: {e}");
                return Ok(());
            }
        };

        let mut instructions = Vec::new();
        let mut valid_cancels = Vec::new();

        for cancel in &cmd.cancels {
            match cancel.venue_order_id {
                Some(venue_order_id) => {
                    let bet_id: BetId = venue_order_id.to_string();
                    instructions.push(CancelInstruction {
                        bet_id,
                        size_reduction: None,
                    });
                    valid_cancels.push(cancel);
                }
                None => {
                    log::warn!(
                        "Cannot batch cancel order {}: no venue_order_id",
                        cancel.client_order_id,
                    );
                }
            }
        }

        if valid_cancels.is_empty() {
            return Ok(());
        }

        let params = CancelOrdersParams {
            market_id: Some(market_id),
            instructions: Some(instructions),
            customer_ref: Some(order_customer_ref()),
        };

        let cancel_data: Vec<_> = valid_cancels
            .iter()
            .map(|c| {
                (
                    c.strategy_id,
                    c.instrument_id,
                    c.client_order_id,
                    c.venue_order_id,
                )
            })
            .collect();

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;

        self.spawn_task("batch-cancel-orders", async move {
            let report: CancelExecutionReport = match http_client
                .send_betting_order(METHOD_CANCEL_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    match classify_http_error(&e) {
                        CommandFailure::Ambiguous(_) => log::warn!(
                            "Ambiguous batch cancel response for {} orders, awaiting OCM reconciliation: {e}",
                            cancel_data.len(),
                        ),
                        CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                            let reason = format!("batch-cancel-orders error: {e}");
                            let ts_event = clock.get_time_ns();

                            for (strategy_id, instr_id, client_oid, venue_oid) in &cancel_data {
                                emitter.emit_order_cancel_rejected_event(
                                    *strategy_id,
                                    *instr_id,
                                    *client_oid,
                                    *venue_oid,
                                    &reason,
                                    ts_event,
                                );
                            }
                        }
                    }
                    return Ok(());
                }
            };

            let instruction_reports = report.instruction_reports.as_deref().unwrap_or_default();
            if instruction_reports.len() > cancel_data.len() {
                log::warn!(
                    "Batch cancel returned {} reports for {} instructions; ignoring unmatched reports",
                    instruction_reports.len(),
                    cancel_data.len(),
                );
            }

            for (index, (strategy_id, instr_id, client_oid, venue_oid)) in
                cancel_data.iter().enumerate()
            {
                let instruction_report = instruction_reports.get(index);
                let instruction_result = instruction_report.map(|ir| {
                    classify_instruction_report(ir.status, ir.error_code, true, || {
                        format_cancel_instruction_reason(ir.error_code, report.error_code)
                    })
                });
                let result = classify_execution_report(
                    report.status,
                    report.error_code,
                    instruction_result,
                    || {
                        format_betfair_reason(report.error_code, None, "unknown error")
                    },
                );

                match result {
                    Ok(()) => {
                        if instruction_report.is_some_and(|ir| {
                            ir.error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed)
                        }) {
                            log::debug!(
                                "Cancel {client_oid}: BetTakenOrLapsed, treating as success",
                            );
                        }
                    }
                    Err(CommandFailure::Ambiguous(_)) => log::warn!(
                        "Ambiguous cancel result for {client_oid}, awaiting OCM reconciliation",
                    ),
                    Err(
                        CommandFailure::NotSent(reason)
                        | CommandFailure::VenueRejected(reason),
                    ) => {
                        let ts_event = clock.get_time_ns();
                        emitter.emit_order_cancel_rejected_event(
                            *strategy_id,
                            *instr_id,
                            *client_oid,
                            *venue_oid,
                            &reason,
                            ts_event,
                        );
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.process_pending_resync();

        if self.submissions_halted() {
            log::warn!(
                "Halting submit_order_list ({} orders) while the execution stream is \
                 unavailable or reconciling",
                cmd.order_list.client_order_ids.len(),
            );

            let denied = OrderDeniedReason::StreamReconciling.to_string();

            for client_order_id in &cmd.order_list.client_order_ids {
                if let Ok(order) = self.core.get_order(client_order_id) {
                    self.emitter.emit_order_denied(&order, &denied);
                }
            }
            return Ok(());
        }

        let instrument_id = cmd.instrument_id;
        let market_id = extract_market_id(&instrument_id)?;
        let (selection_id, handicap) = extract_selection_id(&instrument_id)?;

        let mut instructions = Vec::new();
        let mut order_snapshots = Vec::new();

        for client_order_id in &cmd.order_list.client_order_ids {
            let order = self.core.get_order(client_order_id)?;

            if order.is_closed() {
                log::warn!("Skipping closed order {client_order_id}");
                continue;
            }

            if let Ok(mut state) = self.ocm_state.lock() {
                state.register_customer_order_ref(order.client_order_id());
                state.register_order_identity(order.client_order_id(), order.strategy_id());
            }

            let instruction = create_place_instruction(&order, selection_id, handicap)?;

            instructions.push(instruction);
            order_snapshots.push((order.client_order_id(), order.strategy_id(), order.clone()));
        }

        if instructions.is_empty() {
            return Ok(());
        }

        let market_version = self.get_market_version(&instrument_id);

        let params = PlaceOrdersParams {
            market_id,
            instructions,
            customer_ref: Some(order_customer_ref()),
            market_version,
            customer_strategy_ref: None,
        };

        for (_, _, order) in &order_snapshots {
            log::debug!("OrderSubmitted client_order_id={}", order.client_order_id());
            self.emitter.emit_order_submitted(order);
        }

        let http_client = Arc::clone(&self.http_client);
        let emitter = self.emitter.clone();
        let clock = self.clock;
        let ocm_state = Arc::clone(&self.ocm_state);

        self.spawn_task("submit-order-list", async move {
            let report: PlaceExecutionReport = match http_client
                .send_betting_order(METHOD_PLACE_ORDERS, &params)
                .await
            {
                Ok(r) => r,
                Err(e) => {
                    match classify_http_error(&e) {
                        CommandFailure::Ambiguous(_) => log::warn!(
                            "Ambiguous submit response for order list: {e}. \
                             Orders may be live, awaiting OCM reconciliation",
                        ),
                        CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                            let ts_event = clock.get_time_ns();
                            let reason = format!("submit-order-list error: {e}");

                            for (client_oid, strategy_id, _) in &order_snapshots {
                                if should_emit_http_reject(&ocm_state, client_oid, &reason) {
                                    emitter.emit_order_rejected_event(
                                        *strategy_id,
                                        instrument_id,
                                        *client_oid,
                                        &reason,
                                        ts_event,
                                        false,
                                    );
                                }
                            }
                        }
                    }
                    return Ok(());
                }
            };

            let instruction_reports = report.instruction_reports.as_deref().unwrap_or_default();
            if instruction_reports.len() > order_snapshots.len() {
                log::warn!(
                    "Order list returned {} reports for {} instructions; ignoring unmatched reports",
                    instruction_reports.len(),
                    order_snapshots.len(),
                );
            }

            for (index, (client_oid, strategy_id, order)) in
                order_snapshots.iter().enumerate()
            {
                let instruction_report = instruction_reports.get(index);
                let instruction_result = instruction_report.map(|ir| {
                    classify_instruction_report(ir.status, ir.error_code, false, || {
                        format_place_instruction_reason(ir, &report)
                    })
                });
                let result = classify_execution_report(
                    report.status,
                    report.error_code,
                    instruction_result,
                    || {
                        format_betfair_reason(report.error_code, None, "unknown error")
                    },
                );

                match result {
                    Ok(()) => {
                        if let Some(bet_id) = instruction_report.and_then(|ir| ir.bet_id.as_ref()) {
                            let venue_order_id = VenueOrderId::from(bet_id.as_str());
                            let ts_event = clock.get_time_ns();
                            emit_http_accept_if_claimed(&ocm_state, client_oid, || {
                                emitter.emit_order_accepted(order, venue_order_id, ts_event);
                            });
                        } else {
                            log::warn!(
                                "Submit succeeded without a bet ID for {client_oid}; \
                                 awaiting OCM reconciliation",
                            );
                        }
                    }
                    Err(CommandFailure::Ambiguous(_)) => log::warn!(
                        "Ambiguous submit result for {client_oid}, awaiting OCM reconciliation",
                    ),
                    Err(
                        CommandFailure::NotSent(reason)
                        | CommandFailure::VenueRejected(reason),
                    ) => {
                        if should_emit_http_reject(&ocm_state, client_oid, &reason) {
                            let ts_event = clock.get_time_ns();
                            emitter.emit_order_rejected_event(
                                *strategy_id,
                                instrument_id,
                                *client_oid,
                                &reason,
                                ts_event,
                                false,
                            );
                        }
                    }
                }
            }

            Ok(())
        });

        Ok(())
    }
}

fn create_place_instruction(
    order: &impl Order,
    selection_id: SelectionId,
    handicap: Decimal,
) -> anyhow::Result<PlaceInstruction> {
    let side = BetfairSide::from(order.order_side());
    let size = order.quantity().as_decimal();
    let handicap = (handicap != Decimal::ZERO).then_some(handicap);
    let customer_order_ref = Some(make_customer_order_ref(order.client_order_id().as_str()));

    match order.order_type() {
        OrderType::Limit => {
            let price = order
                .price()
                .ok_or_else(|| anyhow::anyhow!("Limit order missing price"))?
                .as_decimal();

            if matches!(
                order.time_in_force(),
                TimeInForce::AtTheClose | TimeInForce::AtTheOpen
            ) {
                return Ok(PlaceInstruction {
                    order_type: BetfairOrderType::LimitOnClose,
                    selection_id,
                    handicap,
                    side,
                    limit_order: None,
                    limit_on_close_order: Some(LimitOnCloseOrder {
                        liability: size,
                        price,
                    }),
                    market_on_close_order: None,
                    customer_order_ref,
                });
            }

            let (persistence_type, time_in_force, min_fill_size) = match order.time_in_force() {
                TimeInForce::Ioc => (
                    None,
                    Some(BetfairTimeInForce::FillOrKill),
                    Some(Decimal::ZERO),
                ),
                TimeInForce::Fok => (None, Some(BetfairTimeInForce::FillOrKill), None),
                TimeInForce::Gtc => (Some(PersistenceType::Persist), None, None),
                _ => (Some(PersistenceType::Lapse), None, None),
            };

            Ok(PlaceInstruction {
                order_type: BetfairOrderType::Limit,
                selection_id,
                handicap,
                side,
                limit_order: Some(LimitOrder {
                    size,
                    price,
                    persistence_type,
                    time_in_force,
                    min_fill_size,
                    bet_target_type: None,
                    bet_target_size: None,
                }),
                limit_on_close_order: None,
                market_on_close_order: None,
                customer_order_ref,
            })
        }
        OrderType::Market => {
            if order.time_in_force() != TimeInForce::AtTheClose {
                anyhow::bail!(
                    "Market orders on Betfair are only supported with AtTheClose \
                     time in force (BSP MarketOnClose)"
                );
            }

            Ok(PlaceInstruction {
                order_type: BetfairOrderType::MarketOnClose,
                selection_id,
                handicap,
                side,
                limit_order: None,
                limit_on_close_order: None,
                market_on_close_order: Some(MarketOnCloseOrder { liability: size }),
                customer_order_ref,
            })
        }
        other => anyhow::bail!("Unsupported order type for Betfair: {other:?}"),
    }
}

// Even states admit submissions; each halt advances to a distinct odd generation.
// Generation exhaustion stays halted until lifecycle cleanup wraps the gate to zero.
#[derive(Debug, Default)]
struct ReconciliationGate {
    state: AtomicU64,
}

impl ReconciliationGate {
    fn is_halted(&self) -> bool {
        self.state.load(Ordering::Acquire) & 1 == 1
    }

    fn halt(&self) -> u64 {
        let mut current = self.state.load(Ordering::Acquire);

        loop {
            let next = if current == u64::MAX {
                current
            } else if current & 1 == 0 {
                current + 1
            } else {
                current.saturating_add(2)
            };

            if next == current {
                return current;
            }

            match self.state.compare_exchange_weak(
                current,
                next,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return next,
                Err(actual) => current = actual,
            }
        }
    }

    fn try_resume(&self, generation: u64) -> bool {
        generation != u64::MAX
            && generation & 1 == 1
            && self
                .state
                .compare_exchange(
                    generation,
                    generation + 1,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
    }

    fn clear(&self) {
        let mut current = self.state.load(Ordering::Acquire);

        while current & 1 == 1 {
            match self.state.compare_exchange_weak(
                current,
                current.wrapping_add(1),
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return,
                Err(actual) => current = actual,
            }
        }
    }
}

#[derive(Debug)]
struct ReceivedOcm {
    message: OCM,
    ts_init: UnixNanos,
}

struct OcmProcessingContext<'a> {
    account_id: AccountId,
    currency: Currency,
    emitter: &'a ExecutionEventEmitter,
    ocm_state: &'a Arc<Mutex<OcmState>>,
    data_sender: &'a tokio::sync::mpsc::UnboundedSender<DataEvent>,
    ignore_external_orders: bool,
    account_refresh_tx: Option<&'a tokio::sync::mpsc::UnboundedSender<()>>,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum CancelAction {
    Emit,
    Suppress,
    RecloseAfterFill,
}

struct UnmatchedOrderContext<'a> {
    order: &'a UnmatchedOrder,
    instrument_id: InstrumentId,
    account_id: AccountId,
    currency: Currency,
    emitter: &'a ExecutionEventEmitter,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
}

/// Paginates `list_current_orders` into `OrderStatusReport`s without touching
/// the engine cache, so it is callable from any tokio task.
#[expect(
    clippy::too_many_arguments,
    reason = "report context and stream session state remain explicit at the HTTP boundary"
)]
async fn fetch_order_status_reports_http(
    http_client: &Arc<BetfairHttpClient>,
    account_id: AccountId,
    ts_init: UnixNanos,
    market_ids: Option<Vec<String>>,
    open_only: bool,
    ocm_state: &Arc<Mutex<OcmState>>,
    emitter: &ExecutionEventEmitter,
    stream_session: StreamSession<'_>,
    session_refresh: &mut SessionRefresh,
) -> anyhow::Result<Vec<OrderStatusReport>> {
    let order_projection = if open_only {
        Some(OrderProjection::Executable)
    } else {
        Some(OrderProjection::All)
    };

    let mut reports = Vec::new();
    let mut active_quantities = AHashMap::new();
    let mut from_record: u32 = 0;

    loop {
        let params = ListCurrentOrdersParams {
            bet_ids: None,
            market_ids: market_ids.clone(),
            order_projection,
            customer_order_refs: None,
            customer_strategy_refs: None,
            date_range: None,
            order_by: None,
            sort_dir: None,
            from_record: if from_record > 0 {
                Some(from_record)
            } else {
                None
            },
            record_count: None,
        };

        let response =
            list_current_orders_with_retry(http_client, &params, stream_session, session_refresh)
                .await?;
        let page_size = response.current_orders.len() as u32;

        for order in &response.current_orders {
            match parse_current_order_report(order, account_id, ts_init) {
                Ok(mut r) => {
                    if let Some(ref rfo) = order.customer_order_ref
                        && let Ok(state) = ocm_state.lock()
                        && let Some(full_id) = state.resolve_client_order_id(Some(rfo.as_str()))
                    {
                        r.client_order_id = Some(full_id);
                    }

                    if let Some(active_quantity) = current_order_active_quantity(order) {
                        active_quantities.insert(order.bet_id.clone(), active_quantity);
                    }

                    reports.push(r);
                }
                Err(e) => log::warn!("Failed to parse order report for {}: {e}", order.bet_id),
            }
        }

        if !response.more_available {
            break;
        }

        from_record += page_size;
    }

    resolve_pending_modifies(&mut reports, &active_quantities, ocm_state, emitter);

    Ok(reports)
}

fn resolve_pending_modifies(
    reports: &mut Vec<OrderStatusReport>,
    active_quantities: &AHashMap<String, Quantity>,
    ocm_state: &Arc<Mutex<OcmState>>,
    emitter: &ExecutionEventEmitter,
) {
    let Ok(mut state) = ocm_state.lock() else {
        log::error!("OcmState mutex poisoned");
        return;
    };

    let mut resolved_bet_ids = AHashSet::new();

    for report in reports.iter_mut() {
        let bet_id = report.venue_order_id.to_string();

        if let Some(quantity) = state.reduced_quantity(&bet_id) {
            report.quantity = quantity;
        }

        let Some(client_order_id) = report.client_order_id else {
            continue;
        };

        let Some(strategy_id) = state.order_strategy_id(&client_order_id) else {
            continue;
        };

        if let Some(total_quantity) =
            state.promote_pending_replace(&client_order_id, &bet_id, report.quantity)
        {
            emit_reconciled_update(
                emitter,
                report,
                client_order_id,
                strategy_id,
                total_quantity,
                report.price,
            );
            resolved_bet_ids.insert(bet_id);
            continue;
        }

        if let Some(quantity) = resolve_pending_reduction_from_reconciliation(
            &mut state,
            active_quantities,
            &client_order_id,
            &bet_id,
        ) {
            emit_reconciled_update(
                emitter,
                report,
                client_order_id,
                strategy_id,
                quantity,
                None,
            );
            resolved_bet_ids.insert(bet_id.clone());
        }
    }

    reports.retain(|report| {
        let bet_id = report.venue_order_id.as_str();
        !state.replaced_venue_order_ids.contains(bet_id) && !resolved_bet_ids.contains(bet_id)
    });
}

fn resolve_pending_reduction_from_reconciliation(
    state: &mut OcmState,
    active_quantities: &AHashMap<String, Quantity>,
    client_order_id: &ClientOrderId,
    bet_id: &str,
) -> Option<Quantity> {
    let active_quantity = active_quantities.get(bet_id).copied()?;
    state.confirm_pending_reduction(client_order_id, bet_id, active_quantity)
}

fn emit_reconciled_update(
    emitter: &ExecutionEventEmitter,
    report: &OrderStatusReport,
    client_order_id: ClientOrderId,
    strategy_id: StrategyId,
    quantity: Quantity,
    price: Option<Price>,
) {
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        strategy_id,
        report.instrument_id,
        client_order_id,
        quantity,
        UUID4::new(),
        report.ts_last,
        report.ts_init,
        true,
        Some(report.venue_order_id),
        Some(report.account_id),
        price,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

fn current_order_active_quantity(order: &CurrentOrderSummary) -> Option<Quantity> {
    let active =
        order.size_matched.unwrap_or(Decimal::ZERO) + order.size_remaining.unwrap_or(Decimal::ZERO);
    parse_betfair_quantity(active).ok()
}

/// Paginates `list_current_orders` into `FillReport`s without touching the
/// engine cache, so it is callable from any tokio task.
#[expect(
    clippy::too_many_arguments,
    reason = "report context and session refresh state remain explicit at the HTTP boundary"
)]
async fn fetch_fill_reports_http(
    http_client: &Arc<BetfairHttpClient>,
    account_id: AccountId,
    currency: Currency,
    ts_init: UnixNanos,
    market_ids: Option<Vec<String>>,
    date_range: Option<TimeRange>,
    ocm_state: &Arc<Mutex<OcmState>>,
    stream_session: StreamSession<'_>,
    session_refresh: &mut SessionRefresh,
) -> anyhow::Result<Vec<FillReport>> {
    let mut orders = Vec::new();
    let mut from_record: u32 = 0;

    loop {
        let params = ListCurrentOrdersParams {
            bet_ids: None,
            market_ids: market_ids.clone(),
            order_projection: Some(OrderProjection::All),
            customer_order_refs: None,
            customer_strategy_refs: None,
            date_range: date_range.clone(),
            order_by: None,
            sort_dir: None,
            from_record: if from_record > 0 {
                Some(from_record)
            } else {
                None
            },
            record_count: None,
        };

        let response =
            list_current_orders_with_retry(http_client, &params, stream_session, session_refresh)
                .await?;
        let page_size = response.current_orders.len() as u32;

        orders.extend(response.current_orders);

        if !response.more_available {
            break;
        }

        from_record += page_size;
    }

    let mut state = ocm_state
        .lock()
        .map_err(|_| anyhow::anyhow!("OCM state lock poisoned"))?;
    Ok(build_incremental_fill_reports(
        &orders, &mut state, account_id, currency, ts_init,
    ))
}

fn build_incremental_fill_reports(
    orders: &[CurrentOrderSummary],
    state: &mut OcmState,
    account_id: AccountId,
    currency: Currency,
    ts_init: UnixNanos,
) -> Vec<FillReport> {
    let mut reports = Vec::new();

    for order in orders {
        let size_matched = order.size_matched.unwrap_or(Decimal::ZERO);
        let size_voided = order.size_voided.unwrap_or(Decimal::ZERO);
        let gross_matched = size_matched + size_voided;
        if gross_matched == Decimal::ZERO {
            continue;
        }

        if let Err(e) = parse_betfair_timestamp(&order.placed_date) {
            log::warn!("Failed to parse fill report for {}: {e}", order.bet_id);
            continue;
        }

        let has_applied_fill_lots = state.fill_tracker.has_fill_lots(&order.bet_id);
        let cumulative = if has_applied_fill_lots {
            gross_matched
        } else {
            size_matched
        };
        let incremental_fill = if has_applied_fill_lots && size_voided > Decimal::ZERO {
            state.fill_tracker.advance_cumulative_fill_with_voids(
                &order.bet_id,
                cumulative,
                size_voided,
                order.average_price_matched,
                order.price_size.price,
            )
        } else {
            state.fill_tracker.advance_cumulative_fill(
                &order.bet_id,
                cumulative,
                order.average_price_matched,
                order.price_size.price,
            )
        };

        if !has_applied_fill_lots && incremental_fill.is_some() {
            state.fill_tracker.sync_order(
                &order.bet_id,
                gross_matched,
                order.average_price_matched.unwrap_or(Decimal::ZERO),
            );
        }

        if !has_applied_fill_lots {
            state
                .fill_tracker
                .sync_voided_qty(&order.bet_id, size_voided);
        }
        let Some((trade_id, last_qty, last_px)) = incremental_fill else {
            continue;
        };

        match parse_current_order_fill_report(
            order, account_id, currency, trade_id, last_qty, last_px, ts_init,
        ) {
            Ok(mut report) => {
                if let Some(ref rfo) = order.customer_order_ref
                    && let Some(full_id) = state.resolve_client_order_id(Some(rfo.as_str()))
                {
                    report.client_order_id = Some(full_id);
                }
                reports.push(report);
            }
            Err(e) => log::warn!("Failed to parse fill report for {}: {e}", order.bet_id),
        }
    }

    reports
}

/// Builds an [`ExecutionMassStatus`] over `lookback_mins` of REST history.
#[expect(clippy::too_many_arguments)]
async fn fetch_post_reconnect_mass_status(
    http_client: &Arc<BetfairHttpClient>,
    client_id: ClientId,
    account_id: AccountId,
    currency: Currency,
    clock: &'static AtomicTime,
    market_ids: Option<Vec<String>>,
    lookback_mins: u64,
    ocm_state: &Arc<Mutex<OcmState>>,
    emitter: &ExecutionEventEmitter,
    stream_session: StreamSession<'_>,
    session_refresh: &mut SessionRefresh,
) -> anyhow::Result<ExecutionMassStatus> {
    let ts_now = clock.get_time_ns();
    let lookback_ns = lookback_mins
        .saturating_mul(60)
        .saturating_mul(NANOSECONDS_IN_SECOND);
    let start = UnixNanos::from(ts_now.as_u64().saturating_sub(lookback_ns));

    let date_range = TimeRange {
        from: Some(start.to_rfc3339()),
        to: None,
    };

    let order_reports = fetch_order_status_reports_http(
        http_client,
        account_id,
        ts_now,
        market_ids.clone(),
        false,
        ocm_state,
        emitter,
        stream_session,
        session_refresh,
    )
    .await?;

    let fill_reports = fetch_fill_reports_http(
        http_client,
        account_id,
        currency,
        ts_now,
        market_ids,
        Some(date_range),
        ocm_state,
        stream_session,
        session_refresh,
    )
    .await?;

    let mut mass_status =
        ExecutionMassStatus::new(client_id, account_id, *BETFAIR_VENUE, ts_now, None);
    mass_status.add_order_reports(order_reports);
    mass_status.add_fill_reports(fill_reports);
    Ok(mass_status)
}

fn list_current_orders_filter_bet_id(bet_id: String) -> ListCurrentOrdersParams {
    ListCurrentOrdersParams {
        bet_ids: Some(vec![bet_id]),
        market_ids: None,
        order_projection: None,
        customer_order_refs: None,
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: None,
    }
}

fn list_current_orders_filter_ref(customer_order_ref: String) -> ListCurrentOrdersParams {
    ListCurrentOrdersParams {
        bet_ids: None,
        market_ids: None,
        order_projection: None,
        customer_order_refs: Some(vec![customer_order_ref]),
        customer_strategy_refs: None,
        date_range: None,
        order_by: None,
        sort_dir: None,
        from_record: None,
        record_count: None,
    }
}

fn extend_unique(
    candidates: &mut Vec<CurrentOrderSummary>,
    seen: &mut AHashSet<String>,
    orders: Vec<CurrentOrderSummary>,
) {
    for order in orders {
        if seen.insert(order.bet_id.clone()) {
            candidates.push(order);
        }
    }
}

fn select_order_for_query(
    orders: &[CurrentOrderSummary],
    expected_instrument_id: InstrumentId,
    expected_client_order_id: ClientOrderId,
    expected_venue_order_id: Option<VenueOrderId>,
) -> Option<&CurrentOrderSummary> {
    let matching: Vec<&CurrentOrderSummary> = orders
        .iter()
        .filter(|o| {
            make_instrument_id(&o.market_id, o.selection_id, o.handicap) == expected_instrument_id
        })
        .collect();

    let candidates: Vec<&CurrentOrderSummary> = if matching.is_empty() {
        // No instrument match: accept only an exact venue_order_id hit
        // (pre-existing orders without a recognizable customer_order_ref).
        // A lone foreign-instrument candidate is not enough, since a 32-char
        // customer_order_ref collision can surface a single unrelated bet.
        if let Some(vid) = expected_venue_order_id
            && let Some(order) = orders.iter().find(|o| o.bet_id == vid.as_str())
        {
            return Some(order);
        }
        log::warn!(
            "Betfair query_order returned {} orders for client_order_id={expected_client_order_id}, none matching instrument {expected_instrument_id}; skipping to avoid cross-instrument reconciliation",
            orders.len(),
        );
        return None;
    } else {
        matching
    };

    // Prefer EXECUTABLE so a live replacement wins over a cancelled
    // predecessor sharing the same customer_order_ref.
    let executable: Vec<&CurrentOrderSummary> = candidates
        .iter()
        .copied()
        .filter(|o| o.status == BetfairOrderStatus::Executable)
        .collect();

    let pool = if executable.is_empty() {
        candidates
    } else {
        executable
    };

    // Tiebreaker: most recently placed bet. Picks the replacement over the
    // predecessor even when both are already terminal by poll time.
    pool.into_iter()
        .max_by(|a, b| a.placed_date.cmp(&b.placed_date))
}

async fn list_current_orders_with_retry(
    http_client: &Arc<BetfairHttpClient>,
    params: &ListCurrentOrdersParams,
    stream_session: StreamSession<'_>,
    session_refresh: &mut SessionRefresh,
) -> anyhow::Result<CurrentOrderSummaryReport> {
    const RATE_LIMIT_RETRY_DELAY_SECS: u64 = 5;

    match http_client
        .send_betting(METHOD_LIST_CURRENT_ORDERS, params)
        .await
    {
        Ok(r) => Ok(r),
        Err(e) if e.is_session_error() || e.is_rate_limit_error() => {
            if e.is_rate_limit_error() {
                log::warn!("Rate limited, retrying in {RATE_LIMIT_RETRY_DELAY_SECS}s");
                tokio::time::sleep(tokio::time::Duration::from_secs(
                    RATE_LIMIT_RETRY_DELAY_SECS,
                ))
                .await;
            } else {
                log::warn!("Session error, refreshing session");

                let refreshed = match http_client.keep_alive_with_token().await {
                    Ok(_) => Some(false),
                    Err(_) => http_client.reconnect_with_token().await.ok().map(|_| true),
                };

                if let Some(session_replaced) = refreshed {
                    session_refresh.refreshed = true;
                    session_refresh.replaced |= session_replaced;
                    stream_session.publish(http_client).await;
                }
            }
            http_client
                .send_betting(METHOD_LIST_CURRENT_ORDERS, params)
                .await
                .map_err(|e| anyhow::anyhow!("{e}"))
        }
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

#[derive(Default)]
struct SessionRefresh {
    refreshed: bool,
    replaced: bool,
}

impl SessionRefresh {
    fn merge(&mut self, other: &Self) {
        self.refreshed |= other.refreshed;
        self.replaced |= other.replaced;
    }
}

#[derive(Clone, Copy)]
struct StreamSession<'a> {
    client: Option<&'a Arc<BetfairStreamClient>>,
    app_key: &'a str,
}

impl StreamSession<'_> {
    async fn publish(self, http_client: &BetfairHttpClient) {
        let Some(client) = self.client else {
            return;
        };

        let _ = http_client
            .with_session_token(|token| {
                client.update_auth(self.app_key, token.to_string());
            })
            .await;
    }
}

async fn apply_stream_session_refresh(
    http_client: &BetfairHttpClient,
    stream_client: Option<&Arc<BetfairStreamClient>>,
    app_key: &str,
    session_refresh: SessionRefresh,
) {
    if !session_refresh.refreshed {
        return;
    }
    let Some(stream_client) = stream_client else {
        return;
    };

    let _ = http_client
        .with_session_token(|token| {
            stream_client.update_auth(app_key, token.to_string());
            if session_refresh.replaced {
                let _ = stream_client.request_reconnect();
            }
        })
        .await;
}

// Claims and emits the HTTP place acceptance while holding the `OcmState` lock. The OCM
// handler emits under the same lock, so a racing OCM fill cannot enqueue ahead of this
// acceptance. No-op if the OCM stream already reported the order or the accept was claimed.
fn emit_http_accept_if_claimed(
    ocm_state: &Arc<Mutex<OcmState>>,
    client_order_id: &ClientOrderId,
    emit: impl FnOnce(),
) {
    let Ok(mut state) = ocm_state.lock() else {
        log::error!("OcmState mutex poisoned");
        emit();
        return;
    };

    if state
        .stream_reported_client_orders
        .contains(client_order_id)
    {
        log::debug!(
            "Suppressing late HTTP acceptance for {client_order_id}: OCM already reported order state"
        );
        return;
    }

    if state.mark_accepted(*client_order_id) {
        emit();
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "order identity and reconciliation state remain explicit at the replace failure boundary"
)]
fn emit_replace_failure(
    ocm_state: &Arc<Mutex<OcmState>>,
    emitter: &ExecutionEventEmitter,
    clock: &AtomicTime,
    strategy_id: StrategyId,
    instrument_id: InstrumentId,
    client_order_id: ClientOrderId,
    venue_order_id: VenueOrderId,
    old_bet_id: &str,
    reason: &str,
) {
    let ts_event = clock.get_time_ns();
    let old_terminal = ocm_state
        .lock()
        .ok()
        .and_then(|mut state| {
            let pending = state.take_pending_replace(client_order_id, old_bet_id);
            if pending.as_ref().is_some_and(|pending| pending.old_terminal) {
                state.cleanup_terminal_order(&client_order_id);
            }
            pending
        })
        .is_some_and(|pending| pending.old_terminal);

    if old_terminal {
        let canceled = OrderCanceled::new(
            emitter.trader_id(),
            strategy_id,
            instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            ts_event,
            false,
            Some(venue_order_id),
            Some(emitter.account_id()),
        );
        emitter.send_order_event(OrderEventAny::Canceled(canceled));
    } else {
        emitter.emit_order_modify_rejected_event(
            strategy_id,
            instrument_id,
            client_order_id,
            Some(venue_order_id),
            reason,
            ts_event,
        );
    }
}

// Returns `false` if the OCM stream already reported on this order, so the
// HTTP rejection event should be suppressed to avoid an `InvalidStateTrigger`
// against the local order state machine.
fn should_emit_http_reject(
    ocm_state: &Arc<Mutex<OcmState>>,
    client_order_id: &ClientOrderId,
    reason: &str,
) -> bool {
    let Ok(state) = ocm_state.lock() else {
        log::error!("OcmState mutex poisoned");
        return true;
    };

    if state
        .stream_reported_client_orders
        .contains(client_order_id)
    {
        log::debug!(
            "Suppressing late HTTP rejection for {client_order_id}: OCM already reported order state ({reason})"
        );
        return false;
    }

    true
}

fn order_customer_ref() -> String {
    UUID4::new().to_string().replace('-', "")
}

fn classify_http_error(error: &BetfairHttpError) -> CommandFailure {
    let reason = error.to_string();

    if error.is_order_ambiguous() {
        CommandFailure::ambiguous(reason)
    } else if matches!(
        error,
        BetfairHttpError::MissingCredentials
            | BetfairHttpError::LoginFailed { .. }
            | BetfairHttpError::JsonError(_)
            | BetfairHttpError::InvalidConfiguration(_)
    ) {
        CommandFailure::not_sent(reason)
    } else {
        CommandFailure::venue_rejected(reason)
    }
}

fn single_instruction_report<T>(reports: Option<&[T]>) -> Option<&T> {
    match reports? {
        [report] => Some(report),
        _ => None,
    }
}

fn stream_active_quantity(uo: &UnmatchedOrder) -> Option<Quantity> {
    let active = uo.sm.unwrap_or(Decimal::ZERO) + uo.sr.unwrap_or(Decimal::ZERO);
    parse_betfair_quantity(active).ok()
}

fn classify_execution_report<F>(
    status: ExecutionReportStatus,
    error_code: Option<ExecutionReportErrorCode>,
    instruction_result: Option<Result<(), CommandFailure>>,
    report_reason: F,
) -> Result<(), CommandFailure>
where
    F: FnOnce() -> String,
{
    match (status, instruction_result) {
        (ExecutionReportStatus::Timeout, _) => Err(CommandFailure::ambiguous(report_reason())),
        (ExecutionReportStatus::Failure, Some(Ok(()))) => {
            Err(CommandFailure::ambiguous(report_reason()))
        }
        (_, Some(result)) => result,
        (ExecutionReportStatus::Failure, None)
            if execution_error_is_venue_rejection(error_code) =>
        {
            Err(CommandFailure::venue_rejected(report_reason()))
        }
        (
            ExecutionReportStatus::Success
            | ExecutionReportStatus::Failure
            | ExecutionReportStatus::ProcessedWithErrors,
            None,
        ) => Err(CommandFailure::ambiguous(report_reason())),
    }
}

fn execution_error_is_venue_rejection(error_code: Option<ExecutionReportErrorCode>) -> bool {
    matches!(
        error_code,
        Some(
            ExecutionReportErrorCode::InvalidAccountState
                | ExecutionReportErrorCode::InvalidWalletStatus
                | ExecutionReportErrorCode::InsufficientFunds
                | ExecutionReportErrorCode::LossLimitExceeded
                | ExecutionReportErrorCode::MarketSuspended
                | ExecutionReportErrorCode::MarketNotOpenForBetting
                | ExecutionReportErrorCode::InvalidOrder
                | ExecutionReportErrorCode::InvalidMarketId
                | ExecutionReportErrorCode::PermissionDenied
                | ExecutionReportErrorCode::DuplicateBetids
                | ExecutionReportErrorCode::NoActionRequired
                | ExecutionReportErrorCode::RejectedByRegulator
                | ExecutionReportErrorCode::NoChasing
                | ExecutionReportErrorCode::RegulatorIsNotAvailable
                | ExecutionReportErrorCode::TooManyInstructions
                | ExecutionReportErrorCode::InvalidMarketVersion
                | ExecutionReportErrorCode::InvalidProfitRatio
                | ExecutionReportErrorCode::EventExposureLimitExceeded
                | ExecutionReportErrorCode::EventMatchedExposureLimitExceeded
                | ExecutionReportErrorCode::EventBlocked
        )
    )
}

fn classify_replace_instruction(
    report: &ReplaceInstructionReport,
    reason: String,
) -> Result<(), CommandFailure> {
    classify_instruction_report(report.status, report.error_code, false, || reason.clone())?;

    let Some(cancel) = &report.cancel_instruction_report else {
        return Err(CommandFailure::ambiguous(reason));
    };
    classify_instruction_report(cancel.status, cancel.error_code, false, || reason.clone())?;

    let Some(place) = &report.place_instruction_report else {
        return Err(CommandFailure::ambiguous(reason));
    };
    classify_instruction_report(place.status, place.error_code, false, || reason)
}

fn classify_instruction_report<F>(
    status: InstructionReportStatus,
    error_code: Option<InstructionReportErrorCode>,
    bet_taken_is_success: bool,
    reason: F,
) -> Result<(), CommandFailure>
where
    F: FnOnce() -> String,
{
    match status {
        InstructionReportStatus::Success => Ok(()),
        InstructionReportStatus::Timeout => Err(CommandFailure::ambiguous(reason())),
        InstructionReportStatus::Failure
            if error_code == Some(InstructionReportErrorCode::BetInProgress) =>
        {
            Err(CommandFailure::ambiguous(reason()))
        }
        InstructionReportStatus::Failure
            if bet_taken_is_success
                && error_code == Some(InstructionReportErrorCode::BetTakenOrLapsed) =>
        {
            Ok(())
        }
        InstructionReportStatus::Failure => Err(CommandFailure::venue_rejected(reason())),
    }
}

fn replace_cancelled_without_replacement(report: Option<&ReplaceInstructionReport>) -> bool {
    report.is_some_and(|report| {
        report.error_code == Some(InstructionReportErrorCode::CancelledNotPlaced)
            && report
                .cancel_instruction_report
                .as_ref()
                .is_some_and(|cancel| cancel.status == InstructionReportStatus::Success)
            && report
                .place_instruction_report
                .as_ref()
                .is_some_and(|place| place.status == InstructionReportStatus::Failure)
    })
}

fn format_place_instruction_reason(
    instruction_report: &PlaceInstructionReport,
    report: &PlaceExecutionReport,
) -> String {
    format_betfair_reason(
        instruction_report.error_code,
        report_fallback(report.error_code),
        "unknown error",
    )
}

fn format_cancel_instruction_reason(
    error_code: Option<InstructionReportErrorCode>,
    report_error_code: Option<ExecutionReportErrorCode>,
) -> String {
    format_betfair_reason(
        error_code,
        report_fallback(report_error_code),
        "unknown instruction error",
    )
}

fn format_replace_instruction_reason(
    instruction_report: &ReplaceInstructionReport,
    report: &ReplaceExecutionReport,
) -> String {
    let nested_reason = instruction_report
        .place_instruction_report
        .as_ref()
        .and_then(|ir| instruction_fallback(ir.error_code))
        .or_else(|| {
            instruction_report
                .cancel_instruction_report
                .as_ref()
                .and_then(|ir| instruction_fallback(ir.error_code))
        });

    format_betfair_reason(
        instruction_report.error_code,
        nested_reason.or_else(|| report_fallback(report.error_code)),
        "unknown instruction error",
    )
}

fn format_betfair_reason(
    error_code: Option<impl fmt::Debug>,
    fallback: Option<String>,
    unknown: &str,
) -> String {
    error_code
        .map(|code| format!("{code:?}"))
        .or(fallback)
        .unwrap_or_else(|| unknown.to_string())
}

fn report_fallback(error_code: Option<ExecutionReportErrorCode>) -> Option<String> {
    error_code.map(|code| format!("{code:?}"))
}

fn instruction_fallback(error_code: Option<InstructionReportErrorCode>) -> Option<String> {
    error_code.map(|code| format!("{code:?}"))
}

#[cfg(test)]
mod tests {
    use std::{cell::RefCell, rc::Rc, time::Duration};

    use nautilus_common::{
        cache::Cache,
        live::runner::{replace_data_event_sender, replace_exec_event_sender},
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_model::{
        identifiers::{StrategyId, TraderId},
        orders::builder::OrderTestBuilder,
        types::{Price, Quantity},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;

    use super::*;
    use crate::{
        common::{
            consts::METHOD_GET_ACCOUNT_DETAILS,
            enums::SegmentType,
            testing::{load_test_json, parse_jsonrpc},
        },
        http::models::{AccountDetailsResponse, CancelInstructionReport},
        stream::messages::stream_decode,
    };

    #[rstest]
    #[case(
        ExecutionReportStatus::Success,
        None,
        Some(Ok(())),
        Ok(())
    )]
    #[case(
        ExecutionReportStatus::Success,
        None,
        None,
        Err(CommandFailure::Ambiguous("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::Timeout,
        None,
        Some(Err(CommandFailure::VenueRejected("instruction reason".to_string()))),
        Err(CommandFailure::Ambiguous("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::Failure,
        Some(ExecutionReportErrorCode::DuplicateTransaction),
        None,
        Err(CommandFailure::Ambiguous("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::Failure,
        Some(ExecutionReportErrorCode::MarketSuspended),
        Some(Ok(())),
        Err(CommandFailure::Ambiguous("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::Failure,
        Some(ExecutionReportErrorCode::MarketSuspended),
        None,
        Err(CommandFailure::VenueRejected("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::ProcessedWithErrors,
        None,
        None,
        Err(CommandFailure::Ambiguous("report reason".to_string()))
    )]
    #[case(
        ExecutionReportStatus::ProcessedWithErrors,
        None,
        Some(Err(CommandFailure::VenueRejected("instruction reason".to_string()))),
        Err(CommandFailure::VenueRejected("instruction reason".to_string()))
    )]
    fn test_classify_execution_report(
        #[case] status: ExecutionReportStatus,
        #[case] error_code: Option<ExecutionReportErrorCode>,
        #[case] instruction_result: Option<Result<(), CommandFailure>>,
        #[case] expected: Result<(), CommandFailure>,
    ) {
        assert_eq!(
            classify_execution_report(status, error_code, instruction_result, || {
                "report reason".to_string()
            }),
            expected,
        );
    }

    #[rstest]
    #[case(InstructionReportStatus::Success, None, false, Ok(()))]
    #[case(
        InstructionReportStatus::Timeout,
        None,
        false,
        Err(CommandFailure::Ambiguous("instruction reason".to_string()))
    )]
    #[case(
        InstructionReportStatus::Failure,
        Some(InstructionReportErrorCode::BetInProgress),
        false,
        Err(CommandFailure::Ambiguous("instruction reason".to_string()))
    )]
    #[case(
        InstructionReportStatus::Failure,
        Some(InstructionReportErrorCode::BetTakenOrLapsed),
        true,
        Ok(())
    )]
    #[case(
        InstructionReportStatus::Failure,
        Some(InstructionReportErrorCode::BetTakenOrLapsed),
        false,
        Err(CommandFailure::VenueRejected("instruction reason".to_string()))
    )]
    #[case(
        InstructionReportStatus::Failure,
        Some(InstructionReportErrorCode::ErrorInOrder),
        false,
        Err(CommandFailure::VenueRejected("instruction reason".to_string()))
    )]
    fn test_classify_instruction_report(
        #[case] status: InstructionReportStatus,
        #[case] error_code: Option<InstructionReportErrorCode>,
        #[case] bet_taken_is_success: bool,
        #[case] expected: Result<(), CommandFailure>,
    ) {
        assert_eq!(
            classify_instruction_report(status, error_code, bet_taken_is_success, || {
                "instruction reason".to_string()
            }),
            expected,
        );
    }

    #[rstest]
    #[case(
        Some((InstructionReportStatus::Success, None)),
        Some((InstructionReportStatus::Success, None)),
        Ok(())
    )]
    #[case(
        None,
        Some((InstructionReportStatus::Success, None)),
        Err(CommandFailure::Ambiguous("replace reason".to_string()))
    )]
    #[case(
        Some((InstructionReportStatus::Success, None)),
        None,
        Err(CommandFailure::Ambiguous("replace reason".to_string()))
    )]
    #[case(
        Some((InstructionReportStatus::Success, None)),
        Some((InstructionReportStatus::Timeout, None)),
        Err(CommandFailure::Ambiguous("replace reason".to_string()))
    )]
    #[case(
        Some((
            InstructionReportStatus::Failure,
            Some(InstructionReportErrorCode::ErrorInOrder),
        )),
        Some((InstructionReportStatus::Success, None)),
        Err(CommandFailure::VenueRejected("replace reason".to_string()))
    )]
    fn test_classify_replace_instruction(
        #[case] cancel: Option<(InstructionReportStatus, Option<InstructionReportErrorCode>)>,
        #[case] place: Option<(InstructionReportStatus, Option<InstructionReportErrorCode>)>,
        #[case] expected: Result<(), CommandFailure>,
    ) {
        let report = ReplaceInstructionReport {
            status: InstructionReportStatus::Success,
            error_code: None,
            error_message: None,
            cancel_instruction_report: cancel.map(|(status, error_code)| CancelInstructionReport {
                status,
                error_code,
                error_message: None,
                instruction: None,
                size_cancelled: None,
                cancelled_date: None,
            }),
            place_instruction_report: place.map(|(status, error_code)| PlaceInstructionReport {
                status,
                error_code,
                error_message: None,
                order_status: None,
                instruction: None,
                bet_id: None,
                placed_date: None,
                average_price_matched: None,
                size_matched: None,
            }),
        };

        assert_eq!(
            classify_replace_instruction(&report, "replace reason".to_string()),
            expected,
        );
    }

    #[rstest]
    #[case(
        Some(InstructionReportErrorCode::CancelledNotPlaced),
        Some(InstructionReportStatus::Success),
        Some(InstructionReportStatus::Failure),
        true
    )]
    #[case(
        Some(InstructionReportErrorCode::ErrorInOrder),
        Some(InstructionReportStatus::Success),
        Some(InstructionReportStatus::Failure),
        false
    )]
    #[case(
        Some(InstructionReportErrorCode::CancelledNotPlaced),
        Some(InstructionReportStatus::Failure),
        Some(InstructionReportStatus::Failure),
        false
    )]
    #[case(
        Some(InstructionReportErrorCode::CancelledNotPlaced),
        Some(InstructionReportStatus::Success),
        Some(InstructionReportStatus::Success),
        false
    )]
    #[case(
        Some(InstructionReportErrorCode::CancelledNotPlaced),
        None,
        Some(InstructionReportStatus::Failure),
        false
    )]
    #[case(
        Some(InstructionReportErrorCode::CancelledNotPlaced),
        Some(InstructionReportStatus::Success),
        None,
        false
    )]
    fn test_replace_cancelled_without_replacement(
        #[case] error_code: Option<InstructionReportErrorCode>,
        #[case] cancel_status: Option<InstructionReportStatus>,
        #[case] place_status: Option<InstructionReportStatus>,
        #[case] expected: bool,
    ) {
        let report = ReplaceInstructionReport {
            status: InstructionReportStatus::Failure,
            error_code,
            error_message: None,
            cancel_instruction_report: cancel_status.map(|status| CancelInstructionReport {
                status,
                error_code: None,
                error_message: None,
                instruction: None,
                size_cancelled: None,
                cancelled_date: None,
            }),
            place_instruction_report: place_status.map(|status| PlaceInstructionReport {
                status,
                error_code: Some(InstructionReportErrorCode::InvalidOdds),
                error_message: None,
                order_status: None,
                instruction: None,
                bet_id: None,
                placed_date: None,
                average_price_matched: None,
                size_matched: None,
            }),
        };

        assert_eq!(
            replace_cancelled_without_replacement(Some(&report)),
            expected
        );
        assert!(!replace_cancelled_without_replacement(None));
    }

    #[rstest]
    #[case(
        BetfairHttpError::MissingCredentials,
        CommandFailure::NotSent("Missing API credentials".to_string())
    )]
    #[case(
        BetfairHttpError::Timeout("request timed out".to_string()),
        CommandFailure::Ambiguous("Timeout: request timed out".to_string())
    )]
    #[case(
        BetfairHttpError::OrderRequestAmbiguous("earlier attempt".to_string()),
        CommandFailure::Ambiguous("Ambiguous order request: earlier attempt".to_string())
    )]
    #[case(
        BetfairHttpError::JsonError("request encoding".to_string()),
        CommandFailure::NotSent("JSON error: request encoding".to_string())
    )]
    #[case(
        BetfairHttpError::ResponseError("truncated response".to_string()),
        CommandFailure::Ambiguous("Response error: truncated response".to_string())
    )]
    #[case(
        BetfairHttpError::UnexpectedStatus {
            status: 500,
            body: "server error".to_string(),
        },
        CommandFailure::Ambiguous("Unexpected status 500: server error".to_string())
    )]
    #[case(
        BetfairHttpError::UnexpectedStatus {
            status: 429,
            body: "too many requests".to_string(),
        },
        CommandFailure::VenueRejected(
            "Unexpected status 429: too many requests".to_string()
        )
    )]
    #[case(
        BetfairHttpError::BetfairError {
            code: -32099,
            message: "ANGX-0001".to_string(),
            api_error_code: Some("TOO_MUCH_DATA".to_string()),
            api_error_details: Some(
                "MaxResults must be less than or equal to 1000".to_string()
            ),
        },
        CommandFailure::VenueRejected(
            "Betfair error -32099: ANGX-0001 (TOO_MUCH_DATA: MaxResults must be less than or equal to 1000)".to_string()
        )
    )]
    fn test_classify_http_error(#[case] error: BetfairHttpError, #[case] expected: CommandFailure) {
        assert_eq!(classify_http_error(&error), expected);
    }

    #[rstest]
    #[case(
        Some(InstructionReportErrorCode::ErrorInOrder),
        None,
        "unknown",
        "ErrorInOrder"
    )]
    #[case(None, Some("report error".to_string()), "unknown", "report error")]
    #[case(None, None, "unknown error", "unknown error")]
    fn test_format_betfair_reason(
        #[case] error_code: Option<InstructionReportErrorCode>,
        #[case] fallback: Option<String>,
        #[case] unknown: &str,
        #[case] expected: &str,
    ) {
        assert_eq!(
            format_betfair_reason(error_code, fallback, unknown),
            expected
        );
    }

    #[rstest]
    fn test_ocm_state_register_and_resolve() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-20240101-001");

        state.register_customer_order_ref(client_oid);

        let rfo = make_customer_order_ref(client_oid.as_str());
        let resolved = state.resolve_client_order_id(Some(&rfo));
        assert_eq!(resolved, Some(client_oid));
    }

    #[rstest]
    fn test_ocm_state_resolve_none_for_unknown_rfo() {
        let state = OcmState::default();
        assert!(state.resolve_client_order_id(Some("unknown")).is_none());
        assert!(state.resolve_client_order_id(None).is_none());
    }

    #[rstest]
    fn test_ocm_state_register_with_legacy() {
        let mut state = OcmState::default();
        let id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
        let client_oid = ClientOrderId::from(id);

        state.register_customer_order_ref_with_legacy(client_oid);

        let rfo_current = make_customer_order_ref(id);
        let rfo_legacy = make_customer_order_ref_legacy(id);
        assert_ne!(rfo_current, rfo_legacy);

        assert_eq!(
            state.resolve_client_order_id(Some(&rfo_current)),
            Some(client_oid)
        );
        assert_eq!(
            state.resolve_client_order_id(Some(&rfo_legacy)),
            Some(client_oid)
        );
    }

    #[rstest]
    fn test_ocm_state_remove_customer_order_refs() {
        let mut state = OcmState::default();
        let id = "O-20240101-550e8400-e29b-41d4-a716-446655440000";
        let client_oid = ClientOrderId::from(id);

        state.register_customer_order_ref_with_legacy(client_oid);
        state.remove_customer_order_refs(&client_oid);

        let rfo_current = make_customer_order_ref(id);
        let rfo_legacy = make_customer_order_ref_legacy(id);
        assert!(state.resolve_client_order_id(Some(&rfo_current)).is_none());
        assert!(state.resolve_client_order_id(Some(&rfo_legacy)).is_none());
    }

    #[rstest]
    fn test_http_accept_emits_when_unclaimed() {
        let state = Arc::new(Mutex::new(OcmState::default()));
        let client_oid = ClientOrderId::from("O-001");

        let mut emitted = false;
        emit_http_accept_if_claimed(&state, &client_oid, || emitted = true);
        assert!(emitted, "first HTTP accept must emit");

        // A second claim for the same order is suppressed (already accepted).
        let mut emitted_again = false;
        emit_http_accept_if_claimed(&state, &client_oid, || emitted_again = true);
        assert!(!emitted_again, "already-accepted order must not re-emit");
    }

    #[rstest]
    fn test_http_accept_suppressed_after_stream_report() {
        let client_oid = ClientOrderId::from("O-001");
        let mut inner = OcmState::default();
        inner.stream_reported_client_orders.insert(client_oid);
        let state = Arc::new(Mutex::new(inner));

        let mut emitted = false;
        emit_http_accept_if_claimed(&state, &client_oid, || emitted = true);
        assert!(!emitted, "OCM-reported order must suppress the HTTP accept");
    }

    #[rstest]
    fn test_should_emit_http_reject_without_stream_report() {
        let state = Arc::new(Mutex::new(OcmState::default()));
        let client_oid = ClientOrderId::from("O-001");

        assert!(should_emit_http_reject(
            &state,
            &client_oid,
            "BetLapsedPriceImprovementTooLarge",
        ));
    }

    #[rstest]
    fn test_should_not_emit_http_reject_after_stream_report() {
        // OCM-first race: the stream has already moved the order through
        // a terminal state (e.g. lapsed). A late HTTP rejection would
        // hit InvalidStateTrigger and pollute the own book audit log.
        let client_oid = ClientOrderId::from("O-001");
        let mut inner = OcmState::default();
        inner.stream_reported_client_orders.insert(client_oid);
        let state = Arc::new(Mutex::new(inner));

        assert!(!should_emit_http_reject(
            &state,
            &client_oid,
            "BetLapsedPriceImprovementTooLarge",
        ));
    }

    fn cancel_unmatched_order(
        bet_id: &str,
        rfo: Option<String>,
    ) -> crate::stream::messages::UnmatchedOrder {
        crate::stream::messages::UnmatchedOrder {
            id: bet_id.to_string(),
            p: Decimal::new(30, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::ExecutionComplete,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: None,
            sm: None,
            sr: None,
            sl: None,
            sc: Some(Decimal::new(20, 0)),
            sv: None,
            lsrc: Some(crate::common::enums::LapseStatusReasonCode::SpInPlay),
        }
    }

    fn emitter_with_receiver(
        account_id: AccountId,
    ) -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let clock = get_atomic_clock_realtime();
        let mut emitter = ExecutionEventEmitter::new(
            clock,
            TraderId::from("TESTER-001"),
            account_id,
            AccountType::Betting,
            None,
        );
        let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(tx);
        (emitter, rx)
    }

    #[expect(
        clippy::type_complexity,
        reason = "The tuple exposes each test channel and the replay buffer"
    )]
    fn ocm_handler_at(
        ts_init: UnixNanos,
        pending_resync: bool,
    ) -> (
        StreamMessageHandler,
        tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
        Arc<Mutex<Vec<ReceivedOcm>>>,
    ) {
        let account_id = AccountId::from("BETFAIR-001");
        let (emitter, execution_rx) = emitter_with_receiver(account_id);
        let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (reconnect_tx, _reconnect_rx) = tokio::sync::mpsc::unbounded_channel();
        let (account_refresh_tx, _account_refresh_rx) = tokio::sync::mpsc::unbounded_channel();
        let replay_buffer = Arc::new(Mutex::new(Vec::new()));
        let clock = Box::leak(Box::new(AtomicTime::new(false, ts_init)));

        let handler = BetfairExecutionClient::create_ocm_handler(
            emitter,
            account_id,
            Currency::GBP(),
            Arc::new(Mutex::new(OcmState::default())),
            data_tx,
            None,
            false,
            reconnect_tx,
            Arc::new(AtomicBool::new(pending_resync)),
            Arc::new(ReconciliationGate::default()),
            Arc::clone(&replay_buffer),
            account_refresh_tx,
            clock,
        );

        (handler, data_rx, execution_rx, replay_buffer)
    }

    #[rstest]
    fn test_ocm_handler_sets_init_from_clock() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_004);

        let (handler, mut data_rx, _execution_rx, replay_buffer) = ocm_handler_at(ts_init, false);
        let data = load_test_json("stream/ocm_VOIDED.json");

        handler(stream_decode(data.as_bytes()).unwrap());

        let custom = std::iter::from_fn(|| data_rx.try_recv().ok())
            .find_map(|event| match event {
                DataEvent::Data(Data::Custom(custom))
                    if custom.data.as_any().is::<BetfairOrderVoided>() =>
                {
                    Some(custom.data)
                }
                _ => None,
            })
            .expect("expected BetfairOrderVoided custom data");
        let voided = custom
            .as_any()
            .downcast_ref::<BetfairOrderVoided>()
            .unwrap();

        assert_eq!(voided.ts_event, UnixNanos::from(1_617_863_371_576_000_000));
        assert_eq!(voided.ts_init, ts_init);
        assert!(replay_buffer.lock().unwrap().is_empty());
    }

    #[rstest]
    fn test_ocm_handler_preserves_buffered_init() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_005);

        let (handler, mut data_rx, mut execution_rx, replay_buffer) = ocm_handler_at(ts_init, true);
        let data = load_test_json("stream/ocm_VOIDED.json");

        handler(stream_decode(data.as_bytes()).unwrap());

        let received = replay_buffer.lock().unwrap().pop().unwrap();

        assert!(data_rx.try_recv().is_err());
        assert!(execution_rx.try_recv().is_err());

        let account_id = AccountId::from("BETFAIR-001");
        let (emitter, _execution_rx) = emitter_with_receiver(account_id);
        let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
        BetfairExecutionClient::process_ocm(
            &received,
            account_id,
            Currency::GBP(),
            &emitter,
            &Arc::new(Mutex::new(OcmState::default())),
            &data_tx,
            None,
            false,
            None,
        );

        let custom = std::iter::from_fn(|| data_rx.try_recv().ok())
            .find_map(|event| match event {
                DataEvent::Data(Data::Custom(custom))
                    if custom.data.as_any().is::<BetfairOrderVoided>() =>
                {
                    Some(custom.data)
                }
                _ => None,
            })
            .expect("expected buffered BetfairOrderVoided custom data");
        let voided = custom
            .as_any()
            .downcast_ref::<BetfairOrderVoided>()
            .unwrap();

        assert_eq!(voided.ts_event, UnixNanos::from(1_617_863_371_576_000_000));
        assert_eq!(voided.ts_init, ts_init);
        assert!(replay_buffer.lock().unwrap().is_empty());
    }

    #[rstest]
    fn test_ocm_handler_buffers_each_segment_once() {
        let ts_init = UnixNanos::from(1_800_000_000_000_000_006);
        let (handler, mut data_rx, mut execution_rx, replay_buffer) = ocm_handler_at(ts_init, true);
        let data = load_test_json("stream/ocm_SEGMENTS.jsonl");

        for line in data.lines() {
            handler(stream_decode(line.as_bytes()).unwrap());
        }

        let received = replay_buffer.lock().unwrap();
        assert_eq!(received.len(), 3);
        assert_eq!(
            received[0].message.segment_type,
            Some(SegmentType::SegStart)
        );
        assert_eq!(received[1].message.segment_type, Some(SegmentType::Seg));
        assert_eq!(received[2].message.segment_type, Some(SegmentType::SegEnd));
        assert!(received.iter().all(|message| message.ts_init == ts_init));
        assert_eq!(
            received
                .iter()
                .map(|message| message.message.oc.as_ref().unwrap()[0].id.as_str())
                .collect::<Vec<_>>(),
            vec!["1.100001", "1.100002", "1.100003"],
        );
        assert!(data_rx.try_recv().is_err());
        assert!(execution_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_ocm_handler_stress_buffers_segments_once_in_order() {
        const SEQUENCE_COUNT: usize = 1_024;
        const MAX_MIDDLE_SEGMENTS: usize = 15;

        let ts_init = UnixNanos::from(1_800_000_000_000_000_007);
        let (handler, mut data_rx, mut execution_rx, replay_buffer) = ocm_handler_at(ts_init, true);
        let data = load_test_json("stream/ocm_SEGMENTS.jsonl");
        let segments = data.lines().collect::<Vec<_>>();
        let mut expected = Vec::new();

        for sequence in 0..SEQUENCE_COUNT {
            handler(stream_decode(segments[0].as_bytes()).unwrap());
            expected.push((SegmentType::SegStart, "1.100001"));

            for _ in 0..sequence % (MAX_MIDDLE_SEGMENTS + 1) {
                handler(stream_decode(segments[1].as_bytes()).unwrap());
                expected.push((SegmentType::Seg, "1.100002"));
            }

            handler(stream_decode(segments[2].as_bytes()).unwrap());
            expected.push((SegmentType::SegEnd, "1.100003"));
        }

        let received = replay_buffer.lock().unwrap();
        assert_eq!(received.len(), expected.len());
        for (message, (segment_type, market_id)) in received.iter().zip(expected) {
            assert_eq!(message.message.segment_type, Some(segment_type));
            assert_eq!(message.ts_init, ts_init);
            assert_eq!(
                message.message.oc.as_ref().unwrap()[0].id.as_str(),
                market_id,
            );
        }
        assert!(data_rx.try_recv().is_err());
        assert!(execution_rx.try_recv().is_err());
    }

    #[rstest]
    fn test_tracked_cancel_emits_direct_order_canceled() {
        // A tracked cancel must emit a direct OrderCanceled, not a deferrable report.
        let account_id = AccountId::from("BETFAIR-001");
        let client_order_id = ClientOrderId::from("O-CANCEL-001");
        let strategy_id = StrategyId::from("S-QUOTER");

        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id);
        inner.register_order_identity(client_order_id, strategy_id);
        inner.mark_accepted(client_order_id); // accepted before cancel (no synthesized accept)
        let ocm_state = Arc::new(Mutex::new(inner));

        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let uo = cancel_unmatched_order("bet_cancel", Some(rfo));

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(processed);

        match rx.try_recv().expect("expected an execution event") {
            ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) => {
                assert_eq!(canceled.client_order_id, client_order_id);
                assert_eq!(canceled.strategy_id, strategy_id);
                assert_eq!(
                    canceled.venue_order_id,
                    Some(VenueOrderId::from("bet_cancel"))
                );
            }
            other => panic!("expected a direct OrderCanceled event, was {other:?}"),
        }

        assert!(
            rx.try_recv().is_err(),
            "tracked cancel must not also emit a status report",
        );
    }

    #[rstest]
    fn test_untracked_cancel_emits_status_report() {
        // An order with no registered identity is external: its cancel takes the report path.
        let account_id = AccountId::from("BETFAIR-001");
        let ocm_state = Arc::new(Mutex::new(OcmState::default()));

        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let uo = cancel_unmatched_order("bet_external", Some("EXTERNAL-REF".to_string()));

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(processed);

        match rx.try_recv().expect("expected an execution event") {
            ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                assert_eq!(report.order_status, OrderStatus::Canceled);
                assert_eq!(report.venue_order_id, VenueOrderId::from("bet_external"));
            }
            other => panic!("expected an OrderStatusReport, was {other:?}"),
        }
    }

    #[rstest]
    fn test_untracked_order_with_fill_reports_with_resolved_id() {
        // Resolved rfo but untracked: the fill is reported with the resolved
        // client_order_id patched on.
        let account_id = AccountId::from("BETFAIR-001");
        let client_order_id = ClientOrderId::from("O-EXT-FILL");

        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id); // resolves, but no identity
        let ocm_state = Arc::new(Mutex::new(inner));

        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let uo = fill_unmatched_order("bet_ext_fill", Some(rfo), Decimal::new(10, 0));

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(processed);

        match rx.try_recv().expect("expected a fill report") {
            ExecutionEvent::Report(ExecutionReport::Fill(fill)) => {
                assert_eq!(fill.client_order_id, Some(client_order_id));
                assert_eq!(fill.last_qty.as_decimal(), Decimal::new(10, 0));
            }
            other => panic!("expected a FillReport for untracked fill, was {other:?}"),
        }

        match rx.try_recv().expect("expected a status report") {
            ExecutionEvent::Report(ExecutionReport::Order(_)) => {}
            other => panic!("expected an OrderStatusReport, was {other:?}"),
        }
    }

    #[rstest]
    fn test_unparsable_order_returns_false_without_emitting() {
        let account_id = AccountId::from("BETFAIR-001");
        let ocm_state = Arc::new(Mutex::new(OcmState::default()));
        let (emitter, mut rx) = emitter_with_receiver(account_id);

        let mut uo = cancel_unmatched_order("bet_bad", None);
        uo.pt = None; // LIMIT without persistence type -> parse error

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(!processed);
        assert!(rx.try_recv().is_err(), "unparsable order must emit nothing");
    }

    #[rstest]
    fn test_process_ocm_voided_emits_custom_data() {
        // An ExecutionComplete order with size_voided > 0 publishes a
        // BetfairOrderVoided custom data event.
        let data = crate::common::testing::load_test_json("stream/ocm_VOIDED.json");
        let ocm = match crate::stream::messages::stream_decode(data.as_bytes()).unwrap() {
            crate::stream::messages::StreamMessage::OrderChange(ocm) => ocm,
            other => panic!("expected an OCM stream message, was {other:?}"),
        };

        let account_id = AccountId::from("BETFAIR-001");
        let ocm_state = Arc::new(Mutex::new(OcmState::default()));
        let (emitter, _rx) = emitter_with_receiver(account_id);
        let (data_tx, mut data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (account_refresh_tx, mut account_refresh_rx) = tokio::sync::mpsc::unbounded_channel();

        let received = ReceivedOcm {
            message: ocm,
            ts_init: UnixNanos::from(1_800_000_000_000_000_004),
        };

        BetfairExecutionClient::process_ocm(
            &received,
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            &data_tx,
            None,
            false,
            Some(&account_refresh_tx),
        );

        let voided = std::iter::from_fn(|| data_rx.try_recv().ok())
            .any(|event| matches!(event, DataEvent::Data(Data::Custom(_))));
        assert!(voided, "voided OCM must publish a custom voided data event");
        assert!(account_refresh_rx.try_recv().is_ok());
    }

    #[rstest]
    #[case("stream/ocm_VOIDED.json", Decimal::new(50, 0), Decimal::new(50, 0))]
    #[case(
        "stream/ocm_VOIDED_partial.json",
        Decimal::new(60, 0),
        Decimal::new(40, 0)
    )]
    fn test_tracked_void_fixture_emits_terminal_fill_correction(
        #[case] fixture: &str,
        #[case] matched: Decimal,
        #[case] voided: Decimal,
    ) {
        let data = crate::common::testing::load_test_json(fixture);
        let ocm = match crate::stream::messages::stream_decode(data.as_bytes()).unwrap() {
            crate::stream::messages::StreamMessage::OrderChange(ocm) => ocm,
            other => panic!("expected an OCM stream message, was {other:?}"),
        };
        let market = &ocm.oc.as_ref().unwrap()[0];
        let runner = &market.orc.as_ref().unwrap()[0];
        let uo = &runner.uo.as_ref().unwrap()[0];
        let client_order_id = ClientOrderId::from(uo.rfo.as_deref().unwrap());
        let strategy_id = StrategyId::from("S-VOID");
        let account_id = AccountId::from("BETFAIR-001");
        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id);
        inner.register_order_identity(client_order_id, strategy_id);
        inner.mark_accepted(client_order_id);
        let state = Arc::new(Mutex::new(inner));
        let (emitter, mut rx) = emitter_with_receiver(account_id);

        let processed = BetfairExecutionClient::process_unmatched_order(
            uo,
            make_instrument_id(&market.id, runner.id, Decimal::ZERO),
            account_id,
            Currency::GBP(),
            &emitter,
            &state,
            parse_millis_timestamp(ocm.pt),
            parse_millis_timestamp(ocm.pt),
        );

        let filled = match rx.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected OrderFilled, was {other:?}"),
        };
        let fill_voided = match rx.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected OrderFillVoided, was {other:?}"),
        };
        assert!(processed);
        assert_eq!(filled.last_qty.as_decimal(), matched);
        assert_ne!(fill_voided.trade_id, filled.trade_id);
        assert_eq!(fill_voided.voided_qty.as_decimal(), voided);
        assert_eq!(fill_voided.commission_voided, None);
        assert!(!fill_voided.is_reopened);
        assert_eq!(fill_voided.causation_id, Some(filled.event_id));
        assert!(rx.try_recv().is_err());
    }

    #[rstest]
    fn test_terminal_bet_allows_increased_cumulative_void_report() {
        let data = crate::common::testing::load_test_json("stream/ocm_VOIDED_partial.json");
        let ocm = match crate::stream::messages::stream_decode(data.as_bytes()).unwrap() {
            crate::stream::messages::StreamMessage::OrderChange(ocm) => ocm,
            other => panic!("expected an OCM stream message, was {other:?}"),
        };
        let market = &ocm.oc.as_ref().unwrap()[0];
        let runner = &market.orc.as_ref().unwrap()[0];
        let mut uo = runner.uo.as_ref().unwrap()[0].clone();
        let client_order_id = ClientOrderId::from(uo.rfo.as_deref().unwrap());
        let account_id = AccountId::from("BETFAIR-001");
        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id);
        inner.register_order_identity(client_order_id, StrategyId::from("S-VOID"));
        inner.mark_accepted(client_order_id);
        let state = Arc::new(Mutex::new(inner));
        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let instrument_id = make_instrument_id(&market.id, runner.id, Decimal::ZERO);
        let ts = parse_millis_timestamp(ocm.pt);

        let first = BetfairExecutionClient::process_unmatched_order(
            &uo,
            instrument_id,
            account_id,
            Currency::GBP(),
            &emitter,
            &state,
            ts,
            ts,
        );
        let first_events = std::iter::from_fn(|| rx.try_recv().ok()).collect::<Vec<_>>();
        uo.sm = Some(Decimal::new(50, 0));
        uo.sv = Some(Decimal::new(50, 0));
        let increased = BetfairExecutionClient::process_unmatched_order(
            &uo,
            instrument_id,
            account_id,
            Currency::GBP(),
            &emitter,
            &state,
            ts,
            ts,
        );
        let report = match rx.try_recv().unwrap() {
            ExecutionEvent::Report(ExecutionReport::Order(report)) => report,
            other => panic!("expected an order status report, was {other:?}"),
        };
        let duplicate = BetfairExecutionClient::process_unmatched_order(
            &uo,
            instrument_id,
            account_id,
            Currency::GBP(),
            &emitter,
            &state,
            ts,
            ts,
        );

        assert!(first);
        assert_eq!(first_events.len(), 2);
        assert!(increased);
        assert_eq!(report.order_status, OrderStatus::Voided);
        assert_eq!(report.filled_qty, Quantity::from("50.00"));
        assert!(!duplicate);
        assert!(rx.try_recv().is_err());
    }

    fn fill_unmatched_order(
        bet_id: &str,
        rfo: Option<String>,
        size_matched: Decimal,
    ) -> crate::stream::messages::UnmatchedOrder {
        crate::stream::messages::UnmatchedOrder {
            id: bet_id.to_string(),
            p: Decimal::new(20, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(20, 1)),
            sm: Some(size_matched),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        }
    }

    #[rstest]
    fn test_tracked_fill_synthesizes_accept_then_emits_filled() {
        // First OCM is a fill with no prior accept: synthesize OrderAccepted, then OrderFilled.
        let account_id = AccountId::from("BETFAIR-001");
        let client_order_id = ClientOrderId::from("O-FILL-100");
        let strategy_id = StrategyId::from("S-QUOTER");

        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id);
        inner.register_order_identity(client_order_id, strategy_id);
        let ocm_state = Arc::new(Mutex::new(inner));

        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let uo = fill_unmatched_order("bet_fill", Some(rfo), Decimal::new(10, 0));

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(processed);

        match rx.try_recv().expect("expected an accepted event") {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.client_order_id, client_order_id);
                assert_eq!(accepted.strategy_id, strategy_id);
            }
            other => panic!("expected a synthesized OrderAccepted, was {other:?}"),
        }

        match rx.try_recv().expect("expected a filled event") {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(filled.strategy_id, strategy_id);
                assert_eq!(filled.venue_order_id, VenueOrderId::from("bet_fill"));
                assert_eq!(filled.last_qty.as_decimal(), Decimal::new(10, 0));
            }
            other => panic!("expected a direct OrderFilled event, was {other:?}"),
        }

        assert!(
            rx.try_recv().is_err(),
            "tracked fill must not also emit a report",
        );
    }

    #[rstest]
    fn test_tracked_fill_on_accepted_order_skips_synth_accept() {
        // Already accepted via HTTP ack: the tracked fill emits only OrderFilled,
        // no duplicate accept.
        let account_id = AccountId::from("BETFAIR-001");
        let client_order_id = ClientOrderId::from("O-FILL-200");
        let strategy_id = StrategyId::from("S-QUOTER");

        let mut inner = OcmState::default();
        inner.register_customer_order_ref(client_order_id);
        inner.register_order_identity(client_order_id, strategy_id);
        inner.mark_accepted(client_order_id); // already accepted via HTTP place ack
        let ocm_state = Arc::new(Mutex::new(inner));

        let (emitter, mut rx) = emitter_with_receiver(account_id);
        let rfo = make_customer_order_ref(client_order_id.as_str());
        let uo = fill_unmatched_order("bet_fill2", Some(rfo), Decimal::new(10, 0));

        let processed = BetfairExecutionClient::process_unmatched_order(
            &uo,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            account_id,
            Currency::from("GBP"),
            &emitter,
            &ocm_state,
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(processed);

        match rx.try_recv().expect("expected a filled event") {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(filled.last_qty.as_decimal(), Decimal::new(10, 0));
            }
            other => panic!("expected OrderFilled with no preceding accept, was {other:?}"),
        }

        assert!(
            rx.try_recv().is_err(),
            "already-accepted order must not re-synthesize OrderAccepted",
        );
    }

    #[rstest]
    fn test_ocm_state_suppress_cancel_for_replaced() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.replaced_venue_order_ids.insert("old_bet".to_string());
        assert!(state.should_suppress_cancel(&client_oid, "old_bet"));
        assert!(!state.should_suppress_cancel(&client_oid, "new_bet"));
    }

    #[rstest]
    fn test_ocm_state_suppress_cancel_for_pending_replace() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.register_pending_replace(client_oid, "old_bet".to_string(), None);

        assert!(state.should_suppress_cancel(&client_oid, "old_bet"));
        assert!(!state.should_suppress_cancel(&client_oid, "other_bet"));
    }

    #[rstest]
    fn test_ocm_state_cleanup_terminal_with_pending_replace() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.register_customer_order_ref(client_oid);
        state.register_pending_replace(client_oid, "old_bet".to_string(), None);

        // Should NOT remove refs because replace is pending
        state.cleanup_terminal_order(&client_oid);
        let rfo = make_customer_order_ref(client_oid.as_str());
        assert!(state.resolve_client_order_id(Some(&rfo)).is_some());
    }

    #[rstest]
    fn test_ocm_state_cleanup_terminal_without_pending() {
        let mut state = OcmState::default();
        let client_oid = ClientOrderId::from("O-001");

        state.register_customer_order_ref(client_oid);

        // Should remove refs because no pending replace
        state.cleanup_terminal_order(&client_oid);
        let rfo = make_customer_order_ref(client_oid.as_str());
        assert!(state.resolve_client_order_id(Some(&rfo)).is_none());
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders() {
        let mut state = OcmState::default();

        let orders = vec![
            OrderSyncEntry {
                bet_id: "bet1".to_string(),
                client_order_id: ClientOrderId::from("O-001"),
                filled_qty: Decimal::new(10, 0),
                avg_px: Decimal::new(25, 1),
                is_closed: false,
                trade_ids: Vec::new(),
            },
            OrderSyncEntry {
                bet_id: "bet2".to_string(),
                client_order_id: ClientOrderId::from("O-002"),
                filled_qty: Decimal::new(5, 0),
                avg_px: Decimal::new(30, 1),
                is_closed: true,
                trade_ids: Vec::new(),
            },
        ];

        state.sync_from_orders(&orders);

        // Open order: should have customer_order_ref registered
        let rfo1 = make_customer_order_ref("O-001");
        assert!(state.resolve_client_order_id(Some(&rfo1)).is_some());

        // Closed order: should be in terminal_orders, no customer_order_ref
        assert!(state.terminal_orders.contains("bet2"));
        let rfo2 = make_customer_order_ref("O-002");
        assert!(state.resolve_client_order_id(Some(&rfo2)).is_none());
    }

    #[rstest]
    fn test_sync_ocm_state_preserves_per_bet_fill_cursors_after_replace() {
        let trader_id = TraderId::from("TESTER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let client_order_id = ClientOrderId::from("O-SYNC-REPLACE");
        let account_id = AccountId::from("BETFAIR-001");
        let old_bet_id = VenueOrderId::from("old-bet");
        let new_bet_id = VenueOrderId::from("new-bet");
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from(10))
            .price(Price::from("3.0"))
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                old_bet_id,
                account_id,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(1),
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Filled(OrderFilled::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                old_bet_id,
                account_id,
                TradeId::from("old-bet-2"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from(2),
                Price::from("3.0"),
                Currency::GBP(),
                LiquiditySide::Maker,
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                None,
                None,
                None,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Updated(OrderUpdated::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                Quantity::from(10),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                Some(new_bet_id),
                Some(account_id),
                Some(Price::from("4.0")),
                None,
                None,
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Filled(OrderFilled::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                new_bet_id,
                account_id,
                TradeId::from("new-bet-1"),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from(1),
                Price::from("4.0"),
                Currency::GBP(),
                LiquiditySide::Taker,
                UUID4::new(),
                UnixNanos::from(4),
                UnixNanos::from(4),
                false,
                None,
                None,
                None,
            )))
            .unwrap();

        let cache = Rc::new(RefCell::new(Cache::default()));
        cache
            .borrow_mut()
            .add_order(order, None, Some(ClientId::from("BETFAIR-SYNC")), false)
            .unwrap();
        let config = BetfairExecConfig::default();
        let core = ExecutionClientCore::new(
            trader_id,
            ClientId::from("BETFAIR-SYNC"),
            *BETFAIR_VENUE,
            OmsType::Netting,
            account_id,
            AccountType::Betting,
            None,
            cache,
        );
        let credential = BetfairCredential::new(
            "username".to_string(),
            "password".to_string(),
            "app-key".to_string(),
        );
        let http_client =
            BetfairHttpClient::new(credential.clone(), None, None, None, None, None, None).unwrap();
        let client = BetfairExecutionClient::new(
            core,
            http_client,
            credential,
            config.stream_config(),
            config,
            Currency::GBP(),
        );

        client.sync_ocm_state_from_cache();

        let mut state = client.ocm_state.lock().unwrap();
        let old_increment = state.fill_tracker.advance_cumulative_fill(
            old_bet_id.as_str(),
            Decimal::from(3),
            Some(Decimal::from(3)),
            Decimal::from(3),
        );
        let current_replay = state.fill_tracker.advance_cumulative_fill(
            new_bet_id.as_str(),
            Decimal::ONE,
            Some(Decimal::from(4)),
            Decimal::from(4),
        );

        assert_eq!(
            old_increment.map(|(_, quantity, price)| (quantity, price)),
            Some((Quantity::from(1), Price::from("3.0"))),
        );
        assert_eq!(current_replay, None);
    }

    #[rstest]
    fn test_sync_cached_fills_restores_terminal_void_cursor() {
        let trader_id = TraderId::from("TESTER-001");
        let strategy_id = StrategyId::from("S-001");
        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let client_order_id = ClientOrderId::from("O-SYNC-VOID");
        let account_id = AccountId::from("BETFAIR-001");
        let venue_order_id = VenueOrderId::from("voided-bet");
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .trader_id(trader_id)
            .strategy_id(strategy_id)
            .instrument_id(instrument_id)
            .client_order_id(client_order_id)
            .quantity(Quantity::from(10))
            .price(Price::from("3.0"))
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(1),
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::FillVoided(OrderFillVoided::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                Ustr::from("voided-bet-sv"),
                TradeId::from("VOID-voided-bet"),
                Quantity::from(10),
                None,
                OrderSide::Buy,
                OrderType::Limit,
                Price::from("3.0"),
                Currency::GBP(),
                LiquiditySide::NoLiquiditySide,
                None,
                None,
                None,
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                false,
            )))
            .unwrap();

        let mut state = OcmState::default();
        BetfairExecutionClient::sync_cached_fills(&mut state, [&order]);
        let mut replay = cancel_unmatched_order(venue_order_id.as_str(), None);
        replay.s = Decimal::from(10);
        replay.sc = None;
        replay.sm = Some(Decimal::ZERO);
        replay.sv = Some(Decimal::from(10));

        assert_eq!(order.status(), OrderStatus::Voided);
        assert_eq!(order.voided_qty(), Quantity::from(10));
        assert!(!state.fill_tracker.has_unseen_fill_void(&replay));
        assert!(state.fill_tracker.maybe_fill_voids(&replay).is_empty());
    }

    #[rstest]
    #[tokio::test]
    #[ignore = "requires authorized live Betfair mainnet access"]
    async fn live_execution_reconnect_reconciles_before_resuming() {
        let credential = BetfairCredential::from_env()
            .expect("BETFAIR_USERNAME, BETFAIR_PASSWORD, and BETFAIR_APP_KEY must be set");
        let http_client = BetfairHttpClient::new(
            credential.clone(),
            None,
            None,
            None,
            None,
            Some(5),
            Some(20),
        )
        .expect("live HTTP client");
        http_client.connect().await.expect("Betfair login");

        let account_details: AccountDetailsResponse = http_client
            .send_accounts(METHOD_GET_ACCOUNT_DETAILS, serde_json::json!({}))
            .await
            .expect("account details");
        let currency_code = account_details
            .currency_code
            .expect("account details must include currencyCode");
        let currency = currency_code
            .as_str()
            .parse::<Currency>()
            .expect("registered account currency");

        let config = BetfairExecConfig {
            account_currency: currency_code.to_string(),
            calculate_account_state: false,
            ignore_external_orders: true,
            ..Default::default()
        };
        let stream_config = config.stream_config();
        let cache = Rc::new(RefCell::new(Cache::default()));
        let core = ExecutionClientCore::new(
            config.trader_id,
            ClientId::from("BETFAIR-LIVE-SMOKE"),
            *BETFAIR_VENUE,
            OmsType::Netting,
            config.account_id,
            AccountType::Betting,
            None,
            cache,
        );

        let (exec_tx, mut exec_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_exec_event_sender(exec_tx);
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        replace_data_event_sender(data_tx);

        let mut client = BetfairExecutionClient::new(
            core,
            http_client,
            credential,
            stream_config,
            config,
            currency,
        );
        client.start().expect("execution client start");
        client.connect().await.expect("execution client connect");

        while exec_rx.try_recv().is_ok() {}

        let stream_client = Arc::clone(
            client
                .stream_client
                .as_ref()
                .expect("execution stream after connect"),
        );
        assert!(
            stream_client.request_reconnect(),
            "live smoke must start a stream transport replacement",
        );

        let (order_count, fill_count) = tokio::time::timeout(Duration::from_secs(30), async {
            loop {
                if let Some(ExecutionEvent::Report(ExecutionReport::MassStatus(status))) =
                    exec_rx.recv().await
                {
                    let fill_count: usize = status.fill_reports().values().map(Vec::len).sum();
                    break (status.order_reports().len(), fill_count);
                }
            }
        })
        .await
        .expect("post-reconnect mass status within 30 seconds");

        nautilus_common::testing::wait_until_async(
            || {
                let halted = client.is_reconciling();
                async move { !halted }
            },
            Duration::from_secs(5),
        )
        .await;

        assert!(stream_client.is_active());
        assert!(!client.is_reconciling());
        eprintln!(
            "Betfair read-only reconnect smoke completed: orders={order_count}, fills={fill_count}",
        );

        client
            .disconnect()
            .await
            .expect("execution client disconnect");
    }

    #[rstest]
    fn test_reconnect_signal_not_sent_on_initial_connection() {
        let has_initial_connection = Arc::new(AtomicBool::new(false));
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let has_initial = Arc::clone(&has_initial_connection);
        let handler = move |_data: &[u8]| {
            if has_initial.swap(true, Ordering::SeqCst) {
                let _ = reconnect_tx.send(());
            }
        };

        // First connection message: no signal
        handler(br#"{"op":"connection","connectionId":"abc"}"#);
        assert!(reconnect_rx.try_recv().is_err());
        assert!(has_initial_connection.load(Ordering::SeqCst));
    }

    #[rstest]
    fn test_reconnect_signal_sent_on_subsequent_connection() {
        let has_initial_connection = Arc::new(AtomicBool::new(false));
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel::<()>();

        let has_initial = Arc::clone(&has_initial_connection);
        let tx = reconnect_tx;
        let handler = move |_data: &[u8]| {
            if has_initial.swap(true, Ordering::SeqCst) {
                let _ = tx.send(());
            }
        };

        // First connection: no signal
        handler(br#"{"op":"connection","connectionId":"abc"}"#);
        assert!(reconnect_rx.try_recv().is_err());

        // Second connection: signal sent
        handler(br#"{"op":"connection","connectionId":"def"}"#);
        assert!(reconnect_rx.try_recv().is_ok());

        // Third connection: signal sent again
        handler(br#"{"op":"connection","connectionId":"ghi"}"#);
        assert!(reconnect_rx.try_recv().is_ok());
    }

    #[rstest]
    fn test_reconnect_sets_reconciliation_gate() {
        let pending_resync = Arc::new(AtomicBool::new(false));
        let reconciliation_gate = Arc::new(ReconciliationGate::default());
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = ocm_reconnect_handler(
            reconnect_tx,
            Arc::clone(&pending_resync),
            Arc::clone(&reconciliation_gate),
        );

        handler(stream_decode(br#"{"op":"connection","connectionId":"first"}"#).unwrap());
        assert!(reconnect_rx.try_recv().is_err());
        assert!(!pending_resync.load(Ordering::Acquire));
        assert!(!reconciliation_gate.is_halted());

        handler(stream_decode(br#"{"op":"connection","connectionId":"second"}"#).unwrap());
        assert_eq!(reconnect_rx.try_recv().unwrap(), 1);
        assert!(pending_resync.load(Ordering::Acquire));
        assert!(reconciliation_gate.is_halted());
    }

    #[rstest]
    fn test_first_connection_after_transport_loss_schedules_reconciliation() {
        let pending_resync = Arc::new(AtomicBool::new(false));
        let reconciliation_gate = Arc::new(ReconciliationGate::default());
        let (reconnect_tx, mut reconnect_rx) = tokio::sync::mpsc::unbounded_channel();
        let handler = ocm_reconnect_handler(
            reconnect_tx,
            Arc::clone(&pending_resync),
            Arc::clone(&reconciliation_gate),
        );

        assert_eq!(reconciliation_gate.halt(), 1);
        handler(stream_decode(br#"{"op":"connection","connectionId":"replacement"}"#).unwrap());

        assert_eq!(reconnect_rx.try_recv().unwrap(), 3);
        assert!(pending_resync.load(Ordering::Acquire));
        assert!(reconciliation_gate.is_halted());
    }

    fn ocm_reconnect_handler(
        reconnect_tx: tokio::sync::mpsc::UnboundedSender<u64>,
        pending_resync: Arc<AtomicBool>,
        reconciliation_gate: Arc<ReconciliationGate>,
    ) -> StreamMessageHandler {
        let account_id = AccountId::from("BETFAIR-001");
        let (emitter, _execution_rx) = emitter_with_receiver(account_id);
        let (data_tx, _data_rx) = tokio::sync::mpsc::unbounded_channel();
        let (account_refresh_tx, _account_refresh_rx) = tokio::sync::mpsc::unbounded_channel();

        BetfairExecutionClient::create_ocm_handler(
            emitter,
            account_id,
            Currency::GBP(),
            Arc::new(Mutex::new(OcmState::default())),
            data_tx,
            None,
            false,
            reconnect_tx,
            pending_resync,
            reconciliation_gate,
            Arc::new(Mutex::new(Vec::new())),
            account_refresh_tx,
            get_atomic_clock_realtime(),
        )
    }

    #[rstest]
    fn test_reconciliation_gate_rejects_stale_completion() {
        let gate = ReconciliationGate::default();

        let stale = gate.halt();
        let current = gate.halt();

        assert!(gate.is_halted());
        assert!(!gate.try_resume(stale));
        assert!(gate.is_halted());
        assert!(gate.try_resume(current));
        assert!(!gate.is_halted());
    }

    #[rstest]
    fn test_ocm_state_persists_across_reconnections() {
        let ocm_state = Arc::new(Mutex::new(OcmState::default()));

        // Populate state before "reconnect"
        {
            let mut state = ocm_state.lock().unwrap();
            let orders = vec![
                OrderSyncEntry {
                    bet_id: "bet1".to_string(),
                    client_order_id: ClientOrderId::from("O-001"),
                    filled_qty: Decimal::new(10, 0),
                    avg_px: Decimal::new(25, 1),
                    is_closed: false,
                    trade_ids: Vec::new(),
                },
                OrderSyncEntry {
                    bet_id: "bet2".to_string(),
                    client_order_id: ClientOrderId::from("O-002"),
                    filled_qty: Decimal::ZERO,
                    avg_px: Decimal::ZERO,
                    is_closed: true,
                    trade_ids: Vec::new(),
                },
            ];
            state.sync_from_orders(&orders);
        }

        // Verify state survives (simulates reconnection where Arc<Mutex<OcmState>> persists)
        let state = ocm_state.lock().unwrap();
        let rfo = make_customer_order_ref("O-001");
        assert_eq!(
            state.resolve_client_order_id(Some(&rfo)),
            Some(ClientOrderId::from("O-001")),
        );
        assert!(state.terminal_orders.contains("bet2"));
        assert!(!state.terminal_orders.contains("bet1"));
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_populates_fill_tracker() {
        let mut state = OcmState::default();

        let orders = vec![OrderSyncEntry {
            bet_id: "bet_fill".to_string(),
            client_order_id: ClientOrderId::from("O-FILL-001"),
            filled_qty: Decimal::new(15, 0),
            avg_px: Decimal::new(30, 1),
            is_closed: false,
            trade_ids: Vec::new(),
        }];

        state.sync_from_orders(&orders);

        // Fill tracker should be pre-populated so that a stream update with
        // sm=15 does NOT produce a duplicate fill
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_fill".to_string(),
            p: Decimal::new(30, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: Some("O-FILL-001".to_string()),
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(30, 1)),
            sm: Some(Decimal::new(15, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };

        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(
            result.is_none(),
            "synced fill should prevent duplicate fill report"
        );
    }

    #[rstest]
    fn test_terminal_stream_state_prevents_rest_fill_replay() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let response: CurrentOrderSummaryReport = parse_jsonrpc(&data);
        let mut state = OcmState::default();
        let account_id = AccountId::from("BETFAIR-001");
        let currency = Currency::GBP();

        let mut stream_fill_count = 0;

        for order in &response.current_orders {
            let size_matched = order.size_matched.unwrap_or(Decimal::ZERO);

            if state
                .fill_tracker
                .advance_cumulative_fill(
                    &order.bet_id,
                    size_matched,
                    order.average_price_matched,
                    order.price_size.price,
                )
                .is_some()
            {
                stream_fill_count += 1;
            }
            state.mark_terminal_order(order.bet_id.clone());
        }

        let replay = build_incremental_fill_reports(
            &response.current_orders,
            &mut state,
            account_id,
            currency,
            UnixNanos::default(),
        );

        assert_eq!(stream_fill_count, 2);
        assert!(replay.is_empty());
    }

    #[rstest]
    fn test_terminal_marker_without_fill_state_allows_rest_recovery() {
        let data = load_test_json("rest/list_current_orders_execution_complete.json");
        let response: CurrentOrderSummaryReport = parse_jsonrpc(&data);
        let mut state = OcmState::default();
        for order in &response.current_orders {
            state.mark_terminal_order(order.bet_id.clone());
        }

        let reports = build_incremental_fill_reports(
            &response.current_orders,
            &mut state,
            AccountId::from("BETFAIR-001"),
            Currency::GBP(),
            UnixNanos::default(),
        );

        assert!(!reports.is_empty());
    }

    #[rstest]
    fn test_first_seen_rest_void_does_not_suppress_later_matched_quantity() {
        let mut order = make_summary(
            "bet_void_then_fill",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::ExecutionComplete,
            "2026-04-18T10:00:00Z",
        );
        order.price_size.size = Decimal::new(60, 0);
        order.size_matched = Some(Decimal::ZERO);
        order.size_remaining = Some(Decimal::ZERO);
        order.size_voided = Some(Decimal::new(50, 0));
        let mut state = OcmState::default();
        let account_id = AccountId::from("BETFAIR-001");

        let first = build_incremental_fill_reports(
            &[order.clone()],
            &mut state,
            account_id,
            Currency::GBP(),
            UnixNanos::default(),
        );
        order.size_matched = Some(Decimal::new(10, 0));
        order.average_price_matched = Some(Decimal::new(25, 1));
        let later = build_incremental_fill_reports(
            &[order],
            &mut state,
            account_id,
            Currency::GBP(),
            UnixNanos::default(),
        );

        assert!(first.is_empty());
        assert_eq!(later.len(), 1);
        assert_eq!(later[0].last_qty, Quantity::from("10.00"));
        assert_eq!(later[0].last_px.as_decimal(), Decimal::new(25, 1));
    }

    #[rstest]
    fn test_rest_void_with_applied_fill_lots_remains_unseen_until_applied() {
        let bet_id = "bet_rest_void_pending";
        let mut state = OcmState::default();
        let initial = state.fill_tracker.advance_cumulative_fill(
            bet_id,
            Decimal::new(50, 0),
            Some(Decimal::new(20, 1)),
            Decimal::new(20, 1),
        );
        let mut order = make_summary(
            bet_id,
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::ExecutionComplete,
            "2026-04-18T10:00:00Z",
        );
        order.price_size.size = Decimal::new(70, 0);
        order.size_matched = Some(Decimal::new(50, 0));
        order.size_remaining = Some(Decimal::ZERO);
        order.size_voided = Some(Decimal::new(20, 0));
        order.average_price_matched = Some(Decimal::new(20, 1));

        let reports = build_incremental_fill_reports(
            &[order],
            &mut state,
            AccountId::from("BETFAIR-001"),
            Currency::GBP(),
            UnixNanos::default(),
        );
        let update = crate::stream::messages::UnmatchedOrder {
            id: bet_id.to_string(),
            p: Decimal::new(20, 1),
            s: Decimal::new(70, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::ExecutionComplete,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: None,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(20, 1)),
            sm: Some(Decimal::new(50, 0)),
            sr: Some(Decimal::ZERO),
            sl: None,
            sc: None,
            sv: Some(Decimal::new(20, 0)),
            lsrc: None,
        };

        assert!(initial.is_some());
        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].last_qty, Quantity::from("20.00"));
        assert!(state.fill_tracker.has_unseen_fill_void(&update));
    }

    #[rstest]
    fn test_ocm_state_sync_seeds_published_trade_ids_from_cache() {
        use crate::stream::parse::make_trade_id;

        let mut state = OcmState::default();

        // Seeded filled_qty < sm: cumulative-size gate would pass, so only
        // the trade-ids seeded by sync_from_orders can block the fill
        let uo_for_id = crate::stream::messages::UnmatchedOrder {
            id: "bet_seed".to_string(),
            p: Decimal::new(25, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: Some("O-SEED-001".to_string()),
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(25, 1)),
            sm: Some(Decimal::new(8, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };
        let trade_id = make_trade_id(&uo_for_id).to_string();

        let orders = vec![OrderSyncEntry {
            bet_id: "bet_seed".to_string(),
            client_order_id: ClientOrderId::from("O-SEED-001"),
            filled_qty: Decimal::new(4, 0),
            avg_px: Decimal::new(25, 1),
            is_closed: false,
            trade_ids: vec![trade_id],
        }];

        state.sync_from_orders(&orders);

        let result = state.fill_tracker.maybe_fill_report(
            &uo_for_id,
            uo_for_id.s,
            InstrumentId::from("1.234567-12345-0.0.BETFAIR"),
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert!(
            result.is_none(),
            "sync_from_orders must seed trade-ids so a known fill is not re-emitted \
             even when the cumulative-size gate would pass",
        );
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_incremental_fill_after_sync() {
        let mut state = OcmState::default();

        let orders = vec![OrderSyncEntry {
            bet_id: "bet_inc".to_string(),
            client_order_id: ClientOrderId::from("O-INC-001"),
            filled_qty: Decimal::new(10, 0),
            avg_px: Decimal::new(25, 1),
            is_closed: false,
            trade_ids: Vec::new(),
        }];

        state.sync_from_orders(&orders);

        // Stream update with sm=18 (8 more than synced 10)
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_inc".to_string(),
            p: Decimal::new(25, 1),
            s: Decimal::new(20, 0),
            side: crate::common::enums::StreamingSide::Lay,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Persist),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: Some("O-INC-001".to_string()),
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(26, 1)),
            sm: Some(Decimal::new(18, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };

        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        let fill = result.expect("should produce incremental fill of 8");
        assert_eq!(fill.last_qty, Quantity::from("8.00"));
    }

    #[rstest]
    fn test_ocm_state_sync_from_orders_zero_filled_not_synced() {
        let mut state = OcmState::default();

        let orders = vec![OrderSyncEntry {
            bet_id: "bet_zero".to_string(),
            client_order_id: ClientOrderId::from("O-ZERO-001"),
            filled_qty: Decimal::ZERO,
            avg_px: Decimal::ZERO,
            is_closed: false,
            trade_ids: Vec::new(),
        }];

        state.sync_from_orders(&orders);

        // RFO should still be registered even if no fills
        let rfo = make_customer_order_ref("O-ZERO-001");
        assert!(state.resolve_client_order_id(Some(&rfo)).is_some());

        // A stream update with sm=5 should produce a fill (not blocked by sync)
        let uo = crate::stream::messages::UnmatchedOrder {
            id: "bet_zero".to_string(),
            p: Decimal::new(30, 1),
            s: Decimal::new(10, 0),
            side: crate::common::enums::StreamingSide::Back,
            status: crate::common::enums::StreamingOrderStatus::Executable,
            pt: Some(crate::common::enums::StreamingPersistenceType::Lapse),
            ot: crate::common::enums::StreamingOrderType::Limit,
            pd: 1617863365000,
            bsp: None,
            rfo: None,
            rfs: None,
            rc: None,
            rac: None,
            md: None,
            cd: None,
            ld: None,
            avp: Some(Decimal::new(30, 1)),
            sm: Some(Decimal::new(5, 0)),
            sr: None,
            sl: None,
            sc: None,
            sv: None,
            lsrc: None,
        };
        let instrument_id = InstrumentId::from("1.234567-12345-0.0.BETFAIR");
        let result = state.fill_tracker.maybe_fill_report(
            &uo,
            uo.s,
            instrument_id,
            AccountId::from("BETFAIR-001"),
            Currency::from("GBP"),
            UnixNanos::default(),
            UnixNanos::default(),
        );
        assert!(
            result.is_some(),
            "zero-filled order should not block new fills"
        );
    }

    #[rstest]
    fn test_ocm_state_sync_multiple_open_and_closed() {
        let mut state = OcmState::default();

        let orders = vec![
            OrderSyncEntry {
                bet_id: "bet_a".to_string(),
                client_order_id: ClientOrderId::from("O-A"),
                filled_qty: Decimal::new(5, 0),
                avg_px: Decimal::new(20, 1),
                is_closed: false,
                trade_ids: Vec::new(),
            },
            OrderSyncEntry {
                bet_id: "bet_b".to_string(),
                client_order_id: ClientOrderId::from("O-B"),
                filled_qty: Decimal::ZERO,
                avg_px: Decimal::ZERO,
                is_closed: true,
                trade_ids: Vec::new(),
            },
            OrderSyncEntry {
                bet_id: "bet_c".to_string(),
                client_order_id: ClientOrderId::from("O-C"),
                filled_qty: Decimal::new(100, 0),
                avg_px: Decimal::new(15, 1),
                is_closed: true,
                trade_ids: Vec::new(),
            },
            OrderSyncEntry {
                bet_id: "bet_d".to_string(),
                client_order_id: ClientOrderId::from("O-D"),
                filled_qty: Decimal::ZERO,
                avg_px: Decimal::ZERO,
                is_closed: false,
                trade_ids: Vec::new(),
            },
        ];

        state.sync_from_orders(&orders);

        // Open orders have RFO registered
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-A")))
                .is_some()
        );
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-D")))
                .is_some()
        );

        // Closed orders are terminal
        assert!(state.terminal_orders.contains("bet_b"));
        assert!(state.terminal_orders.contains("bet_c"));
        assert!(!state.terminal_orders.contains("bet_a"));
        assert!(!state.terminal_orders.contains("bet_d"));

        // Closed orders do NOT get RFO registered
        assert!(
            state
                .resolve_client_order_id(Some(&make_customer_order_ref("O-B")))
                .is_none()
        );
    }

    fn make_summary(
        bet_id: &str,
        market_id: &str,
        selection_id: u64,
        handicap: Decimal,
        status: BetfairOrderStatus,
        placed_date: &str,
    ) -> CurrentOrderSummary {
        CurrentOrderSummary {
            bet_id: bet_id.to_string(),
            market_id: market_id.to_string(),
            selection_id,
            handicap,
            price_size: crate::http::models::PriceSize {
                price: Decimal::new(20, 1),
                size: Decimal::new(10, 0),
            },
            bsp_liability: Decimal::ZERO,
            side: BetfairSide::Back,
            status,
            persistence_type: PersistenceType::Lapse,
            order_type: BetfairOrderType::Limit,
            placed_date: placed_date.to_string(),
            matched_date: None,
            average_price_matched: None,
            size_matched: None,
            size_remaining: Some(Decimal::new(10, 0)),
            size_lapsed: None,
            size_cancelled: None,
            size_voided: None,
            regulator_auth_code: None,
            regulator_code: None,
            customer_order_ref: None,
            customer_strategy_ref: None,
        }
    }

    #[rstest]
    fn test_select_order_for_query_single_executable() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_1"));
    }

    #[rstest]
    fn test_select_order_for_query_single_terminal() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::ExecutionComplete,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_1"));
    }

    #[rstest]
    fn test_select_order_for_query_replace_prefers_executable() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_multiple_executable_prefers_most_recent() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_multiple_terminal_prefers_most_recent() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_old",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_new",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_new"));
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_without_vid_returns_none() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_foreign",
            "1.999",
            99999,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_with_vid_match_returns_match() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![make_summary(
            "bet_foreign",
            "1.999",
            99999,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);
        let vid = VenueOrderId::from("bet_foreign");

        let selected = select_order_for_query(&orders, expected, cid, Some(vid));
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_foreign"));
    }

    #[rstest]
    fn test_select_order_for_query_foreign_only_vid_mismatch_returns_none() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_foreign_1",
                "1.999",
                99999,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_foreign_2",
                "1.888",
                88888,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);
        let vid = VenueOrderId::from("bet_unknown");

        let selected = select_order_for_query(&orders, expected, cid, Some(vid));
        assert!(selected.is_none());
    }

    #[rstest]
    fn test_select_order_for_query_mixed_returns_matching_instrument() {
        let cid = ClientOrderId::from("O-001");
        let orders = vec![
            make_summary(
                "bet_foreign",
                "1.999",
                99999,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:05:00Z",
            ),
            make_summary(
                "bet_match",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::ExecutionComplete,
                "2026-04-18T10:00:00Z",
            ),
        ];
        let expected = make_instrument_id("1.100", 12345, Decimal::ZERO);

        let selected = select_order_for_query(&orders, expected, cid, None);
        assert_eq!(selected.map(|o| o.bet_id.as_str()), Some("bet_match"));
    }

    #[rstest]
    fn test_extend_unique_filters_duplicates() {
        let mut candidates: Vec<CurrentOrderSummary> = Vec::new();
        let mut seen: AHashSet<String> = AHashSet::new();

        let orders = vec![
            make_summary(
                "bet_1",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:00:00Z",
            ),
            make_summary(
                "bet_1",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:01:00Z",
            ),
            make_summary(
                "bet_2",
                "1.100",
                12345,
                Decimal::ZERO,
                BetfairOrderStatus::Executable,
                "2026-04-18T10:02:00Z",
            ),
        ];

        extend_unique(&mut candidates, &mut seen, orders);

        assert_eq!(candidates.len(), 2);
        assert_eq!(candidates[0].bet_id, "bet_1");
        assert_eq!(candidates[0].placed_date, "2026-04-18T10:00:00Z");
        assert_eq!(candidates[1].bet_id, "bet_2");
        assert!(seen.contains("bet_1"));
        assert!(seen.contains("bet_2"));
    }

    #[rstest]
    fn test_extend_unique_skips_already_seen() {
        let mut candidates: Vec<CurrentOrderSummary> = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:00:00Z",
        )];
        let mut seen: AHashSet<String> = AHashSet::new();
        seen.insert("bet_1".to_string());

        let orders = vec![make_summary(
            "bet_1",
            "1.100",
            12345,
            Decimal::ZERO,
            BetfairOrderStatus::Executable,
            "2026-04-18T10:05:00Z",
        )];

        extend_unique(&mut candidates, &mut seen, orders);

        assert_eq!(candidates.len(), 1);
        assert_eq!(candidates[0].placed_date, "2026-04-18T10:00:00Z");
    }

    #[rstest]
    fn test_list_current_orders_filter_bet_id_sets_only_bet_ids() {
        let params = list_current_orders_filter_bet_id("bet_abc".to_string());

        assert_eq!(
            params.bet_ids.as_deref(),
            Some(&["bet_abc".to_string()][..])
        );
        assert!(params.customer_order_refs.is_none());
        assert!(params.market_ids.is_none());
        assert!(params.order_projection.is_none());
        assert!(params.customer_strategy_refs.is_none());
        assert!(params.date_range.is_none());
        assert!(params.order_by.is_none());
        assert!(params.sort_dir.is_none());
        assert!(params.from_record.is_none());
        assert!(params.record_count.is_none());
    }

    #[rstest]
    fn test_list_current_orders_filter_ref_sets_only_customer_order_refs() {
        let params = list_current_orders_filter_ref("rfo_abc".to_string());

        assert_eq!(
            params.customer_order_refs.as_deref(),
            Some(&["rfo_abc".to_string()][..])
        );
        assert!(params.bet_ids.is_none());
        assert!(params.market_ids.is_none());
        assert!(params.order_projection.is_none());
        assert!(params.customer_strategy_refs.is_none());
        assert!(params.date_range.is_none());
        assert!(params.order_by.is_none());
        assert!(params.sort_dir.is_none());
        assert!(params.from_record.is_none());
        assert!(params.record_count.is_none());
    }
}
