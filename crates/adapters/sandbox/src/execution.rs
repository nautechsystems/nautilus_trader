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

//! Sandbox execution client implementation.

use std::{cell::RefCell, collections::BinaryHeap, fmt::Debug, rc::Rc};

use ahash::{AHashMap, AHashSet};
use async_trait::async_trait;
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    clock::Clock,
    factories::OrderEventFactory,
    live::try_get_exec_event_sender,
    messages::{
        ExecutionEvent,
        execution::{
            BatchCancelOrders, BatchModifyOrders, CancelAllOrders, CancelOrder,
            GenerateFillReports, GenerateOrderStatusReport, GenerateOrderStatusReports,
            GeneratePositionStatusReports, ModifyOrder, QueryAccount, QueryOrder, SubmitOrder,
            SubmitOrderList, TradingCommand,
        },
    },
    msgbus::{
        self, MStr, MessagingSwitchboard, Pattern, TypedHandler,
        typed_handler::ShareableMessageHandler,
    },
    runner::{OrderEventDispatchGuard, order_event_is_dispatching},
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{Params, UUID4, UnixNanos, WeakCell, datetime::NANOSECONDS_IN_SECOND};
use nautilus_execution::{
    client::core::ExecutionClientCore,
    matching_engine::OrderMatchingEngine,
    models::{fee::FeeModelHandle, fill::FillModelHandle, latency::LatencyModel},
};
use nautilus_model::{
    accounts::AccountAny,
    data::{Bar, InstrumentClose, InstrumentStatus, OrderBookDeltas, QuoteTick, TradeTick},
    enums::OmsType,
    events::{
        OrderCancelRejected, OrderEventAny, OrderModifyRejected, OrderRejected, PositionEvent,
    },
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, MarginBalance, Money},
};
use ustr::Ustr;

use crate::config::SandboxExecutionClientConfig;

/// Interval between periodic sweeps that retire expired matching engines with no open position.
///
/// This bounds retained matching-engine and cache state for quote-only instruments that expire
/// without an `InstrumentClose`, expired-order, or `PositionClosed` event to trigger cleanup.
const EXPIRED_ENGINE_SWEEP_INTERVAL_NS: u64 = 60 * NANOSECONDS_IN_SECOND;

/// Inner state for the sandbox execution client.
///
/// This is wrapped in `Rc<RefCell<>>` so message handlers can hold weak references.
struct SandboxInner {
    /// Dynamic clock for matching engines.
    clock: Rc<RefCell<dyn Clock>>,
    /// Reference to the cache.
    cache: Rc<RefCell<Cache>>,
    /// The sandbox configuration.
    config: SandboxExecutionClientConfig,
    /// Shared fill-model handle for every matching engine on this client.
    fill_model: FillModelHandle,
    /// Matching engines per instrument.
    matching_engines: AHashMap<InstrumentId, OrderMatchingEngine>,
    /// Next raw ID assigned to a matching engine.
    next_engine_raw_id: u32,
    /// Current account balances.
    balances: AHashMap<String, Money>,
    /// Order-event handler shared by this client and every matching engine it owns.
    event_handler: Rc<dyn Fn(OrderEventAny)>,
    /// The route every order event this client emits takes.
    router: Rc<EventRouter>,
    /// Inbound commands deferred by latency, ordered as a min-heap by due time.
    inbound_queue: BinaryHeap<DelayedCommand>,
    /// Monotonic sequence providing FIFO tie-breaking for deferred commands sharing a due time, so
    /// no queued command can be overtaken by one enqueued after it.
    inbound_seq: u64,
    /// The execution client identifier (alert name for the inbound drain).
    client_id: ClientId,
    /// The account identifier applied to deferred engine commands.
    account_id: AccountId,
    /// Weak self-reference, so the inbound-drain timer callback can reach this state.
    self_weak: WeakCell<Self>,
}

/// Forwards an order event onward from a sandbox client, installed by `start` when a runner is
/// bound.
type EventSink = Rc<dyn Fn(OrderEventAny)>;

/// The single route every order event a sandbox client emits takes.
#[derive(Default)]
struct EventRouter {
    /// Installed while one command is applied, so its events can be flushed once the inner borrow
    /// is released.
    capture: RefCell<Option<Vec<OrderEventAny>>>,
    /// Forwards to the runner's execution channel.
    sink: RefCell<Option<EventSink>>,
    /// Raised while an order-event dispatch was already in progress, and flushed once it returns.
    deferred: RefCell<Vec<OrderEventAny>>,
}

impl Debug for EventRouter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(EventRouter))
            .finish_non_exhaustive()
    }
}

impl EventRouter {
    /// Routes one event: into the capture buffer while a command is being applied, otherwise out
    /// through the sink or the message bus.
    fn dispatch(&self, event: OrderEventAny) {
        if let Some(buffer) = self.capture.borrow_mut().as_mut() {
            buffer.push(event);
            return;
        }

        // Cloned out so the borrow is released before the sink can re-enter
        let sink = self.sink.borrow().clone();
        if let Some(sink) = sink {
            sink(event);
        } else {
            msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), event);
        }
    }

    /// Sends `events` into the execution engine in order, each processed into the cache before the
    /// next: the settlement barrier itself.
    fn flush(&self, events: Vec<OrderEventAny>) {
        if order_event_is_dispatching() {
            self.deferred.borrow_mut().extend(events);
            return;
        }

        let endpoint = MessagingSwitchboard::exec_engine_process();
        let mut batch = events;
        loop {
            if !batch.is_empty() {
                let _dispatching = OrderEventDispatchGuard::enter();

                for event in batch {
                    msgbus::send_order_event(endpoint, event);
                }
            }

            batch = std::mem::take(&mut *self.deferred.borrow_mut());
            if batch.is_empty() {
                return;
            }
        }
    }
}

/// A [`TradingCommand`] deferred by inbound latency, ordered by `due_ns` then `seq` so the
/// `BinaryHeap` behaves as a min-heap for FIFO draining.
#[derive(Debug, Eq, PartialEq)]
struct DelayedCommand {
    due_ns: UnixNanos,
    seq: u64,
    command: TradingCommand,
}

impl Ord for DelayedCommand {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Reverse ordering for min-heap (earliest due time first then lowest sequence)
        other
            .due_ns
            .cmp(&self.due_ns)
            .then_with(|| other.seq.cmp(&self.seq))
    }
}

impl PartialOrd for DelayedCommand {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

/// An RAII installer for the buffer [`SandboxInner::drain_inbound`] captures one command's events
/// into.
struct EventCaptureGuard(Rc<EventRouter>);

impl EventCaptureGuard {
    fn install(router: &Rc<EventRouter>) -> Self {
        router.capture.replace(Some(Vec::new()));
        Self(router.clone())
    }

