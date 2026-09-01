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

//! Order and fill reconciliation.
//!
//! Event construction, order state reconciliation, and fill reconciliation. Venue-sourced
//! reports become zero or more `OrderEventAny`s that are safe to apply to the local order model.

use nautilus_common::enums::LogColor;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderStatus, OrderType},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled,
        OrderRejected, OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, PositionId, TradeId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny, TRIGGERABLE_ORDER_TYPES},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    ids::create_inferred_reconciliation_trade_id,
    positions::{cap_price_at_instrument_max, is_within_single_unit_tolerance},
};

/// Generates reconciliation events for a live order status report.
///
/// Events are produced in venue-temporal order: any missing `Accepted` event
/// first, then an `OrderUpdated` if the venue snapshot shows a confirmed
/// quantity/price amendment, then any status/fill events from
/// [`reconcile_order_report`]. Emitting the amendment before the fill matters
/// when the venue increased the total quantity and reported a fill that would
/// otherwise close the order under the stale local quantity. The `Updated`
/// step is suppressed for pending venue states so a still-unconfirmed amend
/// cannot mutate the local projection ahead of venue confirmation.
#[must_use]
pub fn generate_reconciliation_order_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    generate_reconciliation_order_events_inner(
        order,
        report,
        instrument,
        ts_now,
        report.order_status == OrderStatus::Voided,
        None,
    )
}

/// Generates reconciliation events for an authoritative venue snapshot.
///
/// Unlike [`generate_reconciliation_order_events`], a material decrease in cumulative filled
/// quantity is treated as evidence that previously applied fills were voided. This is intended for
/// status reports paired with the snapshot's fill reports, where the caller can project the full
/// venue state before applying corrections.
#[must_use]
pub fn generate_reconciliation_order_snapshot_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    generate_reconciliation_order_events_inner(order, report, instrument, ts_now, true, None)
}

/// Generates reconciliation events for an authoritative venue snapshot with an inferred-fill
/// commission supplied by the responsible execution client.
#[must_use]
pub fn generate_reconciliation_order_snapshot_events_with_commission(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Vec<OrderEventAny> {
    generate_reconciliation_order_events_inner(order, report, instrument, ts_now, true, commission)
}

fn generate_reconciliation_order_events_inner(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
    allow_fill_decrease: bool,
    commission: Option<Money>,
) -> Vec<OrderEventAny> {
    if is_superseded_cancel_report(order, report) {
        let _ = reconcile_order_report(order, report, instrument, ts_now);
        return Vec::new();
    }

    if has_material_fill_decrease(order, report) {
        if !allow_fill_decrease {
            log::warn!(
                "Ignoring fill decrease without explicit void evidence for {}: cached={}, venue={}",
                order.client_order_id(),
                order.filled_qty(),
                report.filled_qty,
            );
            return reconcile_fill_decrease_terminal(order, report, instrument, ts_now);
        }

        let mut working = order.clone();
        let mut events = create_reconciliation_fill_voids(&working, report, ts_now);
        if events.is_empty() {
            return reconcile_fill_decrease_terminal(order, report, instrument, ts_now);
        }

        for event in &events {
            if let Err(e) = working.apply(event.clone()) {
                log::warn!(
                    "Cannot project reconciliation fill void for {}: {e}",
                    order.client_order_id()
                );
                return reconcile_fill_decrease_terminal(order, report, instrument, ts_now);
            }
        }

        if report_is_working(report) && should_reconciliation_update(&working, report) {
            let updated = create_reconciliation_updated(&working, report, ts_now);
            if let Err(e) = working.apply(updated.clone()) {
                log::warn!(
                    "Cannot project reopening reconciliation update for {}: {e}",
                    order.client_order_id()
                );
            } else {
                events.push(updated);
            }
        }

        if let Some(terminal) = reconcile_order_report(&working, report, instrument, ts_now) {
            events.push(terminal);
        }
        return events;
    }

    let (mut working, mut events) = prepare_reconciliation_order(order, report, ts_now);

    if matches!(
        report.order_status,
        OrderStatus::Canceled | OrderStatus::Expired,
    ) && report.filled_qty > working.filled_qty()
        && let Some(instrument) = instrument
        && let Some(filled) = create_incremental_inferred_fill(
            &working,
            report,
            &report.account_id,
            instrument,
            ts_now,
            commission,
        )
    {
        if let Err(e) = working.apply(filled.clone()) {
            log::warn!(
                "Failed to pre-apply reconciliation fill for {}: {e}",
                order.client_order_id(),
            );
        } else {
            events.push(filled);
        }
    }

    if working.status() == OrderStatus::Filled
        && matches!(
            report.order_status,
            OrderStatus::Canceled | OrderStatus::Expired,
        )
    {
        return events;
    }

    if let Some(event) =
        reconcile_order_report_with_commission(&working, report, instrument, ts_now, commission)
    {
        events.push(event);
    }

    events
}

fn has_material_fill_decrease(order: &OrderAny, report: &OrderStatusReport) -> bool {
    if report.filled_qty >= order.filled_qty() {
        return false;
    }

    let precision = order
        .filled_qty()
        .precision
        .max(report.filled_qty.precision);
    !is_within_single_unit_tolerance(
        report.filled_qty.as_decimal(),
        order.filled_qty().as_decimal(),
        precision,
    )
}

fn reconcile_fill_decrease_terminal(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    let should_reconcile = report.order_status == OrderStatus::Voided
        || (!order.is_closed()
            && matches!(
                report.order_status,
                OrderStatus::Canceled | OrderStatus::Expired
            ));

    if !should_reconcile {
        return Vec::new();
    }

    reconcile_order_report(order, report, instrument, ts_now)
        .into_iter()
        .collect()
}

/// Generates acceptance, amendment, and triggering events for a report paired with real fills.
#[must_use]
pub fn generate_reconciliation_order_pre_fill_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    if is_superseded_cancel_report(order, report) {
        return Vec::new();
    }

    let (working, mut events) = prepare_reconciliation_order(order, report, ts_now);

    if report.order_status == OrderStatus::Triggered
        && let Some(triggered) = reconcile_order_report(&working, report, None, ts_now)
    {
        events.push(triggered);
    }

    events
}

