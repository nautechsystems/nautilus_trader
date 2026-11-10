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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use nautilus_common::{
    live::{dst::time, sender::EventSender},
    messages::DataEvent,
};
use nautilus_core::UnixNanos;
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::OrderSide,
    identifiers::{InstrumentId, VenueOrderId},
    instruments::{BinaryOption, InstrumentAny},
    orders::Order,
};
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{
    KalshiExecutionSnapshot,
    account::ReservationId,
    collect_recovery_snapshot,
    context::{OrderPlan, PrivateState, RecoveryContextError, Settlement},
    load_definition, reports, timestamp,
};
use crate::{
    common::enums::MarketStatus,
    config::KalshiExecutionClientConfig,
    http::{
        client::KalshiHttpClient,
        models::{Balance, Field, OrderStatus as VenueStatus},
        query::{AccountQuery, SettlementsQuery},
    },
    websocket::{
        client::{KalshiWebSocketClient, KalshiWsEvent},
        messages::KalshiWsMessage,
    },
};

pub(super) struct PrivateStream {
    pub(super) config: KalshiExecutionClientConfig,
    pub(super) http: KalshiHttpClient,
    pub(super) emitter: ExecutionEventEmitter,
    pub(super) data_sender: Option<EventSender<DataEvent>>,
    pub(super) state: Arc<Mutex<PrivateState>>,
    pub(super) definitions: HashMap<InstrumentId, BinaryOption>,
    pub(super) instruments: HashSet<InstrumentId>,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) ws_client: KalshiWebSocketClient,
}

impl PrivateStream {
    pub(super) async fn run(
        mut self,
        mut receiver: tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
        mut requests: tokio::sync::mpsc::UnboundedReceiver<()>,
        cancellation: CancellationToken,
        startup: tokio::sync::oneshot::Sender<anyhow::Result<()>>,
    ) {
        let mut startup = Some(startup);
        let mut interval = time::interval(Duration::from_secs(1));
        interval.set_missed_tick_behavior(time::MissedTickBehavior::Skip);

        loop {
            let recover = tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                event = receiver.recv() => match event {
                    Some(KalshiWsEvent::Ready { .. }) => {
                        self.ready.store(false, Ordering::Release);
                        self.state.lock().expect("private state lock is not poisoned").invalidate();
                        true
                    }
                    Some(KalshiWsEvent::Error { message, .. }) => {
                        self.ready.store(false, Ordering::Release);
                        self.state.lock().expect("private state lock is not poisoned").invalidate();
                        log::warn!("Kalshi private stream requires recovery: {message}");
                        false
                    }
                    Some(KalshiWsEvent::Message { message, .. }) => {
                        self.notice(&message)
                    },
                    Some(_) => false,
                    None => break,
                },
                request = requests.recv() => if request.is_some() { true } else { break },
                _ = interval.tick() => self.ws_client.is_active(),
            };

            if !recover {
                continue;
            }

            let result = tokio::select! {
                biased;
                () = cancellation.cancelled() => break,
                result = self.recover(&mut receiver) => result,
            };

            match result {
                Ok(()) => {
                    if let Some(startup) = startup.take() {
                        let _ = startup.send(Ok(()));
                    }
                }
                Err(e) => {
                    let fatal = e.downcast_ref::<RecoveryContextError>().is_some();
                    self.ready.store(false, Ordering::Release);
                    self.state
                        .lock()
                        .expect("private state lock is not poisoned")
                        .invalidate();
                    log::warn!("Kalshi private recovery remains incomplete: {e}");

                    if fatal && let Some(startup) = startup.take() {
                        let _ = startup.send(Err(e));
                        break;
                    }
                }
            }
        }

        self.ready.store(false, Ordering::Release);
        self.state
            .lock()
            .expect("private state lock is not poisoned")
            .invalidate();