    fn take(self) -> Vec<OrderEventAny> {
        self.0.capture.replace(None).unwrap_or_default()
    }
}

impl Drop for EventCaptureGuard {
    fn drop(&mut self) {
        self.0.capture.replace(None);
    }
}

fn inbound_alert_name(client_id: ClientId) -> String {
    format!("SANDBOX-INBOUND-{client_id}")
}

fn check_quote_or_drop(context: &str, quote: &QuoteTick, instrument: &InstrumentAny) -> bool {
    if quote_matches_instrument_precision(quote, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (bid_px={}, ask_px={}, bid_sz={}, ask_sz={}, expected_price={}, expected_size={})",
        instrument.id(),
        quote.bid_price.precision,
        quote.ask_price.precision,
        quote.bid_size.precision,
        quote.ask_size.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn check_trade_or_drop(context: &str, trade: &TradeTick, instrument: &InstrumentAny) -> bool {
    if trade_matches_instrument_precision(trade, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (px={}, sz={}, expected_price={}, expected_size={})",
        instrument.id(),
        trade.price.precision,
        trade.size.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn check_bar_or_drop(context: &str, bar: &Bar, instrument: &InstrumentAny) -> bool {
    if bar_matches_instrument_precision(bar, instrument) {
        return true;
    }

    log::warn!(
        "Dropping {context} for {} due to precision mismatch \
         (open={}, high={}, low={}, close={}, volume={}, expected_price={}, expected_size={})",
        instrument.id(),
        bar.open.precision,
        bar.high.precision,
        bar.low.precision,
        bar.close.precision,
        bar.volume.precision,
        instrument.price_precision(),
        instrument.size_precision(),
    );
    false
}

fn quote_matches_instrument_precision(quote: &QuoteTick, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    quote.bid_price.precision == price_precision
        && quote.ask_price.precision == price_precision
        && quote.bid_size.precision == size_precision
        && quote.ask_size.precision == size_precision
}

fn trade_matches_instrument_precision(trade: &TradeTick, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    trade.price.precision == price_precision && trade.size.precision == size_precision
}

fn bar_matches_instrument_precision(bar: &Bar, instrument: &InstrumentAny) -> bool {
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    bar.open.precision == price_precision
        && bar.high.precision == price_precision
        && bar.low.precision == price_precision
        && bar.close.precision == price_precision
        && bar.volume.precision == size_precision
}

impl SandboxInner {
    /// Ensures a matching engine exists for the given instrument.
    fn ensure_matching_engine(&mut self, instrument: &InstrumentAny) {
        let instrument_id = instrument.id();

        if !self.matching_engines.contains_key(&instrument_id) {
            let engine_config = self.config.to_matching_engine_config();
            let fill_model = self.fill_model.clone();
            let fee_model = self
                .config
                .fee_model
                .clone()
                .map(FeeModelHandle::from)
                .unwrap_or_default();
            let raw_id = self.next_engine_raw_id;
            self.next_engine_raw_id = self.next_engine_raw_id.wrapping_add(1);

            let mut engine = OrderMatchingEngine::new(
                instrument.clone(),
                raw_id,
                fill_model,
                fee_model,
                self.config.book_type,
                self.config.oms_type,
                self.config.account_type,
                self.clock.clone(),
                self.cache.clone(),
                engine_config,
            );

            engine.set_event_handler(self.event_handler.clone());

            self.matching_engines.insert(instrument_id, engine);
        }
    }

    /// Processes a quote tick through the matching engine.
    fn process_quote_tick(&mut self, quote: &QuoteTick) {
        let instrument_id = quote.instrument_id;

        // Try to get instrument from cache, create engine if found
        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_quote_or_drop("quote tick", quote, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_quote_tick(quote);
            }
        }
    }

    /// Processes a trade tick through the matching engine.
    fn process_trade_tick(&mut self, trade: &TradeTick) {
        if !self.config.trade_execution {
            return;
        }

        let instrument_id = trade.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_trade_or_drop("trade tick", trade, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_trade_tick(trade);
            }
        }
    }

    /// Processes a bar through the matching engine.
    fn process_bar(&mut self, bar: &Bar) {
        if !self.config.bar_execution {
            return;
        }

        let instrument_id = bar.bar_type.instrument_id();

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            if !check_bar_or_drop("bar", bar, &instrument) {
                return;
            }

            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_bar(bar);
            }
        }
    }

    /// Processes order book deltas through the matching engine.
    fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        let instrument_id = deltas.instrument_id;

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id)
                && let Err(e) = engine.process_order_book_deltas(deltas)
            {
                log::error!("Error processing order book deltas: {e}");
            }
        }
    }

    /// Processes an instrument status update through the matching engine.
    fn process_instrument_status(&mut self, status: &InstrumentStatus) {
        let instrument_id = status.instrument_id;

        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_status(status.action);
            return;
        }

        let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
        if let Some(instrument) = instrument {
            self.ensure_matching_engine(&instrument);

            if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                engine.process_status(status.action);
            }
        } else {
            log::warn!(
                "Ignoring instrument status for {instrument_id}: instrument missing from cache",
            );
        }
    }

    /// Processes an instrument close through the matching engine.
    fn process_instrument_close(&mut self, close: &InstrumentClose) {
        let instrument_id = close.instrument_id;

        // A delayed close belongs to an existing exposure lifecycle. Unlike an
        // instrument status update, it must not recreate execution state from
        // cache after rotation/unsubscribe; pending-settlement ownership stays
        // with the already-initialized matching engine.
        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_instrument_close(*close);
            self.sync_expired_cleanup(instrument_id);
        } else {
            log::warn!(
                "Ignoring instrument close for {instrument_id}: no existing matching engine",
            );
        }
    }

    fn is_expired_now(&self, instrument_id: InstrumentId) -> bool {
        let Some(engine) = self.matching_engines.get(&instrument_id) else {
            return false;
        };

        let now_ns = self.clock.borrow().timestamp_ns();
        engine
            .instrument
            .expiration_ns()
            .is_some_and(|ns| now_ns >= ns)
    }

    fn has_open_orders(&self, instrument_id: InstrumentId) -> bool {
        self.cache.borrow().has_orders_open(
            Some(&self.config.venue),
            Some(&instrument_id),
            None,
            None,
            None,
        )
    }

    fn sync_expired_cleanup(&mut self, instrument_id: InstrumentId) {
        if !self.is_expired_now(instrument_id) {
            return;
        }

        let has_open_positions = self.cache.borrow().has_positions_open(
            Some(&self.config.venue),
            Some(&instrument_id),
            None,
            None,
            None,
        );

        if has_open_positions {
            return;
        }

        self.matching_engines.remove(&instrument_id);
        self.cache
            .borrow_mut()
            .purge_instrument_skip_order_guard(instrument_id);
    }

    fn sync_expired_cleanup_many(&mut self, instrument_ids: &[InstrumentId]) {
        for &instrument_id in instrument_ids {
            self.sync_expired_cleanup(instrument_id);
        }
    }

    /// Retires matching engines whose instrument has expired with no open position or order.
    ///
    /// This is the periodic trigger for quote-only instruments that create a matching engine from
    /// market data but never reach an `InstrumentClose`, expired-order, or `PositionClosed` event.
    /// It performs no settlement: `sync_expired_cleanup` retains any expired engine that still has
    /// an open position.
    ///
    /// Instruments with open orders are retained too. The event-driven callers of
    /// `sync_expired_cleanup` each terminalize order state through the matching engine first, which
    /// is what `Cache::purge_instrument_skip_order_guard` requires of its callers; this sweep has
    /// no such event, so purging here would orphan a resting order behind a removed engine.
    fn sweep_expired_engines(&mut self) {
        let expired_ids: Vec<InstrumentId> = self
            .matching_engines
            .keys()
            .copied()
            .filter(|instrument_id| {
                self.is_expired_now(*instrument_id) && !self.has_open_orders(*instrument_id)
            })
            .collect();

        self.sync_expired_cleanup_many(&expired_ids);
    }

    /// Routes a deferred [`TradingCommand`] to its venue-side apply helper.
    fn apply_trading_command(&mut self, cmd: &TradingCommand) -> anyhow::Result<()> {
        // Only a deferred command can overtake the submit that would have created the engine, so
        // build it here and let the venue raise the rejection.
        if matches!(
            cmd,
            TradingCommand::ModifyOrder(_)
                | TradingCommand::ModifyOrders(_)
                | TradingCommand::CancelOrder(_)
                | TradingCommand::CancelOrders(_)
        ) && self.ensure_engine_for(cmd.instrument_id()).is_none()
        {
            self.reject_command(cmd, "No matching engine for instrument");
            return Ok(());
        }

        match cmd {
            TradingCommand::SubmitOrder(cmd) => self.apply_submit_order(cmd)?,
            TradingCommand::SubmitOrderList(cmd) => self.apply_submit_order_list(cmd),
            TradingCommand::ModifyOrder(cmd) => self.apply_modify_order(cmd),
            TradingCommand::ModifyOrders(cmd) => self.apply_batch_modify_orders(cmd),
            TradingCommand::CancelOrder(cmd) => self.apply_cancel_order(cmd),
            TradingCommand::CancelOrders(cmd) => self.apply_batch_cancel_orders(cmd),
            TradingCommand::CancelAllOrders(cmd) => self.apply_cancel_all_orders(cmd),
            TradingCommand::QueryOrder(_) | TradingCommand::QueryAccount(_) => {}
        }
        Ok(())
    }

    /// Dispatches the rejection that terminalizes a command which never reached the venue.
    fn reject_command(&self, command: &TradingCommand, reason: &str) {
        self.reject_command_deduped(command, reason, &mut AHashSet::new());
    }

    /// Dispatches the rejection for `command`, skipping any order that has already received a
    /// modify or cancel rejection recorded in `pending_rejected`.
    fn reject_command_deduped(
        &self,
        command: &TradingCommand,
        reason: &str,
        pending_rejected: &mut AHashSet<ClientOrderId>,
    ) {
        let ts_now = self.clock.borrow().timestamp_ns();
        let account_id = self.account_id;
        let reason = Ustr::from(reason);

        let reject_submit = |trader_id, strategy_id, instrument_id, client_order_id| {
            self.dispatch_order_event(OrderEventAny::Rejected(OrderRejected::new(
                trader_id,
                strategy_id,
                instrument_id,
                client_order_id,
                account_id,
                reason,
                UUID4::new(),
                ts_now,
                ts_now,
                false,
                false,
            )));
        };

        match command {
            TradingCommand::SubmitOrder(cmd) => reject_submit(
                cmd.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
            ),
            TradingCommand::SubmitOrderList(cmd) => {
                // A leg already closed when `submit_order_list` first ran never received an
                // `OrderSubmitted` (`apply_submit_order_list` skips it the same way), so only the
                // legs still in flight have a status the FSM will accept a rejection from.
                let cache = self.cache.borrow();
                let in_flight_ids: Vec<ClientOrderId> = cmd
                    .order_list
                    .client_order_ids
                    .iter()
                    .copied()
                    .filter(|id| cache.order(id).is_some_and(|order| !order.is_closed()))
                    .collect();
                drop(cache);

                for client_order_id in in_flight_ids {
                    reject_submit(
                        cmd.trader_id,
                        cmd.strategy_id,
                        cmd.instrument_id,
                        client_order_id,
                    );
                }
            }
            TradingCommand::ModifyOrder(cmd) => {
                if pending_rejected.insert(cmd.client_order_id) {
                    self.reject_modify(cmd, reason, ts_now);
                }
            }
            TradingCommand::ModifyOrders(cmd) => {
                for modify in &cmd.modifies {
                    if pending_rejected.insert(modify.client_order_id) {
                        self.reject_modify(modify, reason, ts_now);
                    }
                }
            }
            TradingCommand::CancelOrder(cmd) => {
                if pending_rejected.insert(cmd.client_order_id) {
                    self.reject_cancel(
                        cmd.trader_id,
                        cmd.strategy_id,
                        cmd.instrument_id,
                        cmd.client_order_id,
                        cmd.venue_order_id,
                        reason,
                        ts_now,
                    );
                }
            }
            TradingCommand::CancelOrders(cmd) => {
                for cancel in &cmd.cancels {
                    if pending_rejected.insert(cancel.client_order_id) {
                        self.reject_cancel(
                            cancel.trader_id,
                            cancel.strategy_id,
                            cancel.instrument_id,
                            cancel.client_order_id,
                            cancel.venue_order_id,
                            reason,
                            ts_now,
                        );
                    }
                }
            }
            // `CancelAllOrders` names no orders, and unlike `cancel_order` / `cancel_orders` the
            // strategy marks none `PENDING_CANCEL` before sending it, so there is no pending state
            // to release and the FSM would refuse the rejection.
            TradingCommand::CancelAllOrders(_) => {}
            TradingCommand::QueryOrder(_) | TradingCommand::QueryAccount(_) => {}
        }
    }

    fn reject_modify(&self, cmd: &ModifyOrder, reason: Ustr, ts_now: UnixNanos) {
        self.dispatch_order_event(OrderEventAny::ModifyRejected(OrderModifyRejected::new(
            cmd.trader_id,
            cmd.strategy_id,
            cmd.instrument_id,
            cmd.client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            cmd.venue_order_id,
            Some(self.account_id),
        )));
    }

    #[expect(clippy::too_many_arguments, reason = "mirrors the event's own fields")]
    fn reject_cancel(
        &self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Option<VenueOrderId>,
        reason: Ustr,
        ts_now: UnixNanos,
    ) {
        self.dispatch_order_event(OrderEventAny::CancelRejected(OrderCancelRejected::new(
            trader_id,
            strategy_id,
            instrument_id,
            client_order_id,
            reason,
            UUID4::new(),
            ts_now,
            ts_now,
            false,
            venue_order_id,
            Some(self.account_id),
        )));
    }

    /// Builds the order-event handler shared by this client and every matching engine it owns.
    fn build_event_handler(router: Rc<EventRouter>) -> Rc<dyn Fn(OrderEventAny)> {
        Rc::new(move |event: OrderEventAny| router.dispatch(event))
    }

    fn dispatch_order_event(&self, event: OrderEventAny) {
        (self.event_handler)(event);
    }

    /// Creates the matching engine from the cached instrument when it does not exist yet, so it can
    /// answer for an order it has never seen.
    fn ensure_engine_for(
        &mut self,
        instrument_id: InstrumentId,
    ) -> Option<&mut OrderMatchingEngine> {
        if !self.matching_engines.contains_key(&instrument_id) {
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();
            let Some(instrument) = instrument else {
                log::warn!(
                    "Cannot process command for {instrument_id}: instrument missing from cache",
                );
                return None;
            };
            self.ensure_matching_engine(&instrument);
        }

        self.matching_engines.get_mut(&instrument_id)
    }

    /// Applies a submit-order command to the matching engine (venue-side).
    fn apply_submit_order(&mut self, cmd: &SubmitOrder) -> anyhow::Result<()> {
        let mut order = self.cache.borrow().try_order_owned(&cmd.client_order_id)?;

        let instrument_id = order.instrument_id();
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        self.ensure_matching_engine(&instrument);

        // Update matching engine with latest market data from cache
        let cache = self.cache.borrow();

        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            if let Some(quote) = cache.quote(&instrument_id)
                && check_quote_or_drop("cached quote tick", quote, &instrument)
            {
                engine.process_quote_tick(quote);
            }

            if self.config.trade_execution
                && let Some(trade) = cache.trade(&instrument_id)
                && check_trade_or_drop("cached trade tick", trade, &instrument)
            {
                engine.process_trade_tick(trade);
            }
        }
        drop(cache);

        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_order(&mut order, self.account_id);
            self.sync_expired_cleanup(instrument_id);
        }

        Ok(())
    }

    /// Applies a submit-order-list command to the matching engines (venue-side), less the per-order
    /// `OrderSubmitted` dispatch kept by the client handler.
    fn apply_submit_order_list(&mut self, cmd: &SubmitOrderList) {
        let orders: Vec<OrderAny> = self
            .cache
            .borrow()
            .orders_for_ids(&cmd.order_list.client_order_ids, cmd);

        let mut cleanup_instrument_ids = Vec::new();

        for order in &orders {
            if order.is_closed() {
                continue;
            }

            let instrument_id = order.instrument_id();
            if !cleanup_instrument_ids.contains(&instrument_id) {
                cleanup_instrument_ids.push(instrument_id);
            }
            let instrument = self.cache.borrow().instrument(&instrument_id).cloned();

            if let Some(instrument) = instrument {
                self.ensure_matching_engine(&instrument);

                // Update with latest market data
                let cache = self.cache.borrow();

                if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                    if let Some(quote) = cache.quote(&instrument_id)
                        && check_quote_or_drop("cached quote tick", quote, &instrument)
                    {
                        engine.process_quote_tick(quote);
                    }

                    if self.config.trade_execution
                        && let Some(trade) = cache.trade(&instrument_id)
                        && check_trade_or_drop("cached trade tick", trade, &instrument)
                    {
                        engine.process_trade_tick(trade);
                    }
                }
                drop(cache);

                if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
                    let mut order_clone = order.clone();
                    engine.process_order(&mut order_clone, self.account_id);
                }
            }
        }

        if !cleanup_instrument_ids.is_empty() {
            self.sync_expired_cleanup_many(&cleanup_instrument_ids);
        }
    }

    fn apply_modify_order(&mut self, cmd: &ModifyOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_modify(cmd, account_id);
        }
    }

    fn apply_batch_modify_orders(&mut self, cmd: &BatchModifyOrders) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_batch_modify(cmd, account_id);
        }
    }

    fn apply_cancel_order(&mut self, cmd: &CancelOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_cancel(cmd, account_id);
        }
    }

    /// Applies a cancel-all-orders command to the matching engine (venue-side).
    fn apply_cancel_all_orders(&mut self, cmd: &CancelAllOrders) {
        let instrument_id = cmd.instrument_id;
        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_cancel_all(cmd, self.account_id);
        } else {
            log::debug!("No open orders to cancel for {instrument_id}: no matching engine");
        }
    }

    fn apply_batch_cancel_orders(&mut self, cmd: &BatchCancelOrders) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_batch_cancel(cmd, account_id);
        }
    }

    /// Enqueues a trading command to be applied after its inbound latency elapses, as backtest
    /// `generate_inflight_command` does, but keyed off arrival rather than `command.ts_init()`.
    fn enqueue(&mut self, command: TradingCommand) {
        let leg_latency = self.command_leg_latency(&command);
        let due_ns = self.clock.borrow().timestamp_ns() + leg_latency;

        // Monotonic rather than the backtest's per-due-ts counter: a command enqueued from a
        // callback can land on a `due_ns` the pass already popped from, restarting that count.
        let seq = self.inbound_seq;
        self.inbound_seq += 1;

        self.inbound_queue.push(DelayedCommand {
            due_ns,
            seq,
            command,
        });

        // Not while a pass is draining: it re-peeks the heap and either applies this command or
        // arms for it on its exit path.
        if !order_event_is_dispatching() {
            self.arm_inbound_alert();
        }
    }

    /// Defers `command` by its inbound latency leg, or applies it inline when that leg is zero.
    fn defer_or_apply(&mut self, command: TradingCommand) {
        if !self.inbound_queue.is_empty()
            || order_event_is_dispatching()
            || self.command_leg_latency(&command) > UnixNanos::default()
        {
            self.enqueue(command);
            return;
        }

        if let Err(e) = self.apply_trading_command(&command) {
            log::error!("Error applying command: {e}");
            self.reject_command(&command, "Command could not be applied at the venue");
        }
    }

    /// Returns the inbound latency leg for `command`, or zero when no model is set (which callers
    /// never reach: they enqueue only when one is).
    fn command_leg_latency(&self, command: &TradingCommand) -> UnixNanos {
        let Some(latency_model) = self.config.latency_model.as_ref() else {
            return UnixNanos::default();
        };

        match command {
            TradingCommand::SubmitOrder(_) | TradingCommand::SubmitOrderList(_) => {
                latency_model.get_insert_latency()
            }
            TradingCommand::ModifyOrder(_) | TradingCommand::ModifyOrders(_) => {
                latency_model.get_update_latency()
            }
            TradingCommand::CancelOrder(_)
            | TradingCommand::CancelOrders(_)
            | TradingCommand::CancelAllOrders(_) => latency_model.get_delete_latency(),
            TradingCommand::QueryOrder(_) | TradingCommand::QueryAccount(_) => UnixNanos::default(),
        }
    }

    fn pop_due(&mut self, now_ns: UnixNanos) -> Option<DelayedCommand> {
        self.inbound_queue
            .peek()
            .is_some_and(|delayed| delayed.due_ns <= now_ns)
            .then(|| self.inbound_queue.pop().expect("peek returned Some"))
    }

    fn on_quote_tick(inner: &Rc<RefCell<Self>>, quote: &QuoteTick) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_quote_tick(quote);
    }

    fn on_trade_tick(inner: &Rc<RefCell<Self>>, trade: &TradeTick) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_trade_tick(trade);
    }

    fn on_bar(inner: &Rc<RefCell<Self>>, bar: &Bar) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_bar(bar);
    }

    fn on_order_book_deltas(inner: &Rc<RefCell<Self>>, deltas: &OrderBookDeltas) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_order_book_deltas(deltas);
    }

    fn on_instrument_status(inner: &Rc<RefCell<Self>>, status: &InstrumentStatus) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_instrument_status(status);
    }

    fn on_instrument_close(inner: &Rc<RefCell<Self>>, close: &InstrumentClose) {
        Self::drain_inbound(inner);
        inner.borrow_mut().process_instrument_close(close);
    }

    /// Applies every inbound command released by its latency, settling each one before the next.
    fn drain_inbound(inner: &Rc<RefCell<Self>>) {
        // No release while an order event is being dispatched, whoever started it: the engine holds
        // its own borrow across `process` and publishes from inside it, so a flush reached from
        // there would send back into that borrow and panic the runner.
        if order_event_is_dispatching() {
            log::debug!("Skipping sandbox inbound drain during order-event dispatch");
            return;
        }

        loop {
            let (router, events) = {
                // The alert fires on the runner task, where a nested msgbus dispatch may already
                // hold the borrow; the next data tick or alert retries the drain.
                let Ok(mut this) = inner.try_borrow_mut() else {
                    log::debug!("Skipping sandbox inbound drain due to active borrow");
                    return;
                };

                let now_ns = this.clock.borrow().timestamp_ns();
                let Some(delayed) = this.pop_due(now_ns) else {
                    this.arm_inbound_alert();
                    return;
                };

                let capture = EventCaptureGuard::install(&this.router);

                if let Err(e) = this.apply_trading_command(&delayed.command) {
                    log::error!("Error applying deferred command: {e}");
                    this.reject_command(
                        &delayed.command,
                        "Command could not be applied at the venue",
                    );
                }

                // Re-armed only on the exit path below, not per command
                (this.router.clone(), capture.take())
            };

            // Borrow released: dispatching can re-enter this client through a strategy callback
            router.flush(events);
        }
    }

    /// Discards every command still deferred by inbound latency, rejecting each one.
    fn discard_inbound_queue(&mut self) {
        if self.inbound_queue.is_empty() {
            return;
        }

        log::warn!(
            "Discarding {} command(s) still in flight at stop",
            self.inbound_queue.len(),
        );

        // Unwind last-issued-first by `seq`, so each rejection restores the state the command
        // before it established.
        let mut discarded = std::mem::take(&mut self.inbound_queue).into_vec();
        discarded.sort_unstable_by_key(|delayed| std::cmp::Reverse(delayed.seq));

        // Shared across the whole unwind: one order can have several pending commands in flight,
        // but only the first rejection it receives has a valid FSM transition.
        let mut pending_rejected = AHashSet::new();

        for delayed in discarded {
            self.reject_command_deduped(
                &delayed.command,
                "Client stopped before the command was sent",
                &mut pending_rejected,
            );
        }
    }

    /// Clears every command still deferred by inbound latency, without rejecting any of them:
    /// `reset` discards all client state, so there is nothing to release.
    fn clear_inbound_queue(&mut self) {
        self.inbound_queue.clear();
    }

    /// (Re)arms the `LiveClock` alert to fire at the earliest queued `due_ns`.
    fn arm_inbound_alert(&self) {
        let Some(earliest_due) = self.inbound_queue.peek().map(|delayed| delayed.due_ns) else {
            return;
        };

        let name = inbound_alert_name(self.client_id);
        let armed_ns = self.clock.borrow().next_time_ns(&name);

        match armed_ns {
            // Already armed no later than the new earliest due, so that alert still wakes the drain
            // - this is what stops a market-data drain re-setting the timer every tick.
            Some(armed_ns) if armed_ns <= earliest_due => return,
            // Cancelling first removes the entry `replace_existing_timer` would warn about; the
            // warning is on a shared path, so the intent has to be expressed at this call site.
            Some(_) => self.clock.borrow_mut().cancel_timer(&name),
            None => {}
        }

        let inner_weak = self.self_weak.clone();
        let alert_name = name.clone();

        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event: TimeEvent| {
            let Some(inner_rc) = inner_weak.upgrade() else {
                return;
            };

            // Retire the spent one-shot before anything can read it back as still armed.
            if let Ok(this) = inner_rc.try_borrow() {
                this.clock.borrow_mut().cancel_timer(&alert_name);
            }

            Self::drain_inbound(&inner_rc);

            // A drain that ran to its own exit path has already armed, and the guard makes this a
            // no-op; a drain skipped for re-entrancy or an active borrow never got there, and this
            // is what keeps its queue from sitting unwoken with no data flowing.
            if let Ok(this) = inner_rc.try_borrow() {
                this.arm_inbound_alert();
            }
        });

        if let Err(e) = self.clock.borrow_mut().set_time_alert_ns(
            &name,
            earliest_due,
            Some(TimeEventCallback::from(callback)),
            Some(true),
        ) {
            log::error!("Failed to arm sandbox inbound alert '{name}': {e}");
        }
    }
}