fn prepare_reconciliation_order(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> (OrderAny, Vec<OrderEventAny>) {
    let mut working = order.clone();
    let mut events: Vec<OrderEventAny> = Vec::new();

    if should_accept_before_reconciliation(&working, report) {
        let Some(accepted) = create_reconciliation_accepted(&working, report, ts_now) else {
            log::warn!(
                "Cannot create reconciliation acceptance for {}: missing account_id",
                order.client_order_id(),
            );
            return (working, events);
        };

        if let Err(e) = working.apply(accepted.clone()) {
            log::warn!(
                "Failed to pre-apply reconciliation acceptance for {}: {e}",
                order.client_order_id(),
            );
            return (working, events);
        }
        events.push(accepted);
    }

    if report_is_confirmed_state(report)
        && (local_accepts_amendment(&working)
            || (matches!(
                working.status(),
                OrderStatus::PendingUpdate | OrderStatus::PendingCancel,
            ) && matches!(
                report.order_status,
                OrderStatus::Canceled | OrderStatus::Expired,
            )))
        && should_reconciliation_update(&working, report)
    {
        let updated = create_reconciliation_updated(&working, report, ts_now);
        if let Err(e) = working.apply(updated.clone()) {
            log::warn!(
                "Failed to pre-apply reconciliation update for {}: {e}",
                order.client_order_id(),
            );
        } else {
            events.push(updated);
        }
    }

    (working, events)
}

/// Reconciles an order with a venue status report, generating appropriate events.
///
/// This is the core reconciliation logic that handles all order status transitions.
/// For the higher-level wrapper that emits a venue-temporal sequence of events,
/// use [`generate_reconciliation_order_events`].
///
/// Returns `None` for pending venue states (`PendingUpdate`, `PendingCancel`)
/// regardless of local state, since an unconfirmed amend or cancel must not
/// drive any local mutation until the venue surfaces a confirmed status.
///
/// Returns `None` for a `Canceled` report that references a previously-promoted
/// `venue_order_id` whose successor is still live in the cache (the cancel-half
/// of a cancel-replace modify).
#[must_use]
pub fn reconcile_order_report(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    reconcile_order_report_with_commission(order, report, instrument, ts_now, None)
}

/// Reconciles an order with a venue status report using a precomputed inferred-fill commission.
#[must_use]
pub fn reconcile_order_report_with_commission(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    if matches!(
        report.order_status,
        OrderStatus::PendingUpdate | OrderStatus::PendingCancel
    ) {
        log::debug!(
            "Order {} venue report in pending state: {:?}",
            order.client_order_id(),
            report.order_status,
        );
        return None;
    }

    if is_unchanged_accepted_report_during_pending_command(order, report) {
        log::debug!(
            "Order {} remains inflight while venue reports an unchanged accepted snapshot",
            order.client_order_id(),
        );
        return None;
    }

    if is_superseded_cancel_report(order, report) {
        let cached_venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);
        log::info!(
            "Suppressing Canceled for {} on previously-promoted venue_order_id {}: \
             current venue_order_id is {}",
            order.client_order_id(),
            report.venue_order_id,
            cached_venue_order_id,
        );
        return None;
    }

    if order.status() == report.order_status && order.filled_qty() == report.filled_qty {
        if should_reconciliation_update(order, report) {
            log::info!(
                "Order {} has been updated at venue: qty={}->{}, price={:?}->{:?}",
                order.client_order_id(),
                order.quantity(),
                report.quantity,
                order.price(),
                report.price
            );
            return Some(create_reconciliation_updated(order, report, ts_now));
        }
        return None; // Already in sync
    }

    match report.order_status {
        OrderStatus::Accepted => {
            if order.status() == OrderStatus::Accepted
                && should_reconciliation_update(order, report)
            {
                return Some(create_reconciliation_updated(order, report, ts_now));
            }
            create_reconciliation_accepted(order, report, ts_now)
        }
        OrderStatus::Rejected => {
            create_reconciliation_rejected(order, report.cancel_reason.as_deref(), ts_now)
        }
        OrderStatus::Triggered => {
            if TRIGGERABLE_ORDER_TYPES.contains(&order.order_type()) {
                Some(create_reconciliation_triggered(order, report, ts_now))
            } else {
                log::debug!(
                    "Skipping OrderTriggered for {} order {}: market-style stops have no TRIGGERED state",
                    order.order_type(),
                    order.client_order_id(),
                );
                None
            }
        }
        OrderStatus::Canceled => Some(create_reconciliation_canceled(order, report, ts_now)),
        OrderStatus::Expired => Some(create_reconciliation_expired(order, report, ts_now)),

        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            reconcile_fill_quantity_mismatch(order, report, instrument, ts_now, commission)
        }

        OrderStatus::Voided => {
            create_reconciliation_terminal_fill_void(order, report, instrument, ts_now)
        }

        OrderStatus::PendingUpdate | OrderStatus::PendingCancel => None,

        // Internal states - should not appear in venue reports
        OrderStatus::Initialized
        | OrderStatus::Submitted
        | OrderStatus::Denied
        | OrderStatus::Emulated
        | OrderStatus::Released => {
            log::warn!(
                "Unexpected order status in venue report for {}: {:?}",
                order.client_order_id(),
                report.order_status
            );
            None
        }
    }
}

