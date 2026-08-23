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

use std::{sync::Arc, time::Duration};

use nautilus_common::live::{get_runtime, task::TaskHandles};
use nautilus_core::{UUID4, time::AtomicTime};
use nautilus_live::{ExecutionEventEmitter, execution::failure::CommandFailure};
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderFilled, OrderUpdated},
    identifiers::{AccountId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;

use super::{
    cancellations::execute_deferred_cancel,
    identity::{OrderIdentity, OrderIdentityRegistry},
    order_fill_tracker::{BufferedFill, FillCorrectionMetadata, OrderFillTrackerMap},
    pending::{PendingCancelTracker, PendingSubmitTracker},
    reconciliation::cap_order_report_filled_qty,
    reports::get_pusd_currency,
    submitter::{
        OrderSubmitter, SubmitResponseOutcome, is_fok_unfilled, submit_response_outcome,
        submit_response_unknown_reason, submit_response_venue_order_id,
    },
    types::{BatchLimitOrderContext, classify_http_command_failure},
};
use crate::http::{
    error::{sanitize_error_text, strategy_rejection_reason},
    query::{OrderResponse, OrderResponseStatus},
};

#[expect(clippy::too_many_arguments)]
pub(super) async fn handle_batch_order_responses(
    responses: Vec<OrderResponse>,
    batch_orders: Vec<BatchLimitOrderContext>,
    expected_venue_order_ids: Vec<VenueOrderId>,
    submitter: &OrderSubmitter,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &Arc<OrderIdentityRegistry>,
    pending_submits: &PendingSubmitTracker,
    pending_cancels: &PendingCancelTracker,
    pending_tasks: &Arc<TaskHandles>,
    account_id: AccountId,
) {
    let response_len = responses.len();
    let order_len = batch_orders.len();

    if response_len != order_len {
        log::warn!(
            "Batch submit response length ({response_len}) does not match order count ({order_len})"
        );
    }

    let mut follow_ups = Vec::new();

    for ((batch_order, response), expected_venue_order_id) in batch_orders
        .iter()
        .zip(responses)
        .zip(&expected_venue_order_ids)
    {
        let (deferred_cancel, fok_order_id) = if submit_response_outcome(
            &response,
            batch_order.order.time_in_force() == TimeInForce::Fok,
        ) == SubmitResponseOutcome::Unknown
        {
            (
                handle_unknown_submit_result(
                    &batch_order.order,
                    *expected_venue_order_id,
                    &submit_response_unknown_reason(&response, false, *expected_venue_order_id),
                    None,
                    emitter,
                    clock,
                    fill_tracker,
                    order_identities,
                    pending_submits,
                    pending_cancels,
                    account_id,
                    batch_order.size_precision,
                    batch_order.price_precision,
                ),
                None,
            )
        } else {
            let fok_order_id = fok_check_order_id(&response, batch_order.order.time_in_force());
            let deferred_cancel = handle_order_response(
                Ok(response),
                &batch_order.order,
                emitter,
                clock,
                fill_tracker,
                order_identities,
                pending_cancels,
                account_id,
                batch_order.size_precision,
                batch_order.price_precision,
            );

            (deferred_cancel, fok_order_id)
        };

        if deferred_cancel.is_some() || fok_order_id.is_some() {
            follow_ups.push((batch_order.clone(), deferred_cancel, fok_order_id));
        }
    }

    for (batch_order, expected_venue_order_id) in batch_orders
        .iter()
        .zip(expected_venue_order_ids)
        .skip(response_len)
    {
        let deferred_cancel = handle_unknown_submit_result(
            &batch_order.order,
            expected_venue_order_id,
            "batch response omitted order",
            None,
            emitter,
            clock,
            fill_tracker,
            order_identities,
            pending_submits,
            pending_cancels,
            account_id,
            batch_order.size_precision,
            batch_order.price_precision,
        );

        if deferred_cancel.is_some() {
            follow_ups.push((batch_order.clone(), deferred_cancel, None));
        }
    }

    for (batch_order, deferred_cancel, fok_order_id) in follow_ups {
        let submitter = submitter.clone();
        let emitter = emitter.clone();
        let fill_tracker = fill_tracker.clone();
        let order_identities = order_identities.clone();
        let pending_cancels = pending_cancels.clone();

        let handle = get_runtime().spawn(async move {
            if let Some((order_id_str, venue_order_id)) = deferred_cancel {
                execute_deferred_cancel(
                    &submitter,
                    &batch_order.order,
                    &order_id_str,
                    venue_order_id,
                    &emitter,
                    &pending_cancels,
                    clock,
                )
                .await;
            }

            if let Some(order_id) = fok_order_id {
                check_fok_status(
                    &submitter,
                    &order_id,
                    &batch_order.order,
                    &fill_tracker,
                    &order_identities,
                    &emitter,
                    account_id,
                    batch_order.size_precision,
                    batch_order.price_precision,
                    clock,
                )
                .await;
            }
        });
        pending_tasks.push(handle);
    }
}

pub(super) fn reject_submit_order(
    order: &OrderAny,
    reason: &str,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    pending_cancels: &PendingCancelTracker,
) {
    let reason = strategy_rejection_reason(reason);

    let ts_now = clock.get_time_ns();

    emitter.emit_order_rejected(order, &reason, ts_now, is_post_only_crossing(&reason));
    pending_cancels.remove(&order.client_order_id());
}

#[expect(clippy::too_many_arguments)]
pub(super) fn emit_market_order_submitted(
    order: &mut OrderAny,
    is_quote_qty: bool,
    side: OrderSide,
    amount: Quantity,
    expected_base_qty: Decimal,
    update_quantity: bool,
    size_precision: u8,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    emitter.emit_order_submitted(order);

    if !update_quantity || expected_base_qty.is_zero() {
        return;
    }

    let Ok(base_qty) = Quantity::from_decimal_dp(expected_base_qty, size_precision) else {
        return;
    };

    if base_qty == order.quantity() && !order.is_quote_quantity() {
        return;
    }

    log::debug!(
        "Normalized {} {side:?} {} quantity {amount} to signed base quantity {base_qty}",
        order.instrument_id(),
        if is_quote_qty { "quote" } else { "base" },
    );

    let ts_now = clock.get_time_ns();
    let updated = OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        base_qty,
        UUID4::new(),
        ts_now,
        ts_now,
        false,
        order.venue_order_id(),
        order.account_id(),
        order.price(),
        None,
        None,
        false,
    );

    let event = OrderEventAny::Updated(updated);
    emitter.send_order_event(event.clone());

    if let Err(e) = order.apply(event) {
        log::error!("Failed to apply signed base-quantity OrderUpdated: {e}");
    }
}

#[expect(clippy::too_many_arguments)]
pub(super) async fn handle_single_order_response(
    result: crate::http::error::Result<OrderResponse>,
    batch_order: BatchLimitOrderContext,
    expected_venue_order_id: VenueOrderId,
    submitter: &OrderSubmitter,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    pending_submits: &PendingSubmitTracker,
    pending_cancels: &PendingCancelTracker,
    account_id: AccountId,
) {
    match result {
        Ok(response) => {
            let fok_order_id = fok_check_order_id(&response, batch_order.order.time_in_force());
            if let Some((order_id_str, venue_order_id)) = handle_order_response(
                Ok(response),
                &batch_order.order,
                emitter,
                clock,
                fill_tracker,
                order_identities,
                pending_cancels,
                account_id,
                batch_order.size_precision,
                batch_order.price_precision,
            ) {
                execute_deferred_cancel(
                    submitter,
                    &batch_order.order,
                    &order_id_str,
                    venue_order_id,
                    emitter,
                    pending_cancels,
                    clock,
                )
                .await;
            }

            if let Some(order_id) = fok_order_id {
                check_fok_status(
                    submitter,
                    &order_id,
                    &batch_order.order,
                    fill_tracker,
                    order_identities,
                    emitter,
                    account_id,
                    batch_order.size_precision,
                    batch_order.price_precision,
                    clock,
                )
                .await;
            }
        }
        Err(e) => match classify_http_command_failure(&e) {
            CommandFailure::Ambiguous(reason) => {
                if let Some((order_id_str, venue_order_id)) = handle_unknown_submit_result(
                    &batch_order.order,
                    expected_venue_order_id,
                    &reason,
                    None,
                    emitter,
                    clock,
                    fill_tracker,
                    order_identities,
                    pending_submits,
                    pending_cancels,
                    account_id,
                    batch_order.size_precision,
                    batch_order.price_precision,
                ) {
                    execute_deferred_cancel(
                        submitter,
                        &batch_order.order,
                        &order_id_str,
                        venue_order_id,
                        emitter,
                        pending_cancels,
                        clock,
                    )
                    .await;
                }
            }
            CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                reject_submit_order(&batch_order.order, &reason, emitter, clock, pending_cancels);
            }
        },
    }
}