        if let Err(e) = self.ws_client.stop().await {
            log::warn!("Kalshi private shutdown failed: {e}");
        }
    }

    async fn recover(
        &mut self,
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
    ) -> anyhow::Result<()> {
        let epoch = self
            .ws_client
            .connection_epoch()
            .context("private transport is not active")?;
        self.scope_position_notices().await?;

        let query = AccountQuery {
            subaccount: 0,
            exchange_index: self.config.exchange_index,
        };

        let (start, refresh_ticket) = {
            let state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");
            (state.start, state.ledger.begin_refresh())
        };

        let (balance, snapshot) = self.collect(receiver, &query, start).await?;
        anyhow::ensure!(
            balance.balance_dollars.0 >= Decimal::ZERO
                && balance.balance_dollars.0.normalize().scale() <= 4,
            "invalid Direct cash precision"
        );
        let seconds = u64::try_from(balance.updated_ts).context("negative account timestamp")?;
        let nanos = seconds
            .checked_mul(1_000_000_000)
            .context("account timestamp overflow")?;
        let ts_event = UnixNanos::from(nanos);
        let settlements = self.prepare_settlements(&snapshot).await?;
        self.load_definitions(&snapshot, &settlements).await?;
        let (plans, mut holds) = self.prepare_orders(&snapshot).await?;

        let missing: Vec<_> = {
            let state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");
            state
                .orders
                .values()
                .map(|context| &context.order)
                .filter(|order| !order.is_closed())
                .filter(|order| {
                    !snapshot.orders.iter().any(|source| {
                        source.client_order_id == order.client_order_id().as_str()
                            || order
                                .venue_order_id()
                                .is_some_and(|id| id.as_str() == source.order_id)
                    })
                })
                .cloned()
                .collect()
        };

        for order in &missing {
            holds.insert(
                ReservationId::Client(order.client_order_id()),
                self.hold(
                    order.instrument_id(),
                    order.order_side(),
                    order.leaves_qty().as_decimal(),
                    order
                        .price()
                        .context("pending order lacks price")?
                        .as_decimal(),
                )
                .await?,
            );
        }

        self.check_preparation(receiver)?;

        let mut external_leaves = HashMap::new();

        for source in &snapshot.orders {
            let venue_id = VenueOrderId::new_checked(&source.order_id)?;
            if holds.contains_key(&ReservationId::Venue(venue_id)) {
                let id = crate::common::parse::parse_instrument_id(&source.ticker)?;
                let side = reports::direction(source.book_side, source.outcome_side)?;
                let leaves: &mut Decimal =
                    external_leaves.entry((id, side)).or_insert(Decimal::ZERO);
                *leaves = leaves
                    .checked_add(source.remaining_count_fp.0)
                    .context("external Kalshi remaining quantity overflow")?;
            }
        }

        anyhow::ensure!(
            self.ws_client.connection_epoch() == Some(epoch),
            "private connection changed during recovery"
        );
        let mut state = self
            .state
            .lock()
            .expect("private state lock is not poisoned");
        state.ledger.check_refresh(refresh_ticket)?;
        for source in &snapshot.orders {
            state.source_order(source)?;
        }

        state.check_notices(&snapshot, &plans)?;
        let openings = state.openings(&plans, &snapshot.positions, &settlements)?;
        let amendments = state.check_amendments(&plans);
        let mapped = plans.iter().all(|plan| plan.mapped);
        state.check_settlements(&plans, &settlements)?;
        let (_, projected) = state.preview(
            &plans,
            &openings,
            &settlements,
            balance.balance_dollars.0,
            &mut holds,
        )?;
        let mut positions = Vec::new();

        for position in &snapshot.positions {
            let id = crate::common::parse::parse_instrument_id(&position.ticker)?;
            let definition = self
                .definitions
                .get(&id)
                .context("private position definition missing")?;
            let report = reports::position(
                position,
                definition,
                self.emitter.account_id(),
                snapshot.ts_init,
            )?;
            anyhow::ensure!(
                projected.get(&id) == Some(&report.signed_decimal_qty),
                "private position does not yet cover fills"
            );
            positions.push(report);
        }

        for (id, net) in &projected {
            anyhow::ensure!(
                net.is_zero() || positions.iter().any(|report| report.instrument_id == *id),
                "open position lacks current venue evidence"
            );
        }

        if !state.ledger.matches(balance.balance_dollars.0, &holds)
            || projected != state.positions
            || external_leaves != state.external_leaves
            || plans.iter().any(super::context::OrderPlan::has_events)
        {
            self.ready.store(false, Ordering::Release);
            state.invalidate();
        }

        state.positions = openings;
        state.apply_plans(plans, &self.emitter)?;

        for Settlement {
            instrument_id: id,
            value,
            ts_event: ts_settled,
            ..
        } in settlements
        {
            let definition = self
                .definitions
                .get(&id)
                .context("settlement instrument definition missing")?;
            state.settle(
                definition,
                value,
                ts_settled,
                snapshot.ts_init,
                &self.emitter,
            )?;
        }

        for position in positions {
            state.apply_position(position, &self.emitter)?;
        }

        let ticket = state.ledger.begin_refresh();
        state
            .ledger
            .apply_refresh(ticket, balance.balance_dollars.0, holds)?;
        state.external_leaves = external_leaves;

        state.publish_balance(&self.emitter, ts_event)?;
        state.epoch = Some(epoch);
        if mapped {
            state.clear_notices();
            state.advance_recovery(snapshot.ts_init);
        }

        if let Err(e) = &amendments {
            log::debug!("Kalshi command admission remains blocked: {e}");
        }

        self.ready.store(amendments.is_ok(), Ordering::Release);
        Ok(())
    }

    async fn scope_position_notices(&self) -> anyhow::Result<()> {
        let pending: Vec<_> = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .notices_positions
            .keys()
            .copied()
            .collect();

        for id in pending {
            if !self.definitions.contains_key(&id) {
                let market = crate::providers::KalshiInstrumentProvider::load_market(
                    &self.http,
                    id.symbol.as_str(),
                )
                .await?;
                anyhow::ensure!(
                    market.ticker == id.symbol.as_str(),
                    "private position market identity mismatch"
                );

                let Field::Value(shard) = market.exchange_index else {
                    anyhow::bail!("private position lacks confirmed exchange shard");
                };

                if i64::from(shard) != i64::from(self.config.exchange_index) {
                    self.state
                        .lock()
                        .expect("private state lock is not poisoned")
                        .notices_positions
                        .remove(&id);
                    continue;
                }
            }

            let mut state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");

            // Explicit fills supersede cumulative volume, which includes pre-recovery trades
            if state.has_fill_notice(id) {
                continue;
            }

            state.start = state
                .orders
                .values()
                .filter(|context| context.order.instrument_id() == id)
                .fold(state.start, |start, context| {
                    start.min(context.order.ts_init())
                });
        }

        Ok(())
    }

    fn notice(&self, message: &KalshiWsMessage) -> bool {
        if !relevant(message, self.config.exchange_index) {
            return false;
        }

        self.ready.store(false, Ordering::Release);
        let mut state = self
            .state
            .lock()
            .expect("private state lock is not poisoned");

        if let Err(e) = state.notice(message) {
            log::warn!("Kalshi private notification requires authoritative recovery: {e}");
        }

        true
    }

    fn check_preparation(
        &self,
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
    ) -> anyhow::Result<()> {
        while let Ok(event) = receiver.try_recv() {
            match event {
                KalshiWsEvent::Ready { .. } | KalshiWsEvent::Error { .. } => {
                    self.ready.store(false, Ordering::Release);
                    self.state
                        .lock()
                        .expect("private state lock is not poisoned")
                        .invalidate();
                    anyhow::bail!("private connection changed during preparation")
                }
                KalshiWsEvent::Message { message, .. }
                    if relevant(&message, self.config.exchange_index) =>
                {
                    self.notice(&message);
                    anyhow::bail!("private evidence changed during preparation")
                }
                _ => {}
            }
        }

        Ok(())
    }

    async fn collect(
        &self,
        receiver: &mut tokio::sync::mpsc::UnboundedReceiver<KalshiWsEvent>,
        query: &AccountQuery,
        start: UnixNanos,
    ) -> anyhow::Result<(Balance, KalshiExecutionSnapshot)> {
        let http = self.http.clone();
        let collection = async {
            let before = http.get_account_balance(query).await?;

            let (tracked, owned, external): (Vec<_>, Vec<_>, Vec<_>) = {
                let state = self
                    .state
                    .lock()
                    .expect("private state lock is not poisoned");

                (
                    state.recovery_targets(),
                    state
                        .orders
                        .values()
                        .filter(|context| !context.order.is_closed())
                        .map(|context| context.order.clone())
                        .collect(),
                    state.external_order_notices(),
                )
            };

            let mut snapshot =
                collect_recovery_snapshot(&http, query.exchange_index, start, &tracked, &owned)
                    .await?;

            for id in external {
                if snapshot
                    .orders
                    .iter()
                    .any(|order| order.order_id == id.as_str())
                {
                    continue;
                }

                let order = http.get_order(id.as_str()).await?.order;
                super::validate_order_scope(&order, query.exchange_index, false)?;
                anyhow::ensure!(
                    order.order_id == id.as_str(),
                    "external order lookup identity mismatch"
                );
                snapshot.orders.push(order);
            }

            let after = http.get_account_balance(query).await?;
            anyhow::ensure!(
                before.balance == after.balance
                    && before.balance_dollars == after.balance_dollars
                    && before.balance_breakdown == after.balance_breakdown,
                "Kalshi account changed during recovery"
            );
            Ok::<_, anyhow::Error>((after, snapshot))
        };

        tokio::pin!(collection);
        let mut changed = false;

        let collected = loop {
            tokio::select! {
                biased;
                event = receiver.recv() => match event {
                    Some(KalshiWsEvent::Error { .. }) | None => anyhow::bail!("private transport changed during recovery"),
                    Some(KalshiWsEvent::Ready { .. }) => { changed = true; self.state.lock().expect("private state lock is not poisoned").invalidate(); },
                    Some(KalshiWsEvent::Message { message, .. }) => {
                        if relevant(&message, query.exchange_index) {
                            changed = true;
                            self.notice(&message);
                        }
                    },
                    Some(_) => {},
                },
                result = &mut collection => break result?,
            }
        };

        anyhow::ensure!(
            !changed && self.ws_client.is_active(),
            "private evidence changed during recovery"
        );
        Ok(collected)
    }

    async fn prepare_settlements(
        &self,
        snapshot: &KalshiExecutionSnapshot,
    ) -> anyhow::Result<Vec<Settlement>> {
        let mut settlement_targets: HashSet<_> = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .positions
            .keys()
            .map(|id| id.symbol.to_string())
            .collect();
        settlement_targets.extend(
            self.state
                .lock()
                .expect("private state lock is not poisoned")
                .recovery_targets()
                .iter()
                .map(|id| id.symbol.to_string()),
        );
        settlement_targets.extend(snapshot.fills.iter().map(|fill| fill.ticker.clone()));
        let settled: Vec<_> = snapshot
            .positions_settled
            .iter()
            .filter(|position| settlement_targets.contains(&position.ticker))
            .collect();
        let mut settlements = Vec::new();

        for position in &settled {
            anyhow::ensure!(
                !snapshot
                    .positions
                    .iter()
                    .any(|current| current.ticker == position.ticker),
                "position appears both current and settled"
            );
            let market = crate::providers::KalshiInstrumentProvider::load_market(
                &self.http,
                &position.ticker,
            )
            .await?;
            anyhow::ensure!(
                market.ticker == position.ticker
                    && market.status == MarketStatus::Finalized
                    && matches!(market.exchange_index, Field::Value(shard) if i64::from(shard) == i64::from(self.config.exchange_index)),
                "settlement market identity or shard mismatch"
            );

            let Field::Value(value) = market.settlement_value_dollars else {
                anyhow::bail!("settled position lacks a settlement value");
            };

            let ts_settled = timestamp(&market.settlement_ts)?;
            anyhow::ensure!(
                ts_settled >= timestamp(&Field::Value(position.last_updated_ts))?,
                "settlement predates position evidence"
            );
            let id = crate::common::parse::parse_instrument_id(&position.ticker)?;
            let needs_opening = !self
                .state
                .lock()
                .expect("private state lock is not poisoned")
                .positions
                .contains_key(&id);

            let mut net = None;

            if needs_opening {
                let page = self
                    .http
                    .get_all_settlements(
                        SettlementsQuery {
                            ticker: Some(position.ticker.clone()),
                            subaccount: 0,
                            ..Default::default()
                        },
                        100,
                    )
                    .await?;

                anyhow::ensure!(page.complete, "settlement pagination is incomplete");

                let records: Vec<_> = page
                    .records
                    .iter()
                    .filter(|record| {
                        record.ticker == position.ticker
                            && i64::from(record.exchange_index)
                                == i64::from(self.config.exchange_index)
                    })
                    .collect();

                anyhow::ensure!(
                    records.len() == 1,
                    "settlement lacks unique contract-count authority"
                );
                let record = records[0];
                anyhow::ensure!(
                    record.yes_count_fp.0 >= Decimal::ZERO
                        && record.no_count_fp.0 >= Decimal::ZERO
                        && timestamp(&Field::Value(record.settled_time))? == ts_settled,
                    "settlement counts or timestamp disagree"
                );
                let count = record
                    .yes_count_fp
                    .0
                    .checked_sub(record.no_count_fp.0)
                    .context("settled contract count overflow")?;
                net = Some(count);
            }

            settlements.push(Settlement {
                instrument_id: id,
                value: value.0,
                ts_event: ts_settled,
                net,
            });
        }

        let requests = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .barrier
            .lock()
            .expect("settlement barrier lock")
            .requested();

        for (id, value, ts_event) in requests {
            if settlements
                .iter()
                .any(|settlement| settlement.instrument_id == id)
            {
                continue;
            }

            let market = crate::providers::KalshiInstrumentProvider::load_market(
                &self.http,
                id.symbol.as_str(),
            )
            .await?;
            anyhow::ensure!(
                market.ticker == id.symbol.as_str()
                    && market.status == MarketStatus::Finalized
                    && matches!(market.exchange_index, Field::Value(shard) if i64::from(shard) == i64::from(self.config.exchange_index))
                    && matches!(market.settlement_value_dollars, Field::Value(actual) if actual.0 == value)
                    && timestamp(&market.settlement_ts)? == ts_event,
                "final close lacks authoritative settlement metadata"
            );
            let state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");
            anyhow::ensure!(
                state.positions.get(&id).is_some_and(Decimal::is_zero),
                "final close lacks authoritative flat exposure"
            );
            settlements.push(Settlement {
                instrument_id: id,
                value,
                ts_event,
                net: None,
            });
        }

        Ok(settlements)
    }

    async fn load_definitions(
        &mut self,
        snapshot: &KalshiExecutionSnapshot,
        settlements: &[Settlement],
    ) -> anyhow::Result<()> {
        let mut needed = HashSet::new();
        for settlement in settlements {
            needed.insert(settlement.instrument_id.symbol.to_string());
        }

        for order in &snapshot.orders {
            needed.insert(order.ticker.clone());
        }

        for position in &snapshot.positions {
            needed.insert(position.ticker.clone());
        }

        {
            let state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");

            for context in state
                .orders
                .values()
                .filter(|context| !context.order.is_closed())
            {
                let order = &context.order;
                needed.insert(order.instrument_id().symbol.to_string());
            }
        }

        for ticker in needed {
            let id = crate::common::parse::parse_instrument_id(&ticker)?;
            if !self.definitions.contains_key(&id) {
                let definition = load_definition(&self.http, &ticker, snapshot.ts_init).await?;
                super::validate_definition(&definition, self.config.exchange_index)?;
                if let Some(sender) = &self.data_sender {
                    let data = InstrumentAny::BinaryOption(definition.clone()).into();
                    sender
                        .send(DataEvent::Data(data))
                        .map_err(|_| anyhow::anyhow!("private instrument receiver closed"))?;
                }

                self.definitions.insert(id, definition);
            }
        }

        for definition in self.definitions.values() {
            super::validate_definition(definition, self.config.exchange_index)?;
        }

        Ok(())
    }

    async fn prepare_orders(
        &self,
        snapshot: &KalshiExecutionSnapshot,
    ) -> anyhow::Result<(Vec<OrderPlan>, HashMap<ReservationId, Decimal>)> {
        let mut holds = HashMap::new();
        let mut plans = Vec::new();

        for source in &snapshot.orders {
            let id = crate::common::parse::parse_instrument_id(&source.ticker)?;
            let definition = self
                .definitions
                .get(&id)
                .context("private definition missing")?;

            let context = {
                let state = self
                    .state
                    .lock()
                    .expect("private state lock is not poisoned");

                anyhow::ensure!(
                    state.source_order(source)?.is_some()
                        || source.fill_count_fp.0 <= Decimal::ZERO
                        || self.instruments.contains(&id),
                    "external fill instrument must be loaded before execution connects"
                );
                state.recovery_order(source, definition, &self.emitter, snapshot.ts_init)?
            };

            let Some(context) = context else {
                reports::order(
                    source,
                    definition,
                    self.emitter.account_id(),
                    None,
                    snapshot.ts_init,
                )?;

                anyhow::ensure!(
                    source.fill_count_fp.0.is_zero()
                        && snapshot
                            .fills
                            .iter()
                            .all(|fill| fill.order_id != source.order_id),
                    RecoveryContextError::Order
                );

                if source.status != VenueStatus::Resting {
                    continue;
                }

                let side = reports::direction(source.book_side, source.outcome_side)?;
                let venue_id = VenueOrderId::new_checked(&source.order_id)?;
                holds.insert(
                    ReservationId::Venue(venue_id),
                    self.hold(
                        id,
                        side,
                        source.remaining_count_fp.0,
                        source.yes_price_dollars.0,
                    )
                    .await?,
                );

                continue;
            };

            let mut fills = Vec::new();

            for fill in snapshot
                .fills
                .iter()
                .filter(|fill| fill.order_id == source.order_id)
            {
                let mut report = reports::fill(
                    fill,
                    definition,
                    self.emitter.account_id(),
                    source,
                    snapshot.ts_init,
                )?;
                report.client_order_id = Some(context.client_order_id());
                fills.push(report);
            }

            let plan = self
                .state
                .lock()
                .expect("private state lock is not poisoned")
                .plan_order(source, &fills, definition, &self.emitter, snapshot.ts_init)?;

            if plan
                .as_ref()
                .map_or(!context.is_closed(), OrderPlan::is_open)
            {
                let (qty, price) = if let Some(plan) = &plan {
                    plan.hold_terms()
                } else {
                    (
                        context.leaves_qty().as_decimal(),
                        context
                            .price()
                            .context("stale order lacks cached price")?
                            .as_decimal(),
                    )
                };

                holds.insert(
                    ReservationId::Client(context.client_order_id()),
                    self.hold(id, context.order_side(), qty, price).await?,
                );
            }

            if let Some(plan) = plan {
                plans.push(plan);
            }
        }

        Ok((plans, holds))
    }

    async fn hold(
        &self,
        instrument_id: InstrumentId,
        side: OrderSide,
        qty: Decimal,
        price: Decimal,
    ) -> anyhow::Result<Decimal> {
        let definition = self
            .definitions
            .get(&instrument_id)
            .context("pending order instrument metadata is missing")?;
        super::account::hold(&self.http, definition, side, qty, price).await
    }
}

