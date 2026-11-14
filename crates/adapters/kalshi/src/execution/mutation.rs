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
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use anyhow::Context;
use nautilus_common::messages::execution::{CancelOrder, ModifyOrder};
use nautilus_core::{
    UUID4, UnixNanos, datetime::NANOSECONDS_IN_MILLISECOND, time::get_atomic_clock_realtime,
};
use nautilus_live::{ExecutionEventEmitter, execution::failure::CommandFailure};
use nautilus_model::{
    enums::OrderType,
    events::{
        OrderCancelDenied, OrderCancelRejected, OrderDeniedReason, OrderEventAny,
        OrderModifyDenied, OrderModifyRejected, OrderUpdated,
    },
    identifiers::ClientOrderId,
    instruments::BinaryOption,
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use nautilus_network::dst::time::Instant;
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{context::PrivateState, reports, submit};
use crate::{
    http::{
        client::KalshiHttpClient,
        error::KalshiHttpError,
        models::{Field, FixedPoint},
        query::{AccountQuery, AmendOrderRequest, CreateOrderRequest, DecreaseOrderRequest},
    },
    websocket::client::KalshiWebSocketClient,
};

pub(super) const RECOVERY_POLL_INTERVAL: Duration = Duration::from_millis(20);
pub(super) const ADMISSION_TIMEOUT: Duration = Duration::from_secs(15);
const ADMISSION_TIMEOUT_REASON: &str = "Kalshi change admission timed out before dispatch";

pub(super) struct Mutation {
    pub(super) command: Command,
    pub(super) order: OrderAny,
    pub(super) definition: BinaryOption,
    pub(super) shard: u32,
    pub(super) subaccount: u32,
    pub(super) http: KalshiHttpClient,
    pub(super) state: Arc<Mutex<PrivateState>>,
    pub(super) emitter: ExecutionEventEmitter,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) ws_client: KalshiWebSocketClient,
    pub(super) epoch: (u64, u64),
    pub(super) deadline: Instant,
    pub(super) refresh: tokio::sync::mpsc::UnboundedSender<()>,
    pub(super) session: String,
    pub(super) cancellation: CancellationToken,
}

impl Mutation {
    pub(super) async fn run(mut self) {
        let cancellation = self.cancellation.clone();
        let timeout = self.deadline.saturating_duration_since(Instant::now());
        let prepared = tokio::select! {
            biased;
            () = cancellation.cancelled() => Err(anyhow::anyhow!("Kalshi change canceled before dispatch")),
            result = nautilus_network::dst::time::timeout(timeout, self.admit()) => match result {
                Ok(prepared) => prepared,
                Err(_) => Err(anyhow::anyhow!(ADMISSION_TIMEOUT_REASON)),
            },
        };

        let prepared = match prepared {
            Ok(value) => value,
            Err(e) => {
                self.reject_validation(&e.to_string());
                return;
            }
        };

        self.http = submit::transport(
            &self.http,
            &self.state,
            &self.ws_client,
            &self.cancellation,
            &self.session,
        );
        let result = self.send(&prepared).await;

        match result {
            Ok((order_id, client_id, ts_ms, remaining)) => {
                let result = self.acknowledge(&order_id, &client_id, ts_ms, remaining);

                if let Err(e) = result {
                    log::warn!("Kalshi change requires recovery: {e}");
                }
            }
            Err(e) => match submit::classify_failure(&e) {
                CommandFailure::NotSent(_) => {
                    self.reject_validation(&submit::rejection_reason(&e));
                }
                CommandFailure::VenueRejected(_) => {
                    self.reject(&submit::rejection_reason(&e));
                }
                CommandFailure::Ambiguous(_) => log::warn!("Kalshi change requires recovery: {e}"),
            },
        }

        if !self.cancellation.is_cancelled() && self.refresh.send(()).is_err() {
            log::warn!("Kalshi change recovery stopped");
        }
    }