#[expect(clippy::too_many_arguments)]
pub(super) fn handle_unknown_submit_result(
    order: &OrderAny,
    expected_venue_order_id: VenueOrderId,
    reason: &str,
    fill_tracker_quantity: Option<Quantity>,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    pending_submits: &PendingSubmitTracker,
    pending_cancels: &PendingCancelTracker,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) -> Option<(String, VenueOrderId)> {
    log::warn!(
        "Submit outcome unknown for {}: {reason}. Tracking expected venue order ID {}",
        order.client_order_id(),
        expected_venue_order_id
    );

    order_identities
        .register_order_identity(expected_venue_order_id, OrderIdentity::from_order(order));
    pending_submits.insert(expected_venue_order_id, order.client_order_id());

    drain_pending_reports_for_known_order(
        order,
        expected_venue_order_id,
        emitter,
        clock,
        fill_tracker,
        order_identities,
        fill_tracker_quantity,
        account_id,
        size_precision,
        price_precision,
    );

    if pending_cancels.contains(&order.client_order_id()) {
        let order_id_str = expected_venue_order_id.to_string();
        return Some((order_id_str, expected_venue_order_id));
    }

    None
}

#[expect(clippy::too_many_arguments)]
pub(super) fn drain_pending_reports_for_known_order(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    fill_tracker_quantity: Option<Quantity>,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) {
    let buffered = fill_tracker.take_pending_reports(&venue_order_id);
    if buffered.is_empty() {
        accept_order_with_pending_fills(
            order,
            venue_order_id,
            emitter,
            clock,
            fill_tracker,
            order_identities,
            fill_tracker_quantity,
            account_id,
            size_precision,
            price_precision,
        );
        return;
    }

    let should_register = buffered
        .iter()
        .any(|report| report.order_status != OrderStatus::Rejected);

    let buffered_fills = if should_register {
        let tracker_quantity = fill_tracker_quantity.unwrap_or_else(|| order.quantity());
        fill_tracker.register_and_take_pending_fills(
            venue_order_id,
            Some(order.client_order_id()),
            tracker_quantity,
            order.order_side(),
        )
    } else {
        Vec::new()
    };

    // The unknown-submit path did not emit OrderAccepted at submit; synthesize it once now
    // that buffered activity confirms the venue accepted the order, before terminal events.
    if should_register {
        let ts_event = buffered
            .iter()
            .map(|report| report.ts_last)
            .min()
            .unwrap_or_else(|| clock.get_time_ns());

        if order_identities.mark_accepted(venue_order_id) {
            emitter.emit_order_accepted(order, venue_order_id, ts_event);
        }
    }

    emit_drained_activity(
        order,
        venue_order_id,
        buffered_fills,
        &buffered,
        fill_tracker,
        emitter,
        clock,
    );
}

#[expect(clippy::too_many_arguments)]
pub(super) fn accept_order_with_pending_fills(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    fill_tracker_quantity: Option<Quantity>,
    _account_id: AccountId,
    _size_precision: u8,
    _price_precision: u8,
) {
    // Accept only once a buffered fill proves the venue took the order
    let tracker_quantity = fill_tracker_quantity.unwrap_or_else(|| order.quantity());
    let Some(fills) = fill_tracker.register_and_take_pending_fills_if_buffered(
        venue_order_id,
        Some(order.client_order_id()),
        tracker_quantity,
        order.order_side(),
    ) else {
        return;
    };

    let ts_event = fills
        .iter()
        .map(|fill| fill.report.ts_event)
        .min()
        .unwrap_or_else(|| clock.get_time_ns());

    if order_identities.mark_accepted(venue_order_id) {
        emitter.emit_order_accepted(order, venue_order_id, ts_event);
    }

    emit_drained_activity(
        order,
        venue_order_id,
        fills,
        &[],
        fill_tracker,
        emitter,
        clock,
    );
}

#[expect(clippy::too_many_arguments)]
pub(super) fn handle_order_response(
    result: crate::http::error::Result<OrderResponse>,
    order: &OrderAny,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    pending_cancels: &PendingCancelTracker,
    _account_id: AccountId,
    _size_precision: u8,
    _price_precision: u8,
) -> Option<(String, VenueOrderId)> {
    match result {
        Ok(response) => {
            if let Some(reason) = fok_rejection_reason(&response, order.time_in_force()) {
                reject_submit_order(order, reason, emitter, clock, pending_cancels);
                return None;
            }

            if response.success {
                if let Some(venue_order_id) = submit_response_venue_order_id(&response) {
                    let decision = order_response_decision(response.status);
                    let ts_now = clock.get_time_ns();

                    order_identities
                        .register_order_identity(venue_order_id, OrderIdentity::from_order(order));
                    if decision.emit_accepted && order_identities.mark_accepted(venue_order_id) {
                        emitter.emit_order_accepted(order, venue_order_id, ts_now);
                    }

                    let fills = fill_tracker.register_and_take_pending_fills(
                        venue_order_id,
                        Some(order.client_order_id()),
                        order.quantity(),
                        order.order_side(),
                    );

                    // The register above precedes this drain, so a racing report can't be orphaned
                    let buffered = fill_tracker.take_pending_reports(&venue_order_id);
                    let activity_proves_accepted = !fills.is_empty()
                        || buffered.iter().any(|report| {
                            matches!(
                                report.order_status,
                                OrderStatus::Accepted
                                    | OrderStatus::PartiallyFilled
                                    | OrderStatus::Filled
                                    | OrderStatus::Canceled
                                    | OrderStatus::Expired
                            )
                        });

                    if !decision.emit_accepted
                        && activity_proves_accepted
                        && order_identities.mark_accepted(venue_order_id)
                    {
                        let ts_accepted = fills
                            .iter()
                            .map(|fill| fill.report.ts_event)
                            .chain(buffered.iter().map(|report| report.ts_last))
                            .min()
                            .unwrap_or(ts_now);
                        emitter.emit_order_accepted(order, venue_order_id, ts_accepted);
                    }

                    emit_drained_activity(
                        order,
                        venue_order_id,
                        fills,
                        &buffered,
                        fill_tracker,
                        emitter,
                        clock,
                    );

                    if pending_cancels.contains(&order.client_order_id()) {
                        log::debug!(
                            "Order {} has pending cancel, issuing deferred cancel for {}",
                            order.client_order_id(),
                            venue_order_id
                        );
                        return Some((venue_order_id.to_string(), venue_order_id));
                    }
                } else if let Some(reason) = response
                    .error_msg
                    .filter(|reason| !reason.trim().is_empty())
                {
                    // Batch endpoint reports a rejected leg as success=true with an empty orderID; reason in error_msg
                    reject_submit_order(order, &reason, emitter, clock, pending_cancels);
                } else {
                    log::warn!(
                        "Order accepted but no order_id returned for {}",
                        order.client_order_id()
                    );
                }
            } else {
                let reason = response
                    .error_msg
                    .unwrap_or_else(|| "unknown error".to_string());
                reject_submit_order(order, &reason, emitter, clock, pending_cancels);
            }
        }
        Err(e) => match classify_http_command_failure(&e) {
            CommandFailure::Ambiguous(reason) => {
                log::warn!(
                    "Submit outcome unknown for {} without an expected venue order ID: {reason}",
                    order.client_order_id()
                );
            }
            CommandFailure::NotSent(reason) | CommandFailure::VenueRejected(reason) => {
                reject_submit_order(order, &reason, emitter, clock, pending_cancels);
            }
        },
    }

    None
}

struct OrderResponseDecision {
    emit_accepted: bool,
    poll_fok: bool,
}

fn order_response_decision(status: Option<OrderResponseStatus>) -> OrderResponseDecision {
    match status {
        Some(OrderResponseStatus::Matched) => OrderResponseDecision {
            emit_accepted: true,
            poll_fok: false,
        },
        Some(OrderResponseStatus::Delayed) => OrderResponseDecision {
            emit_accepted: false,
            poll_fok: true,
        },
        Some(OrderResponseStatus::Live | OrderResponseStatus::Unmatched) | None => {
            OrderResponseDecision {
                emit_accepted: true,
                poll_fok: true,
            }
        }
    }
}

pub(super) fn fok_check_order_id(
    response: &OrderResponse,
    time_in_force: TimeInForce,
) -> Option<String> {
    let decision = order_response_decision(response.status);
    submit_response_venue_order_id(response)
        .filter(|_| {
            response.success
                && time_in_force == TimeInForce::Fok
                && decision.poll_fok
                && fok_rejection_reason(response, time_in_force).is_none()
        })
        .map(|venue_order_id| venue_order_id.to_string())
}