fn relevant(message: &KalshiWsMessage, shard: u32) -> bool {
    let data = match message {
        KalshiWsMessage::UserOrder(data)
        | KalshiWsMessage::Fill(data)
        | KalshiWsMessage::MarketPosition(data) => data,
        _ => return false,
    };

    let account = data
        .msg
        .get("subaccount")
        .or_else(|| data.msg.get("subaccount_number"));

    if account
        .and_then(serde_json::Value::as_u64)
        .is_some_and(|value| value != 0)
    {
        return false;
    }

    data.msg
        .get("exchange_index")
        .and_then(serde_json::Value::as_u64)
        .is_none_or(|actual| actual == u64::from(shard))
}

#[cfg(test)]
mod tests {
    #[cfg(not(all(feature = "simulation", madsim)))]
    use nautilus_core::UUID4;
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{enums::AccountType, types::Currency};
    use rstest::rstest;

    use super::*;
    use crate::config::KalshiDataClientConfig;

    #[rstest]
    #[case(KalshiWsEvent::Ready {generation: 2})]
    #[case(KalshiWsEvent::Error {generation: 2, message: "transport lost".into()})]
    fn preparation_epoch_change_invalidates_account_coverage(#[case] event: KalshiWsEvent) {
        let stream = stream(&KalshiDataClientConfig::default());
        let state = Arc::clone(&stream.state);
        let ready = Arc::clone(&stream.ready);
        let session = state.lock().unwrap().session.clone();
        let ticket = state.lock().unwrap().ledger.begin_refresh();

        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        sender.send(event).unwrap();
        assert_eq!(
            stream
                .check_preparation(&mut receiver)
                .unwrap_err()
                .to_string(),
            "private connection changed during preparation"
        );
        assert!(!ready.load(Ordering::Acquire));
        assert_ne!(state.lock().unwrap().session, session);
        assert!(state.lock().unwrap().ledger.check_refresh(ticket).is_err());
        assert!(receiver.try_recv().is_err());
    }