    async fn admit(&mut self) -> anyhow::Result<Prepared> {
        loop {
            self.wait_for_recovery().await?;
            let (prepared, hold) = self.prepare().await?;
            let mut state = self
                .state
                .lock()
                .expect("private state lock is not poisoned");

            anyhow::ensure!(Instant::now() < self.deadline, ADMISSION_TIMEOUT_REASON);

            if !self.ready.load(Ordering::Acquire) || state.session != self.session {
                continue;
            }

            let authoritative = {
                let barrier = state.barrier.lock().expect("settlement barrier lock");
                state.epoch == Some(self.epoch)
                    && self.ws_client.connection_epoch() == Some(self.epoch)
                    && barrier.is_observed()
                    && !barrier.is_blocked()
            };

            anyhow::ensure!(
                authoritative,
                "Kalshi change requires current-session readiness"
            );
            let current = &state
                .orders
                .get(&self.order.client_order_id())
                .context("Kalshi change context missing")?
                .order;
            anyhow::ensure!(!current.is_closed(), "Kalshi order is already closed");
            anyhow::ensure!(
                current.instrument_id() == self.order.instrument_id()
                    && (current.strategy_id() == self.order.strategy_id()
                        || current.strategy_id().is_external())
                    && current.order_side() == self.order.order_side()
                    && current.venue_order_id() == self.order.venue_order_id()
                    && (matches!(&self.command, Command::Cancel(_))
                        || current.quantity() == self.order.quantity()
                            && current.price() == self.order.price()
                            && current.filled_qty() == self.order.filled_qty()),
                "Kalshi order changed before admission"
            );

            if let (Some(hold), Prepared::Amend { target, .. }) = (hold, &prepared) {
                let increase = (target.quantity().as_decimal()
                    - self.order.quantity().as_decimal())
                .max(Decimal::ZERO);
                state.ledger.reserve_amendment(
                    self.order.client_order_id(),
                    hold,
                    target.leaves_qty().as_decimal(),
                    increase,
                )?;
            }

            self.mark_dispatched(&mut state, prepared.terms())?;
            return Ok(prepared);
        }
    }

    pub(super) fn mark_dispatched(
        &self,
        state: &mut PrivateState,
        terms: Option<(Quantity, Price)>,
    ) -> anyhow::Result<()> {
        let started = state
            .orders
            .get(&self.order.client_order_id())
            .context("Kalshi change context missing")?
            .order
            .ts_last();
        let pending = state
            .mutations
            .get_mut(&self.order.client_order_id())
            .context("Kalshi change claim is missing")?;
        anyhow::ensure!(
            pending.command_id == self.command.id(),
            "Kalshi change claim was replaced"
        );
        pending.started = started;
        pending.terms = terms;
        pending.dispatched = true;
        state.mutations_dispatched.insert(self.command.id());
        state.ledger.invalidate();
        self.ready.store(false, Ordering::Release);
        Ok(())
    }

    pub(super) fn acknowledge(
        &self,
        order_id: &str,
        client_id: &Field<String>,
        ts_ms: i64,
        remaining: Option<FixedPoint>,
    ) -> anyhow::Result<()> {
        anyhow::ensure!(
            Some(order_id) == self.order.venue_order_id().as_ref().map(|id| id.as_str()),
            "Kalshi change acknowledgement venue identity mismatch"
        );
        anyhow::ensure!(
            matches!(client_id, Field::Missing)
                || client_id == &Field::Value(self.order.client_order_id().to_string()),
            "Kalshi change acknowledgement client identity mismatch"
        );
        let nanos = u64::try_from(ts_ms)?
            .checked_mul(NANOSECONDS_IN_MILLISECOND)
            .context("Kalshi change timestamp overflow")?;
        let remaining = remaining
            .map(|qty| reports::quantity(qty, self.definition.size_precision))
            .transpose()?;
        let mut state = self
            .state
            .lock()
            .expect("private state lock is not poisoned");

        if let Some(pending) = state
            .mutations
            .get_mut(&self.order.client_order_id())
            .filter(|pending| pending.command_id == self.command.id())
        {
            anyhow::ensure!(
                UnixNanos::from(nanos)
                    >= UnixNanos::from(
                        pending.started.as_u64() / NANOSECONDS_IN_MILLISECOND
                            * NANOSECONDS_IN_MILLISECOND
                    ),
                "stale Kalshi change acknowledgement"
            );
            pending.acknowledgement = Some(Acknowledgement {
                timestamp: UnixNanos::from(nanos),
                remaining,
            });
        }

        Ok(())
    }

