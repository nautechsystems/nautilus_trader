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

use std::{
    collections::{HashMap, HashSet},
    sync::{Arc, Mutex},
};

use anyhow::Context;
use nautilus_common::{
    cache::{Cache, fifo::FifoCache},
    messages::ExecutionReport,
};
use nautilus_core::{Params, UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_live::{ExecutionEventEmitter, execution::context::OrderContext};
use nautilus_model::{
    enums::{AccountType, OrderSide, OrderStatus, OrderType, PositionSide, TimeInForce},
    events::{
        AccountState, OrderAccepted, OrderCanceled, OrderDenied, OrderEventAny, OrderFilled,
        OrderRejected, OrderSubmitted, OrderUpdated,
    },
    identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId, VenueOrderId},
    instruments::BinaryOption,
    orders::{LimitOrder, Order, OrderAny},
    reports::{FillReport, OrderStatusReport, PositionStatusReport},
    types::{AccountBalance, Currency, Price, Quantity},
};
use rust_decimal::Decimal;
use serde::Deserialize;

use super::{
    account::{AccountLedger, ReservationId},
    mutation::{Command, PendingMutation},
    reports,
    settlement::SettlementBarrier,
    submit, timestamp,
};
use crate::{
    common::enums::OrderType as VenueOrderType,
    http::models::{
        Field, FixedPoint, MarketPosition, Order as VenueOrder, OrderStatus as VenueStatus,
    },
    websocket::messages::KalshiWsMessage,
};

/// Execution state the private stream uses to turn venue evidence into order events.
#[derive(Debug)]
pub struct PrivateState {
    pub(super) ledger: AccountLedger,
    pub(super) barrier: Arc<Mutex<SettlementBarrier>>,
    pub(super) orders: HashMap<ClientOrderId, PrivateOrder>,
    pub(super) session: String,
    pub(super) epoch: Option<(u64, u64)>,
    pub(super) start: UnixNanos,
    pub(super) submissions: HashSet<ClientOrderId>,
    pub(super) batches: HashSet<UUID4>,
    pub(super) mutations: HashMap<ClientOrderId, PendingMutation>,
    pub(super) mutations_dispatched: HashSet<UUID4>,
    bindings: HashMap<VenueOrderId, ClientOrderId>,
    fills: FifoCache<TradeId, 100_000>,
    pub(super) positions: HashMap<InstrumentId, Decimal>,
    pub(super) external_leaves: HashMap<(InstrumentId, OrderSide), Decimal>,
    positions_ts: HashMap<InstrumentId, UnixNanos>,
    positions_initialized: bool,
    notices_fills: HashMap<TradeId, FillNotice>,
    covered_fills: HashMap<InstrumentId, UnixNanos>,
    notices_orders: HashMap<VenueOrderId, OrderNotice>,
    pub(super) notices_positions: HashMap<InstrumentId, PositionNotice>,
}

