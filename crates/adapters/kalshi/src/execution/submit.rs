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

use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, Ordering},
};

use anyhow::Context;
use nautilus_core::{Params, UnixNanos};
use nautilus_live::{ExecutionEventEmitter, execution::failure::CommandFailure};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderDeniedReason,
    identifiers::VenueOrderId,
    instruments::BinaryOption,
    orders::{Order, OrderAny},
};
use rust_decimal::Decimal;
use tokio_util::sync::CancellationToken;

use super::{account, context::PrivateState};
use crate::{
    common::{
        enums::{MarketStatus, MarketType},
        parse::KalshiPriceGrid,
    },
    http::{
        client::KalshiHttpClient,
        error::KalshiHttpError,
        models::{
            BookSide, CreateOrderResponse, Field, FixedPoint, SelfTradePrevention,
            TimeInForce as VenueTimeInForce, VenueError,
        },
        query::CreateOrderRequest,
    },
    websocket::client::KalshiWebSocketClient,
};

pub(super) struct Submission {
    pub(super) order: OrderAny,
    pub(super) position_qty: Option<Decimal>,
    pub(super) definition: BinaryOption,
    pub(super) request: CreateOrderRequest,
    pub(super) http: KalshiHttpClient,
    pub(super) state: Arc<Mutex<PrivateState>>,
    pub(super) emitter: ExecutionEventEmitter,
    pub(super) ready: Arc<AtomicBool>,
    pub(super) ws_client: KalshiWebSocketClient,
    pub(super) refresh: tokio::sync::mpsc::UnboundedSender<()>,
    pub(super) session: String,
    pub(super) cancellation: CancellationToken,
}

impl Submission {
    pub(super) async fn run(mut self) {
        let prepared = tokio::select! {
            biased;
            () = self.cancellation.cancelled() => Err(anyhow::anyhow!("Kalshi submission canceled before dispatch")),
            result = prepare(&self.http, &self.order, &self.definition, &self.request) => result,
        };

        let state = Arc::clone(&self.state);
        let admission = prepared.and_then(|hold| {
            let mut state = state.lock().expect("private state lock is not poisoned");
            self.admit(&mut state, hold)
        });

        if let Err(e) = admission {
            let reason = OrderDeniedReason::ValidationFailed {
                detail: e.to_string(),
            };

            self.deny(&reason.to_string());
            return;
        }

        self.http = transport(
            &self.http,
            &self.state,
            &self.ws_client,
            &self.cancellation,
            &self.session,
        );
        let result = self.http.create_order(&self.request).await;

        self.resolve(result);
    }