    async fn wait_for_recovery(&mut self) -> anyhow::Result<()> {
        let epoch = self.epoch;

        loop {
            {
                let state = self
                    .state
                    .lock()
                    .expect("private state lock is not poisoned");
                anyhow::ensure!(
                    self.ws_client.connection_epoch() == Some(epoch),
                    "Kalshi change connection generation changed before admission"
                );
                let current = state
                    .orders
                    .get(&self.order.client_order_id())
                    .context("Kalshi change context missing")?;
                anyhow::ensure!(!current.order.is_closed(), "Kalshi order is already closed");
                let barrier = state.barrier.lock().expect("settlement barrier lock");
                anyhow::ensure!(
                    !barrier.is_blocked(),
                    "Kalshi change is blocked by settlement authority"
                );

                if self.ready.load(Ordering::Acquire)
                    && state.epoch == Some(epoch)
                    && barrier.is_observed()
                {
                    self.session = state.session.clone();
                    return Ok(());
                }
            }

            nautilus_network::dst::time::sleep(RECOVERY_POLL_INTERVAL).await;
        }
    }

    async fn prepare(&self) -> anyhow::Result<(Prepared, Option<Decimal>)> {
        let prepared = prepare(
            &self.command,
            &self.order,
            self.shard,
            self.subaccount,
            &self.definition,
        )?;
        let mut hold = None;
        if let Prepared::Amend { target, request } = &prepared {
            hold = Some(submit::prepare(&self.http, target, &self.definition, request).await?);
        }

        Ok((prepared, hold))
    }

    async fn send(
        &self,
        prepared: &Prepared,
    ) -> Result<(String, Field<String>, i64, Option<FixedPoint>), KalshiHttpError> {
        let venue_id = self
            .order
            .venue_order_id()
            .expect("preparation requires a venue identity");

        match prepared {
            Prepared::Amend { request, .. } => {
                let amendment = AmendOrderRequest {
                    ticker: request.ticker.clone(),
                    side: request.side,
                    price: request.price,
                    count: request.count,
                    exchange_index: request.exchange_index,
                    client_order_id: request.client_order_id.clone(),
                    updated_client_order_id: None,
                    expiration_time: None,
                };

                let ack = self
                    .http
                    .amend_order(venue_id.as_str(), self.subaccount, &amendment)
                    .await?;

                let remaining = match ack.remaining_count {
                    Field::Value(value) => Some(value),
                    Field::Missing | Field::Null => None,
                };

                Ok((ack.order_id, ack.client_order_id, ack.ts_ms, remaining))
            }
            Prepared::Decrease { request, .. } => {
                let ack = self
                    .http
                    .decrease_order(venue_id.as_str(), self.subaccount, request)
                    .await?;

                if ack.remaining_count.0 < Decimal::ZERO
                    || ack.remaining_count.0.normalize().scale() > 2
                {
                    return Err(KalshiHttpError::Parse(
                        "invalid decrease acknowledgement count".into(),
                    ));
                }

                Ok((
                    ack.order_id,
                    ack.client_order_id,
                    ack.ts_ms,
                    Some(ack.remaining_count),
                ))
            }
            Prepared::Cancel => {
                let query = AccountQuery {
                    subaccount: self.subaccount,
                    exchange_index: self.shard,
                };

                let ticker = self.order.instrument_id().symbol.to_string();
                let ack = self
                    .http
                    .cancel_order_for_market(venue_id.as_str(), &query, Some(&ticker))
                    .await?;

                if ack.reduced_by.0 < Decimal::ZERO || ack.reduced_by.0.normalize().scale() > 2 {
                    return Err(KalshiHttpError::Parse(
                        "invalid cancel acknowledgement count".into(),
                    ));
                }

                Ok((ack.order_id, ack.client_order_id, ack.ts_ms, None))
            }
        }
    }

    pub(super) fn reject_validation(&self, reason: &str) {
        self.resolve_failure(&format!("VALIDATION_FAILED: {reason}"), true);
    }

    pub(super) fn reject(&self, reason: &str) {
        self.resolve_failure(reason, false);
    }

    fn resolve_failure(&self, reason: &str, denied: bool) {
        if let Err(e) = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .resolve_mutation(&self.command, &self.order, reason, denied, &self.emitter)
        {
            log::warn!("Kalshi change rejection could not be routed: {e}");
        }

        let _ = self.refresh.send(());
    }
}

impl Drop for Mutation {
    fn drop(&mut self) {
        let unsent = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .mutations
            .get(&self.order.client_order_id())
            .is_some_and(|pending| pending.command_id == self.command.id() && !pending.dispatched);

        if unsent {
            self.reject_validation("Kalshi change canceled before dispatch");
        }
    }
}

#[derive(Debug, Clone)]
pub(super) enum Command {
    Modify(ModifyOrder),
    Cancel(CancelOrder),
}

