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

//! WebSocket message dispatch for the Polymarket execution client.
//!
//! Routes user-channel WS messages (order updates and trades) for orders submitted through this
//! client into Nautilus order events (`OrderAccepted` / `OrderFilled` / `OrderCanceled` /
//! `OrderRejected` / `OrderExpired`), building them from the identity captured at submit
//! (`OrderIdentityRegistry`). Order-channel messages drive lifecycle events; trade-channel
//! messages drive fills, and acceptance is synthesized before a fill or cancel that races ahead.
//! Messages are emitted once the order is known (accepted, or with a submit in flight), otherwise
//! buffered until acceptance. Reports are reserved for the `generate_*` query and reconciliation
//! methods. Trade fills are deduped via a FIFO cache; maker and taker fills are handled separately
//! to account for multi-leg maker order matching.

use std::str::FromStr;

use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{UUID4, UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFilled, OrderRejected,
        OrderUpdated,
    },
    identifiers::{AccountId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Money, Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    messages::{PolymarketUserOrder, PolymarketUserTrade, UserWsMessage},
    parse::parse_timestamp_ms,
};
use crate::{
    common::enums::{PolymarketLiquiditySide, PolymarketOrderStatus},
    execution::{
        get_pusd_currency,
        identity::{OrderIdentity, OrderIdentityRegistry},
        order_fill_tracker::OrderFillTrackerMap,
        parse::{
            build_maker_fill_report, compute_commission, determine_order_side,
            instrument_taker_fee, make_composite_trade_id, parse_liquidity_side,
        },
        pending::PendingSubmitTracker,
    },
};

/// Signal returned when a finalized trade requires an async account refresh.
#[derive(Debug)]
pub(crate) struct AccountRefreshRequest;

/// Mutable state owned by the WS message loop (not shared via Arc).
#[derive(Debug, Default)]
pub(crate) struct WsDispatchState {
    pub processed_fills: FifoCache<String, 10_000>,
    /// Cancel reports saved for orders known to be terminal at the venue.
    /// Re-emitted after a fill to restore terminal state when fills race
    /// ahead of (or arrive after) cancel messages.
    terminal_cancel_reports: FifoCacheMap<VenueOrderId, OrderStatusReport, 10_000>,
}

/// Immutable context borrowed from the async block's owned values.
#[derive(Debug)]
pub(crate) struct WsDispatchContext<'a> {
    pub token_instruments: &'a AtomicMap<Ustr, InstrumentAny>,
    pub fill_tracker: &'a OrderFillTrackerMap,
    pub pending_submits: &'a PendingSubmitTracker,
    pub order_identities: &'a OrderIdentityRegistry,
    pub emitter: &'a ExecutionEventEmitter,
    pub account_id: AccountId,
    pub clock: &'static AtomicTime,
    pub user_address: &'a str,
    pub user_api_key: &'a str,
}