    fn stream(transport: &KalshiDataClientConfig) -> PrivateStream {
        let (ws_client, _) = KalshiWebSocketClient::new(transport.clone()).unwrap();
        let clock = get_atomic_clock_realtime();

        let emitter = ExecutionEventEmitter::new(
            clock,
            "TRADER-001".into(),
            "KALSHI-001".into(),
            AccountType::Cash,
            Some(Currency::USD()),
        );

        let state = Arc::new(Mutex::new(PrivateState::new()));
        let ready = Arc::new(AtomicBool::new(true));

        PrivateStream {
            config: KalshiExecutionClientConfig::default(),
            http: KalshiHttpClient::new(transport).unwrap(),
            emitter,
            data_sender: None,
            state: Arc::clone(&state),
            definitions: HashMap::new(),
            instruments: HashSet::new(),
            ready: Arc::clone(&ready),
            ws_client,
        }
    }

    #[rstest]
    #[case::collected(false)]
    #[case::delayed(true)]
    #[tokio::test]
    async fn external_recovery_requires_complete_fills(#[case] delayed: bool) {
        use crate::http::{
            models::{BookSide, Event, Fill, FixedPoint, Market, Order as VenueOrder, OutcomeSide},
            parse::parse_instrument,
        };

        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let mut orders = Vec::new();
        let mut fills = Vec::new();

        for (index, side, outcome) in [
            (0, BookSide::Bid, OutcomeSide::Yes),
            (1, BookSide::Ask, OutcomeSide::No),
        ] {
            let mut order: VenueOrder =
                serde_json::from_value(fixtures["order"]["order"].clone()).unwrap();
            order.order_id = format!("external-{index}");
            order.client_order_id.clear();
            order.ticker = instrument.id.symbol.to_string();
            order.book_side = side;
            order.outcome_side = outcome;
            order.status = VenueStatus::Executed;
            order.fill_count_fp = FixedPoint(Decimal::ONE);
            order.remaining_count_fp = FixedPoint(Decimal::ZERO);
            let mut fill: Fill =
                serde_json::from_value(fixtures["fills"]["fills"][0].clone()).unwrap();
            fill.fill_id = format!("external-fill-{index}");
            fill.order_id = order.order_id.clone();
            fill.book_side = side;
            fill.outcome_side = outcome;
            fill.ticker = order.ticker.clone();
            fill.market_ticker = order.ticker.clone();
            fill.count_fp = FixedPoint(Decimal::ONE);
            orders.push(order);
            fills.push(fill);
        }

        if delayed {
            fills.clear();
        }

        let mut private = stream(&KalshiDataClientConfig::default());
        private.instruments.insert(instrument.id);
        private.definitions.insert(instrument.id, instrument);

        let snapshot = KalshiExecutionSnapshot {
            orders,
            fills,
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: None,
            ts_init: UnixNanos::from(123),
        };

        let (sender, mut output) = tokio::sync::mpsc::unbounded_channel();
        private.emitter.set_sender(sender);
        let result = private.prepare_orders(&snapshot).await;

        if delayed {
            assert_eq!(
                result.err().unwrap().to_string(),
                "private order fill coverage is incomplete"
            );
        } else {
            let (plans, holds) = result.unwrap();
            assert_eq!(plans.len(), 2);
            assert!(holds.is_empty());
            assert!(
                plans
                    .iter()
                    .all(|plan| !plan.is_open() && plan.has_events())
            );
        }

        private.instruments.clear();
        assert_eq!(
            private
                .prepare_orders(&snapshot)
                .await
                .err()
                .unwrap()
                .to_string(),
            "external fill instrument must be loaded before execution connects"
        );
        assert!(output.try_recv().is_err());
        assert!(private.state.lock().unwrap().ledger.total_is_unknown());
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[case::matching(None, None)]
    #[case::wrong_event(Some("WRONG-EVENT"), None)]
    #[case::wrong_series(None, Some("WRONG-SERIES"))]
    #[tokio::test]
    async fn reservation_fees_require_matching_response_identities(
        #[case] event_ticker: Option<&'static str>,
        #[case] series_ticker: Option<&'static str>,
    ) {
        use crate::http::{
            models::{Event, Market},
            parse::parse_instrument,
        };

        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let transport = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
            ..Default::default()
        };

        let mut response_event = definitions["definitions"]["live"]["event"].clone();
        let mut response_series = definitions["series"].clone();

        if let Some(ticker) = event_ticker {
            response_event["event_ticker"] = ticker.into();
        }

        if let Some(ticker) = series_ticker {
            response_series["series"]["ticker"] = ticker.into();
        }

        let observed = Arc::new(Mutex::new(Vec::new()));

        let server = serve_recovery(listener, observed, move |target| {
            if target.starts_with("/events/") {
                (
                    200,
                    serde_json::json!({"event": response_event, "markets": []}),
                )
            } else {
                assert!(target.starts_with("/series/"));
                (200, response_series.clone())
            }
        });

        let server = tokio::spawn(server); // tokio-import-ok
        let mut private = stream(&transport);
        let id = instrument.id;
        private.definitions.insert(id, instrument);
        let result = private
            .hold(id, OrderSide::Buy, Decimal::ONE, Decimal::new(6001, 4))
            .await;
        server.abort();

        match (event_ticker, series_ticker) {
            (Some(_), _) => assert_eq!(
                result.unwrap_err().to_string(),
                "fee event identity mismatch"
            ),
            (_, Some(_)) => assert_eq!(
                result.unwrap_err().to_string(),
                "fee series identity mismatch"
            ),
            (None, None) => assert_eq!(result.unwrap(), Decimal::new(6277, 4)),
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn successive_stale_mapped_orders_reserve_preserved_plan_terms() {
        use nautilus_model::{
            enums::OrderType,
            events::{OrderAccepted, OrderEventAny, OrderSubmitted},
            orders::builder::OrderTestBuilder,
            types::{Price, Quantity},
        };

        use crate::http::{
            models::{BookSide, Event, Fill, FixedPoint, Market, Order as VenueOrder, OutcomeSide},
            parse::parse_instrument,
        };

        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let id = instrument.id;
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let transport = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
            ..Default::default()
        };

        let server = serve_recovery(listener, Arc::new(Mutex::new(Vec::new())), move |target| {
            if target.starts_with("/events/") {
                (200, serde_json::json!({"event": event, "markets": []}))
            } else {
                assert!(target.starts_with("/series/"));
                (200, definitions["series"].clone())
            }
        });

        let server = tokio::spawn(server); // tokio-import-ok
        let mut private = stream(&transport);
        private.definitions.insert(id, instrument);
        let (sender, _output) = tokio::sync::mpsc::unbounded_channel();
        private.emitter.set_sender(sender);
        private
            .state
            .lock()
            .unwrap()
            .positions
            .insert(id, Decimal::ZERO);
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut source: VenueOrder =
            serde_json::from_value(fixtures["order"]["order"].clone()).unwrap();
        source.ticker = id.symbol.to_string();
        source.book_side = BookSide::Bid;
        source.outcome_side = OutcomeSide::Yes;
        source.fill_count_fp = FixedPoint(Decimal::new(75, 2));
        source.remaining_count_fp = FixedPoint(Decimal::new(25, 2));
        source.initial_count_fp = FixedPoint(Decimal::ONE);
        source.yes_price_dollars = FixedPoint(Decimal::new(6, 1));
        source.no_price_dollars = FixedPoint(Decimal::new(4, 1));
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(id)
            .client_order_id(source.client_order_id.as_str().into())
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.700000"))
            .build();
        let client_id = order.client_order_id();
        order
            .apply(OrderEventAny::Submitted(OrderSubmitted::new(
                order.trader_id(),
                order.strategy_id(),
                id,
                client_id,
                private.emitter.account_id(),
                UUID4::new(),
                UnixNanos::from(1),
                UnixNanos::from(1),
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                id,
                client_id,
                source.order_id.as_str().into(),
                private.emitter.account_id(),
                UUID4::new(),
                UnixNanos::from(timestamp(&source.last_update_time).unwrap().as_u64() + 1),
                UnixNanos::from(2),
                false,
            )))
            .unwrap();
        private.state.lock().unwrap().restore(order);
        let mut fill: Fill = serde_json::from_value(fixtures["fills"]["fills"][0].clone()).unwrap();
        fill.ticker = source.ticker.clone();
        fill.market_ticker = source.ticker.clone();
        fill.book_side = source.book_side;
        fill.outcome_side = source.outcome_side;
        fill.count_fp = source.fill_count_fp;
        fill.yes_price_dollars = source.yes_price_dollars;
        fill.no_price_dollars = source.no_price_dollars;

        let mut snapshot = KalshiExecutionSnapshot {
            orders: vec![source],
            fills: vec![fill],
            positions: Vec::new(),
            positions_settled: Vec::new(),
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: None,
            ts_init: UnixNanos::from(3),
        };

        let (plans, holds) = private.prepare_orders(&snapshot).await.unwrap();

        assert_eq!(plans.len(), 1);
        assert!(plans[0].mapped);
        assert_eq!(
            plans[0].hold_terms(),
            (Decimal::new(125, 2), Decimal::new(7, 1))
        );
        assert_eq!(
            holds,
            HashMap::from([(ReservationId::Client(client_id), Decimal::new(9095, 4))])
        );
        assert!(private.state.lock().unwrap().mutations.is_empty());
        private
            .state
            .lock()
            .unwrap()
            .apply_plans(plans, &private.emitter)
            .unwrap();

        snapshot.orders[0].fill_count_fp = FixedPoint(Decimal::new(80, 2));
        snapshot.orders[0].remaining_count_fp = FixedPoint(Decimal::new(20, 2));
        let mut next = snapshot.fills[0].clone();
        next.fill_id = "fill-b".into();
        next.trade_id = "trade-b".into();
        next.count_fp = FixedPoint(Decimal::new(5, 2));
        next.created_time = Field::Value("2026-10-16T02:02:02.200000002Z".parse().unwrap());
        snapshot.fills.push(next);

        let (plans, holds) = private.prepare_orders(&snapshot).await.unwrap();
        server.abort();

        assert_eq!(plans.len(), 1);
        assert!(plans[0].mapped);
        assert_eq!(
            plans[0].hold_terms(),
            (Decimal::new(120, 2), Decimal::new(7, 1))
        );
        assert_eq!(
            holds,
            HashMap::from([(ReservationId::Client(client_id), Decimal::new(87312, 5))])
        );
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[case("determined", false)]
    #[case("finalized", true)]
    #[tokio::test]
    async fn settled_position_requires_final_market_evidence(
        #[case] status: &str,
        #[case] final_market: bool,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};
        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = definitions["definitions"]["settled"]["market"].clone();
        market["status"] = status.into();
        let ticker = market["ticker"].as_str().unwrap().to_string();
        let timestamp = market["settlement_ts"]
            .as_str()
            .unwrap()
            .parse::<jiff::Timestamp>()
            .unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let transport = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
            ..Default::default()
        };

        let body = serde_json::json!({"market": market}).to_string();

        let server = async move {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }

            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        };

        let server = tokio::spawn(server); // tokio-import-ok

        let mut stream = stream(&transport);
        stream.config.exchange_index = 2;
        let id = crate::common::parse::parse_instrument_id(&ticker).unwrap();
        stream
            .state
            .lock()
            .unwrap()
            .positions
            .insert(id, Decimal::ZERO);
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
        let mut position: crate::http::models::MarketPosition =
            serde_json::from_value(fixtures["positions"]["market_positions"][0].clone()).unwrap();
        position.ticker = ticker;
        position.exchange_index = 2;
        position.position_fp = crate::http::models::FixedPoint(Decimal::ZERO);
        position.last_updated_ts = timestamp;

        let snapshot = KalshiExecutionSnapshot {
            orders: Vec::new(),
            fills: Vec::new(),
            positions: Vec::new(),
            positions_settled: vec![position],
            cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
            lookback_start: None,
            ts_init: UnixNanos::from(u64::MAX - 1000),
        };

        let result = stream.prepare_settlements(&snapshot).await;
        server.await.unwrap();

        if final_market {
            let settlements = result.unwrap();
            assert_eq!(settlements.len(), 1);
            assert_eq!(settlements[0].instrument_id, id);
            assert_eq!(settlements[0].value, Decimal::ONE);
            assert_eq!(
                settlements[0].ts_event,
                nautilus_core::datetime::try_datetime_to_unix_nanos(timestamp).unwrap()
            );
            assert_eq!(settlements[0].net, None);
        } else {
            assert_eq!(
                result.err().unwrap().to_string(),
                "settlement market identity or shard mismatch"
            );
        }

        assert_eq!(stream.state.lock().unwrap().positions[&id], Decimal::ZERO);
        assert!(stream.state.lock().unwrap().ledger.total_is_unknown());
    }