fn is_unchanged_accepted_report_during_pending_command(
    order: &OrderAny,
    report: &OrderStatusReport,
) -> bool {
    matches!(
        order.status(),
        OrderStatus::PendingUpdate | OrderStatus::PendingCancel
    ) && report.order_status == OrderStatus::Accepted
        && order.venue_order_id() == Some(report.venue_order_id)
        && order.filled_qty() == report.filled_qty
        && !should_reconciliation_update(order, report)
}

/// Generates the appropriate order events for an external order and order status report.
///
/// After creating an external order, we need to transition it to its actual state
/// based on the order status report from the venue. For terminal states like
/// Canceled/Expired/Filled, we return multiple events to properly transition
/// the order through Accepted before reaching the terminal state.
pub fn generate_external_order_status_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    generate_external_order_status_events_with_commission(
        order, report, account_id, instrument, ts_now, None,
    )
}

/// Generates external-order status events with a precomputed inferred-fill commission.
#[must_use]
pub fn generate_external_order_status_events_with_commission(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Vec<OrderEventAny> {
    let accepted = OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.venue_order_id,
        *account_id,
        UUID4::new(),
        report.ts_accepted,
        ts_now,
        true, // reconciliation
    ));

    match report.order_status {
        OrderStatus::Accepted | OrderStatus::Triggered => vec![accepted],
        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            let mut events = vec![accepted];

            if !report.filled_qty.is_zero()
                && let Some(filled) =
                    create_inferred_fill(order, report, *account_id, instrument, ts_now, commission)
            {
                events.push(filled);
            }

            events
        }
        OrderStatus::Voided => {
            let mut working = order.clone();
            if let Err(e) = working.apply(accepted.clone()) {
                log::warn!(
                    "Cannot project external order acceptance for {}: {e}",
                    order.client_order_id()
                );
                return vec![accepted];
            }
            let mut events = vec![accepted];

            if !report.filled_qty.is_zero()
                && let Some(filled) =
                    create_inferred_fill(order, report, *account_id, instrument, ts_now, commission)
            {
                if let Err(e) = working.apply(filled.clone()) {
                    log::warn!(
                        "Cannot project external order fill for {}: {e}",
                        order.client_order_id()
                    );
                } else {
                    events.push(filled);
                }
            }

            if let Some(voided) =
                create_reconciliation_terminal_fill_void(&working, report, Some(instrument), ts_now)
            {
                events.push(voided);
            }
            events
        }
        OrderStatus::Canceled | OrderStatus::Expired => {
            let terminal = create_external_terminal_event(order, report, *account_id, ts_now);
            let mut events = vec![accepted];

            let inferred_fill = if report.filled_qty.is_zero() {
                None
            } else {
                create_inferred_fill(order, report, *account_id, instrument, ts_now, commission)
            };
            let filled_to_quantity =
                inferred_fill.is_some() && report.filled_qty >= report.quantity;
            if let Some(filled) = inferred_fill {
                events.push(filled);
            }

            if !filled_to_quantity {
                events.push(terminal);
            }
            events
        }
        OrderStatus::Rejected => {
            // Rejected goes directly to terminal state without acceptance
            let reason = report.cancel_reason.as_deref().unwrap_or("UNKNOWN");
            vec![OrderEventAny::Rejected(OrderRejected::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                *account_id,
                Ustr::from(reason),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                reason_indicates_post_only_rejection(reason),
            ))]
        }
        _ => {
            log::warn!(
                "Unhandled order status {} for external order {}",
                report.order_status,
                order.client_order_id()
            );
            Vec::new()
        }
    }
}

