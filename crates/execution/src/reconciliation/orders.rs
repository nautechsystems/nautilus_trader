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
//! Event constructors, order state reconciliation, and fill reconciliation. Every
//! helper turns a venue-sourced report into zero or more `OrderEventAny`s that are
//! safe to apply to the local order model.

use std::str::FromStr;

use nautilus_common::enums::LogColor;
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{LiquiditySide, OrderStatus, OrderType},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, PositionId},
    instruments::{Instrument, InstrumentAny},
    orders::{Order, OrderAny, TRIGGERABLE_ORDER_TYPES},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    ids::create_inferred_reconciliation_trade_id, positions::is_within_single_unit_tolerance,
};

fn reconciliation_position_id(
    report: &OrderStatusReport,
    instrument: &InstrumentAny,
) -> PositionId {
    report
        .venue_position_id
        .unwrap_or_else(|| PositionId::new(format!("{}-EXTERNAL", instrument.id())))
}

pub fn generate_external_order_status_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
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
                    create_inferred_fill(order, report, account_id, instrument, ts_now, None)
            {
                events.push(filled);
            }

            events
        }
        OrderStatus::Canceled => {
            let canceled = OrderEventAny::Canceled(OrderCanceled::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                Some(report.venue_order_id),
                Some(*account_id),
            ));
            vec![accepted, canceled]
        }
        OrderStatus::Expired => {
            let expired = OrderEventAny::Expired(OrderExpired::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                Some(report.venue_order_id),
                Some(*account_id),
            ));
            vec![accepted, expired]
        }
        OrderStatus::Rejected => {
            // Rejected goes directly to terminal state without acceptance
            vec![OrderEventAny::Rejected(OrderRejected::new(
                order.trader_id(),
                order.strategy_id(),
                order.instrument_id(),
                order.client_order_id(),
                *account_id,
                Ustr::from(report.cancel_reason.as_deref().unwrap_or("UNKNOWN")),
                UUID4::new(),
                report.ts_last,
                ts_now,
                true, // reconciliation
                false,
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

/// Creates an inferred fill event for reconciliation when fill reports are missing.
pub fn create_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
    let liquidity_side = match order.order_type() {
        OrderType::Market | OrderType::StopMarket | OrderType::TrailingStopMarket => {
            LiquiditySide::Taker
        }
        _ if report.post_only => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = if let Some(avg_px) = report.avg_px {
        match Price::from_decimal_dp(avg_px, instrument.price_precision()) {
            Ok(px) => px,
            Err(e) => {
                log::warn!("Failed to create price from avg_px for inferred fill: {e}");
                return None;
            }
        }
    } else if let Some(price) = report.price {
        price
    } else {
        log::warn!(
            "Cannot create inferred fill for {}: no avg_px or price available",
            order.client_order_id()
        );
        return None;
    };

    let position_id = reconciliation_position_id(report, instrument);
    let trade_id = create_inferred_reconciliation_trade_id(
        *account_id,
        order.instrument_id(),
        order.client_order_id(),
        Some(report.venue_order_id),
        report.order_side,
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
        *account_id,
        trade_id,
        report.order_side,
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
    )))
}

/// Creates an OrderAccepted event for reconciliation.
///
/// # Panics
///
/// Panics if the order does not have an `account_id` set.
#[must_use]
pub fn create_reconciliation_accepted(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    OrderEventAny::Accepted(OrderAccepted::new(
        order.trader_id(),
        order.strategy_id(),
        order.instrument_id(),
        order.client_order_id(),
        order.venue_order_id().unwrap_or(report.venue_order_id),
        order
            .account_id()
            .expect("Order should have account_id for reconciliation"),
        UUID4::new(),
        report.ts_accepted,
        ts_now,
        true, // reconciliation
    ))
}

/// Creates an OrderRejected event for reconciliation.
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
        true,  // reconciliation
        false, // due_post_only
    )))
}

/// Creates an OrderTriggered event for reconciliation.
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

/// Creates an OrderCanceled event for reconciliation.
#[must_use]
pub fn create_reconciliation_canceled(
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
    ))
}