/// Top-level router: synchronous, returns signal for async account refresh.
pub(crate) fn dispatch_user_message(
    message: &UserWsMessage,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> Option<AccountRefreshRequest> {
    match message {
        UserWsMessage::Order(order) => {
            dispatch_order_update(order, ctx, state);
            None
        }
        UserWsMessage::Trade(trade) => dispatch_trade_update(trade, ctx, state),
    }
}

fn dispatch_order_update(
    order: &PolymarketUserOrder,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let instruments = ctx.token_instruments.load();
    let instrument = match instruments.get(&order.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in order update: {}", order.asset_id);
            return;
        }
    };

    let ts_event = parse_timestamp_ms(&order.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let venue_order_id = VenueOrderId::from(order.id.as_str());

    let ts_init = ctx.clock.get_time_ns();
    let mut report =
        build_ws_order_status_report(order, instrument, ctx.account_id, ts_event, ts_init);
    let local_client_order_id = ctx.pending_submits.client_order_id(&venue_order_id);
    let mut is_accepted = ctx.fill_tracker.contains(&venue_order_id);
    report.client_order_id = local_client_order_id;

    // A known own order (submit in flight) self-registers on its first WS update
    let buffered_fills = if local_client_order_id.is_some()
        && !is_accepted
        && report.order_status != OrderStatus::Rejected
    {
        is_accepted = true;
        ctx.fill_tracker.register_and_take_pending_fills(
            venue_order_id,
            local_client_order_id,
            report.quantity,
            report.order_side,
            report.instrument_id,
            instrument.size_precision(),
            instrument.price_precision(),
        )
    } else if is_accepted {
        ctx.fill_tracker
            .take_pending_fills(venue_order_id, local_client_order_id)
    } else {
        Vec::new()
    };

    // Order updates can race ahead of trade messages, so cap filled_qty
    // to what the fill tracker has recorded to prevent duplicate inferred fills
    if let Some(tracked_filled) = ctx.fill_tracker.get_cumulative_filled(&venue_order_id) {
        let tracked_qty = Quantity::new(tracked_filled, instrument.size_precision());
        if report.filled_qty > tracked_qty {
            log::debug!(
                "Capping filled_qty for {venue_order_id} from {} to {} (awaiting trade messages)",
                report.filled_qty,
                tracked_qty,
            );
            report.filled_qty = tracked_qty;
        }
    }

    // Track cancel reports so we can re-emit them after late-arriving fills.
    // Saved regardless of acceptance state so that cancels arriving during
    // the HTTP round-trip are available once the order is later accepted.
    if report.order_status == OrderStatus::Canceled {
        state
            .terminal_cancel_reports
            .insert(venue_order_id, report.clone());
    }

    // Tracked own orders route through order events; externally-managed orders
    // (no captured identity) buffer until accepted or fall back to reports.
    let identity = ctx.order_identities.get(&venue_order_id);
    if is_accepted || local_client_order_id.is_some() {
        match identity {
            Some(identity) => emit_tracked_order_status(&report, &identity, ts_event, ctx),
            None => ctx.emitter.send_order_status_report(report),
        }
    } else if let Some(report) = ctx
        .fill_tracker
        .accept_or_buffer_report(venue_order_id, report)
    {
        // Registered between the early accepted-check and here: emit rather than buffer
        match ctx.order_identities.get(&venue_order_id) {
            Some(identity) => emit_tracked_order_status(&report, &identity, ts_event, ctx),
            None => ctx.emitter.send_order_status_report(report),
        }
    }

    for fill in buffered_fills {
        match identity {
            Some(identity) => emit_order_filled(&identity, &fill, ctx),
            None => ctx.emitter.send_fill_report(fill),
        }
    }

    // MATCHED convergence: check for dust residual. A dust fill is only built for a
    // registered order, so it always emits (accepted by construction, no gate).
    if order.status == PolymarketOrderStatus::Matched {
        let price_precision = instrument.price_precision();
        let price = Decimal::from_str(&order.price)
            .ok()
            .and_then(|d| Price::from_decimal_dp(d, price_precision).ok())
            .unwrap_or_else(|| Price::zero(price_precision));

        if let Some(dust_fill) = ctx.fill_tracker.check_dust_and_build_fill(
            &venue_order_id,
            ctx.account_id,
            &order.id,
            price.as_f64(),
            get_pusd_currency(),
            ts_event,
            ts_init,
        ) {
            match ctx.order_identities.get(&venue_order_id) {
                Some(identity) => emit_order_filled(&identity, &dust_fill, ctx),
                None => ctx.emitter.send_fill_report(dust_fill),
            }
        }
    }
}

fn dispatch_trade_update(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> Option<AccountRefreshRequest> {
    if !trade.status.is_finalized()
        && !matches!(
            trade.status,
            crate::common::enums::PolymarketTradeStatus::Matched
        )
    {
        log::debug!(
            "Skipping trade with status {:?}: {}",
            trade.status,
            trade.id
        );
        return None;
    }

    let dedup_key = format!("{}-{}", trade.id, trade.taker_order_id);
    let is_duplicate = state.processed_fills.contains(&dedup_key);

    let needs_refresh = trade.status.is_finalized();

    if is_duplicate {
        log::debug!("Duplicate fill skipped: {dedup_key}");
        return if needs_refresh {
            Some(AccountRefreshRequest)
        } else {
            None
        };
    }
    state.processed_fills.add(dedup_key);

    let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;
    let liquidity_side = parse_liquidity_side(trade.trader_side);
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let ts_init = ctx.clock.get_time_ns();

    if is_maker {
        dispatch_maker_fills(trade, ctx, state, liquidity_side, ts_event, ts_init);
    } else {
        dispatch_taker_fill(trade, ctx, state, liquidity_side, ts_event, ts_init);
    }

    if needs_refresh {
        Some(AccountRefreshRequest)
    } else {
        None
    }
}

fn dispatch_maker_fills(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    state: &WsDispatchState,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let user_orders: Vec<_> = trade
        .maker_orders
        .iter()
        .filter(|mo| mo.maker_address == ctx.user_address || mo.owner == ctx.user_api_key)
        .collect();

    if user_orders.is_empty() {
        log::warn!("No matching maker orders for user in trade: {}", trade.id);
        return;
    }

    let instruments = ctx.token_instruments.load();

    for mo in user_orders {
        let asset_id = Ustr::from(mo.asset_id.as_str());
        let instrument = match instruments.get(&asset_id) {
            Some(i) => i,
            None => {
                log::warn!("Unknown asset_id in maker order: {asset_id}");
                continue;
            }
        };
        let mut report = build_maker_fill_report(
            mo,
            &trade.id,
            trade.trader_side,
            trade.side,
            trade.asset_id.as_str(),
            ctx.account_id,
            instrument.id(),
            instrument.price_precision(),
            instrument.size_precision(),
            crate::execution::get_pusd_currency(),
            liquidity_side,
            ts_event,
            ts_init,
        );
        let maker_venue_order_id = report.venue_order_id;
        report.client_order_id = ctx.pending_submits.client_order_id(&maker_venue_order_id);
        report.last_qty = ctx
            .fill_tracker
            .snap_fill_qty(&maker_venue_order_id, report.last_qty);

        if let Some(report) = ctx
            .fill_tracker
            .accept_or_buffer_fill(maker_venue_order_id, report)
        {
            match ctx.order_identities.get(&maker_venue_order_id) {
                Some(identity) => emit_order_filled(&identity, &report, ctx),
                None => ctx.emitter.send_fill_report(report),
            }
            reemit_terminal_cancel(maker_venue_order_id, state, ctx);
        }
    }
}

fn dispatch_taker_fill(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    state: &WsDispatchState,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let instruments = ctx.token_instruments.load();
    let instrument = match instruments.get(&trade.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in trade: {}", trade.asset_id);
            return;
        }
    };

    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

    let mut report = build_ws_taker_fill_report(
        trade,
        instrument,
        ctx.account_id,
        liquidity_side,
        ts_event,
        ts_init,
    );
    report.client_order_id = ctx.pending_submits.client_order_id(&venue_order_id);
    report.last_qty = ctx
        .fill_tracker
        .snap_fill_qty(&venue_order_id, report.last_qty);

    if let Some(report) = ctx
        .fill_tracker
        .accept_or_buffer_fill(venue_order_id, report)
    {
        match ctx.order_identities.get(&venue_order_id) {
            Some(identity) => emit_order_filled(&identity, &report, ctx),
            None => ctx.emitter.send_fill_report(report),
        }
        reemit_terminal_cancel(venue_order_id, state, ctx);
    }
}

/// Re-emits a saved cancel report after a fill to restore terminal state.
///
/// When fills race ahead of (or arrive after) cancel messages, the order can
/// get stuck in `PartiallyFilled`. This re-emission ensures the execution
/// engine transitions the order back to `Canceled`.
///
/// Skips re-emission when the fill tracker shows the order is fully filled,
/// because `Filled` is already terminal and a spurious cancel would fail
/// the `Filled -> Canceled` state transition.
fn reemit_terminal_cancel(
    venue_order_id: VenueOrderId,
    state: &WsDispatchState,
    ctx: &WsDispatchContext<'_>,
) {
    if ctx.fill_tracker.is_fully_filled(&venue_order_id) {
        return;
    }

    if let Some(cancel_report) = state.terminal_cancel_reports.get(&venue_order_id) {
        log::debug!("Re-emitting cancel for {venue_order_id} after fill to restore terminal state");
        match ctx.order_identities.get(&venue_order_id) {
            Some(identity) => {
                emit_order_canceled(&identity, venue_order_id, cancel_report.ts_last, ctx);
            }
            None => ctx.emitter.send_order_status_report(cancel_report.clone()),
        }
    }
}

fn build_ws_order_status_report(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_status =
        crate::execution::parse::resolve_order_status(order.status, order.event_type);
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order.order_type);
    let size_precision = instrument.size_precision();
    let price_precision = instrument.price_precision();
    let quantity = Decimal::from_str(&order.original_size)
        .ok()
        .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok())
        .unwrap_or_else(|| Quantity::zero(size_precision));
    let filled_qty = Decimal::from_str(&order.size_matched)
        .ok()
        .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok())
        .unwrap_or_else(|| Quantity::zero(size_precision));
    let price = Decimal::from_str(&order.price)
        .ok()
        .and_then(|d| Price::from_decimal_dp(d, price_precision).ok())
        .unwrap_or_else(|| Price::zero(price_precision));

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        None,
        venue_order_id,
        order_side,
        OrderType::Limit,
        time_in_force,
        order_status,
        quantity,
        filled_qty,
        ts_event,
        ts_event,
        ts_init,
        None,
    );
    report.price = Some(price);
    report
}