fn create_reconciliation_fill_voids(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    let mut remaining = order.filled_qty() - report.filled_qty;
    let order_events = order.events();
    let mut corrections = Vec::new();

    for candidate in order_events.iter().rev() {
        let OrderEventAny::Filled(fill) = candidate else {
            continue;
        };

        if remaining.is_zero() {
            break;
        }
        let previous = order_events.iter().rev().find_map(|event| match event {
            OrderEventAny::FillVoided(voided) if voided.trade_id == fill.trade_id => Some(voided),
            _ => None,
        });
        let prior_qty = previous.map_or_else(
            || Quantity::zero(fill.last_qty.precision),
            |voided| voided.voided_qty.min(fill.last_qty),
        );
        let effective = fill.last_qty - prior_qty;
        if effective.is_zero() {
            continue;
        }
        let removed = remaining.min(effective);
        let voided_qty = prior_qty + removed;
        let commission_voided = fill.commission.and_then(|commission| {
            let fraction = voided_qty.as_decimal() / fill.last_qty.as_decimal();
            Money::from_decimal(commission.as_decimal() * fraction, commission.currency).ok()
        });
        let mut event = OrderFillVoided::new(
            fill.trader_id,
            fill.strategy_id,
            fill.instrument_id,
            fill.client_order_id,
            fill.venue_order_id,
            fill.account_id,
            Ustr::from(&format!(
                "reconciliation-{}-{}",
                report.report_id, fill.trade_id
            )),
            fill.trade_id,
            voided_qty,
            commission_voided,
            fill.order_side,
            fill.order_type,
            fill.last_px,
            fill.currency,
            fill.liquidity_side,
            fill.position_id,
            report.cancel_reason.as_deref().map(Ustr::from),
            None,
            UUID4::new(),
            report.ts_last,
            ts_now,
            true,
            report_is_working(report),
        );
        event.causation_id = Some(report.report_id);
        corrections.push(OrderEventAny::FillVoided(event));
        remaining = remaining - removed;
    }

    if !remaining.is_zero() {
        log::warn!(
            "Cannot reconcile fill decrease for {}: {} is outside retained fill history",
            order.client_order_id(),
            remaining
        );
        return Vec::new();
    }
    corrections
}

/// Creates a fill void reversing the remaining leaves of a terminally voided order.
///
/// An unresolved price falls back to zero rather than `None`: the price is a placeholder that
/// never reaches `Position::avg_px_open`, whereas suppressing the void would leave the order
/// open locally against a venue that considers it gone.
fn create_reconciliation_terminal_fill_void(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let voided_qty = order.leaves_qty();
    if voided_qty.is_zero() {
        return None;
    }
    let instrument = instrument?;
    let order_side = order.order_side();

    let last_px = resolve_fill_price(order, report, instrument)
        .unwrap_or_else(|| Price::zero(instrument.price_precision()));

    let mut event = OrderFillVoided::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.venue_order_id,
        report.account_id,
        Ustr::from(&format!("reconciliation-{}", report.report_id)),
        TradeId::new(format!("VOID-{}", report.venue_order_id)),
        voided_qty,
        None,
        order_side,
        order.order_type(),
        last_px,
        instrument.quote_currency(),
        LiquiditySide::NoLiquiditySide,
        None,
        report.cancel_reason.as_deref().map(Ustr::from),
        None,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true,
        false,
    );
    event.causation_id = Some(report.report_id);
    Some(OrderEventAny::FillVoided(event))
}

fn create_external_terminal_event(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: AccountId,
    ts_now: UnixNanos,
) -> OrderEventAny {
    match report.order_status {
        OrderStatus::Canceled => OrderEventAny::Canceled(OrderCanceled::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            report.ts_last,
            ts_now,
            true, // reconciliation
            Some(report.venue_order_id),
            Some(account_id),
            report.cancel_reason.as_deref().map(Ustr::from),
        )),
        OrderStatus::Expired => OrderEventAny::Expired(OrderExpired::new(
            order.trader_id(),
            order.strategy_id(),
            order.instrument_id(),
            order.client_order_id(),
            UUID4::new(),
            report.ts_last,
            ts_now,
            true, // reconciliation
            Some(report.venue_order_id),
            Some(account_id),
        )),
        status => unreachable!("cannot create external terminal event for {status}"),
    }
}