#[allow(
    clippy::missing_panics_doc,
    reason = "private state mutex poisoning is not expected"
)]
impl PrivateState {
    /// Creates empty private execution state.
    #[must_use]
    #[allow(
        clippy::new_without_default,
        reason = "construction records a session id and clock time, so Default would not be stable"
    )]
    pub fn new() -> Self {
        Self {
            ledger: AccountLedger::new(),
            barrier: Arc::new(Mutex::new(SettlementBarrier::default())),
            orders: HashMap::new(),
            session: UUID4::new().to_string(),
            epoch: None,
            start: get_atomic_clock_realtime().get_time_ns(),
            submissions: HashSet::new(),
            batches: HashSet::new(),
            mutations: HashMap::new(),
            mutations_dispatched: HashSet::new(),
            bindings: HashMap::new(),
            fills: FifoCache::new(),
            positions: HashMap::new(),
            external_leaves: HashMap::new(),
            positions_ts: HashMap::new(),
            positions_initialized: false,
            notices_fills: HashMap::new(),
            covered_fills: HashMap::new(),
            notices_orders: HashMap::new(),
            notices_positions: HashMap::new(),
        }
    }

    pub(super) fn requires_rebase(&self, order: &OrderAny) -> bool {
        !self.ledger.total_is_unknown()
            && !order.trade_ids().is_empty()
            && !self.orders.contains_key(&order.client_order_id())
    }

    pub(super) fn rebase(&mut self) {
        self.ledger.rebase();
        self.positions_initialized = false;
        self.invalidate();
    }

    /// Records opening position evidence once.
    ///
    /// Later calls keep the first seed until an accounting rebase. Each restored order receives a
    /// zero net when the seed omits its instrument.
    ///
    /// # Errors
    ///
    /// Returns an error if a supplied net overflows.
    pub fn seed_positions(
        &mut self,
        positions: impl IntoIterator<Item = (InstrumentId, Decimal, UnixNanos)>,
    ) -> anyhow::Result<()> {
        if self.positions_initialized {
            return Ok(());
        }

        let mut nets = HashMap::new();
        let mut timestamps = HashMap::new();

        for (id, qty, timestamp) in positions {
            let net: &mut Decimal = nets.entry(id).or_insert(Decimal::ZERO);
            *net = net
                .checked_add(qty)
                .context("cached account net overflow")?;
            timestamps
                .entry(id)
                .and_modify(|ts: &mut UnixNanos| *ts = (*ts).max(timestamp))
                .or_insert(timestamp);
        }

        for id in nets.keys() {
            self.barrier
                .lock()
                .expect("settlement barrier lock")
                .track(*id);
        }

        for context in self.orders.values() {
            nets.entry(context.order.instrument_id())
                .or_insert(Decimal::ZERO);
        }

        self.positions = nets;
        self.positions_ts = timestamps;
        self.positions_initialized = true;
        Ok(())
    }

    /// Inserts or replaces one cached order when the incoming record is not older.
    ///
    /// A new cached order that already has fills rebases private cash when the ledger has a known
    /// total.
    pub fn restore(&mut self, order: OrderAny) {
        if self.requires_rebase(&order) {
            self.rebase();
        }

        let id = order.client_order_id();
        let prior = self.orders.get(&id);
        if prior.is_some_and(|prior| {
            let cached_trades: HashSet<_> = order.trade_ids().into_iter().collect();
            prior.order.ts_last() > order.ts_last()
                || prior.order.venue_order_id().is_some() && order.venue_order_id().is_none()
                || prior.order.is_closed() && !order.is_closed()
                || prior
                    .order
                    .trade_ids()
                    .into_iter()
                    .any(|id| !cached_trades.contains(id))
        }) {
            return;
        }

        let source_ts = prior.map_or(order.ts_last(), |prior| {
            prior.source_ts.max(order.ts_last())
        });

        let mut accounted = prior.map_or_default(|prior| prior.accounted.clone());

        if self.ledger.total_is_unknown() {
            for trade_id in order.trade_ids() {
                self.fills.add(*trade_id);
                accounted.insert(*trade_id);
            }
        }

        self.barrier
            .lock()
            .expect("settlement barrier lock")
            .track(order.instrument_id());
        if prior.is_none() || !order.is_closed() {
            self.start = self.start.min(order.ts_init());
        }

        self.invalidate();
        self.orders.remove(&id);
        let venue_id = order.venue_order_id();

        let context = PrivateOrder {
            order,
            accounted,
            source_ts,
        };

        if let Some(venue_id) = venue_id {
            self.bindings.insert(venue_id, id);
        }

        self.orders.insert(id, context);
    }

    pub(super) fn submit(
        &mut self,
        order: &mut OrderAny,
        hold: Decimal,
        position_qty: Option<Decimal>,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = order.client_order_id();
        anyhow::ensure!(
            !self.orders.contains_key(&id),
            "Kalshi order already tracked"
        );
        self.check_reduction(order, position_qty)?;
        let now = get_atomic_clock_realtime().get_time_ns();
        let event = OrderEventAny::Submitted(OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            id,
            emitter.account_id(),
            UUID4::new(),
            now,
            now,
        ));
        let mut next = order.clone();
        next.apply(event.clone())?;
        self.ledger.reserve(id, hold)?;
        if let Err(e) = self
            .publish_balance(emitter, now)
            .and_then(|()| emitter.try_send_order_event(event))
        {
            self.ledger.release(id);

            if let Err(e) = self.publish_balance(emitter, now) {
                log::warn!("Kalshi admission balance rollback could not be routed: {e}");
            }

            return Err(e);
        }

        self.start = self.start.min(order.ts_init());
        self.barrier
            .lock()
            .expect("settlement barrier lock")
            .track(order.instrument_id());
        self.orders.insert(
            id,
            PrivateOrder {
                source_ts: next.ts_last(),
                order: next.clone(),
                accounted: HashSet::new(),
            },
        );

        *order = next;
        Ok(())
    }

    fn check_reduction(
        &self,
        order: &OrderAny,
        position_qty: Option<Decimal>,
    ) -> anyhow::Result<()> {
        if !order.is_reduce_only() {
            return self.check_position_flow(order);
        }

        let id = order.instrument_id();
        let side = order.order_side();
        let net = self.positions.get(&id).copied().unwrap_or(Decimal::ZERO);
        anyhow::ensure!(
            net > Decimal::ZERO && side == OrderSide::Sell
                || net < Decimal::ZERO && side == OrderSide::Buy,
            "Kalshi reduce-only side does not reduce the account net"
        );
        let mut available = net
            .abs()
            .checked_sub(
                self.external_leaves
                    .get(&(id, side))
                    .copied()
                    .unwrap_or(Decimal::ZERO),
            )
            .context("Kalshi reducible quantity overflow")?;

        for context in self.orders.values() {
            let pending = &context.order;
            if !pending.is_closed() && pending.instrument_id() == id && pending.order_side() == side
            {
                available = available
                    .checked_sub(pending.leaves_qty().as_decimal())
                    .context("Kalshi reducible quantity overflow")?;
            }
        }

        anyhow::ensure!(
            order.quantity().as_decimal() <= available,
            "Kalshi reduce-only quantity exceeds the unreserved account net"
        );
        self.check_position_flow(order)?;
        let position_qty = position_qty
            .context("Kalshi reduce-only requires a strategy-owned canonical position")?;
        anyhow::ensure!(
            position_qty > Decimal::ZERO && side == OrderSide::Sell
                || position_qty < Decimal::ZERO && side == OrderSide::Buy,
            "Kalshi reduce-only side does not reduce the canonical position"
        );
        anyhow::ensure!(
            order.quantity().as_decimal() <= position_qty.abs(),
            "Kalshi reduce-only quantity exceeds the canonical position"
        );
        Ok(())
    }

    fn check_position_flow(&self, order: &OrderAny) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.orders.values().any(|context| {
                let pending = &context.order;
                !pending.is_closed()
                    && pending.instrument_id() == order.instrument_id()
                    && pending.strategy_id() == order.strategy_id()
                    && (order.is_reduce_only() || pending.is_reduce_only())
            }),
            "Kalshi reduce-only requires exclusive strategy-instrument order flow"
        );

        if order.is_reduce_only() {
            let id = order.instrument_id();
            let side = order.order_side();
            anyhow::ensure!(
                self.external_leaves
                    .get(&(id, side))
                    .is_none_or(Decimal::is_zero)
                    && !self.orders.values().any(|context| {
                        let pending = &context.order;
                        !pending.is_closed()
                            && pending.instrument_id() == id
                            && pending.order_side() == side
                            && pending.strategy_id().is_external()
                    }),
                "Kalshi reduce-only requires resolved external orders on its reducing side"
            );
        }

        Ok(())
    }

    pub(super) fn deny_submission(
        &mut self,
        order: &mut OrderAny,
        reason: &str,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = order.client_order_id();
        if let Some(prior) = self.orders.get(&id)
            && prior.order.status() != OrderStatus::Initialized
        {
            *order = prior.order.clone();
            return Ok(());
        }

        let now = get_atomic_clock_realtime().get_time_ns();
        let event = OrderEventAny::Denied(OrderDenied::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            id,
            reason.into(),
            UUID4::new(),
            now,
            now,
        ));
        order.apply(event.clone())?;

        let prior = self.orders.insert(
            id,
            PrivateOrder {
                source_ts: order.ts_last(),
                order: order.clone(),
                accounted: HashSet::new(),
            },
        );

        // A restored initialized order can own a hold in the in-flight recovery
        if prior.is_some() {
            self.ledger.invalidate();
        }

        emitter.try_send_order_event(event)
    }

    pub(super) fn acknowledge(
        &mut self,
        id: ClientOrderId,
        venue_id: VenueOrderId,
        ts_event: UnixNanos,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.bindings
                .get(&venue_id)
                .is_none_or(|owner| *owner == id),
            "Kalshi acknowledgement venue identity mismatch"
        );
        let context = self
            .orders
            .get_mut(&id)
            .context("Kalshi submission context missing")?;
        anyhow::ensure!(
            context
                .order
                .venue_order_id()
                .is_none_or(|known| known == venue_id),
            "Kalshi acknowledgement order identity mismatch"
        );

        if context.order.venue_order_id().is_some() || context.order.is_closed() {
            return Ok(());
        }

        let order = &context.order;
        let now = get_atomic_clock_realtime().get_time_ns();
        let event = OrderEventAny::Accepted(OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            id,
            venue_id,
            emitter.account_id(),
            UUID4::new(),
            ts_event,
            now,
            false,
        ));
        let mut next = order.clone();
        next.apply(event.clone())?;
        emitter.try_send_order_event(event)?;
        context.order = next;
        self.bindings.insert(venue_id, id);
        self.ledger.invalidate();
        Ok(())
    }

    pub(super) fn resolve_mutation(
        &mut self,
        command: &Command,
        order: &OrderAny,
        reason: &str,
        denied: bool,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = order.client_order_id();

        if !self
            .mutations
            .get(&id)
            .is_some_and(|pending| pending.command_id == command.id())
        {
            return Ok(());
        }

        let Some(context) = self.orders.get_mut(&id) else {
            self.mutations.remove(&id);
            return Ok(());
        };

        if !context.order.is_closed() {
            let claimed = context.order.strategy_id().is_external()
                && context.order.strategy_id() != order.strategy_id();
            let event = if claimed {
                command.outcome_at(order, context.order.ts_last(), reason, denied, emitter)
            } else {
                command.outcome(&context.order, reason, denied, emitter)
            };

            if !claimed {
                let mut next = context.order.clone();
                next.apply(event.clone())?;
                context.order = next;
            }

            emitter.try_send_order_event(event)?;
        }

        self.mutations.remove(&id);
        self.ledger.invalidate();
        Ok(())
    }

    pub(super) fn reject_submission(
        &mut self,
        id: ClientOrderId,
        reason: &str,
        due_post_only: bool,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let context = self
            .orders
            .get_mut(&id)
            .context("Kalshi submission context missing")?;
        anyhow::ensure!(
            context.order.venue_order_id().is_none() && !context.order.is_closed(),
            "Kalshi submission already has venue evidence"
        );
        let order = &context.order;
        let now = get_atomic_clock_realtime().get_time_ns();
        let event = OrderEventAny::Rejected(OrderRejected::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            id,
            emitter.account_id(),
            reason.into(),
            UUID4::new(),
            now,
            now,
            false,
            due_post_only,
        ));
        let mut next = order.clone();
        next.apply(event.clone())?;
        emitter.try_send_order_event(event)?;
        context.order = next;
        self.ledger.release(id);
        self.publish_balance(emitter, now)
    }

    pub(super) fn publish_balance(
        &self,
        emitter: &ExecutionEventEmitter,
        ts_event: UnixNanos,
    ) -> anyhow::Result<()> {
        let balance = self.ledger.balance()?;

        let info = Params::new(
            [(
                "kalshi_session".into(),
                serde_json::Value::String(self.session.clone()),
            )]
            .into(),
        );

        let account = AccountState::new(
            emitter.account_id(),
            AccountType::Cash,
            vec![balance],
            Vec::new(),
            true,
            UUID4::new(),
            ts_event,
            get_atomic_clock_realtime().get_time_ns(),
            Some(Currency::USD()),
        )
        .with_info(Some(info));
        emitter.try_send_account_state(account)
    }

    pub(super) fn invalidate(&mut self) {
        self.session = UUID4::new().to_string();
        self.epoch = None;
        self.ledger.invalidate();
        self.barrier
            .lock()
            .expect("settlement barrier lock")
            .invalidate(&self.session);
    }

    pub(super) fn notice(&mut self, message: &KalshiWsMessage) -> anyhow::Result<()> {
        self.invalidate();

        match message {
            KalshiWsMessage::Fill(data) => self.notice_fill(&data.msg),
            KalshiWsMessage::UserOrder(data) => self.notice_order(&data.msg),
            KalshiWsMessage::MarketPosition(data) => self.notice_position(&data.msg),
            _ => Ok(()),
        }
    }

    fn notice_fill(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
        let raw = value
            .get("trade_id")
            .and_then(serde_json::Value::as_str)
            .context("private fill notification lacks stable identity")?;
        let id = TradeId::new_checked(raw)?;

        if let Some(alias) = value.get("fill_id").and_then(serde_json::Value::as_str) {
            anyhow::ensure!(
                alias == raw,
                "private fill notification identities disagree"
            );
        }

        let instrument_id = value
            .get("market_ticker")
            .and_then(serde_json::Value::as_str)
            .context("private fill notification lacks market identity")
            .and_then(crate::common::parse::parse_instrument_id);

        let start = value
            .get("ts_ms")
            .and_then(serde_json::Value::as_u64)
            .and_then(|time| time.checked_mul(1_000_000))
            .map(UnixNanos::from)
            .or_else(|| {
                value
                    .get("ts")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|time| time.checked_mul(1_000_000_000))
                    .map(UnixNanos::from)
            })
            .unwrap_or_default();

        let notice = FillNotice {
            instrument: instrument_id.as_ref().ok().copied(),
            ts_event: start,
        };

        self.notices_fills.insert(id, notice);
        self.start = self.start.min(start);
        instrument_id?;
        Ok(())
    }

    fn notice_order(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
        let id = VenueOrderId::new_checked(
            value
                .get("order_id")
                .and_then(serde_json::Value::as_str)
                .context("private order notification lacks identity")?,
        )?;

        let updated = value
            .get("last_updated_ts_ms")
            .and_then(serde_json::Value::as_u64)
            .map(|time| {
                time.checked_mul(1_000_000)
                    .context("private order notification timestamp overflow")
            })
            .transpose()?
            .map(UnixNanos::from);

        let notice = self.notices_orders.entry(id).or_insert(OrderNotice {
            filled: Decimal::ZERO,
            updated,
        });

        notice.updated = notice.updated.max(updated);
        let filled: FixedPoint = serde_json::from_value(
            value
                .get("fill_count_fp")
                .context("private order notification lacks filled count")?
                .clone(),
        )?;
        anyhow::ensure!(
            filled.0 >= Decimal::ZERO,
            "negative private order notification count"
        );
        notice.filled = notice.filled.max(filled.0);

        let has_new_fills = self
            .bindings
            .get(&id)
            .and_then(|id| self.orders.get(id))
            .is_some_and(|context| notice.filled > context.order.filled_qty().as_decimal());

        if self.has_external_order(id) && !has_new_fills {
            return Ok(());
        }

        let start = self
            .bindings
            .get(&id)
            .and_then(|id| self.orders.get(id))
            .map(|context| context.order.ts_init())
            .or_else(|| {
                value
                    .get("created_ts_ms")
                    .and_then(serde_json::Value::as_u64)
                    .and_then(|time| time.checked_mul(1_000_000))
                    .map(UnixNanos::from)
            })
            .unwrap_or_default();

        self.start = self.start.min(start);
        Ok(())
    }

    fn notice_position(&mut self, value: &serde_json::Value) -> anyhow::Result<()> {
        let ticker = value
            .get("market_ticker")
            .and_then(serde_json::Value::as_str)
            .context("private position notification lacks market identity")?;
        let id = crate::common::parse::parse_instrument_id(ticker)?;

        let notice = self.notices_positions.entry(id).or_insert(PositionNotice {
            volume: Decimal::ZERO,
            economics: None,
        });

        notice.economics = None;
        let volume: FixedPoint = serde_json::from_value(
            value
                .get("volume_fp")
                .context("private position notification lacks traded volume")?
                .clone(),
        )?;
        anyhow::ensure!(
            volume.0 >= Decimal::ZERO,
            "negative private position traded volume"
        );
        notice.volume = notice.volume.max(volume.0);
        notice.economics = Some(serde_json::from_value(value.clone())?);
        Ok(())
    }

    pub(super) fn recovery_targets(&self) -> Vec<InstrumentId> {
        self.positions
            .iter()
            .filter(|(_, net)| !net.is_zero())
            .map(|(id, _)| *id)
            .chain(self.notices_positions.keys().copied())
            .collect()
    }

    pub(super) fn external_order_notices(&self) -> Vec<VenueOrderId> {
        self.notices_orders
            .keys()
            .filter(|id| self.has_external_order(**id))
            .copied()
            .collect()
    }

    fn has_external_order(&self, id: VenueOrderId) -> bool {
        self.ledger.has_external(id)
            || self
                .bindings
                .get(&id)
                .and_then(|id| self.orders.get(id))
                .is_some_and(|context| context.order.strategy_id().is_external())
    }

    pub(super) fn check_notices(
        &mut self,
        snapshot: &super::KalshiExecutionSnapshot,
        plans: &[OrderPlan],
    ) -> anyhow::Result<()> {
        let fills: HashMap<_, _> = snapshot
            .fills
            .iter()
            .map(|fill| (fill.fill_id.as_str(), fill))
            .collect();

        for (id, notice) in &mut self.notices_fills {
            let fill = fills
                .get(id.as_str())
                .context("REST recovery does not yet cover received fill identities")?;

            if let Some(expected) = notice.instrument {
                let actual = crate::common::parse::parse_instrument_id(&fill.ticker)?;
                anyhow::ensure!(
                    actual == expected,
                    "REST recovery does not yet cover received fill identities"
                );
            }

            notice.ts_event = timestamp(&fill.created_time)?;
        }

        let orders: HashMap<_, _> = snapshot
            .orders
            .iter()
            .map(|order| (order.order_id.as_str(), order))
            .collect();

        for (id, notice) in &self.notices_orders {
            let order = orders
                .get(id.as_str())
                .context("REST recovery lacks a notified order")?;
            anyhow::ensure!(
                order.fill_count_fp.0 >= notice.filled,
                "REST order does not yet cover notified fills"
            );

            if let Some(updated) = notice.updated {
                anyhow::ensure!(
                    timestamp(&order.last_update_time)? >= updated,
                    "REST order does not yet cover notified update time"
                );
            }
        }

        if self.notices_positions.is_empty() {
            self.record_execution(plans);
            return Ok(());
        }

        let orders: HashMap<_, _> = self
            .orders
            .values()
            .map(|context| (context.order.client_order_id(), &context.order))
            .chain(
                plans
                    .iter()
                    .map(|plan| (plan.result.client_order_id(), &plan.result)),
            )
            .collect();

        for (id, notice) in &self.notices_positions {
            let economics = notice
                .economics
                .as_ref()
                .context("private position notification remains malformed")?;
            let position = snapshot
                .positions
                .iter()
                .chain(&snapshot.positions_settled)
                .find(|position| position.ticker == id.symbol.as_str())
                .context("REST recovery lacks a notified position")?;
            // WebSocket realized PnL includes fees; REST realized PnL excludes them
            let net_pnl = position
                .realized_pnl_dollars
                .0
                .checked_sub(position.fees_paid_dollars.0)
                .context("private position PnL overflow")?;
            anyhow::ensure!(
                position.position_fp == economics.position_fp
                    && net_pnl == economics.realized_pnl_dollars.0
                    && position.fees_paid_dollars == economics.fees_paid_dollars,
                "REST position does not yet cover notified economics"
            );

            // Only retained coverage is bounded by newer execution evidence
            if self.has_fill_notice(*id) {
                if self
                    .notices_fills
                    .values()
                    .any(|notice| notice.instrument == Some(*id))
                {
                    continue;
                }

                if !self.covered_fills.contains_key(id) {
                    continue;
                }

                let latest = self
                    .covered_fills
                    .get(id)
                    .copied()
                    .into_iter()
                    .chain(
                        plans
                            .iter()
                            .filter(|plan| plan.result.instrument_id() == *id)
                            .flat_map(|plan| &plan.fresh)
                            .map(|(_, fill)| fill.ts_event),
                    )
                    .chain(
                        plans
                            .iter()
                            .filter(|plan| plan.result.instrument_id() == *id)
                            .map(|plan| plan.source_ts),
                    )
                    .max();
                let updated = timestamp(&Field::Value(position.last_updated_ts))?;

                if latest.is_some_and(|latest| updated <= latest) {
                    continue;
                }

                self.start = self
                    .orders
                    .values()
                    .filter(|context| context.order.instrument_id() == *id)
                    .fold(self.start, |start, context| {
                        start.min(context.order.ts_init())
                    });
            }

            let volume = orders
                .values()
                .filter(|order| order.instrument_id() == *id)
                .try_fold(Decimal::ZERO, |total, order| {
                    total
                        .checked_add(order.filled_qty().as_decimal())
                        .context("private traded volume overflow")
                })?;

            anyhow::ensure!(
                volume >= notice.volume,
                "REST recovery does not yet cover notified traded volume"
            );
        }

        self.record_execution(plans);
        Ok(())
    }

    fn record_execution(&mut self, plans: &[OrderPlan]) {
        for plan in plans {
            let instrument = plan.result.instrument_id();
            for notice in self.notices_fills.values_mut() {
                if notice.instrument == Some(instrument) {
                    notice.ts_event = notice.ts_event.max(plan.source_ts);
                }
            }

            if let Some(timestamp) = self.covered_fills.get_mut(&instrument) {
                *timestamp = (*timestamp).max(plan.source_ts);
            }
        }
    }

    pub(super) fn has_fill_notice(&self, instrument_id: InstrumentId) -> bool {
        self.covered_fills.contains_key(&instrument_id)
            || self
                .notices_fills
                .values()
                .any(|notice| notice.instrument == Some(instrument_id))
    }

    pub(super) fn clear_notices(&mut self) {
        for notice in self.notices_fills.values() {
            if let Some(instrument) = notice.instrument
                && !self.notices_positions.contains_key(&instrument)
            {
                self.covered_fills
                    .entry(instrument)
                    .and_modify(|timestamp| *timestamp = (*timestamp).max(notice.ts_event))
                    .or_insert(notice.ts_event);
            }
        }

        for instrument in self.notices_positions.keys() {
            self.covered_fills.remove(instrument);
        }

        self.notices_fills.clear();
        self.notices_orders.clear();
        self.notices_positions.clear();
    }

    pub(super) fn advance_recovery(&mut self, start: UnixNanos) {
        let start = self
            .orders
            .values()
            .filter(|context| {
                !context.order.is_closed() && context.order.venue_order_id().is_none()
            })
            .fold(start, |start, context| start.min(context.order.ts_init()));

        self.start = self
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .recovery_start(start, self.start);
    }

    pub(super) fn source_order(&self, value: &VenueOrder) -> anyhow::Result<Option<OrderAny>> {
        let venue_id = VenueOrderId::new_checked(&value.order_id)?;
        let wire_id = (!value.client_order_id.is_empty())
            .then(|| ClientOrderId::new_checked(&value.client_order_id))
            .transpose()?;

        let by_venue = self
            .bindings
            .get(&venue_id)
            .and_then(|id| self.orders.get(id));

        let by_client = wire_id.and_then(|id| self.orders.get(&id));
        anyhow::ensure!(
            by_venue
                .zip(by_client)
                .is_none_or(|(a, b)| a.order.client_order_id() == b.order.client_order_id()),
            "private order identities disagree"
        );
        let order = by_client.or(by_venue);
        if order.is_none() && wire_id.is_none() {
            let id = ClientOrderId::new_checked(&value.order_id)?;
            anyhow::ensure!(
                !self.orders.contains_key(&id),
                "recovered order identity conflicts with an existing order"
            );
        }

        if let Some(order) = order {
            let rejected = order.order.status() == OrderStatus::Rejected;

            let unsent = order.order.status() == OrderStatus::Denied
                || rejected
                    && order.order.events().into_iter().any(|event| {
                        matches!(
                            event,
                            OrderEventAny::Rejected(rejected) if rejected.reason.as_str() == submit::NOT_SENT_REASON
                        )
                    });

            let recoverable_rejection =
                value.status == VenueStatus::Canceled && value.fill_count_fp.0.is_zero();

            anyhow::ensure!(
                !unsent && (!rejected || recoverable_rejection),
                "venue evidence conflicts with an unsent Kalshi order"
            );
            anyhow::ensure!(
                order.order.venue_order_id().is_none_or(|id| id == venue_id)
                    && wire_id.is_none_or(|id| id == order.order.client_order_id()),
                "private venue order binding mismatch"
            );
        }

        Ok(order.map(|context| context.order.clone()))
    }

    /// Plans the order and fill events one venue order requires.
    ///
    /// Returns `Ok(None)` when the venue record is already covered. The plan is not emitted until
    /// [`Self::apply_plans`].
    ///
    /// Owned limit orders retain independently valid fills with an unmapped type or status.
    /// These plans keep recovery unresolved. Fills that require unconfirmed quantity wait for
    /// authoritative order terms.
    ///
    /// # Errors
    ///
    /// Returns an error if identity, counts, prices, or timestamps are inconsistent, an external
    /// order has an unmapped classification, or fills require unconfirmed order quantity.
    pub fn plan_order(
        &self,
        source: &VenueOrder,
        fills: &[FillReport],
        instrument: &BinaryOption,
        emitter: &ExecutionEventEmitter,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<OrderPlan>> {
        let mut order = self
            .recovery_order(source, instrument, emitter, ts_init)?
            .context("external Kalshi order lacks recoverable quantity")?;
        let context = OrderContext::from(&order);
        let mapped =
            source.order_type == VenueOrderType::Limit && source.status != VenueStatus::Unknown;

        if !mapped {
            log::warn!(
                "Observed unmapped Kalshi order classification, order_id={}, ticker={}, type={:?}, status={:?}",
                source.order_id,
                source.ticker,
                source.order_type,
                source.status
            );
            anyhow::ensure!(
                !order.strategy_id().is_external(),
                "unmapped external Kalshi order classification"
            );
        }

        let id = context.identity.client_order_id;
        let venue_id = VenueOrderId::new_checked(&source.order_id)?;
        let ts_accepted = timestamp(&source.created_time)?;
        let ts_last = timestamp(&source.last_update_time)?;
        anyhow::ensure!(
            ts_last >= ts_accepted,
            "private order timestamps out of sequence"
        );
        anyhow::ensure!(
            context.identity.instrument_id == instrument.id
                && order
                    .account_id()
                    .is_none_or(|id| id == emitter.account_id())
                && (source.order_type == VenueOrderType::Limit
                    || matches!(source.order_type, VenueOrderType::Other(_)))
                && context.identity.order_type == OrderType::Limit
                && reports::direction(source.book_side, source.outcome_side)?
                    == context.identity.order_side,
            "private order identity or direction mismatch"
        );
        reports::complementary(source.yes_price_dollars, source.no_price_dollars)?;
        anyhow::ensure!(
            source.fill_count_fp.0 >= Decimal::ZERO && source.remaining_count_fp.0 >= Decimal::ZERO,
            "negative private order count"
        );
        anyhow::ensure!(
            source.status != VenueStatus::Executed
                || source.remaining_count_fp.0.is_zero() && source.fill_count_fp.0 > Decimal::ZERO,
            "invalid executed order counts"
        );

        if self.order_is_stale(source, fills, &order, ts_last) {
            return Ok(None);
        }

        self.check_settled_order(source, fills, &order, instrument.id)?;

        let original = order.clone();
        let mut before = Vec::new();
        let mut fresh = Vec::new();
        let mut after = Vec::new();

        if !order.is_closed() && order.venue_order_id().is_none() {
            let accepted = OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                venue_id,
                emitter.account_id(),
                UUID4::new(),
                ts_accepted,
                ts_init,
                false,
            );

            let event = OrderEventAny::Accepted(accepted);
            order.apply(event.clone())?;
            before.push(event);
        }

        if let Some((quantity, price)) =
            self.recovery_terms(&order, source, instrument, ts_last, mapped)?
        {
            let update = OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                quantity,
                UUID4::new(),
                ts_last,
                ts_init,
                true,
                Some(venue_id),
                Some(emitter.account_id()),
                Some(price),
                None,
                None,
                false,
            );

            let event = OrderEventAny::Updated(update);
            order.apply(event.clone())?;
            before.push(event);
        }

        let before_status = order.status();
        let before_filled = order.filled_qty();

        for fill in fills {
            if self.fill_accounted(id, fill.trade_id) {
                continue;
            }

            anyhow::ensure!(
                fill.instrument_id == instrument.id
                    && fill.account_id == emitter.account_id()
                    && fill.venue_order_id == venue_id
                    && fill.order_side == context.identity.order_side
                    && fill.commission.currency == Currency::USD()
                    && fill.commission.as_decimal() >= Decimal::ZERO
                    && fill.ts_event >= ts_accepted
                    && fill.ts_event <= ts_last,
                "private fill identity or fee mismatch"
            );

            if order.trade_ids().contains(&&fill.trade_id) {
                fresh.push((None, fill.clone()));
                continue;
            }

            let filled = OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                venue_id,
                emitter.account_id(),
                fill.trade_id,
                context.identity.order_side,
                context.identity.order_type,
                fill.last_qty,
                fill.last_px,
                Currency::USD(),
                fill.liquidity_side,
                UUID4::new(),
                fill.ts_event,
                ts_init,
                false,
                None,
                Some(fill.commission),
                None,
            );

            let event = OrderEventAny::Filled(filled);
            order.apply(event.clone())?;
            fresh.push((Some(event), fill.clone()));
        }

        anyhow::ensure!(
            order.filled_qty().as_decimal() == source.fill_count_fp.0,
            "private order fill coverage is incomplete"
        );

        if mapped
            && source.status == VenueStatus::Canceled
            && self.order_is_current(&order, source, ts_last)
        {
            let canceled = OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                UUID4::new(),
                ts_last,
                ts_init,
                false,
                Some(venue_id),
                Some(emitter.account_id()),
                None,
            );

            let event = OrderEventAny::Canceled(canceled);
            order.apply(event.clone())?;
            after.push(event);
        }

        let external = if order.strategy_id().is_external() {
            Some(
                reports::order(
                    source,
                    instrument,
                    emitter.account_id(),
                    Some(&order),
                    ts_init,
                )?
                .context("external Kalshi order metadata is incomplete")?,
            )
        } else {
            None
        };

        let external_before = external
            .as_ref()
            .filter(|_| !before.is_empty())
            .map(|report| {
                let mut report = report.clone();
                report.order_status = before_status;
                report.filled_qty = before_filled;
                report
            });

        Ok(Some(OrderPlan {
            original,
            result: order,
            venue_id,
            before,
            fresh,
            after,
            external,
            external_before,
            source_ts: ts_last,
            mapped,
        }))
    }

    fn recovery_terms(
        &self,
        order: &OrderAny,
        source: &VenueOrder,
        instrument: &BinaryOption,
        ts_last: UnixNanos,
        mapped: bool,
    ) -> anyhow::Result<Option<(Quantity, Price)>> {
        reports::quantity(source.fill_count_fp, instrument.size_precision)?;
        reports::quantity(source.remaining_count_fp, instrument.size_precision)?;
        let qty = source
            .fill_count_fp
            .0
            .checked_add(source.remaining_count_fp.0)
            .context("private order count overflow")?;
        let price =
            crate::data::parse::price(source.yes_price_dollars, instrument.price_precision)?;

        let (qty, price) = if mapped {
            (qty, price)
        } else {
            let known_qty = order.quantity().as_decimal();

            let increase_pending = order.has_pending_update_command()
                || self
                    .mutations
                    .get(&order.client_order_id())
                    .is_some_and(|pending| {
                        pending.dispatched
                            && pending.terms.is_some_and(|(qty, _)| qty > order.quantity())
                    });

            anyhow::ensure!(
                source.remaining_count_fp.0 > Decimal::ZERO
                    || source.fill_count_fp.0 <= known_qty
                        && !(increase_pending && source.fill_count_fp.0 == known_qty),
                "Kalshi fills require authoritative order quantity"
            );

            let qty = if source.remaining_count_fp.0 > Decimal::ZERO {
                qty.max(known_qty)
            } else {
                known_qty
            };

            anyhow::ensure!(
                qty == known_qty
                    || !order.has_pending_update_command()
                        && !order.has_pending_cancel_command()
                        && !self.mutations.contains_key(&order.client_order_id()),
                "Kalshi fills require authoritative order quantity"
            );

            (qty, order.price().context("owned limit order lacks price")?)
        };

        if (mapped && source.status == VenueStatus::Canceled)
            || !self.order_is_current(order, source, ts_last)
            || order.quantity().as_decimal() == qty && order.price() == Some(price)
        {
            return Ok(None);
        }

        let quantity = Quantity::from_decimal_dp(qty, instrument.size_precision)?;
        Ok(Some((quantity, price)))
    }

    pub(super) fn recovery_order(
        &self,
        source: &VenueOrder,
        instrument: &BinaryOption,
        emitter: &ExecutionEventEmitter,
        ts_init: UnixNanos,
    ) -> anyhow::Result<Option<OrderAny>> {
        if let Some(order) = self.source_order(source)? {
            return Ok(Some(order));
        }

        // Resting totals are authoritative from their filled and remaining counts
        if source.fill_count_fp.0.is_zero() && source.status != VenueStatus::Resting {
            return Ok(None);
        }

        let Some(report) = reports::order(source, instrument, emitter.account_id(), None, ts_init)?
        else {
            return Ok(None);
        };

        let client_order_id = report
            .client_order_id
            .unwrap_or_else(|| ClientOrderId::from(report.venue_order_id.as_str()));

        let order_ts_init = if source.fill_count_fp.0.is_zero() {
            ts_init
        } else {
            report.ts_accepted
        };

        let order = LimitOrder::new_checked(
            emitter.trader_id(),
            StrategyId::external(),
            instrument.id,
            client_order_id,
            report.order_side.context("recovered order lacks side")?,
            report.quantity,
            report.price.context("recovered order lacks price")?,
            report.time_in_force,
            report.expire_time,
            report.post_only,
            report.reduce_only,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            UUID4::new(),
            order_ts_init,
        )?;
        Ok(Some(OrderAny::Limit(order)))
    }

    fn check_settled_order(
        &self,
        source: &VenueOrder,
        fills: &[FillReport],
        order: &OrderAny,
        instrument_id: InstrumentId,
    ) -> anyhow::Result<()> {
        let id = order.client_order_id();

        if self
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .sealed(instrument_id)
            && (source.fill_count_fp.0 > order.filled_qty().as_decimal()
                || fills
                    .iter()
                    .any(|fill| !self.fill_accounted(id, fill.trade_id)))
        {
            self.barrier
                .lock()
                .expect("settlement barrier lock")
                .block();
            anyhow::bail!("unapplied execution evidence after settlement");
        }

        Ok(())
    }

    pub(super) fn report_covered(
        &self,
        id: ClientOrderId,
        trade: Option<TradeId>,
        filled: Decimal,
    ) -> bool {
        self.orders.get(&id).is_some_and(|context| {
            context.order.filled_qty().as_decimal() >= filled
                && trade.is_none_or(|trade| context.accounted.contains(&trade))
        })
    }

    pub(super) fn admit_reports(
        &mut self,
        cache: &Cache,
        orders: &[OrderStatusReport],
        fills: &[FillReport],
    ) -> anyhow::Result<()> {
        let mut barrier = self.barrier.lock().expect("settlement barrier lock");
        let mut changed = false;

        for report in orders {
            let cached = report.client_order_id.and_then(|id| cache.order_ref(&id));
            let received = cached
                .as_ref()
                .is_some_and(|order| order.filled_qty() >= report.filled_qty);
            let covered = report
                .client_order_id
                .is_some_and(|id| self.report_covered(id, None, report.filled_qty.as_decimal()));

            if !received
                && (cache.instrument_close(&report.instrument_id).is_some()
                    || barrier.sealed(report.instrument_id) && !covered)
            {
                barrier.block();
                anyhow::bail!("unapplied order report after settlement");
            }

            if !received {
                self.start = self.start.min(report.ts_accepted);
                barrier.admit_order(report);
                changed = true;
            }
        }

        for report in fills {
            if self.admit_fill_report(cache, &mut barrier, report)? {
                self.start = self.start.min(report.ts_event);
                changed = true;
            }
        }

        if changed {
            self.session = UUID4::new().to_string();
            self.ledger.invalidate();
            barrier.invalidate(&self.session);
        }

        Ok(())
    }

    fn admit_fill_report(
        &self,
        cache: &Cache,
        barrier: &mut SettlementBarrier,
        report: &FillReport,
    ) -> anyhow::Result<bool> {
        let id = report
            .client_order_id
            .or_else(|| cache.client_order_id(&report.venue_order_id).copied());
        let received = id
            .and_then(|id| cache.order_ref(&id))
            .is_some_and(|order| order.trade_ids().contains(&&report.trade_id));
        let covered =
            id.is_some_and(|id| self.report_covered(id, Some(report.trade_id), Decimal::ZERO));

        if !received
            && (cache.instrument_close(&report.instrument_id).is_some()
                || barrier.sealed(report.instrument_id) && !covered)
        {
            barrier.block();
            anyhow::bail!("unapplied fill report after settlement");
        }

        if !received {
            barrier.admit_fill(report);
        }

        Ok(!received)
    }

    fn order_is_stale(
        &self,
        source: &VenueOrder,
        fills: &[FillReport],
        order: &OrderAny,
        ts_last: UnixNanos,
    ) -> bool {
        if terminal_progress(order, source) {
            return false;
        }

        let id = order.client_order_id();

        let Some(context) = self.orders.get(&id) else {
            return false;
        };

        let source_ts = context.source_ts.max(order.ts_last());

        ts_last < source_ts
            && source.fill_count_fp.0 <= order.filled_qty().as_decimal()
            && fills
                .iter()
                .all(|fill| self.fill_accounted(id, fill.trade_id))
    }

    fn order_is_current(&self, order: &OrderAny, source: &VenueOrder, ts_last: UnixNanos) -> bool {
        let source_ts = self
            .orders
            .get(&order.client_order_id())
            .map_or(order.ts_last(), |context| {
                context.source_ts.max(order.ts_last())
            });

        !order.is_closed() && (ts_last >= source_ts || terminal_progress(order, source))
    }

    fn fill_accounted(&self, id: ClientOrderId, trade_id: TradeId) -> bool {
        self.fills.contains(&trade_id)
            || self
                .orders
                .get(&id)
                .is_some_and(|context| context.accounted.contains(&trade_id))
    }

    pub(super) fn openings(
        &self,
        plans: &[OrderPlan],
        positions: &[MarketPosition],
        settlements: &[Settlement],
    ) -> anyhow::Result<HashMap<InstrumentId, Decimal>> {
        let mut openings = self.positions.clone();
        let mut deltas = HashMap::new();

        for (_, fill) in plans.iter().flat_map(|plan| &plan.fresh) {
            let signed = if fill.order_side == OrderSide::Buy {
                fill.last_qty.as_decimal()
            } else {
                -fill.last_qty.as_decimal()
            };

            let delta: &mut Decimal = deltas.entry(fill.instrument_id).or_insert(Decimal::ZERO);
            *delta = delta
                .checked_add(signed)
                .context("initial position delta overflow")?;
        }

        for position in positions {
            let id = crate::common::parse::parse_instrument_id(&position.ticker)?;
            let delta = deltas.remove(&id).unwrap_or(Decimal::ZERO);
            openings.insert(id, self.opening(id, position.position_fp.0, delta)?);
        }

        for settlement in settlements {
            let id = &settlement.instrument_id;
            let delta = deltas.remove(id).unwrap_or(Decimal::ZERO);
            if let Some(net) = settlement.net {
                openings.insert(*id, self.opening(*id, net, delta)?);
            } else {
                anyhow::ensure!(
                    openings.contains_key(id),
                    "settlement lacks cached opening authority"
                );
            }
        }

        anyhow::ensure!(deltas.is_empty(), "fills lack explicit position coverage");
        Ok(openings)
    }

    fn opening(&self, id: InstrumentId, net: Decimal, delta: Decimal) -> anyhow::Result<Decimal> {
        let opening = net
            .checked_sub(delta)
            .context("initial position overflow")?;

        let prior = self.positions.get(&id);
        if !prior.map_or(opening.is_zero(), |prior| *prior == opening) {
            if prior.is_none() || !self.positions_ts.contains_key(&id) {
                return Err(RecoveryContextError::Position.into());
            }

            anyhow::bail!("position cycle lacks cached opening authority");
        }

        Ok(opening)
    }

    pub(super) fn check_amendments(&self, plans: &[OrderPlan]) -> anyhow::Result<()> {
        anyhow::ensure!(
            plans.iter().all(|plan| plan.mapped),
            "Kalshi order classification remains unmapped"
        );
        anyhow::ensure!(
            self.mutations.iter().all(|(id, pending)| {
                !pending.dispatched
                    || plans.iter().any(|plan| {
                        plan.result.client_order_id() == *id
                            && pending.covered(&plan.result, plan.source_ts)
                    })
            }),
            "Kalshi change lacks definitive order evidence"
        );
        anyhow::ensure!(
            self.orders.values().all(|context| {
                context.order.status() != OrderStatus::PendingUpdate
                    || plans.iter().any(|plan| {
                        plan.original.client_order_id() == context.order.client_order_id()
                            && plan.result.status() != OrderStatus::PendingUpdate
                    })
            }),
            "uncertain amendment lacks authoritative terms"
        );
        Ok(())
    }

    pub(super) fn check_settlements(
        &self,
        plans: &[OrderPlan],
        settlements: &[Settlement],
    ) -> anyhow::Result<()> {
        for settlement in settlements {
            let id = settlement.instrument_id;
            anyhow::ensure!(
                plans
                    .iter()
                    .filter(|plan| plan.result.instrument_id() == id)
                    .all(|plan| plan.mapped),
                "settlement has unmapped order evidence"
            );
            anyhow::ensure!(
                self.orders.values().all(|context| {
                    context.order.is_closed()
                        || context.order.instrument_id() != id
                        || plans.iter().any(|plan| {
                            plan.original.client_order_id() == context.order.client_order_id()
                                && plan.result.is_closed()
                        })
                }),
                "settlement has unresolved orders"
            );
            self.barrier
                .lock()
                .expect("settlement barrier lock")
                .check_signature(id, settlement.value, settlement.ts_event)?;
        }

        Ok(())
    }

    pub(super) fn preview(
        &self,
        plans: &[OrderPlan],
        positions: &HashMap<InstrumentId, Decimal>,
        settlements: &[Settlement],
        total: Decimal,
        holds: &mut HashMap<ReservationId, Decimal>,
    ) -> anyhow::Result<(AccountBalance, HashMap<InstrumentId, Decimal>)> {
        let mut ledger = self.ledger.clone();
        let mut positions = positions.clone();
        let mut timestamps = self.positions_ts.clone();

        let mut retained: HashMap<_, _> = plans
            .iter()
            .filter(|plan| !plan.mapped)
            .filter_map(|plan| {
                let id = plan.original.client_order_id();
                self.ledger.reservation(id).map(|hold| (id, hold))
            })
            .collect();

        for (id, pending) in &self.mutations {
            let covered = plans.iter().any(|plan| {
                plan.mapped
                    && plan.result.client_order_id() == *id
                    && pending.covered(&plan.result, plan.source_ts)
            });

            if pending.dispatched
                && pending.terms.is_some()
                && !covered
                && let Some(hold) = self.ledger.reservation(*id)
            {
                retained.insert(*id, hold);
            }
        }

        for (index, _, fill) in ordered_fills(plans) {
            let change = apply_fill(&mut ledger, &mut positions, fill.instrument_id, &fill)?;
            if let Some(hold) = retained.get_mut(&plans[index].original.client_order_id()) {
                *hold = (*hold + change.min(Decimal::ZERO)).max(Decimal::ZERO);
            }

            timestamps
                .entry(fill.instrument_id)
                .and_modify(|ts| *ts = (*ts).max(fill.ts_event))
                .or_insert(fill.ts_event);
        }

        for (id, retained) in retained {
            let hold = holds.entry(ReservationId::Client(id)).or_default();
            *hold = (*hold).max(retained);
        }

        for Settlement {
            instrument_id: id,
            value,
            ts_event: timestamp,
            ..
        } in settlements
        {
            anyhow::ensure!(
                timestamps.get(id).is_none_or(|last| *last <= *timestamp),
                "settlement precedes collected execution evidence"
            );
            apply_settlement(&mut ledger, &mut positions, *id, *value)?;
        }

        let ticket = ledger.begin_refresh();
        let balance = ledger.apply_refresh(ticket, total, holds.clone())?;
        Ok((balance, positions))
    }

    /// Emits planned order events and applies their fills to private cash and position state.
    ///
    /// # Errors
    ///
    /// Returns an error if an event receiver is closed or a fill lacks opening position evidence.
    pub fn apply_plans(
        &mut self,
        plans: Vec<OrderPlan>,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let fills = ordered_fills(&plans);

        let mut ledger = self.ledger.clone();
        let mut positions = self.positions.clone();
        for (_, _, fill) in &fills {
            apply_fill(&mut ledger, &mut positions, fill.instrument_id, fill)?;
        }

        for plan in &plans {
            self.begin_plan(plan, emitter)?;
        }

        for (index, event, report) in fills {
            let plan = &plans[index];
            let id = plan.original.client_order_id();
            if let Some(event) = event {
                if plan.external.is_some() {
                    emitter.try_send_execution_report(ExecutionReport::Fill(Box::new(
                        report.clone(),
                    )))?;
                } else {
                    emitter.try_send_order_event(event.clone())?;
                }

                self.orders
                    .get_mut(&id)
                    .expect("plan owns context")
                    .order
                    .apply(event)?;
            }

            apply_fill(
                &mut self.ledger,
                &mut self.positions,
                report.instrument_id,
                &report,
            )?;
            self.orders
                .get_mut(&id)
                .expect("plan owns context")
                .accounted
                .insert(report.trade_id);
            self.positions_ts
                .entry(report.instrument_id)
                .and_modify(|ts| *ts = (*ts).max(report.ts_event))
                .or_insert(report.ts_event);
            self.fills.add(report.trade_id);
        }

        for plan in plans {
            self.finish_plan(plan, emitter)?;
        }

        Ok(())
    }

    fn begin_plan(
        &mut self,
        plan: &OrderPlan,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = plan.original.client_order_id();
        if plan.has_events() {
            self.barrier
                .lock()
                .expect("settlement barrier lock")
                .receipt(&plan.result);
        }

        self.bindings.insert(plan.venue_id, id);
        let accounted = self
            .orders
            .get(&id)
            .map_or_default(|context| context.accounted.clone());
        let source_ts = self
            .orders
            .get(&id)
            .map_or(plan.source_ts, |context| context.source_ts);
        self.orders.insert(
            id,
            PrivateOrder {
                order: plan.original.clone(),
                accounted,
                source_ts,
            },
        );

        if let Some(report) = &plan.external_before {
            emitter.try_send_execution_report(ExecutionReport::OrderWithFills(
                Box::new(report.clone()),
                Vec::new(),
            ))?;
        }

        for event in &plan.before {
            if plan.external.is_none() {
                emitter.try_send_order_event(event.clone())?;
            }

            self.orders
                .get_mut(&id)
                .expect("plan owns context")
                .order
                .apply(event.clone())?;
        }

        Ok(())
    }

    fn finish_plan(
        &mut self,
        plan: OrderPlan,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = plan.original.client_order_id();
        if plan.mapped
            && self.mutations.get(&id).is_some_and(|pending| {
                pending.dispatched && pending.covered(&plan.result, plan.source_ts)
            })
        {
            self.mutations.remove(&id);
        }

        if let Some(report) = plan.external {
            emitter.try_send_execution_report(ExecutionReport::OrderWithFills(
                Box::new(report),
                Vec::new(),
            ))?;

            for event in plan.after {
                self.orders
                    .get_mut(&id)
                    .expect("plan owns context")
                    .order
                    .apply(event)?;
            }
        } else {
            for event in plan.after {
                emitter.try_send_order_event(event.clone())?;
                self.orders
                    .get_mut(&id)
                    .expect("plan owns context")
                    .order
                    .apply(event)?;
            }
        }

        let context = self.orders.get_mut(&id).expect("plan owns context");
        if plan.mapped {
            context.source_ts = context.source_ts.max(plan.source_ts);
        }

        if plan.mapped && plan.result.is_closed() {
            self.ledger.release(id);
        }

        Ok(())
    }

    pub(super) fn apply_position(
        &mut self,
        report: PositionStatusReport,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        let id = report.instrument_id;
        let timestamp = report.ts_last;
        emitter.try_send_execution_report(ExecutionReport::Position(Box::new(report)))?;
        self.positions_ts
            .entry(id)
            .and_modify(|last| *last = (*last).max(timestamp))
            .or_insert(timestamp);
        Ok(())
    }

    pub(super) fn settle(
        &mut self,
        instrument: &BinaryOption,
        value: Decimal,
        ts_event: UnixNanos,
        ts_init: UnixNanos,
        emitter: &ExecutionEventEmitter,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            value >= Decimal::ZERO && value <= Decimal::ONE,
            "invalid settlement value"
        );
        anyhow::ensure!(
            self.positions_ts
                .get(&instrument.id)
                .is_none_or(|last| *last <= ts_event),
            "settlement precedes collected execution evidence"
        );
        anyhow::ensure!(
            self.orders.values().all(|context| context.order.is_closed()
                || context.order.instrument_id() != instrument.id),
            "settlement has unresolved orders"
        );
        let exposure = self
            .positions
            .get(&instrument.id)
            .copied()
            .unwrap_or(Decimal::ZERO);
        self.barrier
            .lock()
            .expect("settlement barrier lock")
            .prepare(instrument.id, value, ts_event, exposure)?;
        let mut ledger = self.ledger.clone();
        let mut positions = self.positions.clone();
        if !apply_settlement(&mut ledger, &mut positions, instrument.id, value)? {
            return Ok(());
        }

        let report = PositionStatusReport::new(
            emitter.account_id(),
            instrument.id,
            PositionSide::Flat,
            Quantity::zero(instrument.size_precision),
            ts_event,
            ts_init,
            None,
            None,
            None,
        );

        emitter.try_send_execution_report(ExecutionReport::Position(Box::new(report)))?;
        self.ledger = ledger;
        self.positions = positions;
        self.positions_ts.insert(instrument.id, ts_event);
        Ok(())
    }
}

