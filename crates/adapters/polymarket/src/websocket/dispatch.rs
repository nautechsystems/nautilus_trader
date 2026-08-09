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
//! client into Nautilus order events (`OrderAccepted` / `OrderFilled` / `OrderFillVoided` /
//! `OrderCanceled` / `OrderRejected` / `OrderExpired`), building them from the identity captured at
//! submit (`OrderIdentityRegistry`). Order-channel messages drive lifecycle events; trade-channel
//! messages drive fills, and acceptance is synthesized before a fill or cancel that races ahead.
//! Messages are emitted once the order is known (accepted, or with a submit in flight), otherwise
//! buffered until acceptance. Reports are reserved for the `generate_*` query and reconciliation
//! methods. Trade fills are emitted at `MATCHED`, retained until terminal settlement, and reversed
//! with `OrderFillVoided` if the trade reaches `FAILED`.

use std::str::FromStr;

use ahash::AHashSet;
use indexmap::IndexMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{UUID4, UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled,
        OrderRejected, OrderUpdated,
    },
    identifiers::{AccountId, StrategyId, TradeId, VenueOrderId},
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
    common::{
        enums::{
            PolymarketLiquiditySide, PolymarketOrderSide, PolymarketOrderStatus,
            PolymarketOrderType, PolymarketTradeStatus,
        },
        fifo_ext::{add_to_fifo_map_with_eviction_warn, add_to_fifo_with_eviction_warn},
        models::PolymarketMakerOrder,
    },
    execution::{
        get_pusd_currency,
        identity::{OrderIdentity, OrderIdentityRegistry},
        order_fill_tracker::{
            AppliedPendingFill, BufferedFill, FillCorrectionMetadata, OrderFillTrackerMap,
        },
        parse::{
            build_maker_fill_report, compute_commission, determine_order_side,
            instrument_fee_exponent, instrument_taker_fee, parse_fill_values, parse_liquidity_side,
        },
        pending::PendingSubmitTracker,
    },
};

/// Signal returned when a finalized trade requires an async account refresh.
#[derive(Debug)]
pub(crate) struct AccountRefreshRequest;

#[cfg(test)]
std::thread_local! {
    static VOID_FAILED_AFTER_BUFFERED_EVIDENCE_REMOVAL: std::cell::RefCell<Option<Box<dyn FnOnce()>>> =
        std::cell::RefCell::new(None);
}

#[cfg(test)]
fn run_void_failed_after_buffered_evidence_removal() {
    VOID_FAILED_AFTER_BUFFERED_EVIDENCE_REMOVAL.with(|hook| {
        if let Some(hook) = hook.borrow_mut().take() {
            hook();
        }
    });
}

/// Mutable state retained across user WebSocket stream generations.
#[derive(Debug, Default)]
pub(crate) struct WsDispatchState {
    pub processed_fills: FifoCache<String, 10_000>,
    matched_fills: FifoCacheMap<String, Vec<OrderFilled>, 10_000>,
    matched_fill_evictions: u64,
    raw_applied_fills: FifoCacheMap<String, Vec<FillReport>, 10_000>,
    raw_applied_fill_evictions: u64,
    consumed_legs: FifoCacheMap<String, AHashSet<VenueOrderId>, 10_000>,
    ws_dedup_evictions: u64,
    voided_trades: FifoCache<String, 10_000>,
    confirmed_trades: FifoCache<String, 10_000>,
    pending_terminal_orders: FifoCacheMap<VenueOrderId, PendingTerminalOrder, 10_000>,
    /// Cancel reports saved for orders known to be terminal at the venue.
    /// Re-emitted after a fill to restore terminal state when fills race
    /// ahead of (or arrive after) cancel messages.
    terminal_cancel_reports: FifoCacheMap<VenueOrderId, OrderStatusReport, 10_000>,
}

impl WsDispatchState {
    pub(crate) fn restore_matched_trade(&mut self, key: String, fills: Vec<OrderFilled>) {
        self.add_processed_trade(key.clone());
        self.add_matched_trade(key, fills);
    }

    pub(crate) fn restore_voided_trade(&mut self, key: String) {
        self.add_processed_trade(key.clone());
        self.matched_fills.remove(&key);
        add_to_fifo_with_eviction_warn(&mut self.voided_trades, key, "WS voided-trade");
    }

    fn add_processed_trade(&mut self, key: String) {
        let evicted =
            add_to_fifo_with_eviction_warn(&mut self.processed_fills, key, "WS processed-trade");

        if evicted {
            self.ws_dedup_evictions = self.ws_dedup_evictions.saturating_add(1);
        }
    }

    fn add_consumed_legs(&mut self, key: String, consumed: AHashSet<VenueOrderId>) {
        let evicted = add_to_fifo_map_with_eviction_warn(
            &mut self.consumed_legs,
            key,
            consumed,
            "WS consumed-leg",
        );

        if evicted {
            self.ws_dedup_evictions = self.ws_dedup_evictions.saturating_add(1);
        }
    }

    fn add_matched_trade(&mut self, key: String, fills: Vec<OrderFilled>) {
        if let Some(held) = self.matched_fills.get_mut(&key) {
            held.extend(fills);
            return;
        }

        let evicted = add_to_fifo_map_with_eviction_warn(
            &mut self.matched_fills,
            key,
            fills,
            "WS matched-fill evidence",
        );

        if evicted {
            self.matched_fill_evictions = self.matched_fill_evictions.saturating_add(1);
            self.ws_dedup_evictions = self.ws_dedup_evictions.saturating_add(1);
        }
    }

    fn keep_raw_fill_retryable(&mut self, key: String, report: FillReport) {
        self.processed_fills.remove(&key);
        self.consumed_legs.remove(&key);

        if let Some(fills) = self.raw_applied_fills.get_mut(&key) {
            if !fills.iter().any(|fill| {
                fill.venue_order_id == report.venue_order_id && fill.trade_id == report.trade_id
            }) {
                fills.push(report);
            }
            return;
        }

        let evicted = add_to_fifo_map_with_eviction_warn(
            &mut self.raw_applied_fills,
            key,
            vec![report],
            "WS raw applied-fill evidence",
        );

        if evicted {
            self.raw_applied_fill_evictions = self.raw_applied_fill_evictions.saturating_add(1);
        }
    }
}

#[cfg(test)]
impl WsDispatchState {
    pub(crate) fn matched_fill_count(&self, key: &str) -> usize {
        self.matched_fills.get(&key.to_string()).map_or(0, Vec::len)
    }

    pub(crate) fn is_voided_trade(&self, key: &str) -> bool {
        self.voided_trades.contains(&key.to_string())
    }
}

#[derive(Clone, Debug)]
struct PendingTerminalOrder {
    trade_ids: Vec<String>,
    ts_event: UnixNanos,
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
    let Some(mut report) =
        build_ws_order_status_report(order, instrument, ctx.account_id, ts_event, ts_init)
    else {
        salvage_terminal_order_update(order, instrument, venue_order_id, ts_event, ctx);
        return;
    };
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
        )
    } else if is_accepted {
        ctx.fill_tracker
            .take_pending_fills(venue_order_id, local_client_order_id)
    } else {
        Vec::new()
    };

    // Order updates can race ahead of trade messages, so cap filled_qty
    // to what the fill tracker has recorded to prevent duplicate inferred fills
    if let Some(tracked_filled) = ctx.fill_tracker.get_cumulative_filled(&venue_order_id)
        && report.filled_qty > tracked_filled
    {
        log::debug!(
            "Capping filled_qty for {venue_order_id} from {} to {} (awaiting trade messages)",
            report.filled_qty,
            tracked_filled,
        );
        report.filled_qty = tracked_filled;
    }

    // Track cancel reports so we can re-emit them after late-arriving fills.
    // Saved regardless of acceptance state so that cancels arriving during
    // the HTTP round-trip are available once the order is later accepted.
    if report.order_status == OrderStatus::Canceled {
        add_to_fifo_map_with_eviction_warn(
            &mut state.terminal_cancel_reports,
            venue_order_id,
            report.clone(),
            "WS terminal-cancel-report",
        );
    }

    // Tracked own orders route through order events; externally-managed orders
    // (no captured identity) buffer until accepted or fall back to reports.
    let identity = ctx.order_identities.get(&venue_order_id);

    // Emit fills first: a terminal status would otherwise close the order ahead of them
    for fill in buffered_fills {
        match identity {
            Some(identity) => {
                emit_buffered_order_filled(&identity, &fill, ctx);
            }
            None => {
                let correction_key = fill
                    .correction
                    .as_ref()
                    .map(|correction| correction.correction_key.clone());
                ctx.emitter.send_fill_report(fill.report.clone());

                if let Some(correction_key) = correction_key {
                    state.keep_raw_fill_retryable(correction_key, fill.report);
                }
            }
        }
    }

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

    if order.status == PolymarketOrderStatus::Matched
        && let Some(trade_ids) = order.associate_trades.clone().filter(|ids| !ids.is_empty())
    {
        add_to_fifo_map_with_eviction_warn(
            &mut state.pending_terminal_orders,
            venue_order_id,
            PendingTerminalOrder {
                trade_ids,
                ts_event,
            },
            "WS pending-terminal-order",
        );
        emit_quantity_normalization_if_ready(venue_order_id, ctx, state);
    }
}

fn emit_buffered_order_filled(
    identity: &OrderIdentity,
    buffered: &BufferedFill,
    ctx: &WsDispatchContext<'_>,
) {
    let fill = &buffered.report;
    ensure_accepted(identity, fill.venue_order_id, fill.ts_event, ctx);

    let info = buffered
        .correction
        .as_ref()
        .and_then(|correction| correction.info.clone());
    let filled = build_order_filled(identity, fill, info, ctx);
    ctx.fill_tracker
        .emit_buffered_fill(filled, buffered.correction.as_ref(), |filled, new_qty| {
            if let Some(new_qty) = new_qty {
                emit_buy_overfill_update(
                    identity,
                    fill.venue_order_id,
                    new_qty,
                    fill.ts_event,
                    ctx,
                );
            }
            ctx.emitter.send_order_event(OrderEventAny::Filled(filled));
        });
}

fn emit_quantity_normalization_if_ready(
    venue_order_id: VenueOrderId,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let is_ready = state
        .pending_terminal_orders
        .get(&venue_order_id)
        .is_some_and(|pending| {
            pending
                .trade_ids
                .iter()
                .all(|trade_id| state.confirmed_trades.contains(trade_id))
        });

    if !is_ready {
        return;
    }

    let Some(pending) = state.pending_terminal_orders.remove(&venue_order_id) else {
        return;
    };

    let Some(identity) = ctx.order_identities.get(&venue_order_id) else {
        log::warn!("Cannot normalize terminal order {venue_order_id} without a local identity");
        return;
    };

    if let Some(quantity) = ctx
        .fill_tracker
        .check_terminal_quantity_normalization(&venue_order_id)
    {
        emit_terminal_quantity_update(&identity, venue_order_id, quantity, pending.ts_event, ctx);
    }
}

/// Emits the terminal order event for a taker order once its trade confirms.
///
/// Taker fills receive no order-channel `MATCHED` update. FOK is atomic, so a sub-cent quantity
/// difference can be normalized. IOC maps to FAK, so every positive remainder was killed by the
/// venue and must close as `Canceled` without changing the venue-reported fill quantity.
fn emit_taker_terminal_status(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    ts_event: UnixNanos,
) {
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

    let Some(identity) = ctx.order_identities.get(&venue_order_id) else {
        return;
    };

    if identity.requires_terminal_quantity_normalization() {
        if let Some(quantity) = ctx
            .fill_tracker
            .check_terminal_quantity_normalization(&venue_order_id)
        {
            emit_terminal_quantity_update(&identity, venue_order_id, quantity, ts_event, ctx);
        }
        return;
    }

    if identity.time_in_force == TimeInForce::Ioc
        && let Some(remainder) = ctx
            .fill_tracker
            .take_terminal_ioc_remainder(&venue_order_id)
    {
        log::debug!(
            "Closing terminal IOC order {venue_order_id} as Canceled (unfilled remainder={remainder})"
        );
        emit_order_canceled(&identity, venue_order_id, ts_event, ctx);
    }
}