/// Registered message handlers for later deregistration.
struct RegisteredHandlers {
    deltas_pattern: MStr<Pattern>,
    deltas_handler: TypedHandler<OrderBookDeltas>,
    quote_pattern: MStr<Pattern>,
    quote_handler: TypedHandler<QuoteTick>,
    trade_pattern: MStr<Pattern>,
    trade_handler: TypedHandler<TradeTick>,
    bar_pattern: MStr<Pattern>,
    bar_handler: TypedHandler<Bar>,
    status_pattern: MStr<Pattern>,
    status_handler: ShareableMessageHandler,
    close_pattern: MStr<Pattern>,
    close_handler: ShareableMessageHandler,
    position_pattern: MStr<Pattern>,
    position_handler: TypedHandler<PositionEvent>,
}

/// A sandbox execution client for paper trading against live market data.
///
/// The `SandboxExecutionClient` simulates order execution using the `OrderMatchingEngine`
/// to match orders against market data. This enables strategy testing in real-time
/// without actual order execution on exchanges.
pub struct SandboxExecutionClient {
    /// The core execution client functionality.
    core: RefCell<ExecutionClientCore>,
    /// Factory for generating order events.
    factory: OrderEventFactory,
    /// The sandbox configuration.
    config: SandboxExecutionClientConfig,
    /// Inner state wrapped for handler access.
    inner: Rc<RefCell<SandboxInner>>,
    /// Registered message handlers for cleanup.
    handlers: RefCell<Option<RegisteredHandlers>>,
    /// Reference to the clock.
    clock: Rc<RefCell<dyn Clock>>,
    /// Reference to the cache.
    cache: Rc<RefCell<Cache>>,
}