    pub(super) fn admit(&mut self, state: &mut PrivateState, hold: Decimal) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.ready.load(Ordering::Acquire)
                && state.session == self.session
                && state.epoch.is_some()
                && state.epoch == self.ws_client.connection_epoch()
                && !state
                    .barrier
                    .lock()
                    .expect("settlement barrier lock")
                    .is_blocked(),
            "Kalshi submission requires current-session readiness"
        );
        expiry(
            &self.order,
            nautilus_core::time::get_atomic_clock_realtime().get_time_ns(),
        )?;
        state.submit(&mut self.order, hold, self.position_qty, &self.emitter)
    }

    pub(super) fn resolve(&self, result: Result<CreateOrderResponse, KalshiHttpError>) -> bool {
        let reason = result.as_ref().err().map(rejection_reason);

        let (outcome, due_post_only) = match result {
            Ok(ack) => (
                self.acknowledge(&ack)
                    .map_err(|e| CommandFailure::ambiguous(e.to_string())),
                false,
            ),
            Err(e) => {
                let due_post_only = post_only_rejection(&e);
                (Err(classify_failure(&e)), due_post_only)
            }
        };

        let resolved = !matches!(&outcome, Err(CommandFailure::Ambiguous(_)));

        if let Err(failure) = outcome {
            match failure {
                CommandFailure::NotSent(_) | CommandFailure::VenueRejected(_) => {
                    self.reject(reason.as_deref().unwrap_or("Order rejected"), due_post_only);
                }
                CommandFailure::Ambiguous(reason) => {
                    self.invalidate();
                    log::warn!("Kalshi submission requires recovery: {reason}");
                }
            }
        }

        if !self.cancellation.is_cancelled() && self.refresh.send(()).is_err() {
            log::warn!("Kalshi submission recovery stopped");
        }

        resolved
    }

    pub(super) fn invalidate(&self) {
        self.ready.store(false, Ordering::Release);
        self.state
            .lock()
            .expect("private state lock is not poisoned")
            .invalidate();
    }

    pub(super) fn reject(&self, reason: &str, due_post_only: bool) {
        let result = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .reject_submission(
                self.order.client_order_id(),
                reason,
                due_post_only,
                &self.emitter,
            );

        if let Err(e) = result {
            log::warn!("Kalshi submission retains venue evidence: {e}");
        }
    }

    pub(super) fn deny(&mut self, reason: &str) {
        let result = self
            .state
            .lock()
            .expect("private state lock is not poisoned")
            .deny_submission(&mut self.order, reason, &self.emitter);

        if let Err(e) = result {
            log::warn!("Kalshi submission denial could not be routed: {e}");
        }
    }

    fn acknowledge(&self, ack: &CreateOrderResponse) -> anyhow::Result<()> {
        anyhow::ensure!(
            ack.client_order_id == Field::Value(self.order.client_order_id().to_string()),
            "Kalshi acknowledgement client identity mismatch"
        );
        let venue_id = VenueOrderId::new_checked(&ack.order_id)?;
        anyhow::ensure!(
            ack.fill_count.0 >= Decimal::ZERO
                && ack.remaining_count.0 >= Decimal::ZERO
                && ack.fill_count.0.normalize().scale() <= 2
                && ack.remaining_count.0.normalize().scale() <= 2
                && ack
                    .fill_count
                    .0
                    .checked_add(ack.remaining_count.0)
                    .is_some_and(|qty| qty <= self.request.count.0),
            "Kalshi acknowledgement counts are invalid"
        );
        let nanos = u64::try_from(ack.ts_ms)?
            .checked_mul(1_000_000)
            .context("Kalshi acknowledgement timestamp overflow")?;
        let ts_event = UnixNanos::from(nanos);
        self.state
            .lock()
            .expect("private state lock is not poisoned")
            .acknowledge(
                self.order.client_order_id(),
                venue_id,
                ts_event,
                &self.emitter,
            )
    }
}

impl Drop for Submission {
    fn drop(&mut self) {
        if self.order.status() == OrderStatus::Initialized {
            let reason = OrderDeniedReason::ValidationFailed {
                detail: "Kalshi submission canceled before dispatch".into(),
            };

            self.deny(&reason.to_string());
        }
    }
}

pub(super) fn transport(
    http: &KalshiHttpClient,
    state: &Arc<Mutex<PrivateState>>,
    ws_client: &KalshiWebSocketClient,
    cancellation: &CancellationToken,
    session: &str,
) -> KalshiHttpClient {
    let state = Arc::clone(state);
    let epoch = state
        .lock()
        .expect("private state lock is not poisoned")
        .epoch;
    let ws_client = ws_client.clone();
    let cancellation = cancellation.clone();
    let session = session.to_owned();
    http.clone()
        .with_dispatch_check(cancellation.clone(), move || {
            let state = state.lock().expect("private state lock is not poisoned");
            !cancellation.is_cancelled()
                && state.session == session
                && epoch.is_some()
                && state.epoch == epoch
                && ws_client.connection_epoch() == epoch
                && !state
                    .barrier
                    .lock()
                    .expect("settlement barrier lock")
                    .is_blocked()
        })
}