fn terminal_progress(order: &OrderAny, source: &VenueOrder) -> bool {
    !order.is_closed()
        && matches!(order.time_in_force(), TimeInForce::Ioc | TimeInForce::Fok)
        && source.client_order_id == order.client_order_id().as_str()
        && matches!(source.status, VenueStatus::Canceled | VenueStatus::Executed)
        && source.fill_count_fp.0 >= order.filled_qty().as_decimal()
}

#[derive(Debug, Clone, Copy)]
struct FillNotice {
    instrument: Option<InstrumentId>,
    ts_event: UnixNanos,
}

#[derive(Debug)]
struct OrderNotice {
    filled: Decimal,
    updated: Option<UnixNanos>,
}

#[derive(Debug)]
pub(super) struct PositionNotice {
    volume: Decimal,
    economics: Option<PositionEconomics>,
}

#[derive(Debug, thiserror::Error)]
pub(super) enum RecoveryContextError {
    #[error(
        "KALSHI_ORDER_CONTEXT_REQUIRED: filled quantity lacks command metadata; restore persisted orders"
    )]
    Order,
    #[error(
        "KALSHI_OPENING_CONTEXT_REQUIRED: position cycle lacks cached opening authority; restore complete cached orders and positions"
    )]
    Position,
}

#[derive(Debug, Deserialize)]
struct PositionEconomics {
    position_fp: FixedPoint,
    realized_pnl_dollars: FixedPoint,
    fees_paid_dollars: FixedPoint,
}

pub(super) struct Settlement {
    pub(super) instrument_id: InstrumentId,
    pub(super) value: Decimal,
    pub(super) ts_event: UnixNanos,
    pub(super) net: Option<Decimal>,
}

#[derive(Debug, Clone)]
pub(super) struct PrivateOrder {
    pub(super) order: OrderAny,
    accounted: HashSet<TradeId>,
    source_ts: UnixNanos,
}

/// Planned order events for one venue order, before they are emitted.
#[derive(Debug)]
pub struct OrderPlan {
    original: OrderAny,
    result: OrderAny,
    venue_id: VenueOrderId,
    before: Vec<OrderEventAny>,
    fresh: Vec<(Option<OrderEventAny>, FillReport)>,
    after: Vec<OrderEventAny>,
    external: Option<OrderStatusReport>,
    external_before: Option<OrderStatusReport>,
    source_ts: UnixNanos,
    pub(super) mapped: bool,
}

impl OrderPlan {
    pub(super) fn hold_terms(&self) -> (Decimal, Decimal) {
        (
            self.result.leaves_qty().as_decimal(),
            self.result
                .price()
                .expect("planned order is a limit order")
                .as_decimal(),
        )
    }

    pub(super) fn is_open(&self) -> bool {
        !self.result.is_closed()
    }

    pub(super) fn has_events(&self) -> bool {
        !self.before.is_empty() || !self.fresh.is_empty() || !self.after.is_empty()
    }
}

fn ordered_fills(plans: &[OrderPlan]) -> Vec<(usize, Option<OrderEventAny>, FillReport)> {
    let mut fills = Vec::new();

    for (index, plan) in plans.iter().enumerate() {
        for (event, report) in &plan.fresh {
            fills.push((index, event.clone(), report.clone()));
        }
    }

    fills.sort_by(|a, b| {
        a.2.ts_event
            .cmp(&b.2.ts_event)
            .then_with(|| a.2.trade_id.cmp(&b.2.trade_id))
    });

    fills
}

fn apply_settlement(
    ledger: &mut AccountLedger,
    positions: &mut HashMap<InstrumentId, Decimal>,
    id: InstrumentId,
    value: Decimal,
) -> anyhow::Result<bool> {
    anyhow::ensure!(
        value >= Decimal::ZERO && value <= Decimal::ONE,
        "invalid settlement value"
    );
    let net = positions
        .get(&id)
        .copied()
        .context("settlement lacks opening exposure")?;

    if net.is_zero() {
        return Ok(false);
    }

    let payout = if net > Decimal::ZERO {
        net.checked_mul(value)
    } else {
        (-net).checked_mul(Decimal::ONE - value)
    }
    .context("settlement payout overflow")?;

    ledger.note_cash_change(payout)?;
    positions.insert(id, Decimal::ZERO);
    Ok(true)
}