impl Command {
    pub(super) fn id(&self) -> UUID4 {
        match self {
            Self::Modify(cmd) => cmd.command_id,
            Self::Cancel(cmd) => cmd.command_id,
        }
    }

    pub(super) fn client_order_id(&self) -> ClientOrderId {
        match self {
            Self::Modify(cmd) => cmd.client_order_id,
            Self::Cancel(cmd) => cmd.client_order_id,
        }
    }

    pub(super) fn outcome(
        &self,
        order: &OrderAny,
        reason: &str,
        denied: bool,
        emitter: &ExecutionEventEmitter,
    ) -> OrderEventAny {
        let ts_event = order
            .events()
            .into_iter()
            .rev()
            .find(|event| {
                !matches!(
                    event,
                    OrderEventAny::PendingUpdate(_)
                        | OrderEventAny::PendingCancel(_)
                        | OrderEventAny::ModifyRejected(_)
                        | OrderEventAny::CancelRejected(_)
                )
            })
            .expect("order has an initialization event")
            .ts_event();

        self.outcome_at(order, ts_event, reason, denied, emitter)
    }

    pub(super) fn outcome_at(
        &self,
        order: &OrderAny,
        ts_event: UnixNanos,
        reason: &str,
        denied: bool,
        emitter: &ExecutionEventEmitter,
    ) -> OrderEventAny {
        let now = get_atomic_clock_realtime().get_time_ns();

        match self {
            Self::Modify(cmd) => {
                let mut event = OrderModifyRejected::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason.into(),
                    UUID4::new(),
                    ts_event,
                    now,
                    false,
                    order.venue_order_id(),
                    Some(emitter.account_id()),
                );

                event.causation_id = Some(cmd.command_id);

                if denied {
                    OrderEventAny::ModifyDenied(OrderModifyDenied {
                        trader_id: event.trader_id,
                        strategy_id: event.strategy_id,
                        instrument_id: event.instrument_id,
                        client_order_id: event.client_order_id,
                        reason: event.reason,
                        event_id: event.event_id,
                        ts_event: event.ts_event,
                        ts_init: event.ts_init,
                        venue_order_id: event.venue_order_id,
                        account_id: event.account_id,
                        causation_id: event.causation_id,
                    })
                } else {
                    OrderEventAny::ModifyRejected(event)
                }
            }
            Self::Cancel(cmd) => {
                let mut event = OrderCancelRejected::new(
                    order.trader_id(),
                    order.strategy_id(),
                    order.instrument_id(),
                    order.client_order_id(),
                    reason.into(),
                    UUID4::new(),
                    ts_event,
                    now,
                    false,
                    order.venue_order_id(),
                    Some(emitter.account_id()),
                );

                event.causation_id = Some(cmd.command_id);

                if denied {
                    OrderEventAny::CancelDenied(OrderCancelDenied {
                        trader_id: event.trader_id,
                        strategy_id: event.strategy_id,
                        instrument_id: event.instrument_id,
                        client_order_id: event.client_order_id,
                        reason: event.reason,
                        event_id: event.event_id,
                        ts_event: event.ts_event,
                        ts_init: event.ts_init,
                        venue_order_id: event.venue_order_id,
                        account_id: event.account_id,
                        causation_id: event.causation_id,
                    })
                } else {
                    OrderEventAny::CancelRejected(event)
                }
            }
        }
    }
}

#[derive(Debug)]
pub(super) struct PendingMutation {
    pub(super) command_id: UUID4,
    pub(super) started: UnixNanos,
    pub(super) dispatched: bool,
    pub(super) acknowledgement: Option<Acknowledgement>,
    pub(super) terms: Option<(Quantity, Price)>,
}

impl PendingMutation {
    pub(super) fn covered(&self, order: &OrderAny, source_ts: UnixNanos) -> bool {
        if order.is_closed() {
            return true;
        }

        let Some(ack) = &self.acknowledgement else {
            return false;
        };

        source_ts >= self.started.max(ack.timestamp)
            && ack
                .remaining
                .is_none_or(|remaining| order.leaves_qty() <= remaining)
            && self.terms.is_some_and(|(qty, price)| {
                order.price() == Some(price)
                    && (order.quantity() == qty
                        || ack.remaining.is_some() && order.quantity() >= qty)
            })
    }
}

#[derive(Debug)]
pub(super) struct Acknowledgement {
    timestamp: UnixNanos,
    remaining: Option<Quantity>,
}

enum Prepared {
    Amend {
        target: Box<OrderAny>,
        request: CreateOrderRequest,
    },
    Decrease {
        request: DecreaseOrderRequest,
        quantity: Quantity,
        price: Price,
    },
    Cancel,
}