/// Creates an OrderExpired event for reconciliation.
#[must_use]
pub fn create_reconciliation_expired(
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

/// Creates an OrderUpdated event for reconciliation.
#[must_use]
pub fn create_reconciliation_updated(
    order: &OrderAny,
    report: &OrderStatusReport,
    ts_now: UnixNanos,
) -> OrderEventAny {
    // Only pass trigger_price for order types that support it.
    // Limit, Market, and MarketToLimit orders assert trigger_price.is_none()
    // in their update() methods — passing a spurious trigger_price from the
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

/// Checks if the order should be updated based on quantity, price, or trigger price
/// differences from the venue report.
pub fn should_reconciliation_update(order: &OrderAny, report: &OrderStatusReport) -> bool {
    // Quantity change only valid if new qty >= filled qty
    if report.quantity != order.quantity() && report.quantity >= order.filled_qty() {
        return true;
    }

    match order.order_type() {
        OrderType::Limit => report.price != order.price(),
        OrderType::StopMarket | OrderType::TrailingStopMarket => {
            report.trigger_price != order.trigger_price()
        }
        OrderType::StopLimit | OrderType::TrailingStopLimit => {
            report.trigger_price != order.trigger_price() || report.price != order.price()
        }
        _ => false,
    }
}

/// Reconciles an order with a venue status report, generating appropriate events.
///
/// This is the core reconciliation logic that handles all order status transitions.
/// For fill reconciliation with inferred fills, use `reconcile_order_with_fills`.
#[must_use]
pub fn reconcile_order_report(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Option<OrderEventAny> {
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
            Some(create_reconciliation_accepted(order, report, ts_now))
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
            reconcile_fill_quantity_mismatch(order, report, instrument, ts_now)
        }

        // Pending states - venue will confirm, just log
        OrderStatus::PendingUpdate | OrderStatus::PendingCancel => {
            log::debug!(
                "Order {} in pending state: {:?}",
                order.client_order_id(),
                report.order_status
            );
            None
        }

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

/// Generates reconciliation events for a live order status report.
///
/// If a venue report advances a locally submitted order beyond `Submitted`,
/// this synthesizes the missing `Accepted` event first so downstream order
/// state transitions stay valid.
#[must_use]
pub fn generate_reconciliation_order_events(
    order: &OrderAny,
    report: &OrderStatusReport,
    instrument: Option<&InstrumentAny>,
    ts_now: UnixNanos,
) -> Vec<OrderEventAny> {
    if should_accept_before_reconciliation(order, report) {
        let accepted = create_reconciliation_accepted(order, report, ts_now);
        let mut accepted_order = order.clone();

        if let Err(e) = accepted_order.apply(accepted.clone()) {
            log::warn!(
                "Failed to pre-apply reconciliation acceptance for {}: {e}",
                order.client_order_id(),
            );
            return reconcile_order_report(order, report, instrument, ts_now)
                .into_iter()
                .collect();
        }

        let mut events = vec![accepted];

        if let Some(event) = reconcile_order_report(&accepted_order, report, instrument, ts_now) {
            events.push(event);
        }
        return events;
    }

    reconcile_order_report(order, report, instrument, ts_now)
        .into_iter()
        .collect()
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
) -> Option<OrderEventAny> {
    let order_filled_qty = order.filled_qty();
    let report_filled_qty = report.filled_qty;

    if report_filled_qty < order_filled_qty {
        // Venue reports less filled than we have - potential state corruption
        log::error!(
            "Fill qty mismatch for {}: cached={}, venue={} (venue < cached)",
            order.client_order_id(),
            order_filled_qty,
            report_filled_qty
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
            None,
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

/// Creates an inferred fill for the quantity difference between order and report.
pub fn create_incremental_inferred_fill(
    order: &OrderAny,
    report: &OrderStatusReport,
    account_id: &AccountId,
    instrument: &InstrumentAny,
    ts_now: UnixNanos,
    commission: Option<Money>,
) -> Option<OrderEventAny> {
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

    let liquidity_side = match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = calculate_incremental_fill_price(order, report, instrument)?;

    let venue_order_id = order.venue_order_id().unwrap_or(report.venue_order_id);
    let position_id = reconciliation_position_id(report, instrument);
    let trade_id = create_inferred_reconciliation_trade_id(
        *account_id,
        order.instrument_id(),
        order.client_order_id(),
        Some(venue_order_id),
        order.order_side(),
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
        order.order_side(),
        order.order_type(),
        last_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        None, // venue_position_id
        commission,
    )))
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

    let liquidity_side = match order.order_type() {
        OrderType::Market
        | OrderType::StopMarket
        | OrderType::MarketToLimit
        | OrderType::TrailingStopMarket => LiquiditySide::Taker,
        _ if order.is_post_only() => LiquiditySide::Maker,
        _ => LiquiditySide::NoLiquiditySide,
    };

    let last_px = if let Some(avg_px) = report.avg_px {
        Price::from_decimal_dp(avg_px, instrument.price_precision()).ok()?
    } else if let Some(price) = report.price {
        price
    } else if let Some(price) = order.price() {
        price
    } else {
        log::warn!(
            "Cannot determine fill price for {}: no avg_px or price available",
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
        order.order_side(),
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
        order.order_side(),
        order.order_type(),
        fill_qty,
        last_px,
        instrument.quote_currency(),
        liquidity_side,
        UUID4::new(),
        report.ts_last,
        ts_now,
        true, // reconciliation
        None, // venue_position_id
        commission,
    )))
}

/// Calculates the fill price for an incremental inferred fill.
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

    // First fill - use avg_px from report or order price
    if order_filled_qty.is_zero() {
        if let Some(avg_px) = report.avg_px {
            return Price::from_decimal_dp(avg_px, instrument.price_precision()).ok();
        }

        if let Some(price) = report.price {
            return Some(price);
        }

        if let Some(price) = order.price() {
            return Some(price);
        }
        log::warn!(
            "Cannot determine fill price for {}: no avg_px, report price, or order price",
            order.client_order_id()
        );
        return None;
    }

    // Incremental fill - calculate price using weighted average
    if let Some(report_avg_px) = report.avg_px {
        let Some(order_avg_px) = order.avg_px() else {
            // No previous avg_px, use report avg_px
            return Price::from_decimal_dp(report_avg_px, instrument.price_precision()).ok();
        };
        let report_filled_qty = report.filled_qty;
        let last_qty = report_filled_qty - order_filled_qty;

        let report_notional = report_avg_px * report_filled_qty.as_decimal();
        let order_notional = Decimal::from_str(&order_avg_px.to_string()).unwrap_or_default()
            * order_filled_qty.as_decimal();
        let last_notional = report_notional - order_notional;
        let last_px_decimal = last_notional / last_qty.as_decimal();

        return Price::from_decimal_dp(last_px_decimal, instrument.price_precision()).ok();
    }

    // Fallback to report price or order price
    if let Some(price) = report.price {
        return Some(price);
    }

    order.price()
}

/// Creates an OrderFilled event from a FillReport.
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

    // Use order's account_id if available, fallback to report's account_id
    let account_id = order.account_id().unwrap_or(report.account_id);
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
    )))
}
