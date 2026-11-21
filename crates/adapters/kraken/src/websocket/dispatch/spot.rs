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

//! WebSocket execution dispatch for the Kraken Spot v2 API.
//!
//! A single spot execution can carry both a status update (handled via the
//! order event path) and a fill (when `exec_id` is present, handled via the
//! fill path). Tracked orders emit typed events; external orders fall through
//! to reports.

use std::sync::Arc;

use nautilus_core::{AtomicMap, UUID4, UnixNanos};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderTriggered, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::Quantity,
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    OrderIdentity, WsDispatchState, ensure_accepted_emitted, fill_report_to_order_filled,
    resolve_client_order_id,
};
use crate::{
    common::lookup_instrument_in_snapshot,
    websocket::spot_v2::{
        enums::KrakenExecType,
        messages::KrakenWsExecutionData,
        parse::{
            RetainedOrderFields, parse_ws_fill_report, parse_ws_order_status_report,
            parse_ws_retained_order_status_report,
        },
    },
};

/// What the dispatch keeps of a spot order this client does not track: the fields its frames
/// stated, and whether one reported it triggered.
#[derive(Debug, Clone, Default)]
pub(crate) struct SpotOrderShape {
    retained: RetainedOrderFields,
    triggered: bool,
}

/// Dispatches a Kraken Spot v2 execution message.
#[expect(clippy::too_many_arguments)]
pub fn execution(
    exec: &KrakenWsExecutionData,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_qty_cache: &Arc<AtomicMap<String, Decimal>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    execution_inner(
        exec,
        state,
        emitter,
        instruments,
        truncated_id_map,
        order_qty_cache,
        account_id,
        ts_init,
    );

    // Run terminal cache cleanup regardless of which early return the inner
    // dispatch hit (symbol miss, instrument miss, stale-filled suppression,
    // parse error). Keying eviction off `exec.order_id` means it does not
    // depend on `cl_ord_id` or identity resolution succeeding.
    if is_terminal_exec_type(exec.exec_type) {
        state.forget_order_symbol(&exec.order_id);
        state.forget_order_client_id(&exec.order_id);
        state.forget_spot_order_shape(&exec.order_id);
    }
}

