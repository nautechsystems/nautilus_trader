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

use nautilus_common::live::get_runtime;
use nautilus_core::{MUTEX_POISONED, UUID4, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderEventAny, OrderUpdated},
    identifiers::{AccountId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use super::{
    cancellations::execute_deferred_cancel,
    identity::{OrderIdentity, OrderIdentityRegistry},
    order_fill_tracker::OrderFillTrackerMap,
    pending::{PendingCancelTracker, PendingSubmitTracker},
    reports::get_pusd_currency,
    submitter::OrderSubmitter,
    types::BatchLimitOrderContext,
};
use crate::http::query::OrderResponse;

#[expect(clippy::too_many_arguments)]
pub(super) async fn handle_batch_order_responses(
    responses: Vec<OrderResponse>,
    batch_orders: Vec<BatchLimitOrderContext>,
    submitter: &OrderSubmitter,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    order_identities: &OrderIdentityRegistry,
    pending_cancels: &PendingCancelTracker,
    pending_tasks: &Arc<Mutex<Vec<JoinHandle<()>>>>,
    stopping: &Arc<AtomicBool>,
    account_id: AccountId,
) {
    let response_len = responses.len();
    let order_len = batch_orders.len();

    if response_len != order_len {
        log::warn!(
            "Batch submit response length ({response_len}) does not match order count ({order_len})"
        );
    }

    let mut deferred = Vec::new();

    for (batch_order, response) in batch_orders.iter().zip(responses) {
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
            deferred.push((batch_order.order.clone(), order_id_str, venue_order_id));
        }
    }

    if order_len > response_len {
        for batch_order in batch_orders.iter().skip(response_len) {
            reject_submit_order(
                &batch_order.order,
                "Order not included in API response",
                emitter,
                clock,
                pending_cancels,
            );
        }
    }

    if !deferred.is_empty() {
        let mut tasks = pending_tasks.lock().expect(MUTEX_POISONED);

        if stopping.load(Ordering::Acquire) {
            return;
        }
        tasks.retain(|handle| !handle.is_finished());

        for (order, order_id_str, venue_order_id) in deferred {
            let submitter = submitter.clone();
            let emitter = emitter.clone();
            let pending_cancels = pending_cancels.clone();

            let handle = get_runtime().spawn(async move {
                execute_deferred_cancel(
                    &submitter,
                    &order,
                    &order_id_str,
                    venue_order_id,
                    &emitter,
                    &pending_cancels,
                    clock,
                )
                .await;
            });
            tasks.push(handle);
        }
    }
}