impl Debug for SandboxExecutionClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(SandboxExecutionClient))
            .field("venue", &self.config.venue)
            .field("account_id", &self.core.borrow().account_id)
            .field("connected", &self.core.borrow().is_connected())
            .field(
                "matching_engines",
                &self.inner.borrow().matching_engines.len(),
            )
            .finish()
    }
}

impl SandboxExecutionClient {
    /// Creates a new [`SandboxExecutionClient`] instance.
    #[must_use]
    pub fn new(
        core: ExecutionClientCore,
        config: SandboxExecutionClientConfig,
        clock: Rc<RefCell<dyn Clock>>,
        cache: Rc<RefCell<Cache>>,
    ) -> Self {
        let mut balances = AHashMap::new();
        for money in &config.starting_balances {
            balances.insert(money.currency.code.to_string(), *money);
        }

        let fill_model = config
            .fill_model
            .clone()
            .map(FillModelHandle::from)
            .unwrap_or_default();
        let router = Rc::new(EventRouter::default());
        let event_handler = SandboxInner::build_event_handler(router.clone());
        let inner = Rc::new_cyclic(|weak: &std::rc::Weak<RefCell<SandboxInner>>| {
            RefCell::new(SandboxInner {
                clock: clock.clone(),
                cache: cache.clone(),
                config: config.clone(),
                fill_model,
                matching_engines: AHashMap::new(),
                next_engine_raw_id: 0,
                balances,
                event_handler,
                router,
                inbound_queue: BinaryHeap::new(),
                inbound_seq: 0,
                client_id: core.client_id,
                account_id: core.account_id,
                self_weak: WeakCell::from(weak.clone()),
            })
        });

        let factory = OrderEventFactory::new(
            core.trader_id,
            core.account_id,
            core.account_type,
            core.base_currency,
        );

        Self {
            core: RefCell::new(core),
            factory,
            config,
            inner,
            handlers: RefCell::new(None),
            clock,
            cache,
        }
    }