fn apply_fill(
    ledger: &mut AccountLedger,
    positions: &mut HashMap<InstrumentId, Decimal>,
    instrument_id: InstrumentId,
    fill: &FillReport,
) -> anyhow::Result<Decimal> {
    let net = positions
        .get(&instrument_id)
        .copied()
        .context("fill lacks opening position authority")?;

    let signed = if fill.order_side == OrderSide::Buy {
        fill.last_qty.as_decimal()
    } else {
        -fill.last_qty.as_decimal()
    };

    let next = net
        .checked_add(signed)
        .context("private position overflow")?;
    let change = fill
        .last_px
        .as_decimal()
        .checked_mul(-signed)
        .and_then(|value| value.checked_add(next.min(Decimal::ZERO)))
        .and_then(|value| value.checked_sub(net.min(Decimal::ZERO)))
        .and_then(|value| value.checked_sub(fill.commission.as_decimal()))
        .context("private fill cash overflow")?;
    ledger.note_cash_change(change)?;
    positions.insert(instrument_id, next);
    Ok(change)
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{DataEvent, ExecutionEvent, execution::CancelOrder};
    use nautilus_model::{
        data::{Data, InstrumentClose},
        enums::{
            AccountType, InstrumentCloseType, LiquiditySide, OrderStatus, OrderType, TimeInForce,
        },
        events::{OrderModifyDenied, OrderPendingCancel, OrderPendingUpdate, OrderSubmitted},
        identifiers::{AccountId, StrategyId},
        orders::builder::OrderTestBuilder,
        types::{Money, Price},
    };
    use rstest::rstest;
    use serde_json::Value;

    use super::*;
    use crate::http::{
        models::{BookSide, Event, Field, Fill, FixedPoint, Market, OutcomeSide},
        parse::parse_instrument,
    };

    fn instrument() -> BinaryOption {
        let f: Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(f["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(f["definitions"]["live"]["event"].clone()).unwrap();
        parse_instrument(&market, &event, UnixNanos::from(1)).unwrap()
    }

    fn source(instrument: &BinaryOption, side: OrderSide) -> VenueOrder {
        let f: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source: VenueOrder = serde_json::from_value(f["order"]["order"].clone()).unwrap();
        source.ticker = instrument.id.symbol.to_string();

        source.book_side = if side == OrderSide::Buy {
            BookSide::Bid
        } else {
            BookSide::Ask
        };

        source.outcome_side = if side == OrderSide::Buy {
            OutcomeSide::Yes
        } else {
            OutcomeSide::No
        };

        source.yes_price_dollars = FixedPoint(Decimal::new(6001, 4));
        source.no_price_dollars = FixedPoint(Decimal::new(3999, 4));
        source.fill_count_fp = FixedPoint(Decimal::new(75, 2));
        source.remaining_count_fp = FixedPoint(Decimal::new(125, 2));
        source.initial_count_fp = FixedPoint(Decimal::from(2));
        source.expiration_time = Field::Null;
        source
    }

    fn report(source: &VenueOrder, instrument: &BinaryOption, account_id: AccountId) -> FillReport {
        let f: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut fill: Fill = serde_json::from_value(f["fills"]["fills"][0].clone()).unwrap();
        fill.order_id = source.order_id.clone();
        fill.ticker = source.ticker.clone();
        fill.market_ticker = source.ticker.clone();
        fill.book_side = source.book_side;
        fill.outcome_side = source.outcome_side;
        fill.yes_price_dollars = source.yes_price_dollars;
        fill.no_price_dollars = source.no_price_dollars;
        fill.count_fp = source.fill_count_fp;
        reports::fill(&fill, instrument, account_id, source, UnixNanos::from(543)).unwrap()
    }

    fn submitted(instrument: &BinaryOption, side: OrderSide, client_id: &str) -> OrderAny {
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id(ClientOrderId::from(client_id))
            .side(side)
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();

        let submitted = OrderSubmitted::new(
            order.trader_id(),
            order.strategy_id(),
            instrument.id,
            order.client_order_id(),
            AccountId::from("KALSHI-007"),
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(2),
        );

        order.apply(OrderEventAny::Submitted(submitted)).unwrap();
        order
    }

    fn state(order: OrderAny, instrument: &BinaryOption) -> PrivateState {
        let mut state = PrivateState::new();
        state.restore(order);
        state.positions.insert(instrument.id, Decimal::ZERO);
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, Decimal::new(1000001, 4), HashMap::new())
            .unwrap();
        state
    }

    fn emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            get_atomic_clock_realtime(),
            "TRADER-001".into(),
            AccountId::from("KALSHI-007"),
            AccountType::Cash,
            Some(Currency::USD()),
        );

        emitter.set_sender(sender);
        (emitter, receiver)
    }

    #[rstest]
    #[case::new(false)]
    #[case::restored(true)]
    fn unsent_denial_fences_only_tracked_orders_and_rejects_conflicting_venue_identity(
        #[case] restored: bool,
    ) {
        let instrument = instrument();
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id("client-a".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let mut state = PrivateState::new();
        if restored {
            state.restore(order.clone());
        }

        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(
                ticket,
                Decimal::ONE,
                HashMap::from([(ReservationId::Venue("external".into()), Decimal::new(25, 2))]),
            )
            .unwrap();
        let balance = state.ledger.balance().unwrap();
        let ticket = state.ledger.begin_refresh();
        let (emitter, mut output) = emitter();
        state
            .deny_submission(&mut order, "local validation", &emitter)
            .unwrap();
        assert_eq!(order.status(), OrderStatus::Denied);

        if restored {
            assert_eq!(
                state.ledger.check_refresh(ticket).unwrap_err().to_string(),
                "stale Kalshi account refresh"
            );
        } else {
            state.ledger.check_refresh(ticket).unwrap();
        }

        assert_eq!(
            state
                .source_order(&source(&instrument, OrderSide::Buy))
                .unwrap_err()
                .to_string(),
            "venue evidence conflicts with an unsent Kalshi order"
        );
        assert_eq!(state.ledger.balance().unwrap(), balance);
        assert!(matches!(
            output.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Denied(_))
        ));
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn venue_rejection_does_not_conflict_with_recovered_order() {
        let instrument = instrument();
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        let (emitter, mut output) = emitter();
        state
            .reject_submission(id, "Post-only order would cross", true, &emitter)
            .unwrap();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);

        let recovered = state.source_order(&source).unwrap().unwrap();

        assert_eq!(recovered.status(), OrderStatus::Rejected);
        assert!(matches!(
            output.try_recv().unwrap(),
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) if event.due_post_only
        ));
    }

    #[rstest]
    #[case(VenueStatus::Resting, Decimal::ZERO, Decimal::new(1, 2))]
    #[case(VenueStatus::Canceled, Decimal::new(75, 2), Decimal::ZERO)]
    #[case(VenueStatus::Executed, Decimal::new(75, 2), Decimal::ZERO)]
    fn venue_rejection_rejects_live_or_filled_order(
        #[case] status: VenueStatus,
        #[case] filled: Decimal,
        #[case] remaining: Decimal,
    ) {
        let instrument = instrument();
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        let (emitter, _output) = emitter();
        state
            .reject_submission(id, "Post-only order would cross", true, &emitter)
            .unwrap();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = status;
        source.fill_count_fp = FixedPoint(filled);
        source.remaining_count_fp = FixedPoint(remaining);

        let error = state.source_order(&source).unwrap_err();

        assert_eq!(
            error.to_string(),
            "venue evidence conflicts with an unsent Kalshi order"
        );
    }

    #[rstest]
    fn not_sent_rejection_fences_conflicting_venue_identity() {
        let instrument = instrument();
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        let (emitter, _output) = emitter();
        state
            .reject_submission(id, submit::NOT_SENT_REASON, false, &emitter)
            .unwrap();
        let rejected = state.orders[&id].order.clone();

        let event = OrderModifyDenied::new(
            rejected.trader_id(),
            rejected.strategy_id(),
            rejected.instrument_id(),
            rejected.client_order_id(),
            "closed order".into(),
            UUID4::new(),
            UnixNanos::from(rejected.ts_last().as_u64() + 1),
            UnixNanos::from(rejected.ts_last().as_u64() + 2),
            rejected.venue_order_id(),
            Some(emitter.account_id()),
        );

        let mut rejected = rejected;
        rejected.apply(OrderEventAny::ModifyDenied(event)).unwrap();
        state.restore(rejected);
        assert!(matches!(
            state.orders[&id].order.last_event(),
            OrderEventAny::ModifyDenied(_)
        ));
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);

        let error = state.source_order(&source).unwrap_err();

        assert_eq!(
            error.to_string(),
            "venue evidence conflicts with an unsent Kalshi order"
        );
    }

    #[rstest]
    fn admission_denial_cannot_replace_an_owned_submission() {
        let instrument = instrument();
        let submitted = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut state = state(submitted.clone(), &instrument);
        let mut local = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id("client-a".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let (emitter, mut output) = emitter();
        state
            .deny_submission(&mut local, "late local validation", &emitter)
            .unwrap();
        assert_eq!(local.status(), OrderStatus::Submitted);
        assert_eq!(state.orders[&local.client_order_id()].order, submitted);
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn submission_ack_fences_refresh_and_cannot_replace_venue_evidence() {
        let instrument = instrument();
        let order = submitted(&instrument, OrderSide::Buy, "ack-client");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        let (emitter, mut output) = emitter();
        state.ledger.reserve(id, Decimal::new(62, 2)).unwrap();
        let held = state.ledger.balance().unwrap();
        let stale = state.ledger.begin_refresh();
        let venue_id = VenueOrderId::from("ack-venue");
        state
            .acknowledge(id, venue_id, UnixNanos::from(3), &emitter)
            .unwrap();
        let accepted = output.try_recv().unwrap();
        assert!(matches!(
            accepted,
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert_eq!(
            state.ledger.check_refresh(stale).unwrap_err().to_string(),
            "stale Kalshi account refresh"
        );
        state
            .acknowledge(id, venue_id, UnixNanos::from(4), &emitter)
            .unwrap();
        assert_eq!(
            state
                .acknowledge(
                    id,
                    VenueOrderId::from("other-venue"),
                    UnixNanos::from(5),
                    &emitter
                )
                .unwrap_err()
                .to_string(),
            "Kalshi acknowledgement order identity mismatch"
        );
        assert_eq!(
            state
                .reject_submission(id, "late failure", false, &emitter)
                .unwrap_err()
                .to_string(),
            "Kalshi submission already has venue evidence"
        );
        assert!(output.try_recv().is_err());
        assert_eq!(state.orders[&id].order.status(), OrderStatus::Accepted);
        assert_eq!(state.orders[&id].order.venue_order_id(), Some(venue_id));
        assert_eq!(state.ledger.balance().unwrap(), held);
    }

    #[rstest]
    #[case(OrderSide::Buy, "99.550024", "0.75")]
    #[case(OrderSide::Sell, "99.700174", "-0.75")]
    fn partial_fill_accepts_and_updates_before_filling_once(
        #[case] side: OrderSide,
        #[case] cash: &str,
        #[case] net: &str,
    ) {
        let instrument = instrument();
        let source = source(&instrument, side);
        let order = submitted(&instrument, side, "client-a");
        let mut state = state(order.clone(), &instrument);
        let (emitter, mut output) = emitter();
        let report = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&report),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            state.orders[&report.client_order_id.unwrap()]
                .order
                .venue_order_id(),
            None
        );
        state.apply_plans(vec![plan], &emitter).unwrap();
        let accepted = output.try_recv().unwrap();
        let updated = output.try_recv().unwrap();
        let filled = output.try_recv().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = accepted else {
            panic!("expected acceptance");
        };

        let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = updated else {
            panic!("expected update");
        };

        let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = filled else {
            panic!("expected fill");
        };

        assert_eq!(
            accepted,
            OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                report.venue_order_id,
                emitter.account_id(),
                accepted.event_id,
                timestamp(&source.created_time).unwrap(),
                UnixNanos::from(543),
                false
            )
        );
        assert_eq!(
            updated,
            OrderUpdated::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                Quantity::from("2.00"),
                updated.event_id,
                timestamp(&source.last_update_time).unwrap(),
                UnixNanos::from(543),
                true,
                Some(report.venue_order_id),
                Some(emitter.account_id()),
                Some(Price::from("0.600100")),
                None,
                None,
                false
            )
        );
        assert_eq!(
            filled,
            OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                report.venue_order_id,
                emitter.account_id(),
                report.trade_id,
                side,
                OrderType::Limit,
                Quantity::from("0.75"),
                Price::from("0.600100"),
                Currency::USD(),
                LiquiditySide::Taker,
                filled.event_id,
                report.ts_event,
                UnixNanos::from(543),
                false,
                None,
                Some(report.commission),
                None
            )
        );
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&report),
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert!(output.try_recv().is_err());
        assert_eq!(
            state.positions[&instrument.id],
            net.parse::<Decimal>().unwrap()
        );
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, cash.parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), cash.parse::<Decimal>().unwrap());
        assert_eq!(
            state.orders[&report.client_order_id.unwrap()]
                .order
                .filled_qty(),
            Quantity::from("0.75")
        );
    }

    #[rstest]
    #[case(false, "99.550024", "0.75")]
    #[case(true, "98.799899", "2.00")]
    fn cached_fill_is_accounted_without_routing_twice_and_survives_replay_eviction(
        #[case] terminal: bool,
        #[case] cash: &str,
        #[case] net: &str,
    ) {
        let cash: Decimal = cash.parse().unwrap();
        let net: Decimal = net.parse().unwrap();
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        if terminal {
            source.status = VenueStatus::Executed;
            source.fill_count_fp = FixedPoint(Decimal::from(2));
            source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        }

        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, output) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.restore(plan.result);
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        for i in 0..100_001 {
            state.fills.add(TradeId::new(format!("other-{i}")));
        }

        state.invalidate();
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(545),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, cash, HashMap::new())
            .unwrap();
        assert_eq!(output.len(), 0);
        assert!(!state.fills.contains(&fill.trade_id));
        assert_eq!(state.positions[&instrument.id], net);
        assert_eq!(balance.total.as_decimal(), cash);
        assert_eq!(
            state.orders[&ClientOrderId::from("client-a")]
                .order
                .is_closed(),
            terminal
        );
    }

    #[rstest]
    #[case("0.75", "1", "100.7501")]
    #[case("-0.75", "0", "100.7501")]
    #[case("0.75", "0.42", "100.3151")]
    #[case("-0.75", "0.42", "100.4351")]
    fn settlement_uses_exact_payout_and_timestamp_without_inventing_a_fill(
        #[case] net: &str,
        #[case] value: &str,
        #[case] cash: &str,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, "100.0001".parse().unwrap(), HashMap::new())
            .unwrap();
        state.positions.insert(instrument.id, net.parse().unwrap());
        state
            .positions_ts
            .insert(instrument.id, UnixNanos::from(123));
        let (emitter, mut output) = emitter();
        let value = value.parse().unwrap();
        assert_eq!(
            state
                .settle(
                    &instrument,
                    value,
                    UnixNanos::from(122),
                    UnixNanos::from(456),
                    &emitter
                )
                .unwrap_err()
                .to_string(),
            "settlement precedes collected execution evidence"
        );
        state
            .settle(
                &instrument,
                value,
                UnixNanos::from(124),
                UnixNanos::from(456),
                &emitter,
            )
            .unwrap();
        state
            .settle(
                &instrument,
                value,
                UnixNanos::from(124),
                UnixNanos::from(457),
                &emitter,
            )
            .unwrap();

        let ExecutionEvent::Report(ExecutionReport::Position(actual)) = output.try_recv().unwrap()
        else {
            panic!("expected settlement position report");
        };

        let expected = PositionStatusReport::new(
            emitter.account_id(),
            instrument.id,
            PositionSide::Flat,
            Quantity::zero(instrument.size_precision),
            UnixNanos::from(124),
            UnixNanos::from(456),
            Some(actual.report_id),
            None,
            None,
        );

        assert_eq!(*actual, expected);
        assert_eq!(output.len(), 0);
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        assert_eq!(state.positions_ts[&instrument.id], UnixNanos::from(124));
        let ticket = state.ledger.begin_refresh();
        let cash: Decimal = cash.parse().unwrap();
        let balance = state
            .ledger
            .apply_refresh(ticket, cash, HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), cash);
    }

    #[rstest]
    fn invalid_fee_or_identity_remains_replayable_and_failed_routing_does_not_advance_context() {
        let instrument = instrument();
        let source = source(&instrument, OrderSide::Buy);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, output) = emitter();
        let valid = report(&source, &instrument, emitter.account_id());
        let mut invalid = valid.clone();
        invalid.commission =
            Money::from_decimal_exact(Decimal::new(-1, 6), Currency::USD()).unwrap();
        assert!(
            state
                .plan_order(
                    &source,
                    &[invalid],
                    &instrument,
                    &emitter,
                    UnixNanos::from(543)
                )
                .is_err()
        );
        let mut invalid = valid.clone();
        invalid.venue_order_id = VenueOrderId::from("wrong-order");
        assert!(
            state
                .plan_order(
                    &source,
                    &[invalid],
                    &instrument,
                    &emitter,
                    UnixNanos::from(543)
                )
                .is_err()
        );
        assert!(!state.fills.contains(&valid.trade_id));
        drop(output);
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&valid),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        assert!(state.apply_plans(vec![plan], &emitter).is_err());
        assert_eq!(
            state.orders[&valid.client_order_id.unwrap()].order.status(),
            OrderStatus::Submitted
        );
        assert!(!state.fills.contains(&valid.trade_id));
        let (emitter, mut output) = self::emitter();
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&valid),
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert_eq!(output.len(), 3);
        assert!(output.try_recv().is_ok());
        assert!(state.fills.contains(&valid.trade_id));
    }

    #[rstest]
    fn malformed_terminal_counts_and_unknown_outcomes_do_not_release_or_reject() {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        let state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, output) = emitter();
        let valid = report(&source, &instrument, emitter.account_id());
        source.status = VenueStatus::Executed;
        assert_eq!(
            state
                .plan_order(
                    &source,
                    std::slice::from_ref(&valid),
                    &instrument,
                    &emitter,
                    UnixNanos::from(543)
                )
                .err()
                .unwrap()
                .to_string(),
            "invalid executed order counts"
        );
        source.status = VenueStatus::Unknown;
        let plan = state
            .plan_order(
                &source,
                &[valid],
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        assert!(!plan.mapped);
        assert!(state.check_amendments(&[plan]).is_err());
        assert_eq!(
            state.orders[&ClientOrderId::from("client-a")]
                .order
                .status(),
            OrderStatus::Submitted
        );
        assert_eq!(output.len(), 0);
    }

    #[rstest]
    #[case::buy_open(OrderSide::Buy, "0", "99.550024", "1.549924", "0.75")]
    #[case::sell_open(OrderSide::Sell, "0", "99.700174", "1.700074", "-0.75")]
    #[case::sell_reduce(OrderSide::Sell, "1", "100.450174", "2", "0.25")]
    #[case::buy_reduce(OrderSide::Buy, "-1", "100.300024", "2", "-0.25")]
    fn unresolved_reservation_consumes_only_cash_debits(
        #[case] side: OrderSide,
        #[case] opening: &str,
        #[case] total: &str,
        #[case] retained: &str,
        #[case] position: &str,
        #[values(false, true)] mapped: bool,
    ) {
        let instrument = instrument();
        let order = submitted(&instrument, side, "client-a");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        state
            .positions
            .insert(instrument.id, opening.parse().unwrap());
        state.ledger.reserve(id, Decimal::ONE).unwrap();
        state
            .ledger
            .reserve_amendment(id, Decimal::from(2), Decimal::from(2), Decimal::ONE)
            .unwrap();
        let command_id = UUID4::new();
        state.mutations.insert(
            id,
            PendingMutation {
                command_id,
                started: UnixNanos::from(2),
                dispatched: true,
                acknowledgement: None,
                terms: Some((Quantity::from("3.00"), Price::from("0.600000"))),
            },
        );

        let mut source = source(&instrument, side);

        source.status = if mapped {
            VenueStatus::Resting
        } else {
            VenueStatus::Unknown
        };

        let (emitter, mut output) = emitter();
        let mut fill = report(&source, &instrument, emitter.account_id());
        fill.commission = Money::from_decimal_exact(Decimal::new(1, 6), Currency::USD()).unwrap();
        let mut holds = HashMap::from([(ReservationId::Client(id), Decimal::new(1, 2))]);

        for replay in 0..2 {
            let plan = state
                .plan_order(
                    &source,
                    std::slice::from_ref(&fill),
                    &instrument,
                    &emitter,
                    UnixNanos::from(543),
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                state
                    .check_settlements(
                        std::slice::from_ref(&plan),
                        &[Settlement {
                            instrument_id: instrument.id,
                            value: Decimal::ONE,
                            ts_event: UnixNanos::from(544),
                            net: None
                        }]
                    )
                    .unwrap_err()
                    .to_string(),
                if mapped {
                    "settlement has unresolved orders"
                } else {
                    "settlement has unmapped order evidence"
                }
            );
            let (balance, positions) = state
                .preview(
                    std::slice::from_ref(&plan),
                    &state.positions,
                    &[],
                    total.parse().unwrap(),
                    &mut holds,
                )
                .unwrap();
            assert_eq!(
                positions[&instrument.id],
                position.parse::<Decimal>().unwrap()
            );
            assert_eq!(
                balance.total.as_decimal(),
                total.parse::<Decimal>().unwrap()
            );
            assert_eq!(
                balance.locked.as_decimal(),
                retained.parse::<Decimal>().unwrap()
            );
            assert_eq!(
                balance.free.as_decimal(),
                total.parse::<Decimal>().unwrap() - retained.parse::<Decimal>().unwrap()
            );
            assert_eq!(balance.currency, Currency::USD());
            state.apply_plans(vec![plan], &emitter).unwrap();
            let ticket = state.ledger.begin_refresh();
            state
                .ledger
                .apply_refresh(ticket, total.parse().unwrap(), holds.clone())
                .unwrap();
            assert_eq!(
                state.positions[&instrument.id],
                position.parse::<Decimal>().unwrap()
            );
            assert_eq!(state.ledger.balance().unwrap(), balance);
            assert_eq!(
                state.ledger.reservation(id),
                Some(retained.parse().unwrap())
            );
            assert_eq!(state.mutations[&id].command_id, command_id);
            assert_eq!(
                output.len(),
                if replay == 0 {
                    if mapped { 3 } else { 2 }
                } else {
                    0
                }
            );

            while output.try_recv().is_ok() {}
        }
    }

    #[rstest]
    fn unmapped_quantity_change_waits_without_resolving_commands(
        #[values(false, true)] amendment: bool,
        #[values(false, true)] fresh_claim: bool,
        #[values(0, 1)] filled: u32,
    ) {
        let instrument = instrument();
        let mut order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let mut source = source(&instrument, OrderSide::Buy);
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                VenueOrderId::from(source.order_id.as_str()),
                order.account_id().unwrap(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
            )))
            .unwrap();
        let command_id = UUID4::new();

        if amendment {
            let mut pending = OrderPendingUpdate::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                order.account_id(),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                order.venue_order_id(),
            );

            pending.causation_id = Some(command_id);
            order.apply(OrderEventAny::PendingUpdate(pending)).unwrap();
        } else {
            let mut pending = OrderPendingCancel::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                order.account_id(),
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                order.venue_order_id(),
            );

            pending.causation_id = Some(command_id);
            order.apply(OrderEventAny::PendingCancel(pending)).unwrap();
        }

        let mut state = state(order.clone(), &instrument);
        if fresh_claim {
            state.mutations.insert(
                id,
                PendingMutation {
                    command_id,
                    started: UnixNanos::from(3),
                    dispatched: true,
                    acknowledgement: None,
                    terms: amendment.then_some((Quantity::from("3.00"), Price::from("0.700000"))),
                },
            );
        }

        let (emitter, mut output) = emitter();
        source.status = VenueStatus::Unknown;
        source.fill_count_fp = FixedPoint(Decimal::from(filled));
        source.remaining_count_fp = FixedPoint(Decimal::from(3 - filled));

        let fills = if filled == 0 {
            vec![]
        } else {
            vec![report(&source, &instrument, emitter.account_id())]
        };

        let before = state.ledger.balance().unwrap();
        assert_eq!(
            state
                .plan_order(&source, &fills, &instrument, &emitter, UnixNanos::from(543))
                .unwrap_err()
                .to_string(),
            "Kalshi fills require authoritative order quantity"
        );
        assert_eq!(state.orders[&id].order, order);
        assert_eq!(
            state.orders[&id].order.has_pending_update_command(),
            amendment
        );
        assert_eq!(
            state.orders[&id].order.has_pending_cancel_command(),
            !amendment
        );
        assert_eq!(state.ledger.balance().unwrap(), before);
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        assert_eq!(
            state.mutations.get(&id).map(|pending| pending.command_id),
            fresh_claim.then_some(command_id)
        );
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    fn unknown_quantity_boundary_waits_for_authoritative_terms(
        #[values(false, true)] restored: bool,
    ) {
        let instrument = instrument();
        let mut order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let command_id = UUID4::new();

        if restored {
            let venue = source(&instrument, OrderSide::Buy);
            order
                .apply(OrderEventAny::Accepted(OrderAccepted::new(
                    order.trader_id(),
                    order.strategy_id(),
                    instrument.id,
                    id,
                    VenueOrderId::from(venue.order_id.as_str()),
                    order.account_id().unwrap(),
                    UUID4::new(),
                    UnixNanos::from(2),
                    UnixNanos::from(2),
                    false,
                )))
                .unwrap();

            let mut pending = OrderPendingUpdate::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                id,
                order.account_id(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
                order.venue_order_id(),
            );

            pending.causation_id = Some(command_id);
            order.apply(OrderEventAny::PendingUpdate(pending)).unwrap();
        }

        let mut state = state(order.clone(), &instrument);
        let (emitter, mut output) = emitter();

        if !restored {
            state.mutations.insert(
                id,
                PendingMutation {
                    command_id,
                    started: UnixNanos::from(2),
                    dispatched: true,
                    acknowledgement: None,
                    terms: Some((Quantity::from("3.00"), Price::from("0.600000"))),
                },
            );
        }

        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Unknown;
        source.fill_count_fp = FixedPoint(Decimal::from(2));
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let fill = report(&source, &instrument, emitter.account_id());
        let before = state.ledger.balance().unwrap();
        assert_eq!(
            state
                .plan_order(
                    &source,
                    std::slice::from_ref(&fill),
                    &instrument,
                    &emitter,
                    UnixNanos::from(543)
                )
                .unwrap_err()
                .to_string(),
            "Kalshi fills require authoritative order quantity"
        );
        assert_eq!(state.orders[&id].order, order);
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        assert_eq!(state.ledger.balance().unwrap(), before);

        if restored {
            assert!(!state.mutations.contains_key(&id));
        } else {
            assert_eq!(state.mutations[&id].command_id, command_id);
        }

        assert!(output.try_recv().is_err());
        source.status = VenueStatus::Resting;
        source.remaining_count_fp = FixedPoint(Decimal::ONE);
        let plan = state
            .plan_order(
                &source,
                &[fill],
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let current = &state.orders[&id].order;
        assert_eq!(current.quantity(), Quantity::from("3.00"));
        assert_eq!(current.filled_qty(), Quantity::from("2.00"));
        assert_eq!(current.leaves_qty(), Quantity::from("1.00"));
        assert_eq!(current.status(), OrderStatus::PartiallyFilled);
        assert_eq!(state.positions[&instrument.id], Decimal::from(2));

        if restored {
            assert!(!state.mutations.contains_key(&id));
        } else {
            assert_eq!(state.mutations[&id].command_id, command_id);
        }

        assert_eq!(output.len(), if restored { 2 } else { 3 });
    }

    #[rstest]
    fn active_ownership_survives_replay_eviction_and_reconnect() {
        let instrument = instrument();
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let id = order.client_order_id();
        let mut state = state(order, &instrument);
        let source = source(&instrument, OrderSide::Buy);
        let retained = TradeId::from("retained-fill");
        state.fills.add(retained);
        for i in 0..100_001 {
            state.fills.add(TradeId::new(format!("other-{i}")));
        }

        state.invalidate();
        assert!(!state.fills.contains(&retained));
        assert_eq!(
            state
                .source_order(&source)
                .unwrap()
                .unwrap()
                .client_order_id(),
            id
        );
        assert_eq!(state.orders[&id].order.status(), OrderStatus::Submitted);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn reconnect_replays_cash_in_fill_time_order_across_orders(#[case] tied: bool) {
        let instrument = instrument();
        let close = submitted(&instrument, OrderSide::Sell, "client-z");
        let reopen = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut state = state(close, &instrument);
        state.restore(reopen);
        state.positions.insert(instrument.id, Decimal::ONE);
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, Decimal::new(1, 4), HashMap::new())
            .unwrap();
        let (emitter, mut output) = emitter();
        let mut closing = source(&instrument, OrderSide::Sell);
        closing.order_id = "z-close".into();
        closing.client_order_id = "client-z".into();
        let mut opening = source(&instrument, OrderSide::Buy);
        opening.order_id = "a-open".into();
        let mut close_fill = report(&closing, &instrument, emitter.account_id());
        close_fill.trade_id = TradeId::from("fill-z-close");
        let mut open_fill = report(&opening, &instrument, emitter.account_id());
        open_fill.trade_id = TradeId::from("fill-a-open");
        open_fill.ts_event = UnixNanos::from(close_fill.ts_event.as_u64() + u64::from(!tied));
        let opening = state
            .plan_order(
                &opening,
                &[open_fill],
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        let closing = state
            .plan_order(
                &closing,
                &[close_fill],
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![opening, closing], &emitter).unwrap();
        let mut actual = Vec::new();

        while let Ok(event) = output.try_recv() {
            if let ExecutionEvent::Order(OrderEventAny::Filled(event)) = event {
                actual.push(event.trade_id);
            }
        }

        assert_eq!(
            actual,
            if tied {
                vec![TradeId::from("fill-a-open"), TradeId::from("fill-z-close")]
            } else {
                vec![TradeId::from("fill-z-close"), TradeId::from("fill-a-open")]
            }
        );
        assert_eq!(state.positions[&instrument.id], Decimal::ONE);
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, Decimal::new(98, 6), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), Decimal::new(98, 6));
    }

    #[rstest]
    fn full_fill_and_stale_terminal_replay_preserve_the_completed_order_and_cash() {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Executed;
        source.fill_count_fp = FixedPoint(Decimal::from(2));
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, mut output) = emitter();
        let mut fill = report(&source, &instrument, emitter.account_id());
        fill.ts_event = UnixNanos::from(fill.ts_event.as_u64() + 1);
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert_eq!(output.len(), 3);

        while output.try_recv().is_ok() {}
        let mut stale = source.clone();
        stale.status = VenueStatus::Canceled;
        stale.fill_count_fp = FixedPoint(Decimal::ZERO);
        stale.last_update_time = source.created_time.clone();
        assert!(
            state
                .plan_order(&stale, &[], &instrument, &emitter, UnixNanos::from(544))
                .unwrap()
                .is_none()
        );
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(545),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        state
            .acknowledge(
                ClientOrderId::from("client-a"),
                VenueOrderId::new_checked(&source.order_id).unwrap(),
                UnixNanos::from(546),
                &emitter,
            )
            .unwrap();
        assert!(output.try_recv().is_err());
        assert!(
            state.orders[&ClientOrderId::from("client-a")]
                .order
                .is_closed()
        );
        assert_eq!(
            state
                .orders
                .get(&ClientOrderId::from("client-a"))
                .unwrap()
                .order
                .status(),
            OrderStatus::Filled
        );
        assert_eq!(state.positions[&instrument.id], Decimal::from(2));
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, "98.799899".parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), "98.799899".parse().unwrap());
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn external_cached_metadata_routes_reports_and_applies_account_evidence_once(
        #[case] acknowledged: bool,
    ) {
        let instrument = instrument();
        let source = source(&instrument, OrderSide::Buy);
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id(ClientOrderId::from("client-a"))
            .strategy_id(StrategyId::external())
            .side(OrderSide::Buy)
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.600100"))
            .time_in_force(TimeInForce::Gtc)
            .build();

        let accepted = OrderAccepted::new(
            order.trader_id(),
            order.strategy_id(),
            instrument.id,
            order.client_order_id(),
            VenueOrderId::from("order-a"),
            AccountId::from("KALSHI-007"),
            UUID4::new(),
            timestamp(&source.created_time).unwrap(),
            UnixNanos::from(9),
            false,
        );

        if acknowledged {
            order.apply(OrderEventAny::Accepted(accepted)).unwrap();
        }

        let mut state = state(order, &instrument);
        let (emitter, mut output) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();

        if !acknowledged {
            let ExecutionEvent::Report(ExecutionReport::OrderWithFills(before, fills)) =
                output.try_recv().unwrap()
            else {
                panic!("expected external acceptance report before fill");
            };

            assert_eq!(before.order_status, OrderStatus::Accepted);
            assert_eq!(before.filled_qty, Quantity::from("0.00"));
            assert_eq!(before.quantity, Quantity::from("2.00"));
            assert_eq!(before.price, Some(fill.last_px));
            assert_eq!(before.client_order_id, fill.client_order_id);
            assert_eq!(before.venue_order_id, fill.venue_order_id);
            assert_eq!(fills, Vec::new());
        }

        let ExecutionEvent::Report(ExecutionReport::Fill(actual)) = output.try_recv().unwrap()
        else {
            panic!("expected external fill report");
        };

        assert_eq!(*actual, fill);

        let ExecutionEvent::Report(ExecutionReport::OrderWithFills(order, fills)) =
            output.try_recv().unwrap()
        else {
            panic!("expected external snapshot report");
        };

        assert_eq!(order.order_status, OrderStatus::PartiallyFilled);
        assert_eq!(order.filled_qty, Quantity::from("0.75"));
        assert_eq!(order.quantity, Quantity::from("2.00"));
        assert_eq!(order.client_order_id, Some(ClientOrderId::from("client-a")));
        assert_eq!(order.price, Some(fill.last_px));
        assert_eq!(fills, Vec::new());
        assert!(output.try_recv().is_err());
        let plan = state
            .plan_order(
                &source,
                &[fill],
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert_eq!(output.len(), 1);
        assert_eq!(state.positions[&instrument.id], "0.75".parse().unwrap());
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, "99.550024".parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), "99.550024".parse().unwrap());
    }

    #[rstest]
    #[case::buy(false, true, OrderSide::Buy, "99.550024", "0.75")]
    #[case::partial(false, false, OrderSide::Buy, "99.550024", "0.75")]
    #[case::sell_without_client_id(true, true, OrderSide::Sell, "99.700174", "-0.75")]
    fn unknown_external_recovery_preserves_core_economics_and_owned_context(
        #[case] empty_id: bool,
        #[case] terminal: bool,
        #[case] side: OrderSide,
        #[case] cash: &str,
        #[case] net: &str,
    ) {
        use nautilus_live::testing::ExecutionHarness;
        use nautilus_model::{accounts::AccountAny, events::AccountState};

        let instrument = instrument();
        let (emitter, mut output) = emitter();

        let harness = ExecutionHarness::new(
            emitter.trader_id(),
            "KALSHI".into(),
            emitter.account_id(),
            instrument.clone().into(),
        );

        let account = AccountState::new(
            emitter.account_id(),
            AccountType::Cash,
            Vec::new(),
            Vec::new(),
            true,
            UUID4::new(),
            UnixNanos::from(1),
            UnixNanos::from(1),
            Some(Currency::USD()),
        );

        harness
            .cache()
            .borrow_mut()
            .add_account(AccountAny::from(account))
            .unwrap();
        let owned = submitted(&instrument, side, "owned-order");
        harness
            .cache()
            .borrow_mut()
            .add_order(owned.clone(), None, Some(harness.client_id()), false)
            .unwrap();
        let mut state = state(owned.clone(), &instrument);
        let mut owned_source = source(&instrument, side);
        owned_source.client_order_id = "owned-order".into();
        owned_source.order_id = "owned-venue-order".into();
        assert_eq!(
            state
                .recovery_order(&owned_source, &instrument, &emitter, UnixNanos::from(543))
                .unwrap(),
            Some(owned.clone())
        );

        let mut source = source(&instrument, side);
        if terminal {
            source.status = VenueStatus::Executed;
            source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        }

        if empty_id {
            source.client_order_id.clear();
        }

        let id = ClientOrderId::from(if empty_id { "order-a" } else { "client-a" });
        let mut fill = report(&source, &instrument, emitter.account_id());
        fill.client_order_id = Some(id);

        for iteration in 0..2 {
            let plan = state
                .plan_order(
                    &source,
                    std::slice::from_ref(&fill),
                    &instrument,
                    &emitter,
                    UnixNanos::from(543 + iteration),
                )
                .unwrap()
                .unwrap();
            state.apply_plans(vec![plan], &emitter).unwrap();
            let mut fill_reports = 0;

            while let Ok(event) = output.try_recv() {
                let ExecutionEvent::Report(report) = event else {
                    panic!("external recovery emitted owned order event")
                };

                if let ExecutionReport::Fill(actual) = &report {
                    assert_eq!(actual.as_ref(), &fill);
                    fill_reports += 1;
                }

                harness
                    .exec_engine()
                    .borrow_mut()
                    .reconcile_execution_report(&report);
            }

            assert_eq!(fill_reports, usize::from(iteration == 0));
        }

        let cache = harness.cache().borrow();
        let external = cache.order(&id).unwrap();
        assert_eq!(external.strategy_id(), StrategyId::external());
        assert_eq!(
            external.status(),
            if terminal {
                OrderStatus::Filled
            } else {
                OrderStatus::PartiallyFilled
            }
        );
        assert_eq!(
            external.quantity(),
            Quantity::from(if terminal { "0.75" } else { "2.00" })
        );
        assert_eq!(external.filled_qty(), Quantity::from("0.75"));
        assert_eq!(external.trade_ids(), vec![&fill.trade_id]);
        assert_eq!(external.time_in_force(), TimeInForce::Gtc);
        assert!(!external.is_post_only());
        assert!(!external.is_reduce_only());
        assert_eq!(
            cache.order(&owned.client_order_id()).unwrap().cloned(),
            owned
        );
        let positions = cache.positions_open(
            None,
            Some(&instrument.id),
            None,
            Some(&emitter.account_id()),
            None,
        );
        assert_eq!(positions.len(), 1);
        assert_eq!(positions[0].signed_decimal_qty(), net.parse().unwrap());
        assert_eq!(positions[0].commissions(), vec![fill.commission]);
        assert_eq!(
            positions[0].trade_ids,
            [fill.trade_id].into_iter().collect()
        );
        assert_eq!(state.positions[&instrument.id], net.parse().unwrap());
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, cash.parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), cash.parse().unwrap());
    }

    #[rstest]
    fn external_local_identity_cannot_replace_an_owned_order() {
        let instrument = instrument();
        let owned = submitted(&instrument, OrderSide::Buy, "order-a");
        let state = state(owned.clone(), &instrument);
        let (emitter, mut output) = emitter();
        let mut source = source(&instrument, OrderSide::Buy);
        source.client_order_id.clear();
        let error = state
            .recovery_order(&source, &instrument, &emitter, UnixNanos::from(543))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "recovered order identity conflicts with an existing order"
        );
        assert_eq!(state.orders[&owned.client_order_id()].order, owned);
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    #[case(OrderStatus::Accepted, false, "100.0001", "0")]
    #[case(OrderStatus::Accepted, true, "100.0001", "0")]
    #[case(OrderStatus::Canceled, false, "100.0001", "0")]
    #[case(OrderStatus::Canceled, true, "100.0001", "0")]
    #[case(OrderStatus::PartiallyFilled, false, "99.550024", "0.75")]
    #[case(OrderStatus::PartiallyFilled, true, "99.550024", "0.75")]
    fn restore_preserves_execution_progress_when_timestamps_tie(
        #[case] progressed_status: OrderStatus,
        #[case] cached_ahead: bool,
        #[case] cash: &str,
        #[case] net: &str,
    ) {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let id = ClientOrderId::from("client-a");
        let ts_event = state.orders[&id].order.ts_last();
        let timestamp = jiff::Timestamp::from_nanosecond(i128::from(ts_event.as_u64())).unwrap();
        source.created_time = Field::Value(timestamp);
        source.last_update_time = Field::Value(timestamp);
        if progressed_status != OrderStatus::PartiallyFilled {
            source.fill_count_fp = FixedPoint(Decimal::ZERO);
            source.remaining_count_fp = FixedPoint(Decimal::from(2));
        }

        if progressed_status == OrderStatus::Canceled {
            source.status = VenueStatus::Canceled;
        }

        let (emitter, mut output) = emitter();

        if progressed_status != OrderStatus::Accepted {
            state
                .acknowledge(id, source.order_id.as_str().into(), ts_event, &emitter)
                .unwrap();
        }

        let lagging = state.orders[&id].order.clone();

        let fills = if progressed_status == OrderStatus::PartiallyFilled {
            let mut fill = report(&source, &instrument, emitter.account_id());
            fill.ts_event = ts_event;
            vec![fill]
        } else {
            Vec::new()
        };

        let plan = state
            .plan_order(&source, &fills, &instrument, &emitter, UnixNanos::from(543))
            .unwrap()
            .unwrap();
        let progressed = plan.result.clone();
        assert_eq!(lagging.ts_last(), progressed.ts_last());

        if cached_ahead {
            state.restore(progressed.clone());
        } else {
            state.apply_plans(vec![plan], &emitter).unwrap();
            state.restore(lagging);
        }

        assert_eq!(state.orders[&id].order, progressed);

        while output.try_recv().is_ok() {}
        let plan = state
            .plan_order(&source, &fills, &instrument, &emitter, UnixNanos::from(544))
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, cash.parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(state.orders[&id].order.status(), progressed_status);
        assert_eq!(
            state.positions[&instrument.id],
            net.parse::<Decimal>().unwrap()
        );
        assert_eq!(balance.total.as_decimal(), cash.parse::<Decimal>().unwrap());
        assert_eq!(output.len(), 0);
    }

    #[rstest]
    fn new_cached_completed_accounting_rebases_without_applying_cached_fill_again() {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Executed;
        source.fill_count_fp = FixedPoint(Decimal::from(2));
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, mut output) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        let cached = plan.result.clone();
        state.apply_plans(vec![plan], &emitter).unwrap();

        while output.try_recv().is_ok() {}
        let ticket = state.ledger.begin_refresh();
        let total: Decimal = "98.799899".parse().unwrap();
        state
            .ledger
            .apply_refresh(ticket, total, HashMap::new())
            .unwrap();
        state.orders.clear();
        state.bindings.clear();
        state.fills.clear();
        state.restore(cached);
        let plan = state
            .plan_order(
                &source,
                &[fill],
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert_eq!(state.positions[&instrument.id], Decimal::from(2));
        assert_eq!(output.len(), 0);
        let ticket = state.ledger.begin_refresh();
        assert_eq!(
            state
                .ledger
                .apply_refresh(ticket, total, HashMap::new())
                .unwrap()
                .total
                .as_decimal(),
            total
        );
    }

    #[rstest]
    fn cached_positions_seed_once_until_an_accounting_rebase() {
        let instrument = instrument();
        let mut state = PrivateState::new();
        let cached = vec![
            (instrument.id, Decimal::new(275, 2), UnixNanos::from(123)),
            (instrument.id, Decimal::new(-75, 2), UnixNanos::from(125)),
        ];
        state.seed_positions(cached.clone()).unwrap();
        state.seed_positions(cached).unwrap();
        assert!(state.ledger.total_is_unknown());
        assert_eq!(
            state.positions,
            HashMap::from([(instrument.id, Decimal::from(2))])
        );
        assert_eq!(
            state.positions_ts,
            HashMap::from([(instrument.id, UnixNanos::from(125))])
        );
        state.rebase();
        state
            .seed_positions([(instrument.id, Decimal::new(75, 2), UnixNanos::from(126))])
            .unwrap();
        assert_eq!(
            state.positions,
            HashMap::from([(instrument.id, Decimal::new(75, 2))])
        );
        assert_eq!(
            state.positions_ts,
            HashMap::from([(instrument.id, UnixNanos::from(126))])
        );
    }

    #[rstest]
    fn failed_initial_settlement_preview_keeps_the_opening_net_and_is_retryable() {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state
            .seed_positions([(instrument.id, Decimal::new(75, 2), UnixNanos::from(123))])
            .unwrap();

        let settlements = [Settlement {
            instrument_id: instrument.id,
            value: Decimal::ONE,
            ts_event: UnixNanos::from(124),
            net: None,
        }];

        let openings = state.openings(&[], &[], &settlements).unwrap();
        let mut holds = HashMap::from([(
            ReservationId::Client(ClientOrderId::from("pending")),
            Decimal::from(2),
        )]);
        assert_eq!(
            state
                .preview(&[], &openings, &settlements, Decimal::ONE, &mut holds)
                .unwrap_err()
                .to_string(),
            "insufficient Kalshi cash for admission holds"
        );
        assert!(state.ledger.total_is_unknown());
        assert_eq!(state.positions, openings);
        assert_eq!(state.positions_ts[&instrument.id], UnixNanos::from(123));
        let (balance, projected) = state
            .preview(
                &[],
                &openings,
                &settlements,
                Decimal::ONE,
                &mut HashMap::new(),
            )
            .unwrap();
        assert_eq!(projected[&instrument.id], Decimal::ZERO);
        assert_eq!(balance.total.as_decimal(), Decimal::ONE);
        assert_eq!(balance.free.as_decimal(), Decimal::ONE);
        assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
        assert!(state.ledger.total_is_unknown());
        assert_eq!(state.positions, openings);
    }

    #[rstest]
    fn late_fill_after_cancellation_uses_the_retained_owned_context() {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, mut output) = emitter();
        let first = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&first),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();

        while output.try_recv().is_ok() {}
        let stale = source.clone();
        source.status = VenueStatus::Executed;
        source.fill_count_fp = FixedPoint(Decimal::from(2));
        source.last_update_time = Field::Value("2026-10-16T08:08:08.800000008Z".parse().unwrap());
        let mut late = first.clone();
        late.trade_id = TradeId::from("late-fill");
        late.last_qty = Quantity::from("1.25");
        late.ts_event = UnixNanos::from(first.ts_event.as_u64() + 1);
        let plan = state
            .plan_order(
                &source,
                &[first, late.clone()],
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert!(
            matches!(output.try_recv().unwrap(), ExecutionEvent::Order(OrderEventAny::Filled(event)) if event.trade_id == late.trade_id && event.last_qty == late.last_qty && event.client_order_id == ClientOrderId::from("client-a"))
        );
        assert_eq!(output.len(), 0);
        assert_eq!(
            state
                .orders
                .get(&ClientOrderId::from("client-a"))
                .unwrap()
                .order
                .status(),
            OrderStatus::Filled
        );
        assert_eq!(state.positions[&instrument.id], Decimal::from(2));
        assert!(
            state
                .plan_order(&stale, &[], &instrument, &emitter, UnixNanos::from(545))
                .unwrap()
                .is_none()
        );
    }

    #[rstest]
    #[case(true)]
    #[case(false)]
    fn uncertain_amendment_or_cancellation_is_retained_until_authoritative_terminal_evidence(
        #[case] amendment: bool,
    ) {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        source.remaining_count_fp = FixedPoint(Decimal::from(2));
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, mut output) = emitter();
        let plan = state
            .plan_order(&source, &[], &instrument, &emitter, UnixNanos::from(543))
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();

        while output.try_recv().is_ok() {}
        let mut order = state.orders[&ClientOrderId::from("client-a")].order.clone();

        let pending = if amendment {
            OrderEventAny::PendingUpdate(OrderPendingUpdate::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                order.account_id(),
                UUID4::new(),
                UnixNanos::from(timestamp(&source.last_update_time).unwrap().as_u64() + 1),
                UnixNanos::from(544),
                false,
                order.venue_order_id(),
            ))
        } else {
            OrderEventAny::PendingCancel(OrderPendingCancel::new(
                order.trader_id(),
                order.strategy_id(),
                instrument.id,
                order.client_order_id(),
                order.account_id(),
                UUID4::new(),
                UnixNanos::from(timestamp(&source.last_update_time).unwrap().as_u64() + 1),
                UnixNanos::from(544),
                false,
                order.venue_order_id(),
            ))
        };

        order.apply(pending).unwrap();
        let status = order.status();
        state.restore(order);
        assert!(
            state
                .plan_order(&source, &[], &instrument, &emitter, UnixNanos::from(545))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            state.orders[&ClientOrderId::from("client-a")]
                .order
                .status(),
            status
        );

        if amendment {
            assert_eq!(
                state.check_amendments(&[]).unwrap_err().to_string(),
                "uncertain amendment lacks authoritative terms"
            );
        } else {
            state.check_amendments(&[]).unwrap();
        }

        assert_eq!(output.len(), 0);
        source.status = VenueStatus::Canceled;
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        source.last_update_time = Field::Value("2026-10-16T08:08:08.800000008Z".parse().unwrap());
        let plan = state
            .plan_order(&source, &[], &instrument, &emitter, UnixNanos::from(546))
            .unwrap()
            .unwrap();
        state.check_amendments(std::slice::from_ref(&plan)).unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert!(
            matches!(output.try_recv().unwrap(), ExecutionEvent::Order(OrderEventAny::Canceled(event)) if event.client_order_id == ClientOrderId::from("client-a") && event.ts_event == timestamp(&source.last_update_time).unwrap())
        );
        assert_eq!(output.len(), 0);
        assert_eq!(
            state
                .orders
                .get(&ClientOrderId::from("client-a"))
                .unwrap()
                .order
                .status(),
            OrderStatus::Canceled
        );
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        let ticket = state.ledger.begin_refresh();
        let cash: Decimal = "100.0001".parse().unwrap();
        assert_eq!(
            state
                .ledger
                .apply_refresh(ticket, cash, HashMap::new())
                .unwrap()
                .total
                .as_decimal(),
            cash
        );
    }

    #[rstest]
    fn successfully_routed_position_evidence_prevents_an_older_settlement() {
        let instrument = instrument();
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        state.positions.insert(instrument.id, Decimal::new(75, 2));
        state
            .positions_ts
            .insert(instrument.id, UnixNanos::from(100));
        let (closed, output) = emitter();

        let report = PositionStatusReport::new(
            closed.account_id(),
            instrument.id,
            PositionSide::Long,
            Quantity::from("0.75"),
            UnixNanos::from(200),
            UnixNanos::from(210),
            None,
            None,
            None,
        );

        drop(output);
        assert!(state.apply_position(report.clone(), &closed).is_err());
        assert_eq!(state.positions_ts[&instrument.id], UnixNanos::from(100));
        let (emitter, mut output) = emitter();
        state.apply_position(report.clone(), &emitter).unwrap();

        let ExecutionEvent::Report(ExecutionReport::Position(actual)) = output.try_recv().unwrap()
        else {
            panic!("expected current position report");
        };

        assert_eq!(*actual, report);
        assert_eq!(state.positions_ts[&instrument.id], UnixNanos::from(200));
        assert_eq!(
            state
                .preview(
                    &[],
                    &state.positions,
                    &[Settlement {
                        instrument_id: instrument.id,
                        value: Decimal::ONE,
                        ts_event: UnixNanos::from(150),
                        net: None
                    }],
                    "100.0001".parse().unwrap(),
                    &mut HashMap::new()
                )
                .unwrap_err()
                .to_string(),
            "settlement precedes collected execution evidence"
        );
        assert_eq!(
            state
                .settle(
                    &instrument,
                    Decimal::ONE,
                    UnixNanos::from(150),
                    UnixNanos::from(211),
                    &emitter
                )
                .unwrap_err()
                .to_string(),
            "settlement precedes collected execution evidence"
        );
        assert_eq!(state.positions[&instrument.id], Decimal::new(75, 2));
        assert_eq!(output.len(), 0);
    }

    #[rstest]
    fn authoritative_reduction_to_already_filled_quantity_closes_without_another_fill() {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, mut output) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();

        while output.try_recv().is_ok() {}
        let id = ClientOrderId::from("client-a");
        let total: Decimal = "99.550024".parse().unwrap();
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(
                ticket,
                total,
                HashMap::from([(ReservationId::Client(id), Decimal::new(5, 1))]),
            )
            .unwrap();
        source.status = VenueStatus::Executed;
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        source.last_update_time = Field::Value("2026-10-16T08:08:08.800000008Z".parse().unwrap());
        let plan = state
            .plan_order(
                &source,
                &[fill],
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        assert!(
            matches!(output.try_recv().unwrap(), ExecutionEvent::Order(OrderEventAny::Updated(event)) if event.quantity == Quantity::from("0.75") && event.reconciliation)
        );
        assert_eq!(output.len(), 0);
        assert!(state.orders[&id].order.is_closed());
        assert_eq!(
            state.orders.get(&id).unwrap().order.status(),
            OrderStatus::Filled
        );
        assert_eq!(
            state.orders.get(&id).unwrap().order.filled_qty(),
            Quantity::from("0.75")
        );
        assert_eq!(state.positions[&instrument.id], Decimal::new(75, 2));
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, total, HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), total);
        assert_eq!(balance.locked.as_decimal(), Decimal::ZERO);
        assert_eq!(balance.free.as_decimal(), total);
    }

    #[rstest]
    fn position_notice_counts_new_and_retained_plans_once() {
        let instrument = instrument();
        let owned = submitted(&instrument, OrderSide::Buy, "buy");
        let mut state = state(owned, &instrument);
        let (emitter, _output) = emitter();
        let mut plans = Vec::new();

        for (side, id) in [(OrderSide::Buy, "buy"), (OrderSide::Sell, "sell")] {
            let mut source = source(&instrument, side);
            source.client_order_id = id.into();
            source.order_id = format!("venue-{id}");
            source.status = VenueStatus::Executed;
            source.remaining_count_fp = FixedPoint(Decimal::ZERO);
            let mut fill = report(&source, &instrument, emitter.account_id());
            fill.trade_id = TradeId::from(id);
            fill.commission = Money::from("0 USD");

            if id == "buy" {
                let plan = state
                    .plan_order(
                        &source,
                        std::slice::from_ref(&fill),
                        &instrument,
                        &emitter,
                        UnixNanos::from(543),
                    )
                    .unwrap()
                    .unwrap();
                state.apply_plans(vec![plan], &emitter).unwrap();
            }

            plans.push(
                state
                    .plan_order(
                        &source,
                        &[fill],
                        &instrument,
                        &emitter,
                        UnixNanos::from(543),
                    )
                    .unwrap()
                    .unwrap(),
            );
        }

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::ZERO);
        position.realized_pnl_dollars = FixedPoint(Decimal::ZERO);
        position.fees_paid_dollars = FixedPoint(Decimal::ZERO);
        state.notices_positions.insert(
            instrument.id,
            PositionNotice {
                volume: Decimal::new(150, 2),
                economics: Some(PositionEconomics {
                    position_fp: position.position_fp,
                    realized_pnl_dollars: position.realized_pnl_dollars,
                    fees_paid_dollars: position.fees_paid_dollars,
                }),
            },
        );

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: vec![position],
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(543),
        };

        assert_eq!(
            state
                .check_notices(&snapshot, &plans[..1])
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
        state.check_notices(&snapshot, &plans).unwrap();
        assert_eq!(state.orders.len(), 1);
        state
            .notices_positions
            .get_mut(&instrument.id)
            .unwrap()
            .volume = Decimal::new(225, 2);
        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
        state
            .notices_positions
            .get_mut(&instrument.id)
            .unwrap()
            .volume = Decimal::new(150, 2);
        state.apply_plans(plans, &emitter).unwrap();
        state.check_notices(&snapshot, &[]).unwrap();
        state
            .notices_positions
            .get_mut(&instrument.id)
            .unwrap()
            .volume = Decimal::new(225, 2);
        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
    }

    #[rstest]
    fn fill_notice_requires_market_identity() {
        let mut state = PrivateState::new();
        let notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["fill_without_market"]
                .clone();
        let notice = serde_json::from_value(notice).unwrap();

        let error = state.notice(&notice).unwrap_err();

        assert_eq!(
            error.to_string(),
            "private fill notification lacks market identity"
        );
        assert_eq!(state.notices_fills.len(), 1);
        assert!(
            state.notices_fills[&TradeId::from("current-fill")]
                .instrument
                .is_none(),
            "malformed fill must retain identity without an instrument bypass"
        );
        assert_eq!(state.start, UnixNanos::from(1_792_112_523_000_000_000));
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST recovery does not yet cover received fill identities"
        );
    }

    #[rstest]
    fn explicit_fill_notice_covers_position_with_older_traded_volume() {
        let instrument = instrument();
        let owned = submitted(&instrument, OrderSide::Buy, "current");
        let mut state = state(owned, &instrument);
        let (emitter, _output) = emitter();
        let mut source = source(&instrument, OrderSide::Buy);
        source.client_order_id = "current".into();
        source.order_id = "venue-current".into();
        source.status = VenueStatus::Executed;
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut fill: Fill = serde_json::from_value(fixtures["fills"]["fills"][0].clone()).unwrap();
        fill.fill_id = "current-fill".into();
        fill.trade_id = "current-fill".into();
        fill.order_id = source.order_id.clone();
        fill.ticker = source.ticker.clone();
        fill.market_ticker = source.ticker.clone();
        fill.book_side = source.book_side;
        fill.outcome_side = source.outcome_side;
        fill.count_fp = source.fill_count_fp;
        fill.yes_price_dollars = source.yes_price_dollars;
        fill.no_price_dollars = source.no_price_dollars;
        fill.fee_cost = FixedPoint(Decimal::ZERO);
        let report = reports::fill(
            &fill,
            &instrument,
            emitter.account_id(),
            &source,
            UnixNanos::from(543),
        )
        .unwrap();
        let plan = state
            .plan_order(
                &source,
                &[report],
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();

        let mut fill_notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["fill"]
                .clone();
        fill_notice["msg"]["trade_id"] = serde_json::json!(fill.fill_id);
        fill_notice["msg"]["market_ticker"] = serde_json::json!(fill.ticker);
        let fill_notice = serde_json::from_value(fill_notice).unwrap();
        state.notice(&fill_notice).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::new(75, 2));
        position.realized_pnl_dollars = FixedPoint(Decimal::ZERO);
        position.fees_paid_dollars = FixedPoint(Decimal::ZERO);

        let mut position_notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_pending"]
                .clone();
        position_notice["msg"]["market_ticker"] = serde_json::json!(position.ticker);
        let position_notice = serde_json::from_value(position_notice).unwrap();
        state.notice(&position_notice).unwrap();

        let mut snapshot = super::super::KalshiExecutionSnapshot {
            orders: vec![source],
            fills: vec![fill],
            positions: vec![position],
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(543),
        };

        let plans = vec![plan];
        state.check_notices(&snapshot, &plans).unwrap();

        let mut future =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["fill_future"]
                .clone();
        future["msg"]["market_ticker"] = serde_json::json!(instrument.id.symbol.as_str());
        let future = serde_json::from_value(future).unwrap();
        state.notice(&future).unwrap();
        state.check_notices(&snapshot, &plans).unwrap();
        assert_eq!(
            state.notices_fills[&TradeId::from("current-fill")].ts_event,
            plans[0].source_ts
        );

        snapshot.positions[0].position_fp = FixedPoint(Decimal::ZERO);
        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST position does not yet cover notified economics"
        );

        snapshot.positions[0].position_fp = FixedPoint(Decimal::new(75, 2));
        let other = InstrumentId::from("OTHER.KALSHI");
        let mut other_position = snapshot.positions[0].clone();
        other_position.ticker = other.symbol.to_string();
        state.notices_positions.insert(
            other,
            PositionNotice {
                volume: Decimal::ONE,
                economics: Some(PositionEconomics {
                    position_fp: other_position.position_fp,
                    realized_pnl_dollars: other_position.realized_pnl_dollars,
                    fees_paid_dollars: other_position.fees_paid_dollars,
                }),
            },
        );

        snapshot.positions.push(other_position);
        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover notified traded volume"
        );

        let mut mismatched =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["fill"]
                .clone();
        mismatched["msg"]["market_ticker"] = serde_json::json!(other.symbol.as_str());
        let mismatched = serde_json::from_value(mismatched).unwrap();
        state.notice(&mismatched).unwrap();
        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover received fill identities"
        );

        state.notice(&fill_notice).unwrap();
        state.notices_positions.clear();
        state.clear_notices();
        state.notice(&fill_notice).unwrap();
        let position_updated = snapshot.positions[0].last_updated_ts;
        snapshot.positions[0].last_updated_ts = "2100-01-01T00:00:00Z".parse().unwrap();

        let (position_fp, realized_pnl, fees_paid) = (
            snapshot.positions[0].position_fp,
            snapshot.positions[0].realized_pnl_dollars,
            snapshot.positions[0].fees_paid_dollars,
        );

        let position_economics = || PositionEconomics {
            position_fp,
            realized_pnl_dollars: realized_pnl,
            fees_paid_dollars: fees_paid,
        };

        state.notices_positions.clear();
        state.notices_positions.insert(
            instrument.id,
            PositionNotice {
                volume: Decimal::new(300, 2),
                economics: Some(position_economics()),
            },
        );

        state.check_notices(&snapshot, &plans).unwrap();
        state.clear_notices();
        state.notice(&fill_notice).unwrap();
        state.notices_positions.clear();
        state.clear_notices();
        assert!(state.has_fill_notice(instrument.id));
        assert_eq!(
            state.covered_fills[&instrument.id],
            UnixNanos::from(1_792_112_523_000_000_000)
        );
        state.notices_positions.insert(
            instrument.id,
            PositionNotice {
                volume: Decimal::new(300, 2),
                economics: Some(position_economics()),
            },
        );

        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover notified traded volume"
        );

        snapshot.positions[0].last_updated_ts = position_updated;
        state.check_notices(&snapshot, &plans).unwrap();
        state.clear_notices();
        assert!(!state.has_fill_notice(instrument.id));

        state.notices_positions.insert(
            instrument.id,
            PositionNotice {
                volume: Decimal::new(300, 2),
                economics: Some(position_economics()),
            },
        );

        assert_eq!(
            state
                .check_notices(&snapshot, &plans)
                .unwrap_err()
                .to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
    }

    #[rstest]
    fn final_close_waits_for_each_offsetting_fill_in_the_canonical_cache() {
        use nautilus_common::{cache::Cache, live::sender::EventSender};
        use nautilus_model::{
            accounts::AccountAny, data::InstrumentClose, enums::InstrumentCloseType,
            events::AccountState,
        };

        use super::super::settlement::{Registration, publish_close};

        let instrument = instrument();
        let buy = submitted(&instrument, OrderSide::Buy, "buy");
        let sell = submitted(&instrument, OrderSide::Sell, "sell");
        let mut cache = Cache::default();
        cache.add_order(buy.clone(), None, None, false).unwrap();
        cache.add_order(sell.clone(), None, None, false).unwrap();
        let mut state = state(buy, &instrument);
        state.restore(sell);
        let (emitter, mut output) = emitter();
        let (sender, mut closes) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<_> = sender.into();
        let _registration = Registration::new(sender.clone(), &state.barrier);

        let mut plans = Vec::new();

        for (side, id, venue) in [
            (OrderSide::Buy, "buy", "venue-buy"),
            (OrderSide::Sell, "sell", "venue-sell"),
        ] {
            let mut source = source(&instrument, side);
            source.client_order_id = id.into();
            source.order_id = venue.into();
            source.status = VenueStatus::Canceled;
            source.remaining_count_fp = FixedPoint(Decimal::ZERO);
            let mut fill = report(&source, &instrument, emitter.account_id());
            fill.trade_id = TradeId::from(id);
            fill.commission = Money::from("0 USD");
            plans.push(
                state
                    .plan_order(
                        &source,
                        &[fill],
                        &instrument,
                        &emitter,
                        UnixNanos::from(543),
                    )
                    .unwrap()
                    .unwrap(),
            );
        }

        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_flat"]
                .clone();
        notice["msg"]["market_ticker"] = serde_json::json!(instrument.id.symbol.as_str());
        let notice = serde_json::from_value(notice).unwrap();
        state.notice(&notice).unwrap();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::ZERO);
        position.realized_pnl_dollars = FixedPoint(Decimal::ZERO);
        position.fees_paid_dollars = FixedPoint(Decimal::ZERO);

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: vec![position],
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
        state.check_notices(&snapshot, &plans).unwrap();
        state.apply_plans(plans, &emitter).unwrap();
        state.check_notices(&snapshot, &[]).unwrap();
        state.clear_notices();
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        assert!(
            state
                .orders
                .values()
                .all(|context| context.order.is_closed())
        );
        let ts_event = UnixNanos::from(u64::MAX - 1000);
        state
            .settle(&instrument, Decimal::ONE, ts_event, ts_event, &emitter)
            .unwrap();

        let close = InstrumentClose::new(
            instrument.id,
            Price::from("1.000000"),
            InstrumentCloseType::ContractExpired,
            ts_event,
            ts_event,
        );

        let mut account = AccountState::new(
            emitter.account_id(),
            AccountType::Cash,
            Vec::new(),
            Vec::new(),
            true,
            UUID4::new(),
            ts_event,
            ts_event,
            Some(Currency::USD()),
        );

        account.info = Some(nautilus_core::Params::new(
            [(
                "kalshi_session".into(),
                Value::String(state.session.clone()),
            )]
            .into(),
        ));
        cache.add_account(AccountAny::from(account)).unwrap();

        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        assert!(!publish_close(&sender, close).unwrap());

        while let Ok(ExecutionEvent::Order(event)) = output.try_recv() {
            cache.update_order(&event).unwrap();
            state
                .barrier
                .lock()
                .unwrap()
                .observe(&cache, emitter.account_id());

            if event.client_order_id().as_str() == "buy" {
                assert!(!publish_close(&sender, close).unwrap());
            }
        }

        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        assert!(publish_close(&sender, close).unwrap());
        assert!(
            matches!(closes.try_recv().unwrap(), DataEvent::Data(Data::InstrumentClose(actual)) if actual == close)
        );
        assert!(closes.try_recv().is_err());
        assert_eq!(
            cache
                .order_ref(&ClientOrderId::from("buy"))
                .unwrap()
                .trade_ids(),
            vec![&TradeId::from("buy")]
        );
        assert_eq!(
            cache
                .order_ref(&ClientOrderId::from("sell"))
                .unwrap()
                .trade_ids(),
            vec![&TradeId::from("sell")]
        );

        state.invalidate();
        assert!(!publish_close(&sender, close).unwrap());
        assert!(state.barrier.lock().unwrap().sealed(instrument.id));
        assert!(
            state
                .barrier
                .lock()
                .unwrap()
                .prepare(instrument.id, Decimal::ZERO, ts_event, Decimal::ZERO)
                .is_err()
        );
        assert!(state.barrier.lock().unwrap().is_blocked());
    }

    #[rstest]
    fn settled_context_rejects_unapplied_fill_replays_and_report_admission() {
        use nautilus_common::cache::Cache;
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut state = state(order.clone(), &instrument);
        let (emitter, mut output) = emitter();
        let mut fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let mut cache = Cache::default();
        cache.add_order(order, None, None, false).unwrap();
        while let Ok(ExecutionEvent::Order(event)) = output.try_recv() {
            cache.update_order(&event).unwrap();
        }

        let timestamp = UnixNanos::from(u64::MAX - 1000);
        state
            .settle(&instrument, Decimal::ONE, timestamp, timestamp, &emitter)
            .unwrap();
        let total = state
            .ledger
            .clone()
            .apply_refresh(
                state.ledger.begin_refresh(),
                "100.300024".parse().unwrap(),
                HashMap::new(),
            )
            .unwrap()
            .total;
        let prior_trade = fill.trade_id;
        fill.trade_id = TradeId::from("late-unapplied");
        fill.last_qty = Quantity::from("0.25");
        source.fill_count_fp = FixedPoint(Decimal::ONE);
        assert_eq!(
            state
                .plan_order(
                    &source,
                    std::slice::from_ref(&fill),
                    &instrument,
                    &emitter,
                    timestamp
                )
                .err()
                .unwrap()
                .to_string(),
            "unapplied execution evidence after settlement"
        );
        assert!(!state.fill_accounted(ClientOrderId::from("client-a"), fill.trade_id));
        assert!(state.fill_accounted(ClientOrderId::from("client-a"), prior_trade));
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        assert!(state.admit_reports(&cache, &[], &[fill]).is_err());
        assert!(state.barrier.lock().unwrap().is_blocked());
        assert_eq!(total.as_decimal(), "100.300024".parse::<Decimal>().unwrap());
    }

    #[rstest]
    #[case::absent(false)]
    #[case::restored_order_only(true)]
    fn missing_opening_evidence_is_fatal_even_when_order_restore_seeds_zero(
        #[case] restored_order: bool,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        if restored_order {
            state.restore(submitted(&instrument, OrderSide::Buy, "client-a"));
        }

        state.seed_positions([]).unwrap();

        let error = state
            .opening(instrument.id, Decimal::ONE, Decimal::ZERO)
            .unwrap_err();
        assert!(matches!(
            error.downcast_ref::<RecoveryContextError>(),
            Some(RecoveryContextError::Position)
        ));
        assert_eq!(
            state.positions.get(&instrument.id),
            restored_order.then_some(&Decimal::ZERO)
        );
    }

    #[rstest]
    fn retained_opening_mismatch_remains_retryable() {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state
            .seed_positions([(instrument.id, Decimal::ONE, UnixNanos::from(123))])
            .unwrap();

        let error = state
            .opening(instrument.id, Decimal::TWO, Decimal::ZERO)
            .unwrap_err();
        assert!(error.downcast_ref::<RecoveryContextError>().is_none());
        assert_eq!(
            error.to_string(),
            "position cycle lacks cached opening authority"
        );
        assert_eq!(state.positions[&instrument.id], Decimal::ONE);
        assert_eq!(state.positions_ts[&instrument.id], UnixNanos::from(123));
    }

    #[rstest]
    fn restored_unfilled_orders_seed_a_flat_cycle_and_settlement_requires_resolution() {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.restore(submitted(&instrument, OrderSide::Buy, "client-a"));
        state.seed_positions([]).unwrap();
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);

        let settlement = Settlement {
            instrument_id: instrument.id,
            value: Decimal::ONE,
            ts_event: UnixNanos::from(u64::MAX - 1000),
            net: None,
        };

        assert_eq!(
            state
                .check_settlements(&[], &[settlement])
                .unwrap_err()
                .to_string(),
            "settlement has unresolved orders"
        );
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn final_report_admission_fences_a_close_and_rejects_a_close_that_won_the_race(
        #[case] cached_close: bool,
    ) {
        use nautilus_common::cache::Cache;
        let instrument = instrument();
        let source = source(&instrument, OrderSide::Buy);
        let order = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut cache = Cache::default();
        cache.add_order(order.clone(), None, None, false).unwrap();
        let mut state = state(order, &instrument);
        let (emitter, _) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let session = state.session.clone();
        let ticket = state.ledger.begin_refresh();
        state
            .admit_reports(&cache, &[], std::slice::from_ref(&fill))
            .unwrap();
        assert_ne!(state.session, session);
        assert!(state.ledger.check_refresh(ticket).is_err());
        assert!(!state.fill_accounted(ClientOrderId::from("client-a"), fill.trade_id));

        if cached_close {
            cache
                .add_instrument_close(InstrumentClose::new(
                    instrument.id,
                    Price::from("1.000000"),
                    InstrumentCloseType::ContractExpired,
                    UnixNanos::from(u64::MAX - 1000),
                    UnixNanos::from(u64::MAX - 999),
                ))
                .unwrap();
        } else {
            state
                .barrier
                .lock()
                .unwrap()
                .prepare(
                    instrument.id,
                    Decimal::ONE,
                    UnixNanos::from(u64::MAX - 1000),
                    Decimal::ZERO,
                )
                .unwrap();
        }

        assert_eq!(
            state
                .admit_reports(&cache, &[], std::slice::from_ref(&fill))
                .unwrap_err()
                .to_string(),
            "unapplied fill report after settlement"
        );
        assert!(!state.fill_accounted(ClientOrderId::from("client-a"), fill.trade_id));
        assert!(state.barrier.lock().unwrap().is_blocked());
    }

    #[rstest]
    fn terminal_ownership_survives_more_than_ten_thousand_unobserved_completions() {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, Decimal::ZERO);
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, Decimal::from(100_000), HashMap::new())
            .unwrap();
        let (emitter, _output) = emitter();
        let original = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut first = source(&instrument, OrderSide::Buy);
        first.status = VenueStatus::Executed;
        first.fill_count_fp = FixedPoint(Decimal::from(2));
        first.remaining_count_fp = FixedPoint(Decimal::ZERO);
        for index in 0..10_001 {
            let mut current = first.clone();

            let id = if index == 0 {
                "client-a".to_string()
            } else {
                format!("completed-{index}")
            };

            current.client_order_id = id.clone();

            current.order_id = if index == 0 {
                first.order_id.clone()
            } else {
                format!("venue-{index}")
            };

            state.restore(submitted(&instrument, OrderSide::Buy, &id));
            let mut fill = report(&current, &instrument, emitter.account_id());
            fill.trade_id = TradeId::new(format!("completed-fill-{index}"));
            let plan = state
                .plan_order(
                    &current,
                    &[fill],
                    &instrument,
                    &emitter,
                    UnixNanos::from(543),
                )
                .unwrap()
                .unwrap();
            state.apply_plans(vec![plan], &emitter).unwrap();
        }

        state.restore(original);
        let retained = state.source_order(&first).unwrap().unwrap();
        assert_eq!(retained.client_order_id(), ClientOrderId::from("client-a"));
        assert_eq!(retained.status(), OrderStatus::Filled);
        assert_eq!(retained.filled_qty(), Quantity::from("2.00"));
        assert_eq!(
            retained.trade_ids(),
            vec![&TradeId::from("completed-fill-0")]
        );
        assert_eq!(state.positions[&instrument.id], Decimal::from(20_002));
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, "87996.789799".parse().unwrap(), HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), "87996.789799".parse().unwrap());
    }

    #[rstest]
    fn recovery_window_waits_for_canonical_receipts_and_reopens_for_old_notices() {
        use nautilus_common::cache::Cache;

        let instrument = instrument();
        let source = source(&instrument, OrderSide::Buy);
        let original = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut state = state(original.clone(), &instrument);
        let (emitter, mut output) = emitter();
        let fill = report(&source, &instrument, emitter.account_id());
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let next = UnixNanos::from(u64::MAX - 1000);
        state.advance_recovery(next);
        assert_eq!(state.start, original.ts_init());
        let mut cache = Cache::default();
        cache
            .add_order(original.clone(), None, None, false)
            .unwrap();
        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        state.advance_recovery(next);
        assert_eq!(state.start, original.ts_init());

        while let Ok(event) = output.try_recv() {
            let ExecutionEvent::Order(event) = event else {
                panic!("expected order event");
            };

            cache.update_order(&event).unwrap();
        }

        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        state.advance_recovery(next);
        assert_eq!(
            state.start, next,
            "an observed resting GTC order must not pin broad history"
        );

        if let Some(plan) = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                UnixNanos::from(544),
            )
            .unwrap()
        {
            state.apply_plans(vec![plan], &emitter).unwrap();
        }

        assert!(output.try_recv().is_err());
        state.advance_recovery(next);
        assert_eq!(
            state.start, next,
            "a no-op replay must not recreate a receipt"
        );
        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_partial_payload"]
                .clone();
        notice["order_id"] = serde_json::json!(source.order_id);
        state.notice_order(&notice).unwrap();
        assert_eq!(
            state.start,
            original.ts_init(),
            "a replayed notice must reopen order coverage even without another fill"
        );
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: vec![source],
            fills: Vec::new(),
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: next,
        };

        state.check_notices(&snapshot, &[]).unwrap();
    }

    #[rstest]
    #[case(0, "REST recovery lacks a notified order")]
    #[case(1, "REST order does not yet cover notified fills")]
    #[case(2, "REST order does not yet cover notified update time")]
    #[case(3, "")]
    fn received_order_evidence_waits_for_rest_identity_quantity_and_time(
        #[case] evidence: u8,
        #[case] error: &str,
    ) {
        let instrument = instrument();
        let mut source = source(&instrument, OrderSide::Buy);
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let updated = timestamp(&source.last_update_time).unwrap();
        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_updated_payload"]
                .clone();
        notice["order_id"] = serde_json::json!(source.order_id);
        notice["last_updated_ts_ms"] = serde_json::json!(updated.as_u64() / 1_000_000 + 1);
        state.notice_order(&notice).unwrap();

        if evidence == 1 {
            source.fill_count_fp = FixedPoint(Decimal::ZERO);
        }

        if evidence == 3 {
            let time =
                jiff::Timestamp::from_nanosecond(i128::from(updated.as_u64()) + 1_000_000).unwrap();
            source.last_update_time = Field::Value(time);
        }

        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: if evidence == 0 {
                Vec::new()
            } else {
                vec![source]
            },
            fills: Vec::new(),
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: updated,
        };

        let result = state.check_notices(&snapshot, &[]);
        if error.is_empty() {
            result.unwrap();
        } else {
            assert_eq!(result.unwrap_err().to_string(), error);
        }
    }

    #[rstest]
    #[case::external(true)]
    #[case::same_text_client(false)]
    fn held_external_order_notices_preserve_the_fill_window(#[case] external: bool) {
        let mut state = PrivateState::new();
        state.start = UnixNanos::from(20_000_000_000);
        let id = VenueOrderId::from("old-order");
        let reservation = if external {
            ReservationId::Venue(id)
        } else {
            ReservationId::Client(ClientOrderId::from(id.as_str()))
        };

        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(
                ticket,
                Decimal::ONE,
                HashMap::from([(reservation, Decimal::new(1, 1))]),
            )
            .unwrap();
        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["order_created_payload"]
                .clone();
        notice["order_id"] = serde_json::json!(id.as_str());
        state.notice_order(&notice).unwrap();
        assert_eq!(
            state.start,
            UnixNanos::from(if external {
                20_000_000_000
            } else {
                1_000_000_000
            })
        );
        assert_eq!(
            state.external_order_notices(),
            if external { vec![id] } else { Vec::new() }
        );
    }

    #[rstest]
    #[case(false, true)]
    #[case(true, true)]
    #[case(true, false)]
    fn admitted_historical_reports_reopen_coverage_until_canonical_consumption(
        #[case] include_order: bool,
        #[case] include_fill: bool,
    ) {
        use nautilus_common::cache::Cache;

        let instrument = instrument();
        let source = source(&instrument, OrderSide::Buy);
        let (emitter, mut output) = emitter();
        let mut original = submitted(&instrument, OrderSide::Buy, "client-a");
        let accepted = timestamp(&source.created_time).unwrap();

        let event = OrderAccepted::new(
            original.trader_id(),
            original.strategy_id(),
            instrument.id,
            original.client_order_id(),
            source.order_id.as_str().into(),
            emitter.account_id(),
            UUID4::new(),
            accepted,
            accepted,
            false,
        );

        original.apply(OrderEventAny::Accepted(event)).unwrap();
        let mut cache = Cache::default();
        cache
            .add_order(original.clone(), None, None, false)
            .unwrap();
        let mut state = state(original.clone(), &instrument);
        let next = UnixNanos::from(u64::MAX - 1000);
        state.start = next;
        let fill = report(&source, &instrument, emitter.account_id());

        let orders = if include_order {
            vec![
                reports::order(
                    &source,
                    &instrument,
                    emitter.account_id(),
                    Some(&original),
                    next,
                )
                .unwrap()
                .unwrap(),
            ]
        } else {
            Vec::new()
        };

        state
            .admit_reports(
                &cache,
                &orders,
                if include_fill {
                    std::slice::from_ref(&fill)
                } else {
                    &[]
                },
            )
            .unwrap();

        assert_eq!(state.start, accepted.min(fill.ts_event));
        assert!(!state.fill_accounted(original.client_order_id(), fill.trade_id));
        assert_eq!(state.positions[&instrument.id], Decimal::ZERO);
        state.advance_recovery(next);
        assert_eq!(state.start, accepted.min(fill.ts_event));
        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        state.advance_recovery(next);
        assert_eq!(state.start, accepted.min(fill.ts_event));
        let plan = state
            .plan_order(
                &source,
                std::slice::from_ref(&fill),
                &instrument,
                &emitter,
                next,
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();

        while let Ok(event) = output.try_recv() {
            let ExecutionEvent::Order(event) = event else {
                panic!("expected order event");
            };

            cache.update_order(&event).unwrap();
        }

        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        state.advance_recovery(next);
        assert_eq!(state.start, next);
        assert_eq!(state.positions[&instrument.id], Decimal::new(75, 2));
        assert!(state.fill_accounted(original.client_order_id(), fill.trade_id));
        let expected = Decimal::from_str_exact("99.550024").unwrap();
        let ticket = state.ledger.begin_refresh();
        let balance = state
            .ledger
            .apply_refresh(ticket, expected, HashMap::new())
            .unwrap();
        assert_eq!(balance.total.as_decimal(), expected);
    }

    #[rstest]
    #[case("missing", false)]
    #[case("net", false)]
    #[case("pnl", false)]
    #[case("fees", false)]
    #[case("covered", false)]
    #[case("covered", true)]
    fn position_notifications_require_matching_rest_economics(
        #[case] evidence: &str,
        #[case] settled: bool,
    ) {
        let instrument = instrument();
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::new(75, 2));
        position.realized_pnl_dollars = FixedPoint(Decimal::new(-37, 4));
        position.fees_paid_dollars = FixedPoint(Decimal::new(23, 4));

        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_economics"]
                .clone();
        notice["msg"]["market_ticker"] = serde_json::json!(position.ticker);
        let notice = serde_json::from_value(notice).unwrap();
        state.advance_recovery(UnixNanos::from(u64::MAX - 1000));
        state.notice(&notice).unwrap();
        state.invalidate();
        assert_eq!(state.start, UnixNanos::from(0));

        match evidence {
            "net" => position.position_fp = FixedPoint(Decimal::ZERO),
            "pnl" => position.realized_pnl_dollars = FixedPoint(Decimal::ZERO),
            "fees" => position.fees_paid_dollars = FixedPoint(Decimal::ZERO),
            _ => {}
        }

        let positions = if evidence == "missing" {
            Vec::new()
        } else {
            vec![position]
        };

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: if settled {
                Vec::new()
            } else {
                positions.clone()
            },
            positions_settled: if settled { positions } else { Vec::new() },
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        let result = state.check_notices(&snapshot, &[]);
        if evidence == "covered" {
            result.unwrap();
        } else {
            assert!(
                result.is_err(),
                "stale {evidence} evidence covered a position notification"
            );
        }

        state.clear_notices();
        state.check_notices(&snapshot, &[]).unwrap();
    }

    #[rstest]
    fn position_only_notice_keeps_a_flat_final_close_withheld() {
        use nautilus_common::{cache::Cache, live::sender::EventSender};
        use nautilus_model::{
            accounts::AccountAny, data::InstrumentClose, enums::InstrumentCloseType,
            events::AccountState,
        };

        use super::super::settlement::{Registration, publish_close};
        let instrument = instrument();
        let original = submitted(&instrument, OrderSide::Buy, "client-a");
        let mut state = state(original.clone(), &instrument);
        let (emitter, mut output) = emitter();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let plan = state
            .plan_order(&source, &[], &instrument, &emitter, UnixNanos::from(543))
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let mut cache = Cache::default();
        cache.add_order(original, None, None, false).unwrap();
        while let Ok(ExecutionEvent::Order(event)) = output.try_recv() {
            cache.update_order(&event).unwrap();
        }

        assert!(
            state.orders[&ClientOrderId::from("client-a")]
                .order
                .is_closed()
        );
        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        let (sender, mut closes) = tokio::sync::mpsc::unbounded_channel();
        let sender: EventSender<_> = sender.into();
        let _registration = Registration::new(sender.clone(), &state.barrier);
        let next = UnixNanos::from(u64::MAX - 1000);
        state.advance_recovery(next);
        assert_eq!(state.start, next);

        let mut notice =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_economics"]
                .clone();
        notice["msg"]["market_ticker"] = serde_json::json!(instrument.id.symbol.as_str());
        let notice = serde_json::from_value(notice).unwrap();
        state.notice(&notice).unwrap();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: vec![source],
            fills: Vec::new(),
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: next,
        };

        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST recovery lacks a notified position"
        );
        assert_eq!(state.start, next);

        let close = InstrumentClose::new(
            instrument.id,
            Price::from("1.000000"),
            InstrumentCloseType::ContractExpired,
            next,
            next,
        );

        let account = AccountState::new(
            emitter.account_id(),
            AccountType::Cash,
            Vec::new(),
            Vec::new(),
            true,
            UUID4::new(),
            next,
            next,
            Some(Currency::USD()),
        );

        cache.add_account(AccountAny::from(account)).unwrap();
        state
            .barrier
            .lock()
            .unwrap()
            .observe(&cache, emitter.account_id());
        assert!(!publish_close(&sender, close).unwrap());
        assert!(closes.try_recv().is_err());
    }

    #[rstest]
    fn malformed_position_economics_remain_pending_until_valid_replay() {
        let instrument = instrument();
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::ZERO);
        position.realized_pnl_dollars = FixedPoint(Decimal::new(-42000, 6));
        position.fees_paid_dollars = FixedPoint(Decimal::new(1700, 6));

        let mut message =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_malformed_fees"]
                .clone();
        message["msg"]["market_ticker"] = serde_json::json!(position.ticker);
        message["msg"]["position_fp"] = serde_json::json!(position.position_fp);
        message["msg"]["realized_pnl_dollars"] = serde_json::json!(FixedPoint(
            position.realized_pnl_dollars.0 - position.fees_paid_dollars.0
        ));

        assert!(
            state
                .notice(&serde_json::from_value(message.clone()).unwrap())
                .is_err()
        );
        state.invalidate();

        let mut snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: vec![position.clone()],
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "private position notification remains malformed"
        );
        assert_eq!(state.recovery_targets(), vec![instrument.id]);
        message["msg"]["fees_paid_dollars"] =
            serde_json::to_value(position.fees_paid_dollars).unwrap();
        state
            .notice(&serde_json::from_value(message.clone()).unwrap())
            .unwrap();
        state.check_notices(&snapshot, &[]).unwrap();
        message["msg"]["position_fp"] = "-0.37".into();
        state
            .notice(&serde_json::from_value(message).unwrap())
            .unwrap();
        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST position does not yet cover notified economics"
        );
        snapshot.positions[0].position_fp = FixedPoint(Decimal::new(-37, 2));
        state.check_notices(&snapshot, &[]).unwrap();
        state.clear_notices();
        assert!(state.recovery_targets().is_empty());
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn position_volume_requirement_survives_older_and_malformed_notices(#[case] malformed: bool) {
        let instrument = instrument();
        let mut state = state(
            submitted(&instrument, OrderSide::Buy, "client-a"),
            &instrument,
        );
        let (emitter, _output) = emitter();
        let mut source = source(&instrument, OrderSide::Buy);
        source.fill_count_fp = FixedPoint(Decimal::new(150, 2));
        source.remaining_count_fp = FixedPoint(Decimal::new(50, 2));
        let mut fill = report(&source, &instrument, emitter.account_id());
        fill.commission = Money::from("0 USD");
        let plan = state
            .plan_order(
                &source,
                &[fill],
                &instrument,
                &emitter,
                UnixNanos::from(543),
            )
            .unwrap()
            .unwrap();
        state.apply_plans(vec![plan], &emitter).unwrap();
        let fixtures: Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = instrument.id.symbol.to_string();
        position.position_fp = FixedPoint(Decimal::new(150, 2));
        position.realized_pnl_dollars = FixedPoint(Decimal::ZERO);
        position.fees_paid_dollars = FixedPoint(Decimal::ZERO);

        let mut message =
            serde_json::from_str::<Value>(include_str!("../../test_data/websocket_messages.json"))
                .unwrap()["position_volume"]
                .clone();
        message["msg"]["market_ticker"] = serde_json::json!(position.ticker);
        message["msg"]["fees_paid_dollars"] =
            serde_json::json!(if malformed { "malformed" } else { "0.000000" });

        let result = state.notice(&serde_json::from_value(message.clone()).unwrap());
        assert_eq!(result.is_err(), malformed);
        state.invalidate();
        message["msg"]["volume_fp"] = "1.50".into();
        message["msg"]["fees_paid_dollars"] = "0.000000".into();
        state
            .notice(&serde_json::from_value(message).unwrap())
            .unwrap();

        let snapshot = super::super::KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: vec![position],
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: Some(state.start),
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        assert_eq!(
            state.check_notices(&snapshot, &[]).unwrap_err().to_string(),
            "REST recovery does not yet cover notified traded volume"
        );
    }

    #[rstest]
    #[case("2.50", OrderSide::Sell, "1.25", "0", true)]
    #[case("-2.50", OrderSide::Buy, "1.25", "0", true)]
    #[case("2.50", OrderSide::Buy, "1.25", "0", false)]
    #[case("-2.50", OrderSide::Sell, "1.25", "0", false)]
    #[case("0", OrderSide::Sell, "1.25", "0", false)]
    #[case("2.50", OrderSide::Sell, "2.51", "0", false)]
    #[case("2.50", OrderSide::Sell, "1.25", "1.25", false)]
    #[case("2.50", OrderSide::Sell, "1.26", "1.25", false)]
    fn reduction_uses_account_net_and_external_remaining_quantity(
        #[case] net: &str,
        #[case] side: OrderSide,
        #[case] qty: &str,
        #[case] external: &str,
        #[case] admitted: bool,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, net.parse().unwrap());
        state
            .external_leaves
            .insert((instrument.id, side), external.parse().unwrap());
        let other = InstrumentId::from("OTHER.KALSHI");
        state
            .external_leaves
            .insert((other, side), Decimal::from(100));
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .strategy_id("REDUCE-002".into())
            .side(side)
            .quantity(Quantity::from(qty))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        assert_eq!(
            state
                .check_reduction(&order, Some(net.parse().unwrap()))
                .is_ok(),
            admitted
        );
    }

    #[rstest]
    #[case(OrderSide::Sell, "2.50")]
    #[case(OrderSide::Buy, "-2.50")]
    fn concurrent_reductions_retain_quantity_until_definitive_rejection(
        #[case] side: OrderSide,
        #[case] net: &str,
        #[values(false, true)] same_strategy: bool,
    ) {
        let instrument = instrument();
        let position_qty: Decimal = net.parse().unwrap();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, net.parse().unwrap());
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, Decimal::from(10), HashMap::new())
            .unwrap();
        let state = Arc::new(Mutex::new(state));
        let barrier = Arc::new(std::sync::Barrier::new(3));
        let (emitter, mut output) = emitter();

        #[allow(
            clippy::needless_collect,
            reason = "both workers must start before the admission barrier"
        )]
        let threads: Vec<_> = (0..2)
            .map(|i| {
                let state = Arc::clone(&state);
                let barrier = Arc::clone(&barrier);
                let emitter = emitter.clone();

                let mut order = OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(instrument.id)
                    .strategy_id(if same_strategy {
                        "REDUCE-001".into()
                    } else {
                        format!("REDUCE-00{i}").into()
                    })
                    .client_order_id(format!("reduction-{i}").into())
                    .quantity(Quantity::from("1.50"))
                    .price(Price::from("0.600000"))
                    .side(side)
                    .time_in_force(TimeInForce::Ioc)
                    .reduce_only(true)
                    .build();

                std::thread::spawn(move || {
                    barrier.wait();
                    let result = state.lock().unwrap().submit(
                        &mut order,
                        Decimal::ONE,
                        Some(position_qty),
                        &emitter,
                    );
                    (order, result.is_ok())
                })
            })
            .collect();

        barrier.wait();
        let outcomes: Vec<_> = threads
            .into_iter()
            .map(|thread| thread.join().unwrap())
            .collect();
        let mut state = state.lock().unwrap();
        let successful = outcomes
            .iter()
            .find(|(_, success)| *success)
            .unwrap()
            .0
            .clone();
        let mut denied = outcomes
            .iter()
            .find(|(_, success)| !*success)
            .unwrap()
            .0
            .clone();
        assert_eq!(outcomes.iter().filter(|(_, success)| *success).count(), 1);
        assert_eq!(state.orders.len(), 1);
        assert_eq!(
            state.ledger.balance().unwrap().locked.as_decimal(),
            Decimal::ONE
        );
        assert_eq!(denied.status(), OrderStatus::Initialized);
        assert!(state.check_reduction(&denied, Some(position_qty)).is_err());
        state
            .reject_submission(
                successful.client_order_id(),
                "definitive venue rejection",
                false,
                &emitter,
            )
            .unwrap();
        state
            .submit(&mut denied, Decimal::ONE, Some(position_qty), &emitter)
            .unwrap();
        assert_eq!(
            state.ledger.balance().unwrap().locked.as_decimal(),
            Decimal::ONE
        );
        assert_eq!(
            state.positions[&instrument.id],
            net.parse::<Decimal>().unwrap()
        );
        let mut events = Vec::new();

        while let Ok(event) = output.try_recv() {
            if let ExecutionEvent::Order(event) = event {
                events.push(event);
            }
        }

        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Submitted(_)))
                .count(),
            2
        );
        assert_eq!(
            events
                .iter()
                .filter(|event| matches!(event, OrderEventAny::Rejected(_)))
                .count(),
            1
        );
    }

    #[rstest]
    fn immediate_terminal_evidence_precedes_local_submission_and_ack(
        #[values(false, true)] ack_first: bool,
        #[values(TimeInForce::Ioc, TimeInForce::Fok)] tif: TimeInForce,
    ) {
        let instrument = instrument();
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id("client-a".into())
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.600100"))
            .time_in_force(tif)
            .build();
        let mut state = PrivateState::new();
        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, Decimal::from(10), HashMap::new())
            .unwrap();
        let (emitter, mut output) = emitter();
        state
            .submit(&mut order, Decimal::ONE, None, &emitter)
            .unwrap();
        let mut source = source(&instrument, OrderSide::Buy);
        source.status = VenueStatus::Canceled;
        source.fill_count_fp = FixedPoint(Decimal::ZERO);
        source.remaining_count_fp = FixedPoint(Decimal::ZERO);
        let before_submit =
            jiff::Timestamp::from_nanosecond(i128::from(order.ts_last().as_u64() - 2_000_000_000))
                .unwrap();
        source.created_time = Field::Value(before_submit);
        source.last_update_time = Field::Value(before_submit);
        let venue_id = VenueOrderId::from(source.order_id.as_str());

        if ack_first {
            state
                .acknowledge(
                    order.client_order_id(),
                    venue_id,
                    UnixNanos::from(order.ts_last().as_u64() + 1_000_000_000),
                    &emitter,
                )
                .unwrap();
        }

        let plan = state
            .plan_order(&source, &[], &instrument, &emitter, order.ts_last())
            .unwrap();
        assert!(
            plan.is_some(),
            "terminal venue evidence was discarded as stale"
        );
        state.apply_plans(vec![plan.unwrap()], &emitter).unwrap();
        state
            .acknowledge(order.client_order_id(), venue_id, order.ts_last(), &emitter)
            .unwrap();
        assert_eq!(
            state.orders[&order.client_order_id()].order.status(),
            OrderStatus::Canceled
        );
        assert_eq!(
            state.ledger.balance().unwrap().locked.as_decimal(),
            Decimal::ZERO
        );
        let mut events = Vec::new();

        while let Ok(event) = output.try_recv() {
            if let ExecutionEvent::Order(event) = event {
                events.push(event);
            }
        }

        assert_eq!(events.len(), 3);

        let OrderEventAny::Canceled(canceled) = &events[2] else {
            panic!("expected Canceled")
        };

        assert_eq!(
            canceled.ts_event,
            timestamp(&source.last_update_time).unwrap()
        );
    }

    #[rstest]
    #[case(
        None,
        "Kalshi reduce-only requires a strategy-owned canonical position"
    )]
    #[case(
        Some("-2.00"),
        "Kalshi reduce-only side does not reduce the canonical position"
    )]
    #[case(
        Some("1.00"),
        "Kalshi reduce-only quantity exceeds the canonical position"
    )]
    fn canonical_reduction_capacity_is_independent_of_account_capacity(
        #[case] canonical: Option<&str>,
        #[case] reason: &str,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, Decimal::from(5));
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        let error = state
            .check_reduction(&order, canonical.map(|qty| qty.parse().unwrap()))
            .unwrap_err();
        assert_eq!(error.to_string(), reason);
    }

    #[rstest]
    #[case(false)]
    #[case(true)]
    fn externally_claimable_orders_cannot_consume_canonical_reduction_capacity(
        #[case] tracked: bool,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, Decimal::from(5));

        if tracked {
            let pending = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(instrument.id)
                .client_order_id("external-pending".into())
                .strategy_id(StrategyId::external())
                .side(OrderSide::Sell)
                .quantity(Quantity::from("1.00"))
                .price(Price::from("0.600000"))
                .build();
            state.restore(pending);
        } else {
            state
                .external_leaves
                .insert((instrument.id, OrderSide::Sell), Decimal::ONE);
        }

        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .strategy_id("OWNER-001".into())
            .side(OrderSide::Sell)
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(true)
            .build();
        let error = state
            .check_reduction(&order, Some(Decimal::from(2)))
            .unwrap_err();
        assert_eq!(
            error.to_string(),
            "Kalshi reduce-only requires resolved external orders on its reducing side"
        );
    }

    #[rstest]
    #[case(false, true)]
    #[case(true, false)]
    fn unresolved_strategy_order_excludes_competing_reduction_flow(
        #[case] pending_reduce: bool,
        #[case] next_reduce: bool,
    ) {
        let instrument = instrument();
        let mut state = PrivateState::new();
        state.positions.insert(instrument.id, Decimal::from(5));
        let pending = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id("flow-pending".into())
            .quantity(Quantity::from("0.25"))
            .price(Price::from("0.600000"))
            .side(OrderSide::Sell)
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(pending_reduce)
            .build();
        let next = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id)
            .client_order_id("flow-next".into())
            .quantity(Quantity::from("0.25"))
            .price(Price::from("0.600000"))
            .side(OrderSide::Buy)
            .time_in_force(TimeInForce::Ioc)
            .reduce_only(next_reduce)
            .build();
        state.restore(pending);
        assert_eq!(
            state.check_position_flow(&next).unwrap_err().to_string(),
            "Kalshi reduce-only requires exclusive strategy-instrument order flow"
        );
    }

    #[rstest]
    fn late_mutation_rejection_preserves_the_current_command() {
        let definition = instrument();
        let order = submitted(&definition, OrderSide::Buy, "change-correlation");
        let mut private = state(order.clone(), &definition);
        let (emitter, mut output) = emitter();
        private
            .acknowledge(
                order.client_order_id(),
                "venue-a".into(),
                UnixNanos::from(3),
                &emitter,
            )
            .unwrap();
        output.try_recv().unwrap();
        let current = private.orders[&order.client_order_id()].order.clone();

        let old = CancelOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            current.venue_order_id(),
            UUID4::new(),
            UnixNanos::from(4),
            None,
            None,
        );

        let mut next = old.clone();
        next.command_id = UUID4::new();
        private.mutations.insert(
            order.client_order_id(),
            PendingMutation {
                command_id: next.command_id,
                started: UnixNanos::from(5),
                dispatched: true,
                acknowledgement: None,
                terms: None,
            },
        );

        private
            .resolve_mutation(
                &Command::Cancel(old),
                &current,
                "Old failure",
                false,
                &emitter,
            )
            .unwrap();
        assert!(output.try_recv().is_err());
        assert_eq!(private.orders[&order.client_order_id()].order, current);
        assert_eq!(
            private.mutations[&order.client_order_id()].command_id,
            next.command_id
        );
        private
            .resolve_mutation(
                &Command::Cancel(next.clone()),
                &current,
                "Invalid parameters",
                false,
                &emitter,
            )
            .unwrap();

        let ExecutionEvent::Order(OrderEventAny::CancelRejected(event)) =
            output.try_recv().unwrap()
        else {
            panic!("expected cancel rejection")
        };

        assert_eq!(event.causation_id, Some(next.command_id));
        assert_eq!(event.trader_id, current.trader_id());
        assert_eq!(event.strategy_id, current.strategy_id());
        assert_eq!(event.instrument_id, current.instrument_id());
        assert_eq!(event.client_order_id, current.client_order_id());
        assert_eq!(event.venue_order_id, current.venue_order_id());
        assert_eq!(event.account_id, Some(emitter.account_id()));
        assert_eq!(event.reason.as_str(), "Invalid parameters");
        assert_eq!(
            private.orders[&order.client_order_id()].order.status(),
            OrderStatus::Accepted
        );
        assert!(!private.mutations.contains_key(&order.client_order_id()));
    }

    #[rstest]
    fn mutation_outcome_preserves_venue_freshness(#[values(false, true)] denied: bool) {
        let definition = instrument();
        let order = submitted(&definition, OrderSide::Buy, "change-freshness");
        let mut private = state(order.clone(), &definition);
        let (emitter, mut output) = emitter();
        let id = order.client_order_id();
        private.ledger.reserve(id, Decimal::new(12, 1)).unwrap();
        private
            .acknowledge(id, "venue-a".into(), UnixNanos::from(3), &emitter)
            .unwrap();
        output.try_recv().unwrap();
        let accepted = private.orders[&id].order.clone();

        let command_id = UUID4::new();

        let mut pending = OrderPendingCancel::new(
            order.trader_id(),
            order.strategy_id(),
            definition.id,
            id,
            Some(emitter.account_id()),
            UUID4::new(),
            UnixNanos::from(20),
            UnixNanos::from(20),
            false,
            accepted.venue_order_id(),
        );

        pending.causation_id = Some(command_id);
        private
            .orders
            .get_mut(&id)
            .unwrap()
            .order
            .apply(OrderEventAny::PendingCancel(pending))
            .unwrap();
        let command = Command::Cancel(CancelOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            definition.id,
            id,
            accepted.venue_order_id(),
            command_id,
            UnixNanos::from(20),
            None,
            None,
        ));
        private.mutations.insert(
            id,
            PendingMutation {
                command_id: command.id(),
                started: UnixNanos::from(20),
                dispatched: false,
                acknowledgement: None,
                terms: None,
            },
        );

        private
            .resolve_mutation(&command, &accepted, "Not ready", denied, &emitter)
            .unwrap();

        let ExecutionEvent::Order(event) = output.try_recv().unwrap() else {
            panic!("expected rejection")
        };

        let mut cached = accepted;
        cached.apply(event.clone()).unwrap();
        private.restore(cached);
        let mut venue = source(&definition, OrderSide::Buy);
        venue.order_id = "venue-a".into();
        venue.client_order_id = id.to_string();
        venue.status = VenueStatus::Canceled;
        venue.fill_count_fp = FixedPoint(Decimal::ZERO);
        venue.remaining_count_fp = FixedPoint(Decimal::ZERO);
        venue.yes_price_dollars = FixedPoint(Decimal::new(6, 1));
        venue.no_price_dollars = FixedPoint(Decimal::new(4, 1));
        venue.created_time = Field::Value(jiff::Timestamp::from_nanosecond(2).unwrap());
        venue.last_update_time = Field::Value(jiff::Timestamp::from_nanosecond(4).unwrap());
        let plan = private
            .plan_order(&venue, &[], &definition, &emitter, UnixNanos::from(30))
            .unwrap();
        assert_eq!(event.ts_event(), UnixNanos::from(3));
        assert_eq!(private.orders[&id].order.ts_last(), UnixNanos::from(3));
        assert!(
            plan.is_some(),
            "venue cancellation was discarded after rejection"
        );
        private.apply_plans(vec![plan.unwrap()], &emitter).unwrap();
        assert_eq!(private.orders[&id].order.status(), OrderStatus::Canceled);
        assert_eq!(
            private.ledger.balance().unwrap().locked.as_decimal(),
            Decimal::ZERO
        );
    }
}