pub(super) fn reject_submit_order(
    order: &OrderAny,
    reason: &str,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    pending_cancels: &PendingCancelTracker,
) {
    let ts_now = clock.get_time_ns();
    emitter.emit_order_rejected(order, reason, ts_now, is_post_only_crossing(reason));
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

    if !update_quantity || !is_quote_qty || side != OrderSide::Buy || expected_base_qty.is_zero() {
        return;
    }

    let Ok(base_qty) = Quantity::from_decimal_dp(expected_base_qty, size_precision) else {
        return;
    };

    log::info!(
        "Converted {} quote quantity {} to base quantity {} (from signed taker_amount)",
        order.instrument_id(),
        amount,
        base_qty,
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
        log::error!("Failed to apply quote-to-base OrderUpdated: {e}");
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
        }
        Err(e) if e.is_submit_outcome_unknown() => {
            if let Some((order_id_str, venue_order_id)) = handle_unknown_submit_result(
                &batch_order.order,
                expected_venue_order_id,
                &e.to_string(),
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
        Err(e) => {
            reject_submit_order(
                &batch_order.order,
                &format!("{e}"),
                emitter,
                clock,
                pending_cancels,
            );
        }
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
            order.instrument_id(),
            size_precision,
            price_precision,
        )
    } else {
        Vec::new()
    };

    let has_filled = buffered
        .iter()
        .any(|report| report.order_status == OrderStatus::Filled);

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

    for report in &buffered {
        emit_drained_order_report(order, report, emitter);
    }

    for fill in buffered_fills {
        emit_drained_fill(order, &fill, fill_tracker, emitter, clock);
    }

    if has_filled {
        let fallback_px = order.price().map_or(0.0, |p| p.as_f64());
        let ts_now = clock.get_time_ns();

        if let Some(dust_fill) = fill_tracker.check_dust_and_build_fill(
            &venue_order_id,
            account_id,
            venue_order_id.as_str(),
            fallback_px,
            get_pusd_currency(),
            ts_now,
            ts_now,
        ) {
            emit_drained_fill(order, &dust_fill, fill_tracker, emitter, clock);
        }
    }
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
    size_precision: u8,
    price_precision: u8,
) {
    // Accept only once a buffered fill proves the venue took the order
    let tracker_quantity = fill_tracker_quantity.unwrap_or_else(|| order.quantity());
    let Some(fills) = fill_tracker.register_and_take_pending_fills_if_buffered(
        venue_order_id,
        Some(order.client_order_id()),
        tracker_quantity,
        order.order_side(),
        order.instrument_id(),
        size_precision,
        price_precision,
    ) else {
        return;
    };

    let ts_event = fills
        .iter()
        .map(|fill| fill.ts_event)
        .min()
        .unwrap_or_else(|| clock.get_time_ns());

    if order_identities.mark_accepted(venue_order_id) {
        emitter.emit_order_accepted(order, venue_order_id, ts_event);
    }

    for fill in fills {
        emit_drained_fill(order, &fill, fill_tracker, emitter, clock);
    }
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
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) -> Option<(String, VenueOrderId)> {
    match result {
        Ok(response) => {
            if response.success {
                // VenueOrderId panics on an empty string
                if let Some(order_id) = response.order_id.filter(|s| !s.is_empty()) {
                    let venue_order_id = VenueOrderId::from(order_id.as_str());
                    let ts_now = clock.get_time_ns();
                    order_identities
                        .register_order_identity(venue_order_id, OrderIdentity::from_order(order));
                    if order_identities.mark_accepted(venue_order_id) {
                        emitter.emit_order_accepted(order, venue_order_id, ts_now);
                    }

                    for fill in fill_tracker.register_and_take_pending_fills(
                        venue_order_id,
                        Some(order.client_order_id()),
                        order.quantity(),
                        order.order_side(),
                        order.instrument_id(),
                        size_precision,
                        price_precision,
                    ) {
                        emit_drained_fill(order, &fill, fill_tracker, emitter, clock);
                    }

                    // The register above precedes this drain, so a racing report can't be orphaned
                    let buffered = fill_tracker.take_pending_reports(&venue_order_id);
                    if !buffered.is_empty() {
                        let has_filled = buffered
                            .iter()
                            .any(|report| report.order_status == OrderStatus::Filled);

                        for report in &buffered {
                            emit_drained_order_report(order, report, emitter);
                        }

                        if has_filled {
                            let fallback_px = order.price().map_or(0.0, |p| p.as_f64());
                            if let Some(dust_fill) = fill_tracker.check_dust_and_build_fill(
                                &venue_order_id,
                                account_id,
                                &order_id,
                                fallback_px,
                                get_pusd_currency(),
                                ts_now,
                                ts_now,
                            ) {
                                emit_drained_fill(order, &dust_fill, fill_tracker, emitter, clock);
                            }
                        }
                    }

                    if pending_cancels.contains(&order.client_order_id()) {
                        log::info!(
                            "Order {} has pending cancel, issuing deferred cancel for {}",
                            order.client_order_id(),
                            venue_order_id
                        );
                        return Some((order_id, venue_order_id));
                    }
                } else if let Some(reason) = response.error_msg.filter(|s| !s.is_empty()) {
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
        Err(e) => {
            reject_submit_order(
                order,
                &format!("HTTP request failed: {e}"),
                emitter,
                clock,
                pending_cancels,
            );
        }
    }
    None
}

// Require both terms so only a post-only crossing matches, not any post-only reason
fn is_post_only_crossing(reason: &str) -> bool {
    reason.contains("post-only") && reason.contains("cross")
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
    fill_tracker: &OrderFillTrackerMap,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    if let Some(new_qty) = fill_tracker.buy_overfill_bump(&fill.venue_order_id) {
        emit_buy_overfill_update(order, fill.venue_order_id, new_qty, emitter, clock);
    }

    emitter.emit_order_filled(
        order,
        fill.venue_order_id,
        fill.venue_position_id,
        fill.trade_id,
        fill.last_qty,
        fill.last_px,
        get_pusd_currency(),
        Some(fill.commission),
        fill.liquidity_side,
        fill.ts_event,
    );
}

/// Emits an `OrderUpdated` raising the order quantity to the actual BUY fill, before the fill.
fn emit_buy_overfill_update(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    new_qty: Quantity,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    log::info!(
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
            emitter.emit_order_rejected(order, &reason, report.ts_last, false);
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

    log::info!("FOK order {order_id} unresolved after 5s, checking REST status");

    let venue_order = match submitter.get_order(order_id).await {
        Ok(Some(o)) => o,
        Ok(None) => {
            log::info!("FOK order {order_id} not found (empty response), WS will reconcile");
            return;
        }
        Err(e) => {
            log::warn!("FOK status check failed for {order_id}: {e}");
            return;
        }
    };

    let order_status = OrderStatus::from(venue_order.status);
    let ts_now = clock.get_time_ns();

    match order_status {
        OrderStatus::Rejected => {
            log::info!("FOK order {order_id} resolved via REST as Rejected");
            emitter.emit_order_rejected(order, "FOK order unfilled", ts_now, false);
        }
        OrderStatus::Canceled => {
            log::info!("FOK order {order_id} resolved via REST as Canceled");
            emitter.emit_order_canceled(order, Some(venue_order_id), ts_now);
        }
        OrderStatus::Expired => {
            log::info!("FOK order {order_id} resolved via REST as Expired");
            emitter.emit_order_expired(order, Some(venue_order_id), ts_now);
        }
        OrderStatus::Filled => {
            // The venue reports Filled but no fills reached the tracker. Build a report so the
            // engine reconciles from venue state rather than synthesizing fabricated fills.
            let quantity = Quantity::new(
                venue_order
                    .original_size
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(0.0),
                size_precision,
            );
            let filled_qty = Quantity::new(
                venue_order
                    .size_matched
                    .to_string()
                    .parse::<f64>()
                    .unwrap_or(0.0),
                size_precision,
            );
            let price = Price::new(
                venue_order.price.to_string().parse::<f64>().unwrap_or(0.0),
                price_precision,
            );

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

            log::info!("FOK order {order_id} resolved via REST as Filled; reconciling via report");
            emitter.send_order_status_report(report);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::ExecutionEvent;
    use nautilus_core::{UnixNanos, collections::AtomicMap};
    use nautilus_model::{
        enums::{AccountType, LiquiditySide},
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId},
        instruments::{Instrument, InstrumentAny},
        orders::{LimitOrder, MarketOrder, Order, stubs::TestOrderEventStubs},
        types::{Currency, Money},
    };
    use rstest::rstest;
    use ustr::Ustr;

    use super::*;
    use crate::{
        common::enums::{
            PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOutcome,
            PolymarketTradeStatus,
        },
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
            Money::new(0.0, Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            ts_event,
            UnixNanos::from(1_000_000_100u64),
            Some(UUID4::new()),
        )
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
            Some(18.18)
        );
        assert!(!fill_tracker.has_pending_fill(&venue_order_id));
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
            status: PolymarketTradeStatus::Matched,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            trader_side: PolymarketLiquiditySide::Taker,
            event_type: PolymarketEventType::Trade,
        }
    }

    // A fast-filling marketable limit order whose WS taker trade arrives before the HTTP submit
    // response: the fill buffers (order not yet registered), then the submit response registers and
    // drains it under one tracker lock. The buffered fill must surface as `OrderFilled` and carry
    // the order to `Filled`, not orphan with the order stuck `Accepted`.
    #[rstest]
    fn test_ws_taker_fill_before_submit_response_reaches_filled() {
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
        dispatch_user_message(
            &UserWsMessage::Trade(test_taker_trade(asset_id, venue_order_id, "10", "0.50")),
            &ctx,
            &mut state,
        );

        assert!(
            fill_tracker.has_pending_fill(&venue_order_id),
            "fill must buffer while the order is unregistered",
        );

        // Step 2: the submit response arrives, registers the order, and drains the buffered fill
        let response = OrderResponse {
            success: true,
            order_id: Some(venue_order_id.to_string()),
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
        let filled = match receiver.try_recv().expect("expected filled event") {
            ExecutionEvent::Order(event @ OrderEventAny::Filled(_)) => {
                if let OrderEventAny::Filled(ref fill) = event {
                    assert_eq!(fill.venue_order_id, venue_order_id);
                    assert_eq!(fill.last_qty, Quantity::new(10.0, size_precision));
                }
                event
            }
            other => panic!("expected filled event, was {other:?}"),
        };

        // Applying the drained events carries the order to Filled: the fill is not orphaned
        order.apply(accepted).unwrap();
        order.apply(filled).unwrap();
        assert_eq!(order.status(), OrderStatus::Filled);
        assert!(!fill_tracker.has_pending_fill(&venue_order_id));
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
}