/// Creates an `OrderFilled` event from a `FillReport`.
///
/// This is used during reconciliation when a fill report is received from the venue.
/// Returns `None` if the fill is a duplicate or would cause an overfill.
pub fn reconcile_fill_report(
    order: &OrderAny,
    report: &FillReport,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    allow_overfills: bool,
) -> Option<OrderEventAny> {
    debug_assert!(
        !report.last_qty.is_zero(),
        "fill report last_qty must be non-zero for {}",
        order.client_order_id(),
    );

    if order.trade_ids().iter().any(|id| **id == report.trade_id) {
        log::debug!(
            "Duplicate fill detected: trade_id {} already exists for order {}",
            report.trade_id,
            order.client_order_id()
        );
        return None;
    }

    let potential_filled_qty = order.filled_qty() + report.last_qty;
    if potential_filled_qty > order.quantity() {
        if !allow_overfills {
            log::warn!(
                "Rejecting fill that would cause overfill for {}: order.quantity={}, order.filled_qty={}, fill.last_qty={}, would result in filled_qty={}",
                order.client_order_id(),
                order.quantity(),
                order.filled_qty(),
                report.last_qty,
                potential_filled_qty
            );
            return None;
        }
        log::warn!(
            "Allowing overfill during reconciliation for {}: order.quantity={}, order.filled_qty={}, fill.last_qty={}, will result in filled_qty={}",
            order.client_order_id(),
            order.quantity(),
            order.filled_qty(),
            report.last_qty,
            potential_filled_qty
        );
    }

    let account_id = report.account_id;
    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);

    log::info!(
        color = LogColor::Blue as u8;
        "Reconciling fill for {}: qty={}, px={}, trade_id={}",
        order.client_order_id(),
        report.last_qty,
        report.last_px,
        report.trade_id,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        account_id,
        report.trade_id,
        order.order_side(),
        order.order_type(),
        report.last_qty,
        report.last_px,
        instrument.quote_currency(),
        report.liquidity_side,
        UUID4::new(),
        report.ts_event,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        Some(report.commission),
        None,
    )))
}

/// Checks if the order should be updated based on quantity, price, or trigger price
/// differences from the venue report.
///
/// A `None` value in `report.price` or `report.trigger_price` is treated as
/// "venue did not include this field" rather than as drift, so a partial
/// snapshot (for example, a Filled report that omits price but supplies
/// `avg_px`) does not trigger a spurious `OrderUpdated`.
pub fn should_reconciliation_update(order: &OrderAny, report: &OrderStatusReport) -> bool {
    if report.quantity != order.quantity() && report.quantity >= order.filled_qty() {
        return true;
    }

    let price_drift = report.price.is_some() && report.price != order.price();
    let trigger_drift =
        report.trigger_price.is_some() && report.trigger_price != order.trigger_price();

    match order.order_type() {
        OrderType::Limit => price_drift,
        OrderType::StopMarket | OrderType::TrailingStopMarket | OrderType::MarketIfTouched => {
            trigger_drift
        }
        OrderType::StopLimit | OrderType::TrailingStopLimit | OrderType::LimitIfTouched => {
            trigger_drift || price_drift
        }
        _ => false,
    }
}

/// Creates an `OrderAccepted` event for reconciliation.
#[must_use]
pub(super) fn create_reconciliation_accepted(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let account_id = order.account_id()?;

    Some(OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.venue_order_id().unwrap_or(report.venue_order_id),
        account_id,
        UUID4::new(),
        report.ts_accepted,
        ts_now,
        true, // reconciliation
    )))
}

/// Creates an `OrderRejected` event for reconciliation.
#[must_use]
pub fn create_reconciliation_rejected(
    order: &OrderAny,
    reason: Option<&str>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
    let account_id = order.account_id()?;
    let reason = reason.unwrap_or("UNKNOWN");

    Some(OrderEventAny::Rejected(OrderRejected::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        account_id,
        Ustr::from(reason),
        UUID4::new(),
        ts_now,
        ts_now,
        true, // reconciliation
        reason_indicates_post_only_rejection(reason),
    )))
}

fn reason_indicates_post_only_rejection(reason: &str) -> bool {
    let normalized: String = reason
        .chars()
        .filter_map(|ch| {
            if ch == '-' || ch == '_' || ch.is_whitespace() {
                None
            } else {
                Some(ch.to_ascii_lowercase())
            }
        })
        .collect();

    normalized.contains("postonly") || normalized.contains("postwouldexecute")
}

/// Creates an `OrderTriggered` event for reconciliation.
#[must_use]
pub fn create_reconciliation_triggered(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Triggered(OrderTriggered::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_triggered.unwrap_or(ts_now),
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
    ))
}

/// Creates an `OrderCanceled` event for reconciliation.
#[must_use]
pub(super) fn create_reconciliation_canceled(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Canceled(OrderCanceled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
        report.cancel_reason.as_deref().map(Ustr::from),
    ))
}

/// Creates an `OrderExpired` event for reconciliation.
#[must_use]
pub(super) fn create_reconciliation_expired(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Expired(OrderExpired::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
    ))
}