#[expect(clippy::too_many_arguments)]
fn execution_inner(
    exec: &KrakenWsExecutionData,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    instruments: &Arc<AtomicMap<InstrumentId, InstrumentAny>>,
    truncated_id_map: &Arc<AtomicMap<String, ClientOrderId>>,
    order_qty_cache: &Arc<AtomicMap<String, Decimal>>,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    // Resolve the trading symbol. Per Kraken's executions docs, follow-up
    // frames (`new`, `amended`, `restated`, `status`) carry only changed
    // fields and omit `symbol`. We cache the symbol from the first frame for
    // the venue order id and consult it here when the current frame omits it.
    let cached_symbol;
    let symbol = match exec.symbol.as_deref() {
        Some(s) => {
            state.cache_order_symbol(&exec.order_id, s);
            s
        }
        None => match state.lookup_order_symbol(&exec.order_id) {
            Some(s) => {
                cached_symbol = s;
                cached_symbol.as_str()
            }
            None => {
                log::debug!(
                    "Execution message without symbol and no cached mapping: \
                     exec_type={:?}, order_id={}",
                    exec.exec_type,
                    exec.order_id
                );
                return;
            }
        },
    };
    let instruments = instruments.load();
    let Some(instrument) = lookup_instrument_in_snapshot(&instruments, symbol) else {
        log::warn!("No instrument for symbol: {symbol}");
        return;
    };

    // Mirror the existing behavior: cache the order quantity by truncated cli
    // ord id so the parser can fall back to it for quote-quantity orders.
    let cached_qty = exec
        .cl_ord_id
        .as_ref()
        .and_then(|id| order_qty_cache.load().get(id).copied());
    if let (Some(qty), Some(cl_ord_id)) = (exec.order_qty, &exec.cl_ord_id) {
        order_qty_cache.insert(cl_ord_id.clone(), qty);
    }

    // Resolve the `ClientOrderId`. When the frame carries `cl_ord_id` we
    // resolve it through the truncation map and seed the venue-id cache so
    // later delta frames (which routinely omit `cl_ord_id`) can recover it.
    // When `cl_ord_id` is absent we consult the cache; without this lookup a
    // tracked order's delta `new` would fall through to the untracked report
    // path and the strategy would never see `OrderAccepted` (issue #4051).
    let resolved_id = match exec.cl_ord_id.as_ref() {
        Some(id) => {
            let cid = resolve_client_order_id(id, truncated_id_map);
            state.cache_order_client_id(&exec.order_id, cid);
            Some(cid)
        }
        None => state.lookup_order_client_id(&exec.order_id),
    };

    // Stale-report suppression for previously-tracked orders that already
    // reached the filled terminal state.
    if let Some(cid) = resolved_id
        && state.filled_orders.contains(&cid)
    {
        log::debug!(
            "Skipping stale spot execution for filled order: cid={cid}, order_id={}",
            exec.order_id,
        );
        return;
    }

    let identity = resolved_id.and_then(|cid| state.lookup_identity(&cid));

    if identity.is_none()
        && shaped_execution(
            exec,
            resolved_id,
            instrument,
            cached_qty,
            state,
            emitter,
            account_id,
            ts_init,
        )
    {
        return;
    }

    // Status update.
    match parse_ws_order_status_report(exec, instrument, account_id, cached_qty, ts_init) {
        Ok(mut report) => {
            if let Some(cid) = resolved_id {
                report = report.with_client_order_id(cid);
            }

            if let (Some(client_order_id), Some(identity)) = (resolved_id, identity.as_ref()) {
                status_tracked(
                    &report,
                    exec.exec_type,
                    exec.exec_id.is_some(),
                    client_order_id,
                    identity,
                    state,
                    emitter,
                    account_id,
                    ts_init,
                );
            } else {
                emitter.send_order_status_report(report);
            }
        }
        Err(e) => log::error!("Failed to parse order status report: {e}"),
    }

    // Fill (when present).
    if exec.exec_id.is_some() {
        match parse_ws_fill_report(exec, instrument, account_id, ts_init) {
            Ok(mut report) => {
                if let Some(cid) = resolved_id {
                    report.client_order_id = Some(cid);
                }

                if let (Some(client_order_id), Some(identity)) = (resolved_id, identity.as_ref()) {
                    fill_tracked(
                        &report,
                        client_order_id,
                        identity,
                        instrument,
                        state,
                        emitter,
                        account_id,
                        ts_init,
                    );
                } else {
                    if state.check_and_insert_trade(report.trade_id) {
                        log::debug!(
                            "Skipping duplicate external spot fill: trade_id={}",
                            report.trade_id
                        );
                        return;
                    }
                    emitter.send_fill_report(report);
                }
            }
            Err(e) => log::error!("Failed to parse fill report: {e}"),
        }
    }
}

/// Reports a frame of an untracked order completed from the order's shape. Returns false when the
/// order has no shape, leaving the frame to report only what it states.
#[expect(clippy::too_many_arguments)]
fn shaped_execution(
    exec: &KrakenWsExecutionData,
    resolved_id: Option<ClientOrderId>,
    instrument: &InstrumentAny,
    cached_qty: Option<Decimal>,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    ts_init: UnixNanos,
) -> bool {
    let Some(mut shape) = state
        .spot_order_shape(&exec.order_id)
        .or_else(|| seed_order_shape(exec, instrument, account_id, cached_qty, ts_init))
    else {
        return false;
    };

    // A replayed trade is already applied and states a stale cumulative, so it reaches neither
    // the shape nor the engine.
    let fill = match exec
        .exec_id
        .is_some()
        .then(|| parse_ws_fill_report(exec, instrument, account_id, ts_init))
    {
        Some(Ok(mut fill)) => {
            if let Some(cid) = resolved_id {
                fill.client_order_id = Some(cid);
            }

            if state.check_and_insert_trade(fill.trade_id) {
                log::debug!(
                    "Skipping duplicate external spot fill: trade_id={}",
                    fill.trade_id
                );
                return true;
            }
            Some(fill)
        }
        Some(Err(e)) => {
            log::error!("Failed to parse fill report: {e}");
            None
        }
        None => None,
    };

    shape.retained.merge(exec);
    let status = match parse_ws_retained_order_status_report(
        exec,
        Some(&shape.retained),
        instrument,
        account_id,
        cached_qty,
        ts_init,
    ) {
        Ok(mut report) => {
            if let Some(cid) = resolved_id {
                report = report.with_client_order_id(cid);
            }
            report.order_status = untracked_order_status(&report, &shape);
            shape.triggered |= report.order_status == OrderStatus::Triggered;
            Some(report)
        }
        Err(e) => {
            log::error!("Failed to parse order status report: {e}");
            None
        }
    };
    state.store_spot_order_shape(&exec.order_id, shape);

    // The engine can create a shaped order from its `pending_new` report, so a trade's fill goes
    // with the status even after an amendment leaves a status it could not create from.
    match (status, fill) {
        (Some(status), Some(fill)) => emitter.send_order_with_fills(status, vec![fill]),
        (Some(status), None) => emitter.send_order_status_report(status),
        (None, Some(fill)) => emitter.send_fill_report(fill),
        (None, None) => {}
    }
    true
}