    /// Returns a reference to the configuration.
    #[must_use]
    pub const fn config(&self) -> &SandboxExecutionClientConfig {
        &self.config
    }

    /// Returns the number of active matching engines.
    #[must_use]
    pub fn matching_engine_count(&self) -> usize {
        self.inner.borrow().matching_engines.len()
    }

    fn dispatch_order_event(&self, event: OrderEventAny) {
        // Cloned out so the inner borrow is released before a handler that can re-enter runs
        let handler = self.inner.borrow().event_handler.clone();
        handler(event);
    }

    /// Runs `f` with the order events it emits captured, flushing them once the borrow is released.
    fn with_settlement_barrier<R>(&self, f: impl FnOnce() -> R) -> R {
        let (router, deferred) = {
            let inner = self.inner.borrow();
            (inner.router.clone(), inner.config.latency_model.is_some())
        };

        if !deferred || router.capture.borrow().is_some() {
            return f();
        }

        let guard = EventCaptureGuard::install(&router);
        let result = f();
        router.flush(guard.take());
        result
    }

    /// Registers message handlers for market data subscriptions.
    ///
    /// This subscribes to order book deltas, quotes, trades, and bars for the
    /// configured venue, routing all received data to the matching engines.
    fn register_message_handlers(&self) {
        if self.handlers.borrow().is_some() {
            log::warn!("Sandbox message handlers already registered");
            return;
        }

        let inner_weak = WeakCell::from(Rc::downgrade(&self.inner));
        let venue = self.config.venue;
        let account_id = self.core.borrow().account_id;

        // Order book deltas handler
        let deltas_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |deltas: &OrderBookDeltas| {
                if deltas.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_order_book_deltas(&inner_rc, deltas);
                }
            })
        };

        // Quote tick handler
        let quote_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |quote: &QuoteTick| {
                if quote.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_quote_tick(&inner_rc, quote);
                }
            })
        };

        // Trade tick handler
        let trade_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |trade: &TradeTick| {
                if trade.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_trade_tick(&inner_rc, trade);
                }
            })
        };

        // Bar handler (topic is data.bars.{bar_type}, filter by venue in handler)
        let bar_handler = {
            let inner = inner_weak.clone();
            TypedHandler::from(move |bar: &Bar| {
                if bar.bar_type.instrument_id().venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_bar(&inner_rc, bar);
                }
            })
        };

        let status_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |status: &InstrumentStatus| {
                if status.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_instrument_status(&inner_rc, status);
                }
            })
        };

        let close_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |close: &InstrumentClose| {
                if close.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    SandboxInner::on_instrument_close(&inner_rc, close);
                }
            })
        };

        let position_handler = {
            TypedHandler::from(move |event: &PositionEvent| {
                let PositionEvent::PositionClosed(position_closed) = event else {
                    return;
                };

                if position_closed.instrument_id.venue == venue
                    && position_closed.account_id == account_id
                    && let Some(inner_rc) = inner_weak.upgrade()
                {
                    // ExecutionEngine updates the cached position state before publishing
                    // PositionClosed, so this retry observes the post-settlement cache view.
                    if let Ok(mut inner) = inner_rc.try_borrow_mut() {
                        inner.sync_expired_cleanup(position_closed.instrument_id);
                    } else {
                        log::debug!(
                            "Skipping immediate expired cleanup retry for {} due to active sandbox borrow",
                            position_closed.instrument_id,
                        );
                    }
                }
            })
        };

        // Subscribe patterns
        let deltas_pattern: MStr<Pattern> = format!("data.book.deltas.{venue}.*").into();
        let quote_pattern: MStr<Pattern> = format!("data.quotes.{venue}.*").into();
        let trade_pattern: MStr<Pattern> = format!("data.trades.{venue}.*").into();
        let bar_pattern: MStr<Pattern> = "data.bars.*".into();
        let status_pattern: MStr<Pattern> = format!("data.status.{venue}.*").into();
        let close_pattern: MStr<Pattern> = format!("data.close.{venue}.*").into();
        let position_pattern: MStr<Pattern> = "events.position.*".into();

        msgbus::subscribe_book_deltas(deltas_pattern, deltas_handler.clone(), Some(10));
        msgbus::subscribe_quotes(quote_pattern, quote_handler.clone(), Some(10));
        msgbus::subscribe_trades(trade_pattern, trade_handler.clone(), Some(10));
        msgbus::subscribe_bars(bar_pattern, bar_handler.clone(), Some(10));
        msgbus::subscribe_any(status_pattern, status_handler.clone(), Some(10));
        msgbus::subscribe_instrument_close(close_pattern, close_handler.clone(), Some(10));
        msgbus::subscribe_position_events(position_pattern, position_handler.clone(), Some(10));

        // Store handlers for later deregistration
        *self.handlers.borrow_mut() = Some(RegisteredHandlers {
            deltas_pattern,
            deltas_handler,
            quote_pattern,
            quote_handler,
            trade_pattern,
            trade_handler,
            bar_pattern,
            bar_handler,
            status_pattern,
            status_handler,
            close_pattern,
            close_handler,
            position_pattern,
            position_handler,
        });

        log::debug!(
            "Sandbox registered message handlers for venue={}",
            self.config.venue
        );
    }

    /// Deregisters message handlers to stop receiving market data.
    fn deregister_message_handlers(&self) {
        if let Some(handlers) = self.handlers.borrow_mut().take() {
            msgbus::unsubscribe_book_deltas(handlers.deltas_pattern, &handlers.deltas_handler);
            msgbus::unsubscribe_quotes(handlers.quote_pattern, &handlers.quote_handler);
            msgbus::unsubscribe_trades(handlers.trade_pattern, &handlers.trade_handler);
            msgbus::unsubscribe_bars(handlers.bar_pattern, &handlers.bar_handler);
            msgbus::unsubscribe_any(handlers.status_pattern, &handlers.status_handler);
            msgbus::unsubscribe_instrument_close(handlers.close_pattern, &handlers.close_handler);
            msgbus::unsubscribe_position_events(
                handlers.position_pattern,
                &handlers.position_handler,
            );

            log::debug!(
                "Sandbox deregistered message handlers for venue={}",
                self.config.venue
            );
        }
    }

    fn expiry_sweep_timer_name(&self) -> String {
        format!("{}-sandbox-expiry-sweep", self.core.borrow().client_id)
    }

    /// Registers the periodic sweep that retires expired matching engines with no open position.
    fn register_expiry_sweep_timer(&self) {
        let inner_weak = WeakCell::from(Rc::downgrade(&self.inner));
        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |_event: TimeEvent| {
            let Some(inner_rc) = inner_weak.upgrade() else {
                return;
            };

            // The timer fires on the runner task, but a nested msgbus dispatch may already hold the
            // borrow; skipping is safe because the next interval retries.
            if let Ok(mut inner) = inner_rc.try_borrow_mut() {
                inner.sweep_expired_engines();
            } else {
                log::debug!("Skipping sandbox expiry sweep due to active borrow");
            }
        });

        let name = self.expiry_sweep_timer_name();

        if let Err(e) = self.clock.borrow_mut().set_timer_ns(
            &name,
            EXPIRED_ENGINE_SWEEP_INTERVAL_NS,
            None,
            None,
            Some(TimeEventCallback::from(callback)),
            None,
            None,
        ) {
            log::error!("Failed to register sandbox expiry sweep timer: {e}");
        }
    }

    /// Cancels the periodic expired-engine sweep timer.
    fn cancel_expiry_sweep_timer(&self) {
        self.clock
            .borrow_mut()
            .cancel_timer(&self.expiry_sweep_timer_name());
    }

    fn cancel_inbound_alert(&self) {
        let client_id = self.core.borrow().client_id;
        self.clock
            .borrow_mut()
            .cancel_timer(&inbound_alert_name(client_id));
    }

    /// Returns current account balances, preferring cache state over starting balances.
    fn get_current_account_balances(&self) -> Vec<AccountBalance> {
        let account_id = self.core.borrow().account_id;
        let cache = self.cache.borrow();

        // Use account from cache if available (updated by fill events)
        if let Some(account) = cache.account(&account_id) {
            return account.balances().into_values().collect();
        }

        // Fall back to starting balances
        self.get_account_balances()
    }

    fn sync_cached_account_config(&self) -> anyhow::Result<()> {
        let Some(mut account) = self.get_account() else {
            return Ok(());
        };

        account.set_calculate_account_state(!self.config.frozen_account);

        if let AccountAny::Margin(margin_account) = &mut account {
            margin_account.set_default_leverage(self.config.default_leverage);
            for (instrument_id, leverage) in &self.config.leverages {
                margin_account.set_leverage(*instrument_id, *leverage);
            }
        }

        self.cache.borrow_mut().update_account(&account)
    }

    /// Processes a quote tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_quote_tick(&self, quote: &QuoteTick) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        let instrument_id = quote.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_quote_or_drop("quote tick", quote, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_quote_tick(quote);
        }
        Ok(())
    }

    /// Processes a trade tick through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_trade_tick(&self, trade: &TradeTick) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        if !self.config.trade_execution {
            return Ok(());
        }

        let instrument_id = trade.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_trade_or_drop("trade tick", trade, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_trade_tick(trade);
        }
        Ok(())
    }

    /// Processes a bar through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_bar(&self, bar: &Bar) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        if !self.config.bar_execution {
            return Ok(());
        }

        let instrument_id = bar.bar_type.instrument_id();
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        if !check_bar_or_drop("bar", bar, &instrument) {
            return Ok(());
        }

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_bar(bar);
        }
        Ok(())
    }

    /// Processes order book deltas through the matching engine.
    ///
    /// # Errors
    ///
    /// Returns an error if the instrument is not found in the cache.
    pub fn process_order_book_deltas(&self, deltas: &OrderBookDeltas) -> anyhow::Result<()> {
        SandboxInner::drain_inbound(&self.inner);

        let instrument_id = deltas.instrument_id;
        let instrument = self.cache.borrow().try_instrument(&instrument_id)?.clone();

        let mut inner = self.inner.borrow_mut();
        inner.ensure_matching_engine(&instrument);
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id) {
            engine.process_order_book_deltas(deltas)?;
        }
        Ok(())
    }

    /// Resets the sandbox to its initial state.
    pub fn reset(&self) {
        let mut inner = self.inner.borrow_mut();
        for engine in inner.matching_engines.values_mut() {
            engine.reset();
        }

        inner.balances.clear();
        for money in &self.config.starting_balances {
            inner
                .balances
                .insert(money.currency.code.to_string(), *money);
        }

        inner.clear_inbound_queue();

        self.cancel_inbound_alert();

        log::info!(
            "Sandbox execution client reset: venue={}",
            self.config.venue
        );
    }

    /// Generates account balance entries from current balances.
    fn get_account_balances(&self) -> Vec<AccountBalance> {
        self.inner
            .borrow()
            .balances
            .values()
            .map(|money| AccountBalance::new(*money, Money::zero(money.currency), *money))
            .collect()
    }

    fn get_order(&self, client_order_id: &ClientOrderId) -> anyhow::Result<OrderAny> {
        Ok(self.cache.borrow().try_order_owned(client_order_id)?)
    }
}