pub(super) async fn prepare(
    http: &KalshiHttpClient,
    order: &OrderAny,
    definition: &BinaryOption,
    request: &CreateOrderRequest,
) -> anyhow::Result<Decimal> {
    let market = http.get_market(&request.ticker).await?.market;
    anyhow::ensure!(
        market.ticker == request.ticker
            && market.event_ticker
                == definition
                    .event_id
                    .context("Kalshi event identity missing")?
                    .as_str()
            && market.exchange_index == Field::Value(i32::try_from(request.exchange_index)?)
            && market.market_type == MarketType::Binary
            && market.notional_value_dollars.0 == Decimal::ONE,
        "Kalshi submission market identity mismatch"
    );
    let qty = order.quantity();
    anyhow::ensure!(
        qty.as_decimal() % definition.size_increment.as_decimal() == Decimal::ZERO
            && definition.min_quantity.is_none_or(|min| qty >= min)
            && definition.max_quantity.is_none_or(|max| qty <= max),
        "Kalshi quantity violates instrument limits"
    );
    let now = nautilus_core::time::get_atomic_clock_realtime().get_time_ns();
    anyhow::ensure!(
        market.status == MarketStatus::Active
            && nautilus_core::datetime::try_datetime_to_unix_nanos(market.open_time)? <= now
            && now < nautilus_core::datetime::try_datetime_to_unix_nanos(market.close_time)?,
        "Kalshi market is not open for trading"
    );
    anyhow::ensure!(
        KalshiPriceGrid::new(&market.price_ranges)?.contains(request.price.0),
        "Kalshi price is off the market grid"
    );
    let exchange = http.get_status().await?;
    anyhow::ensure!(
        exchange.exchange_active && exchange.trading_active,
        "Kalshi exchange is not trading"
    );
    let shard = i32::try_from(request.exchange_index)?;

    if let Field::Value(statuses) = exchange.exchange_index_statuses {
        anyhow::ensure!(
            statuses.iter().any(|status| status.exchange_index == shard
                && status.exchange_active
                && status.trading_active),
            "Kalshi shard is not trading"
        );
    } else {
        anyhow::ensure!(
            request.exchange_index == 0,
            "Kalshi shard availability is unknown"
        );
    }

    account::hold(
        http,
        definition,
        order.order_side(),
        order.leaves_qty().as_decimal(),
        request.price.0,
    )
    .await
}

/// Builds the V2 limit request the execution client sends for one order.
///
/// Prices use at most four decimal places and counts at most two. A GTD expiry must be a future
/// whole Unix second on the realtime clock. The returned request is not sent and does not reserve
/// funds.
///
/// # Errors
///
/// Returns an error if the order type, time in force, quantity mode, instruction combination,
/// expiry, ticker, submission parameter, or exact price or count is unsupported.
pub fn request(
    order: &OrderAny,
    params: Option<&Params>,
    shard: u32,
) -> Result<CreateOrderRequest, OrderDeniedReason> {
    if order.order_type() != OrderType::Limit {
        return Err(OrderDeniedReason::UnsupportedOrderType {
            order_type: order.order_type(),
        });
    }

    if !matches!(
        order.time_in_force(),
        TimeInForce::Gtc | TimeInForce::Ioc | TimeInForce::Fok | TimeInForce::Gtd
    ) {
        return Err(OrderDeniedReason::UnsupportedTimeInForce(
            order.time_in_force(),
        ));
    }

    if order.is_reduce_only() && order.time_in_force() != TimeInForce::Ioc {
        return Err(OrderDeniedReason::UnsupportedReduceOnly);
    }

    if order.is_quote_quantity() {
        return Err(OrderDeniedReason::unsupported_quote_quantity());
    }

    let validated = (|| -> anyhow::Result<_> {
        anyhow::ensure!(
            order.display_qty().is_none()
                && !order.is_contingency()
                && order.linked_order_ids().is_none()
                && order.parent_order_id().is_none()
                && !order.is_emulated()
                && order.exec_algorithm_id().is_none(),
            "unsupported Kalshi limit instruction combination"
        );
        anyhow::ensure!(
            !order.is_post_only()
                || matches!(order.time_in_force(), TimeInForce::Gtc | TimeInForce::Gtd),
            "Kalshi post-only requires GTC or GTD"
        );
        let expiration_time = expiry(
            order,
            nautilus_core::time::get_atomic_clock_realtime().get_time_ns(),
        )?;
        let price = order
            .price()
            .context("Kalshi limit price missing")?
            .as_decimal()
            .normalize();
        let count = order.quantity().as_decimal().normalize();
        anyhow::ensure!(
            price > Decimal::ZERO
                && price < Decimal::ONE
                && price.scale() <= 4
                && count > Decimal::ZERO
                && count.scale() <= 2,
            "invalid exact Kalshi quantity or price"
        );
        let (stp, cancel_on_pause) = flags(params)?;

        Ok(CreateOrderRequest {
            ticker: crate::common::parse::parse_ticker(order.instrument_id())?,
            side: if order.order_side() == OrderSide::Buy {
                BookSide::Bid
            } else {
                BookSide::Ask
            },
            count: FixedPoint(count),
            price: FixedPoint(price),
            time_in_force: match order.time_in_force() {
                TimeInForce::Ioc => VenueTimeInForce::ImmediateOrCancel,
                TimeInForce::Fok => VenueTimeInForce::FillOrKill,
                _ => VenueTimeInForce::GoodTillCanceled,
            },
            self_trade_prevention_type: stp,
            subaccount: 0,
            exchange_index: shard,
            client_order_id: Some(order.client_order_id().to_string()),
            expiration_time,
            post_only: Some(order.is_post_only()),
            cancel_order_on_pause: Some(cancel_on_pause),
            reduce_only: Some(order.is_reduce_only()),
        })
    })();

    validated.map_err(|e| OrderDeniedReason::ValidationFailed {
        detail: e.to_string(),
    })
}