/// Returns the shape of an untracked order from its `pending_new` frame, when the engine can build
/// the order from it.
fn seed_order_shape(
    exec: &KrakenWsExecutionData,
    instrument: &InstrumentAny,
    account_id: AccountId,
    cached_qty: Option<Decimal>,
    ts_init: UnixNanos,
) -> Option<SpotOrderShape> {
    if exec.exec_type != KrakenExecType::PendingNew {
        return None;
    }
    let mut shape = SpotOrderShape::default();
    shape.retained.merge(exec);
    let report = parse_ws_retained_order_status_report(
        exec,
        Some(&shape.retained),
        instrument,
        account_id,
        cached_qty,
        ts_init,
    )
    .ok()?;
    can_create_order(&report).then_some(shape)
}

/// Restates a Kraken status the engine cannot apply to an order it holds as external.
fn untracked_order_status(report: &OrderStatusReport, shape: &SpotOrderShape) -> OrderStatus {
    match report.order_status {
        // `pending_new` is an order Kraken has validated, and the engine creates an external
        // order only from an accepted or later status.
        OrderStatus::Submitted => OrderStatus::Accepted,
        // A triggered order's limit reaching the book reports `new`; once filled, the engine holds
        // the order partially filled, which has no triggered transition.
        OrderStatus::Accepted if shape.triggered && report.filled_qty.is_zero() => {
            OrderStatus::Triggered
        }
        // A trade can state `partially_filled` for the fill that completes the order
        OrderStatus::PartiallyFilled if report.filled_qty >= report.quantity => OrderStatus::Filled,
        status => status,
    }
}

/// Returns whether the engine can build an external order from `report`, by the fields each order
/// type's constructor requires.
fn can_create_order(report: &OrderStatusReport) -> bool {
    let Some(side) = report.order_side else {
        return false;
    };

    if !report.quantity.is_positive() {
        return false;
    }

    if report.time_in_force == TimeInForce::Gtd
        && (report.order_type == OrderType::Market || report.expire_time.is_none())
    {
        return false;
    }

    let has_trigger = report.trigger_price.is_some() && report.trigger_type.is_some();
    match report.order_type {
        OrderType::Market => true,
        OrderType::Limit => report.price.is_some(),
        OrderType::StopMarket | OrderType::MarketIfTouched => has_trigger,
        OrderType::StopLimit => report.price.is_some() && has_trigger,
        OrderType::LimitIfTouched => {
            has_trigger
                && match (report.price, report.trigger_price, side) {
                    (Some(price), Some(trigger), OrderSide::Buy) => trigger <= price,
                    (Some(price), Some(trigger), OrderSide::Sell) => trigger >= price,
                    _ => false,
                }
        }
        _ => false,
    }
}