/// Creates an `OrderUpdated` event for reconciliation.
#[must_use]
pub(super) fn create_reconciliation_updated(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    // Only pass trigger_price for order types that support it.
    // Limit, Market, and MarketToLimit orders assert trigger_price.is_none()
    // in their update() methods - passing a spurious trigger_price from the
    // venue report (e.g. Bybit sends "0.00" for non-conditional orders)
    // causes a panic. Positive list ensures new order types without
    // trigger_price support won't accidentally receive one.
    let trigger_price = match order.order_type() {
        OrderType::StopMarket
        | OrderType::StopLimit
        | OrderType::MarketIfTouched
        | OrderType::LimitIfTouched
        | OrderType::TrailingStopMarket
        | OrderType::TrailingStopLimit => report.trigger_price,
        _ => None,
    };

    OrderEventAny::Updated(OrderUpdated::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.quantity,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        order.venue_order_id(),
        order.account_id(),
        report.price,
        trigger_price,
        None, // protection_price
        order.is_quote_quantity(),
    ))
}

/// Creates an inferred fill event for reconciliation when fill reports are missing.
pub(super) fn create_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    let cached_order_side = order.order_side();
    let order_side = report.order_side.unwrap_or(cached_order_side);
    if order_side != cached_order_side {
        log::warn!(
            "Order side mismatch for {}: cached={:?}, venue={order_side:?}",
            order.client_order_id(),
            cached_order_side,
        );
    }

    let liquidity_side = match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let Some(last_px) = resolve_fill_price(order, report, instrument) else {
        log::warn!(
            "Cannot create inferred fill for {}: no avg_px, report price, or order price",
            order.client_order_id()
        );

        return None;
    };
    let last_px = clamp_inferred_fill_price(last_px, instrument);

    let position_id = reconciliation_position_id(report, instrument);
    let trade_id = create_inferred_reconciliation_trade_id(
        account_id,
        order.instrument_id(),
        order.client_order_id(),
        Some(report.venue_order_id),
        order_side,
        order.order_type(),
        report.filled_qty,
        report.filled_qty,
        last_px,
        position_id,
        report.ts_last,
    );

    log::info!(
        "Generated inferred fill for {} ({}) qty={} px={}",
        order.client_order_id(),
        report.venue_order_id,
        report.filled_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        report.venue_order_id,
        account_id,
        trade_id,
        order_side,
        order.order_type(),
        report.filled_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        commission,
        None,
    )))
}

/// Creates an inferred fill for the quantity difference between order and report.
pub fn create_incremental_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    let order_side = order.order_side();

    let order_filled_qty = order.filled_qty();
    debug_assert!(
        report.filled_qty >= order_filled_qty,
        "incremental inferred fill requires report.filled_qty ({}) >= order.filled_qty ({}) for {}",
        report.filled_qty,
        order_filled_qty,
        order.client_order_id(),
    );
    let last_qty = report.filled_qty - order_filled_qty;

    if last_qty <= Quantity::zero(instrument.size_precision()) {
        return None;
    }

    let (last_px, liquidity_side) =
        incremental_inferred_fill_price_and_liquidity(order, report, instrument)?;

    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);
    let position_id = reconciliation_position_id(report, instrument);
    let trade_id = create_inferred_reconciliation_trade_id(
        *account_id,
        order.instrument_id(),
        order.client_order_id(),
        Some(venue_order_id),
        order_side,
        order.order_type(),
        report.filled_qty,
        last_qty,
        last_px,
        position_id,
        report.ts_last,
    );

    log::info!(
        color = LogColor::Blue as u8;
        "Generated inferred fill for {}: qty={}, px={}",
        order.client_order_id(),
        last_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        *account_id,
        trade_id,
        order_side,
        order.order_type(),
        last_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        commission,
        None,
    )))
}

/// Resolves the price and liquidity side that an incremental inferred fill will carry.
///
/// This uses the cached order's filled quantity and average price to derive the price of only the
/// unbooked quantity. Callers that calculate commission for an incremental fill must use these
/// values rather than the venue report's cumulative average.
///
/// Returns `None` when no fill price can be determined from the order, report, or instrument.
#[must_use]
pub fn incremental_inferred_fill_price_and_liquidity(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> Option<(Price, LiquiditySide)> {
    let last_px = calculate_incremental_fill_price(order, report, instrument)?;

    Some((
        clamp_inferred_fill_price(last_px, instrument),
        inferred_fill_liquidity_side(order),
    ))
}

/// Resolves the price and liquidity side that an inferred fill will carry.
///
/// Callers that need the venue commission for an inferred fill resolve these values first, so the
/// price and liquidity rules stay defined here rather than being restated at each call site.
///
/// Returns `None` when no fill price can be determined from the order, report, or instrument.
#[must_use]
pub fn inferred_fill_price_and_liquidity(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> Option<(Price, LiquiditySide)> {
    let last_px = resolve_fill_price(order, report, instrument)?;

    Some((
        clamp_inferred_fill_price(last_px, instrument),
        inferred_fill_liquidity_side(order),
    ))
}

fn inferred_fill_liquidity_side(order: &OrderAny) -> LiquiditySide {
    match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    }
}

/// Creates an inferred fill with a specific quantity.
///
/// Unlike `create_incremental_inferred_fill`, this takes the fill quantity directly
/// rather than calculating it from order state. Useful when order state hasn't been
/// updated yet (e.g., during external order processing).
pub fn create_inferred_fill_for_qty(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    fill_qty: Quantity,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    if fill_qty.is_zero() {
        return None;
    }

    let order_side = order.order_side();

    let Some((last_px, liquidity_side)) =
        inferred_fill_price_and_liquidity(order, report, instrument)
    else {
        log::warn!(
            "Cannot determine fill price for {}: no avg_px, report price, or order price",
            order.client_order_id()
        );

        return None;
    };

    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);
    let position_id = reconciliation_position_id(report, instrument);
    let trade_id = create_inferred_reconciliation_trade_id(
        *account_id,
        order.instrument_id(),
        order.client_order_id(),
        Some(venue_order_id),
        order_side,
        order.order_type(),
        report.filled_qty,
        fill_qty,
        last_px,
        position_id,
        report.ts_last,
    );

    log::info!(
        color = LogColor::Blue as u8;
        "Generated inferred fill for {}: qty={}, px={}",
        order.client_order_id(),
        fill_qty,
        last_px,
    );

    Some(OrderEventAny::Filled(OrderFilled::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        venue_order_id,
        *account_id,
        trade_id,
        order_side,
        order.order_type(),
        fill_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        report.venue_position_id,
        commission,
        None,
    )))
}