    #[allow(
        dead_code,
        clippy::duplicate_mod,
        reason = "reuse the authentication fixtures in the library tests"
    )]
    #[cfg(not(all(feature = "simulation", madsim)))]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[case(false, false, "2026-10-15T00:00:00Z", false)]
    #[case(true, false, "2026-10-15T00:00:00Z", false)]
    #[case(false, true, "2026-10-24T00:00:00Z", false)]
    #[case(false, true, "2026-10-24T00:00:00Z", true)]
    #[tokio::test]
    async fn bounded_recovery_resolves_old_orders_and_scopes_settlement_history(
        #[case] archived: bool,
        #[case] resting: bool,
        #[case] lookback: &str,
        #[case] canceled_empty: bool,
    ) {
        use nautilus_model::{
            enums::OrderType,
            events::{OrderAccepted, OrderEventAny},
            orders::builder::OrderTestBuilder,
            types::{Price, Quantity},
        };
        tokio::time::timeout(Duration::from_secs(10), async {
            let fixtures: serde_json::Value =
                serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
            let start = timestamp(&Field::Value(lookback.parse().unwrap())).unwrap();
            let mut source = fixtures["order"]["order"].clone();
            let mut fill = fixtures["fills"]["fills"][0].clone();
            let ticker = source["ticker"].as_str().unwrap().to_owned();
            fill["ticker"] = ticker.clone().into();
            fill["market_ticker"] = ticker.clone().into();
            fill["order_id"] = source["order_id"].clone();
            source["created_time"] = "2026-01-01T00:00:00Z".into();
            source["last_update_time"] = fill["created_time"].clone();
            source["status"] = if resting && !canceled_empty {
                "resting"
            } else {
                "canceled"
            }
            .into();

            if canceled_empty {
                source["fill_count_fp"] = "0.00".into();
                source["remaining_count_fp"] = "0.00".into();
            }

            let id = crate::common::parse::parse_instrument_id(&ticker).unwrap();
            let mut position = fixtures["positions"]["market_positions"][0].clone();
            position["ticker"] = ticker.clone().into();
            let mut owned = Vec::new();

            if resting {
                let mut order = OrderTestBuilder::new(OrderType::Limit)
                    .instrument_id(id)
                    .quantity(Quantity::from("2.00"))
                    .price(Price::from("0.600000"))
                    .build();

                let accepted = OrderAccepted::new(
                    order.trader_id(),
                    order.strategy_id(),
                    id,
                    order.client_order_id(),
                    source["order_id"].as_str().unwrap().into(),
                    "KALSHI-001".into(),
                    UUID4::new(),
                    UnixNanos::from(2),
                    UnixNanos::from(3),
                    false,
                );

                order.apply(OrderEventAny::Accepted(accepted)).unwrap();
                owned.push(order);
            }

            let expected_order: crate::http::models::Order =
                serde_json::from_value(source.clone()).unwrap();
            let expected_fill: crate::http::models::Fill =
                serde_json::from_value(fill.clone()).unwrap();
            let expected_position: crate::http::models::MarketPosition =
                serde_json::from_value(position.clone()).unwrap();
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let transport = KalshiDataClientConfig {
                base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
                api_key_id: Some("bounded-test-key".into()),
                private_key: Some(keys::rsa_pkcs8().into()),
                ..Default::default()
            };

            let http = KalshiHttpClient::new_authenticated(&transport).unwrap();
            let requests = Arc::new(Mutex::new(Vec::new()));
            let observed = Arc::clone(&requests);

            let peer = RecoveryResponses {
                fixtures,
                order: source,
                fill,
                position,
                archived,
                bound: resting,
            };

            let server = serve_recovery(listener, observed, move |target| peer.response(target));
            let server = tokio::spawn(server); // tokio-import-ok

            let tracked = if canceled_empty { Vec::new() } else { vec![id] };
            let snapshot = collect_recovery_snapshot(&http, 3, start, &tracked, &owned)
                .await
                .unwrap();
            server.abort();
            assert_eq!(snapshot.orders, vec![expected_order]);
            assert_eq!(
                snapshot.fills,
                if canceled_empty {
                    Vec::new()
                } else {
                    vec![expected_fill]
                }
            );
            assert_eq!(snapshot.lookback_start, Some(start));

            if canceled_empty {
                assert_eq!(snapshot.positions, Vec::new());
                assert_eq!(snapshot.positions_settled, Vec::new());
            } else if resting {
                assert_eq!(snapshot.positions, vec![expected_position]);
                assert_eq!(snapshot.positions_settled, Vec::new());
            } else {
                assert_eq!(snapshot.positions, Vec::new());
                assert_eq!(snapshot.positions_settled, vec![expected_position]);
            }

            let requests = requests.lock().unwrap();
            assert!(requests.iter().any(
                |target| target.starts_with("/portfolio/orders?") && target.contains("min_ts=")
            ));
            assert!(
                requests
                    .iter()
                    .any(|target| target.starts_with("/historical/orders?")
                        && target.contains("min_ts="))
            );
            assert_eq!(
                requests
                    .iter()
                    .filter(|target| target.starts_with("/portfolio/orders/"))
                    .count(),
                usize::from(!resting || canceled_empty)
            );
        })
        .await
        .unwrap();
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    struct RecoveryResponses {
        fixtures: serde_json::Value,
        order: serde_json::Value,
        fill: serde_json::Value,
        position: serde_json::Value,
        archived: bool,
        bound: bool,
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    impl RecoveryResponses {
        fn response(&self, target: &str) -> (u16, serde_json::Value) {
            let ticker = self.order["ticker"].as_str().unwrap();
            let resting = self.order["status"] == "resting";
            let no_fills = self.order["fill_count_fp"] == "0.00";
            let path = target.split('?').next().unwrap();

            let (status, body) = match path {
                "/account/limits" => (200, self.fixtures["limits"].clone()),
                "/account/endpoint_costs" => (200, self.fixtures["costs"].clone()),
                "/historical/cutoff" => (200, self.fixtures["cutoff"].clone()),
                "/portfolio/orders" if resting && target.contains("status=resting") => (
                    200,
                    serde_json::json!({"orders": [self.order], "cursor":""}),
                ),
                "/portfolio/orders" | "/historical/orders"
                    if target.contains("min_ts=") || target.contains("status=resting") =>
                {
                    (200, self.fixtures["empty_orders"].clone())
                }
                "/portfolio/orders" | "/historical/orders" => {
                    assert!(
                        target.contains(&format!("ticker={ticker}")),
                        "unbounded archive query {target}"
                    );

                    (
                        200,
                        serde_json::json!({"orders": [self.order], "cursor":""}),
                    )
                }
                "/portfolio/fills" | "/historical/fills"
                    if self.bound && (no_fills || !target.contains("ticker=")) =>
                {
                    (200, self.fixtures["empty_fills"].clone())
                }
                "/portfolio/fills" | "/historical/fills" => {
                    (200, serde_json::json!({"fills": [self.fill], "cursor":""}))
                }
                "/portfolio/positions" if !target.contains("settlement_status=settled") => {
                    let positions = if resting {
                        vec![self.position.clone()]
                    } else {
                        Vec::new()
                    };

                    (
                        200,
                        serde_json::json!({"market_positions":positions,"event_positions":[],"cursor":""}),
                    )
                }
                "/portfolio/positions" | "/historical/positions" => {
                    assert!(
                        target.contains(&format!("ticker={ticker}")),
                        "unbounded position history {target}"
                    );

                    (
                        200,
                        serde_json::json!({"market_positions":[self.position],"event_positions":[],"cursor":""}),
                    )
                }
                _ if path.starts_with("/portfolio/orders/") => {
                    if self.archived {
                        (404, serde_json::json!({}))
                    } else {
                        (200, serde_json::json!({"order":self.order}))
                    }
                }
                _ => panic!("unexpected target {target}"),
            };

            (status, body)
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    async fn serve_recovery(
        listener: tokio::net::TcpListener,
        observed: Arc<Mutex<Vec<String>>>,
        reply: impl Fn(&str) -> (u16, serde_json::Value) + Send + 'static,
    ) {
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut request = Vec::new();
            while !request.ends_with(b"\r\n\r\n") {
                request.push(socket.read_u8().await.unwrap());
            }

            let request = String::from_utf8(request).unwrap();
            let target = request
                .lines()
                .next()
                .unwrap()
                .split_whitespace()
                .nth(1)
                .unwrap();
            observed.lock().unwrap().push(target.to_owned());
            let (status, body) = reply(target);
            let body = body.to_string();
            let response = format!(
                "HTTP/1.1 {status} Response\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(response.as_bytes()).await.unwrap();
        }
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn transport_epoch_change_without_a_queued_notice_rejects_account_recovery() {
        use futures_util::StreamExt;
        use tokio::io::{AsyncReadExt, AsyncWriteExt};

        tokio::time::timeout(Duration::from_secs(15), async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let http_address = listener.local_addr().unwrap();
            let websocket = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let ws_address = websocket.local_addr().unwrap();
            let blocked = Arc::new(tokio::sync::Notify::new());
            let resume = Arc::new(tokio::sync::Notify::new());
            let reached = Arc::clone(&blocked);
            let release = Arc::clone(&resume);
            let mut fixtures: serde_json::Value = serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();
            fixtures["balance"]["balance_dollars"] = "1.0001".into();

            let http = tokio::spawn(async move { // tokio-import-ok
                let mut paused = false;

                loop {
                    let (mut socket, _) = listener.accept().await.unwrap();
                    let mut request = Vec::new();
                    while !request.ends_with(b"\r\n\r\n") {
                        request.push(socket.read_u8().await.unwrap());
                    }

                    let request = String::from_utf8(request).unwrap();
                    let target = request.lines().next().unwrap().split_whitespace().nth(1).unwrap();
                    let path = target.split('?').next().unwrap();

                    let body = match path {
                        "/account/limits" => &fixtures["limits"],
                        "/account/endpoint_costs" => &fixtures["costs"],
                        "/portfolio/balance" => &fixtures["balance"],
                        "/historical/cutoff" => &fixtures["cutoff"],
                        "/portfolio/orders" | "/historical/orders" => {
                            if !paused {
                                paused = true;
                                reached.notify_one();
                                release.notified().await;
                            }

                            &fixtures["empty_orders"]
                        }
                        "/portfolio/fills" | "/historical/fills" => &fixtures["empty_fills"],
                        "/portfolio/positions" | "/historical/positions" => &fixtures["empty_positions"],
                        _ => panic!("unexpected request {target}"),
                    }.to_string();

                    let response = format!("HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len());
                    socket.write_all(response.as_bytes()).await.unwrap();
                }
            });

            let sockets = tokio::spawn(async move { // tokio-import-ok

                loop {
                    let (socket, _) = websocket.accept().await.unwrap();
                    let mut ws_client = tokio_tungstenite::accept_async(socket).await.unwrap();
                    while ws_client.next().await.is_some() {}
                }
            });

            let transport = KalshiDataClientConfig {
                api_key_id: Some("epoch-test".into()),
                private_key: Some(keys::rsa_pkcs8().into()),
                base_url_http: Some(format!("http://{http_address}").into()),
                base_url_ws: Some(format!("ws://{ws_address}/ws").into()),
                ..Default::default()
            };

            let mut private = stream(&transport);
            private.http = KalshiHttpClient::new_authenticated(&transport).unwrap();
            let (ws_client, _transport_events) = KalshiWebSocketClient::new(transport).unwrap();
            ws_client.connect().await.unwrap();
            let epoch = ws_client.connection_epoch().unwrap();
            private.ws_client = ws_client.clone();
            let (output, mut events) = tokio::sync::mpsc::unbounded_channel();
            private.emitter.set_sender(output);
            let (_pending, mut receiver) = tokio::sync::mpsc::unbounded_channel();
            let recovery = private.recover(&mut receiver);
            tokio::pin!(recovery);
            tokio::select! {
                result = &mut recovery => panic!("recovery finished before blocked collection: {result:?}"),
                () = blocked.notified() => {},
            }
            ws_client.stop().await.unwrap();
            ws_client.connect().await.unwrap();
            assert_ne!(ws_client.connection_epoch(), Some(epoch));
            resume.notify_one();
            assert_eq!(recovery.await.unwrap_err().to_string(), "private connection changed during recovery");
            assert!(events.try_recv().is_err(), "stale recovery emitted account state");
            ws_client.stop().await.unwrap();
            http.abort();
            sockets.abort();
        }).await.unwrap();
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[tokio::test]
    async fn released_closes_do_not_repeat_metadata_reads_or_lose_late_fill_protection() {
        use std::sync::atomic::AtomicUsize;

        use nautilus_common::cache::Cache;
        use nautilus_model::{data::InstrumentClose, enums::InstrumentCloseType, types::Price};

        tokio::time::timeout(Duration::from_secs(5), async {
            let definitions: serde_json::Value =
                serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
            let mut market = definitions["definitions"]["settled"]["market"].clone();
            market["status"] = "finalized".into();
            let ticker = market["ticker"].as_str().unwrap();
            let id = crate::common::parse::parse_instrument_id(ticker).unwrap();
            let ts_event = timestamp(&Field::Value(
                market["settlement_ts"].as_str().unwrap().parse().unwrap(),
            ))
            .unwrap();

            let close = InstrumentClose::new(
                id,
                Price::from("1.000000"),
                InstrumentCloseType::ContractExpired,
                ts_event,
                ts_event,
            );

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

            let transport = KalshiDataClientConfig {
                base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
                ..Default::default()
            };

            let requests = Arc::new(Mutex::new(Vec::new()));
            let reads = Arc::new(AtomicUsize::new(0));
            let observed = Arc::clone(&reads);
            let server = serve_recovery(listener, requests, move |_| {
                observed.fetch_add(1, Ordering::SeqCst);
                (200, serde_json::json!({"market":market}))
            });

            let server = tokio::spawn(server); // tokio-import-ok

            let mut stream = stream(&transport);
            stream.config.exchange_index = 2;
            let barrier = Arc::clone(&stream.state.lock().unwrap().barrier);
            stream
                .state
                .lock()
                .unwrap()
                .positions
                .insert(id, Decimal::ZERO);
            let mut cache = Cache::default();
            cache.add_instrument_close(close).unwrap();
            barrier.lock().unwrap().track(id);
            barrier
                .lock()
                .unwrap()
                .observe(&cache, stream.emitter.account_id());
            let fixtures: serde_json::Value =
                serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

            let snapshot = KalshiExecutionSnapshot {
                orders: Vec::new(),
                fills: Vec::new(),
                positions: Vec::new(),
                positions_settled: Vec::new(),
                cutoff: serde_json::from_value(fixtures["cutoff"].clone()).unwrap(),
                lookback_start: None,
                ts_init: UnixNanos::from(u64::MAX - 1000),
            };

            let mut batches = Vec::new();

            for _ in 0..3 {
                barrier.lock().unwrap().invalidate("next session");
                batches.push(stream.prepare_settlements(&snapshot).await.unwrap());
            }

            server.abort();
            assert_eq!(reads.load(Ordering::SeqCst), 0);
            assert!(batches.iter().all(Vec::is_empty));
            assert!(barrier.lock().unwrap().sealed(id));
            assert_eq!(
                barrier
                    .lock()
                    .unwrap()
                    .check_signature(id, Decimal::ZERO, ts_event)
                    .unwrap_err()
                    .to_string(),
                "settlement signature changed"
            );
            assert!(barrier.lock().unwrap().is_blocked());
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn explicit_fill_notice_does_not_rewind_recovery_to_older_orders() {
        use nautilus_model::{
            enums::OrderType,
            orders::builder::OrderTestBuilder,
            types::{Price, Quantity},
        };

        use crate::http::{
            models::{Event, Market},
            parse::parse_instrument,
        };

        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market: Market =
            serde_json::from_value(definitions["definitions"]["live"]["market"].clone()).unwrap();
        let event: Event =
            serde_json::from_value(definitions["definitions"]["live"]["event"].clone()).unwrap();
        let instrument = parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let id = instrument.id;
        let mut private = stream(&KalshiDataClientConfig::default());
        private.definitions.insert(id, instrument);
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(id)
            .quantity(Quantity::from("1.00"))
            .price(Price::from("0.600000"))
            .build();
        private.state.lock().unwrap().restore(order);
        let start = UnixNanos::from(1_792_800_000_000_000_000);
        private.state.lock().unwrap().start = start;

        let mut fill_notice = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../test_data/websocket_messages.json"
        ))
        .unwrap()["fill"]
            .clone();
        fill_notice["msg"]["market_ticker"] = serde_json::json!(id.symbol.as_str());
        fill_notice["msg"]["ts"] = serde_json::json!(start.as_u64() / 1_000_000_000);
        let fill_notice = serde_json::from_value(fill_notice).unwrap();
        assert!(private.notice(&fill_notice));

        let mut position_notice = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../test_data/websocket_messages.json"
        ))
        .unwrap()["position_small"]
            .clone();
        position_notice["msg"]["market_ticker"] = serde_json::json!(id.symbol.as_str());
        let position_notice = serde_json::from_value(position_notice).unwrap();
        assert!(private.notice(&position_notice));

        private.scope_position_notices().await.unwrap();

        assert_eq!(private.state.lock().unwrap().start, start);
    }

    #[cfg(not(all(feature = "simulation", madsim)))]
    #[rstest]
    #[case(2, false, false)]
    #[case(3, false, true)]
    #[case(3, true, true)]
    #[tokio::test]
    async fn position_notification_scope_uses_confirmed_market_shard(
        #[case] shard: i32,
        #[case] owned: bool,
        #[case] retained: bool,
    ) {
        use nautilus_model::{
            enums::OrderType,
            orders::builder::OrderTestBuilder,
            types::{Price, Quantity},
        };
        let definitions: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let mut market = definitions["definitions"]["live"]["market"].clone();
        market["exchange_index"] = shard.into();
        let ticker = market["ticker"].as_str().unwrap().to_string();
        let id = crate::common::parse::parse_instrument_id(&ticker).unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();

        let transport = KalshiDataClientConfig {
            base_url_http: Some(format!("http://{}", listener.local_addr().unwrap()).into()),
            ..Default::default()
        };

        let requests = Arc::new(Mutex::new(Vec::new()));
        let observed = Arc::clone(&requests);

        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/rest_responses.json")).unwrap();

        let server = serve_recovery(listener, observed, move |target| {
            let body = if target.starts_with("/markets/") {
                serde_json::json!({"market":market})
            } else if target.starts_with("/account/limits") {
                fixtures["limits"].clone()
            } else if target.starts_with("/account/endpoint_costs") {
                fixtures["costs"].clone()
            } else if target.starts_with("/historical/cutoff") {
                fixtures["cutoff"].clone()
            } else if target.contains("/orders?") {
                fixtures["empty_orders"].clone()
            } else if target.contains("/fills?") {
                fixtures["empty_fills"].clone()
            } else if target.contains("/positions?") {
                fixtures["empty_positions"].clone()
            } else {
                panic!("unexpected request {target}")
            };

            (200, body)
        });

        let server = tokio::spawn(server); // tokio-import-ok

        let mut stream = stream(&transport);
        stream.config.exchange_index = 3;
        let start = UnixNanos::from(1_792_800_000_000_000_000);

        if owned {
            let order = OrderTestBuilder::new(OrderType::Limit)
                .instrument_id(id)
                .quantity(Quantity::from("1.00"))
                .price(Price::from("0.600000"))
                .build();
            stream.state.lock().unwrap().restore(order);
        }

        stream.state.lock().unwrap().start = start;

        let mut notice = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../test_data/websocket_messages.json"
        ))
        .unwrap()["position_refreshed"]
            .clone();
        notice["msg"]["market_ticker"] = serde_json::json!(ticker);
        let notice = serde_json::from_value(notice).unwrap();

        assert!(stream.notice(&notice));
        assert_eq!(
            stream.state.lock().unwrap().start,
            start,
            "unscoped position notice must not rewind history"
        );
        stream.scope_position_notices().await.unwrap();
        assert_eq!(
            stream.state.lock().unwrap().start,
            if owned { UnixNanos::from(0) } else { start }
        );

        if !retained {
            let private = KalshiDataClientConfig {
                api_key_id: Some("scoped-test-key".into()),
                private_key: Some(keys::rsa_pkcs8().into()),
                ..transport.clone()
            };

            stream.http = KalshiHttpClient::new_authenticated(&private).unwrap();
            let recovery_start = stream.state.lock().unwrap().start;
            let snapshot = collect_recovery_snapshot(&stream.http, 3, recovery_start, &[], &[])
                .await
                .unwrap();
            assert_eq!(snapshot.lookback_start, Some(start));
            assert!(snapshot.orders.is_empty());
            assert!(snapshot.fills.is_empty());
        }

        server.abort();
        assert_eq!(
            stream.state.lock().unwrap().recovery_targets(),
            if retained { vec![id] } else { Vec::new() }
        );
        assert!(!stream.ready.load(Ordering::Acquire));
        let requests = requests.lock().unwrap();
        assert_eq!(requests[0], format!("/markets/{ticker}"));

        if retained {
            assert_eq!(requests.len(), 1);
        } else {
            let reads: Vec<_> = requests
                .iter()
                .filter(|target| {
                    (target.contains("/orders?") || target.contains("/fills?"))
                        && !target.contains("status=resting")
                })
                .collect();

            assert_eq!(reads.len(), 4);
            let min_ts = format!("min_ts={}", start.as_u64() / 1_000_000_000 - 1);
            assert!(reads.iter().all(|target| target.contains(&min_ts)));
            assert_eq!(requests.len(), 10);
        }
    }
}