impl Prepared {
    fn terms(&self) -> Option<(Quantity, Price)> {
        match self {
            Self::Amend { target, .. } => {
                Some((target.quantity(), target.price().expect("limit price")))
            }
            Self::Decrease {
                quantity, price, ..
            } => Some((*quantity, *price)),
            Self::Cancel => None,
        }
    }
}

fn prepare(
    command: &Command,
    order: &OrderAny,
    shard: u32,
    subaccount: u32,
    definition: &BinaryOption,
) -> anyhow::Result<Prepared> {
    anyhow::ensure!(
        order.order_type() == OrderType::Limit
            && order.venue_order_id().is_some()
            && !order.is_closed(),
        "Kalshi change requires an open venue limit order"
    );

    let Command::Modify(cmd) = command else {
        let Command::Cancel(cmd) = command else {
            unreachable!()
        };

        anyhow::ensure!(
            cmd.params.as_ref().is_none_or(|params| params.is_empty()),
            "unsupported Kalshi cancel parameters"
        );
        return Ok(Prepared::Cancel);
    };

    anyhow::ensure!(
        cmd.params.as_ref().is_none_or(|params| params.is_empty()) && cmd.trigger_price.is_none(),
        "unsupported Kalshi modification parameters"
    );
    let quantity = cmd.quantity.unwrap_or(order.quantity());
    let price = cmd
        .price
        .or(order.price())
        .context("Kalshi limit price missing")?;
    let qty = quantity.as_decimal();
    anyhow::ensure!(
        qty >= order.filled_qty().as_decimal()
            && qty.normalize().scale() <= 2
            && qty % definition.size_increment.as_decimal() == Decimal::ZERO
            && definition.max_quantity.is_none_or(|max| quantity <= max),
        "invalid Kalshi amendment quantity"
    );
    anyhow::ensure!(
        quantity != order.quantity() || Some(price) != order.price(),
        "Kalshi amendment does not change order terms"
    );

    if Some(price) == order.price() && quantity < order.quantity() {
        return Ok(Prepared::Decrease {
            request: DecreaseOrderRequest {
                exchange_index: shard,
                reduce_by: Some(FixedPoint(order.quantity().as_decimal() - qty)),
                reduce_to: None,
                market_ticker: Some(order.instrument_id().symbol.to_string()),
            },
            quantity,
            price,
        });
    }

    let mut target = order.clone();
    let now = get_atomic_clock_realtime().get_time_ns();

    let event = OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        quantity,
        UUID4::new(),
        now,
        now,
        false,
        order.venue_order_id(),
        order.account_id(),
        Some(price),
        None,
        None,
        false,
    );

    target.apply(OrderEventAny::Updated(event))?;

    let create =
        submit::request(&target, None, shard, subaccount).map_err(|reason| match reason {
            OrderDeniedReason::ValidationFailed { detail } => anyhow::anyhow!(detail),
            reason => anyhow::anyhow!(reason.to_string()),
        })?;

    Ok(Prepared::Amend {
        target: Box::new(target),
        request: create,
    })
}