#[async_trait(?Send)]
impl ExecutionClient for SandboxExecutionClient {
    fn is_connected(&self) -> bool {
        self.core.borrow().is_connected()
    }

    fn client_id(&self) -> ClientId {
        self.core.borrow().client_id
    }

    fn account_id(&self) -> AccountId {
        self.core.borrow().account_id
    }

    fn venue(&self) -> Venue {
        self.core.borrow().venue
    }

    fn oms_type(&self) -> OmsType {
        self.config.oms_type
    }

    fn on_instrument(&mut self, instrument: InstrumentAny) {
        let instrument_id = instrument.id();
        let mut inner = self.inner.borrow_mut();
        if let Some(engine) = inner.matching_engines.get_mut(&instrument_id)
            && let Err(e) = engine.update_instrument(instrument)
        {
            log::error!("Failed to update instrument {instrument_id} in sandbox engine: {e}");
        }
    }

    fn get_account(&self) -> Option<AccountAny> {
        let account_id = self.core.borrow().account_id;
        self.cache.borrow().account_owned(&account_id)
    }

    fn generate_account_state(
        &self,
        balances: Vec<AccountBalance>,
        margins: Vec<MarginBalance>,
        reported: bool,
        ts_event: UnixNanos,
        info: Option<Params>,
    ) -> anyhow::Result<()> {
        let ts_init = self.clock.borrow().timestamp_ns();
        let state = self
            .factory
            .generate_account_state(balances, margins, reported, ts_event, ts_init, info);
        let endpoint = MessagingSwitchboard::portfolio_update_account();
        msgbus::send_account_state(endpoint, &state);
        self.sync_cached_account_config()?;
        Ok(())
    }

