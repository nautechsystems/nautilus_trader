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

//! WebSocket execution dispatch for the Kraken Futures API.
//!
//! Routes `OpenOrdersDelta`, `OpenOrdersCancel`, and `FillsDelta` messages to
//! typed order events (for tracked orders) or status / fill reports (for
//! external orders) under the two-tier dispatch contract.

use std::sync::Arc;

use nautilus_core::{AtomicMap, UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{OrderCanceled, OrderEventAny, OrderUpdated},
    identifiers::{AccountId, ClientOrderId, InstrumentId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::OrderStatusReport,
    types::{Price, Quantity},
};

use super::{
    DeltaSnapshot, OrderIdentity, WsDispatchState, ensure_accepted_emitted,
    fill_report_to_order_filled, lookup_instrument, resolve_client_order_id,
};
use crate::websocket::futures::{
    messages::{
        KrakenFuturesFill, KrakenFuturesFillsDelta, KrakenFuturesOpenOrdersCancel,
        KrakenFuturesOpenOrdersDelta,
    },
    parse::{parse_futures_ws_fill_report, parse_futures_ws_order_status_report},
};

/// Dispatches a Kraken Futures `OpenOrdersDelta` message.
///
/// Fill-driven cancel deltas (`is_cancel=true` with reason `full_fill` /
/// `partial_fill`) are skipped — the corresponding `FillsDelta` carries the
/// real fill, so emitting a synthetic Canceled here would race with the
/// genuine `OrderFilled`.
#[expect(clippy::too_many_arguments)]
pub fn open_orders_delta(
    delta: &KrakenFuturesOpenOrdersDelta,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: &Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: &Arc<AtomicMap<String, Quantity>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    if delta.is_fill_driven_cancel() {
        log::debug!(
            "Skipping fill-driven open_orders delta: order_id={}, reason={:?}",
            delta.order.order_id,
            delta.reason,
        );
        return;
    }

    let product_id = delta.order.instrument.as_str();
    let Some(instrument) = lookup_instrument(instruments, product_id) else {
        log::warn!("No instrument for product_id: {product_id}");
        return;
    };

    // Cache instrument and qty by venue order id so cancel-only messages
    // (which arrive without the order body) can be reconstructed for the
    // external fallback path.
    order_instrument_map.insert(delta.order.order_id.clone(), instrument.id());
    let qty = Quantity::new(delta.order.qty, instrument.size_precision());
    venue_order_qty.insert(delta.order.order_id.clone(), qty);

    let resolved_id = delta
        .order
        .cli_ord_id
        .as_ref()
        .map(|id| resolve_client_order_id(id, truncated_id_map));

    // Stale-report suppression: an order that already reached the filled
    // terminal state should not produce more events even if a late delta
    // arrives. `filled_orders` persists past `cleanup_terminal` precisely
    // for this check.
    if let Some(cid) = resolved_id
        && state.filled_orders.contains(&cid)
    {
        log::debug!(
            "Skipping stale open_orders delta for filled order: cid={cid}, order_id={}",
            delta.order.order_id,
        );
        return;
    }

    if let Some(client_order_id) = resolved_id {
        venue_client_map.insert(delta.order.order_id.clone(), client_order_id);

        if let Some(identity) = state.lookup_identity(&client_order_id) {
            delta_tracked(
                delta,
                client_order_id,
                &identity,
                &instrument,
                state,
                emitter,
                account_id,
                ts_init,
            );
            return;
        }
    }

    // External / untracked: fall back to a status report.
    match parse_futures_ws_order_status_report(
        &delta.order,
        delta.is_cancel,
        delta.reason.as_deref(),
        &instrument,
        account_id,
        ts_init,
    ) {
        Ok(mut report) => {
            if let Some(cid) = resolved_id {
                report = report.with_client_order_id(cid);
            }
            emitter.send_order_status_report(report);
        }
        Err(e) => log::error!("Failed to parse futures order status report: {e}"),
    }
}

#[expect(clippy::too_many_arguments)]
fn delta_tracked(
    delta: &KrakenFuturesOpenOrdersDelta,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    instrument: &InstrumentAny,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let venue_order_id = VenueOrderId::new(&delta.order.order_id);
    let ts_event = millis_to_nanos(delta.order.last_update_time);
    let new_filled = Quantity::new(delta.order.filled, instrument.size_precision());

    if delta.is_cancel {
        ensure_accepted_emitted(
            client_order_id,
            venue_order_id,
            account_id,
            identity,
            state,
            emitter,
            ts_event,
            ts_init,
        );
        let canceled = OrderCanceled::new(
            emitter.trader_id(),
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
        );
        emitter.send_order_event(OrderEventAny::Canceled(canceled));
        state.cleanup_terminal(&client_order_id);
        return;
    }

    let already_accepted = state.emitted_accepted.contains(&client_order_id);
    ensure_accepted_emitted(
        client_order_id,
        venue_order_id,
        account_id,
        identity,
        state,
        emitter,
        ts_event,
        ts_init,
    );

    let qty = Quantity::new(delta.order.qty, instrument.size_precision());
    let snapshot = DeltaSnapshot::new(
        qty,
        new_filled,
        delta.order.limit_price,
        delta.order.stop_price,
    );

    if !already_accepted {
        // First delta seen for this order: the placement Accepted is enough.
        state.record_delta_snapshot(client_order_id, snapshot);
        return;
    }

    // Follow-up delta. The two emission-relevant signals are independent:
    //   * filled increased       -> partial-fill notification (FillsDelta has it,
    //                               nothing to emit from here)
    //   * non-fill field changed -> modify acknowledgement (emit OrderUpdated)
    // Both can be true simultaneously when a user amends a partially filled
    // order, so check the modify branch regardless of fill movement.
    let previous = state.previous_delta_snapshot(&client_order_id);
    state.record_delta_snapshot(client_order_id, snapshot);

    let non_fill_changed = previous.is_some_and(|prev| !snapshot.non_fill_fields_match(&prev));
    if !non_fill_changed {
        return;
    }

    // Modify ack: refresh tracked quantity (size may have changed) and emit
    // OrderUpdated so the engine clears PendingUpdate.
    state.update_identity_quantity(&client_order_id, qty);
    let updated = OrderUpdated::new(
        emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        client_order_id,
        qty,
        UUID4::new(),
        ts_event,
        ts_init,
        false,
        Some(venue_order_id),
        Some(account_id),
        delta
            .order
            .limit_price
            .map(|p| Price::new(p, instrument.price_precision())),
        delta
            .order
            .stop_price
            .map(|p| Price::new(p, instrument.price_precision())),
        None,
        false,
    );
    emitter.send_order_event(OrderEventAny::Updated(updated));
}

/// Dispatches a Kraken Futures `OpenOrdersCancel` (cancel-only) message.
#[expect(clippy::too_many_arguments)]
pub fn open_orders_cancel(
    cancel: &KrakenFuturesOpenOrdersCancel,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_instrument_map: &Arc<AtomicMap<String, InstrumentId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_order_qty: &Arc<AtomicMap<String, Quantity>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    // Skip fill-driven removals (the FillsDelta carries the real fill).
    if let Some(ref reason) = cancel.reason
        && (reason == "full_fill" || reason == "partial_fill")
    {
        log::debug!(
            "Skipping fill-driven cancel: order_id={}, reason={reason}",
            cancel.order_id,
        );
        return;
    }

    let venue_order_id = VenueOrderId::new(&cancel.order_id);
    let resolved_id = cancel
        .cli_ord_id
        .as_ref()
        .map(|id| resolve_client_order_id(id, truncated_id_map))
        .or_else(|| venue_client_map.load().get(&cancel.order_id).copied());

    if let Some(client_order_id) = resolved_id
        && let Some(identity) = state.lookup_identity(&client_order_id)
    {
        let ts_event = ts_init;
        ensure_accepted_emitted(
            client_order_id,
            venue_order_id,
            account_id,
            &identity,
            state,
            emitter,
            ts_event,
            ts_init,
        );
        let canceled = OrderCanceled::new(
            emitter.trader_id(),
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
        );
        emitter.send_order_event(OrderEventAny::Canceled(canceled));
        state.cleanup_terminal(&client_order_id);
        return;
    }

    // External fallback: build a status report from the side caches.
    let Some(instrument_id) = order_instrument_map.load().get(&cancel.order_id).copied() else {
        log::warn!(
            "Cannot resolve instrument for cancel: order_id={}, \
             order not seen in previous delta",
            cancel.order_id
        );
        return;
    };

    let Some(quantity) = venue_order_qty.load().get(&cancel.order_id).copied() else {
        log::warn!(
            "Cannot resolve quantity for cancel: order_id={}, skipping",
            cancel.order_id
        );
        return;
    };

    let report = OrderStatusReport::new(
        account_id,
        instrument_id,
        resolved_id,
        venue_order_id,
        OrderSide::NoOrderSide,
        OrderType::Limit,
        TimeInForce::Gtc,
        OrderStatus::Canceled,
        quantity,
        Quantity::zero(0),
        ts_init,
        ts_init,
        ts_init,
        None,
    );
    let report = if let Some(ref reason) = cancel.reason
        && !reason.is_empty()
    {
        report.with_cancel_reason(reason.clone())
    } else {
        report
    };
    emitter.send_order_status_report(report);
}

/// Dispatches a Kraken Futures `FillsDelta` message.
#[expect(clippy::too_many_arguments)]
pub fn fills_delta(
    fills_delta: &KrakenFuturesFillsDelta,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    for fill in &fills_delta.fills {
        single_fill(
            fill,
            state,
            emitter,
            instruments,
            truncated_id_map,
            venue_client_map,
            account_id,
            ts_init,
        );
    }
}

#[expect(clippy::too_many_arguments)]
fn single_fill(
    fill: &KrakenFuturesFill,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    venue_client_map: &Arc<AtomicMap<String, ClientOrderId>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let product_id = match &fill.instrument {
        Some(id) => id.as_str(),
        None => {
            log::warn!("Fill missing instrument field: fill_id={}", fill.fill_id);
            return;
        }
    };

    let Some(instrument) = lookup_instrument(instruments, product_id) else {
        log::warn!("No instrument for product_id: {product_id}");
        return;
    };

    let mut report = match parse_futures_ws_fill_report(fill, &instrument, account_id, ts_init) {
        Ok(report) => report,
        Err(e) => {
            log::error!("Failed to parse futures fill report: {e}");
            return;
        }
    };

    let resolved_id = fill
        .cli_ord_id
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(|id| resolve_client_order_id(id, truncated_id_map))
        .or_else(|| venue_client_map.load().get(&fill.order_id).copied());

    if let Some(cid) = resolved_id
        && state.filled_orders.contains(&cid)
    {
        log::debug!(
            "Skipping stale fill for filled order: cid={cid}, order_id={}",
            fill.order_id,
        );
        return;
    }

    if let Some(client_order_id) = resolved_id {
        report.client_order_id = Some(client_order_id);

        if let Some(identity) = state.lookup_identity(&client_order_id) {
            if state.check_and_insert_trade(report.trade_id) {
                log::debug!(
                    "Skipping duplicate fill for {client_order_id}: trade_id={}",
                    report.trade_id
                );
                return;
            }
            ensure_accepted_emitted(
                client_order_id,
                report.venue_order_id,
                account_id,
                &identity,
                state,
                emitter,
                report.ts_event,
                ts_init,
            );
            let filled = fill_report_to_order_filled(
                &report,
                emitter.trader_id(),
                &identity,
                instrument.quote_currency(),
                client_order_id,
            );
            emitter.send_order_event(OrderEventAny::Filled(filled));

            // Update cumulative filled and cleanup on terminal fill.
            let previous = state
                .previous_filled_qty(&client_order_id)
                .unwrap_or_else(|| Quantity::zero(instrument.size_precision()));
            let cumulative = previous + report.last_qty;
            state.record_filled_qty(client_order_id, cumulative);

            if cumulative >= identity.quantity {
                state.insert_filled(client_order_id);
                state.cleanup_terminal(&client_order_id);
            }
            return;
        }
    }

    // External fallback.
    if state.check_and_insert_trade(report.trade_id) {
        log::debug!(
            "Skipping duplicate external fill: trade_id={}",
            report.trade_id
        );
        return;
    }
    emitter.send_fill_report(report);
}

#[inline]
fn millis_to_nanos(millis: i64) -> UnixNanos {
    UnixNanos::from((millis as u64) * 1_000_000)
}
