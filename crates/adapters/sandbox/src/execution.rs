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
    timer::{TimeEvent, TimeEventCallback},
};
use nautilus_core::{
    Params, UUID4, UnixNanos, WeakCell,
    datetime::{NANOSECONDS_IN_MILLISECOND, NANOSECONDS_IN_SECOND},
};
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

/// Delay before retrying a deferred command held back because an earlier command for the same
/// order was already applied in the current drain pass.
///
/// Sandbox events reach the cache the matching engine reads only once the runner drains the
/// execution channel, so two commands for one order in a single pass would act on a stale view.
const COMMAND_SETTLE_RETRY_NS: u64 = NANOSECONDS_IN_MILLISECOND;

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
    event_handler: Option<Rc<dyn Fn(OrderEventAny)>>,
    /// Inbound commands deferred by latency, ordered as a min-heap by due time.
    inbound_queue: BinaryHeap<DelayedCommand>,
    /// Monotonic sequence providing FIFO tie-breaking for deferred commands sharing a due time,
    /// so no queued command can be overtaken by one enqueued after it.
    inbound_seq: u64,
    /// Earliest time the inbound drain alert may fire, set while a hold-back is outstanding.
    ///
    /// A held-back command's `due_ns` is already in the past, so without this floor a later
    /// `enqueue` would re-arm the alert to fire immediately, cancelling the back-off.
    inbound_retry_floor_ns: Option<UnixNanos>,
    /// The execution client identifier (alert name for the inbound drain).
    client_id: ClientId,
    /// The account identifier applied to deferred engine commands.
    account_id: AccountId,
    /// Weak self-reference, so the inbound-drain timer callback can reach this state. Populated at
    /// construction via `Rc::new_cyclic`, so it is always linked.
    self_weak: WeakCell<Self>,
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

/// Returns the `LiveClock` time-alert name for a sandbox client's inbound drain.
fn inbound_alert_name(client_id: ClientId) -> String {
    format!("SANDBOX-INBOUND-{client_id}")
}

/// Calls `f` with each client order ID a trading command targets, yielding nothing for the
/// instrument-wide commands.
fn for_each_order_id(command: &TradingCommand, mut f: impl FnMut(ClientOrderId)) {
    match command {
        TradingCommand::SubmitOrder(cmd) => f(cmd.client_order_id),
        TradingCommand::SubmitOrderList(cmd) => {
            cmd.order_list.client_order_ids.iter().copied().for_each(f);
        }
        TradingCommand::ModifyOrder(cmd) => f(cmd.client_order_id),
        TradingCommand::ModifyOrders(cmd) => {
            cmd.modifies
                .iter()
                .for_each(|modify| f(modify.client_order_id));
        }
        TradingCommand::CancelOrder(cmd) => f(cmd.client_order_id),
        TradingCommand::CancelOrders(cmd) => {
            cmd.cancels
                .iter()
                .for_each(|cancel| f(cancel.client_order_id));
        }
        TradingCommand::CancelAllOrders(_)
        | TradingCommand::QueryOrder(_)
        | TradingCommand::QueryAccount(_) => {}
    }
}

/// Tracks what a single inbound drain pass has already applied to the venue.
///
/// At most one command per order reaches the venue per pass, so the matching engine never reads a
/// view of the order that predates the command before it; see [`COMMAND_SETTLE_RETRY_NS`].
#[derive(Default)]
struct AppliedThisPass {
    orders: AHashSet<ClientOrderId>,
    instruments: AHashSet<InstrumentId>,
    cancel_all_instruments: AHashSet<InstrumentId>,
}

impl AppliedThisPass {
    /// Returns whether `command` must wait for an already-applied command to settle.
    fn conflicts_with(&self, command: &TradingCommand) -> bool {
        // A cancel-all reaches every open and in-flight order on its instrument, in both
        // directions: `order_side` narrows the venue's target set, but holding the wider set is
        // what keeps a command applied earlier in this pass from being read back stale
        if self
            .cancel_all_instruments
            .contains(&command.instrument_id())
        {
            return true;
        }

        if let TradingCommand::CancelAllOrders(cmd) = command {
            return self.instruments.contains(&cmd.instrument_id);
        }

        let mut conflicts = false;
        for_each_order_id(command, |id| conflicts |= self.orders.contains(&id));
        conflicts
    }

    /// Records `command` as applied in this pass.
    fn record(&mut self, command: &TradingCommand) {
        self.instruments.insert(command.instrument_id());
        if let TradingCommand::CancelAllOrders(cmd) = command {
            self.cancel_all_instruments.insert(cmd.instrument_id);
        }
        for_each_order_id(command, |id| {
            self.orders.insert(id);
        });
    }
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