    fn start(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_started() {
            return Ok(());
        }

        if let Some(sender) = try_get_exec_event_sender() {
            let forward: Rc<dyn Fn(OrderEventAny)> = Rc::new(move |event: OrderEventAny| {
                if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
                    log::warn!("Failed to send order event: {e}");
                }
            });
            *self.inner.borrow().router.sink.borrow_mut() = Some(forward);
        }

        // Register message handlers to receive market data
        self.register_message_handlers();
        self.register_expiry_sweep_timer();

        self.core.borrow().set_started();
        let core = self.core.borrow();
        log::info!(
            "Sandbox execution client started: venue={}, account_id={}, oms_type={:?}, account_type={:?}",
            self.config.venue,
            core.account_id,
            self.config.oms_type,
            self.config.account_type,
        );
        Ok(())
    }

    fn stop(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_stopped() {
            return Ok(());
        }

        // Deregister message handlers to stop receiving data
        self.deregister_message_handlers();
        self.cancel_expiry_sweep_timer();

        self.cancel_inbound_alert();
        self.inner.borrow_mut().discard_inbound_queue();

        self.core.borrow().set_stopped();
        self.core.borrow().set_disconnected();
        log::info!(
            "Sandbox execution client stopped: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn connect(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_connected() {
            return Ok(());
        }

        let balances = self.get_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event, None)?;

        self.core.borrow().set_connected();
        log::info!(
            "Sandbox execution client connected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    async fn disconnect(&mut self) -> anyhow::Result<()> {
        if self.core.borrow().is_disconnected() {
            return Ok(());
        }

        self.core.borrow().set_disconnected();
        log::info!(
            "Sandbox execution client disconnected: venue={}",
            self.config.venue
        );
        Ok(())
    }

    fn submit_order(&self, cmd: SubmitOrder) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let order = self.get_order(&cmd.client_order_id)?;

            if order.is_closed() {
                log::warn!("Cannot submit closed order {}", order.client_order_id());
                return Ok(());
            }

            let ts_init = self.clock.borrow().timestamp_ns();
            let event = self.factory.generate_order_submitted(&order, ts_init);
            self.dispatch_order_event(event);

            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_submit_order(&cmd)?;
            } else {
                inner.defer_or_apply(TradingCommand::SubmitOrder(cmd));
            }
            Ok(())
        })
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let ts_init = self.clock.borrow().timestamp_ns();

            let orders: Vec<OrderAny> = self
                .cache
                .borrow()
                .orders_for_ids(&cmd.order_list.client_order_ids, &cmd);

            for order in &orders {
                if order.is_closed() {
                    log::warn!("Cannot submit closed order {}", order.client_order_id());
                    continue;
                }

                let event = self.factory.generate_order_submitted(order, ts_init);
                self.dispatch_order_event(event);
            }

            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_submit_order_list(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::SubmitOrderList(cmd));
            }
            Ok(())
        })
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_modify_order(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::ModifyOrder(cmd));
            }
            Ok(())
        })
    }

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_batch_modify_orders(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::ModifyOrders(cmd));
            }
            Ok(())
        })
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_cancel_order(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::CancelOrder(cmd));
            }
            Ok(())
        })
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_cancel_all_orders(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::CancelAllOrders(cmd));
            }
            Ok(())
        })
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        self.with_settlement_barrier(|| {
            let mut inner = self.inner.borrow_mut();
            if inner.config.latency_model.is_none() {
                inner.apply_batch_cancel_orders(&cmd);
            } else {
                inner.defer_or_apply(TradingCommand::CancelOrders(cmd));
            }
            Ok(())
        })
    }

    fn query_account(&self, _cmd: QueryAccount) -> anyhow::Result<()> {
        let balances = self.get_current_account_balances();
        let ts_event = self.clock.borrow().timestamp_ns();
        self.generate_account_state(balances, vec![], false, ts_event, None)?;
        Ok(())
    }

    fn query_order(&self, _cmd: QueryOrder) -> anyhow::Result<()> {
        // Orders are tracked in the cache, no external query needed for sandbox
        Ok(())
    }

    async fn generate_order_status_report(
        &self,
        _cmd: &GenerateOrderStatusReport,
    ) -> anyhow::Result<Option<OrderStatusReport>> {
        // Sandbox orders are tracked internally
        Ok(None)
    }

    async fn generate_order_status_reports(
        &self,
        _cmd: &GenerateOrderStatusReports,
    ) -> anyhow::Result<Vec<OrderStatusReport>> {
        // Sandbox orders are tracked internally
        Ok(Vec::new())
    }

    async fn generate_fill_reports(
        &self,
        _cmd: GenerateFillReports,
    ) -> anyhow::Result<Vec<FillReport>> {
        // Sandbox fills are tracked internally
        Ok(Vec::new())
    }

    async fn generate_position_status_reports(
        &self,
        _cmd: &GeneratePositionStatusReports,
    ) -> anyhow::Result<Vec<PositionStatusReport>> {
        // Sandbox positions are tracked internally
        Ok(Vec::new())
    }

    async fn generate_mass_status(
        &self,
        _lookback_mins: Option<u64>,
    ) -> anyhow::Result<Option<ExecutionMassStatus>> {
        let core = self.core.borrow();
        let ts_init = self.clock.borrow().timestamp_ns();
        Ok(Some(ExecutionMassStatus::new(
            core.client_id,
            core.account_id,
            core.venue,
            ts_init,
            None,
        )))
    }
}