fn fok_rejection_reason(response: &OrderResponse, time_in_force: TimeInForce) -> Option<&str> {
    if !response.success || response.status.is_some() || time_in_force != TimeInForce::Fok {
        return None;
    }

    response
        .error_msg
        .as_deref()
        .filter(|reason| is_fok_unfilled(reason))
}

pub(crate) fn is_post_only_crossing(reason: &str) -> bool {
    reason == "invalid post-only order: order crosses book"
}

/// Emits an `OrderFilled` event for a drained own-order fill.
///
/// When the fill drives cumulative BUY fills past the registered quantity (a marketable BUY that
/// filled below its limit returns more shares than its nominal size), an `OrderUpdated` raising the
/// quantity to the actual fill is emitted first, so the engine does not reject the fill as an
/// overfill.
fn emit_drained_fill(
    order: &OrderAny,
    fill: &FillReport,
    correction: Option<&FillCorrectionMetadata>,
    fill_tracker: &OrderFillTrackerMap,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    let filled = OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        fill.venue_order_id,
        emitter.account_id(),
        fill.trade_id,
        order.order_side(),
        order.order_type(),
        fill.last_qty,
        fill.last_px,
        get_pusd_currency(),
        fill.liquidity_side,
        UUID4::new(),
        fill.ts_event,
        clock.get_time_ns(),
        false,
        fill.venue_position_id,
        Some(fill.commission),
        correction.and_then(|metadata| metadata.info.clone()),
    );
    fill_tracker.emit_buffered_fill(filled, correction, |filled, new_qty| {
        if let Some(new_qty) = new_qty {
            emit_buy_overfill_update(order, fill.venue_order_id, new_qty, emitter, clock);
        }
        emitter.send_order_event(OrderEventAny::Filled(filled));
    });
}

fn emit_drained_activity(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    fills: Vec<BufferedFill>,
    reports: &[OrderStatusReport],
    fill_tracker: &OrderFillTrackerMap,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    let has_unconfirmed_fill = fills.iter().any(|fill| {
        fill.correction.as_ref().is_some_and(|metadata| {
            !metadata.is_confirmed && !fill_tracker.is_trade_confirmed(&metadata.correction_key)
        })
    });

    for fill in fills {
        emit_drained_fill(
            order,
            &fill.report,
            fill.correction.as_ref(),
            fill_tracker,
            emitter,
            clock,
        );
    }

    for report in reports {
        emit_drained_order_report(order, report, emitter);
    }

    let has_filled = reports
        .iter()
        .any(|report| report.order_status == OrderStatus::Filled);
    let has_unfilled_terminal = reports.iter().any(|report| {
        matches!(
            report.order_status,
            OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected
        )
    });
    let identity = OrderIdentity::from_order(order);
    let is_taker_terminal = matches!(identity.time_in_force, TimeInForce::Fok | TimeInForce::Ioc);

    if has_unconfirmed_fill || has_unfilled_terminal || (!has_filled && !is_taker_terminal) {
        return;
    }

    if identity.requires_terminal_quantity_normalization() || has_filled {
        if let Some(quantity) = fill_tracker.check_terminal_quantity_normalization(&venue_order_id)
        {
            emit_terminal_quantity_update(order, venue_order_id, quantity, emitter, clock);
        }
    } else if identity.time_in_force == TimeInForce::Ioc
        && let Some(remainder) = fill_tracker.take_terminal_ioc_remainder(&venue_order_id)
    {
        log::debug!(
            "Closing terminal IOC order {venue_order_id} as Canceled after buffered fills \
             (unfilled remainder={remainder})"
        );
        emitter.emit_order_canceled(order, Some(venue_order_id), clock.get_time_ns());
    }
}