fn expiry(order: &OrderAny, now: UnixNanos) -> anyhow::Result<Option<i64>> {
    if order.time_in_force() != TimeInForce::Gtd {
        anyhow::ensure!(order.expire_time().is_none(), "Kalshi expiry requires GTD");
        return Ok(None);
    }

    let expires = order.expire_time().context("Kalshi GTD expiry missing")?;
    anyhow::ensure!(
        expires.as_u64().is_multiple_of(1_000_000_000),
        "Kalshi GTD expiry must use whole Unix seconds"
    );
    anyhow::ensure!(expires > now, "Kalshi GTD expiry must be in the future");
    Ok(Some(i64::try_from(expires.as_u64() / 1_000_000_000)?))
}

fn flags(params: Option<&Params>) -> anyhow::Result<(SelfTradePrevention, bool)> {
    let mut stp = SelfTradePrevention::TakerAtCross;
    let mut cancel_on_pause = false;

    if let Some(params) = params {
        for (key, value) in params {
            match key.as_str() {
                "self_trade_prevention_type" => {
                    stp = serde_json::from_value(value.clone())
                        .context("unsupported Kalshi self-trade prevention")?;
                }
                "cancel_order_on_pause" => {
                    cancel_on_pause = value.as_bool().context("invalid Kalshi cancel-on-pause")?;
                }
                _ => anyhow::bail!("unsupported Kalshi submission parameter"),
            }
        }
    }

    Ok((stp, cancel_on_pause))
}

pub(super) fn classify_failure(error: &KalshiHttpError) -> CommandFailure {
    let reason = error.to_string();
    if error.is_definitely_not_sent() {
        CommandFailure::not_sent(reason)
    } else if matches!(
        venue_code(error),
        Some((400 | 401 | 403, code)) if rejection_code(code).is_some()
    ) {
        CommandFailure::venue_rejected(reason)
    } else {
        CommandFailure::ambiguous(reason)
    }
}

fn venue_code(error: &KalshiHttpError) -> Option<(u16, &str)> {
    let KalshiHttpError::Venue { status, evidence } = error else {
        return None;
    };

    let Field::Value(code) = &evidence.venue.as_ref()?.code else {
        return None;
    };

    Some((*status, code))
}

// Recovery uses this reason to distinguish proven unsent submissions from venue rejections
pub(super) const NOT_SENT_REASON: &str = "Order was not sent";