#[cfg(test)]
mod tests {
    use nautilus_core::Params;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, TimeInForce},
        events::{OrderAccepted, OrderFilled},
        identifiers::VenueOrderId,
        orders::builder::OrderTestBuilder,
        types::{Currency, Money},
    };
    use rstest::rstest;

    use super::*;

    #[rstest]
    #[case("filled", "invalid Kalshi amendment quantity")]
    #[case("precision", "invalid Kalshi amendment quantity")]
    #[case("maximum", "invalid Kalshi amendment quantity")]
    #[case("trigger", "unsupported Kalshi modification parameters")]
    #[case("expiry", "unsupported Kalshi modification parameters")]
    #[case("unchanged", "Kalshi amendment does not change order terms")]
    #[case("remaining", "")]
    #[case("price", "invalid exact Kalshi quantity or price")]
    fn local_modification_boundaries(#[case] mode: &str, #[case] reason: &str) {
        let fixture: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market =
            serde_json::from_value(fixture["definitions"]["live"]["market"].clone()).unwrap();
        let event =
            serde_json::from_value(fixture["definitions"]["live"]["event"].clone()).unwrap();
        let mut definition =
            crate::http::parse::parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        definition.max_quantity = Some(Quantity::from("2.00"));
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(definition.id)
            .quantity(Quantity::from("2.00"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        order
            .apply(OrderEventAny::Accepted(OrderAccepted::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                VenueOrderId::from("venue-order"),
                "KALSHI-007".into(),
                UUID4::new(),
                UnixNanos::from(2),
                UnixNanos::from(2),
                false,
            )))
            .unwrap();
        order
            .apply(OrderEventAny::Filled(OrderFilled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                VenueOrderId::from("venue-order"),
                "KALSHI-007".into(),
                "fill-a".into(),
                OrderSide::Buy,
                OrderType::Limit,
                Quantity::from("0.25"),
                Price::from("0.600000"),
                Currency::USD(),
                LiquiditySide::Taker,
                UUID4::new(),
                UnixNanos::from(3),
                UnixNanos::from(3),
                false,
                None,
                Some(Money::zero(Currency::USD())),
                None,
            )))
            .unwrap();

        let mut command = ModifyOrder::new(
            order.trader_id(),
            None,
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            order.venue_order_id(),
            None,
            None,
            None,
            UUID4::new(),
            UnixNanos::from(4),
            None,
            None,
        );

        match mode {
            "filled" => command.quantity = Some(Quantity::from("0.24")),
            "precision" => command.quantity = Some(Quantity::from("0.251")),
            "maximum" => command.quantity = Some(Quantity::from("2.01")),
            "trigger" => command.trigger_price = Some(Price::from("0.500000")),
            "expiry" => {
                command.params = Some(Params::new([("expiration_time".into(), 0.into())].into()));
            }
            "remaining" => command.quantity = Some(Quantity::from("0.50")),
            "price" => command.price = Some(Price::from("0.000000")),
            "unchanged" => {}
            _ => unreachable!(),
        }

        let result = prepare(&Command::Modify(command), &order, 3, 0, &definition);

        if reason.is_empty() {
            let Ok(Prepared::Decrease {
                request,
                quantity,
                price,
            }) = result
            else {
                panic!("expected an exact decrease")
            };

            assert_eq!(request.reduce_by, Some(FixedPoint(Decimal::new(150, 2))));
            assert_eq!(request.reduce_to, None);
            assert_eq!(
                request.market_ticker.as_deref(),
                Some(definition.id.symbol.as_str())
            );
            assert_eq!(request.exchange_index, 3);
            assert_eq!(quantity, Quantity::from("0.50"));
            assert_eq!(price, Price::from("0.600000"));
        } else {
            assert_eq!(result.err().unwrap().to_string(), reason);
        }
    }

    #[rstest]
    #[case(None, 200, "1.00", Some("1.00"), "0.600000", false)]
    #[case(Some(150), 149, "1.00", Some("1.00"), "0.600000", false)]
    #[case(Some(150), 150, "1.50", Some("0.00"), "0.600000", false)]
    #[case(Some(150), 150, "1.00", Some("1.00"), "0.600000", true)]
    #[case(Some(150), 149, "0.50", Some("0.50"), "0.600000", false)]
    #[case(None, 100, "0.50", Some("0.50"), "0.600000", false)]
    #[case(None, 99, "0.50", Some("0.50"), "0.600000", false)]
    #[case(Some(150), 150, "1.00", Some("1.00"), "0.500000", false)]
    #[case(Some(150), 149, "0.50", None, "0.600000", false)]
    #[case(Some(150), 150, "0.50", None, "0.600000", true)]
    #[case(Some(150), 150, "1.00", None, "0.600000", false)]
    #[case(Some(150), 150, "0.50", Some("0.00"), "0.600000", false)]
    fn partial_terms_need_acknowledgement_and_fresh_progress(
        #[case] acknowledged: Option<u64>,
        #[case] timestamp: u64,
        #[case] qty: &str,
        #[case] remaining: Option<&str>,
        #[case] price: &str,
        #[case] expected: bool,
    ) {
        let result = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("TEST.KALSHI".into())
            .quantity(Quantity::from(qty))
            .price(Price::from(price))
            .build();

        let pending = PendingMutation {
            command_id: UUID4::new(),
            started: UnixNanos::from(100),
            dispatched: true,
            acknowledgement: acknowledged.map(|ts| Acknowledgement {
                timestamp: UnixNanos::from(ts),
                remaining: remaining.map(Quantity::from),
            }),
            terms: Some((Quantity::from("0.50"), Price::from("0.600000"))),
        };

        assert_eq!(
            pending.covered(&result, UnixNanos::from(timestamp)),
            expected
        );
    }
}