fn report_is_confirmed_state(report: &OrderStatusReport) -> bool {
    matches!(
        report.order_status,
        OrderStatus::Accepted
            | OrderStatus::Triggered
            | OrderStatus::PartiallyFilled
            | OrderStatus::Filled
            | OrderStatus::Canceled
            | OrderStatus::Expired
    )
}

fn report_is_working(report: &OrderStatusReport) -> bool {
    matches!(
        report.order_status,
        OrderStatus::Accepted | OrderStatus::Triggered | OrderStatus::PartiallyFilled
    )
}

pub(crate) fn is_superseded_cancel_report(order: &OrderAny, report: &OrderStatusReport) -> bool {
    if report.order_status != OrderStatus::Canceled {
        return false;
    }

    let Some(cached_venue_order_id) = order.venue_order_id() else {
        return false;
    };

    cached_venue_order_id != report.venue_order_id
        && order
            .venue_order_ids()
            .iter()
            .any(|venue_order_id| **venue_order_id == report.venue_order_id)
}

fn local_accepts_amendment(order: &OrderAny) -> bool {
    matches!(
        order.status(),
        OrderStatus::Accepted | OrderStatus::Triggered | OrderStatus::PartiallyFilled
    )
}

fn should_accept_before_reconciliation(order: &OrderAny, report: &OrderStatusReport) -> bool {
    order.status() == OrderStatus::Submitted && report.order_status != OrderStatus::Rejected
}

/// Handles fill quantity mismatch between cached order and venue report.
///
/// Returns an inferred fill event if the venue reports more filled quantity than we have.
fn reconcile_fill_quantity_mismatch(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    let order_filled_qty = order.filled_qty();
    let report_filled_qty = report.filled_qty;

    if report_filled_qty < order_filled_qty {
        // Venue cumulative below cached: apply no event so cached state is
        // preserved. Suppress sub-unit gaps as precision noise.
        let precision = order_filled_qty.precision.max(report_filled_qty.precision);
        if is_within_single_unit_tolerance(
            report_filled_qty.as_decimal(),
            order_filled_qty.as_decimal(),
            precision,
        ) {
            return None;
        }

        log::warn!(
            "Fill qty mismatch for {} ({}): cached={}, venue={}, order_qty={} (venue < cached)",
            order.client_order_id(),
            report.venue_order_id,
            order_filled_qty,
            report_filled_qty,
            order.quantity(),
        );
        return None;
    }

    if report_filled_qty > order_filled_qty {
        // Check if order is already closed - skip inferred fill to avoid invalid state
        // (matching Python behavior in _handle_fill_quantity_mismatch)
        if order.is_closed() {
            let precision = order_filled_qty.precision.max(report_filled_qty.precision);

            if is_within_single_unit_tolerance(
                report_filled_qty.as_decimal(),
                order_filled_qty.as_decimal(),
                precision,
            ) {
                return None;
            }

            log::debug!(
                "{} {} already closed but reported difference in filled_qty: \
                report={}, cached={}, skipping inferred fill generation for closed order",
                order.instrument_id(),
                order.client_order_id(),
                report_filled_qty,
                order_filled_qty,
            );
            return None;
        }

        // Venue has more fills - generate inferred fill for the difference
        let Some(instrument) = instrument else {
            log::warn!(
                "Cannot generate inferred fill for {}: instrument not available",
                order.client_order_id()
            );
            return None;
        };

        let account_id = order.account_id()?;
        return create_incremental_inferred_fill(
            order,
            report,
            &account_id,
            instrument,
            ts_now,
            commission,
        );
    }

    // Quantities match but status differs: if the venue reduced the order
    // quantity (e.g. partial cancel leaving filled_qty==quantity), emit
    // OrderUpdated so the local state machine can transition; do not
    // synthesize a fill since filled_qty already matches.
    if order.status() != report.order_status {
        if should_reconciliation_update(order, report) {
            log::info!(
                "Status mismatch with matching fill qty for {}: local={:?}, venue={:?}, \
                 filled_qty={}, updating quantity {}->{}",
                order.client_order_id(),
                order.status(),
                report.order_status,
                report.filled_qty,
                order.quantity(),
                report.quantity,
            );
            return Some(create_reconciliation_updated(order, report, ts_now));
        }

        log::warn!(
            "Status mismatch with matching fill qty for {}: local={:?}, venue={:?}, filled_qty={}",
            order.client_order_id(),
            order.status(),
            report.order_status,
            report.filled_qty
        );
    }

    None
}