/// Emits an `OrderUpdated` raising the order quantity to the actual BUY fill, before the fill.
fn emit_buy_overfill_update(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    new_qty: Quantity,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    log::warn!(
        "Raising {} BUY quantity to {new_qty} to absorb a marketable fill above the nominal size",
        order.client_order_id(),
    );

    let ts_now = clock.get_time_ns();
    let updated = OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        new_qty,
        UUID4::new(),
        ts_now,
        ts_now,
        false,
        Some(venue_order_id),
        order.account_id(),
        None,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

/// Emits an order-only reconciliation update which cannot change strategy position.
fn emit_terminal_quantity_update(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    quantity: Quantity,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    let ts_now = clock.get_time_ns();
    let updated = OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        quantity,
        UUID4::new(),
        ts_now,
        ts_now,
        true,
        Some(venue_order_id),
        order.account_id(),
        None,
        None,
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

/// Emits the lifecycle event for a drained own-order status report.
///
/// Acceptance is emitted on the submit happy path and fills arrive as `OrderFilled` from the
/// drained fill buffer, so `Accepted` / `PartiallyFilled` / `Filled` reports produce no event
/// here; only terminal transitions (cancel, expire, reject) convert.
fn emit_drained_order_report(
    order: &OrderAny,
    report: &OrderStatusReport,
    emitter: &ExecutionEventEmitter,
) {
    match report.order_status {
        OrderStatus::Canceled => {
            emitter.emit_order_canceled(order, Some(report.venue_order_id), report.ts_last);
        }
        OrderStatus::Expired => {
            emitter.emit_order_expired(order, Some(report.venue_order_id), report.ts_last);
        }
        OrderStatus::Rejected => {
            let reason = report
                .cancel_reason
                .clone()
                .unwrap_or_else(|| "REJECTED".to_string());

            let reason = sanitize_error_text(&reason);

            emitter.emit_order_rejected(
                order,
                &reason,
                report.ts_last,
                is_post_only_crossing(&reason),
            );
        }
        _ => {}
    }
}

#[expect(clippy::too_many_arguments)]
pub(super) async fn check_fok_status(
    submitter: &OrderSubmitter,
    order_id: &str,
    order: &OrderAny,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
    clock: &'static AtomicTime,
) {
    const FOK_CHECK_DELAY: Duration = Duration::from_secs(5);

    tokio::time::sleep(FOK_CHECK_DELAY).await;

    let venue_order_id = VenueOrderId::from(order_id);
    if fill_tracker.has_fills_or_settled(&venue_order_id) {
        return;
    }

    log::warn!("FOK order {order_id} unresolved after 5s, checking REST status");

    let venue_order = match submitter.get_order(order_id).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            log::debug!("FOK order {order_id} not found (empty response), WS will reconcile");
            return;
        }
        Err(e) => {
            log::warn!("FOK status check failed for {order_id}: {e}");
            return;
        }
    };

    if fill_tracker.has_fills_or_settled(&venue_order_id) {
        return;
    }

    let order_status = OrderStatus::from(venue_order.status);
    let ts_now = clock.get_time_ns();

    if matches!(
        order_status,
        OrderStatus::Accepted
            | OrderStatus::PartiallyFilled
            | OrderStatus::Filled
            | OrderStatus::Canceled
            | OrderStatus::Expired
    ) && order_identities.mark_accepted(venue_order_id)
    {
        emitter.emit_order_accepted(order, venue_order_id, ts_now);
    }

    match order_status {
        OrderStatus::Rejected => {
            log::debug!("FOK order {order_id} resolved via REST as Rejected");
            emitter.emit_order_rejected(order, "FOK order unfilled", ts_now, false);
        }
        OrderStatus::Canceled => {
            log::debug!("FOK order {order_id} resolved via REST as Canceled");
            emitter.emit_order_canceled(order, Some(venue_order_id), ts_now);
        }
        OrderStatus::Expired => {
            log::debug!("FOK order {order_id} resolved via REST as Expired");
            emitter.emit_order_expired(order, Some(venue_order_id), ts_now);
        }
        OrderStatus::Filled => {
            let quantity = Quantity::from_decimal_dp(venue_order.original_size, size_precision)
                .unwrap_or_else(|_| Quantity::zero(size_precision));
            let filled_qty = Quantity::from_decimal_dp(venue_order.size_matched, size_precision)
                .unwrap_or_else(|_| Quantity::zero(size_precision));
            let confirmed_filled = fill_tracker
                .get_cumulative_filled(&venue_order_id)
                .unwrap_or_else(|| Quantity::zero(size_precision));
            let price = Price::from_decimal_dp(venue_order.price, price_precision)
                .unwrap_or_else(|_| Price::zero(price_precision));

            let mut report = OrderStatusReport::new(
                account_id,
                order.instrument_id(),
                Some(order.client_order_id()),
                venue_order_id,
                order.order_side(),
                OrderType::Limit,
                TimeInForce::Fok,
                order_status,
                quantity,
                filled_qty,
                ts_now,
                ts_now,
                ts_now,
                None,
            );
            report.price = Some(price);
            cap_order_report_filled_qty(&mut report, confirmed_filled, None);

            log::debug!(
                "FOK order {order_id} resolved via REST as Filled; deferring fill quantity until confirmation"
            );
            emitter.send_order_status_report(report);
        }
        OrderStatus::Accepted | OrderStatus::PartiallyFilled => {}
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_core::{UnixNanos, collections::AtomicMap};
    use nautilus_model::{
        enums::{AccountType, LiquiditySide},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol, TradeId, TraderId},
        instruments::{Instrument, InstrumentAny},
        orders::{LimitOrder, MarketOrder, Order, stubs::TestOrderEventStubs},
        types::{Currency, Money},
    };
    use rstest::rstest;
    use rust_decimal::Decimal;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{
            PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOutcome,
            PolymarketTradeStatus,
        },
        execution::reconciliation::FillReportScope,
        http::{
            models::GammaMarket,
            parse::{create_instrument_from_def, parse_gamma_market},
        },
        websocket::{
            dispatch::{WsDispatchContext, WsDispatchState, dispatch_user_message},
            messages::{PolymarketUserOrder, PolymarketUserTrade, UserWsMessage},
        },
    };

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("failed to read test data");
        serde_json::from_str(&content).expect("failed to parse test data")
    }

    fn test_instrument() -> InstrumentAny {
        let market: GammaMarket = load("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap()
    }

    fn bind_instrument_to_trade(
        instrument: &mut InstrumentAny,
        trade: &crate::http::models::PolymarketTradeReport,
    ) {
        let InstrumentAny::BinaryOption(binary) = instrument else {
            panic!("expected binary option test instrument");
        };
        binary.id =
            InstrumentId::from(format!("{}-{}.POLYMARKET", trade.market, trade.asset_id).as_str());
        binary.raw_symbol = Symbol::from(trade.asset_id.as_str());
        binary.outcome = Some(trade.outcome.inner());
        binary.info = Some(
            serde_json::from_value(serde_json::json!({
                "condition_id": trade.market.as_str(),
                "token_id": trade.asset_id.as_str(),
            }))
            .expect("valid instrument binding metadata"),
        );
    }

    fn test_emitter() -> (
        ExecutionEventEmitter,
        tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    ) {
        let mut emitter = ExecutionEventEmitter::new(
            nautilus_core::time::get_atomic_clock_realtime(),
            TraderId::from("TESTER-001"),
            AccountId::from("POLY-001"),
            AccountType::Cash,
            Some(Currency::pUSD()),
        );
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        (emitter, receiver)
    }

    fn test_limit_order(client_order_id: &str, instrument_id: InstrumentId) -> OrderAny {
        OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::new(10.0, 0),
            Price::new(0.50, 4),
            TimeInForce::Gtc,
            None,
            false,
            false,
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
            UnixNanos::default(),
        ))
    }

    fn test_quote_market_order(client_order_id: &str, instrument_id: InstrumentId) -> OrderAny {
        OrderAny::Market(MarketOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from(client_order_id),
            OrderSide::Buy,
            Quantity::new(10.0, 0),
            TimeInForce::Ioc,
            UUID4::new(),
            UnixNanos::default(),
            false,
            true,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ))
    }

    fn successful_order_response(
        venue_order_id: VenueOrderId,
        status: Option<OrderResponseStatus>,
    ) -> OrderResponse {
        OrderResponse {
            success: true,
            order_id: Some(venue_order_id.to_string()),
            status,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: None,
        }
    }

    #[rstest]
    #[case::constructed_live(Some(OrderResponseStatus::Live), true, true)]
    #[case::constructed_matched(Some(OrderResponseStatus::Matched), true, false)]
    #[case::delayed(Some(OrderResponseStatus::Delayed), false, true)]
    #[case::constructed_unmatched(Some(OrderResponseStatus::Unmatched), true, true)]
    #[case::constructed_absent(None, true, true)]
    fn test_order_response_decision(
        #[case] status: Option<OrderResponseStatus>,
        #[case] expected_accept: bool,
        #[case] expected_check_fok: bool,
    ) {
        let decision = order_response_decision(status);

        assert_eq!(decision.emit_accepted, expected_accept);
        assert_eq!(decision.poll_fok, expected_check_fok);
    }

    #[rstest]
    #[case::absent_status(true, Some("0xfok"), None, TimeInForce::Fok, Some("0xfok"))]
    #[case::matched(
        true,
        Some("0xfok"),
        Some(OrderResponseStatus::Matched),
        TimeInForce::Fok,
        None
    )]
    #[case::failed(false, Some("0xfok"), None, TimeInForce::Fok, None)]
    #[case::empty_id(true, Some(""), None, TimeInForce::Fok, None)]
    #[case::whitespace_id(true, Some(" \t"), None, TimeInForce::Fok, None)]
    #[case::non_ascii_id(true, Some("é"), None, TimeInForce::Fok, None)]
    #[case::non_fok(true, Some("0xgtc"), None, TimeInForce::Gtc, None)]
    fn test_fok_check_order_id(
        #[case] success: bool,
        #[case] order_id: Option<&str>,
        #[case] status: Option<OrderResponseStatus>,
        #[case] time_in_force: TimeInForce,
        #[case] expected: Option<&str>,
    ) {
        let mut response = successful_order_response(VenueOrderId::from("0xunused"), status);
        response.success = success;
        response.order_id = order_id.map(ToString::to_string);

        assert_eq!(
            fok_check_order_id(&response, time_in_force).as_deref(),
            expected,
        );
    }

    #[rstest]
    fn test_fok_unfilled_error_skips_check() {
        let mut response = successful_order_response(VenueOrderId::from("0xfok"), None);
        response.error_msg = Some(
            "order couldn't be fully filled. FOK orders are fully filled or killed.".to_string(),
        );

        assert_eq!(fok_check_order_id(&response, TimeInForce::Fok), None);
    }

    #[rstest]
    #[case::constructed_live(Some(OrderResponseStatus::Live))]
    #[case::constructed_matched(Some(OrderResponseStatus::Matched))]
    #[case::constructed_unmatched(Some(OrderResponseStatus::Unmatched))]
    #[case::constructed_absent(None)]
    fn test_accepting_order_response_status_emits_accepted(
        #[case] status: Option<OrderResponseStatus>,
    ) {
        let instrument = test_instrument();
        let mut order = test_limit_order("O-CONFIRMED", instrument.id());
        order
            .apply(TestOrderEventStubs::submitted(
                &order,
                AccountId::from("POLY-001"),
            ))
            .unwrap();
        let venue_order_id = VenueOrderId::from("0xconfirmed");
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let order_identities = OrderIdentityRegistry::default();
        let pending_cancels = PendingCancelTracker::default();

        let deferred = handle_order_response(
            Ok(successful_order_response(venue_order_id, status)),
            &order,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_cancels,
            AccountId::from("POLY-001"),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(event @ OrderEventAny::Accepted(_)) => {
                let OrderEventAny::Accepted(accepted) = &event else {
                    unreachable!()
                };
                assert_eq!(accepted.client_order_id, order.client_order_id());
                assert_eq!(accepted.venue_order_id, venue_order_id);
                event
            }
            other => panic!("expected accepted event, was {other:?}"),
        };
        order.apply(accepted).unwrap();

        assert!(deferred.is_none());
        assert!(order_identities.get(&venue_order_id).is_some());
        assert!(fill_tracker.contains(&venue_order_id));
        assert_eq!(order.status(), OrderStatus::Accepted);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_captured_delayed_response_stays_submitted_and_registers_tracking() {
        let instrument = test_instrument();
        let mut order = test_limit_order("O-DELAYED", instrument.id());
        order
            .apply(TestOrderEventStubs::submitted(
                &order,
                AccountId::from("POLY-001"),
            ))
            .unwrap();
        let response: OrderResponse = load("http_order_response_ok.json");
        let venue_order_id = VenueOrderId::from(
            response
                .order_id
                .as_deref()
                .expect("captured response should include order ID"),
        );
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let order_identities = OrderIdentityRegistry::default();
        let pending_cancels = PendingCancelTracker::default();

        let deferred = handle_order_response(
            Ok(response),
            &order,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_cancels,
            AccountId::from("POLY-001"),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        assert!(deferred.is_none());
        assert!(order_identities.get(&venue_order_id).is_some());
        assert!(fill_tracker.contains(&venue_order_id));
        assert_eq!(order.status(), OrderStatus::Submitted);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_captured_delayed_response_preserves_deferred_cancel() {
        let instrument = test_instrument();
        let order = test_limit_order("O-DELAYED-CANCEL", instrument.id());
        let response: OrderResponse = load("http_order_response_ok.json");
        let venue_order_id = VenueOrderId::from(
            response
                .order_id
                .as_deref()
                .expect("captured response should include order ID"),
        );
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let order_identities = OrderIdentityRegistry::default();
        let pending_cancels = PendingCancelTracker::default();
        pending_cancels.insert(order.client_order_id());

        let deferred = handle_order_response(
            Ok(response),
            &order,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_cancels,
            AccountId::from("POLY-001"),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        assert_eq!(deferred, Some((venue_order_id.to_string(), venue_order_id)));
        assert!(order_identities.get(&venue_order_id).is_some());
        assert!(fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_captured_batch_delayed_responses_register_each_leg_without_accepting() {
        let instrument = test_instrument();
        let responses: Vec<OrderResponse> = load("http_batch_order_response.json");
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let order_identities = OrderIdentityRegistry::default();
        let pending_cancels = PendingCancelTracker::default();

        for (index, response) in responses.into_iter().enumerate() {
            let mut order = test_limit_order(&format!("O-BATCH-{index}"), instrument.id());
            order
                .apply(TestOrderEventStubs::submitted(
                    &order,
                    AccountId::from("POLY-001"),
                ))
                .unwrap();
            let venue_order_id = VenueOrderId::from(
                response
                    .order_id
                    .as_deref()
                    .expect("captured batch leg should include order ID"),
            );

            let deferred = handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            );

            assert!(deferred.is_none());
            assert!(order_identities.get(&venue_order_id).is_some());
            assert!(fill_tracker.contains(&venue_order_id));
            assert_eq!(order.status(), OrderStatus::Submitted);
        }

        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_market_sell_submission_uses_signed_wire_quantity() {
        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let mut order = OrderAny::Market(MarketOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-MARKET-SELL"),
            OrderSide::Sell,
            Quantity::from("5.208000"),
            TimeInForce::Fok,
            UUID4::new(),
            UnixNanos::default(),
            false,
            false,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
        ));
        let (emitter, mut receiver) = test_emitter();
        let venue_order_id = VenueOrderId::from("0xmarket-sell");
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let order_identities = OrderIdentityRegistry::default();
        let pending_cancels = PendingCancelTracker::default();

        emit_market_order_submitted(
            &mut order,
            false,
            OrderSide::Sell,
            Quantity::from("5.208000"),
            Decimal::new(5_200_000, 6),
            true,
            6,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
        );
        let response = successful_order_response(venue_order_id, None);
        let deferred_cancel = handle_order_response(
            Ok(response),
            &order,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_cancels,
            AccountId::from("POLY-001"),
            6,
            4,
        );

        let submitted = receiver.try_recv().expect("expected submitted event");
        let updated = receiver.try_recv().expect("expected quantity update");
        let accepted = receiver.try_recv().expect("expected accepted event");
        fill_tracker.record_fill(&venue_order_id, Quantity::from("5.200000"));

        assert!(matches!(
            submitted,
            ExecutionEvent::Order(OrderEventAny::Submitted(_))
        ));
        assert!(matches!(
            updated,
            ExecutionEvent::Order(OrderEventAny::Updated(_))
        ));
        assert!(matches!(
            accepted,
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));
        assert!(deferred_cancel.is_none());
        assert_eq!(order.quantity(), Quantity::from("5.200000"));
        assert_eq!(order.filled_qty(), Quantity::zero(6));
        assert!(fill_tracker.is_fully_filled(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    fn test_fill_report(
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
        last_qty: Quantity,
        ts_event: UnixNanos,
    ) -> FillReport {
        FillReport::new(
            AccountId::from("POLY-001"),
            instrument_id,
            venue_order_id,
            TradeId::from("trade-1"),
            OrderSide::Buy,
            last_qty,
            Price::new(0.50, 4),
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            ts_event,
            UnixNanos::from(1_000_000_100u64),
            Some(UUID4::new()),
        )
    }

    #[rstest]
    #[case(PolymarketTradeStatus::Matched)]
    #[case(PolymarketTradeStatus::Mined)]
    #[case(PolymarketTradeStatus::Retrying)]
    #[case(PolymarketTradeStatus::Failed)]
    fn test_non_confirmed_rest_trade_does_not_generate_fill_report(
        #[case] status: PolymarketTradeStatus,
    ) {
        let instrument = test_instrument();
        let mut trade: crate::http::models::PolymarketTradeReport = load("http_trade_report.json");
        trade.status = status;

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = crate::execution::reconciliation::FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (reports, _) = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        )
        .expect("non-confirmed trades do not build fill reports");

        assert!(reports.is_empty());
    }

    #[rstest]
    fn test_confirmed_maker_trade_owned_by_case_variant_address_generates_fill_report() {
        let mut instrument = test_instrument();
        let mut trade: crate::http::models::PolymarketTradeReport = load("http_trade_report.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;
        bind_instrument_to_trade(&mut instrument, &trade);

        // Recorded venue payloads carry EIP-55 checksummed (mixed-case) maker
        // addresses while configured funder addresses are commonly lowercase.
        // Mirror that direction: give the payload side a case variant (all-
        // uppercase hex stands in for the checksummed form), keep the
        // configured side lowercase. Any case variant of the same address
        // must still establish ownership.
        let configured_address = trade.maker_orders[0].maker_address.clone();
        let uppercase_variant_address = configured_address
            .to_ascii_uppercase()
            .replacen("0X", "0x", 1);
        assert_ne!(uppercase_variant_address, configured_address);
        trade.maker_orders[0].maker_address = uppercase_variant_address;
        let foreign_api_key = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        assert_ne!(trade.maker_orders[0].owner, foreign_api_key);
        let expected_venue_order_id = VenueOrderId::from(trade.maker_orders[0].order_id.as_str());

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = crate::execution::reconciliation::FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: &configured_address,
            api_key: foreign_api_key,
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (reports, discards) = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        )
        .expect("owned confirmed maker trade builds a fill report");

        assert_eq!(
            reports.len(),
            1,
            "the account's own confirmed maker fill must be reported",
        );
        assert_eq!(reports[0].venue_order_id, expected_venue_order_id);
        assert_eq!(
            discards.unowned_maker_trades, 0,
            "entry-level skips of foreign entries in an owned trade are not trade drops",
        );
        assert_eq!(discards.unmapped_instruments, 0);
    }

    #[rstest]
    #[case(PolymarketLiquiditySide::Maker)]
    #[case(PolymarketLiquiditySide::Taker)]
    fn test_confirmed_trade_without_instrument_counts_unmapped_discard(
        #[case] trader_side: PolymarketLiquiditySide,
    ) {
        let mut trade: crate::http::models::PolymarketTradeReport = load("http_trade_report.json");
        trade.trader_side = trader_side;
        let instruments = AtomicMap::new();
        let ctx = crate::execution::reconciliation::FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (reports, discards) = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        )
        .expect("unmapped instruments are counted rather than parsed");

        assert_eq!(reports.len(), 0);
        assert_eq!(
            discards,
            crate::execution::reconciliation::FillBuildDiscards {
                has_pending_target: false,
                unmapped_instruments: 1,
                in_scope_historical: 1,
                unowned_maker_trades: 0,
                untimestamped_trades: 0,
            },
        );
    }

    #[rstest]
    fn test_confirmed_maker_trade_without_owned_order_is_counted() {
        let instrument = test_instrument();
        let mut trade: crate::http::models::PolymarketTradeReport = load("http_trade_report.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        // Neither the address nor the API key matches any maker order, so the
        // whole confirmed trade is dropped; the drop must be observable.
        let ctx = crate::execution::reconciliation::FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x000000000000000000000000000000000000dead",
            api_key: "ffffffff-ffff-ffff-ffff-ffffffffffff",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (reports, discards) = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        )
        .expect("unowned maker trades are counted rather than parsed");

        assert!(reports.is_empty());
        assert_eq!(
            discards.unowned_maker_trades, 1,
            "a confirmed maker trade dropped whole must be counted, not silent",
        );
        assert_eq!(discards.unmapped_instruments, 0);
    }

    #[rstest]
    fn test_fill_report_batch_fails_instead_of_returning_valid_prefix() {
        let mut instrument = test_instrument();
        let InstrumentAny::BinaryOption(binary_option) = &mut instrument else {
            panic!("expected binary option test instrument");
        };
        binary_option.taker_fee =
            Decimal::from_i128_with_scale(100_000_000_000_000_000_000_000_000i128, 0);

        let mut taker: crate::http::models::PolymarketTradeReport = load("http_trade_report.json");
        taker.id = "trade-unrepresentable-taker".to_string();
        bind_instrument_to_trade(&mut instrument, &taker);
        let mut maker = taker.clone();
        maker.id = "trade-valid-maker".to_string();
        maker.trader_side = PolymarketLiquiditySide::Maker;
        let configured_address = maker.maker_orders[0].maker_address.clone();

        let instruments = AtomicMap::new();
        instruments.insert(taker.asset_id, instrument);
        let ctx = crate::execution::reconciliation::FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: &configured_address,
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (maker_reports, _) = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[maker.clone()],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        )
        .expect("maker commission is zero and representable");
        let result = crate::execution::reconciliation::build_fill_reports_from_trades(
            &[maker, taker],
            &ctx,
            &instruments,
            FillReportScope::new(None, None),
            UnixNanos::from(1_000_000_000u64),
            None,
            None,
        );

        assert_eq!(maker_reports.len(), 1);
        assert!(
            result
                .unwrap_err()
                .to_string()
                .contains("trade-unrepresentable-taker")
        );
    }

    #[rstest]
    fn test_unknown_submit_tracks_expected_id_for_ws_order_recovery() {
        let ws_order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let order = test_limit_order("O-UNKNOWN-WS", instrument_id);
        let expected_venue_order_id = VenueOrderId::from(ws_order.id.as_str());
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        assert!(
            handle_unknown_submit_result(
                &order,
                expected_venue_order_id,
                "transport timeout",
                None,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_submits,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        assert_eq!(
            pending_submits.client_order_id(&expected_venue_order_id),
            Some(order.client_order_id())
        );

        let token_instruments = AtomicMap::new();
        token_instruments.insert(ws_order.asset_id, instrument);
        let mut state = WsDispatchState::default();
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };

        dispatch_user_message(&UserWsMessage::Order(ws_order), &ctx, &mut state);

        // The tracked own order emits an OrderAccepted event, not a report.
        let event = receiver.try_recv().expect("expected accepted event");
        match event {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.client_order_id, order.client_order_id());
            }
            other => panic!("expected accepted event, was {other:?}"),
        }

        assert!(!fill_tracker.has_pending_report(&expected_venue_order_id));
    }

    #[rstest]
    fn test_unknown_submit_accepts_order_when_pending_fill_proves_venue_order() {
        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let mut order = test_quote_market_order("O-UNKNOWN-FILL", instrument_id);
        let venue_order_id = VenueOrderId::from("0xunknown-fill-order");
        let fill_ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        fill_tracker.buffer_fill_for_test(
            venue_order_id,
            test_fill_report(
                instrument_id,
                venue_order_id,
                Quantity::new(18.181, 3),
                fill_ts,
            ),
        );

        emit_market_order_submitted(
            &mut order,
            true,
            OrderSide::Buy,
            Quantity::new(10.0, 0),
            Decimal::new(18_180, 3),
            true,
            3,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
        );

        match receiver.try_recv().expect("expected submitted event") {
            ExecutionEvent::Order(OrderEventAny::Submitted(event)) => {
                assert_eq!(event.client_order_id, order.client_order_id());
            }
            other => panic!("expected submitted event, was {other:?}"),
        }

        match receiver.try_recv().expect("expected updated event") {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert_eq!(event.quantity, Quantity::new(18.180, 3));
                assert!(!event.is_quote_quantity);
            }
            other => panic!("expected updated event, was {other:?}"),
        }
        assert_eq!(order.quantity(), Quantity::new(18.180, 3));
        assert!(!order.is_quote_quantity());

        assert!(
            handle_unknown_submit_result(
                &order,
                venue_order_id,
                "transport timeout",
                Some(Quantity::new(18.180, 3)),
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_submits,
                &pending_cancels,
                AccountId::from("POLY-001"),
                3,
                4,
            )
            .is_none()
        );

        let accepted = receiver.try_recv().expect("expected accepted event");
        match accepted {
            ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert_eq!(event.venue_order_id, venue_order_id);
                assert_eq!(event.ts_event, fill_ts);
            }
            other => panic!("expected accepted event, was {other:?}"),
        }

        // The drained own-order fill emits an OrderFilled event, not a report.
        let fill = receiver.try_recv().expect("expected filled event");
        match fill {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => {
                assert_eq!(event.client_order_id, order.client_order_id());
                assert_eq!(event.venue_order_id, venue_order_id);
                assert_eq!(event.last_qty, Quantity::new(18.180, 3));
            }
            other => panic!("expected filled event, was {other:?}"),
        }

        assert!(fill_tracker.contains(&venue_order_id));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::new(18.18, 3))
        );
        assert!(!fill_tracker.has_pending_fill(&venue_order_id));
    }

    #[rstest]
    fn test_unknown_submit_cancels_ioc_remainder_when_trade_confirms_before_fill_drain() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let account_id = AccountId::from("POLY-001");
        let venue_order_id = VenueOrderId::from("0xunknown-dust-fill-order");
        let submitted_qty = Quantity::from("5.202910");
        let venue_fill_qty = Quantity::from("5.202897");
        let mut order = test_quote_market_order("O-UNKNOWN-DUST-FILL", instrument_id);
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        let (emitter, mut receiver) = test_emitter();
        emit_market_order_submitted(
            &mut order,
            true,
            OrderSide::Buy,
            Quantity::new(5.0, 0),
            Decimal::new(5_202_910, 6),
            true,
            instrument.size_precision(),
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
        );
        receiver.try_recv().expect("expected submitted event");
        receiver.try_recv().expect("expected quantity update event");

        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let correction_key = "trade-confirmed-before-drain-order";
        assert!(
            fill_tracker
                .accept_or_buffer_fill(
                    venue_order_id,
                    test_fill_report(
                        instrument_id,
                        venue_order_id,
                        venue_fill_qty,
                        UnixNanos::from(900u64),
                    ),
                    FillCorrectionMetadata {
                        correction_key: correction_key.to_string(),
                        info: None,
                        is_confirmed: false,
                    },
                )
                .is_none()
        );
        fill_tracker.mark_trade_confirmed(correction_key);
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        assert!(
            handle_unknown_submit_result(
                &order,
                venue_order_id,
                "transport timeout",
                Some(submitted_qty),
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_submits,
                &pending_cancels,
                account_id,
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(event @ OrderEventAny::Accepted(_)) => event,
            other => panic!("expected accepted event, was {other:?}"),
        };
        let venue_fill = match receiver.try_recv().expect("expected venue fill event") {
            ExecutionEvent::Order(event @ OrderEventAny::Filled(_)) => {
                if let OrderEventAny::Filled(ref fill) = event {
                    assert_eq!(fill.last_qty, venue_fill_qty);
                }
                event
            }
            other => panic!("expected filled event, was {other:?}"),
        };
        let canceled = match receiver.try_recv().expect("expected IOC cancellation") {
            ExecutionEvent::Order(event @ OrderEventAny::Canceled(_)) => event,
            other => panic!("expected canceled event, was {other:?}"),
        };

        order.apply(accepted).unwrap();
        order.apply(venue_fill).unwrap();
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        order.apply(canceled).unwrap();

        assert_eq!(order.status(), OrderStatus::Canceled);
        assert_eq!(order.quantity(), submitted_qty);
        assert_eq!(order.filled_qty(), venue_fill_qty);
        assert_eq!(order.trade_ids().len(), 1);
        assert!(!fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    // A terminal order update can race ahead of the submit confirmation and be buffered. On
    // drain it synthesizes acceptance (never emitted at submit) then converts to the event.
    #[rstest]
    #[case(OrderStatus::Canceled, "Canceled")]
    #[case(OrderStatus::Expired, "Expired")]
    fn test_drain_buffered_terminal_emits_accepted_then_event(
        #[case] status: OrderStatus,
        #[case] expected: &str,
    ) {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let order = test_limit_order("O-DRAIN-TERMINAL", instrument_id);
        let venue_order_id = VenueOrderId::from("0xdrain-terminal-order");
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let report = OrderStatusReport::new(
            AccountId::from("POLY-001"),
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            status,
            Quantity::new(10.0, 0),
            Quantity::new(0.0, 0),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        );
        fill_tracker.buffer_report_for_test(venue_order_id, report);

        let result = handle_unknown_submit_result(
            &order,
            venue_order_id,
            "transport timeout",
            None,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_submits,
            &pending_cancels,
            AccountId::from("POLY-001"),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        assert!(result.is_none());

        // Acceptance is synthesized first, then the buffered terminal report converts to an event.
        match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(OrderEventAny::Accepted(event)) => {
                assert_eq!(event.client_order_id, order.client_order_id());
            }
            other => panic!("expected accepted event, was {other:?}"),
        }

        match receiver.try_recv().expect("expected terminal event") {
            ExecutionEvent::Order(order_event) => {
                assert!(
                    format!("{order_event:?}").starts_with(expected),
                    "expected {expected}, was {order_event:?}"
                );
                assert_eq!(order_event.client_order_id(), order.client_order_id());
                assert_eq!(order_event.venue_order_id(), Some(venue_order_id));
            }
            other => panic!("expected order event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_emit_drained_rejection_uses_clean_classified_reason() {
        let instrument = test_instrument();
        let order = test_limit_order("O-DRAIN-REJECTED", instrument.id());
        let venue_order_id = VenueOrderId::from("0xdrain-rejected-order");
        let (emitter, mut receiver) = test_emitter();
        let mut report = OrderStatusReport::new(
            AccountId::from("POLY-001"),
            instrument.id(),
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Rejected,
            Quantity::new(10.0, 0),
            Quantity::new(0.0, 0),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        );
        report.cancel_reason = Some("  invalid post-only order:\norder crosses book  ".to_string());

        emit_drained_order_report(&order, &report, &emitter);

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(
                    event.reason.as_str(),
                    "invalid post-only order: order crosses book"
                );
                assert!(event.due_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_unknown_submit_drains_partial_fill_before_cancel() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let venue_order_id = VenueOrderId::from("0xdrain-cancel-fill-order");
        let account_id = AccountId::from("POLY-001");
        let submitted_qty = Quantity::from("5.192100");
        let venue_fill_qty = Quantity::from("5.192081");
        let mut order = test_quote_market_order("O-DRAIN-CANCEL-FILL", instrument_id);
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        let (emitter, mut receiver) = test_emitter();
        emit_market_order_submitted(
            &mut order,
            true,
            OrderSide::Buy,
            Quantity::new(5.0, 0),
            Decimal::new(5_192_100, 6),
            true,
            instrument.size_precision(),
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
        );
        receiver.try_recv().expect("expected submitted event");
        receiver.try_recv().expect("expected quantity update event");

        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let cancel_report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Market,
            TimeInForce::Ioc,
            OrderStatus::Canceled,
            submitted_qty,
            venue_fill_qty,
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        );
        fill_tracker.buffer_report_for_test(venue_order_id, cancel_report);
        fill_tracker.buffer_fill_for_test(
            venue_order_id,
            test_fill_report(
                instrument_id,
                venue_order_id,
                venue_fill_qty,
                UnixNanos::from(900u64),
            ),
        );

        assert!(
            handle_unknown_submit_result(
                &order,
                venue_order_id,
                "transport timeout",
                Some(submitted_qty),
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_submits,
                &pending_cancels,
                account_id,
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        let mut emitted = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            if let ExecutionEvent::Order(event) = event {
                order.apply(event.clone()).unwrap();
                emitted.push(event);
            }
        }

        assert_eq!(emitted.len(), 3);
        assert!(matches!(emitted[0], OrderEventAny::Accepted(_)));
        assert!(matches!(emitted[1], OrderEventAny::Filled(_)));
        assert!(matches!(emitted[2], OrderEventAny::Canceled(_)));
        assert_eq!(order.filled_qty(), venue_fill_qty);
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert!(fill_tracker.contains(&venue_order_id));
    }

    #[rstest]
    fn test_unknown_submit_filled_report_normalizes_resting_order_quantity() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let venue_order_id = VenueOrderId::from("0xdrain-filled-dust-order");
        let account_id = AccountId::from("POLY-001");
        let submitted_qty = Quantity::from("5.192100");
        let venue_fill_qty = Quantity::from("5.192081");
        let order = test_limit_order("O-DRAIN-FILLED-DUST", instrument_id);
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let filled_report = OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            submitted_qty,
            submitted_qty,
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            UnixNanos::from(1_000u64),
            None,
        );
        fill_tracker.buffer_report_for_test(venue_order_id, filled_report);
        fill_tracker.buffer_fill_for_test(
            venue_order_id,
            test_fill_report(
                instrument_id,
                venue_order_id,
                venue_fill_qty,
                UnixNanos::from(900u64),
            ),
        );

        assert!(
            handle_unknown_submit_result(
                &order,
                venue_order_id,
                "transport timeout",
                Some(submitted_qty),
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_submits,
                &pending_cancels,
                account_id,
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        assert!(matches!(
            receiver.try_recv().expect("expected accepted event"),
            ExecutionEvent::Order(OrderEventAny::Accepted(_))
        ));

        match receiver.try_recv().expect("expected venue fill event") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
                assert_eq!(fill.last_qty, venue_fill_qty);
            }
            other => panic!("expected filled event, was {other:?}"),
        }

        match receiver.try_recv().expect("expected quantity update") {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.quantity, venue_fill_qty);
                assert!(updated.reconciliation);
            }
            other => panic!("expected updated event, was {other:?}"),
        }

        assert!(!fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    fn test_taker_trade(
        asset_id: Ustr,
        venue_order_id: VenueOrderId,
        size: &str,
        price: &str,
    ) -> PolymarketUserTrade {
        PolymarketUserTrade {
            asset_id,
            bucket_index: 0,
            fee_rate_bps: "0".to_string(),
            id: "trade-race".to_string(),
            last_update: "1700000001".to_string(),
            maker_address: Ustr::from("0xmaker"),
            maker_orders: vec![],
            market: Ustr::from("0xmarket"),
            match_time: "1700000000".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            price: price.to_string(),
            side: PolymarketOrderSide::Buy,
            size: size.to_string(),
            status: PolymarketTradeStatus::Confirmed,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            transaction_hash: None,
            trader_side: PolymarketLiquiditySide::Taker,
            event_type: PolymarketEventType::Trade,
        }
    }

    // A fast-filling marketable limit order whose WS taker trade arrives before the HTTP submit
    // response: the fill buffers (order not yet registered), then the submit response registers and
    // drains it under one tracker lock. A later FAILED update must still find and void that fill.
    #[rstest]
    fn test_ws_taker_fill_before_submit_response_can_be_voided_after_drain() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let asset_id = instrument_id.symbol.inner();
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();
        let account_id = AccountId::from("POLY-001");
        let venue_order_id = VenueOrderId::from("0xrace-taker-fill");

        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-RACE-FILL"),
            OrderSide::Buy,
            Quantity::new(5.192100, size_precision),
            Price::new(0.963, price_precision),
            TimeInForce::Fok,
            None,
            false,
            false,
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
            UnixNanos::default(),
        ));
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        // Step 1: the WS taker trade arrives BEFORE the submit response. The order is not yet
        // registered, so the fill buffers in the tracker rather than emitting.
        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument);
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id,
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();
        let mut trade = test_taker_trade(asset_id, venue_order_id, "5.192081", "0.963");
        trade.status = PolymarketTradeStatus::Matched;
        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(
            fill_tracker.has_pending_fill(&venue_order_id),
            "fill must buffer while the order is unregistered",
        );

        // Step 2: the submit response arrives, registers the order, and drains the buffered fill
        // Reuse the captured delayed response shape with the race-specific order ID
        let mut response: OrderResponse = load("http_order_response_ok.json");
        response.order_id = Some(venue_order_id.to_string());
        assert!(
            handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                account_id,
                size_precision,
                price_precision,
            )
            .is_none()
        );

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(event @ OrderEventAny::Accepted(_)) => event,
            other => panic!("expected accepted event, was {other:?}"),
        };
        let filled = match receiver.try_recv().expect("expected filled event") {
            ExecutionEvent::Order(event @ OrderEventAny::Filled(_)) => {
                if let OrderEventAny::Filled(ref fill) = event {
                    assert_eq!(fill.venue_order_id, venue_order_id);
                    assert_eq!(fill.last_qty, Quantity::new(5.192081, size_precision));
                    let info = fill.info.as_ref().expect("expected trade metadata");
                    assert_eq!(info.get(&Ustr::from("id")), Some(&Ustr::from("trade-race")));
                }
                event
            }
            other => panic!("expected filled event, was {other:?}"),
        };
        let filled_event_id = match &filled {
            OrderEventAny::Filled(fill) => fill.event_id,
            _ => unreachable!(),
        };

        order.apply(accepted).unwrap();
        order.apply(filled).unwrap();
        assert_eq!(order.status(), OrderStatus::PartiallyFilled);
        assert!(!fill_tracker.has_pending_fill(&venue_order_id));

        trade.status = PolymarketTradeStatus::Failed;
        assert!(dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state).is_some());
        let voided = match receiver.try_recv().expect("expected fill correction") {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected fill-void event, was {other:?}"),
        };

        assert_eq!(voided.trade_id, TradeId::from("trade-race"));
        assert_eq!(voided.voided_qty, Quantity::new(5.192081, size_precision));
        assert_eq!(voided.causation_id, Some(filled_event_id));
        assert!(receiver.try_recv().is_err());
    }

    // Symmetric to the fill case: a WS terminal order report (cancel) arrives before the submit
    // response and buffers (order not yet registered). The submit response registers and drains it,
    // so the cancel surfaces as `OrderCanceled` after `OrderAccepted` and carries the order to
    // `Canceled`, not orphaned in the buffer.
    #[rstest]
    fn test_ws_order_report_before_submit_response_reaches_canceled() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();
        let account_id = AccountId::from("POLY-001");
        let venue_order_id = VenueOrderId::from(cancel_order.id.as_str());

        let mut order = test_limit_order("O-RACE-CANCEL", instrument_id);
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        // Step 1: the WS cancel arrives BEFORE the submit response and buffers (order unregistered)
        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument);
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id,
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();
        dispatch_user_message(&UserWsMessage::Order(cancel_order), &ctx, &mut state);

        assert!(
            fill_tracker.has_pending_report(&venue_order_id),
            "report must buffer while the order is unregistered",
        );

        // Step 2: the submit response registers the order and drains the buffered cancel
        let response = OrderResponse {
            success: true,
            order_id: Some(venue_order_id.to_string()),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: None,
        };
        assert!(
            handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                account_id,
                size_precision,
                price_precision,
            )
            .is_none()
        );

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(event @ OrderEventAny::Accepted(_)) => event,
            other => panic!("expected accepted event, was {other:?}"),
        };
        let canceled = match receiver.try_recv().expect("expected canceled event") {
            ExecutionEvent::Order(event @ OrderEventAny::Canceled(_)) => event,
            other => panic!("expected canceled event, was {other:?}"),
        };

        // Applying the drained events carries the order to Canceled: the report is not orphaned
        order.apply(accepted).unwrap();
        order.apply(canceled).unwrap();
        assert_eq!(order.status(), OrderStatus::Canceled);
        assert!(!fill_tracker.has_pending_report(&venue_order_id));
    }

    // Polymarket fills a marketable BUY by spending a USDC amount, so the share fill can exceed the
    // nominal order qty (here 12 vs 10) when it executes below the limit. The adapter must raise the
    // order qty to the actual fill (OrderUpdated) before OrderFilled, otherwise the engine drops the
    // fill as an overfill and the order orphans.
    #[rstest]
    fn test_ws_taker_overfill_bumps_order_qty_then_fills() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let asset_id = instrument_id.symbol.inner();
        let size_precision = instrument.size_precision();
        let price_precision = instrument.price_precision();
        let account_id = AccountId::from("POLY-001");
        let venue_order_id = VenueOrderId::from("0xoverfill-buy");

        let mut order = OrderAny::Limit(LimitOrder::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument_id,
            ClientOrderId::from("O-OVERFILL"),
            OrderSide::Buy,
            Quantity::new(10.0, size_precision),
            Price::new(0.50, price_precision),
            TimeInForce::Fok,
            None,
            false,
            false,
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
            UnixNanos::default(),
        ));
        order
            .apply(TestOrderEventStubs::submitted(&order, account_id))
            .unwrap();

        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = PendingSubmitTracker::default();
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        // WS taker fill of 12 shares (the marketable BUY filled below its limit) before the response.
        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument);
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id,
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();
        dispatch_user_message(
            &UserWsMessage::Trade(test_taker_trade(asset_id, venue_order_id, "12", "0.50")),
            &ctx,
            &mut state,
        );

        let response = OrderResponse {
            success: true,
            order_id: Some(venue_order_id.to_string()),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: None,
        };
        handle_order_response(
            Ok(response),
            &order,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &fill_tracker,
            &order_identities,
            &pending_cancels,
            account_id,
            size_precision,
            price_precision,
        );

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(event @ OrderEventAny::Accepted(_)) => event,
            other => panic!("expected accepted event, was {other:?}"),
        };
        // The overfill must raise the order qty to 12 before the fill is applied.
        let updated = match receiver.try_recv().expect("expected updated event") {
            ExecutionEvent::Order(event @ OrderEventAny::Updated(_)) => {
                if let OrderEventAny::Updated(ref u) = event {
                    assert_eq!(u.quantity, Quantity::new(12.0, size_precision));
                }
                event
            }
            other => panic!("expected updated event raising qty to the fill, was {other:?}"),
        };
        let filled = match receiver.try_recv().expect("expected filled event") {
            ExecutionEvent::Order(event @ OrderEventAny::Filled(_)) => {
                if let OrderEventAny::Filled(ref fill) = event {
                    assert_eq!(fill.last_qty, Quantity::new(12.0, size_precision));
                }
                event
            }
            other => panic!("expected filled event, was {other:?}"),
        };

        order.apply(accepted).unwrap();
        order.apply(updated).unwrap();
        order.apply(filled).unwrap();
        assert_eq!(order.quantity(), Quantity::new(12.0, size_precision));
        assert_eq!(order.status(), OrderStatus::Filled);
    }

    // An empty orderID with no reason is ambiguous: it must not panic constructing a VenueOrderId,
    // and with nothing to report it stays on the warn branch (no event) for reconciliation.
    #[rstest]
    fn test_batch_leg_empty_order_id_no_reason_does_not_panic() {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let order = test_limit_order("O-BATCH-EMPTY", instrument_id);
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let response = OrderResponse {
            success: true,
            order_id: Some(String::new()),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: None,
        };

        assert!(
            handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        // The empty id routes to the warn branch: no order events emitted
        assert!(receiver.try_recv().is_err());
    }

    // The batch endpoint reports a rejected leg as success=true with an empty orderID and the reason
    // in error_msg (live: a naked SELL rejected for no balance). Surface it as OrderRejected fast,
    // carrying due_post_only when the reason is a post-only crossing.
    #[rstest]
    #[case("not enough balance / allowance: the balance is not enough", false)]
    #[case("invalid post-only order: order crosses book", true)]
    fn test_batch_leg_empty_order_id_with_reason_rejects(
        #[case] reason: &str,
        #[case] expected_post_only: bool,
    ) {
        let instrument = test_instrument();
        let order = test_limit_order("O-BATCH-REJECT", instrument.id());
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let response = OrderResponse {
            success: true,
            order_id: Some(String::new()),
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: Some(reason.to_string()),
        };

        assert!(
            handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(event.reason.as_str(), reason);
                assert_eq!(event.due_post_only, expected_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }
    }

    // A post-only limit rejected for crossing the book must surface due_post_only=true so strategies
    // can distinguish it from other venue rejections; any other reason stays false.
    #[rstest]
    #[case("invalid post-only order: order crosses book", true)]
    #[case("not enough balance / allowance", false)]
    fn test_submit_reject_flags_post_only_crossing(
        #[case] reason: &str,
        #[case] expected_post_only: bool,
    ) {
        let instrument = test_instrument();
        let instrument_id = instrument.id();
        let order = test_limit_order("O-REJECT", instrument_id);
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_cancels = PendingCancelTracker::default();
        let order_identities = OrderIdentityRegistry::default();

        let response = OrderResponse {
            success: false,
            order_id: None,
            status: None,
            making_amount: None,
            taking_amount: None,
            transaction_hashes: None,
            trade_ids: None,
            error_msg: Some(reason.to_string()),
        };

        assert!(
            handle_order_response(
                Ok(response),
                &order,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &order_identities,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(event.reason.as_str(), reason);
                assert_eq!(event.due_post_only, expected_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }
    }

    // Live path: a single-order post-only crossing rejection arrives as an HTTP 400 error and is
    // emitted via reject_submit_order, not the success=false branch, so the flag must be set here
    // too. The reason carries the venue message that the HTTP path wraps.
    #[rstest]
    #[case("invalid post-only order: order crosses book", true)]
    #[case("invalid post-only order: unsupported tick size", false)]
    #[case("Invalid post-only order: order crosses book", false)]
    #[case("invalid post-only order: order crosses book now", false)]
    #[case("HTTP error 400: invalid post-only order: order crosses book", false)]
    #[case("not enough balance / allowance", false)]
    fn test_reject_submit_order_flags_post_only_crossing(
        #[case] reason: &str,
        #[case] expected_post_only: bool,
    ) {
        let instrument = test_instrument();
        let order = test_limit_order("O-REJECT-SUBMIT", instrument.id());
        let (emitter, mut receiver) = test_emitter();
        let pending_cancels = PendingCancelTracker::default();
        pending_cancels.insert(order.client_order_id());

        reject_submit_order(
            &order,
            reason,
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &pending_cancels,
        );

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(event.reason.as_str(), reason);
                assert_eq!(event.due_post_only, expected_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }

        // The reject funnel clears any tracked pending cancel for the order
        assert!(!pending_cancels.contains(&order.client_order_id()));
    }

    #[rstest]
    fn test_reject_submit_order_sanitizes_reason_before_classification() {
        let instrument = test_instrument();
        let order = test_limit_order("O-REJECT-SANITIZE", instrument.id());
        let (emitter, mut receiver) = test_emitter();
        let pending_cancels = PendingCancelTracker::default();

        reject_submit_order(
            &order,
            "  invalid post-only order:\norder crosses book  ",
            &emitter,
            nautilus_core::time::get_atomic_clock_realtime(),
            &pending_cancels,
        );

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(
                    event.reason.as_str(),
                    "invalid post-only order: order crosses book"
                );
                assert!(event.due_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }
    }
}
