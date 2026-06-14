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
    identifiers::{AccountId, InstrumentId, VenueOrderId},
    orders::{Order, OrderAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use tokio::task::JoinHandle;

use super::{
    PendingCancelTracker, PendingFillMap, PendingOrderReportMap, PendingSubmitMap,
    cancellations::execute_deferred_cancel, order_fill_tracker::OrderFillTrackerMap,
    reports::get_pusd_currency, submitter::OrderSubmitter, types::BatchLimitOrderContext,
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
    pending_fills: &PendingFillMap,
    pending_order_reports: &PendingOrderReportMap,
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
            pending_fills,
            pending_order_reports,
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
    emitter.emit_order_rejected(order, reason, ts_now, false);
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
    pending_submits: &PendingSubmitMap,
    pending_fills: &PendingFillMap,
    pending_order_reports: &PendingOrderReportMap,
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
                pending_fills,
                pending_order_reports,
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
                pending_submits,
                pending_fills,
                pending_order_reports,
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
    pending_submits: &PendingSubmitMap,
    pending_fills: &PendingFillMap,
    pending_order_reports: &PendingOrderReportMap,
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

    pending_submits
        .lock()
        .expect(MUTEX_POISONED)
        .insert(expected_venue_order_id, order.client_order_id());

    drain_pending_reports_for_known_order(
        order,
        expected_venue_order_id,
        emitter,
        clock,
        fill_tracker,
        fill_tracker_quantity,
        pending_fills,
        pending_order_reports,
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
    fill_tracker_quantity: Option<Quantity>,
    pending_fills: &PendingFillMap,
    pending_order_reports: &PendingOrderReportMap,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) {
    let Some(buffered) = pending_order_reports
        .lock()
        .expect(MUTEX_POISONED)
        .remove(&venue_order_id)
    else {
        accept_order_with_pending_fills(
            order,
            venue_order_id,
            emitter,
            clock,
            fill_tracker,
            fill_tracker_quantity,
            pending_fills,
            size_precision,
            price_precision,
        );
        return;
    };

    let should_register = buffered
        .iter()
        .any(|report| report.order_status != OrderStatus::Rejected);
    if should_register {
        let tracker_quantity = fill_tracker_quantity.unwrap_or_else(|| order.quantity());
        fill_tracker.register(
            venue_order_id,
            tracker_quantity,
            order.order_side(),
            order.instrument_id(),
            size_precision,
            price_precision,
        );
    }

    let buffered_fills = if should_register {
        drain_pending_fills_for_known_order(order, venue_order_id, fill_tracker, pending_fills)
    } else {
        Vec::new()
    };

    let mut has_filled = false;

    for report in &buffered {
        if report.order_status == OrderStatus::Filled {
            has_filled = true;
        }
    }

    let tracked_filled = fill_tracker
        .get_cumulative_filled(&venue_order_id)
        .unwrap_or(0.0);
    let tracked_qty = Quantity::new(tracked_filled, size_precision);

    for mut report in buffered {
        report.client_order_id = Some(order.client_order_id());
        if report.filled_qty > tracked_qty {
            log::debug!(
                "Capping buffered filled_qty for {venue_order_id} from {} to {} \
                 (awaiting trade messages)",
                report.filled_qty,
                tracked_qty,
            );
            report.filled_qty = tracked_qty;
        }
        emitter.send_order_status_report(report);
    }

    for fill in buffered_fills {
        emitter.send_fill_report(fill);
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
            emitter.send_fill_report(dust_fill);
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
    fill_tracker_quantity: Option<Quantity>,
    pending_fills: &PendingFillMap,
    size_precision: u8,
    price_precision: u8,
) {
    let Some(buffered) = pending_fills
        .lock()
        .expect(MUTEX_POISONED)
        .remove(&venue_order_id)
    else {
        return;
    };

    let ts_event = buffered
        .iter()
        .map(|fill| fill.ts_event)
        .min()
        .unwrap_or_else(|| clock.get_time_ns());
    emitter.emit_order_accepted(order, venue_order_id, ts_event);
    let tracker_quantity = fill_tracker_quantity.unwrap_or_else(|| order.quantity());
    fill_tracker.register(
        venue_order_id,
        tracker_quantity,
        order.order_side(),
        order.instrument_id(),
        size_precision,
        price_precision,
    );

    for fill in prepare_pending_fills_for_known_order(order, venue_order_id, fill_tracker, buffered)
    {
        emitter.send_fill_report(fill);
    }
}

pub(super) fn drain_pending_fills_for_known_order(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    pending_fills: &PendingFillMap,
) -> Vec<FillReport> {
    let Some(buffered) = pending_fills
        .lock()
        .expect(MUTEX_POISONED)
        .remove(&venue_order_id)
    else {
        return Vec::new();
    };

    prepare_pending_fills_for_known_order(order, venue_order_id, fill_tracker, buffered)
}

pub(super) fn prepare_pending_fills_for_known_order(
    order: &OrderAny,
    venue_order_id: VenueOrderId,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    buffered: Vec<FillReport>,
) -> Vec<FillReport> {
    buffered
        .into_iter()
        .map(|mut fill| {
            fill.client_order_id = Some(order.client_order_id());
            fill.last_qty = fill_tracker.snap_fill_qty(&venue_order_id, fill.last_qty);
            fill_tracker.record_fill(
                &venue_order_id,
                fill.last_qty.as_f64(),
                fill.last_px.as_f64(),
                fill.ts_event,
            );
            fill
        })
        .collect()
}

#[expect(clippy::too_many_arguments)]
pub(super) fn handle_order_response(
    result: crate::http::error::Result<OrderResponse>,
    order: &OrderAny,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    pending_fills: &PendingFillMap,
    pending_order_reports: &PendingOrderReportMap,
    pending_cancels: &PendingCancelTracker,
    account_id: AccountId,
    size_precision: u8,
    price_precision: u8,
) -> Option<(String, VenueOrderId)> {
    match result {
        Ok(response) => {
            if response.success {
                if let Some(order_id) = response.order_id {
                    let venue_order_id = VenueOrderId::from(order_id.as_str());
                    let ts_now = clock.get_time_ns();
                    emitter.emit_order_accepted(order, venue_order_id, ts_now);

                    fill_tracker.register(
                        venue_order_id,
                        order.quantity(),
                        order.order_side(),
                        order.instrument_id(),
                        size_precision,
                        price_precision,
                    );

                    if let Some(buffered) = pending_fills
                        .lock()
                        .expect(MUTEX_POISONED)
                        .remove(&venue_order_id)
                    {
                        for mut fill in buffered {
                            fill.last_qty =
                                fill_tracker.snap_fill_qty(&venue_order_id, fill.last_qty);
                            fill_tracker.record_fill(
                                &venue_order_id,
                                fill.last_qty.as_f64(),
                                fill.last_px.as_f64(),
                                fill.ts_event,
                            );
                            emitter.send_fill_report(fill);
                        }
                    }

                    if let Some(buffered) = pending_order_reports
                        .lock()
                        .expect(MUTEX_POISONED)
                        .remove(&venue_order_id)
                    {
                        let mut has_filled = false;

                        for report in &buffered {
                            if report.order_status == OrderStatus::Filled {
                                has_filled = true;
                            }
                        }

                        let tracked_filled = fill_tracker
                            .get_cumulative_filled(&venue_order_id)
                            .unwrap_or(0.0);
                        let tracked_qty = Quantity::new(tracked_filled, size_precision);

                        for mut report in buffered {
                            if report.filled_qty > tracked_qty {
                                log::debug!(
                                    "Capping buffered filled_qty for {venue_order_id} \
                                     from {} to {} (awaiting trade messages)",
                                    report.filled_qty,
                                    tracked_qty,
                                );
                                report.filled_qty = tracked_qty;
                            }
                            emitter.send_order_status_report(report);
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
                                emitter.send_fill_report(dust_fill);
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
                let ts_now = clock.get_time_ns();
                emitter.emit_order_rejected(order, &reason, ts_now, false);
                pending_cancels.remove(&order.client_order_id());
            }
        }
        Err(e) => {
            let ts_now = clock.get_time_ns();
            emitter.emit_order_rejected(order, &format!("HTTP request failed: {e}"), ts_now, false);
            pending_cancels.remove(&order.client_order_id());
        }
    }
    None
}

#[expect(clippy::too_many_arguments)]
pub(super) async fn check_fok_status(
    submitter: &OrderSubmitter,
    order_id: &str,
    fill_tracker: &Arc<OrderFillTrackerMap>,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    instrument_id: InstrumentId,
    order_side: OrderSide,
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

    if !matches!(
        order_status,
        OrderStatus::Rejected | OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Filled
    ) {
        return;
    }

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

    let ts_now = clock.get_time_ns();
    let mut report = OrderStatusReport::new(
        account_id,
        instrument_id,
        None,
        venue_order_id,
        order_side,
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

    log::info!("FOK order {order_id} resolved via REST as {order_status:?}");

    emitter.send_order_status_report(report);
}

#[cfg(test)]
mod tests {
    use super::*;
    use nautilus_common::{
        cache::fifo::FifoCacheMap,
        messages::{ExecutionEvent, ExecutionReport},
    };
    use nautilus_core::{UnixNanos, collections::AtomicMap};
    use nautilus_model::{
        enums::{AccountType, LiquiditySide},
        identifiers::{ClientOrderId, StrategyId, TradeId, TraderId},
        instruments::{Instrument, InstrumentAny},
        orders::{LimitOrder, MarketOrder, Order},
        types::{Currency, Money},
    };
    use rstest::rstest;

    use crate::{
        http::{
            models::GammaMarket,
            parse::{create_instrument_from_def, parse_gamma_market},
        },
        websocket::{
            dispatch::{WsDispatchContext, WsDispatchState, dispatch_user_message},
            messages::{PolymarketUserOrder, UserWsMessage},
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
        let pending_submits = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_fills = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_order_reports = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_cancels = PendingCancelTracker::default();

        assert!(
            handle_unknown_submit_result(
                &order,
                expected_venue_order_id,
                "transport timeout",
                None,
                &emitter,
                nautilus_core::time::get_atomic_clock_realtime(),
                &fill_tracker,
                &pending_submits,
                &pending_fills,
                &pending_order_reports,
                &pending_cancels,
                AccountId::from("POLY-001"),
                instrument.size_precision(),
                instrument.price_precision(),
            )
            .is_none()
        );

        assert_eq!(
            pending_submits
                .lock()
                .unwrap()
                .get(&expected_venue_order_id)
                .copied(),
            Some(order.client_order_id())
        );

        let token_instruments = AtomicMap::new();
        token_instruments.insert(ws_order.asset_id, instrument);
        let mut state = WsDispatchState::default();
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            pending_fills: &pending_fills,
            pending_order_reports: &pending_order_reports,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };

        dispatch_user_message(&UserWsMessage::Order(ws_order), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected order report");
        match event {
            ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                assert_eq!(report.client_order_id, Some(order.client_order_id()));
            }
            other => panic!("expected order report, was {other:?}"),
        }

        assert!(
            pending_order_reports
                .lock()
                .unwrap()
                .get(&expected_venue_order_id)
                .is_none()
        );
    }

    #[rstest]
    fn test_unknown_submit_accepts_order_when_pending_fill_proves_venue_order() {
        let instrument_id = InstrumentId::from("TEST-TOKEN.POLYMARKET");
        let mut order = test_quote_market_order("O-UNKNOWN-FILL", instrument_id);
        let venue_order_id = VenueOrderId::from("0xunknown-fill-order");
        let fill_ts = UnixNanos::from(1_700_000_000_000_000_000u64);
        let (emitter, mut receiver) = test_emitter();
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let pending_submits = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_fills = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_order_reports = Arc::new(Mutex::new(FifoCacheMap::default()));
        let pending_cancels = PendingCancelTracker::default();

        pending_fills.lock().unwrap().insert(
            venue_order_id,
            vec![test_fill_report(
                instrument_id,
                venue_order_id,
                Quantity::new(18.181, 3),
                fill_ts,
            )],
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
                &pending_submits,
                &pending_fills,
                &pending_order_reports,
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

        let fill = receiver.try_recv().expect("expected fill report");
        match fill {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.client_order_id, Some(order.client_order_id()));
                assert_eq!(report.venue_order_id, venue_order_id);
                assert_eq!(report.last_qty, Quantity::new(18.180, 3));
            }
            other => panic!("expected fill report, was {other:?}"),
        }

        assert!(fill_tracker.contains(&venue_order_id));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(18.18)
        );
        assert!(pending_fills.lock().unwrap().get(&venue_order_id).is_none());
    }
}