/// Calculates the fill price for an incremental inferred fill.
///
/// The back-solve is guarded because nothing downstream rejects a negative price:
/// [`Price`] admits negatives, [`clamp_inferred_fill_price`] caps only the upper bound,
/// and `Position` takes `OrderFilled::last_px` straight into `avg_px_open` and PnL.
fn calculate_incremental_fill_price(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> Option<Price> {
    let order_filled_qty = order.filled_qty();
    debug_assert!(
        report.filled_qty >= order_filled_qty,
        "incremental fill price requires report.filled_qty ({}) >= order.filled_qty ({}) for {}",
        report.filled_qty,
        order_filled_qty,
        order.client_order_id(),
    );

    // First fill - nothing booked locally to difference against
    if order_filled_qty.is_zero() {
        let last_px = resolve_fill_price(order, report, instrument);
        if last_px.is_none() {
            log::warn!(
                "Cannot determine fill price for {}: no avg_px, report price, or order price",
                order.client_order_id()
            );
        }

        return last_px;
    }

    // Incremental fill - calculate price using weighted average
    if let Some(report_avg_px) = report.avg_px {
        let last_px_decimal = match order.avg_px() {
            // No previous average to difference against
            None => report_avg_px,
            Some(order_avg_px) => {
                let report_filled_qty = report.filled_qty;
                let last_qty = report_filled_qty - order_filled_qty;

                let report_notional = report_avg_px * report_filled_qty.as_decimal();
                let order_notional = order_avg_px * order_filled_qty.as_decimal();
                let last_notional = report_notional - order_notional;
                let back_solved = last_notional / last_qty.as_decimal();

                if back_solved < Decimal::ZERO && !instrument.allows_negative_price() {
                    if report_avg_px < Decimal::ZERO {
                        log::warn!(
                            "Cannot price inferred fill for {}: back-solved {back_solved} and venue average {report_avg_px} are both negative on an instrument that disallows negative prices",
                            order.client_order_id(),
                        );

                        return None;
                    }

                    log::warn!(
                        "Negative back-solved fill price {back_solved} for {}, using venue average {report_avg_px}",
                        order.client_order_id(),
                    );

                    report_avg_px
                } else {
                    back_solved
                }
            }
        };

        return Price::from_decimal_dp(last_px_decimal, instrument.price_precision())
            .inspect_err(|e| {
                log::warn!(
                    "Cannot price {} from incremental {last_px_decimal}, falling back: {e}",
                    order.client_order_id(),
                );
            })
            .ok()
            .or_else(|| resolve_fill_price(order, report, instrument));
    }

    resolve_fill_price(order, report, instrument)
}

/// Resolves a fill price from the venue report, falling back to the order.
///
/// Rungs run from most to least direct evidence of what executed. Callers decide what an
/// unresolved price means: booking a fill at an invented price corrupts position averages,
/// whereas a void event only needs a placeholder.
fn resolve_fill_price(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> Option<Price> {
    report
        .avg_px
        .and_then(|avg_px| {
            Price::from_decimal_dp(avg_px, instrument.price_precision())
                .inspect_err(|e| {
                    log::warn!(
                        "Cannot price {} from venue average {avg_px}, trying next source: {e}",
                        order.client_order_id(),
                    );
                })
                .ok()
        })
        .or(report.price)
        .or_else(|| order.price())
}

/// Caps an inferred fill price at the instrument's maximum price.
///
/// Thin `Price` wrapper over [`cap_price_at_instrument_max`]; see that
/// function for why reconciliation needs to bound synthetic fill prices.
fn clamp_inferred_fill_price(price: Price, instrument: &InstrumentAny) -> Price {
    let px = cap_price_at_instrument_max(price.as_decimal(), instrument);
    Price::from_decimal_dp(px, instrument.price_precision()).unwrap_or(price)
}

fn reconciliation_position_id(
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> PositionId {
    report
        .venue_position_id
        .unwrap_or_else(|| PositionId::new(format!("{}-EXTERNAL", instrument.id())))
}