fn dispatch_trade_update(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> Option<AccountRefreshRequest> {
    let dedup_key = trade.id.clone();

    if trade.status == PolymarketTradeStatus::Failed {
        return void_failed_trade(trade, dedup_key, ctx, state).then_some(AccountRefreshRequest);
    }

    if matches!(
        trade.status,
        PolymarketTradeStatus::Mined | PolymarketTradeStatus::Retrying
    ) {
        log::debug!("Waiting for terminal trade status: {}", trade.id);
        return None;
    }

    if has_unknown_trade_instrument(trade, ctx) {
        log::warn!(
            "Deferring trade {} until its instrument is available",
            trade.id
        );
        return None;
    }

    let is_confirmed = trade.status == PolymarketTradeStatus::Confirmed;
    let trade_id = TradeId::from(trade.id.as_str());
    let rest_applied_fills = ctx.fill_tracker.rest_applied_pending_fills(&trade_id);
    let rest_applied_order_ids = rest_applied_fills
        .iter()
        .map(|fill| fill.venue_order_id)
        .collect::<AHashSet<_>>();

    if is_confirmed && !rest_applied_fills.is_empty() {
        ctx.fill_tracker
            .mark_trade_rest_applied(&dedup_key, &rest_applied_fills);
    }

    let fully_processed = dispatch_trade_fills(
        trade,
        &dedup_key,
        is_confirmed,
        &rest_applied_order_ids,
        ctx,
        state,
    );

    if !is_confirmed {
        return None;
    }

    if !fully_processed {
        log::debug!(
            "Deferring confirmation of trade {} until a corrected redelivery applies its remaining fills",
            trade.id,
        );
        return None;
    }

    confirm_trade(trade, &dedup_key, ctx, state);

    if !rest_applied_fills.is_empty() {
        ctx.fill_tracker.clear_rest_applied_pending_trade(&trade_id);
    }
    Some(AccountRefreshRequest)
}

fn void_failed_trade(
    trade: &PolymarketUserTrade,
    dedup_key: String,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> bool {
    if state.voided_trades.contains(&dedup_key) {
        return true;
    }

    let trade_id = TradeId::from(trade.id.as_str());
    ctx.fill_tracker.note_failed_trade(&trade_id);
    let rest_applied_fills = ctx.fill_tracker.rest_applied_pending_fills(&trade_id);
    let Some(message_derived_reports) =
        build_message_derived_fill_void_reports(trade, &rest_applied_fills, ctx)
    else {
        return false;
    };
    let direct_fills = state.matched_fills.remove(&dedup_key).unwrap_or_default();

    for fill in &direct_fills {
        ctx.fill_tracker
            .reverse_fill(&fill.venue_order_id, fill.last_qty);
    }

    let mut fills = direct_fills;
    fills.extend(ctx.fill_tracker.void_buffered_trade(&dedup_key));
    #[cfg(test)]
    run_void_failed_after_buffered_evidence_removal();

    if fills.is_empty() && message_derived_reports.is_empty() {
        if state.raw_applied_fills.contains_key(&dedup_key)
            || state.matched_fill_evictions > 0
            || state.raw_applied_fill_evictions > 0
            || ctx.order_identities.has_evictions()
            || ctx.fill_tracker.has_correction_evidence_evictions()
        {
            ctx.fill_tracker.defer_trade_without_evidence(trade_id);
            ctx.fill_tracker.keep_buffered_trade_retryable(&dedup_key);
            log::warn!(
                "Cannot safely ignore failed trade {} with no addressable applied fill to void because correction state is incomplete or may have been evicted; leaving the trade retryable",
                trade.id,
            );
            return false;
        }

        log::warn!(
            "Ignoring failed trade {}: no fill was applied for this trade, or correction evidence was lost to restart/eviction",
            trade.id,
        );
        state.add_processed_trade(dedup_key);
        return true;
    }
    let evidence_venue_order_ids = message_derived_reports
        .iter()
        .map(|(report, _, _)| report.venue_order_id)
        .collect::<AHashSet<_>>();
    fills.retain(|fill| !evidence_venue_order_ids.contains(&fill.venue_order_id));

    for fill in &fills {
        emit_order_fill_voided(fill, trade, Some(fill.event_id), ctx);
    }

    for (report, strategy_id, order_type) in &message_derived_reports {
        emit_message_derived_fill_void(report, *strategy_id, *order_type, trade, ctx);
    }

    ctx.fill_tracker.clear_no_evidence_deferral(&trade_id);

    state.add_processed_trade(dedup_key.clone());
    state.consumed_legs.remove(&dedup_key);
    state.raw_applied_fills.remove(&dedup_key);
    add_to_fifo_with_eviction_warn(&mut state.voided_trades, dedup_key, "WS voided-trade");
    state.confirmed_trades.remove(&trade.id);
    ctx.fill_tracker.clear_rest_applied_pending_trade(&trade_id);
    true
}

fn build_message_derived_fill_void_reports(
    trade: &PolymarketUserTrade,
    applied_fills: &[AppliedPendingFill],
    ctx: &WsDispatchContext<'_>,
) -> Option<Vec<(FillReport, StrategyId, OrderType)>> {
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let ts_init = ctx.clock.get_time_ns();
    let mut reports = Vec::with_capacity(applied_fills.len());

    for applied_fill in applied_fills {
        let (Some(client_order_id), Some(strategy_id), Some(order_type)) = (
            applied_fill.client_order_id,
            applied_fill.strategy_id,
            applied_fill.order_type,
        ) else {
            log::error!(
                "Deferring failed trade {} fill void for venue order {} because its recorded identity is incomplete",
                trade.id,
                applied_fill.venue_order_id,
            );
            return None;
        };

        reports.push((
            FillReport {
                account_id: ctx.account_id,
                instrument_id: applied_fill.instrument_id,
                venue_order_id: applied_fill.venue_order_id,
                trade_id: applied_fill.trade_id,
                order_side: applied_fill.order_side,
                last_qty: applied_fill.last_qty,
                last_px: applied_fill.last_px,
                commission: applied_fill.commission,
                liquidity_side: applied_fill.liquidity_side,
                avg_px: None,
                report_id: UUID4::new(),
                ts_event,
                ts_init,
                client_order_id: Some(client_order_id),
                venue_position_id: None,
            },
            strategy_id,
            order_type,
        ));
    }

    Some(reports)
}

fn emit_message_derived_fill_void(
    report: &FillReport,
    strategy_id: StrategyId,
    order_type: OrderType,
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
) {
    let client_order_id = report
        .client_order_id
        .expect("message-derived void identity was prevalidated");
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let voided = OrderFillVoided::new(
        ctx.emitter.trader_id(),
        strategy_id,
        report.instrument_id,
        client_order_id,
        report.venue_order_id,
        ctx.account_id,
        Ustr::from(&format!("{}-FAILED-{client_order_id}", trade.id)),
        report.trade_id,
        report.last_qty,
        Some(report.commission),
        report.order_side,
        order_type,
        report.last_px,
        get_pusd_currency(),
        report.liquidity_side,
        None,
        Some(Ustr::from("FAILED")),
        trade_fill_info(trade),
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        false,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::FillVoided(voided));
}

fn has_unknown_trade_instrument(trade: &PolymarketUserTrade, ctx: &WsDispatchContext<'_>) -> bool {
    let instruments = ctx.token_instruments.load();

    if trade.trader_side == PolymarketLiquiditySide::Maker {
        trade
            .maker_orders
            .iter()
            .filter(|order| is_user_maker_order(order, ctx))
            .any(|order| !instruments.contains_key(&order.asset_id))
    } else {
        !instruments.contains_key(&trade.asset_id)
    }
}

fn dispatch_trade_fills(
    trade: &PolymarketUserTrade,
    dedup_key: &String,
    is_confirmed: bool,
    rest_applied_order_ids: &AHashSet<VenueOrderId>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> bool {
    if state.processed_fills.contains(dedup_key) {
        log::debug!("Duplicate fill skipped: {dedup_key}");
        return true;
    }

    let consumed_before = state
        .consumed_legs
        .get(dedup_key)
        .cloned()
        .unwrap_or_default();
    let (fills, any_invalid, consumed_now) = if trade.trader_side == PolymarketLiquiditySide::Maker
    {
        dispatch_maker_fills(
            trade,
            dedup_key,
            is_confirmed,
            rest_applied_order_ids,
            &consumed_before,
            ctx,
            state,
        )
    } else {
        dispatch_taker_fill(
            trade,
            dedup_key,
            is_confirmed,
            rest_applied_order_ids,
            &consumed_before,
            ctx,
            state,
        )
    };

    if !consumed_now.is_empty() {
        if let Some(consumed) = state.consumed_legs.get_mut(dedup_key) {
            consumed.extend(consumed_now);
        } else {
            state.add_consumed_legs(dedup_key.clone(), consumed_now.into_iter().collect());
        }
    }

    if !fills.is_empty() {
        state.add_matched_trade(dedup_key.clone(), fills);
    }

    if any_invalid {
        return false;
    }

    state.raw_applied_fills.remove(dedup_key);

    state.add_processed_trade(dedup_key.clone());
    true
}

fn confirm_trade(
    trade: &PolymarketUserTrade,
    dedup_key: &str,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    ctx.fill_tracker.mark_trade_confirmed(dedup_key);
    add_to_fifo_with_eviction_warn(
        &mut state.confirmed_trades,
        trade.id.clone(),
        "WS confirmed-trade",
    );

    if trade.trader_side == PolymarketLiquiditySide::Maker {
        for order in trade
            .maker_orders
            .iter()
            .filter(|order| is_user_maker_order(order, ctx))
        {
            emit_quantity_normalization_if_ready(
                VenueOrderId::from(order.order_id.as_str()),
                ctx,
                state,
            );
        }
    } else {
        emit_quantity_normalization_if_ready(
            VenueOrderId::from(trade.taker_order_id.as_str()),
            ctx,
            state,
        );
        emit_taker_terminal_status(trade, ctx, ts_event);
    }
}

fn dispatch_maker_fills(
    trade: &PolymarketUserTrade,
    correction_key: &str,
    is_confirmed: bool,
    rest_applied_order_ids: &AHashSet<VenueOrderId>,
    consumed_legs: &AHashSet<VenueOrderId>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> (Vec<OrderFilled>, bool, Vec<VenueOrderId>) {
    let replay_risk = state.ws_dedup_evictions > 0;
    let owned_orders: Vec<_> = trade
        .maker_orders
        .iter()
        .filter(|order| is_user_maker_order(order, ctx))
        .collect();

    if owned_orders.is_empty() {
        log::warn!("No matching maker orders for user in trade: {}", trade.id);
        return (Vec::new(), true, Vec::new());
    }

    let user_orders: Vec<_> = owned_orders
        .into_iter()
        .filter(|order| {
            !rest_applied_order_ids.contains(&VenueOrderId::from(order.order_id.as_str()))
        })
        .collect();

    if user_orders.is_empty() {
        log::debug!(
            "All owned maker fills for trade {} were already applied via REST reconciliation",
            trade.id,
        );
        return (Vec::new(), false, Vec::new());
    }

    let instruments = ctx.token_instruments.load();
    let fill_info = trade_fill_info(trade);
    let liquidity_side = parse_liquidity_side(trade.trader_side);
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let ts_init = ctx.clock.get_time_ns();
    let mut fills = Vec::new();
    let mut any_invalid = false;
    let mut consumed_now = Vec::new();

    for mo in user_orders {
        let maker_venue_order_id = VenueOrderId::from(mo.order_id.as_str());

        if consumed_legs.contains(&maker_venue_order_id) {
            log::debug!(
                "Skipping already-applied maker leg {maker_venue_order_id} of trade {}",
                trade.id,
            );
            continue;
        }

        let asset_id = Ustr::from(mo.asset_id.as_str());
        let instrument = match instruments.get(&asset_id) {
            Some(i) => i,
            None => {
                log::warn!("Unknown asset_id in maker order: {asset_id}");
                any_invalid = true;
                continue;
            }
        };
        let price_precision = instrument.price_precision();
        let size_precision = instrument.size_precision();

        if parse_fill_values(
            &trade.id,
            mo.matched_amount,
            mo.price,
            price_precision,
            size_precision,
        )
        .is_none()
        {
            log::warn!(
                "Skipping live maker fill for trade {} with invalid quantity or price",
                trade.id,
            );
            any_invalid = true;
            continue;
        }

        let mut report = build_maker_fill_report(
            mo,
            &trade.id,
            trade.trader_side,
            trade.side,
            trade.asset_id.as_str(),
            ctx.account_id,
            instrument.id(),
            price_precision,
            size_precision,
            crate::execution::get_pusd_currency(),
            liquidity_side,
            ts_event,
            ts_init,
        );
        report.client_order_id = ctx.pending_submits.client_order_id(&maker_venue_order_id);
        report.last_qty = ctx
            .fill_tracker
            .snap_fill_qty(&maker_venue_order_id, report.last_qty);
        let track_fill = !replay_risk || !ctx.fill_tracker.contains(&maker_venue_order_id);

        if !track_fill {
            log::error!(
                "WS dedup evidence was evicted; emitting unknown maker trade {correction_key} for {maker_venue_order_id} without updating local fill accumulation or order quantity",
            );
        }

        if let Some(report) = ctx.fill_tracker.accept_or_buffer_fill(
            maker_venue_order_id,
            report,
            FillCorrectionMetadata {
                correction_key: correction_key.to_string(),
                trade_id: TradeId::from(trade.id.as_str()),
                info: fill_info.clone(),
                is_confirmed,
                track_fill,
            },
        ) {
            match ctx.order_identities.get(&maker_venue_order_id) {
                Some(identity) => {
                    fills.push(emit_order_filled(
                        &identity,
                        &report,
                        fill_info.clone(),
                        ctx,
                        track_fill,
                    ));
                    consumed_now.push(maker_venue_order_id);
                }
                None => {
                    ctx.emitter.send_fill_report(report.clone());
                    state.keep_raw_fill_retryable(correction_key.to_string(), report);
                    any_invalid = true;
                }
            }
            reemit_terminal_cancel(maker_venue_order_id, state, ctx);
        } else {
            consumed_now.push(maker_venue_order_id);
        }
    }
    (fills, any_invalid, consumed_now)
}

fn is_user_maker_order(order: &PolymarketMakerOrder, ctx: &WsDispatchContext<'_>) -> bool {
    order.is_owned_by(ctx.user_address, ctx.user_api_key)
}

fn dispatch_taker_fill(
    trade: &PolymarketUserTrade,
    correction_key: &str,
    is_confirmed: bool,
    rest_applied_order_ids: &AHashSet<VenueOrderId>,
    consumed_legs: &AHashSet<VenueOrderId>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> (Vec<OrderFilled>, bool, Vec<VenueOrderId>) {
    let replay_risk = state.ws_dedup_evictions > 0;
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

    if rest_applied_order_ids.contains(&venue_order_id) {
        return (Vec::new(), false, Vec::new());
    }

    if consumed_legs.contains(&venue_order_id) {
        log::debug!(
            "Skipping already-applied taker fill {venue_order_id} of trade {}",
            trade.id,
        );
        return (Vec::new(), false, Vec::new());
    }

    let instruments = ctx.token_instruments.load();
    let instrument = match instruments.get(&trade.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in trade: {}", trade.asset_id);
            return (Vec::new(), true, Vec::new());
        }
    };

    let raw_size = match Decimal::from_str(&trade.size) {
        Ok(size) => size,
        Err(e) => {
            log::warn!(
                "Skipping live taker fill for trade {} with invalid size {}: {e}",
                trade.id,
                trade.size,
            );
            return (Vec::new(), true, Vec::new());
        }
    };

    let raw_price = match Decimal::from_str(&trade.price) {
        Ok(price) => price,
        Err(e) => {
            log::warn!(
                "Skipping live taker fill for trade {} with invalid price {}: {e}",
                trade.id,
                trade.price,
            );
            return (Vec::new(), true, Vec::new());
        }
    };
    let price_precision = instrument.price_precision();
    let size_precision = instrument.size_precision();

    if parse_fill_values(
        &trade.id,
        raw_size,
        raw_price,
        price_precision,
        size_precision,
    )
    .is_none()
    {
        log::warn!(
            "Skipping live taker fill for trade {} with invalid quantity or price",
            trade.id,
        );
        return (Vec::new(), true, Vec::new());
    }

    let liquidity_side = parse_liquidity_side(trade.trader_side);
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let ts_init = ctx.clock.get_time_ns();

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
    let track_fill = !replay_risk || !ctx.fill_tracker.contains(&venue_order_id);

    if !track_fill {
        log::error!(
            "WS dedup evidence was evicted; emitting unknown taker trade {correction_key} for {venue_order_id} without updating local fill accumulation or order quantity",
        );
    }

    if let Some(report) = ctx.fill_tracker.accept_or_buffer_fill(
        venue_order_id,
        report,
        FillCorrectionMetadata {
            correction_key: correction_key.to_string(),
            trade_id: TradeId::from(trade.id.as_str()),
            info: trade_fill_info(trade),
            is_confirmed,
            track_fill,
        },
    ) {
        match ctx.order_identities.get(&venue_order_id) {
            Some(identity) => {
                let fill =
                    emit_order_filled(&identity, &report, trade_fill_info(trade), ctx, track_fill);
                reemit_terminal_cancel(venue_order_id, state, ctx);
                return (vec![fill], false, vec![venue_order_id]);
            }
            None => {
                ctx.emitter.send_fill_report(report.clone());
                state.keep_raw_fill_retryable(correction_key.to_string(), report);
                reemit_terminal_cancel(venue_order_id, state, ctx);
                return (Vec::new(), true, Vec::new());
            }
        }
    }
    (Vec::new(), false, vec![venue_order_id])
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

/// Returns `None` when the payload's quantity or price fields cannot be represented.
fn build_ws_order_status_report(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> Option<OrderStatusReport> {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order.order_type);
    let size_precision = instrument.size_precision();
    let price_precision = instrument.price_precision();
    let price_dec = Decimal::from_str(&order.price).unwrap_or_default();
    let quantity = Decimal::from_str(&order.original_size)
        .ok()
        .map(|size| original_size_to_shares(size, price_dec, order.side, order.order_type))
        .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok())
        .filter(|quantity| !quantity.is_zero());

    let Some(quantity) = quantity else {
        log::warn!(
            "Skipping order update {venue_order_id}: original_size {} is unrepresentable or zero",
            order.original_size,
        );
        return None;
    };

    // The venue omits size_matched until a match occurs.
    let filled_qty = if order.size_matched.is_empty() {
        Some(Quantity::zero(size_precision))
    } else {
        Decimal::from_str(&order.size_matched)
            .ok()
            .and_then(|d| Quantity::from_decimal_dp(d, size_precision).ok())
    };

    let Some(filled_qty) = filled_qty else {
        log::warn!(
            "Skipping order update {venue_order_id}: size_matched {} is unrepresentable",
            order.size_matched,
        );
        return None;
    };

    let order_status = crate::execution::parse::resolve_matched_order_status(
        order.status,
        order.order_type,
        order.event_type,
        filled_qty.as_decimal(),
        quantity.as_decimal(),
    );

    if order_status == OrderStatus::Filled && filled_qty.is_zero() {
        log::warn!(
            "Skipping order update {venue_order_id}: status Filled carries a zero matched quantity",
        );
        return None;
    }

    // Market order types can omit the price; the quantity remains authoritative.
    let price = if order.price.is_empty() {
        Some(Price::zero(price_precision))
    } else {
        Decimal::from_str(&order.price)
            .ok()
            .and_then(|d| Price::from_decimal_dp(d, price_precision).ok())
            .filter(|price| price.as_decimal() > Decimal::ZERO && price.as_decimal() < Decimal::ONE)
    };

    let Some(price) = price else {
        log::warn!(
            "Skipping order update {venue_order_id}: price {} is unrepresentable or outside (0, 1)",
            order.price,
        );
        return None;
    };

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
    Some(report)
}

/// Converts a venue-reported `original_size` on a user-channel order message into shares.
///
/// The venue echoes the signed `makerAmount`, which for a BUY is the pUSD budget rather than a
/// share count (see `compute_maker_taker_amounts`). Dividing by the order price recovers the
/// signed `takerAmount`, which is the share quantity the client submitted.
///
/// This is confirmed for the market order types (`FAK` and `FOK`), where a BUY at 0.01 for 100
/// shares reports `1`. A SELL signs shares as its maker amount and needs no conversion. Resting
/// types pass through unchanged: their denomination is unconfirmed, and converting a
/// share-denominated size would misreport every externally-managed resting order.
fn original_size_to_shares(
    original_size: Decimal,
    price: Decimal,
    side: PolymarketOrderSide,
    order_type: PolymarketOrderType,
) -> Decimal {
    if side != PolymarketOrderSide::Buy
        || !matches!(
            order_type,
            PolymarketOrderType::FAK | PolymarketOrderType::FOK
        )
    {
        return original_size;
    }

    if price <= Decimal::ZERO {
        log::warn!(
            "Cannot convert {order_type} BUY size {original_size} pUSD to shares \
             without a positive price, reporting the venue amount"
        );
        return original_size;
    }

    original_size / price
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
    let trade_id = TradeId::from(trade.id.as_str());
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
    let commission_value = compute_commission(
        fee_rate,
        instrument_fee_exponent(instrument),
        size_dec,
        price_dec,
        liquidity_side,
    );
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
///
/// `info` carries the venue fill metadata (the raw trade fields) for trade-sourced fills, and is
/// `None` for order-path fills that have no originating trade payload.
fn emit_order_filled(
    identity: &OrderIdentity,
    fill: &FillReport,
    info: Option<IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
    allow_overfill_bump: bool,
) -> OrderFilled {
    ensure_accepted(identity, fill.venue_order_id, fill.ts_event, ctx);

    if allow_overfill_bump
        && let Some(new_qty) = ctx.fill_tracker.buy_overfill_bump(&fill.venue_order_id)
    {
        emit_buy_overfill_update(identity, fill.venue_order_id, new_qty, fill.ts_event, ctx);
    }

    let filled = build_order_filled(identity, fill, info, ctx);
    ctx.emitter
        .send_order_event(OrderEventAny::Filled(filled.clone()));
    filled
}

fn build_order_filled(
    identity: &OrderIdentity,
    fill: &FillReport,
    info: Option<IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
) -> OrderFilled {
    OrderFilled::new(
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
        info,
    )
}

fn emit_order_fill_voided(
    fill: &OrderFilled,
    trade: &PolymarketUserTrade,
    causation_id: Option<UUID4>,
    ctx: &WsDispatchContext<'_>,
) {
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let mut voided = OrderFillVoided::new(
        fill.trader_id,
        fill.strategy_id,
        fill.instrument_id,
        fill.client_order_id,
        fill.venue_order_id,
        fill.account_id,
        Ustr::from(&format!("{}-FAILED-{}", trade.id, fill.client_order_id)),
        fill.trade_id,
        fill.last_qty,
        fill.commission,
        fill.order_side,
        fill.order_type,
        fill.last_px,
        fill.currency,
        fill.liquidity_side,
        fill.position_id,
        Some(Ustr::from("FAILED")),
        trade_fill_info(trade),
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        false,
    );
    voided.causation_id = causation_id;
    ctx.emitter
        .send_order_event(OrderEventAny::FillVoided(voided));
}

/// Flattens a user trade into a string map of venue fill metadata for `OrderFilled.info`.
///
/// Mirrors the v1 adapter, which attaches the full raw trade to each fill it generates. Scalar
/// fields map to their string form; nested fields (such as `maker_orders`) become their JSON text.
fn trade_fill_info(trade: &PolymarketUserTrade) -> Option<IndexMap<Ustr, Ustr>> {
    let value = serde_json::to_value(trade).ok()?;
    let object = value.as_object()?;
    let mut info = IndexMap::with_capacity(object.len());
    for (key, val) in object {
        let val_str = match val {
            serde_json::Value::String(s) => s.clone(),
            other => other.to_string(),
        };
        info.insert(Ustr::from(key.as_str()), Ustr::from(val_str.as_str()));
    }
    Some(info)
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

/// Emits an order-only reconciliation update which cannot change strategy position.
fn emit_terminal_quantity_update(
    identity: &OrderIdentity,
    venue_order_id: VenueOrderId,
    quantity: Quantity,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let updated = OrderUpdated::new(
        ctx.emitter.trader_id(),
        identity.strategy_id,
        identity.instrument_id,
        identity.client_order_id,
        quantity,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        true,
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

fn salvage_terminal_order_update(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let invalid_price = !order.price.is_empty()
        && Decimal::from_str(&order.price)
            .ok()
            .and_then(|price| Price::from_decimal_dp(price, instrument.price_precision()).ok())
            .is_none_or(|price| {
                price.as_decimal() <= Decimal::ZERO || price.as_decimal() >= Decimal::ONE
            });
    let is_partial_fak_match = order.status == PolymarketOrderStatus::Matched
        && order.order_type == PolymarketOrderType::FAK
        && invalid_price
        && Decimal::from_str(&order.size_matched)
            .ok()
            .zip(Decimal::from_str(&order.original_size).ok())
            .is_some_and(|(size_matched, original_size)| size_matched < original_size);
    let order_status = if is_partial_fak_match {
        OrderStatus::Canceled
    } else {
        crate::execution::parse::resolve_order_status(order.status, order.event_type)
    };

    if !matches!(
        order_status,
        OrderStatus::Canceled | OrderStatus::Expired | OrderStatus::Rejected
    ) {
        return;
    }

    let Some(identity) = ctx.order_identities.get(&venue_order_id) else {
        return;
    };

    log::warn!(
        "Recovering terminal status {order_status:?} for {venue_order_id} from an order update with unrepresentable values",
    );

    match order_status {
        OrderStatus::Canceled => {
            ensure_accepted(&identity, venue_order_id, ts_event, ctx);
            emit_order_canceled(&identity, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Expired => {
            ensure_accepted(&identity, venue_order_id, ts_event, ctx);
            emit_order_expired(&identity, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Rejected => emit_order_rejected(&identity, "REJECTED", ts_event, ctx),
        _ => {}
    }
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
    use std::sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    };

    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::AtomicTime;
    use nautilus_model::{
        enums::{AccountType, OrderStatus},
        events::OrderEventAny,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{Order, OrderError, builder::OrderTestBuilder},
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::{
            enums::PolymarketOrderType,
            test_logger::{capture_start, records_since},
        },
        execution::parse::make_composite_trade_id,
        http::{
            models::GammaMarket,
            parse::{create_instrument_from_def, parse_gamma_market},
        },
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
                time_in_force: TimeInForce::Gtc,
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

    fn record_applied_pending_fill(
        fill_tracker: &OrderFillTrackerMap,
        trade_id: TradeId,
        venue_order_id: VenueOrderId,
        last_qty: Quantity,
        client_order_id: &str,
    ) -> AppliedPendingFill {
        let fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: test_instrument().id(),
            trade_id,
            client_order_id: Some(ClientOrderId::from(client_order_id)),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty,
            last_px: Price::from("0.50"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Taker,
        };
        record_applied_pending_fills(fill_tracker, &[(trade_id, fill.clone())]);
        fill
    }

    fn record_applied_pending_fills(
        fill_tracker: &OrderFillTrackerMap,
        fills: &[(TradeId, AppliedPendingFill)],
    ) {
        fill_tracker.begin_rest_applied_pending_pass();
        let withheld = fill_tracker.record_rest_applied_pending_fills(fills);
        assert!(withheld.is_empty());
    }

    fn test_order_filled(
        venue_order_id: VenueOrderId,
        trade_id: TradeId,
        client_order_id: &str,
    ) -> OrderFilled {
        OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            InstrumentId::from("TEST.POLYMARKET"),
            ClientOrderId::from(client_order_id),
            venue_order_id,
            AccountId::from("POLY-001"),
            trade_id,
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("1.000000"),
            Price::from("0.50"),
            get_pusd_currency(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::default(),
            UnixNanos::default(),
            false,
            None,
            Some(Money::zero(get_pusd_currency())),
            None,
        )
    }

    fn second_owned_maker_order(trade: &PolymarketUserTrade) -> PolymarketMakerOrder {
        let mut order = trade.maker_orders[0].clone();
        order.order_id =
            "0xmaker02maker02maker02maker02maker02maker02maker02maker02maker02maker02".to_string();
        order
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
        )
        .unwrap();

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        // A resting BUY already reports shares, so its size passes through unconverted
        assert_eq!(report.quantity.as_decimal(), dec!(100));
        assert_eq!(
            report.price.map(|price| price.as_decimal()),
            Some(dec!(0.5))
        );
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
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
    }

    // A market-order-type BUY reports the signed pUSD maker amount, so shares come from
    // dividing by the price. A SELL and the resting types already report shares.
    #[rstest]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::FOK,
        dec!(1.01),
        dec!(0.01),
        dec!(101)
    )]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::FOK,
        dec!(12),
        dec!(0.6),
        dec!(20)
    )]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::FAK,
        dec!(1),
        dec!(0.01),
        dec!(100)
    )]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::GTC,
        dec!(20),
        dec!(0.18),
        dec!(20)
    )]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::GTD,
        dec!(20),
        dec!(0.18),
        dec!(20)
    )]
    #[case(
        PolymarketOrderSide::Sell,
        PolymarketOrderType::FOK,
        dec!(20),
        dec!(0.6),
        dec!(20)
    )]
    #[case(
        PolymarketOrderSide::Buy,
        PolymarketOrderType::FOK,
        dec!(1.01),
        dec!(0),
        dec!(1.01)
    )]
    fn test_original_size_to_shares(
        #[case] side: PolymarketOrderSide,
        #[case] order_type: PolymarketOrderType,
        #[case] original_size: Decimal,
        #[case] price: Decimal,
        #[case] expected: Decimal,
    ) {
        let shares = original_size_to_shares(original_size, price, side, order_type);

        assert_eq!(shares, expected);
    }

    // A non-terminating division must still round to the instrument's size precision, and a
    // price the venue omits must leave the size unconverted rather than drop the report to zero.
    #[rstest]
    #[case("1", "0.03", "33.333333", "0.03")]
    #[case("1.01", "", "1.01", "0")]
    fn test_build_ws_order_status_report_fok_buy_quantity(
        #[case] original_size: &str,
        #[case] price: &str,
        #[case] expected_quantity: &str,
        #[case] expected_price: &str,
    ) {
        let mut order: PolymarketUserOrder = load("ws_user_order_fok_buy_pusd_size.json");
        order.original_size = original_size.to_string();
        order.price = price.to_string();
        let instrument = test_instrument();

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(2_000_000_000u64),
        )
        .expect("FOK BUY order update must build a report");

        assert_eq!(
            report.quantity.as_decimal(),
            Decimal::from_str_exact(expected_quantity).unwrap()
        );
        assert_eq!(
            report.price.map(|price| price.as_decimal()),
            Some(Decimal::from_str_exact(expected_price).unwrap())
        );
    }

    #[rstest]
    fn test_dispatch_fok_buy_registers_share_quantity_for_in_flight_submit() {
        let order: PolymarketUserOrder = load("ws_user_order_fok_buy_pusd_size.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());

        // No registration: the submit response has not landed, so the order update registers it
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let emitter = test_emitter();

        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-FOK-IN-FLIGHT");
        pending_submits.insert(venue_order_id, client_order_id);
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
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

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        // The venue reported 1.01 pUSD for the 101 shares submitted at 0.01
        assert_eq!(
            fill_tracker
                .submitted_qty(&venue_order_id)
                .map(|qty| qty.as_decimal()),
            Some(dec!(101)),
        );
    }

    #[rstest]
    fn test_dispatch_fok_buy_report_quantity_is_shares_without_identity() {
        let order: PolymarketUserOrder = load("ws_user_order_fok_buy_pusd_size.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("101"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        // No identity registered, so the order surfaces as a report for reconciliation
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

        let event = receiver.try_recv().expect("expected order report");
        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("expected an order report, was {event:?}");
        };

        assert_eq!(report.venue_order_id, venue_order_id);
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.time_in_force, TimeInForce::Fok);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.quantity.as_decimal(), dec!(101));
        assert_eq!(report.filled_qty.as_decimal(), dec!(0));
        assert_eq!(
            report.price.map(|price| price.as_decimal()),
            Some(dec!(0.01))
        );
    }

    #[rstest]
    fn test_build_ws_order_status_report_partial_fak_maps_to_canceled() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.status = PolymarketOrderStatus::Matched;
        order.order_type = PolymarketOrderType::FAK;
        order.original_size = "10".to_string();
        order.size_matched = "4".to_string();
        let instrument = test_instrument();

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(2_000_000_000u64),
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Canceled);
        assert_eq!(report.filled_qty, Quantity::from("4.000000"));
    }

    #[rstest]
    fn test_build_ws_order_status_report_full_fak_stays_filled() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.status = PolymarketOrderStatus::Matched;
        order.order_type = PolymarketOrderType::FAK;
        // A SELL signs shares as its maker amount, so original_size needs no conversion.
        order.side = PolymarketOrderSide::Sell;
        order.original_size = "10".to_string();
        order.size_matched = "10".to_string();
        let instrument = test_instrument();

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(2_000_000_000u64),
        )
        .unwrap();

        assert_eq!(report.order_status, OrderStatus::Filled);
    }

    #[rstest]
    fn test_build_ws_order_status_report_rejects_filled_with_zero_matched() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.status = PolymarketOrderStatus::Matched;
        order.order_type = PolymarketOrderType::GTC;
        order.original_size = "10".to_string();
        order.size_matched = "0".to_string();
        let instrument = test_instrument();

        assert!(
            build_ws_order_status_report(
                &order,
                &instrument,
                AccountId::from("POLY-001"),
                UnixNanos::from(1_000_000_000u64),
                UnixNanos::from(2_000_000_000u64),
            )
            .is_none()
        );
    }

    #[rstest]
    fn test_corrupt_terminal_order_update_still_closes_the_order() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.status = PolymarketOrderStatus::Canceled;
        order.original_size = "abc".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-CORRUPT-TERMINAL",
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

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Canceled(event)) => {
                assert_eq!(event.venue_order_id, Some(venue_order_id));
            }
            other => panic!("expected terminal cancel, was {other:?}"),
        }
    }

    #[rstest]
    #[case::partial_fak(PolymarketOrderType::FAK, true)]
    #[case::partial_gtc(PolymarketOrderType::GTC, false)]
    fn test_corrupt_partial_matched_order_salvages_only_fak_as_canceled(
        #[case] order_type: PolymarketOrderType,
        #[case] expect_canceled: bool,
    ) {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.status = PolymarketOrderStatus::Matched;
        order.order_type = order_type;
        order.original_size = "100".to_string();
        order.size_matched = "50".to_string();
        order.price = "NaN".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-CORRUPT-PARTIAL-MATCHED",
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

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        if expect_canceled {
            match receiver.try_recv().expect("expected salvaged cancellation") {
                ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) => {
                    assert_eq!(canceled.venue_order_id, Some(venue_order_id));
                    assert_eq!(
                        canceled.client_order_id,
                        ClientOrderId::from("O-CORRUPT-PARTIAL-MATCHED")
                    );
                }
                other => panic!("expected salvaged cancellation, was {other:?}"),
            }
        }

        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_corrupt_order_update_does_not_register_zero_submitted_qty() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.original_size = "abc".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, ClientOrderId::from("O-CORRUPT-SUBMIT"));
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

        assert!(!fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
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
        assert_eq!(report.trade_id.as_str(), trade.id);
        assert_eq!(report.ts_event, ts_event);
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_trade_fill_info_flattens_raw_trade() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");

        let info = trade_fill_info(&trade).expect("info should be present");

        // Every raw trade field is captured (mirrors v1 info=msg.to_dict()).
        assert_eq!(info.len(), 21);
        assert_eq!(info[&Ustr::from("id")], Ustr::from("trade-0xabcdef1234"));
        assert_eq!(info[&Ustr::from("fee_rate_bps")], Ustr::from("0"));
        assert_eq!(
            info[&Ustr::from("transaction_hash")],
            Ustr::from("0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890ab")
        );
        // Numeric fields flatten to their string form.
        assert_eq!(info[&Ustr::from("bucket_index")], Ustr::from("1"));
        assert_eq!(info[&Ustr::from("size")], Ustr::from("25.0"));
        assert_eq!(
            info[&Ustr::from("taker_order_id")],
            Ustr::from("0x1234567890abcdef1234567890abcdef1234567890abcdef1234567890abcdef12")
        );
        // The `type` serde-rename key is preserved.
        assert_eq!(info[&Ustr::from("type")], Ustr::from("TRADE"));
        // Nested fields become their JSON text.
        let maker_orders = info[&Ustr::from("maker_orders")].as_str();
        assert!(maker_orders.starts_with('['));
        assert!(maker_orders.contains("order_id"));

        let empty_hash_trade: PolymarketUserTrade = load("ws_user_trade_msg.json");
        let empty_hash_info =
            trade_fill_info(&empty_hash_trade).expect("empty hash info should be present");
        assert!(!empty_hash_info.contains_key(&Ustr::from("transaction_hash")));
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
    fn test_dispatch_maker_fill_owned_by_case_variant_address() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let configured_address = trade.maker_orders[0].maker_address.clone();
        let case_variant_address = configured_address
            .to_ascii_uppercase()
            .replacen("0X", "0x", 1);
        assert_ne!(case_variant_address, configured_address);
        trade.maker_orders[0].maker_address = case_variant_address;
        let foreign_api_key = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        assert_ne!(trade.maker_orders[0].owner, foreign_api_key);

        let venue_order_id = VenueOrderId::from(trade.maker_orders[0].order_id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.maker_orders[0].asset_id, test_instrument());
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
            user_address: &configured_address,
            user_api_key: foreign_api_key,
        };
        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fills = fill_tracker.pending_fills_for(&venue_order_id);
        assert_eq!(fills.len(), 1);
        assert_eq!(fills[0].venue_order_id, venue_order_id);
    }

    #[rstest]
    fn test_maker_trade_without_owned_leg_retries_corrected_delivery() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let owned_order = trade.maker_orders[0].clone();
        let venue_order_id = VenueOrderId::from(owned_order.order_id.as_str());
        trade.maker_orders[0].maker_address =
            "0x0000000000000000000000000000000000000001".to_string();
        trade.maker_orders[0].owner = "foreign-api-key".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(owned_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-MAKER-CORRECTED-OWNERSHIP",
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
            user_address: &owned_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(receiver.try_recv().is_err());
        assert!(!state.processed_fills.contains(&trade.id));

        trade.status = PolymarketTradeStatus::Confirmed;
        trade.maker_orders[0] = owned_order.clone();
        let confirmed =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let fill = match receiver.try_recv().expect("expected corrected maker fill") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => fill,
            other => panic!("expected corrected maker fill, was {other:?}"),
        };
        assert_eq!(fill.venue_order_id, venue_order_id);
        assert!(confirmed.is_some());
        assert!(state.processed_fills.contains(&trade.id));
        assert!(receiver.try_recv().is_err());
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
    fn test_restored_raw_trade_id_suppresses_websocket_redelivery() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
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
        let raw_trade_id = trade.id.clone();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        state.restore_matched_trade(raw_trade_id.clone(), Vec::new());

        let refresh = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(refresh.is_some());
        assert!(state.processed_fills.contains(&raw_trade_id));
        assert!(fill_tracker.pending_fills_for(&venue_order_id).is_empty());
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_dispatch_trade_replays_after_instrument_becomes_available() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
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

        let first_result =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        token_instruments.insert(trade.asset_id, instrument);
        let replay_result = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(first_result.is_none());
        assert!(replay_result.is_some());
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 1);
    }

    #[rstest]
    #[case(crate::common::enums::PolymarketTradeStatus::Mined)]
    #[case(crate::common::enums::PolymarketTradeStatus::Retrying)]
    fn test_dispatch_trade_ignores_pending_settlement_status(
        #[case] status: crate::common::enums::PolymarketTradeStatus,
    ) {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = status;
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

        let result = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(result.is_none());
        assert!(fill_tracker.pending_fills_for(&venue_order_id).is_empty());
    }

    #[rstest]
    fn test_dispatch_matched_trade_emits_fill_and_failed_trade_voids_it() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Matched;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
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
            "O-MATCHED-FAILED",
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

        let matched = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected matched fill, was {other:?}"),
        };
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        let failed = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected failed fill correction, was {other:?}"),
        };

        assert!(matched.is_none());
        assert!(failed.is_some());
        assert_eq!(voided.trade_id, filled.trade_id);
        assert_eq!(voided.voided_qty, filled.last_qty);
        assert_eq!(voided.commission_voided, filled.commission);
        assert_eq!(voided.last_px, filled.last_px);
        assert!(!voided.is_reopened);
        assert_eq!(voided.causation_id, Some(filled.event_id));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(instrument.size_precision()))
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_taker_trade_uses_recorded_applied_quantity() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        trade.size = "714.285714".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("714.285710");
        record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-REST-FAILED",
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-FAILED",
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

        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected message-derived fill correction, was {other:?}"),
        };
        let duplicate =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let dedup_key = trade.id;

        assert!(failed.is_some());
        assert!(duplicate.is_some());
        assert_eq!(voided.venue_order_id, venue_order_id);
        assert_eq!(voided.trade_id, trade_id);
        assert_eq!(voided.voided_qty, applied_qty);
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(state.is_voided_trade(&dedup_key));
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_taker_trade_voids_carried_rest_evidence_without_ws_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        trade.size = "714.285714".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("714.285710");
        let applied_fill = record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-REST-FAILED",
        );
        let later_trade_id = TradeId::from("later-rest-trade");
        let later_venue_order_id = VenueOrderId::from("later-rest-order");
        record_applied_pending_fill(
            &fill_tracker,
            later_trade_id,
            later_venue_order_id,
            Quantity::from("5.000000"),
            "O-LATER-REST-EVIDENCE",
        );

        assert_eq!(
            fill_tracker.rest_applied_pending_fills(&trade_id),
            vec![applied_fill],
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-FAILED",
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

        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected message-derived fill correction, was {other:?}"),
        };
        let duplicate =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let dedup_key = trade.id;

        assert!(failed.is_some());
        assert!(duplicate.is_some());
        assert_eq!(voided.venue_order_id, venue_order_id);
        assert_eq!(voided.trade_id, trade_id);
        assert_eq!(voided.voided_qty, applied_qty);
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(state.is_voided_trade(&dedup_key));
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_identity_incomplete_evidence_void_stays_retryable() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id,
            client_order_id: None,
            strategy_id: None,
            order_type: None,
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("25.000000"),
            last_px: Price::from("0.50"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Taker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, applied_fill.clone())]);
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-IDENTITY-EVICTED",
        );

        for index in 0..10_000 {
            let eviction_venue_order_id = VenueOrderId::from(format!("V-EVICT-{index}").as_str());
            let eviction_client_order_id = format!("O-EVICT-{index}");
            register_identity(
                &order_identities,
                eviction_venue_order_id,
                instrument.id(),
                &eviction_client_order_id,
            );
        }

        assert!(order_identities.get(&venue_order_id).is_none());

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
        let dedup_key = trade.id.clone();

        let deferred =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(deferred.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert_eq!(
            fill_tracker.rest_applied_pending_fills(&trade_id),
            vec![applied_fill.clone()],
        );
        assert!(receiver.try_recv().is_err());

        let redelivered = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(redelivered.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert_eq!(
            fill_tracker.rest_applied_pending_fills(&trade_id),
            vec![applied_fill],
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_taker_trade_uses_full_recorded_fill_fidelity() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        trade.price = "0.75".to_string();
        trade.fee_rate_bps = "250".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id,
            client_order_id: Some(ClientOrderId::from("O-REST-FIDELITY")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("17.000000"),
            last_px: Price::from("0.33"),
            commission: Money::new(1.125, get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, applied_fill.clone())]);
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-FIDELITY",
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

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected fidelity fill correction, was {other:?}"),
        };
        assert_eq!(voided.venue_order_id, venue_order_id);
        assert_eq!(voided.voided_qty, applied_fill.last_qty);
        assert_eq!(voided.last_px, applied_fill.last_px);
        assert_eq!(voided.commission_voided, Some(applied_fill.commission));
        assert_eq!(voided.liquidity_side, applied_fill.liquidity_side);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_corrected_maker_redelivery_emits_only_the_dropped_leg() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let first_order = trade.maker_orders[0].clone();
        let mut second_order = second_owned_maker_order(&trade);
        second_order.matched_amount = Decimal::ZERO;
        trade.maker_orders.push(second_order.clone());
        let first_venue_order_id = VenueOrderId::from(first_order.order_id.as_str());
        let second_venue_order_id = VenueOrderId::from(second_order.order_id.as_str());
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(first_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();

        for venue_order_id in [first_venue_order_id, second_venue_order_id] {
            fill_tracker.register(
                venue_order_id,
                Quantity::from("100.000000"),
                OrderSide::Buy,
                instrument.id(),
                instrument.size_precision(),
                instrument.price_precision(),
            );
        }

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            first_venue_order_id,
            instrument.id(),
            "O-LEG-VALID",
        );
        register_identity(
            &order_identities,
            second_venue_order_id,
            instrument.id(),
            "O-LEG-CORRUPT",
        );
        order_identities.mark_accepted(first_venue_order_id);
        order_identities.mark_accepted(second_venue_order_id);
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
            user_address: &first_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let first_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected first maker leg fill, was {other:?}"),
        };
        assert_eq!(first_fill.venue_order_id, first_venue_order_id);
        assert!(receiver.try_recv().is_err());

        trade.status = PolymarketTradeStatus::Confirmed;
        trade.maker_orders[1].matched_amount = Decimal::from_str("7").unwrap();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let second_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected corrected maker leg fill, was {other:?}"),
        };
        assert_eq!(second_fill.venue_order_id, second_venue_order_id);
        assert_eq!(second_fill.last_qty, Quantity::from("7.000000"));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&first_venue_order_id),
            Some(first_fill.last_qty),
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_all_invalid_confirmed_defers_terminal_normalization_until_corrected() {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let maker_order = trade.maker_orders[0].clone();
        let venue_order_id = VenueOrderId::from(maker_order.order_id.as_str());
        order.id = maker_order.order_id.clone();
        order.status = PolymarketOrderStatus::Matched;
        order.associate_trades = Some(vec![trade.id.clone()]);
        order.original_size = "100".to_string();
        order.size_matched = "99.995".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(maker_order.asset_id, instrument.clone());
        token_instruments.insert(order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-DEFERRED-NORMALIZATION",
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
            user_address: &maker_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        while receiver.try_recv().is_ok() {}

        trade.status = PolymarketTradeStatus::Confirmed;
        trade.maker_orders[0].matched_amount = Decimal::ZERO;

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(receiver.try_recv().is_err());

        trade.maker_orders[0].matched_amount = Decimal::from_str("99.995").unwrap();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected corrected maker fill, was {other:?}"),
        };
        assert_eq!(fill.last_qty, Quantity::from("99.995000"));

        let updated = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Updated(event)) => event,
            other => panic!("expected terminal quantity normalization, was {other:?}"),
        };
        assert_eq!(updated.quantity, Quantity::from("99.995000"));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_maker_trade_voids_union_of_ws_and_rest_orders() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let first_order = trade.maker_orders[0].clone();
        let second_order = second_owned_maker_order(&trade);
        let first_venue_order_id = VenueOrderId::from(first_order.order_id.as_str());
        let second_venue_order_id = VenueOrderId::from(second_order.order_id.as_str());
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(first_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            first_venue_order_id,
            Quantity::from("100.000000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            first_venue_order_id,
            instrument.id(),
            "O-UNION-WS",
        );
        register_identity(
            &order_identities,
            second_venue_order_id,
            instrument.id(),
            "O-UNION-REST",
        );
        order_identities.mark_accepted(first_venue_order_id);
        order_identities.mark_accepted(second_venue_order_id);
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
            user_address: &first_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let ws_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected maker fill, was {other:?}"),
        };
        trade.maker_orders.push(second_order);
        let trade_id = TradeId::from(trade.id.as_str());
        let rest_fill = AppliedPendingFill {
            venue_order_id: second_venue_order_id,
            instrument_id: instrument.id(),
            trade_id: make_composite_trade_id(&trade.id, second_venue_order_id.as_str()),
            client_order_id: Some(ClientOrderId::from("O-UNION-REST")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("11.000000"),
            last_px: Price::from("0.42"),
            commission: Money::new(1.25, get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, rest_fill.clone())]);
        trade.status = PolymarketTradeStatus::Failed;

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let first_void = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected first fill correction, was {other:?}"),
        };
        let second_void = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected second fill correction, was {other:?}"),
        };
        let rest_void = if first_void.venue_order_id == second_venue_order_id {
            first_void
        } else {
            second_void
        };

        assert_eq!(ws_fill.venue_order_id, first_venue_order_id);
        assert_eq!(rest_void.venue_order_id, second_venue_order_id);
        assert_eq!(rest_void.voided_qty, rest_fill.last_qty);
        assert_eq!(rest_void.last_px, rest_fill.last_px);
        assert_eq!(rest_void.commission_voided, Some(rest_fill.commission));
        assert_eq!(rest_void.liquidity_side, rest_fill.liquidity_side);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_union_voids_evidence_order_despite_unknown_payload_asset() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let first_order = trade.maker_orders[0].clone();
        let mut second_order = second_owned_maker_order(&trade);
        second_order.asset_id = Ustr::from("missing-union-asset");
        let first_venue_order_id = VenueOrderId::from(first_order.order_id.as_str());
        let second_venue_order_id = VenueOrderId::from(second_order.order_id.as_str());
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(first_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            first_venue_order_id,
            Quantity::from("100.000000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            first_venue_order_id,
            instrument.id(),
            "O-UNION-DEFER-WS",
        );
        register_identity(
            &order_identities,
            second_venue_order_id,
            instrument.id(),
            "O-UNION-DEFER-REST",
        );
        order_identities.mark_accepted(first_venue_order_id);
        order_identities.mark_accepted(second_venue_order_id);
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
            user_address: &first_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();
        let dedup_key = trade.id.clone();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let ws_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected maker fill, was {other:?}"),
        };
        trade.maker_orders.push(second_order);
        let trade_id = TradeId::from(trade.id.as_str());
        let rest_fill = AppliedPendingFill {
            venue_order_id: second_venue_order_id,
            instrument_id: instrument.id(),
            trade_id: make_composite_trade_id(&trade.id, second_venue_order_id.as_str()),
            client_order_id: Some(ClientOrderId::from("O-UNION-DEFER-REST")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("9.000000"),
            last_px: Price::from("0.41"),
            commission: Money::new(0.75, get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, rest_fill.clone())]);
        trade.status = PolymarketTradeStatus::Failed;

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let first_void = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected first fill correction, was {other:?}"),
        };
        let second_void = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected second fill correction, was {other:?}"),
        };
        let (rest_void, ws_void) = if first_void.venue_order_id == second_venue_order_id {
            (&first_void, &second_void)
        } else {
            (&second_void, &first_void)
        };

        assert_eq!(rest_void.venue_order_id, second_venue_order_id);
        assert_eq!(rest_void.voided_qty, rest_fill.last_qty);
        assert_eq!(rest_void.last_px, rest_fill.last_px);
        assert_eq!(ws_void.venue_order_id, first_venue_order_id);
        assert_eq!(ws_void.voided_qty, ws_fill.last_qty);
        assert_eq!(state.matched_fill_count(&dedup_key), 0);
        assert!(state.is_voided_trade(&dedup_key));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&first_venue_order_id),
            Some(Quantity::zero(instrument.size_precision())),
        );
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_without_evidence_allows_redelivery_after_evidence_appears() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let maker_order = trade.maker_orders[0].clone();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(maker_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(maker_order.order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-MAKER-REST-FAILED",
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
            user_address: &maker_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let dedup_key = trade.id.clone();

        assert!(state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());

        let noted_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id: make_composite_trade_id(&trade.id, venue_order_id.as_str()),
            client_order_id: Some(ClientOrderId::from("O-MAKER-REST-FAILED")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: applied_qty,
            last_px: Price::from("0.50"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        fill_tracker.begin_rest_applied_pending_pass();
        let withheld = fill_tracker.record_rest_applied_pending_fills(&[(trade_id, noted_fill)]);
        assert_eq!(withheld, AHashSet::from_iter([trade_id]));
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );

        let fill_tracker = OrderFillTrackerMap::new();
        record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-MAKER-REST-FAILED",
        );
        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_submits: &pending_submits,
            order_identities: &order_identities,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: &maker_order.maker_address,
            user_api_key: "test-key",
        };

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected redelivered fill correction, was {other:?}"),
        };
        assert_eq!(voided.venue_order_id, venue_order_id);
        assert_eq!(voided.voided_qty, applied_qty);
        assert!(state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_fresh_failed_trade_without_evidence_warns_once_and_is_processed() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.id = "fresh-failed-no-evidence".to_string();
        trade.status = PolymarketTradeStatus::Failed;
        let token_instruments = AtomicMap::new();
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
        let log_start = capture_start();

        let refresh = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(refresh.is_some());
        assert!(
            state
                .processed_fills
                .contains(&"fresh-failed-no-evidence".to_string())
        );
        let matching_logs = records_since(log_start)
            .into_iter()
            .filter(|(_, message)| message.contains("fresh-failed-no-evidence"))
            .collect::<Vec<_>>();
        assert_eq!(
            matching_logs,
            vec![(
                log::Level::Warn,
                "Ignoring failed trade fresh-failed-no-evidence: no fill was applied for this trade, or correction evidence was lost to restart/eviction"
                    .to_string(),
            )],
        );
    }

    #[rstest]
    fn test_failed_trade_with_evicted_rest_evidence_stays_retryable() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let trade_id = TradeId::from(trade.id.as_str());
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let fill_tracker = OrderFillTrackerMap::new();
        let instrument_id = test_instrument().id();
        let mut applied_fills = Vec::with_capacity(10_001);
        applied_fills.push((
            trade_id,
            AppliedPendingFill {
                venue_order_id,
                instrument_id,
                trade_id,
                client_order_id: None,
                strategy_id: None,
                order_type: None,
                order_side: OrderSide::Buy,
                last_qty: Quantity::from("1.000000"),
                last_px: Price::from("0.50"),
                commission: Money::zero(get_pusd_currency()),
                liquidity_side: LiquiditySide::Taker,
            },
        ));

        for index in 0..10_000 {
            let evidence_trade_id = TradeId::from(format!("REST-EVICT-{index}").as_str());
            applied_fills.push((
                evidence_trade_id,
                AppliedPendingFill {
                    venue_order_id: VenueOrderId::from(format!("REST-ORDER-{index}").as_str()),
                    instrument_id,
                    trade_id: evidence_trade_id,
                    client_order_id: None,
                    strategy_id: None,
                    order_type: None,
                    order_side: OrderSide::Buy,
                    last_qty: Quantity::from("1.000000"),
                    last_px: Price::from("0.50"),
                    commission: Money::zero(get_pusd_currency()),
                    liquidity_side: LiquiditySide::Taker,
                },
            ));
        }

        record_applied_pending_fills(&fill_tracker, &applied_fills);
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );

        let token_instruments = AtomicMap::new();
        let pending_submits = PendingSubmitTracker::default();
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
        let dedup_key = trade.id.clone();

        let deferred = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(deferred.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_with_evicted_buffered_evidence_stays_retryable() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let trade_id = TradeId::from(trade.id.as_str());
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let dedup_key = trade.id.clone();
        let target_fill = test_order_filled(venue_order_id, trade_id, "O-BUFFERED-EVICTED");
        let target_correction = FillCorrectionMetadata {
            correction_key: dedup_key.clone(),
            trade_id,
            info: None,
            is_confirmed: false,
            track_fill: true,
        };
        let fill_tracker = OrderFillTrackerMap::new();
        assert!(fill_tracker.emit_buffered_fill(
            target_fill.clone(),
            Some(&target_correction),
            |_, _| {},
        ));

        for index in 0..10_000 {
            let correction_key = format!("BUFFERED-EVICT-{index}");
            let correction = FillCorrectionMetadata {
                correction_key,
                trade_id: TradeId::from(format!("BUFFERED-TRADE-{index}").as_str()),
                info: None,
                is_confirmed: false,
                track_fill: true,
            };
            let fill = test_order_filled(
                VenueOrderId::from(format!("BUFFERED-ORDER-{index}").as_str()),
                correction.trade_id,
                "O-BUFFERED-EVICTION-FILLER",
            );
            assert!(fill_tracker.emit_buffered_fill(fill, Some(&correction), |_, _| {}));
        }

        let token_instruments = AtomicMap::new();
        let pending_submits = PendingSubmitTracker::default();
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

        let deferred =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(deferred.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());
        assert!(fill_tracker.emit_buffered_fill(
            target_fill.clone(),
            Some(&target_correction),
            |_, _| {},
        ));

        let retried = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected retried buffered fill correction, was {other:?}"),
        };

        assert!(retried.is_some());
        assert_eq!(voided.trade_id, target_fill.trade_id);
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_removes_buffered_evidence_before_empty_decision() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let trade_id = TradeId::from(trade.id.as_str());
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let dedup_key = trade.id.clone();
        let correction = FillCorrectionMetadata {
            correction_key: dedup_key.clone(),
            trade_id,
            info: None,
            is_confirmed: false,
            track_fill: true,
        };
        let fill_tracker = Arc::new(OrderFillTrackerMap::new());
        let hook_tracker = Arc::clone(&fill_tracker);
        let buffered_fill_landed = Arc::new(AtomicBool::new(false));
        let hook_buffered_fill_landed = Arc::clone(&buffered_fill_landed);
        VOID_FAILED_AFTER_BUFFERED_EVIDENCE_REMOVAL.with(|hook| {
            *hook.borrow_mut() = Some(Box::new(move || {
                let fill = test_order_filled(venue_order_id, trade_id, "O-RACING-BUFFERED-FILL");
                let landed = hook_tracker.emit_buffered_fill(fill, Some(&correction), |_, _| {});
                hook_buffered_fill_landed.store(landed, Ordering::SeqCst);
            }));
        });

        let token_instruments = AtomicMap::new();
        let pending_submits = PendingSubmitTracker::default();
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

        let refresh = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(refresh.is_some());
        assert!(!buffered_fill_landed.load(Ordering::SeqCst));
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_with_evicted_evidence_survives_rest_passes() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let trade_id = TradeId::from(trade.id.as_str());
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let dedup_key = trade.id.clone();
        let target_fill = test_order_filled(venue_order_id, trade_id, "O-MATCHED-EVICTED");
        let mut state = WsDispatchState::default();
        state.restore_matched_trade(dedup_key.clone(), vec![target_fill]);

        for index in 0..10_000 {
            let eviction_key = format!("MATCHED-EVICT-{index}");
            let fill = test_order_filled(
                VenueOrderId::from(format!("MATCHED-ORDER-{index}").as_str()),
                TradeId::from(format!("MATCHED-TRADE-{index}").as_str()),
                "O-MATCHED-EVICTION-FILLER",
            );
            state.restore_matched_trade(eviction_key, vec![fill]);
        }

        assert_eq!(state.matched_fill_count(&dedup_key), 0);

        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
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

        let deferred =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(deferred.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(!state.is_voided_trade(&dedup_key));
        assert!(receiver.try_recv().is_err());

        let still_deferred =
            dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(still_deferred.is_none());
        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(receiver.try_recv().is_err());

        for _ in 0..5 {
            fill_tracker.begin_rest_applied_pending_pass();
            let withheld = fill_tracker.record_rest_applied_pending_fills(&[]);

            assert!(withheld.is_empty());

            let retried =
                dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

            assert!(retried.is_none());
            assert!(!state.processed_fills.contains(&dedup_key));
            assert!(!state.is_voided_trade(&dedup_key));
            assert!(receiver.try_recv().is_err());
        }
    }

    #[rstest]
    fn test_evidence_void_carries_the_composite_trade_id_of_the_applied_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let maker_order = trade.maker_orders[0].clone();
        let venue_order_id = VenueOrderId::from(maker_order.order_id.as_str());
        let raw_trade_id = TradeId::from(trade.id.as_str());
        let composite_trade_id = make_composite_trade_id(&trade.id, maker_order.order_id.as_str());

        assert_ne!(composite_trade_id, raw_trade_id);

        let applied_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id: composite_trade_id,
            client_order_id: Some(ClientOrderId::from("O-COMPOSITE-VOID")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("12.000000"),
            last_px: Price::from("0.40"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        record_applied_pending_fills(&fill_tracker, &[(raw_trade_id, applied_fill)]);
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-COMPOSITE-VOID",
        );
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
            user_address: &maker_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected evidence-derived fill correction, was {other:?}"),
        };

        assert_eq!(voided.trade_id, composite_trade_id);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_evidence_void_uses_recorded_identity_after_identity_eviction() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let client_order_id = ClientOrderId::from("O-EVICTED-IDENTITY");
        let strategy_id = StrategyId::from("S-EVICTED-IDENTITY");
        let order_type = OrderType::Market;
        let applied_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id,
            client_order_id: Some(client_order_id),
            strategy_id: Some(strategy_id),
            order_type: Some(order_type),
            order_side: OrderSide::Buy,
            last_qty: Quantity::from("25.000000"),
            last_px: Price::from("0.40"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Taker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, applied_fill)]);
        let pending_submits = PendingSubmitTracker::default();
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

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected evidence-derived fill correction, was {other:?}"),
        };

        assert_eq!(voided.client_order_id, client_order_id);
        assert_eq!(voided.strategy_id, strategy_id);
        assert_eq!(voided.order_type, order_type);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_with_corrupt_taker_order_id_voids_ws_held_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-CORRUPT-TAKER-KEY",
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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let ws_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected taker fill, was {other:?}"),
        };

        trade.status = PolymarketTradeStatus::Failed;
        trade.taker_order_id = "0xCORRUPT".to_string();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected fill correction, was {other:?}"),
        };
        assert_eq!(voided.venue_order_id, venue_order_id);
        assert_eq!(voided.voided_qty, ws_fill.last_qty);
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(instrument.size_precision())),
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_with_evidence_voids_without_payload_instrument() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        let applied_fill = record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-REST-REPLAY",
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-REPLAY",
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
        let dedup_key = trade.id.clone();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected evidence-derived fill correction, was {other:?}"),
        };
        assert_eq!(voided.voided_qty, applied_qty);
        assert_eq!(voided.last_px, applied_fill.last_px);
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(state.is_voided_trade(&dedup_key));
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_failed_trade_with_ws_and_evidence_for_same_order_voids_evidence_values() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-WS-VS-EVIDENCE",
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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let ws_fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected taker fill, was {other:?}"),
        };

        let evidence_fill = AppliedPendingFill {
            venue_order_id,
            instrument_id: instrument.id(),
            trade_id,
            client_order_id: Some(ClientOrderId::from("O-WS-VS-EVIDENCE")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: ws_fill.last_qty,
            last_px: Price::from("0.40"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Taker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, evidence_fill.clone())]);
        trade.status = PolymarketTradeStatus::Failed;

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected evidence-derived fill correction, was {other:?}"),
        };
        assert_eq!(voided.last_px, evidence_fill.last_px);
        assert_eq!(voided.voided_qty, evidence_fill.last_qty);
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(instrument.size_precision())),
        );
    }

    #[rstest]
    fn test_invalid_matched_then_valid_confirmed_emits_fill_once() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let valid_size = trade.size.clone();
        trade.size = "0".to_string();
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
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
            "O-INVALID-THEN-VALID",
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
        let dedup_key = trade.id.clone();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(!state.processed_fills.contains(&dedup_key));
        assert!(receiver.try_recv().is_err());

        trade.status = PolymarketTradeStatus::Confirmed;
        trade.size = valid_size;
        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fill = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected fill after corrected redelivery, was {other:?}"),
        };
        assert_eq!(fill.venue_order_id, venue_order_id);
        assert!(state.processed_fills.contains(&dedup_key));
    }

    #[rstest]
    fn test_confirmed_trade_with_rest_evidence_does_not_reemit_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Confirmed;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        fill_tracker.restore_order(
            venue_order_id,
            Quantity::from("100.000000"),
            applied_qty,
            OrderSide::Buy,
        );
        record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-REST-CONFIRMED",
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-CONFIRMED",
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
        let dedup_key = trade.id.clone();

        let refresh = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(refresh.is_some());
        assert!(state.processed_fills.contains(&dedup_key));
        assert!(fill_tracker.is_trade_confirmed(&dedup_key));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(applied_qty),
        );
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_confirmed_maker_trade_suppresses_only_evidence_order_without_matched() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Confirmed;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let first_order = trade.maker_orders[0].clone();
        let second_order = second_owned_maker_order(&trade);
        trade.maker_orders.push(second_order.clone());
        let first_venue_order_id = VenueOrderId::from(first_order.order_id.as_str());
        let second_venue_order_id = VenueOrderId::from(second_order.order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(first_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.restore_order(
            first_venue_order_id,
            Quantity::from("100.000000"),
            applied_qty,
            OrderSide::Buy,
        );
        fill_tracker.register(
            second_venue_order_id,
            Quantity::from("100.000000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let applied_fill = AppliedPendingFill {
            venue_order_id: first_venue_order_id,
            instrument_id: instrument.id(),
            trade_id: make_composite_trade_id(&trade.id, first_venue_order_id.as_str()),
            client_order_id: Some(ClientOrderId::from("O-CONFIRMED-EVIDENCE")),
            strategy_id: Some(StrategyId::from("S-001")),
            order_type: Some(OrderType::Limit),
            order_side: OrderSide::Buy,
            last_qty: applied_qty,
            last_px: Price::from("0.50"),
            commission: Money::zero(get_pusd_currency()),
            liquidity_side: LiquiditySide::Maker,
        };
        record_applied_pending_fills(&fill_tracker, &[(trade_id, applied_fill)]);
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            first_venue_order_id,
            instrument.id(),
            "O-CONFIRMED-EVIDENCE",
        );
        register_identity(
            &order_identities,
            second_venue_order_id,
            instrument.id(),
            "O-CONFIRMED-UNCOVERED",
        );
        order_identities.mark_accepted(first_venue_order_id);
        order_identities.mark_accepted(second_venue_order_id);
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
            user_address: &first_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected uncovered confirmed maker fill, was {other:?}"),
        };
        assert_eq!(filled.venue_order_id, second_venue_order_id);
        assert_eq!(
            fill_tracker.get_cumulative_filled(&first_venue_order_id),
            Some(applied_qty),
        );
        assert_eq!(
            fill_tracker.get_cumulative_filled(&second_venue_order_id),
            Some(applied_qty),
        );
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_confirmed_rest_evidence_suppresses_buffered_ws_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Matched;
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-REST-BUFFERED",
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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 1);
        assert!(receiver.try_recv().is_err());

        record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            venue_order_id,
            applied_qty,
            "O-REST-BUFFERED",
        );
        trade.status = crate::common::enums::PolymarketTradeStatus::Confirmed;
        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let buffered = fill_tracker.register_and_take_pending_fills(
            venue_order_id,
            Some(ClientOrderId::from("O-REST-BUFFERED")),
            Quantity::from("100.000000"),
            OrderSide::Buy,
        );
        let identity = order_identities.get(&venue_order_id).unwrap();
        emit_buffered_order_filled(&identity, &buffered[0], &ctx);

        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(6)),
        );
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_confirmed_rest_evidence_suppresses_only_applied_maker_fill() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let first_order = trade.maker_orders[0].clone();
        let mut second_order = first_order.clone();
        second_order.order_id =
            "0xmaker02maker02maker02maker02maker02maker02maker02maker02maker02maker02".to_string();
        trade.maker_orders.push(second_order.clone());
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(first_order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let first_venue_order_id = VenueOrderId::from(first_order.order_id.as_str());
        let second_venue_order_id = VenueOrderId::from(second_order.order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let applied_qty = Quantity::from("25.000000");
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            first_venue_order_id,
            instrument.id(),
            "O-REST-MAKER-1",
        );
        register_identity(
            &order_identities,
            second_venue_order_id,
            instrument.id(),
            "O-REST-MAKER-2",
        );
        order_identities.mark_accepted(first_venue_order_id);
        order_identities.mark_accepted(second_venue_order_id);
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
            user_address: &first_order.maker_address,
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        assert_eq!(
            fill_tracker.pending_fills_for(&first_venue_order_id).len(),
            1,
        );
        assert_eq!(
            fill_tracker.pending_fills_for(&second_venue_order_id).len(),
            1,
        );

        record_applied_pending_fill(
            &fill_tracker,
            trade_id,
            first_venue_order_id,
            applied_qty,
            "O-REST-MAKER-1",
        );
        trade.status = crate::common::enums::PolymarketTradeStatus::Confirmed;
        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let first_buffered = fill_tracker.register_and_take_pending_fills(
            first_venue_order_id,
            Some(ClientOrderId::from("O-REST-MAKER-1")),
            Quantity::from("100.000000"),
            OrderSide::Buy,
        );
        let first_identity = order_identities.get(&first_venue_order_id).unwrap();
        emit_buffered_order_filled(&first_identity, &first_buffered[0], &ctx);
        let second_buffered = fill_tracker.register_and_take_pending_fills(
            second_venue_order_id,
            Some(ClientOrderId::from("O-REST-MAKER-2")),
            Quantity::from("100.000000"),
            OrderSide::Buy,
        );
        let second_identity = order_identities.get(&second_venue_order_id).unwrap();
        emit_buffered_order_filled(&second_identity, &second_buffered[0], &ctx);

        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected uncovered maker fill, was {other:?}"),
        };

        assert_eq!(filled.venue_order_id, second_venue_order_id);
        assert_eq!(
            fill_tracker.get_cumulative_filled(&first_venue_order_id),
            Some(Quantity::zero(6)),
        );
        assert_eq!(
            fill_tracker.get_cumulative_filled(&second_venue_order_id),
            Some(applied_qty),
        );
        assert!(receiver.try_recv().is_err());
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
    fn test_identity_evicted_raw_fill_stays_retryable_when_trade_fails() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let market: GammaMarket = load("gamma_market_sports_market_money_line.json");
        let defs = parse_gamma_market(&market).unwrap();
        let instrument =
            create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
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
            "O-LATE-FILL",
        );
        assert!(order_identities.get(&venue_order_id).is_some());

        for index in 0..10_000 {
            let eviction_venue_order_id = VenueOrderId::from(format!("V-EVICT-{index}").as_str());
            let eviction_client_order_id = format!("O-EVICT-{index}");
            register_identity(
                &order_identities,
                eviction_venue_order_id,
                instrument.id(),
                &eviction_client_order_id,
            );
        }
        assert!(order_identities.get(&venue_order_id).is_none());

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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected late fill report");
        let ExecutionEvent::Report(ExecutionReport::Fill(report)) = event else {
            panic!("expected fill report for evicted identity, was {event:?}");
        };

        assert_eq!(report.venue_order_id, venue_order_id);
        assert_eq!(report.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(report.instrument_id, instrument.id());
        assert_eq!(
            report.last_qty.as_decimal(),
            Decimal::from_str_exact(&trade.size).unwrap()
        );
        assert_eq!(
            report.last_px.as_decimal(),
            Decimal::from_str_exact(&trade.price).unwrap()
        );
        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(
            report.commission.as_decimal(),
            Decimal::from_str_exact("0.1875").unwrap()
        );
        assert_eq!(report.commission.currency, Currency::pUSD());
        assert!(receiver.try_recv().is_err());
        assert!(!state.processed_fills.contains(&trade.id));

        trade.status = PolymarketTradeStatus::Failed;
        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(failed.is_none());
        assert!(!state.processed_fills.contains(&trade.id));
        assert!(!state.is_voided_trade(&trade.id));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(report.last_qty),
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_identity_eviction_defers_failed_trade_without_fill_evidence() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.id = "failed-after-identity-eviction".to_string();
        trade.status = PolymarketTradeStatus::Failed;
        let instrument = test_instrument();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            VenueOrderId::from("V-IDENTITY-EVICTED"),
            instrument.id(),
            "O-IDENTITY-EVICTED",
        );

        for index in 0..10_000 {
            register_identity(
                &order_identities,
                VenueOrderId::from(format!("V-IDENTITY-FILLER-{index}").as_str()),
                instrument.id(),
                format!("O-IDENTITY-FILLER-{index}").as_str(),
            );
        }

        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
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

        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(failed.is_none());
        assert!(!state.processed_fills.contains(&trade.id));
        assert!(!state.is_voided_trade(&trade.id));
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
        fill_tracker.record_fill(&venue_order_id, Quantity::new(50.0, 6));

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
    fn test_dispatch_order_matched_normalizes_quantity_without_fill() {
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
        fill_tracker.record_fill(&venue_order_id, Quantity::new(99.995, 6));

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
        add_to_fifo_with_eviction_warn(
            &mut state.confirmed_trades,
            "trade-0xfill1".to_string(),
            "WS confirmed-trade",
        );

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected quantity update");
        match event {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(
                    updated.ts_event,
                    UnixNanos::from(1_703_875_201_000_000_000u64)
                );
                assert_eq!(updated.ts_init, UnixNanos::from(2_000_000_000u64));
                assert_eq!(updated.quantity, Quantity::new(99.995, 6));
                assert!(updated.reconciliation);
            }
            other => panic!("expected updated event, was {other:?}"),
        }
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_confirmed_trade_normalizes_pending_matched_quantity() {
        let mut order: PolymarketUserOrder = load("ws_user_order_matched.json");
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();
        order.associate_trades = Some(vec![trade.id.clone()]);
        trade.size = "99.995".to_string();
        trade.price = order.price.clone();

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
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            "O-CONFIRMED-DUST",
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

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);
        assert!(receiver.try_recv().is_err());

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let real_fill = receiver.try_recv().expect("expected confirmed venue fill");
        let normalized = receiver
            .try_recv()
            .expect("expected quantity normalization");

        match (real_fill, normalized) {
            (
                ExecutionEvent::Order(OrderEventAny::Filled(real)),
                ExecutionEvent::Order(OrderEventAny::Updated(updated)),
            ) => {
                assert_eq!(real.last_qty, Quantity::from("99.995"));
                assert_eq!(updated.quantity, Quantity::from("99.995"));
                assert!(updated.reconciliation);
            }
            other => panic!("expected fill then quantity update, was {other:?}"),
        }
        assert!(receiver.try_recv().is_err());
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

    // A trade landing before the submit response buffers its fill, so the order update that
    // registers the order must emit that fill before its own terminal status
    #[rstest]
    #[case(PolymarketOrderStatus::Canceled, "Canceled", OrderStatus::Canceled)]
    #[case(
        PolymarketOrderStatus::CanceledMarketResolved,
        "Expired",
        OrderStatus::Expired
    )]
    fn test_buffered_fill_emitted_before_terminal_status(
        #[case] status: PolymarketOrderStatus,
        #[case] expected_terminal: &str,
        #[case] expected_order_status: OrderStatus,
    ) {
        let mut terminal_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        terminal_order.status = status;
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(terminal_order.asset_id, instrument.clone());

        // No registration: the submit response has not landed
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(terminal_order.id.as_str());
        let client_order_id = ClientOrderId::from("O-BUFFERED");

        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, client_order_id);
        let order_identities = OrderIdentityRegistry::default();
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
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

        // The trade arrives first and buffers its fill
        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        assert!(
            receiver.try_recv().is_err(),
            "a buffered fill must emit no event before the order is registered",
        );

        // The order update registers the order and drains the buffered fill
        dispatch_user_message(&UserWsMessage::Order(terminal_order), &ctx, &mut state);

        let mut emitted = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            match event {
                ExecutionEvent::Order(order_event) => emitted.push(order_event),
                other => panic!("expected only order events, was {other:?}"),
            }
        }

        assert_eq!(emitted.len(), 3, "emitted sequence was {emitted:?}");
        match &emitted[0] {
            OrderEventAny::Accepted(accepted) => {
                assert_eq!(accepted.client_order_id, client_order_id);
                assert_eq!(accepted.venue_order_id, venue_order_id);
            }
            other => panic!("expected accepted event first, was {other:?}"),
        }

        match &emitted[1] {
            OrderEventAny::Filled(filled) => {
                assert_eq!(filled.client_order_id, client_order_id);
                assert_eq!(filled.venue_order_id, venue_order_id);
                assert_eq!(filled.last_qty.as_decimal(), dec!(25));
            }
            other => panic!("expected filled event before the terminal status, was {other:?}"),
        }
        let terminal = match &emitted[2] {
            OrderEventAny::Canceled(canceled) => {
                assert_eq!(canceled.client_order_id, client_order_id);
                assert_eq!(canceled.venue_order_id, Some(venue_order_id));
                "Canceled"
            }
            OrderEventAny::Expired(expired) => {
                assert_eq!(expired.client_order_id, client_order_id);
                assert_eq!(expired.venue_order_id, Some(venue_order_id));
                "Expired"
            }
            other => panic!("expected a terminal order event last, was {other:?}"),
        };
        assert_eq!(terminal, expected_terminal);

        // The engine's state machine is what proves the order actually closes
        let mut order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .strategy_id(StrategyId::from("S-001"))
            .side(OrderSide::Buy)
            .price(Price::from("0.5"))
            .quantity(Quantity::from("100"))
            .build();

        for event in emitted {
            order.apply(event).expect("emitted sequence must be valid");
        }

        assert_eq!(order.status(), expected_order_status);
        assert_eq!(order.filled_qty().as_decimal(), dec!(25));
    }

    #[rstest]
    fn test_identity_absent_buffered_raw_fill_becomes_retryable() {
        let order: PolymarketUserOrder = load("ws_user_order_matched.json");
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let instrument = test_instrument();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, ClientOrderId::from("O-BUFFERED-RAW"));
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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        assert!(receiver.try_recv().is_err());

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        match receiver
            .try_recv()
            .expect("expected buffered raw fill report")
        {
            ExecutionEvent::Report(ExecutionReport::Fill(report)) => {
                assert_eq!(report.venue_order_id, venue_order_id);
                assert_eq!(report.trade_id, TradeId::from(trade.id.as_str()));
            }
            other => panic!("expected buffered raw fill report, was {other:?}"),
        }

        match receiver.try_recv().expect("expected matched order report") {
            ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                assert_eq!(report.venue_order_id, venue_order_id);
            }
            other => panic!("expected matched order report, was {other:?}"),
        }
        assert!(!state.processed_fills.contains(&trade.id));

        trade.status = PolymarketTradeStatus::Failed;
        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(failed.is_none());
        assert!(!state.processed_fills.contains(&trade.id));
        assert!(receiver.try_recv().is_err());
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
            status: PolymarketTradeStatus::Confirmed,
            taker_order_id: "0xtaker01".to_string(),
            timestamp: ts.to_string(),
            trade_owner: Ustr::from("other-owner"),
            transaction_hash: None,
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
            status: PolymarketTradeStatus::Confirmed,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            transaction_hash: None,
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
        assert_eq!(cumulative, submitted);

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
    #[case(
        TimeInForce::Ioc,
        OrderType::Market,
        OrderSide::Buy,
        "5.202910",
        "5.202897",
        false,
        true
    )]
    #[case(
        TimeInForce::Fok,
        OrderType::Limit,
        OrderSide::Buy,
        "5.202910",
        "5.202897",
        true,
        false
    )]
    #[case(
        TimeInForce::Ioc,
        OrderType::Limit,
        OrderSide::Buy,
        "30",
        "20",
        false,
        true
    )]
    #[case(
        TimeInForce::Ioc,
        OrderType::Market,
        OrderSide::Sell,
        "5.202910",
        "5.202897",
        false,
        true
    )]
    #[case(
        TimeInForce::Gtc,
        OrderType::Limit,
        OrderSide::Buy,
        "5.202910",
        "5.202897",
        false,
        false
    )]
    fn test_taker_terminal_status_on_trade_confirm(
        #[case] time_in_force: TimeInForce,
        #[case] order_type: OrderType,
        #[case] order_side: OrderSide,
        #[case] submitted_qty: &str,
        #[case] fill_qty: &str,
        #[case] expect_normalization: bool,
        #[case] expect_cancel: bool,
    ) {
        // Takers receive no MATCHED order update. FOK is atomic, so a dust
        // difference normalizes the registered quantity. IOC maps to FAK, so
        // a positive remainder closes as Canceled without changing the fill.
        use crate::common::enums::{
            PolymarketEventType, PolymarketOrderSide, PolymarketOutcome, PolymarketTradeStatus,
        };

        let instrument = test_instrument();
        let asset_id = instrument.id().symbol.inner();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from("0xtaker-one-shot-dust");
        let submitted = Quantity::from_decimal_dp(
            Decimal::from_str_exact(submitted_qty).unwrap(),
            instrument.size_precision(),
        )
        .unwrap();
        fill_tracker.register(
            venue_order_id,
            submitted,
            order_side,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        order_identities.register_order_identity(
            venue_order_id,
            OrderIdentity {
                client_order_id: ClientOrderId::from("O-ONE-SHOT"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: instrument.id(),
                order_side,
                order_type,
                time_in_force,
            },
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
            id: "trade-one-shot-dust".to_string(),
            last_update: "1700000001".to_string(),
            maker_address: Ustr::from("0xmaker"),
            maker_orders: vec![],
            market: Ustr::from("0xmarket"),
            match_time: "1700000000".to_string(),
            outcome: PolymarketOutcome::yes(),
            owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            price: "0.963".to_string(),
            side: if order_side == OrderSide::Buy {
                PolymarketOrderSide::Buy
            } else {
                PolymarketOrderSide::Sell
            },
            size: fill_qty.to_string(),
            status: PolymarketTradeStatus::Confirmed,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            transaction_hash: None,
            trader_side: PolymarketLiquiditySide::Taker,
            event_type: PolymarketEventType::Trade,
        };

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected the venue fill event");
        match event {
            ExecutionEvent::Order(OrderEventAny::Filled(filled)) => {
                assert_eq!(
                    filled.last_qty,
                    Quantity::from_decimal_dp(
                        Decimal::from_str_exact(fill_qty).unwrap(),
                        instrument.size_precision(),
                    )
                    .unwrap(),
                );
            }
            other => panic!("expected filled event, was {other:?}"),
        }

        if expect_normalization {
            let event = receiver.try_recv().expect("expected quantity update");
            match event {
                ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                    assert_eq!(
                        updated.quantity,
                        Quantity::new(5.202897, instrument.size_precision()),
                    );
                    assert_eq!(updated.venue_order_id, Some(venue_order_id));
                    assert!(updated.reconciliation);
                }
                other => panic!("expected updated event, was {other:?}"),
            }
            assert!(
                fill_tracker
                    .get_cumulative_filled(&venue_order_id)
                    .is_none(),
                "order must be settled and removed from the tracker",
            );
        } else if expect_cancel {
            let event = receiver.try_recv().expect("expected IOC cancellation");
            match event {
                ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) => {
                    assert_eq!(canceled.venue_order_id, Some(venue_order_id));
                }
                other => panic!("expected canceled event, was {other:?}"),
            }
            assert!(
                fill_tracker
                    .get_cumulative_filled(&venue_order_id)
                    .is_none(),
                "canceled IOC must be settled and removed from the tracker",
            );
        } else {
            assert!(
                receiver.try_recv().is_err(),
                "resting order must not receive a terminal event",
            );
            assert!(
                fill_tracker
                    .get_cumulative_filled(&venue_order_id)
                    .is_some(),
                "ineligible order must stay tracked with open leaves",
            );
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
            status: PolymarketTradeStatus::Confirmed,
            taker_order_id: venue_order_id.as_str().to_string(),
            timestamp: "1700000000000".to_string(),
            trade_owner: Ustr::from("00000000-0000-0000-0000-000000000001"),
            transaction_hash: None,
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

    #[rstest]
    fn test_replayed_fill_after_ws_dedup_eviction_does_not_bump_quantity() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.size = "60.000000".to_string();
        let instrument = test_instrument();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.000000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_identities = OrderIdentityRegistry::default();
        let client_order_id = ClientOrderId::from("O-REPLAY-EVICTED");
        register_identity(
            &order_identities,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
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

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let accepted = match receiver.try_recv().expect("expected accepted event") {
            ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => accepted,
            other => panic!("expected accepted event, was {other:?}"),
        };
        let first_fill = match receiver.try_recv().expect("expected first fill") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => fill,
            other => panic!("expected first fill, was {other:?}"),
        };
        assert_eq!(first_fill.last_qty, Quantity::from("60.000000"));
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::from("60.000000")),
        );

        let mut engine_order = OrderTestBuilder::new(OrderType::Limit)
            .instrument_id(instrument.id())
            .client_order_id(client_order_id)
            .strategy_id(StrategyId::from("S-001"))
            .side(OrderSide::Buy)
            .price(Price::from("0.5"))
            .quantity(Quantity::from("100.000000"))
            .build();
        engine_order
            .apply(OrderEventAny::Accepted(accepted))
            .expect("accepted event must apply");
        engine_order
            .apply(OrderEventAny::Filled(first_fill.clone()))
            .expect("first fill must apply");

        for index in 0..10_000 {
            let key = format!("newer-trade-{index}");
            let filler_venue_order_id = VenueOrderId::from(format!("newer-order-{index}").as_str());
            state.restore_matched_trade(
                key.clone(),
                vec![test_order_filled(
                    filler_venue_order_id,
                    TradeId::from(key.as_str()),
                    "O-REPLAY-FILLER",
                )],
            );
            add_to_fifo_map_with_eviction_warn(
                &mut state.consumed_legs,
                key,
                AHashSet::from_iter([filler_venue_order_id]),
                "WS consumed-leg",
            );
        }

        assert!(!state.processed_fills.contains(&trade.id));
        assert_eq!(state.matched_fill_count(&trade.id), 0);
        assert!(!state.consumed_legs.contains_key(&trade.id));

        trade.status = PolymarketTradeStatus::Confirmed;
        let confirmed = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(confirmed.is_some());
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::from("60.000000")),
        );
        assert_eq!(
            fill_tracker.submitted_qty(&venue_order_id),
            Some(Quantity::from("100.000000")),
        );
        let replayed_fill = match receiver.try_recv().expect("expected replayed fill") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => fill,
            other => panic!("expected replayed fill without quantity update, was {other:?}"),
        };
        assert_eq!(replayed_fill.trade_id, first_fill.trade_id);
        let duplicate = engine_order.apply(OrderEventAny::Filled(replayed_fill));
        assert!(matches!(
            duplicate,
            Err(OrderError::DuplicateFill(trade_id)) if trade_id == first_fill.trade_id
        ));
        assert_eq!(engine_order.quantity(), Quantity::from("100.000000"));
        assert_eq!(engine_order.filled_qty(), Quantity::from("60.000000"));
        assert!(receiver.try_recv().is_err());
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