fn post_only_rejection(error: &KalshiHttpError) -> bool {
    let KalshiHttpError::Venue {
        status: 400,
        evidence,
    } = error
    else {
        return false;
    };

    let Some(venue) = evidence.venue.as_ref() else {
        return false;
    };

    post_only_error(venue)
}

pub(super) fn post_only_error(error: &VenueError) -> bool {
    matches!(&error.code, Field::Value(code) if code == "post_only_cross")
        || matches!(&error.code, Field::Value(code) if code == "invalid_order")
            && matches!(&error.details, Field::Value(details) if details == "post only cross")
}

pub(super) fn rejection_reason(error: &KalshiHttpError) -> String {
    if post_only_rejection(error) {
        return "Post-only order would cross".into();
    }

    let Some((_, code)) = venue_code(error) else {
        return NOT_SENT_REASON.into();
    };

    rejection_code(code).unwrap_or("Order rejected").into()
}

pub(super) fn rejection_code(code: &str) -> Option<&'static str> {
    Some(match code {
        "invalid_order" => "Invalid order",
        "insufficient_balance" => "Insufficient balance",
        "market_closed" => "Market closed",
        "invalid_parameters" => "Invalid parameters",
        "invalid_order_size" => "Invalid order size",
        "invalid_order_price" => "Invalid order price",
        "post_only_cross" => "Post-only order would cross",
        "authentication_error" => "Authentication failed",
        _ => return None,
    })
}