fn build_ws_taker_fill_report(
    trade: &PolymarketUserTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> FillReport {
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
    let trade_id = make_composite_trade_id(&trade.id, &trade.taker_order_id);
    let order_side = determine_order_side(
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        trade.asset_id.as_str(),
    );

    let size_precision = instrument.size_precision();
    let price_precision = instrument.price_precision();
    let size_dec = Decimal::from_str(&trade.size).unwrap_or_default();
    let price_dec = Decimal::from_str(&trade.price).unwrap_or_default();
    let last_qty = Quantity::from_decimal_dp(size_dec, size_precision)
        .unwrap_or_else(|_| Quantity::zero(size_precision));
    let last_px = Price::from_decimal_dp(price_dec, price_precision)
        .unwrap_or_else(|_| Price::zero(price_precision));

    let fee_rate = instrument_taker_fee(instrument);
    let commission_value = compute_commission(fee_rate, size_dec, price_dec, liquidity_side);
    let pusd = crate::execution::get_pusd_currency();

    FillReport {
        account_id,
        instrument_id: instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission: Money::new(commission_value, pusd),
        liquidity_side,
        avg_px: None,
        report_id: UUID4::new(),
        ts_event,
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    }
}

/// Emits order events for a tracked own-order status update.
///
/// Order-channel messages drive lifecycle events only; fills arrive separately on the trade
/// channel as `OrderFilled`. `PartiallyFilled` / `Filled` statuses therefore emit no fill here,
/// they only ensure acceptance has been emitted so the order lifecycle stays well-formed.
fn emit_tracked_order_status(
    report: &OrderStatusReport,
    identity: &OrderIdentity,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let venue_order_id = report.venue_order_id;
    match report.order_status {
        OrderStatus::Accepted => ensure_accepted(identity, venue_order_id, ts_event, ctx),
        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            ensure_accepted(identity, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Canceled => {
            ensure_accepted(identity, venue_order_id, ts_event, ctx);
            emit_order_canceled(identity, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Expired => {
            ensure_accepted(identity, venue_order_id, ts_event, ctx);
            emit_order_expired(identity, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Rejected => {
            let reason = report
                .cancel_reason
                .clone()
                .unwrap_or_else(|| "REJECTED".to_string());
            emit_order_rejected(identity, &reason, ts_event, ctx);
        }
        other => log::debug!("No order event for status {other:?} on {venue_order_id}"),
    }
}

/// Emits `OrderAccepted` for a tracked order if acceptance has not yet been emitted.
///
/// Acceptance is also emitted on the submit happy path; the registry's dedup set ensures it
/// fires exactly once across the submit confirmation and the WS stream, including when a fill or
/// cancel races ahead of the acceptance message.
fn ensure_accepted(
    identity: &OrderIdentity,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    if !ctx.order_identities.mark_accepted(venue_order_id) {
        return;
    }
    let accepted = OrderAccepted::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        venue_order_id,
        ctx.account_id,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Accepted(accepted));
}

/// Builds and emits an `OrderFilled` event for a tracked order, synthesizing acceptance first.
fn emit_order_filled(identity: &OrderIdentity, fill: &FillReport, ctx: &WsDispatchContext<'_>) {
    ensure_accepted(identity, fill.venue_order_id, fill.ts_event, ctx);

    if let Some(new_qty) = ctx.fill_tracker.buy_overfill_bump(&fill.venue_order_id) {
        emit_buy_overfill_update(identity, fill.venue_order_id, new_qty, fill.ts_event, ctx);
    }

    let filled = OrderFilled::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        fill.venue_order_id,
        ctx.account_id,
        fill.trade_id,
        identity.order_side,
        identity.order_type,
        fill.last_qty,
        fill.last_px,
        get_pusd_currency(),
        fill.liquidity_side,
        UUID4::new(),
        fill.ts_event,
        fill.ts_init,
        false,
        fill.venue_position_id,
        Some(fill.commission),
    );
    ctx.emitter.send_order_event(OrderEventAny::Filled(filled));
}

/// Emits an `OrderUpdated` raising the order quantity to the actual BUY fill, before the fill.
///
/// A Polymarket BUY is bounded by the USDC it spends, so a marketable fill below the limit price
/// returns more shares than the nominal quantity. The engine rejects a fill past the order
/// quantity, so the quantity is raised first. The price is left unchanged (`None`).
fn emit_buy_overfill_update(
    identity: &OrderIdentity,
    venue_order_id: VenueOrderId,
    new_qty: Quantity,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let updated = OrderUpdated::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        new_qty,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(ctx.account_id),
        None,
        None,
        None,
        false,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Updated(updated));
}

fn emit_order_canceled(
    identity: &OrderIdentity,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let canceled = OrderCanceled::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(ctx.account_id),
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Canceled(canceled));
}

fn emit_order_expired(
    identity: &OrderIdentity,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let expired = OrderExpired::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(ctx.account_id),
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Expired(expired));
}

fn emit_order_rejected(
    identity: &OrderIdentity,
    reason: &str,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let rejected = OrderRejected::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        ctx.account_id,
        Ustr::from(reason),
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        false,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Rejected(rejected));
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::AtomicTime;
    use nautilus_model::{
        enums::{AccountType, OrderStatus},
        events::OrderEventAny,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        types::Currency,
    };
    use rstest::rstest;

    use super::*;
    use crate::http::{
        models::GammaMarket,
        parse::{create_instrument_from_def, parse_gamma_market},
    };

    /// Registers a tracked-order identity so the dispatch routes the order through events.
    fn register_identity(
        order_identities: &OrderIdentityRegistry,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        client_order_id: &str,
    ) {
        order_identities.register_order_identity(
            venue_order_id,
            OrderIdentity {
                client_order_id: ClientOrderId::from(client_order_id),
                strategy_id: StrategyId::from("S-001"),
                instrument_id,
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
        );
    }

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn test_instrument() -> InstrumentAny {
        let market: GammaMarket = load("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap()
    }

    fn test_emitter() -> ExecutionEventEmitter {
        ExecutionEventEmitter::new(
            nautilus_core::time::get_atomic_clock_realtime(),
            TraderId::from("TESTER-001"),
            AccountId::from("POLY-001"),
            AccountType::Cash,
            Some(Currency::pUSD()),
        )
    }

    #[rstest]
    fn test_build_ws_order_status_report() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);
        let ts_init = UnixNanos::from(2_000_000_000u64);

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            ts_event,
            ts_init,
        );

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert!(report.price.is_some());
        assert_eq!(report.ts_accepted, ts_event);
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_build_ws_order_status_report_venue_cancel_maps_to_canceled() {
        let order: PolymarketUserOrder = load("ws_user_order_venue_cancel.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);
        let ts_init = UnixNanos::from(2_000_000_000u64);

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            ts_event,
            ts_init,
        );

        assert_eq!(report.order_status, OrderStatus::Canceled);
    }

    #[rstest]
    fn test_build_ws_taker_fill_report() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);
        let ts_init = UnixNanos::from(2_000_000_000u64);

        let report = build_ws_taker_fill_report(
            &trade,
            &instrument,
            AccountId::from("POLY-001"),
            LiquiditySide::Taker,
            ts_event,
            ts_init,
        );

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.ts_event, ts_event);
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_dispatch_order_message_buffers_when_not_accepted() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let emitter = test_emitter();

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
        let mut state = WsDispatchState::default();

        let result = dispatch_user_message(&UserWsMessage::Order(order.clone()), &ctx, &mut state);
        assert!(result.is_none());

        // Order not registered in fill_tracker, so should be buffered
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        assert!(fill_tracker.has_pending_report(&venue_order_id));
    }

    #[rstest]
    fn test_dispatch_order_message_uses_pending_submit_client_order_id() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-UNKNOWN-SUBMIT");
        pending_submits.insert(venue_order_id, client_order_id);
        register_identity(
            &order_identities,
            venue_order_id,
            test_instrument().id(),
            "O-UNKNOWN-SUBMIT",
        );

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
        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        // The tracked own order emits an OrderAccepted event carrying the client order ID.
        let event = receiver.try_recv().expect("expected accepted event");
        match event {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
                assert_eq!(accepted.client_order_id, client_order_id);
            }
            other => panic!("Expected accepted event, was {other:?}"),
        }

        assert!(!fill_tracker.has_pending_report(&venue_order_id));
    }

    #[rstest]
    fn test_dispatch_trade_dedup() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let emitter = test_emitter();

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
        let mut state = WsDispatchState::default();

        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

        // First dispatch processes the trade
        let _ = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 1);

        // Second dispatch should be deduped, no additional fill
        let _ = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 1);
    }

    #[rstest]
    fn test_dispatch_trade_uses_pending_submit_client_order_id() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let emitter = test_emitter();

        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let client_order_id = ClientOrderId::from("O-UNKNOWN-FILL");
        pending_submits.insert(venue_order_id, client_order_id);

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
        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fills = fill_tracker.pending_fills_for(&venue_order_id);
        assert_eq!(fills[0].client_order_id, Some(client_order_id));
    }

    #[rstest]
    fn test_dispatch_order_matched_caps_filled_qty_when_no_trades_tracked() {
        let order: PolymarketUserOrder = load("ws_user_order_matched.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());

        // Register order so it is "accepted" but with no fills tracked
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        // No identity registered, so the order surfaces as a report (the external/reconciliation
        // fallback), where filled_qty is capped to tracked fills.
        let order_identities = OrderIdentityRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        let event = receiver.try_recv().expect("Expected report");
        match event {
            ExecutionEvent::Report(report) => match report {
                ExecutionReport::Order(order_report) => {
                    assert_eq!(order_report.filled_qty, Quantity::from("0"));
                }
                other => panic!("Expected order report, was {other:?}"),
            },
            other => panic!("Expected report event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_matched_uses_tracked_fills_for_filled_qty() {
        let order: PolymarketUserOrder = load("ws_user_order_matched.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());

        // Register and record a partial fill (50 of 100)
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        fill_tracker.record_fill(&venue_order_id, 50.0, 0.5, UnixNanos::from(1_000u64));

        let pending_submits = PendingSubmitTracker::default();
        // No identity registered, so the order surfaces as a report (the external/reconciliation
        // fallback), where filled_qty is capped to tracked fills.
        let order_identities = OrderIdentityRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        let event = receiver.try_recv().expect("Expected report");
        match event {
            ExecutionEvent::Report(report) => match report {
                ExecutionReport::Order(order_report) => {
                    assert_eq!(order_report.filled_qty, Quantity::from("50"));
                }
                other => panic!("Expected order report, was {other:?}"),
            },
            other => panic!("Expected report event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_order_matched_dust_fill_uses_local_ts_init() {
        let order: PolymarketUserOrder = load("ws_user_order_matched.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        fill_tracker.record_fill(&venue_order_id, 99.995, 0.5, UnixNanos::from(1_000u64));

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-MATCHED",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let clock = Box::leak(Box::new(AtomicTime::new(
            false,
            UnixNanos::from(2_000_000_000u64),
        )));

        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock,
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        // Acceptance was already emitted at submit, so the MATCHED message emits no order
        // event; the dust residual converts to an OrderFilled event carrying the local ts_init.
        let event = receiver.try_recv().expect("Expected dust filled event");
        match event {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(
                    filled.ts_event,
                    UnixNanos::from(1_703_875_201_000_000_000u64)
                );
                assert_eq!(filled.ts_init, UnixNanos::from(2_000_000_000u64));
            }
            other => panic!("Expected filled event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_cancel_reemitted_after_fill_for_canceled_order() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(cancel_order.id.as_str());

        // Register order as accepted with original qty=100
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-CANCEL",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        // Step 1: Dispatch cancel (simulates message A from the bug)
        dispatch_user_message(&UserWsMessage::Order(cancel_order), &ctx, &mut state);
        let cancel_event = receiver.try_recv().expect("Expected canceled event");
        match &cancel_event {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("Expected canceled event, was {other:?}"),
        }

        // Step 2: Dispatch trade fill (simulates trade arriving after cancel)
        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        // Should get: filled event, then re-emitted canceled event
        let fill_event = receiver.try_recv().expect("Expected filled event");
        match &fill_event {
            ExecutionEvent::Order(OrderEventAny::Filled(f)) => {
                assert_eq!(f.venue_order_id, venue_order_id);
            }
            other => panic!("Expected filled event, was {other:?}"),
        }

        let reemitted_cancel = receiver
            .try_recv()
            .expect("Expected re-emitted canceled event");

        match &reemitted_cancel {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("Expected canceled event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_cancel_not_reemitted_when_fill_completes_order() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(cancel_order.id.as_str());

        // Register with qty=25 matching the trade size so the fill completes the order
        fill_tracker.register(
            venue_order_id,
            Quantity::from("25"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-CANCEL-FULL",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        // Cancel then fill that completes the order
        dispatch_user_message(&UserWsMessage::Order(cancel_order), &ctx, &mut state);
        let _cancel = receiver.try_recv().expect("Expected canceled event");

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        let _fill = receiver.try_recv().expect("Expected filled event");

        // Channel should be empty: no re-emitted cancel for a fully-filled order
        assert!(
            receiver.try_recv().is_err(),
            "Should not re-emit cancel when fill completes the order"
        );
    }

    #[rstest]
    fn test_cancel_saved_before_acceptance() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument);

        // Fill tracker has NO registration (simulates HTTP still in-flight)
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(cancel_order.id.as_str());

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let emitter = test_emitter();

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
        let mut state = WsDispatchState::default();

        // Dispatch cancel while order is not yet accepted
        dispatch_user_message(&UserWsMessage::Order(cancel_order), &ctx, &mut state);

        // Cancel should be buffered (not emitted) AND saved to terminal_cancel_reports
        assert!(fill_tracker.has_pending_report(&venue_order_id));
        assert!(state.terminal_cancel_reports.get(&venue_order_id).is_some());
    }

    /// Replays the exact 5-message WS sequence from issue #3797.
    ///
    /// Messages in arrival order:
    ///   (A) Order Canceled, size_matched=0
    ///   (B) Trade fill 1.219511 (maker side)
    ///   (C) Order Canceled, size_matched=1.219511
    ///   (D) Order Canceled, size_matched=2.560972 (capped to tracked)
    ///   (E) Trade fill 1.341461 (maker side)
    ///
    /// Without the fix, the order ends in PartiallyFilled after (E).
    /// With the fix, a re-emitted cancel after (E) restores Canceled.
    #[rstest]
    fn test_issue_3797_interleaved_cancel_fill_sequence() {
        use crate::common::{
            enums::{
                PolymarketEventType, PolymarketLiquiditySide, PolymarketOrderSide,
                PolymarketOrderStatus, PolymarketOrderType, PolymarketOutcome,
                PolymarketTradeStatus,
            },
            models::PolymarketMakerOrder,
        };

        let instrument = test_instrument();
        let asset_id = instrument.id().symbol.inner();

        let order_id =
            "0xe743f6c823ecdfa9ddaaf08673b2441d15a38d89e14dcb25b3b70c284be4f6ad".to_string();
        let venue_order_id = VenueOrderId::from(order_id.as_str());

        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("20"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(&order_identities, venue_order_id, instrument.id(), "O-3797");
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xabc",
            user_api_key: "xxx",
        };
        let mut state = WsDispatchState::default();

        // Helper to build order updates
        let make_order =
            |size_matched: &str, ts: &str, event_type: PolymarketEventType| PolymarketUserOrder {
                asset_id,
                associate_trades: None,
                created_at: "1775074735".to_string(),
                expiration: Some("0".to_string()),
                id: order_id.clone(),
                maker_address: Ustr::from("0xabc"),
                market: Ustr::from("0x4134"),
                order_owner: Ustr::from("xxx"),
                order_type: PolymarketOrderType::GTC,
                original_size: "20".to_string(),
                outcome: PolymarketOutcome::yes(),
                owner: Ustr::from("xxx"),
                price: "0.18".to_string(),
                side: PolymarketOrderSide::Buy,
                size_matched: size_matched.to_string(),
                status: PolymarketOrderStatus::Canceled,
                timestamp: ts.to_string(),
                event_type,
            };

        // Helper to build maker trades
        let make_trade = |trade_id: &str, matched_amount: f64, ts: &str| PolymarketUserTrade {
            asset_id,
            bucket_index: 0,
            fee_rate_bps: "1000".to_string(),
            id: trade_id.to_string(),
            last_update: "1775074738".to_string(),
            maker_address: Ustr::from("0xother"),
            maker_orders: vec![PolymarketMakerOrder {
                asset_id,
                maker_address: "0xabc".to_string(),
                matched_amount: Decimal::from_f64_retain(matched_amount).unwrap_or(Decimal::ZERO),
                order_id: order_id.clone(),
                outcome: PolymarketOutcome::yes(),
                owner: "xxx".to_string(),
                price: Decimal::from_f64_retain(0.18).unwrap_or(Decimal::ZERO),
                side: None,
            }],
            market: Ustr::from("0x4134"),
            match_time: "1775074735".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("other-owner"),
            price: "0.82".to_string(),
            side: PolymarketOrderSide::Buy,
            size: "1.219511".to_string(),
            status: PolymarketTradeStatus::Matched,
            taker_order_id: "0xtaker01".to_string(),
            timestamp: ts.to_string(),
            trade_owner: Ustr::from("other-owner"),
            trader_side: PolymarketLiquiditySide::Maker,
            event_type: PolymarketEventType::Trade,
        };

        // (A) Cancel with size_matched=0
        let msg_a = make_order("0", "1775074738031", PolymarketEventType::Cancellation);
        dispatch_user_message(&UserWsMessage::Order(msg_a), &ctx, &mut state);

        let evt = receiver.try_recv().expect("(A) canceled event");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("(A) expected canceled event, was {other:?}"),
        }

        // (B) Trade fill 1.219511
        let msg_b = make_trade("trade-b", 1.219511, "1775074738032");
        dispatch_user_message(&UserWsMessage::Trade(msg_b), &ctx, &mut state);

        let evt = receiver.try_recv().expect("(B) filled event");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Filled(f)) => {
                assert_eq!(f.venue_order_id, venue_order_id);
            }
            other => panic!("(B) expected filled event, was {other:?}"),
        }
        // Re-emitted cancel after fill (B)
        let evt = receiver.try_recv().expect("(B) re-emitted cancel");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("(B) expected re-emitted cancel, was {other:?}"),
        }

        // (C) Cancel with size_matched=1.219511
        let msg_c = make_order("1.219511", "1775074738034", PolymarketEventType::Update);
        dispatch_user_message(&UserWsMessage::Order(msg_c), &ctx, &mut state);

        let evt = receiver.try_recv().expect("(C) canceled event");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("(C) expected canceled event, was {other:?}"),
        }

        // (D) Cancel with size_matched=2.560972 (capped to tracked 1.219511)
        let msg_d = make_order("2.560972", "1775074738038", PolymarketEventType::Update);
        dispatch_user_message(&UserWsMessage::Order(msg_d), &ctx, &mut state);

        let evt = receiver.try_recv().expect("(D) canceled event");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("(D) expected canceled event, was {other:?}"),
        }

        // (E) Trade fill 1.341461
        let msg_e = make_trade("trade-e", 1.341461, "1775074738036");
        dispatch_user_message(&UserWsMessage::Trade(msg_e), &ctx, &mut state);

        let evt = receiver.try_recv().expect("(E) filled event");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Filled(f)) => {
                assert_eq!(f.venue_order_id, venue_order_id);
            }
            other => panic!("(E) expected filled event, was {other:?}"),
        }

        // The fix: re-emitted cancel after (E) restores terminal state
        let evt = receiver.try_recv().expect("(E) re-emitted cancel");
        match &evt {
            ExecutionEvent::Order(OrderEventAny::Canceled(c)) => {
                assert_eq!(c.venue_order_id, Some(venue_order_id));
            }
            other => panic!("(E) expected re-emitted cancel, was {other:?}"),
        }

        // No more events
        assert!(
            receiver.try_recv().is_err(),
            "No further events expected after the sequence"
        );
    }

    #[rstest]
    fn test_dispatch_taker_fill_snaps_overfill_to_submitted_qty() {
        // Reproduces the V2 market-BUY scenario that motivated the dust-snap
        // fix: SDK truncates the registered qty to USDC scale, but the
        // on-chain fill comes back at full precision and exceeds submitted
        // by microshares. Without the snap the engine rejects as overfill.
        use crate::common::enums::{
            PolymarketEventType, PolymarketOrderSide, PolymarketOutcome, PolymarketTradeStatus,
        };

        let instrument = test_instrument();
        let asset_id = instrument.id().symbol.inner();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("0xtaker-overfill");
        // Submitted qty truncated to USDC scale.
        let submitted = Quantity::new(714.285710, instrument.size_precision());
        fill_tracker.register(
            venue_order_id,
            submitted,
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-OVERFILL",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        let trade = PolymarketUserTrade {
            asset_id,
            bucket_index: 0,
            fee_rate_bps: "0".to_string(),
            id: "trade-overfill".to_string(),
            last_update: "1700000001".to_string(),
            maker_address: Ustr::from("0xmaker"),
            maker_orders: vec![],
            market: Ustr::from("0xmarket"),
            match_time: "1700000000".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            price: "0.014".to_string(),
            side: PolymarketOrderSide::Buy,
            // Fill exceeds submitted_qty by 4 ulps at size_precision=6,
            // matching the production drift observed during smoke tests.
            size: "714.285714".to_string(),
            status: PolymarketTradeStatus::Matched,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            trader_side: PolymarketLiquiditySide::Taker,
            event_type: PolymarketEventType::Trade,
        };

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        // The dispatcher must record the snapped quantity in the tracker so
        // any subsequent ORDER MATCHED with size_matched > submitted_qty is
        // capped to it. record_fill happens before the FillReport is sent.
        let cumulative = fill_tracker
            .get_cumulative_filled(&venue_order_id)
            .expect("order must be registered");
        let expected_snapped = submitted.as_f64();
        let drift = (cumulative - expected_snapped).abs();
        assert!(
            drift < 1e-9,
            "cumulative_filled {cumulative} must be snapped to submitted {expected_snapped}",
        );

        // The emitted OrderFilled must carry the snapped qty so the engine
        // does not reject it as an overfill.
        let event = receiver.try_recv().expect("expected a filled event");
        match event {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(
                    filled.last_qty, submitted,
                    "filled qty must be snapped to submitted",
                );
                assert_eq!(filled.venue_order_id, venue_order_id);
            }
            other => panic!("expected filled event, was {other:?}"),
        }
    }

    #[rstest]
    fn test_dispatch_taker_fill_gross_overfill_raises_qty_then_fills() {
        // A marketable BUY filled below its limit returns more shares than the nominal qty (a
        // gross overfill, beyond the dust band). The dispatcher must raise the order qty via
        // OrderUpdated before the OrderFilled, or the engine drops the fill as an overfill.
        use crate::common::enums::{
            PolymarketEventType, PolymarketOrderSide, PolymarketOutcome, PolymarketTradeStatus,
        };

        let instrument = test_instrument();
        let asset_id = instrument.id().symbol.inner();
        let size_precision = instrument.size_precision();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("0xtaker-gross-overfill");
        let submitted = Quantity::new(30.0, size_precision);
        fill_tracker.register(
            venue_order_id,
            submitted,
            OrderSide::Buy,
            instrument.id(),
            size_precision,
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-GROSS-OVERFILL",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        let mut state = WsDispatchState::default();

        // 33.846152 shares against a nominal 30: a marketable fill below the limit price.
        let trade = PolymarketUserTrade {
            asset_id,
            bucket_index: 0,
            fee_rate_bps: "0".to_string(),
            id: "trade-gross-overfill".to_string(),
            last_update: "1700000001".to_string(),
            maker_address: Ustr::from("0xmaker"),
            maker_orders: vec![],
            market: Ustr::from("0xmarket"),
            match_time: "1700000000".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            price: "0.014".to_string(),
            side: PolymarketOrderSide::Buy,
            size: "33.846152".to_string(),
            status: PolymarketTradeStatus::Matched,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            trader_side: PolymarketLiquiditySide::Taker,
            event_type: PolymarketEventType::Trade,
        };

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let expected_qty = Quantity::new(33.846152, size_precision);

        // The raise must precede the fill so the engine accepts the larger quantity.
        match receiver.try_recv().expect("expected an updated event") {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.quantity, expected_qty);
                assert_eq!(updated.venue_order_id, Some(venue_order_id));
            }
            other => panic!("expected updated event raising qty to the fill, was {other:?}"),
        }

        match receiver.try_recv().expect("expected a filled event") {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(filled.last_qty, expected_qty);
                assert_eq!(filled.venue_order_id, venue_order_id);
            }
            other => panic!("expected filled event, was {other:?}"),
        }
    }

    // Unmatched -> Rejected (placement never became live); CanceledMarketResolved -> Expired
    // (market settled). Both are tracked own-order terminal states emitted as order events.
    #[rstest]
    #[case(crate::common::enums::PolymarketOrderStatus::Unmatched, "Rejected")]
    #[case(
        crate::common::enums::PolymarketOrderStatus::CanceledMarketResolved,
        "Expired"
    )]
    fn test_dispatch_order_terminal_status_emits_event(
        #[case] status: crate::common::enums::PolymarketOrderStatus,
        #[case] expected: &str,
    ) {
        use crate::common::enums::{
            PolymarketEventType, PolymarketOrderSide, PolymarketOrderType, PolymarketOutcome,
        };

        let instrument = test_instrument();
        let asset_id = instrument.id().symbol.inner();
        let order_id = "0xterminal-order".to_string();
        let venue_order_id = VenueOrderId::from(order_id.as_str());

        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("10"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-TERMINAL",
        );
        order_identities.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xabc",
            user_api_key: "xxx",
        };
        let mut state = WsDispatchState::default();

        let order = PolymarketUserOrder {
            asset_id,
            associate_trades: None,
            created_at: "1775074735".to_string(),
            expiration: Some("0".to_string()),
            id: order_id,
            maker_address: Ustr::from("0xabc"),
            market: Ustr::from("0x4134"),
            order_owner: Ustr::from("xxx"),
            order_type: PolymarketOrderType::FOK,
            original_size: "10".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("xxx"),
            price: "0.50".to_string(),
            side: PolymarketOrderSide::Buy,
            size_matched: "0".to_string(),
            status,
            timestamp: "1775074738031".to_string(),
            event_type: PolymarketEventType::Placement,
        };

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected terminal order event");
        match event {
            ExecutionEvent::Order(order_event) => {
                assert!(
                    format!("{order_event:?}").starts_with(expected),
                    "expected {expected}, was {order_event:?}"
                );
                assert_eq!(
                    order_event.client_order_id(),
                    ClientOrderId::from("O-TERMINAL")
                );
            }
            other => panic!("expected order event, was {other:?}"),
        }
    }
}