#[expect(clippy::too_many_arguments)]
fn status_tracked(
    report: &OrderStatusReport,
    exec_type: KrakenExecType,
    has_fill: bool,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    let venue_order_id = report.venue_order_id;
    let ts_event = report.ts_last;
    let trader_id = emitter.trader_id();

    // Amended (user modify) and Restated (engine adjustment) both surface
    // post-modify state. Refresh tracked quantity (size may have changed) and
    // emit OrderUpdated so the engine clears PendingUpdate.
    if matches!(
        exec_type,
        KrakenExecType::Amended | KrakenExecType::Restated
    ) && state.emitted_accepted.contains(&client_order_id)
    {
        state.update_identity_quantity(&client_order_id, report.quantity);
        let updated = OrderUpdated::new(
            trader_id,
            identity.strategy_id,
            identity.instrument_id,
            client_order_id,
            report.quantity,
            UUID4::new(),
            ts_event,
            ts_init,
            false,
            Some(venue_order_id),
            Some(account_id),
            report.price,
            report.trigger_price,
            None,
            false,
        );
        emitter.send_order_event(OrderEventAny::Updated(updated));
        return;
    }

    match report.order_status {
        OrderStatus::Accepted => {
            if !state.insert_accepted(client_order_id) {
                // Already accepted; this is a redundant New / Restated / Status
                // exec. The strategy already saw OrderAccepted; nothing to emit.
                return;
            }
            let accepted = OrderAccepted::new(
                trader_id,
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                venue_order_id,
                account_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
            );
            emitter.send_order_event(OrderEventAny::Accepted(accepted));
        }
        OrderStatus::Triggered => {
            // Stop / take-profit transition. Synthesize Accepted first if the
            // venue compressed placement and trigger into one message.
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
            let triggered = OrderTriggered::new(
                trader_id,
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
            emitter.send_order_event(OrderEventAny::Triggered(triggered));
        }
        OrderStatus::PartiallyFilled => {
            // The fill itself is emitted from the trade-side of dispatch via
            // fill_tracked; nothing to do here.
        }

        // Fill dispatch handles cleanup when the report includes a fill
        OrderStatus::Filled if !has_fill => {
            state.insert_filled(client_order_id);
            state.cleanup_terminal(&client_order_id);
        }
        OrderStatus::Canceled => {
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
                trader_id,
                identity.strategy_id,
                identity.instrument_id,
                client_order_id,
                UUID4::new(),
                ts_event,
                ts_init,
                false,
                Some(venue_order_id),
                Some(account_id),
                report.cancel_reason.as_deref().map(Ustr::from),
            );
            emitter.send_order_event(OrderEventAny::Canceled(canceled));
            state.cleanup_terminal(&client_order_id);
        }
        OrderStatus::Expired => {
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
            let expired = OrderExpired::new(
                trader_id,
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
            emitter.send_order_event(OrderEventAny::Expired(expired));
            state.cleanup_terminal(&client_order_id);
        }
        _ => {}
    }
}

#[expect(clippy::too_many_arguments)]
fn fill_tracked(
    report: &FillReport,
    client_order_id: ClientOrderId,
    identity: &OrderIdentity,
    instrument: &InstrumentAny,
    state: &WsDispatchState,
    emitter: &ExecutionEventEmitter,
    account_id: AccountId,
    ts_init: UnixNanos,
) {
    if state.check_and_insert_trade(report.trade_id) {
        log::debug!(
            "Skipping duplicate spot fill for {client_order_id}: trade_id={}",
            report.trade_id
        );
        return;
    }

    ensure_accepted_emitted(
        client_order_id,
        report.venue_order_id,
        account_id,
        identity,
        state,
        emitter,
        report.ts_event,
        ts_init,
    );

    let filled = fill_report_to_order_filled(
        report,
        emitter.trader_id(),
        identity,
        instrument.quote_currency(),
        client_order_id,
    );
    emitter.send_order_event(OrderEventAny::Filled(filled));

    let previous = state
        .previous_filled_qty(&client_order_id)
        .unwrap_or_else(|| Quantity::zero(instrument.size_precision()));
    let cumulative = previous + report.last_qty;
    state.record_filled_qty(client_order_id, cumulative);

    if cumulative >= identity.quantity {
        state.insert_filled(client_order_id);
        state.cleanup_terminal(&client_order_id);
    }
}

/// Returns true when this spot execution carries a terminal status that
/// should remove the order from dispatch state.
#[must_use]
pub fn is_terminal_exec_type(exec_type: KrakenExecType) -> bool {
    matches!(
        exec_type,
        KrakenExecType::Filled | KrakenExecType::Canceled | KrakenExecType::Expired
    )
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::TriggerType,
        identifiers::{InstrumentId, VenueOrderId},
        types::Price,
    };
    use rstest::rstest;

    use super::*;

    /// A buy of 1 in `order_type`, GTC and accepted, with the given limit and `LastPrice` trigger.
    fn report(
        order_type: OrderType,
        price: Option<&str>,
        trigger: Option<&str>,
    ) -> OrderStatusReport {
        let mut report = OrderStatusReport::new(
            AccountId::from("KRAKEN-001"),
            InstrumentId::from("BTC/USDT.KRAKEN"),
            None,
            VenueOrderId::from("V-1"),
            Some(OrderSide::Buy),
            order_type,
            TimeInForce::Gtc,
            OrderStatus::Accepted,
            Quantity::from("1"),
            Quantity::from("0"),
            UnixNanos::default(),
            UnixNanos::default(),
            UnixNanos::default(),
            None,
        );
        report.price = price.map(Price::from);
        report.trigger_price = trigger.map(Price::from);
        report.trigger_type = trigger.map(|_| TriggerType::LastPrice);
        report
    }

    /// The engine builds an external order only from a report carrying what its type's constructor
    /// requires, so a report lacking it never carries the order's fills.
    #[rstest]
    #[case::market(report(OrderType::Market, None, None), true)]
    #[case::limit(report(OrderType::Limit, Some("100"), None), true)]
    #[case::limit_without_price(report(OrderType::Limit, None, None), false)]
    #[case::stop_market(report(OrderType::StopMarket, None, Some("90")), true)]
    #[case::stop_market_without_trigger(report(OrderType::StopMarket, None, None), false)]
    #[case::market_if_touched(report(OrderType::MarketIfTouched, None, Some("110")), true)]
    #[case::market_if_touched_without_trigger(
        report(OrderType::MarketIfTouched, None, None),
        false
    )]
    #[case::stop_limit(report(OrderType::StopLimit, Some("100"), Some("90")), true)]
    #[case::stop_limit_without_price(report(OrderType::StopLimit, None, Some("90")), false)]
    #[case::stop_limit_without_trigger(report(OrderType::StopLimit, Some("100"), None), false)]
    #[case::trigger_without_type(
        OrderStatusReport { trigger_type: None, ..report(OrderType::StopMarket, None, Some("90")) },
        false
    )]
    #[case::buy_limit_if_touched(report(OrderType::LimitIfTouched, Some("100"), Some("90")), true)]
    #[case::buy_limit_if_touched_trigger_above_limit(
        report(OrderType::LimitIfTouched, Some("100"), Some("110")),
        false
    )]
    #[case::sell_limit_if_touched(
        OrderStatusReport {
            order_side: Some(OrderSide::Sell),
            ..report(OrderType::LimitIfTouched, Some("100"), Some("110"))
        },
        true
    )]
    #[case::sell_limit_if_touched_trigger_below_limit(
        OrderStatusReport {
            order_side: Some(OrderSide::Sell),
            ..report(OrderType::LimitIfTouched, Some("100"), Some("90"))
        },
        false
    )]
    #[case::limit_if_touched_without_trigger(
        report(OrderType::LimitIfTouched, Some("100"), None),
        false
    )]
    #[case::trailing_stop_market(report(OrderType::TrailingStopMarket, None, Some("90")), false)]
    #[case::trailing_stop_limit(
        report(OrderType::TrailingStopLimit, Some("100"), Some("90")),
        false
    )]
    #[case::no_side(
        OrderStatusReport { order_side: None, ..report(OrderType::Market, None, None) },
        false
    )]
    #[case::zero_quantity(
        OrderStatusReport { quantity: Quantity::from("0"), ..report(OrderType::Market, None, None) },
        false
    )]
    #[case::gtd_market(
        OrderStatusReport {
            time_in_force: TimeInForce::Gtd,
            expire_time: Some(UnixNanos::from(1)),
            ..report(OrderType::Market, None, None)
        },
        false
    )]
    #[case::gtd_limit_without_expiry(
        OrderStatusReport { time_in_force: TimeInForce::Gtd, ..report(OrderType::Limit, Some("100"), None) },
        false
    )]
    #[case::gtd_limit(
        OrderStatusReport {
            time_in_force: TimeInForce::Gtd,
            expire_time: Some(UnixNanos::from(1)),
            ..report(OrderType::Limit, Some("100"), None)
        },
        true
    )]
    fn test_can_create_order(#[case] report: OrderStatusReport, #[case] expected: bool) {
        assert_eq!(can_create_order(&report), expected);
    }

    #[rstest]
    #[case::filled(KrakenExecType::Filled, true)]
    #[case::canceled(KrakenExecType::Canceled, true)]
    #[case::expired(KrakenExecType::Expired, true)]
    #[case::new(KrakenExecType::New, false)]
    #[case::trade(KrakenExecType::Trade, false)]
    #[case::pending_new(KrakenExecType::PendingNew, false)]
    fn test_is_terminal_exec_type(#[case] exec_type: KrakenExecType, #[case] expected: bool) {
        assert_eq!(is_terminal_exec_type(exec_type), expected);
    }
}