#[cfg(test)]
pub(super) mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_live::task::TaskGroup;
    use nautilus_model::{
        enums::AccountType,
        events::OrderEventAny,
        orders::builder::OrderTestBuilder,
        types::{Currency, Price, Quantity},
    };
    use rstest::rstest;

    use super::*;
    use crate::http::error::ResponseEvidence;

    #[allow(dead_code, reason = "shared key fixtures serve several suites")]
    mod keys {
        include!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/tests/integration/keys.rs"
        ));
    }

    pub(in crate::execution) fn pending() -> (
        Submission,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let fixtures: serde_json::Value =
            serde_json::from_str(include_str!("../../test_data/instruments.json")).unwrap();
        let market =
            serde_json::from_value(fixtures["definitions"]["live"]["market"].clone()).unwrap();
        let event =
            serde_json::from_value(fixtures["definitions"]["live"]["event"].clone()).unwrap();
        let definition =
            crate::http::parse::parse_instrument(&market, &event, UnixNanos::from(1)).unwrap();
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(definition.id)
            .client_order_id("drop-before-send".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.600000"))
            .time_in_force(TimeInForce::Gtc)
            .build();
        let request = request(&order, None, 3).unwrap();

        let transport = crate::config::KalshiDataClientConfig {
            api_key_id: Some("drop-test".into()),
            private_key: Some(keys::rsa_pkcs8().into()),
            ..Default::default()
        };

        let http = KalshiHttpClient::new_authenticated(&transport).unwrap();
        let (ws_client, _) = KalshiWebSocketClient::new(transport).unwrap();
        let (sender, output) = tokio::sync::mpsc::unbounded_channel();

        let mut emitter = ExecutionEventEmitter::new(
            nautilus_core::time::get_atomic_clock_realtime(),
            order.trader_id(),
            "KALSHI-007".into(),
            AccountType::Cash,
            Some(Currency::USD()),
        );

        emitter.set_sender(sender);
        let state = Arc::new(Mutex::new(PrivateState::new()));
        let session = state.lock().unwrap().session.clone();
        let (refresh, _) = tokio::sync::mpsc::unbounded_channel();

        (
            Submission {
                order,
                position_qty: None,
                definition,
                request,
                http,
                state,
                emitter,
                ready: Arc::new(AtomicBool::new(false)),
                ws_client,
                refresh,
                session,
                cancellation: CancellationToken::new(),
            },
            output,
        )
    }

    #[rstest]
    fn ambiguous_submission_invalidates_readiness_until_account_recovery() {
        let (submission, mut output) = pending();
        submission.ready.store(true, Ordering::Release);

        let ticket = {
            let mut state = submission.state.lock().unwrap();
            state.epoch = Some((3, 4));
            state.ledger.begin_refresh()
        };

        let resolved = submission.resolve(Err(KalshiHttpError::Status(503)));

        let (epoch, refresh_valid) = {
            let state = submission.state.lock().unwrap();
            (state.epoch, state.ledger.check_refresh(ticket).is_ok())
        };

        assert!(!resolved);
        assert!(!submission.ready.load(Ordering::Acquire));
        assert_eq!(epoch, None);
        assert!(!refresh_valid);
        assert!(output.try_recv().is_err());
    }

    #[rstest]
    #[cfg_attr(not(all(feature = "simulation", madsim)), tokio::test)]
    #[cfg_attr(all(feature = "simulation", madsim), madsim::test)]
    async fn pending_command_drop_delivers_one_denial(#[values(false, true)] forced: bool) {
        let (submission, mut output) = pending();
        let id = submission.order.client_order_id();
        let state = Arc::clone(&submission.state);
        let tasks = TaskGroup::new();

        if forced {
            let (started, ready) = tokio::sync::oneshot::channel();
            tasks
                .spawn(async move {
                    started.send(()).unwrap();
                    std::future::pending::<()>().await;
                    submission.run().await;
                })
                .unwrap();

            ready.await.unwrap();
            tasks.abort();
            tasks
                .finish_shutdown(
                    std::time::Duration::from_secs(1),
                    std::time::Duration::from_secs(1),
                )
                .await
                .unwrap();
        } else {
            tasks.begin_shutdown();
            assert!(tasks.spawn(submission.run()).is_err());
        }

        let event = output.try_recv().unwrap();

        let ExecutionEvent::Order(OrderEventAny::Denied(denied)) = event else {
            panic!("expected Denied");
        };

        assert_eq!(denied.client_order_id, id);
        assert_eq!(
            denied.reason.as_str(),
            "VALIDATION_FAILED: Kalshi submission canceled before dispatch"
        );
        assert!(output.try_recv().is_err());
        assert_eq!(
            state.lock().unwrap().orders[&id].order.status(),
            OrderStatus::Denied
        );
    }

    #[rstest]
    #[case(400, "insufficient_balance", true)]
    #[case(400, "post_only_cross", true)]
    #[case(403, "authentication_error", true)]
    #[case(400, "duplicate_client_order_id", false)]
    #[case(409, "duplicate_client_order_id", false)]
    #[case(400, "unrecognized_outcome", false)]
    #[case(503, "invalid_order", false)]
    fn failure_classification_requires_definitive_rejection(
        #[case] status: u16,
        #[case] code: &str,
        #[case] rejected: bool,
    ) {
        let error = KalshiHttpError::Venue {
            status,
            evidence: Box::new(ResponseEvidence {
                body: "redacted".into(),
                venue: Some(VenueError {
                    code: Field::Value(code.into()),
                    message: Field::Missing,
                    details: Field::Missing,
                }),
                retry_after: None,
                request_id: None,
            }),
        };

        let reason = error.to_string();
        let expected = if rejected {
            CommandFailure::venue_rejected(reason)
        } else {
            CommandFailure::ambiguous(reason)
        };

        assert_eq!(classify_failure(&error), expected);
    }

    #[rstest]
    #[case(400, "post_only_cross", None, true)]
    #[case(400, "invalid_order", Some("post only cross"), true)]
    #[case(400, "invalid_order", Some("invalid quantity"), false)]
    #[case(400, "invalid_order", None, false)]
    #[case(400, "insufficient_balance", Some("post only cross"), false)]
    #[case(503, "invalid_order", Some("post only cross"), false)]
    #[case(503, "post_only_cross", None, false)]
    fn post_only_rejection_requires_specific_venue_evidence(
        #[case] status: u16,
        #[case] code: &str,
        #[case] details: Option<&str>,
        #[case] expected: bool,
    ) {
        let error = KalshiHttpError::Venue {
            status,
            evidence: Box::new(ResponseEvidence {
                body: "redacted".into(),
                venue: Some(VenueError {
                    code: Field::Value(code.into()),
                    message: Field::Missing,
                    details: details.map_or(Field::Missing, |value| Field::Value(value.into())),
                }),
                retry_after: None,
                request_id: None,
            }),
        };

        assert_eq!(post_only_rejection(&error), expected);

        if expected {
            assert_eq!(rejection_reason(&error), "Post-only order would cross");
        }
    }

    #[rstest]
    #[case(KalshiHttpError::Quota("local quota"), true)]
    #[case(KalshiHttpError::Validation("local validation"), true)]
    #[case(KalshiHttpError::Canceled { dispatched: false }, true)]
    #[case(KalshiHttpError::Canceled { dispatched: true }, false)]
    #[case(KalshiHttpError::Status(503), false)]
    fn local_failure_classification_preserves_dispatch_evidence(
        #[case] error: KalshiHttpError,
        #[case] not_sent: bool,
    ) {
        let reason = error.to_string();
        let expected = if not_sent {
            CommandFailure::not_sent(reason)
        } else {
            CommandFailure::ambiguous(reason)
        };

        assert_eq!(classify_failure(&error), expected);
    }

    #[rstest]
    #[case(TimeInForce::Gtc, "good_till_canceled", None)]
    #[case(TimeInForce::Ioc, "immediate_or_cancel", None)]
    #[case(TimeInForce::Fok, "fill_or_kill", None)]
    #[case(TimeInForce::Gtd, "good_till_canceled", Some(4_102_444_800_i64))]
    fn limit_matrix_encodes_exact_instructions(
        #[case] tif: TimeInForce,
        #[case] wire_tif: &str,
        #[case] expires: Option<i64>,
        #[values(OrderSide::Buy, OrderSide::Sell)] side: OrderSide,
    ) {
        let mut builder = OrderTestBuilder::new(OrderType::Limit);
        builder
            .instrument_id("EVENT-MARKET.KALSHI".into())
            .client_order_id("matrix-order".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.4321"))
            .side(side)
            .time_in_force(tif)
            .post_only(tif == TimeInForce::Gtd)
            .reduce_only(tif == TimeInForce::Ioc);

        if let Some(expires) = expires {
            builder.expire_time(UnixNanos::from(expires as u64 * 1_000_000_000));
        }

        let order = builder.build();
        let wire = serde_json::to_value(request(&order, None, 3).unwrap()).unwrap();

        let mut expected = serde_json::from_str::<serde_json::Value>(include_str!(
            "../../test_data/rest_requests.json"
        ))
        .unwrap()["limit_matrix"]
            .clone();
        expected["side"] = serde_json::json!(if side == OrderSide::Buy { "bid" } else { "ask" });
        expected["time_in_force"] = serde_json::json!(wire_tif);
        expected["post_only"] = serde_json::json!(tif == TimeInForce::Gtd);
        expected["reduce_only"] = serde_json::json!(tif == TimeInForce::Ioc);
        if let Some(expires) = expires {
            expected["expiration_time"] = expires.into();
        }

        assert_eq!(wire, expected);
    }

    #[rstest]
    #[case(1_000_000_000, 999_999_999, Ok(Some(1)))]
    #[case(
        1_000_000_000,
        1_000_000_000,
        Err("Kalshi GTD expiry must be in the future")
    )]
    #[case(
        1_000_000_000,
        1_000_000_001,
        Err("Kalshi GTD expiry must be in the future")
    )]
    #[case(1_000_000_001, 0, Err("Kalshi GTD expiry must use whole Unix seconds"))]
    fn gtd_expiry_is_exact_and_future(
        #[case] expires: u64,
        #[case] now: u64,
        #[case] expected: Result<Option<i64>, &str>,
    ) {
        let order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id("EVENT-MARKET.KALSHI".into())
            .quantity(Quantity::from("1.25"))
            .price(Price::from("0.4321"))
            .time_in_force(TimeInForce::Gtd)
            .expire_time(UnixNanos::from(expires))
            .build();
        assert_eq!(
            expiry(&order, UnixNanos::from(now)).map_err(|e| e.to_string()),
            expected.map_err(str::to_owned)
        );
    }
}