            if let Some(handler) = &self.event_handler {
                engine.set_event_handler(handler.clone());
            }

            self.matching_engines.insert(instrument_id, engine);
        }
    }

    /// Processes a quote tick through the matching engine.
    fn process_quote_tick(&mut self, quote: &QuoteTick) {
        self.drain_due_now();

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
        self.drain_due_now();

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
        self.drain_due_now();

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

    fn process_order_book_deltas(&mut self, deltas: &OrderBookDeltas) {
        self.drain_due_now();

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

    fn process_instrument_status(&mut self, status: &InstrumentStatus) {
        self.drain_due_now();

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

    fn process_instrument_close(&mut self, close: &InstrumentClose) {
        self.drain_due_now();

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
    ///
    /// Mirrors the backtest `process_trading_command`, less its `process_modify_submitted_order`
    /// amendment: a modify overtaking its submit is left for the venue to reject, as a real venue
    /// would for an order it has not received. `QueryOrder` / `QueryAccount` never enter the queue.
    fn apply_trading_command(&mut self, cmd: &TradingCommand) -> anyhow::Result<()> {
        // Only a deferred command can overtake the submit that would have created the engine,
        // so build it here and let the venue raise the rejection. When the instrument itself is
        // absent there is no engine to raise it from, and a silent no-op would strand the order.
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
    ///
    /// By this point the order is already `SUBMITTED` or `PENDING_UPDATE` / `PENDING_CANCEL`, and
    /// the sandbox generates no order status reports to resolve it later.
    fn reject_command(&self, command: &TradingCommand, reason: &str) {
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
                // `OrderSubmitted` (`apply_submit_order_list` skips it the same way), so only
                // the legs still in flight have a status the FSM will accept a rejection from.
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
            TradingCommand::ModifyOrder(cmd) => self.reject_modify(cmd, reason, ts_now),
            TradingCommand::ModifyOrders(cmd) => {
                for modify in &cmd.modifies {
                    self.reject_modify(modify, reason, ts_now);
                }
            }
            TradingCommand::CancelOrder(cmd) => self.reject_cancel(
                cmd.trader_id,
                cmd.strategy_id,
                cmd.instrument_id,
                cmd.client_order_id,
                cmd.venue_order_id,
                reason,
                ts_now,
            ),
            TradingCommand::CancelOrders(cmd) => {
                for cancel in &cmd.cancels {
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
            // `CancelAllOrders` names no orders, and unlike `cancel_order` / `cancel_orders`
            // the strategy marks none `PENDING_CANCEL` before sending it, so there is no
            // pending state to release and the FSM would refuse the rejection.
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

    /// Dispatches an order event through the registered handler, falling back to the msgbus, as
    /// `SandboxExecutionClient::dispatch_order_event` does for matching-engine events.
    fn dispatch_order_event(&self, event: OrderEventAny) {
        if let Some(handler) = &self.event_handler {
            handler(event);
        } else {
            msgbus::send_order_event(MessagingSwitchboard::exec_engine_process(), event);
        }
    }

    /// Creates the matching engine from the cached instrument when it does not exist yet, so it
    /// can answer for an order it has never seen.
    ///
    /// Only reached from the deferred path; the immediate path keeps its plain lookup.
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
    ///
    /// Excludes the `OrderSubmitted` dispatch the client handler keeps as its "sent to venue"
    /// record, and re-reads the order so both call sites share this path.
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

    /// Applies a submit-order-list command to the matching engines (venue-side), less the
    /// per-order `OrderSubmitted` dispatch kept by the client handler.
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

    /// Applies a modify-order command to the matching engine (venue-side).
    fn apply_modify_order(&mut self, cmd: &ModifyOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_modify(cmd, account_id);
        }
    }

    /// Applies a batch-modify command to the matching engine (venue-side).
    fn apply_batch_modify_orders(&mut self, cmd: &BatchModifyOrders) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_batch_modify(cmd, account_id);
        }
    }

    /// Applies a cancel-order command to the matching engine (venue-side).
    fn apply_cancel_order(&mut self, cmd: &CancelOrder) {
        let account_id = self.account_id;
        if let Some(engine) = self.matching_engines.get_mut(&cmd.instrument_id) {
            engine.process_cancel(cmd, account_id);
        }
    }

    /// Applies a cancel-all-orders command to the matching engine (venue-side).
    ///
    /// Unlike the order-targeting commands this does not create the engine when missing:
    /// `process_cancel_all` takes its targets from the cache's *open* and *in-flight* orders.
    /// Reaching either state requires an engine, except for a `SUBMITTED` order whose submit is
    /// still deferred here: that order has not reached the venue yet, so a cancel-all overtaking
    /// it correctly cancels nothing.
    fn apply_cancel_all_orders(&mut self, cmd: &CancelAllOrders) {
        let instrument_id = cmd.instrument_id;
        if let Some(engine) = self.matching_engines.get_mut(&instrument_id) {
            engine.process_cancel_all(cmd, self.account_id);
        } else {
            log::debug!("No open orders to cancel for {instrument_id}: no matching engine");
        }
    }

    /// Applies a batch-cancel command to the matching engine (venue-side).
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

        // A monotonic sequence rather than the backtest's per-due-ts counter map: hold-back can
        // leave a sibling queued at a `due_ns` already popped from, which would restart that
        // key's count and let a later command overtake it.
        let seq = self.inbound_seq;
        self.inbound_seq += 1;

        self.inbound_queue.push(DelayedCommand {
            due_ns,
            seq,
            command,
        });

        // Arm the LiveClock alert so the queue drains even with no subsequent data
        self.arm_inbound_alert();
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

    /// Drains and applies every inbound command whose latency has elapsed by `now_ns`.
    ///
    /// Pops all entries with `due_ns <= now_ns` in FIFO order (min-heap by due time, then enqueue
    /// sequence), routing each through `apply_trading_command`, as backtest `exchange.rs::process`
    /// does for its inflight queue.
    ///
    /// A hold-back settles only because the runner drains the execution channel between passes,
    /// which this cannot enforce: it relies on the alert not being ready again until
    /// [`COMMAND_SETTLE_RETRY_NS`] has elapsed, a window `inbound_retry_floor_ns` protects.
    fn drain_inbound_due(&mut self, now_ns: UnixNanos) {
        let mut applied = AppliedThisPass::default();
        let mut held_back = false;

        while let Some(delayed) = self.inbound_queue.peek() {
            if delayed.due_ns > now_ns {
                break;
            }

            if applied.conflicts_with(&delayed.command) {
                // Stop rather than skip: the venue applies a client's commands in order
                held_back = true;
                break;
            }

            let delayed = self.inbound_queue.pop().expect("peek returned Some");
            applied.record(&delayed.command);
            if let Err(e) = self.apply_trading_command(&delayed.command) {
                log::error!("Error applying deferred command: {e}");
                self.reject_command(
                    &delayed.command,
                    "Command could not be applied at the venue",
                );
            }
        }

        // Re-arm to the new earliest due time, no earlier than the retry delay when a command was
        // held back. The floor is retained rather than applied once, so an `enqueue` landing
        // inside the retry window cannot re-arm onto the held-back command's elapsed due time.
        self.inbound_retry_floor_ns =
            held_back.then(|| UnixNanos::from(*now_ns + COMMAND_SETTLE_RETRY_NS));
        self.arm_inbound_alert_at(self.inbound_retry_floor_ns);
    }

    /// Discards every command still deferred by inbound latency, rejecting each one.
    ///
    /// Called on `stop()`: an in-flight command never arrived, and rejecting it keeps its order
    /// from staying pending forever.
    fn discard_inbound_queue(&mut self) {
        self.inbound_retry_floor_ns = None;

        if self.inbound_queue.is_empty() {
            return;
        }

        log::warn!(
            "Discarding {} command(s) still in flight at stop",
            self.inbound_queue.len(),
        );

        let discarded = std::mem::take(&mut self.inbound_queue);

        for delayed in discarded {
            self.reject_command(
                &delayed.command,
                "Client stopped before the command was sent",
            );
        }
    }

    /// Clears every command still deferred by inbound latency, without rejecting any of them:
    /// `reset` discards all client state, so there is nothing to release.
    fn clear_inbound_queue(&mut self) {
        self.inbound_queue.clear();
        self.inbound_retry_floor_ns = None;
    }

    /// (Re)arms the `LiveClock` time alert to fire at the earliest queued `due_ns`.
    ///
    /// Uses the same `RustLocal` callback shape as the expiry sweep, so the closure stays on its
    /// owner thread. No-op when the queue is empty or the alert is already armed for that time.
    fn arm_inbound_alert(&self) {
        self.arm_inbound_alert_at(self.inbound_retry_floor_ns);
    }

    /// (Re)arms the inbound drain alert, no earlier than `not_before` when given.
    fn arm_inbound_alert_at(&self, not_before: Option<UnixNanos>) {
        let Some(earliest_due) = self.inbound_queue.peek().map(|delayed| delayed.due_ns) else {
            return;
        };
        let earliest_due = not_before.map_or(earliest_due, |floor| earliest_due.max(floor));

        let name = inbound_alert_name(self.client_id);

        let already_armed = self.clock.borrow().next_time_ns(&name) == Some(earliest_due);
        if already_armed {
            return;
        }

        let inner_weak = self.self_weak.clone();

        let callback: Rc<dyn Fn(TimeEvent)> = Rc::new(move |event: TimeEvent| {
            let Some(inner_rc) = inner_weak.upgrade() else {
                return;
            };

            // The timer fires on the runner task, but a nested msgbus dispatch may already hold
            // the borrow; the next enqueue or data tick retries the drain.
            if let Ok(mut inner) = inner_rc.try_borrow_mut() {
                inner.drain_inbound_due(event.ts_event);
            } else {
                log::debug!("Skipping sandbox inbound drain due to active borrow");
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

    /// Opportunistically drains inbound commands already due, tightening ordering against
    /// incoming market data under timer jitter. The `LiveClock` alert remains the backbone.
    fn drain_due_now(&mut self) {
        if self.inbound_queue.is_empty() {
            return;
        }
        let now = self.clock.borrow().timestamp_ns();
        self.drain_inbound_due(now);
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
        let inner = Rc::new_cyclic(|weak: &std::rc::Weak<RefCell<SandboxInner>>| {
            RefCell::new(SandboxInner {
                clock: clock.clone(),
                cache: cache.clone(),
                config: config.clone(),
                fill_model,
                matching_engines: AHashMap::new(),
                next_engine_raw_id: 0,
                balances,
                event_handler: None,
                inbound_queue: BinaryHeap::new(),
                inbound_seq: 0,
                inbound_retry_floor_ns: None,
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
        if let Some(handler) = &self.inner.borrow().event_handler {
            handler(event);
        } else {
            let endpoint = MessagingSwitchboard::exec_engine_process();
            msgbus::send_order_event(endpoint, event);
        }
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
                    inner_rc.borrow_mut().process_order_book_deltas(deltas);
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
                    inner_rc.borrow_mut().process_quote_tick(quote);
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
                    inner_rc.borrow_mut().process_trade_tick(trade);
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
                    inner_rc.borrow_mut().process_bar(bar);
                }
            })
        };

        let status_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |status: &InstrumentStatus| {
                if status.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    inner_rc.borrow_mut().process_instrument_status(status);
                }
            })
        };

        let close_handler = {
            let inner = inner_weak.clone();
            ShareableMessageHandler::from_typed(move |close: &InstrumentClose| {
                if close.instrument_id.venue == venue
                    && let Some(inner_rc) = inner.upgrade()
                {
                    inner_rc.borrow_mut().process_instrument_close(close);
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

    /// Cancels the armed inbound-latency drain alert, if any.
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
        self.inner.borrow_mut().drain_due_now();

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
        self.inner.borrow_mut().drain_due_now();

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
        self.inner.borrow_mut().drain_due_now();

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
        self.inner.borrow_mut().drain_due_now();

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
            let handler: Rc<dyn Fn(OrderEventAny)> = Rc::new(move |event: OrderEventAny| {
                if let Err(e) = sender.send(ExecutionEvent::Order(event)) {
                    log::warn!("Failed to send order event: {e}");
                }
            });
            let mut inner = self.inner.borrow_mut();
            inner.event_handler = Some(handler.clone());
            for engine in inner.matching_engines.values_mut() {
                engine.set_event_handler(handler.clone());
            }
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
            inner.enqueue(TradingCommand::SubmitOrder(cmd));
        }
        Ok(())
    }

    fn submit_order_list(&self, cmd: SubmitOrderList) -> anyhow::Result<()> {
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
            inner.enqueue(TradingCommand::SubmitOrderList(cmd));
        }
        Ok(())
    }

    fn modify_order(&self, cmd: ModifyOrder) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_modify_order(&cmd);
        } else {
            inner.enqueue(TradingCommand::ModifyOrder(cmd));
        }
        Ok(())
    }

    fn batch_modify_orders(&self, cmd: BatchModifyOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_batch_modify_orders(&cmd);
        } else {
            inner.enqueue(TradingCommand::ModifyOrders(cmd));
        }
        Ok(())
    }

    fn cancel_order(&self, cmd: CancelOrder) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_cancel_order(&cmd);
        } else {
            inner.enqueue(TradingCommand::CancelOrder(cmd));
        }
        Ok(())
    }

    fn cancel_all_orders(&self, cmd: CancelAllOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_cancel_all_orders(&cmd);
        } else {
            inner.enqueue(TradingCommand::CancelAllOrders(cmd));
        }
        Ok(())
    }

    fn batch_cancel_orders(&self, cmd: BatchCancelOrders) -> anyhow::Result<()> {
        let mut inner = self.inner.borrow_mut();
        if inner.config.latency_model.is_none() {
            inner.apply_batch_cancel_orders(&cmd);
        } else {
            inner.enqueue(TradingCommand::CancelOrders(cmd));
        }
        Ok(())
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
