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
//! `OrderCanceled` / `OrderRejected` / `OrderExpired`), building them from the context captured at
//! submit (`OrderContextRegistry`). Order-channel messages drive lifecycle events; trade-channel
//! messages drive fills, and acceptance is synthesized before a fill or cancel that races ahead.
//! Messages are emitted once the order is known (accepted, or with a submit in flight), otherwise
//! buffered until acceptance. Reports are reserved for the `generate_*` query and reconciliation
//! methods.
//!
//! Trade evidence is admitted through the shared settlement boundary and governed by the
//! settlement registry: provisional and confirmed stream statuses apply legs once per
//! uninterrupted session, failed or conflicting stream evidence quarantines the trade for
//! targeted terminal REST resolution, and only a REST-established `FAILED` outcome voids
//! applied fills or tombstones absent legs.

use std::fmt::Debug;

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use indexmap::IndexMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{
    UUID4, UnixNanos, collections::AtomicMap, string::secret::REDACTED, time::AtomicTime,
};
use nautilus_live::{ExecutionEventEmitter, execution::context::OrderContext};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::{
        OrderAccepted, OrderCanceled, OrderEventAny, OrderExpired, OrderFillVoided, OrderFilled,
        OrderRejected, OrderUpdated,
    },
    identifiers::{AccountId, ClientOrderId, InstrumentId, TradeId, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{FillReport, OrderStatusReport},
    types::{Price, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    messages::{
        PolymarketUserOrder, PolymarketUserOrderStatus, PolymarketUserTrade, UserWsMessage,
    },
    parse::parse_timestamp_ms,
};
use crate::{
    common::{
        enums::{
            PolymarketOrderSide, PolymarketOrderStatus, PolymarketOrderType, PolymarketSignerType,
            PolymarketTradeStatus,
        },
        parse::parse_decimal_exact,
    },
    execution::{
        context::OrderContextRegistry,
        get_pusd_currency, is_post_only_crossing,
        order_fill_tracker::{BufferedFill, FillCorrectionMetadata, OrderFillTrackerMap},
        parse::parse_order_status_report,
        pending::PendingSubmitTracker,
        reconciliation::admit_selected_trade,
        settlement::{
            AdmissionContext, AdmissionError, AdmittedLeg, AdmittedTrade, SettlementAction,
            SettlementRegistry, TradeEvidence, admit_trade_evidence,
        },
    },
    http::{
        error::sanitize_error_text,
        models::{PolymarketOpenOrder, PolymarketTradeReport},
    },
};

/// Signal returned when a finalized trade requires an async account refresh.
#[derive(Debug)]
pub(crate) struct AccountRefreshRequest;

/// Mutable state retained across user WebSocket stream generations.
///
/// Terminal cancel reports are re-emitted after fills to restore terminal state when fills race
/// ahead of or arrive after cancel messages.
#[derive(Debug, Default)]
pub(crate) struct WsDispatchState {
    pub reconciled_fills: FifoCache<(TradeId, VenueOrderId), 10_000>,

    pending_terminal_orders: FifoCacheMap<VenueOrderId, PendingTerminalOrder, 10_000>,
    terminal_cancel_reports: FifoCacheMap<VenueOrderId, OrderStatusReport, 10_000>,

    pending_commands: AHashMap<ClientOrderId, PendingCommand>,
    inflight_cancel_markets: AHashSet<InstrumentId>,
    replaced_venue_order_ids: FifoCache<VenueOrderId, 10_000>,
    closed_modify_venue_order_ids: FifoCacheMap<VenueOrderId, UnixNanos, 10_000>,
}

impl WsDispatchState {
    pub(crate) fn begin_modify(
        &mut self,
        client_order_id: ClientOrderId,
        old_venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
    ) -> bool {
        if self.pending_commands.contains_key(&client_order_id)
            || self.replaced_venue_order_ids.contains(&old_venue_order_id)
            || self.inflight_cancel_markets.contains(&instrument_id)
        {
            return false;
        }

        self.pending_commands.insert(
            client_order_id,
            PendingCommand::Modify {
                old_venue_order_id,
                instrument_id,
                cancel_ts: None,
                replacement: None,
            },
        );
        true
    }

    pub(crate) fn is_modifying(&self, client_order_id: &ClientOrderId) -> bool {
        matches!(
            self.pending_commands.get(client_order_id),
            Some(PendingCommand::Modify { .. })
        )
    }

    pub(crate) fn confirm_modify_cancel(
        &mut self,
        client_order_id: ClientOrderId,
        expected_old_venue_order_id: VenueOrderId,
        ts_event: UnixNanos,
    ) -> bool {
        let Some(PendingCommand::Modify {
            old_venue_order_id,
            cancel_ts,
            ..
        }) = self.pending_commands.get_mut(&client_order_id)
        else {
            return false;
        };

        if *old_venue_order_id != expected_old_venue_order_id {
            return false;
        }

        cancel_ts.get_or_insert(ts_event);
        true
    }

    pub(crate) fn set_modify_replacement(
        &mut self,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        quantity: Quantity,
        leg_quantity: Quantity,
        price: Price,
    ) -> bool {
        let Some(PendingCommand::Modify { replacement, .. }) =
            self.pending_commands.get_mut(&client_order_id)
        else {
            return false;
        };

        *replacement = Some(PendingModifyReplacement {
            venue_order_id,
            quantity,
            leg_quantity,
            price,
        });
        true
    }

    pub(crate) fn claim_modify_replacement(
        &mut self,
        venue_order_id: VenueOrderId,
    ) -> Option<ModifyPromotion> {
        let promotion = self.pending_modify_promotion(venue_order_id)?;
        self.pending_commands.remove(&promotion.client_order_id)?;
        self.replaced_venue_order_ids
            .add(promotion.old_venue_order_id);
        Some(promotion)
    }

    pub(crate) fn finish_modify_without_replacement(
        &mut self,
        client_order_id: ClientOrderId,
        expected_old_venue_order_id: VenueOrderId,
        cancellation_proven: bool,
        ts_event: UnixNanos,
    ) -> Option<(VenueOrderId, Option<UnixNanos>)> {
        let Some(PendingCommand::Modify {
            old_venue_order_id, ..
        }) = self.pending_commands.get(&client_order_id)
        else {
            return None;
        };

        if *old_venue_order_id != expected_old_venue_order_id {
            return None;
        }

        let PendingCommand::Modify {
            old_venue_order_id,
            cancel_ts,
            ..
        } = self.pending_commands.remove(&client_order_id)?
        else {
            return None;
        };

        let cancel_ts = self
            .terminal_cancel_reports
            .get(&old_venue_order_id)
            .map(|report| report.ts_last)
            .or(cancel_ts)
            .or(cancellation_proven.then_some(ts_event));

        if let Some(cancel_ts) = cancel_ts {
            self.closed_modify_venue_order_ids
                .insert(old_venue_order_id, cancel_ts);
        }

        Some((old_venue_order_id, cancel_ts))
    }

    pub(crate) fn finish_unsubmitted_modifies(
        &mut self,
    ) -> Vec<(ClientOrderId, VenueOrderId, Option<UnixNanos>)> {
        let terminal_cancel_reports = &self.terminal_cancel_reports;
        let closed_modify_venue_order_ids = &mut self.closed_modify_venue_order_ids;
        let mut finished = Vec::new();

        self.pending_commands
            .retain(|client_order_id, command| match command {
                PendingCommand::Modify {
                    old_venue_order_id,
                    cancel_ts,
                    replacement: None,
                    ..
                } => {
                    let cancel_ts = terminal_cancel_reports
                        .get(old_venue_order_id)
                        .map(|report| report.ts_last)
                        .or(*cancel_ts);
                    if let Some(cancel_ts) = cancel_ts {
                        closed_modify_venue_order_ids.insert(*old_venue_order_id, cancel_ts);
                    }

                    finished.push((*client_order_id, *old_venue_order_id, cancel_ts));
                    false
                }
                _ => true,
            });

        finished
    }

    pub(crate) fn pending_modify_promotion(
        &self,
        venue_order_id: VenueOrderId,
    ) -> Option<ModifyPromotion> {
        self.pending_commands
            .iter()
            .find_map(|(client_order_id, command)| match command {
                PendingCommand::Modify {
                    old_venue_order_id,
                    replacement: Some(replacement),
                    ..
                } if replacement.venue_order_id == venue_order_id => Some(ModifyPromotion {
                    client_order_id: *client_order_id,
                    old_venue_order_id: *old_venue_order_id,
                    venue_order_id,
                    quantity: replacement.quantity,
                    leg_quantity: replacement.leg_quantity,
                    price: replacement.price,
                }),
                _ => None,
            })
    }

    pub(crate) fn pending_modify_promotions(&self) -> Vec<ModifyPromotion> {
        self.pending_commands
            .iter()
            .filter_map(|(client_order_id, command)| match command {
                PendingCommand::Modify {
                    old_venue_order_id,
                    replacement: Some(replacement),
                    ..
                } => Some(ModifyPromotion {
                    client_order_id: *client_order_id,
                    old_venue_order_id: *old_venue_order_id,
                    venue_order_id: replacement.venue_order_id,
                    quantity: replacement.quantity,
                    leg_quantity: replacement.leg_quantity,
                    price: replacement.price,
                }),
                _ => None,
            })
            .collect()
    }

    pub(crate) fn begin_cancels(&mut self, orders: &[(ClientOrderId, InstrumentId)]) -> bool {
        if orders.iter().any(|(client_order_id, instrument_id)| {
            self.pending_commands.contains_key(client_order_id)
                || self.inflight_cancel_markets.contains(instrument_id)
        }) {
            return false;
        }

        self.pending_commands.extend(
            orders
                .iter()
                .map(|(client_order_id, _)| (*client_order_id, PendingCommand::Cancel)),
        );
        true
    }

    pub(crate) fn begin_available_cancels(
        &mut self,
        orders: &[(ClientOrderId, InstrumentId)],
    ) -> Option<Vec<ClientOrderId>> {
        if orders.iter().any(|(client_order_id, instrument_id)| {
            matches!(
                self.pending_commands.get(client_order_id),
                Some(PendingCommand::Modify { .. })
            ) || self.inflight_cancel_markets.contains(instrument_id)
        }) {
            return None;
        }

        let client_order_ids = orders
            .iter()
            .filter_map(|(client_order_id, _)| {
                if self.pending_commands.contains_key(client_order_id) {
                    return None;
                }

                self.pending_commands
                    .insert(*client_order_id, PendingCommand::Cancel);
                Some(*client_order_id)
            })
            .collect();
        Some(client_order_ids)
    }

    pub(crate) fn finish_cancels(&mut self, client_order_ids: &[ClientOrderId]) {
        for client_order_id in client_order_ids {
            if matches!(
                self.pending_commands.get(client_order_id),
                Some(PendingCommand::Cancel)
            ) {
                self.pending_commands.remove(client_order_id);
            }
        }
    }

    pub(crate) fn begin_market_cancel(&mut self, instrument_id: InstrumentId) -> bool {
        if self.pending_commands.values().any(|command| match command {
            PendingCommand::Modify {
                instrument_id: pending_instrument_id,
                ..
            } => *pending_instrument_id == instrument_id,
            PendingCommand::Cancel => false,
        }) {
            return false;
        }

        self.inflight_cancel_markets.insert(instrument_id)
    }

    pub(crate) fn finish_market_cancel(&mut self, instrument_id: InstrumentId) {
        self.inflight_cancel_markets.remove(&instrument_id);
    }

    pub(crate) fn record_terminal_cancel_report(&mut self, report: OrderStatusReport) {
        self.terminal_cancel_reports
            .insert(report.venue_order_id, report);
    }

    pub(crate) fn record_reconciled_fill(
        &mut self,
        trade_id: TradeId,
        venue_order_id: VenueOrderId,
    ) {
        self.reconciled_fills.add((trade_id, venue_order_id));
    }

    pub(crate) fn replaced_venue_order_id(&self, venue_order_id: VenueOrderId) -> bool {
        self.replaced_venue_order_ids.contains(&venue_order_id)
    }

    pub(crate) fn reset_session(&mut self) {
        let retained_cancel_reports = self
            .pending_commands
            .values()
            .filter_map(|command| match command {
                PendingCommand::Modify {
                    old_venue_order_id, ..
                } => self
                    .terminal_cancel_reports
                    .get(old_venue_order_id)
                    .cloned(),
                PendingCommand::Cancel => None,
            })
            .collect::<Vec<_>>();

        self.pending_terminal_orders.clear();
        self.terminal_cancel_reports.clear();
        for report in retained_cancel_reports {
            self.terminal_cancel_reports
                .insert(report.venue_order_id, report);
        }

        self.pending_commands
            .retain(|_, command| matches!(command, PendingCommand::Modify { .. }));
        self.inflight_cancel_markets.clear();
    }

    fn suppress_modify_cancel(&self, venue_order_id: VenueOrderId) -> bool {
        self.closed_modify_venue_order_ids
            .contains_key(&venue_order_id)
            || self.suppress_modify_cancel_reemit(venue_order_id)
    }

    pub(crate) fn suppress_modify_cancel_reemit(&self, venue_order_id: VenueOrderId) -> bool {
        self.replaced_venue_order_ids.contains(&venue_order_id)
            || self.pending_commands.values().any(|command| {
                matches!(
                    command,
                    PendingCommand::Modify {
                        old_venue_order_id,
                        ..
                    } if *old_venue_order_id == venue_order_id
                )
            })
    }
}

#[derive(Clone, Copy, Debug)]
enum PendingCommand {
    Modify {
        old_venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        cancel_ts: Option<UnixNanos>,
        replacement: Option<PendingModifyReplacement>,
    },
    Cancel,
}

#[derive(Clone, Copy, Debug)]
struct PendingModifyReplacement {
    venue_order_id: VenueOrderId,
    quantity: Quantity,
    leg_quantity: Quantity,
    price: Price,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ModifyPromotion {
    pub(crate) client_order_id: ClientOrderId,
    pub(crate) old_venue_order_id: VenueOrderId,
    pub(crate) venue_order_id: VenueOrderId,
    pub(crate) quantity: Quantity,
    pub(crate) leg_quantity: Quantity,
    pub(crate) price: Price,
}

#[derive(Clone, Debug)]
struct PendingTerminalOrder {
    trade_ids: Vec<String>,
    ts_event: UnixNanos,
}

/// Immutable context borrowed from the async block's owned values.
pub(crate) struct WsDispatchContext<'a> {
    pub token_instruments: &'a AtomicMap<Ustr, InstrumentAny>,
    pub fill_tracker: &'a OrderFillTrackerMap,
    pub settlement: &'a SettlementRegistry,
    pub pending_submits: &'a PendingSubmitTracker,
    pub order_contexts: &'a OrderContextRegistry,
    pub emitter: &'a ExecutionEventEmitter,
    pub account_id: AccountId,
    pub clock: &'static AtomicTime,
    pub signer_type: PolymarketSignerType,
    pub user_address: &'a str,
    pub user_api_key: &'a str,
}

impl Debug for WsDispatchContext<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct(stringify!(WsDispatchContext))
            .field("token_instruments", &self.token_instruments)
            .field("fill_tracker", &self.fill_tracker)
            .field("settlement", &self.settlement)
            .field("pending_submits", &self.pending_submits)
            .field("order_contexts", &self.order_contexts)
            .field("emitter", &self.emitter)
            .field("account_id", &self.account_id)
            .field("clock", &self.clock)
            .field("user_address", &self.user_address)
            .field("user_api_key", &REDACTED)
            .finish()
    }
}

impl WsDispatchContext<'_> {
    pub(crate) fn admission_context(&self) -> AdmissionContext<'_> {
        AdmissionContext {
            signer_type: self.signer_type,
            user_address: self.user_address,
            api_key: self.user_api_key,
            pusd: get_pusd_currency(),
            instruments: self.token_instruments,
        }
    }
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
    let Some(status) = order.status.as_ref() else {
        log::warn!("Ignoring order update without status: {}", order.id);
        return;
    };

    let Some(order_type) = order.order_type else {
        log::warn!("Ignoring order update without order_type: {}", order.id);
        return;
    };

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
    let mut report = match build_ws_order_status_report(
        order,
        status,
        order_type,
        instrument,
        ctx.account_id,
        ts_event,
        ts_init,
    ) {
        Ok(report) => report,
        Err(e) => {
            log::warn!("Ignoring invalid order update {}: {e}", order.id);
            return;
        }
    };
    let mut promoted_fills = Vec::new();
    let mut promoted_reports = Vec::new();
    let promoted_client_order_id = if state.pending_modify_promotion(venue_order_id).is_some() {
        if report.order_status == OrderStatus::Rejected {
            reject_modify_replacement(venue_order_id, &report, ts_event, ctx, state);
            return;
        }

        promote_modify_replacement_from_ws(
            venue_order_id,
            ts_event,
            ctx,
            state,
            &mut promoted_fills,
            &mut promoted_reports,
        )
    } else {
        None
    };

    let local_client_order_id =
        promoted_client_order_id.or_else(|| ctx.pending_submits.client_order_id(&venue_order_id));
    report.client_order_id = local_client_order_id;
    let (is_accepted, mut buffered_fills) = take_status_update_fills(&report, ctx);

    buffered_fills.splice(0..0, promoted_fills);

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
        state
            .terminal_cancel_reports
            .insert(venue_order_id, report.clone());
    }

    let suppress_cancel = report.order_status == OrderStatus::Canceled
        && state.suppress_modify_cancel(venue_order_id);

    // Tracked own orders route through order events; externally-managed orders
    // (no captured context) buffer until accepted or fall back to reports.
    let context = ctx.order_contexts.get(&venue_order_id);

    // Emit fills first: a terminal status would otherwise close the order ahead of them
    for fill in buffered_fills {
        match context {
            Some(context) => {
                emit_buffered_order_filled(&context, &fill, ctx);
            }
            None => emit_buffered_fill_report(fill, ctx),
        }
    }

    for buffered in promoted_reports {
        if buffered.order_status == OrderStatus::Canceled {
            state
                .terminal_cancel_reports
                .insert(venue_order_id, buffered.clone());
        }

        if let Some(context) = context {
            emit_tracked_order_status(&buffered, &context, buffered.ts_last, ctx);
        }
    }

    if suppress_cancel {
        log::debug!("Suppressing stale cancel for modified venue leg {venue_order_id}");
        return;
    }

    if is_accepted || local_client_order_id.is_some() {
        match context {
            Some(context) => emit_tracked_order_status(&report, &context, ts_event, ctx),
            None => ctx.emitter.send_order_status_report(report),
        }
    } else if let Some(report) = ctx
        .fill_tracker
        .accept_or_buffer_report(venue_order_id, report)
    {
        // Registered between the early accepted-check and here: emit rather than buffer
        match ctx.order_contexts.get(&venue_order_id) {
            Some(context) => emit_tracked_order_status(&report, &context, ts_event, ctx),
            None => ctx.emitter.send_order_status_report(report),
        }
    }

    if status.status == PolymarketOrderStatus::Matched
        && let Some(trade_ids) = order.associate_trades.clone().filter(|ids| !ids.is_empty())
    {
        state.pending_terminal_orders.insert(
            venue_order_id,
            PendingTerminalOrder {
                trade_ids,
                ts_event,
            },
        );
        emit_quantity_normalization_if_ready(venue_order_id, ctx, state);
    }
}

/// Takes the buffered fills released by an order status update and reports whether the order is
/// accepted; a known own order (submit in flight or outcome unknown) self-registers on its first
/// update.
fn take_status_update_fills(
    report: &OrderStatusReport,
    ctx: &WsDispatchContext<'_>,
) -> (bool, Vec<BufferedFill>) {
    let venue_order_id = report.venue_order_id;
    let is_accepted = ctx.fill_tracker.contains(&venue_order_id);

    if report.client_order_id.is_some()
        && !is_accepted
        && report.order_status != OrderStatus::Rejected
    {
        let fills = ctx.fill_tracker.register_and_take_pending_fills(
            venue_order_id,
            report.client_order_id,
            report.quantity,
            report
                .order_side
                .expect("order status report side must be Buy or Sell"),
        );
        return (true, fills);
    }

    if is_accepted {
        let fills = ctx
            .fill_tracker
            .take_pending_fills(venue_order_id, report.client_order_id);
        return (true, fills);
    }

    (false, Vec::new())
}

fn promote_modify_replacement_from_ws(
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
    buffered_fills: &mut Vec<BufferedFill>,
    buffered_reports: &mut Vec<OrderStatusReport>,
) -> Option<ClientOrderId> {
    let promotion = state.claim_modify_replacement(venue_order_id)?;
    let Some(mut context) = ctx.order_contexts.get(&promotion.old_venue_order_id) else {
        log::error!(
            "Cannot promote Polymarket replacement {venue_order_id}: old venue leg {} has no context",
            promotion.old_venue_order_id,
        );
        return None;
    };

    context.quantity = promotion.quantity;
    context.price = Some(promotion.price);
    ctx.order_contexts.register_context(venue_order_id, context);
    ctx.order_contexts.mark_accepted(venue_order_id);

    let updated = OrderUpdated::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        promotion.client_order_id,
        promotion.quantity,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(ctx.account_id),
        Some(promotion.price),
        None,
        None,
        false,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Updated(updated));

    buffered_fills.extend(ctx.fill_tracker.register_and_take_pending_fills(
        venue_order_id,
        Some(promotion.client_order_id),
        promotion.leg_quantity,
        context.identity.order_side,
    ));
    buffered_reports.extend(ctx.fill_tracker.take_pending_reports(&venue_order_id));
    Some(promotion.client_order_id)
}

fn reject_modify_replacement(
    venue_order_id: VenueOrderId,
    report: &OrderStatusReport,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let Some(promotion) = state.pending_modify_promotion(venue_order_id) else {
        return;
    };

    let Some((old_venue_order_id, cancel_ts)) = state.finish_modify_without_replacement(
        promotion.client_order_id,
        promotion.old_venue_order_id,
        true,
        ts_event,
    ) else {
        return;
    };

    let Some(context) = ctx.order_contexts.get(&old_venue_order_id) else {
        return;
    };

    let reason = report
        .cancel_reason
        .as_deref()
        .unwrap_or("replacement order rejected");
    ctx.emitter.emit_order_modify_rejected_event(
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
        Some(old_venue_order_id),
        &sanitize_error_text(reason),
        ts_event,
    );

    if let Some(cancel_ts) = cancel_ts {
        emit_order_canceled(&context, old_venue_order_id, cancel_ts, ctx);
    }
}

fn emit_buffered_order_filled(
    context: &OrderContext,
    buffered: &BufferedFill,
    ctx: &WsDispatchContext<'_>,
) {
    let fill = &buffered.report;
    ensure_accepted(context, fill.venue_order_id, fill.ts_event, ctx);

    let info = buffered
        .correction
        .as_ref()
        .and_then(|correction| correction.info.clone());
    let filled = build_order_filled(context, fill, info, ctx);
    ctx.fill_tracker.emit_buffered_fill(
        filled,
        || buffered.claim(ctx.settlement),
        |filled, new_qty| {
            if let Some(new_qty) = new_qty {
                emit_buy_overfill_update(context, fill.venue_order_id, new_qty, fill.ts_event, ctx);
            }

            ctx.settlement.note_leg_enqueued(&fill.trade_id);
            ctx.emitter.send_order_event(OrderEventAny::Filled(filled));
        },
    );
}

/// Routes a buffered fill for an order without captured context through the report path, if
/// its trade still permits application.
fn emit_buffered_fill_report(buffered: BufferedFill, ctx: &WsDispatchContext<'_>) {
    let report = &buffered.report;
    if !buffered.claim(ctx.settlement) {
        ctx.fill_tracker
            .reverse_fill(&report.venue_order_id, report.last_qty);
        return;
    }

    let trade_id = report.trade_id;
    ctx.emitter.send_fill_report(buffered.report);
    ctx.settlement.note_leg_reported(&trade_id);
}

fn is_ready_for_terminal_normalization(
    venue_order_id: &VenueOrderId,
    ctx: &WsDispatchContext<'_>,
    state: &WsDispatchState,
) -> bool {
    state
        .pending_terminal_orders
        .get(venue_order_id)
        .is_some_and(|pending| {
            pending
                .trade_ids
                .iter()
                .all(|trade_id| ctx.settlement.is_trade_confirmed(trade_id))
        })
}

fn emit_quantity_normalization_if_ready(
    venue_order_id: VenueOrderId,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    if !is_ready_for_terminal_normalization(&venue_order_id, ctx, state) {
        return;
    }

    let Some(pending) = state.pending_terminal_orders.remove(&venue_order_id) else {
        return;
    };

    let Some(context) = ctx.order_contexts.get(&venue_order_id) else {
        log::warn!("Cannot normalize terminal order {venue_order_id} without a local context");
        return;
    };

    if let Some(quantity) = ctx
        .fill_tracker
        .check_terminal_quantity_normalization(&venue_order_id)
    {
        emit_terminal_quantity_update(&context, venue_order_id, quantity, pending.ts_event, ctx);
    }
}

/// Emits the terminal order event for a taker order once its trade confirms.
///
/// Taker fills receive no order-channel `MATCHED` update. FOK is atomic, so a sub-cent quantity
/// difference can be normalized. IOC maps to FAK, so every positive remainder was killed by the
/// venue and must close as `Canceled` without changing the venue-reported fill quantity.
fn emit_taker_terminal_status(
    venue_order_id: VenueOrderId,
    ctx: &WsDispatchContext<'_>,
    ts_event: UnixNanos,
) {
    let Some(context) = ctx.order_contexts.get(&venue_order_id) else {
        return;
    };

    if context.time_in_force == TimeInForce::Fok {
        if let Some(quantity) = ctx
            .fill_tracker
            .check_terminal_quantity_normalization(&venue_order_id)
        {
            emit_terminal_quantity_update(&context, venue_order_id, quantity, ts_event, ctx);
        }
        return;
    }

    if context.time_in_force == TimeInForce::Ioc
        && let Some(remainder) = ctx
            .fill_tracker
            .take_terminal_ioc_remainder(&venue_order_id)
    {
        log::debug!(
            "Closing terminal IOC order {venue_order_id} as Canceled (unfilled remainder={remainder})"
        );
        emit_order_canceled(&context, venue_order_id, ts_event, ctx);
    }
}

fn dispatch_trade_update(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> Option<AccountRefreshRequest> {
    let admitted =
        match admit_trade_evidence(TradeEvidence::Stream(trade), &ctx.admission_context()) {
            Ok(admitted) => admitted,
            Err(AdmissionError::UnknownInstrument(token)) => {
                log::warn!(
                    "Deferring trade {} until its instrument {token} is available",
                    trade.id
                );
                return None;
            }
            Err(AdmissionError::Unowned) => {
                log::warn!(
                    "Dropping trade {} holding no legs owned by the account",
                    trade.id
                );
                return None;
            }
            Err(AdmissionError::Untimestamped(e) | AdmissionError::Invalid(e)) => {
                log::error!("Cannot admit stream evidence for trade {}: {e}", trade.id);
                ctx.settlement.quarantine_invalid_trade(&trade.id);
                return None;
            }
        };

    let actions = ctx.settlement.admit_stream_trade(&admitted);
    execute_settlement_actions(actions, trade_fill_info(trade).as_ref(), ctx, state);

    if !ctx.settlement.is_trade_confirmed(&admitted.venue_trade_id) {
        return None;
    }

    // Quantity normalization and taker terminal statuses key off trade confirmation
    let ts_event = parse_timestamp_ms(&trade.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    confirm_trade_bookkeeping(&admitted, ts_event, ctx, state);
    Some(AccountRefreshRequest)
}

/// Executes the effects a registry transition authorized, in order.
pub(crate) fn execute_settlement_actions(
    actions: Vec<SettlementAction>,
    fill_info: Option<&IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    for action in actions {
        match action {
            SettlementAction::ApplyLeg {
                venue_trade_id,
                leg,
            } => {
                apply_authorized_leg(&venue_trade_id, &leg, fill_info.cloned(), ctx, state);
            }
            SettlementAction::VoidAppliedFill {
                venue_trade_id,
                fill,
            } => {
                emit_void_for_applied_fill(
                    &venue_trade_id,
                    &fill,
                    ctx.fill_tracker,
                    ctx.emitter,
                    ctx.clock,
                );
            }
        }
    }
}

/// Applies a targeted REST trade result through the settlement registry, then runs the same
/// confirmation bookkeeping as a stream confirmation once the trade is confirmed.
///
/// Evidence that fails admission leaves the trade pending for the next targeted read.
pub(crate) fn apply_rest_trade_evidence(
    trade: &PolymarketTradeReport,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let admitted = match admit_trade_evidence(TradeEvidence::Rest(trade), &ctx.admission_context())
    {
        Ok(admitted) => admitted,
        Err(e) => {
            log::warn!(
                "Targeted REST evidence for Polymarket trade {} did not pass admission: {e}; \
                 retrying",
                trade.id
            );
            return;
        }
    };

    apply_admitted_rest_trade(&admitted, trade, ctx, state);
}

/// Applies REST evidence for an order whose submit outcome was unknown, returning `true` once
/// its venue state is applied.
///
/// Terminal trades pass through the settlement registry first, so their fills queue behind the
/// order's acceptance; the order status then registers and accepts the order and releases them.
/// The status is withheld while any trade of the order is still provisional, or while confirmed
/// trades do not yet cover the venue's matched quantity, so the order never reports a state
/// ahead of its fills.
pub(crate) fn apply_uncertain_order_evidence(
    venue_order_id: VenueOrderId,
    order: &PolymarketOpenOrder,
    trades: &[PolymarketTradeReport],
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> bool {
    let Some((context, instrument)) = submitted_order_context(venue_order_id, order, ctx) else {
        return false;
    };

    let trades_applied =
        apply_order_trade_evidence(venue_order_id, order, trades, ctx, |admitted, trade| {
            let actions = ctx.settlement.admit_rest_result(admitted);
            execute_settlement_actions(actions, rest_trade_info(trade).as_ref(), ctx, state);
        });

    if !trades_applied {
        return false;
    }

    let report = match parse_order_status_report(
        order,
        instrument.id(),
        ctx.account_id,
        ctx.pending_submits.client_order_id(&venue_order_id),
        instrument.price_precision(),
        instrument.size_precision(),
        ctx.clock.get_time_ns(),
    ) {
        Ok(report) => report,
        Err(e) => {
            log::warn!("Cannot apply REST status of uncertain order {venue_order_id}: {e}");
            return false;
        }
    };

    let (_, buffered_fills) = take_status_update_fills(&report, ctx);

    for fill in buffered_fills {
        emit_buffered_order_filled(&context, &fill, ctx);
    }

    emit_tracked_order_status(&report, &context, report.ts_last, ctx);

    // A filled taker order reaches the terminal normalization a stream confirmation would apply
    if report.order_status == OrderStatus::Filled {
        emit_taker_terminal_status(venue_order_id, ctx, report.ts_last);
    }

    true
}

/// Applies REST trades for an order that was live while the user stream was disconnected,
/// returning `true` once they cover the venue's matched quantity.
///
/// Only trades the settlement registry has never seen are applied, so each missed fill applies
/// once and trades it already holds keep their established resolution. The order status is not
/// applied: the stream and reconciliation keep driving the lifecycle of an accepted order.
pub(crate) fn apply_stream_gap_order_evidence(
    venue_order_id: VenueOrderId,
    order: &PolymarketOpenOrder,
    trades: &[PolymarketTradeReport],
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> bool {
    if submitted_order_context(venue_order_id, order, ctx).is_none() {
        return false;
    }

    apply_order_trade_evidence(venue_order_id, order, trades, ctx, |admitted, trade| {
        if ctx.settlement.knows_trade(admitted) {
            return;
        }

        log::info!(
            "Discovered Polymarket trade {} on order {venue_order_id} missed during a user \
             stream disconnect",
            admitted.venue_trade_id
        );
        apply_admitted_rest_trade(admitted, trade, ctx, state);
    })
}

fn apply_admitted_rest_trade(
    admitted: &AdmittedTrade,
    trade: &PolymarketTradeReport,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let actions = ctx.settlement.admit_rest_result(admitted);
    execute_settlement_actions(actions, rest_trade_info(trade).as_ref(), ctx, state);

    if ctx.settlement.is_trade_confirmed(&admitted.venue_trade_id) {
        confirm_trade_bookkeeping(admitted, ctx.clock.get_time_ns(), ctx, state);
    }
}

/// Returns the captured context and instrument of a submitted order, or `None` when they are
/// unknown or its REST order evidence contradicts them.
fn submitted_order_context(
    venue_order_id: VenueOrderId,
    order: &PolymarketOpenOrder,
    ctx: &WsDispatchContext<'_>,
) -> Option<(OrderContext, InstrumentAny)> {
    let (Some(context), Some(instrument)) = (
        ctx.order_contexts.get(&venue_order_id),
        ctx.token_instruments.get_cloned(&order.asset_id),
    ) else {
        return None;
    };

    if order.id != venue_order_id.as_str()
        || instrument.id() != context.identity.instrument_id
        || OrderSide::from(order.side) != context.identity.order_side
    {
        log::warn!(
            "REST order evidence for uncertain order {venue_order_id} contradicts the submitted \
             order"
        );
        return None;
    }

    Some((context, instrument))
}

/// Admits the REST trades touching an order and passes each to `apply`, returning `true` once
/// every trade is terminal and the confirmed trades cover the venue's matched quantity.
///
/// Nothing is applied while any trade is still provisional.
fn apply_order_trade_evidence(
    venue_order_id: VenueOrderId,
    order: &PolymarketOpenOrder,
    trades: &[PolymarketTradeReport],
    ctx: &WsDispatchContext<'_>,
    mut apply: impl FnMut(&AdmittedTrade, &PolymarketTradeReport),
) -> bool {
    let Some(order_trades) = uncertain_order_trades(venue_order_id, trades) else {
        return false;
    };

    if order_trades
        .iter()
        .any(|trade| trade.status.is_pending_settlement())
    {
        return false;
    }

    let mut confirmed_qty = Decimal::ZERO;

    for trade in order_trades {
        let admitted =
            match admit_trade_evidence(TradeEvidence::Rest(trade), &ctx.admission_context()) {
                Ok(admitted) => admitted,
                Err(e) => {
                    log::warn!(
                        "Cannot admit REST evidence for trade {} of uncertain order \
                         {venue_order_id}: {e}",
                        trade.id
                    );
                    return false;
                }
            };

        if admitted.status == PolymarketTradeStatus::Confirmed {
            confirmed_qty += admitted
                .legs
                .iter()
                .filter(|leg| leg.venue_order_id == venue_order_id)
                .map(|leg| leg.last_qty.as_decimal())
                .sum::<Decimal>();
        }

        apply(&admitted, trade);
    }

    // REST can list the order as matched before its trades appear
    if confirmed_qty < order.size_matched {
        log::debug!(
            "Confirmed REST trades cover {confirmed_qty} of {} matched for uncertain order \
             {venue_order_id}; retrying",
            order.size_matched
        );
        return false;
    }

    true
}

/// Returns the distinct REST trades touching `venue_order_id`, or `None` when pagination repeats
/// a trade with contradictory evidence.
fn uncertain_order_trades(
    venue_order_id: VenueOrderId,
    trades: &[PolymarketTradeReport],
) -> Option<Vec<&PolymarketTradeReport>> {
    let mut selected = AHashMap::new();
    let mut order_trades = Vec::new();

    for trade in trades.iter().filter(|trade| {
        trade.taker_order_id == venue_order_id.as_str()
            || trade
                .maker_orders
                .iter()
                .any(|maker| maker.order_id == venue_order_id.as_str())
    }) {
        match admit_selected_trade(&mut selected, trade) {
            Ok(true) => order_trades.push(trade),
            Ok(false) => {}
            Err(e) => {
                log::warn!(
                    "REST evidence for uncertain order {venue_order_id} is contradictory: {e}"
                );
                return None;
            }
        }
    }

    Some(order_trades)
}

/// Converts an admitted leg into a fill report and routes it through the established emission
/// machinery: modify promotion first, then tracked-order emission or buffered delivery, with
/// report fallback for orders without captured context.
fn apply_authorized_leg(
    venue_trade_id: &str,
    leg: &AdmittedLeg,
    fill_info: Option<IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let venue_order_id = leg.venue_order_id;

    // Already delivered through a reconciliation fill report
    if state
        .reconciled_fills
        .contains(&(leg.trade_id, venue_order_id))
    {
        ctx.settlement.note_leg_reported(&leg.trade_id);
        return;
    }

    let mut report = leg.fill_report(ctx.account_id, ctx.clock.get_time_ns());
    report.client_order_id = ctx.pending_submits.client_order_id(&venue_order_id);
    report.last_qty = ctx
        .fill_tracker
        .snap_fill_qty(&venue_order_id, report.last_qty);

    let correction_info = fill_info.clone();

    let correction = FillCorrectionMetadata {
        venue_trade_id: venue_trade_id.to_string(),
        info: fill_info,
    };

    let mut promoted_reports = Vec::new();

    if state.pending_modify_promotion(venue_order_id).is_some() {
        let mut buffered_fills = Vec::new();
        report.client_order_id = promote_modify_replacement_from_ws(
            venue_order_id,
            report.ts_event,
            ctx,
            state,
            &mut buffered_fills,
            &mut promoted_reports,
        );
        emit_promoted_ws_fills(venue_order_id, buffered_fills, ctx);
    }

    if let Some(report) = ctx
        .fill_tracker
        .accept_or_buffer_fill(venue_order_id, report, correction)
    {
        match ctx.order_contexts.get(&venue_order_id) {
            Some(context) => {
                emit_order_filled(&context, &report, correction_info, ctx);
            }
            None => {
                ctx.emitter.send_fill_report(report);
                ctx.settlement.note_leg_reported(&leg.trade_id);
            }
        }

        reemit_terminal_cancel(venue_order_id, state, ctx);
    }

    emit_promoted_ws_reports(venue_order_id, promoted_reports, ctx, state);
}

/// Confirmation bookkeeping beyond the settlement registry: terminal quantity normalization
/// and taker terminal statuses once a trade confirms through stream or REST evidence.
fn confirm_trade_bookkeeping(
    admitted: &AdmittedTrade,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    for leg in &admitted.legs {
        emit_quantity_normalization_if_ready(leg.venue_order_id, ctx, state);

        if leg.liquidity_side == LiquiditySide::Taker {
            emit_taker_terminal_status(leg.venue_order_id, ctx, ts_event);
        }
    }
}

fn emit_promoted_ws_fills(
    venue_order_id: VenueOrderId,
    buffered_fills: Vec<BufferedFill>,
    ctx: &WsDispatchContext<'_>,
) {
    let context = ctx.order_contexts.get(&venue_order_id);

    for fill in buffered_fills {
        match context {
            Some(context) => emit_buffered_order_filled(&context, &fill, ctx),
            None => emit_buffered_fill_report(fill, ctx),
        }
    }
}

fn emit_promoted_ws_reports(
    venue_order_id: VenueOrderId,
    buffered_reports: Vec<OrderStatusReport>,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) {
    let context = ctx.order_contexts.get(&venue_order_id);

    for report in buffered_reports {
        if report.order_status == OrderStatus::Canceled {
            state.record_terminal_cancel_report(report.clone());
        }

        match context {
            Some(context) => emit_tracked_order_status(&report, &context, report.ts_last, ctx),
            None => ctx.emitter.send_order_status_report(report),
        }
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

    if state.suppress_modify_cancel_reemit(venue_order_id) {
        return;
    }

    let cancel_ts = state
        .closed_modify_venue_order_ids
        .get(&venue_order_id)
        .copied()
        .or_else(|| {
            state
                .terminal_cancel_reports
                .get(&venue_order_id)
                .map(|report| report.ts_last)
        });

    if let Some(cancel_ts) = cancel_ts {
        log::debug!("Re-emitting cancel for {venue_order_id} after fill to restore terminal state");
        match ctx.order_contexts.get(&venue_order_id) {
            Some(context) => {
                emit_order_canceled(&context, venue_order_id, cancel_ts, ctx);
            }
            None => {
                if let Some(cancel_report) = state.terminal_cancel_reports.get(&venue_order_id) {
                    ctx.emitter.send_order_status_report(cancel_report.clone());
                }
            }
        }
    }
}

fn build_ws_order_status_report(
    order: &PolymarketUserOrder,
    status: &PolymarketUserOrderStatus,
    order_type: PolymarketOrderType,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> anyhow::Result<OrderStatusReport> {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_status =
        crate::execution::parse::resolve_order_status(status.status, order.event_type);
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order_type);
    let size_precision = instrument.size_precision();
    let price_precision = instrument.price_precision();
    let price_dec = parse_decimal_exact(&order.price)?;
    anyhow::ensure!(
        price_dec > Decimal::ZERO && price_dec < Decimal::ONE,
        "order price must be in (0, 1)"
    );
    let quantity_dec = parse_decimal_exact(&order.original_size)?;
    // Unfilled FOK cancellations carry an empty size_matched in captured venue messages
    let filled_dec = if order.size_matched.is_empty() {
        Decimal::ZERO
    } else {
        parse_decimal_exact(&order.size_matched)?
    };
    anyhow::ensure!(
        quantity_dec > Decimal::ZERO && filled_dec >= Decimal::ZERO,
        "invalid order quantity"
    );
    let quantity = Quantity::from_decimal_dp(
        original_size_to_shares(quantity_dec, price_dec, order.side, order_type)?,
        size_precision,
    )?;
    let filled_qty = Quantity::from_decimal_dp(filled_dec, size_precision)?;
    let price = Price::from_decimal_dp(price_dec, price_precision)?;

    let mut report = OrderStatusReport::new(
        account_id,
        instrument.id(),
        None,
        venue_order_id,
        order_side.into(),
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

    if order_status == OrderStatus::Rejected {
        report.cancel_reason.clone_from(&status.reason);
    }

    Ok(report)
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
) -> anyhow::Result<Decimal> {
    if side != PolymarketOrderSide::Buy
        || !matches!(
            order_type,
            PolymarketOrderType::FAK | PolymarketOrderType::FOK
        )
    {
        return Ok(original_size);
    }

    original_size
        .checked_div(price)
        .context("order share quantity overflow")
}

/// Emits order events for a tracked own-order status update.
///
/// Order-channel messages drive lifecycle events only; fills arrive separately on the trade
/// channel as `OrderFilled`. `PartiallyFilled` / `Filled` statuses therefore emit no fill here,
/// they only ensure acceptance has been emitted so the order lifecycle stays well-formed.
fn emit_tracked_order_status(
    report: &OrderStatusReport,
    context: &OrderContext,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let venue_order_id = report.venue_order_id;
    match report.order_status {
        OrderStatus::Accepted => ensure_accepted(context, venue_order_id, ts_event, ctx),
        OrderStatus::PartiallyFilled | OrderStatus::Filled => {
            ensure_accepted(context, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Canceled => {
            ensure_accepted(context, venue_order_id, ts_event, ctx);
            emit_order_canceled(context, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Expired => {
            ensure_accepted(context, venue_order_id, ts_event, ctx);
            emit_order_expired(context, venue_order_id, ts_event, ctx);
        }
        OrderStatus::Rejected => {
            let reason = report
                .cancel_reason
                .clone()
                .unwrap_or_else(|| "REJECTED".to_string());

            emit_order_rejected(context, &reason, ts_event, ctx);
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
    context: &OrderContext,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    if !ctx.order_contexts.mark_accepted(venue_order_id) {
        return;
    }

    let accepted = OrderAccepted::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
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
    context: &OrderContext,
    fill: &FillReport,
    info: Option<IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
) {
    ensure_accepted(context, fill.venue_order_id, fill.ts_event, ctx);

    if let Some(new_qty) = ctx.fill_tracker.buy_overfill_bump(&fill.venue_order_id) {
        emit_buy_overfill_update(context, fill.venue_order_id, new_qty, fill.ts_event, ctx);
    }

    let filled = build_order_filled(context, fill, info, ctx);

    // Pending before sending, so a decline observed during delivery finds the leg in flight
    ctx.settlement.note_leg_enqueued(&fill.trade_id);
    ctx.emitter.send_order_event(OrderEventAny::Filled(filled));
}

fn build_order_filled(
    context: &OrderContext,
    fill: &FillReport,
    info: Option<IndexMap<Ustr, Ustr>>,
    ctx: &WsDispatchContext<'_>,
) -> OrderFilled {
    OrderFilled::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
        fill.venue_order_id,
        ctx.account_id,
        fill.trade_id,
        context.identity.order_side,
        context.identity.order_type,
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

/// Emits the one required `OrderFillVoided` for an applied leg of a REST-established `FAILED`
/// trade, constructed from the canonical applied fill published by core.
pub(crate) fn emit_void_for_applied_fill(
    venue_trade_id: &str,
    fill: &OrderFilled,
    tracker: &OrderFillTrackerMap,
    emitter: &ExecutionEventEmitter,
    clock: &'static AtomicTime,
) {
    let ts_event = clock.get_time_ns();

    let mut voided = OrderFillVoided::new(
        fill.trader_id,
        fill.strategy_id,
        fill.instrument_id,
        fill.client_order_id,
        fill.venue_order_id,
        fill.account_id,
        Ustr::from(&format!("{venue_trade_id}-FAILED-{}", fill.client_order_id)),
        fill.trade_id,
        fill.last_qty,
        fill.commission.clone(),
        fill.order_side,
        fill.order_type,
        fill.last_px,
        fill.currency.clone(),
        fill.liquidity_side,
        fill.position_id,
        Some(Ustr::from("FAILED")),
        fill.info.clone(),
        UUID4::new(),
        ts_event,
        clock.get_time_ns(),
        false,
        false,
    );
    voided.causation_id = Some(fill.event_id);
    tracker.reverse_fill(&fill.venue_order_id, fill.last_qty);
    emitter.send_order_event(OrderEventAny::FillVoided(voided));
}

/// Flattens a REST trade report into a string map of venue fill metadata for
/// `OrderFilled.info`.
pub(crate) fn rest_trade_info(trade: &PolymarketTradeReport) -> Option<IndexMap<Ustr, Ustr>> {
    let value = serde_json::to_value(trade).ok()?;
    flatten_trade_value(&value)
}

/// Flattens a user trade into a string map of venue fill metadata for `OrderFilled.info`.
///
/// Mirrors the v1 adapter, which attaches the full raw trade to each fill it generates. Scalar
/// fields map to their string form; nested fields (such as `maker_orders`) become their JSON text.
fn trade_fill_info(trade: &PolymarketUserTrade) -> Option<IndexMap<Ustr, Ustr>> {
    let value = serde_json::to_value(trade).ok()?;
    flatten_trade_value(&value)
}

fn flatten_trade_value(value: &serde_json::Value) -> Option<IndexMap<Ustr, Ustr>> {
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
    context: &OrderContext,
    venue_order_id: VenueOrderId,
    new_qty: Quantity,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let updated = OrderUpdated::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
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
    context: &OrderContext,
    venue_order_id: VenueOrderId,
    quantity: Quantity,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let updated = OrderUpdated::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
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

fn emit_order_canceled(
    context: &OrderContext,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let canceled = OrderCanceled::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        Some(venue_order_id),
        Some(ctx.account_id),
        None,
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Canceled(canceled));
}

fn emit_order_expired(
    context: &OrderContext,
    venue_order_id: VenueOrderId,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let expired = OrderExpired::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
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
    context: &OrderContext,
    reason: &str,
    ts_event: UnixNanos,
    ctx: &WsDispatchContext<'_>,
) {
    let reason = sanitize_error_text(reason);

    let rejected = OrderRejected::new(
        ctx.emitter.trader_id(),
        context.identity.strategy_id,
        context.identity.instrument_id,
        context.identity.client_order_id,
        ctx.account_id,
        Ustr::from(&reason),
        UUID4::new(),
        ts_event,
        ctx.clock.get_time_ns(),
        false,
        is_post_only_crossing(&reason),
    );
    ctx.emitter
        .send_order_event(OrderEventAny::Rejected(rejected));
}

#[cfg(test)]
mod tests {
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::AtomicTime;
    use nautilus_live::execution::context::OrderIdentity;
    use nautilus_model::{
        enums::{AccountType, LiquiditySide, OrderSide, OrderStatus},
        events::OrderEventAny,
        identifiers::{ClientOrderId, InstrumentId, StrategyId, TraderId},
        orders::{Order, builder::OrderTestBuilder},
        types::Currency,
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::{
        common::enums::{PolymarketLiquiditySide, PolymarketOutcome, PolymarketTradeStatus},
        execution::settlement::{
            admission::AdmittedTrade,
            registry::tests::{settlement_state, trade_hard_fault},
            state::SettlementState,
        },
        http::{
            models::GammaMarket,
            parse::{create_instrument_from_def, parse_gamma_market},
        },
    };

    /// Registers a tracked-order context so the dispatch routes the order through events.
    fn register_context(
        order_contexts: &OrderContextRegistry,
        venue_order_id: VenueOrderId,
        instrument_id: InstrumentId,
        client_order_id: &str,
    ) {
        order_contexts.register_context(
            venue_order_id,
            OrderContext {
                identity: OrderIdentity {
                    client_order_id: ClientOrderId::from(client_order_id),
                    strategy_id: StrategyId::from("S-001"),
                    instrument_id,
                    order_side: OrderSide::Buy,
                    order_type: OrderType::Limit,
                },
                quantity: Quantity::from("10"),
                price: Some(Price::from("0.50")),
                trigger_price: None,
                trigger_type: None,
                time_in_force: TimeInForce::Gtc,
                is_post_only: false,
                is_reduce_only: false,
                is_quote_quantity: false,
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

    fn bind_instrument(
        mut instrument: InstrumentAny,
        market: &str,
        outcome: PolymarketOutcome,
    ) -> InstrumentAny {
        let InstrumentAny::BinaryOption(binary) = &mut instrument else {
            panic!("expected BinaryOption test instrument");
        };

        binary.outcome = Some(Ustr::from(outcome.as_str()));
        let mut info = binary.info.take().unwrap_or_default();
        info.insert(
            "condition_id".to_string(),
            serde_json::Value::String(market.to_string()),
        );
        binary.info = Some(info);
        instrument
    }

    fn instrument_for_trade(trade: &PolymarketUserTrade) -> InstrumentAny {
        bind_instrument(test_instrument(), trade.market.as_str(), trade.outcome)
    }

    fn set_taker_fee_rate(instrument: &mut InstrumentAny, rate: Decimal) {
        let InstrumentAny::BinaryOption(binary) = instrument else {
            panic!("expected binary option test instrument");
        };
        let mut info = binary.info.take().unwrap_or_default();
        info.insert(
            "fee_schedule".into(),
            serde_json::json!({
                "exponent": "1",
                "rate": rate.to_string(),
                "takerOnly": true,
                "rebateRate": "0",
            }),
        );
        binary.info = Some(info);
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
    fn test_emit_order_rejected_uses_bounded_clean_reason() {
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();

        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let context = OrderContext {
            identity: OrderIdentity {
                client_order_id: ClientOrderId::from("O-WS-REJECT"),
                strategy_id: StrategyId::from("S-001"),
                instrument_id: instrument.id(),
                order_side: OrderSide::Buy,
                order_type: OrderType::Limit,
            },
            quantity: Quantity::from("10"),
            price: Some(Price::from("0.50")),
            trigger_price: None,
            trigger_type: None,
            time_in_force: TimeInForce::Gtc,
            is_post_only: false,
            is_reduce_only: false,
            is_quote_quantity: false,
        };

        emit_order_rejected(
            &context,
            "  invalid post-only order:\norder crosses book  ",
            UnixNanos::from(1_000_000_000),
            &ctx,
        );

        match receiver.try_recv().expect("expected rejected event") {
            ExecutionEvent::Order(OrderEventAny::Rejected(event)) => {
                assert_eq!(event.reason, "invalid post-only order: order crosses book");
                assert!(event.due_post_only);
            }
            other => panic!("expected rejected event, was {other:?}"),
        }
    }

    #[rstest]
    #[case::empty_price("", "1.01", "0")]
    #[case::zero_price("0", "1.01", "0")]
    #[case::malformed("bad", "100", "0")]
    #[case::too_precise("0.50000000000000000000000000001", "100", "0")]
    #[case::quantity_overflow("0.5", "79228162514264337593543950335", "0")]
    #[case::filled_overflow("0.5", "100", "79228162514264337593543950335")]
    fn test_ws_order_report_rejects_invalid_values(
        #[case] price: &str,
        #[case] quantity: &str,
        #[case] filled: &str,
    ) {
        let mut order: PolymarketUserOrder = load("ws_user_order_placement.json");
        order.price = price.into();
        order.original_size = quantity.into();
        order.size_matched = filled.into();
        assert!(
            build_ws_order_status_report(
                &order,
                order.status.as_ref().unwrap(),
                order.order_type.unwrap(),
                &test_instrument(),
                AccountId::from("POLY-001"),
                UnixNanos::default(),
                UnixNanos::default()
            )
            .is_err()
        );
    }

    #[rstest]
    fn test_ws_fok_order_report_rejects_share_overflow() {
        let mut order: PolymarketUserOrder = load("ws_user_order_fok_buy_pusd_size.json");
        order.original_size = Decimal::MAX.to_string();
        order.price = "0.5".into();
        let result = build_ws_order_status_report(
            &order,
            order.status.as_ref().unwrap(),
            PolymarketOrderType::FOK,
            &test_instrument(),
            AccountId::from("POLY-001"),
            UnixNanos::default(),
            UnixNanos::default(),
        );

        assert_eq!(
            result.unwrap_err().to_string(),
            "order share quantity overflow"
        );
    }

    #[rstest]
    fn test_build_ws_order_status_report() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);
        let ts_init = UnixNanos::from(2_000_000_000u64);

        let report = build_ws_order_status_report(
            &order,
            order.status.as_ref().unwrap(),
            order.order_type.unwrap(),
            &instrument,
            AccountId::from("POLY-001"),
            ts_event,
            ts_init,
        )
        .unwrap();

        assert_eq!(report.order_side, Some(OrderSide::Buy));
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
            order.status.as_ref().unwrap(),
            order.order_type.unwrap(),
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
    fn test_original_size_to_shares(
        #[case] side: PolymarketOrderSide,
        #[case] order_type: PolymarketOrderType,
        #[case] original_size: Decimal,
        #[case] price: Decimal,
        #[case] expected: Decimal,
    ) {
        let shares = original_size_to_shares(original_size, price, side, order_type).unwrap();

        assert_eq!(shares, expected);
    }

    // A non-terminating division still rounds to the instrument's size precision
    #[rstest]
    #[case("1", "0.03", "33.333333", "0.03")]
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
            order.status.as_ref().unwrap(),
            order.order_type.unwrap(),
            &instrument,
            AccountId::from("POLY-001"),
            UnixNanos::from(1_000_000_000u64),
            UnixNanos::from(2_000_000_000u64),
        )
        .unwrap();

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
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();

        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-FOK-IN-FLIGHT");
        pending_submits.insert(venue_order_id, client_order_id);
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
        // No context registered, so the order surfaces as a report for reconciliation
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
        assert_eq!(report.order_side, Some(OrderSide::Buy));
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
    fn test_admit_stream_taker_trade_builds_the_leg() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let instrument_id = instrument.id();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &OrderFillTrackerMap::new(),
            settlement: &settlement,
            pending_submits: &PendingSubmitTracker::default(),
            order_contexts: &OrderContextRegistry::default(),
            emitter: &test_emitter(),
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let admitted =
            admit_trade_evidence(TradeEvidence::Stream(&trade), &ctx.admission_context())
                .expect("owned taker trade admits");

        assert_eq!(admitted.venue_trade_id, trade.id);
        assert_eq!(admitted.status, trade.status);
        assert!(admitted.has_economics());
        assert_eq!(admitted.legs.len(), 1);
        let leg = &admitted.legs[0];
        assert_eq!(
            leg.venue_order_id,
            VenueOrderId::from(trade.taker_order_id.as_str())
        );
        assert_eq!(leg.trade_id.as_str(), trade.id);
        assert_eq!(leg.instrument_id, instrument_id);
        assert_eq!(leg.order_side, OrderSide::Buy);
        assert_eq!(leg.liquidity_side, LiquiditySide::Taker);
        assert_eq!(leg.last_qty.as_decimal(), dec!(25));
        assert_eq!(leg.last_px.as_decimal(), dec!(0.5));
        // The venue match time, not the message timestamp or the local clock
        assert_eq!(leg.ts_event, UnixNanos::from(1_704_067_200_000_000_000u64));
    }

    fn admission_context_for(
        token_instruments: &AtomicMap<Ustr, InstrumentAny>,
    ) -> AdmissionContext<'_> {
        AdmissionContext {
            signer_type: PolymarketSignerType::Owner,
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: get_pusd_currency(),
            instruments: token_instruments,
        }
    }

    #[rstest]
    fn test_admit_stream_fill_without_venue_match_time_is_untimestamped() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.match_time = String::new();
        trade.timestamp = "not-a-timestamp".to_string();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument_for_trade(&trade));

        let result = admit_trade_evidence(
            TradeEvidence::Stream(&trade),
            &admission_context_for(&token_instruments),
        );

        let Err(AdmissionError::Untimestamped(e)) = result else {
            panic!("expected untimestamped evidence, was {result:?}");
        };

        assert_eq!(
            e.to_string(),
            format!(
                "trade {} has no valid venue match timestamp (match_time=)",
                trade.id
            )
        );
    }

    #[rstest]
    fn test_admit_stream_failed_trade_without_match_time_carries_no_economics() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Failed;
        trade.match_time = String::new();
        trade.timestamp = "not-a-timestamp".to_string();
        let instrument = instrument_for_trade(&trade);
        let instrument_id = instrument.id();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let admitted = admit_trade_evidence(
            TradeEvidence::Stream(&trade),
            &admission_context_for(&token_instruments),
        )
        .expect("failed evidence admits owned-leg identity");

        assert!(!admitted.has_economics());
        assert_eq!(admitted.legs.len(), 1);
        let leg = &admitted.legs[0];
        assert_eq!(
            leg.venue_order_id,
            VenueOrderId::from(trade.taker_order_id.as_str())
        );
        assert_eq!(leg.instrument_id, instrument_id);
        assert_eq!(leg.last_qty.as_decimal(), dec!(0));
        assert_eq!(leg.last_px.as_decimal(), dec!(0));
        assert!(leg.commission.is_zero());
        assert_eq!(leg.ts_event, UnixNanos::default());
    }

    #[rstest]
    fn test_admit_price_keeps_instrument_precision_for_stream_and_wire_precision_for_rest() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let price_precision = instrument.price_precision();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);
        let mut rest: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        rest.id = trade.id.clone();
        rest.taker_order_id = trade.taker_order_id.clone();
        let ctx = admission_context_for(&token_instruments);

        let stream = admit_trade_evidence(TradeEvidence::Stream(&trade), &ctx)
            .expect("stream evidence admits");
        let rest =
            admit_trade_evidence(TradeEvidence::Rest(&rest), &ctx).expect("REST evidence admits");

        assert_eq!(price_precision, 4);
        assert_eq!(stream.legs[0].last_px.as_decimal(), dec!(0.5));
        assert_eq!(stream.legs[0].last_px.precision, price_precision);
        assert_eq!(rest.legs[0].last_px, stream.legs[0].last_px);
        assert_eq!(rest.legs[0].last_px.precision, 1);
    }

    #[rstest]
    #[case::taker_order_id(false, "invalid venue order ID")]
    #[case::maker_trade_id(true, "invalid trade ID source")]
    fn test_admit_stream_rejects_invalid_identifiers_without_panicking(
        #[case] maker: bool,
        #[case] expected_error: &str,
    ) {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument_for_trade(&trade));
        let maker_address = trade.maker_orders[0].maker_address.clone();

        let user_address = if maker {
            trade.trader_side = PolymarketLiquiditySide::Maker;
            trade.id = "trade-\u{e9}".to_string();
            maker_address.as_str()
        } else {
            trade.taker_order_id = "order-\u{e9}".to_string();
            "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266"
        };

        let ctx = AdmissionContext {
            signer_type: PolymarketSignerType::Owner,
            user_address,
            api_key: "ffffffff-ffff-ffff-ffff-ffffffffffff",
            pusd: get_pusd_currency(),
            instruments: &token_instruments,
        };

        let result = admit_trade_evidence(TradeEvidence::Stream(&trade), &ctx);

        let Err(AdmissionError::Invalid(e)) = result else {
            panic!("expected invalid evidence, was {result:?}");
        };

        assert!(
            format!("{e:#}").contains(expected_error),
            "unexpected error: {e:#}"
        );
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
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
    fn test_dispatch_order_message_ignores_missing_lifecycle_fields() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let venue_order_id = VenueOrderId::from(order.id.as_str());

        let mut missing_status = order.clone();
        missing_status.status = None;
        dispatch_user_message(
            &UserWsMessage::Order(missing_status),
            &ctx,
            &mut WsDispatchState::default(),
        );
        assert!(!fill_tracker.has_pending_report(&venue_order_id));

        let mut missing_order_type = order;
        missing_order_type.order_type = None;
        dispatch_user_message(
            &UserWsMessage::Order(missing_order_type),
            &ctx,
            &mut WsDispatchState::default(),
        );
        assert!(!fill_tracker.has_pending_report(&venue_order_id));
    }

    #[rstest]
    fn test_dispatch_order_message_uses_pending_submit_client_order_id() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-UNKNOWN-SUBMIT");
        pending_submits.insert(venue_order_id, client_order_id);
        register_context(
            &order_contexts,
            venue_order_id,
            test_instrument().id(),
            "O-UNKNOWN-SUBMIT",
        );

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
    #[case(PolymarketSignerType::Owner, 1)]
    #[case(PolymarketSignerType::Session, 0)]
    fn test_dispatch_maker_fill_owned_by_case_variant_address(
        #[case] signer_type: PolymarketSignerType,
        #[case] expected_fills: usize,
    ) {
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
        token_instruments.insert(trade.maker_orders[0].asset_id, instrument_for_trade(&trade));
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: &configured_address,
            user_api_key: foreign_api_key,
        };
        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fills = fill_tracker.pending_fills_for(&venue_order_id);
        assert_eq!(fills.len(), expected_fills);

        for fill in fills {
            assert_eq!(fill.venue_order_id, venue_order_id);
        }
    }

    #[rstest]
    #[case(dec!(-1))]
    #[case(Decimal::MAX)]
    fn test_dispatch_maker_numeric_failure_quarantines_trade(#[case] invalid_amount: Decimal) {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let configured_address = trade.maker_orders[0].maker_address.clone();
        let foreign_api_key = "ffffffff-ffff-ffff-ffff-ffffffffffff";
        assert_ne!(trade.maker_orders[0].owner, foreign_api_key);

        let venue_order_id = VenueOrderId::from(trade.maker_orders[0].order_id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.maker_orders[0].asset_id, instrument_for_trade(&trade));
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: &configured_address,
            user_api_key: foreign_api_key,
        };

        let mut state = WsDispatchState::default();

        let mut invalid_trade = trade.clone();
        invalid_trade.maker_orders[0].matched_amount = invalid_amount;
        dispatch_user_message(&UserWsMessage::Trade(invalid_trade), &ctx, &mut state);
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 0);
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(
            fill_tracker.pending_fills_for(&venue_order_id).is_empty(),
            "invalid stream evidence quarantines; a later valid copy stays REST-gated"
        );
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );
    }

    #[rstest]
    fn test_dispatch_trade_dedup() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.note_order_submitted(VenueOrderId::from(trade.taker_order_id.as_str()));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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
    fn test_dispatch_taker_commission_failure_quarantines_trade() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let valid_instrument = instrument_for_trade(&trade);
        let mut invalid_instrument = valid_instrument.clone();
        set_taker_fee_rate(
            &mut invalid_instrument,
            Decimal::from_i128_with_scale(100_000_000_000_000_000_000_000_000i128, 0),
        );

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, invalid_instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            valid_instrument.id(),
            valid_instrument.size_precision(),
            valid_instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            valid_instrument.id(),
            "O-COMMISSION-REPLAY",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();

        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(failed.is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::zero(valid_instrument.size_precision()))
        );
        assert!(receiver.try_recv().is_err());

        // Matching evidence after quarantine remains REST-gated: no emission.
        token_instruments.insert(trade.asset_id, valid_instrument);
        let replay = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(replay.is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_dispatch_untimestamped_trade_quarantines_and_requests_resolution() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.match_time = String::new();
        trade.timestamp = "not-a-timestamp".to_string();
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument_for_trade(&trade));
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(VenueOrderId::from(trade.taker_order_id.as_str()));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();

        let refresh = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        assert!(refresh.is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );
        assert_eq!(settlement.pending_resolutions(), vec![trade.id]);
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_dispatch_trade_replays_after_instrument_becomes_available() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let token_instruments = AtomicMap::new();
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.note_order_submitted(VenueOrderId::from(trade.taker_order_id.as_str()));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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
    fn test_dispatch_trade_applies_mined_and_retrying_provisionally(
        #[case] status: crate::common::enums::PolymarketTradeStatus,
    ) {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = status;
        let instrument = instrument_for_trade(&trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.note_order_submitted(VenueOrderId::from(trade.taker_order_id.as_str()));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = trade.id.clone();

        let result = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert!(result.is_none());
        assert_eq!(fill_tracker.pending_fills_for(&venue_order_id).len(), 1);
        assert_eq!(
            settlement_state(&settlement, &trade_id),
            Some(SettlementState::Provisional)
        );
    }

    #[rstest]
    fn test_dispatch_matched_fill_confirmed_by_rest_at_wire_precision_does_not_conflict() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = PolymarketTradeStatus::Matched;
        let instrument = instrument_for_trade(&trade);
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-MATCHED-CONFIRMED",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected matched fill, was {other:?}"),
        };

        settlement.observe_fill_applied(&filled);

        let mut rest_confirmed: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        rest_confirmed.id = trade.id.clone();
        rest_confirmed.taker_order_id = trade.taker_order_id.clone();
        let rest_evidence = admit_trade_evidence(
            TradeEvidence::Rest(&rest_confirmed),
            &ctx.admission_context(),
        )
        .expect("REST CONFIRMED evidence admits");
        let rest_actions = settlement.admit_rest_result(&rest_evidence);
        execute_settlement_actions(
            rest_actions,
            rest_trade_info(&rest_confirmed).as_ref(),
            &ctx,
            &mut state,
        );

        assert_eq!(rest_evidence.legs[0].last_px, filled.last_px);
        assert_ne!(
            rest_evidence.legs[0].last_px.precision,
            filled.last_px.precision
        );
        assert!(trade_hard_fault(&settlement, &trade.id).is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::RestConfirmed)
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(filled.last_qty)
        );
    }

    #[rstest]
    #[case::open_order(PolymarketOrderStatus::Live, None, true, false)]
    #[case::confirmed_fill(
        PolymarketOrderStatus::Matched,
        Some(PolymarketTradeStatus::Confirmed),
        true,
        true
    )]
    #[case::pending_trade(
        PolymarketOrderStatus::Matched,
        Some(PolymarketTradeStatus::Matched),
        false,
        false
    )]
    #[case::matched_before_trades_listed(PolymarketOrderStatus::Matched, None, false, false)]
    fn test_apply_uncertain_order_evidence(
        #[case] order_status: PolymarketOrderStatus,
        #[case] trade_status: Option<PolymarketTradeStatus>,
        #[case] expected_applied: bool,
        #[case] expected_fill: bool,
    ) {
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
                .expect("REST open order fixture");
        order.status = order_status;
        order.original_size = dec!(25);

        order.size_matched = if order_status == PolymarketOrderStatus::Matched {
            dec!(25)
        } else {
            Decimal::ZERO
        };

        let mut trade: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        trade.status = trade_status.unwrap_or(PolymarketTradeStatus::Confirmed);
        let trades = trade_status
            .map(|_| vec![trade.clone()])
            .unwrap_or_default();
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-UNCERTAIN");
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, client_order_id);
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.begin_session();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();

        let applied =
            apply_uncertain_order_evidence(venue_order_id, &order, &trades, &ctx, &mut state);

        let mut events = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }

        let accepted = events.iter().filter(|event| {
            matches!(event, ExecutionEvent::Order(OrderEventAny::Accepted(accepted))
                if accepted.client_order_id == client_order_id
                    && accepted.venue_order_id == venue_order_id)
        });

        let fills: Vec<&OrderFilled> = events
            .iter()
            .filter_map(|event| match event {
                ExecutionEvent::Order(OrderEventAny::Filled(filled)) => Some(filled),
                _ => None,
            })
            .collect();

        assert_eq!(applied, expected_applied);
        assert_eq!(accepted.count(), usize::from(expected_applied));
        assert_eq!(fill_tracker.contains(&venue_order_id), expected_applied);

        if expected_fill {
            assert_eq!(fills.len(), 1);
            assert_eq!(fills[0].client_order_id, client_order_id);
            assert_eq!(fills[0].venue_order_id, venue_order_id);
            assert_eq!(fills[0].trade_id, TradeId::from(trade.id.as_str()));
            assert_eq!(fills[0].last_qty.as_decimal(), dec!(25));
            assert_eq!(fills[0].last_px.as_decimal(), dec!(0.5));
            assert_eq!(
                settlement_state(&settlement, &trade.id),
                Some(SettlementState::RestConfirmed)
            );
        } else {
            assert!(fills.is_empty());
        }

        assert_eq!(events.len(), usize::from(expected_applied) + fills.len());
    }

    #[rstest]
    fn test_apply_rest_trade_evidence_closes_ioc_remainder_on_confirmation() {
        let trade: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.0000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(&order_contexts, venue_order_id, instrument.id(), "O-IOC");
        let mut context = order_contexts
            .get(&venue_order_id)
            .expect("registered context");
        context.time_in_force = TimeInForce::Ioc;
        order_contexts.register_context(venue_order_id, context);
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.begin_session();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        apply_rest_trade_evidence(&trade, &ctx, &mut state);

        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected REST-confirmed fill, was {other:?}"),
        };

        let canceled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Canceled(event)) => event,
            other => panic!("expected IOC remainder cancel, was {other:?}"),
        };

        assert_eq!(filled.last_qty.as_decimal(), dec!(25));
        assert_eq!(filled.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(canceled.client_order_id, ClientOrderId::from("O-IOC"));
        assert_eq!(canceled.venue_order_id, Some(venue_order_id));
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::RestConfirmed)
        );
    }

    #[rstest]
    fn test_apply_uncertain_order_evidence_normalizes_filled_fok_dust() {
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
                .expect("REST open order fixture");
        order.status = PolymarketOrderStatus::Matched;
        order.original_size = dec!(25.005);
        order.size_matched = dec!(25);
        let trade: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-FOK");
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, client_order_id);
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        let mut context = order_contexts
            .get(&venue_order_id)
            .expect("registered context");
        context.time_in_force = TimeInForce::Fok;
        order_contexts.register_context(venue_order_id, context);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.begin_session();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        let applied = apply_uncertain_order_evidence(
            venue_order_id,
            &order,
            std::slice::from_ref(&trade),
            &ctx,
            &mut state,
        );

        let mut events = Vec::new();

        while let Ok(ExecutionEvent::Order(event)) = receiver.try_recv() {
            events.push(event);
        }

        assert!(applied);
        assert_eq!(events.len(), 3, "unexpected events: {events:?}");
        assert!(matches!(&events[0], OrderEventAny::Accepted(_)));

        let OrderEventAny::Filled(filled) = &events[1] else {
            panic!("expected fill, was {:?}", events[1]);
        };

        let OrderEventAny::Updated(updated) = &events[2] else {
            panic!("expected terminal quantity update, was {:?}", events[2]);
        };

        assert_eq!(filled.last_qty.as_decimal(), dec!(25));
        assert_eq!(updated.client_order_id, client_order_id);
        assert_eq!(updated.quantity.as_decimal(), dec!(25));
    }

    #[rstest]
    #[case::identical_repeat(dec!(25), dec!(50))]
    #[case::contradicting_repeat(dec!(10), dec!(25))]
    fn test_apply_uncertain_order_evidence_deduplicates_repeated_rows(
        #[case] repeat_size: Decimal,
        #[case] size_matched: Decimal,
    ) {
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
                .expect("REST open order fixture");
        order.status = PolymarketOrderStatus::Matched;
        order.original_size = size_matched;
        order.size_matched = size_matched;
        let trade: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        let mut repeat = trade.clone();
        repeat.size = repeat_size;
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, ClientOrderId::from("O-UNCERTAIN"));
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-UNCERTAIN",
        );
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.begin_session();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        let applied = apply_uncertain_order_evidence(
            venue_order_id,
            &order,
            &[trade, repeat],
            &ctx,
            &mut state,
        );

        assert!(!applied);
        assert!(!fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    #[case::other_venue_order_id(Some("0xother"), PolymarketOrderSide::Buy)]
    #[case::contradicting_side(None, PolymarketOrderSide::Sell)]
    fn test_apply_uncertain_order_evidence_rejects_contradicting_row(
        #[case] row_id: Option<&str>,
        #[case] row_side: PolymarketOrderSide,
    ) {
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
                .expect("REST open order fixture");
        let venue_order_id = VenueOrderId::from(order.id.as_str());

        if let Some(row_id) = row_id {
            order.id = row_id.to_string();
        }

        order.side = row_side;
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, ClientOrderId::from("O-UNCERTAIN"));
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-UNCERTAIN",
        );
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        let applied = apply_uncertain_order_evidence(venue_order_id, &order, &[], &ctx, &mut state);

        assert!(!applied);
        assert!(!fill_tracker.contains(&venue_order_id));
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_apply_stream_gap_order_evidence_applies_only_unseen_trades() {
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
                .expect("REST open order fixture");
        order.status = PolymarketOrderStatus::Canceled;
        order.original_size = dec!(100);
        order.size_matched = dec!(50);
        let seen: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        let mut unseen = seen.clone();
        unseen.id = "trade-0xunseen".to_string();
        let trades = vec![seen.clone(), unseen.clone()];
        let ws_trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&ws_trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(order.asset_id, instrument.clone());
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-STREAM-GAP");
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            venue_order_id,
            Quantity::from("100.0000"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, client_order_id);
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.begin_session();

        // A fill rebuilt from the cache at a snapped quantity that REST would contradict
        settlement.hydrate_fill(&OrderFilled::new(
            TraderId::from("TESTER-001"),
            StrategyId::from("S-001"),
            instrument.id(),
            client_order_id,
            venue_order_id,
            AccountId::from("POLY-001"),
            TradeId::from(seen.id.as_str()),
            OrderSide::Buy,
            OrderType::Limit,
            Quantity::from("24.9900"),
            Price::from("0.5000"),
            Currency::pUSD(),
            LiquiditySide::Taker,
            UUID4::new(),
            UnixNanos::from(1_u64),
            UnixNanos::from(1_u64),
            false,
            None,
            None,
            None,
        ));

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();

        let applied =
            apply_stream_gap_order_evidence(venue_order_id, &order, &trades, &ctx, &mut state);

        let mut events = Vec::new();

        while let Ok(event) = receiver.try_recv() {
            events.push(event);
        }

        let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = &events[0] else {
            panic!("expected the unseen trade's fill, was {:?}", events[0]);
        };

        assert!(applied);
        assert_eq!(events.len(), 1);
        assert_eq!(filled.client_order_id, client_order_id);
        assert_eq!(filled.venue_order_id, venue_order_id);
        assert_eq!(filled.trade_id, TradeId::from(unseen.id.as_str()));
        assert_eq!(filled.last_qty.as_decimal(), dec!(25));
        assert_eq!(filled.last_px.as_decimal(), dec!(0.5));
        assert_eq!(
            settlement_state(&settlement, &unseen.id),
            Some(SettlementState::RestConfirmed)
        );
        assert_eq!(
            settlement_state(&settlement, &seen.id),
            Some(SettlementState::Provisional)
        );
        assert!(trade_hard_fault(&settlement, &seen.id).is_none());
    }

    #[rstest]
    fn test_dispatch_matched_trade_emits_fill_and_failed_trade_quarantines_then_rest_voids_it() {
        let mut trade: PolymarketUserTrade = load("ws_user_trade.json");
        trade.status = crate::common::enums::PolymarketTradeStatus::Matched;
        let instrument = instrument_for_trade(&trade);
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-MATCHED-FAILED",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        // MATCHED applies once for a current-session order.
        let matched = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let filled = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::Filled(event)) => event,
            other => panic!("expected matched fill, was {other:?}"),
        };

        assert!(matched.is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Provisional)
        );

        // The applied fill resolves the pending application through observation.
        settlement.observe_fill_applied(&filled);

        // A stream FAILED update quarantines; it never voids directly.
        trade.status = crate::common::enums::PolymarketTradeStatus::Failed;
        let failed = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        assert!(failed.is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::Quarantined)
        );
        assert!(receiver.try_recv().is_err());
        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(filled.last_qty)
        );

        // A targeted terminal REST FAILED result voids the observed fill once.
        let mut rest_failed: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
                .expect("REST trade fixture");
        rest_failed.id = trade.id.clone();
        rest_failed.taker_order_id = trade.taker_order_id.clone();
        rest_failed.status = PolymarketTradeStatus::Failed;
        rest_failed.trader_side = PolymarketLiquiditySide::Taker;
        let rest_evidence =
            admit_trade_evidence(TradeEvidence::Rest(&rest_failed), &ctx.admission_context())
                .expect("REST FAILED evidence admits");
        let rest_actions = settlement.admit_rest_result(&rest_evidence);
        execute_settlement_actions(
            rest_actions,
            rest_trade_info(&rest_failed).as_ref(),
            &ctx,
            &mut state,
        );

        let voided = match receiver.try_recv().unwrap() {
            ExecutionEvent::Order(OrderEventAny::FillVoided(event)) => event,
            other => panic!("expected failed fill correction, was {other:?}"),
        };

        assert!(settlement.take_account_refresh());
        assert!(trade_hard_fault(&settlement, &trade.id).is_none());
        assert_eq!(
            settlement_state(&settlement, &trade.id),
            Some(SettlementState::RestFailed)
        );
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

        // A stale MATCHED update after the terminal REST outcome is absorbed as a tombstone.
        trade.status = PolymarketTradeStatus::Matched;
        let stale_trade_id = trade.id.clone();
        let matched_after_failure =
            dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);
        assert!(matched_after_failure.is_none());
        assert_eq!(
            settlement_state(&settlement, &stale_trade_id),
            Some(SettlementState::RestFailed)
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_dispatch_trade_uses_pending_submit_client_order_id() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);

        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();

        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let client_order_id = ClientOrderId::from("O-UNKNOWN-FILL");
        pending_submits.insert(venue_order_id, client_order_id);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        let _ = dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        let fills = fill_tracker.pending_fills_for(&venue_order_id);
        assert_eq!(fills[0].client_order_id, Some(client_order_id));
    }

    #[rstest]
    fn test_dispatch_late_fill_stays_tracked_after_later_registrations() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let market: GammaMarket = load("gamma_market_sports_market_money_line.json");
        let defs = parse_gamma_market(&market).unwrap();
        let instrument = bind_instrument(
            create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap(),
            trade.market.as_str(),
            trade.outcome,
        );
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-LATE-FILL",
        );
        order_contexts.mark_accepted(venue_order_id);
        assert!(order_contexts.get(&venue_order_id).is_some());

        for index in 0..10_000 {
            let later_venue_order_id = VenueOrderId::from(format!("V-LATER-{index}").as_str());
            let later_client_order_id = format!("O-LATER-{index}");
            register_context(
                &order_contexts,
                later_venue_order_id,
                instrument.id(),
                &later_client_order_id,
            );
            order_contexts.mark_accepted(later_venue_order_id);
            fill_tracker.register(
                later_venue_order_id,
                Quantity::from("1"),
                OrderSide::Sell,
                instrument.id(),
                instrument.size_precision(),
                instrument.price_precision(),
            );
        }
        assert!(order_contexts.get(&venue_order_id).is_some());
        assert!(fill_tracker.contains(&venue_order_id));

        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);

        let event = receiver.try_recv().expect("expected tracked late fill");
        let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = event else {
            panic!("expected tracked OrderFilled after later registrations, was {event:?}");
        };

        assert_eq!(filled.client_order_id, ClientOrderId::from("O-LATE-FILL"));
        assert_eq!(filled.venue_order_id, venue_order_id);
        assert_eq!(filled.trade_id, TradeId::from(trade.id.as_str()));
        assert_eq!(filled.instrument_id, instrument.id());
        assert_eq!(
            filled.last_qty.as_decimal(),
            Decimal::from_str_exact(&trade.size).unwrap()
        );
        assert_eq!(
            filled.last_px.as_decimal(),
            Decimal::from_str_exact(&trade.price).unwrap()
        );
        assert_eq!(filled.order_side, OrderSide::Buy);
        assert_eq!(filled.liquidity_side, LiquiditySide::Taker);
        let commission = filled.commission.expect("tracked fill has commission");
        assert_eq!(commission.as_decimal(), dec!(0.1875));
        assert_eq!(commission.currency, Currency::pUSD());
        assert!(receiver.try_recv().is_err());
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
        // No context registered, so the order surfaces as a report (the external/reconciliation
        // fallback), where filled_qty is capped to tracked fills.
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
        // No context registered, so the order surfaces as a report (the external/reconciliation
        // fallback), where filled_qty is capped to tracked fills.
        let order_contexts = OrderContextRegistry::default();
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-MATCHED",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let clock = Box::leak(Box::new(AtomicTime::new(
            false,
            UnixNanos::from(2_000_000_000u64),
        )));

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock,
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        let _ = settlement.admit_rest_result(&AdmittedTrade {
            venue_trade_id: "trade-0xfill1".to_string(),
            status: PolymarketTradeStatus::Confirmed,
            legs: Vec::new(),
        });

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
        let instrument = instrument_for_trade(&trade);
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-CONFIRMED-DUST",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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
        let instrument = instrument_for_trade(&trade);

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
        let order_contexts = OrderContextRegistry::default();
        register_context(&order_contexts, venue_order_id, instrument.id(), "O-CANCEL");
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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
    fn test_modified_old_leg_suppresses_cancel_but_still_emits_late_fill() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument.clone());

        let fill_tracker = OrderFillTrackerMap::new();
        let old_venue_order_id = VenueOrderId::from(cancel_order.id.as_str());
        fill_tracker.register(
            old_venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );

        let client_order_id = ClientOrderId::from("O-MODIFIED-OLD-LEG");
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            old_venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        order_contexts.mark_accepted(old_venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.note_order_submitted(old_venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();
        let replacement_venue_order_id = VenueOrderId::from("0xreplacement");
        assert!(state.begin_modify(client_order_id, old_venue_order_id, instrument.id()));
        assert!(state.set_modify_replacement(
            client_order_id,
            replacement_venue_order_id,
            Quantity::from("100"),
            Quantity::from("100"),
            Price::from("0.5"),
        ));
        assert!(
            state
                .claim_modify_replacement(replacement_venue_order_id)
                .is_some()
        );

        dispatch_user_message(&UserWsMessage::Order(cancel_order), &ctx, &mut state);
        assert!(receiver.try_recv().is_err());

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        match receiver.try_recv().expect("expected late old-leg fill") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
                assert_eq!(fill.client_order_id, client_order_id);
                assert_eq!(fill.venue_order_id, old_venue_order_id);
            }
            other => panic!("expected late old-leg fill, was {other:?}"),
        }

        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_pending_modify_replacement_ws_activity_emits_updated_without_accepted() {
        let mut replacement: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let old_venue_order_id = VenueOrderId::from("0xold-modify-leg");
        let replacement_venue_order_id = VenueOrderId::from("0xreplacement-modify-leg");
        replacement.id = replacement_venue_order_id.to_string();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(replacement.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            old_venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let client_order_id = ClientOrderId::from("O-PENDING-MODIFY");
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            old_venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        order_contexts.mark_accepted(old_venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };

        let mut state = WsDispatchState::default();
        assert!(state.begin_modify(client_order_id, old_venue_order_id, instrument.id()));
        assert!(state.set_modify_replacement(
            client_order_id,
            replacement_venue_order_id,
            Quantity::from("120"),
            Quantity::from("100"),
            Price::from("0.5"),
        ));

        let original_context = order_contexts.get(&old_venue_order_id).unwrap();

        dispatch_user_message(&UserWsMessage::Order(replacement), &ctx, &mut state);

        match receiver.try_recv().expect("expected replacement update") {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.client_order_id, client_order_id);
                assert_eq!(updated.venue_order_id, Some(replacement_venue_order_id));
                assert_eq!(updated.quantity, Quantity::from("120"));
                assert_eq!(updated.price, Some(Price::from("0.5")));
            }
            other => panic!("expected replacement update, was {other:?}"),
        }

        assert_eq!(
            order_contexts.venue_order_id(&client_order_id),
            Some(replacement_venue_order_id)
        );
        assert_eq!(
            order_contexts.get(&old_venue_order_id),
            Some(original_context)
        );
        assert_eq!(
            order_contexts.get(&replacement_venue_order_id),
            Some(OrderContext {
                quantity: Quantity::from("120"),
                price: Some(Price::from("0.5")),
                ..original_context
            })
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_pending_modify_replacement_ws_rejection_emits_once_and_closes_old_leg() {
        let mut replacement: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let old_venue_order_id = VenueOrderId::from("0xold-rejected-modify-leg");
        let replacement_venue_order_id = VenueOrderId::from("0xrejected-replacement-modify-leg");
        replacement.id = replacement_venue_order_id.to_string();
        replacement.status = Some(PolymarketUserOrderStatus::new(
            PolymarketOrderStatus::Unmatched,
            Some("replacement rejected"),
        ));

        let token_instruments = AtomicMap::new();
        token_instruments.insert(replacement.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            old_venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let client_order_id = ClientOrderId::from("O-REJECTED-MODIFY");
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            old_venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        order_contexts.mark_accepted(old_venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };

        let mut state = WsDispatchState::default();
        assert!(state.begin_modify(client_order_id, old_venue_order_id, instrument.id()));
        let cancel_ts = UnixNanos::from(123);
        assert!(state.confirm_modify_cancel(client_order_id, old_venue_order_id, cancel_ts,));
        assert!(state.set_modify_replacement(
            client_order_id,
            replacement_venue_order_id,
            Quantity::from("120"),
            Quantity::from("100"),
            Price::from("0.5"),
        ));

        dispatch_user_message(&UserWsMessage::Order(replacement), &ctx, &mut state);

        match receiver.try_recv().expect("expected modify rejection") {
            ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => {
                assert_eq!(rejected.client_order_id, client_order_id);
                assert_eq!(rejected.venue_order_id, Some(old_venue_order_id));
                assert_eq!(rejected.reason, "replacement rejected");
            }
            other => panic!("expected modify rejection, was {other:?}"),
        }

        match receiver.try_recv().expect("expected old-leg cancellation") {
            ExecutionEvent::Order(OrderEventAny::Canceled(canceled)) => {
                assert_eq!(canceled.client_order_id, client_order_id);
                assert_eq!(canceled.venue_order_id, Some(old_venue_order_id));
                assert_eq!(canceled.ts_event, cancel_ts);
            }
            other => panic!("expected old-leg cancellation, was {other:?}"),
        }

        assert!(
            state
                .pending_modify_promotion(replacement_venue_order_id)
                .is_none()
        );
        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_pending_modify_replacement_fill_promotes_before_fill() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let old_venue_order_id = VenueOrderId::from("0xold-fill-leg");
        let replacement_venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.register(
            old_venue_order_id,
            Quantity::from("100"),
            OrderSide::Buy,
            instrument.id(),
            instrument.size_precision(),
            instrument.price_precision(),
        );
        let client_order_id = ClientOrderId::from("O-PENDING-MODIFY-FILL");
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            old_venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        order_contexts.mark_accepted(old_venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.note_order_submitted(replacement_venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();
        assert!(state.begin_modify(client_order_id, old_venue_order_id, instrument.id()));
        assert!(state.set_modify_replacement(
            client_order_id,
            replacement_venue_order_id,
            Quantity::from("120"),
            Quantity::from("100"),
            Price::from("0.5"),
        ));
        let mut cancellation: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        cancellation.id = replacement_venue_order_id.to_string();
        let cancellation_report = build_ws_order_status_report(
            &cancellation,
            cancellation.status.as_ref().unwrap(),
            cancellation.order_type.unwrap(),
            &instrument,
            ctx.account_id,
            UnixNanos::from(2_000_000_000),
            UnixNanos::from(3_000_000_000),
        )
        .unwrap();
        assert!(
            fill_tracker
                .accept_or_buffer_report(replacement_venue_order_id, cancellation_report)
                .is_none()
        );

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        match receiver.try_recv().expect("expected replacement update") {
            ExecutionEvent::Order(OrderEventAny::Updated(updated)) => {
                assert_eq!(updated.client_order_id, client_order_id);
                assert_eq!(updated.venue_order_id, Some(replacement_venue_order_id));
                assert_eq!(updated.quantity, Quantity::from("120"));
            }
            other => panic!("expected replacement update, was {other:?}"),
        }

        match receiver.try_recv().expect("expected replacement fill") {
            ExecutionEvent::Order(OrderEventAny::Filled(fill)) => {
                assert_eq!(fill.client_order_id, client_order_id);
                assert_eq!(fill.venue_order_id, replacement_venue_order_id);
            }
            other => panic!("expected replacement fill, was {other:?}"),
        }

        match receiver.try_recv().expect("expected replacement cancel") {
            ExecutionEvent::Order(OrderEventAny::Canceled(cancel)) => {
                assert_eq!(cancel.client_order_id, client_order_id);
                assert_eq!(cancel.venue_order_id, Some(replacement_venue_order_id));
            }
            other => panic!("expected replacement cancel, was {other:?}"),
        }

        assert!(receiver.try_recv().is_err());
    }

    #[rstest]
    fn test_late_modify_completion_does_not_finish_newer_modify() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let client_order_id = ClientOrderId::from("O-MODIFY-GENERATION");
        let old_venue_order_id = VenueOrderId::from("0xmodify-generation-old");
        let first_replacement_venue_order_id = VenueOrderId::from("0xmodify-generation-first");
        let second_replacement_venue_order_id = VenueOrderId::from("0xmodify-generation-second");
        let mut state = WsDispatchState::default();

        assert!(state.begin_modify(client_order_id, old_venue_order_id, instrument_id));
        assert!(state.set_modify_replacement(
            client_order_id,
            first_replacement_venue_order_id,
            Quantity::from("12"),
            Quantity::from("12"),
            Price::from("0.5"),
        ));
        assert!(
            state
                .claim_modify_replacement(first_replacement_venue_order_id)
                .is_some()
        );
        assert!(state.begin_modify(
            client_order_id,
            first_replacement_venue_order_id,
            instrument_id,
        ));
        assert!(state.set_modify_replacement(
            client_order_id,
            second_replacement_venue_order_id,
            Quantity::from("15"),
            Quantity::from("15"),
            Price::from("0.6"),
        ));

        assert!(
            state
                .finish_modify_without_replacement(
                    client_order_id,
                    old_venue_order_id,
                    true,
                    UnixNanos::from(123),
                )
                .is_none()
        );
        let promotion = state
            .pending_modify_promotion(second_replacement_venue_order_id)
            .expect("newer modification must remain pending");
        assert_eq!(promotion.client_order_id, client_order_id);
        assert_eq!(
            promotion.old_venue_order_id,
            first_replacement_venue_order_id
        );
        assert_eq!(promotion.venue_order_id, second_replacement_venue_order_id);
        assert_eq!(promotion.quantity, Quantity::from("15"));
        assert_eq!(promotion.leg_quantity, Quantity::from("15"));
        assert_eq!(promotion.price, Price::from("0.6"));
    }

    #[rstest]
    fn test_pending_modify_lookup_selects_matching_replacement() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let first_client_order_id = ClientOrderId::from("O-MODIFY-LOOKUP-1");
        let second_client_order_id = ClientOrderId::from("O-MODIFY-LOOKUP-2");
        let first_old_venue_order_id = VenueOrderId::from("0xmodify-lookup-old-1");
        let second_old_venue_order_id = VenueOrderId::from("0xmodify-lookup-old-2");
        let first_replacement_venue_order_id = VenueOrderId::from("0xmodify-lookup-new-1");
        let second_replacement_venue_order_id = VenueOrderId::from("0xmodify-lookup-new-2");
        let mut state = WsDispatchState::default();

        assert!(state.begin_modify(
            first_client_order_id,
            first_old_venue_order_id,
            instrument_id,
        ));
        assert!(state.set_modify_replacement(
            first_client_order_id,
            first_replacement_venue_order_id,
            Quantity::from("11"),
            Quantity::from("10"),
            Price::from("0.4"),
        ));
        assert!(state.begin_modify(
            second_client_order_id,
            second_old_venue_order_id,
            instrument_id,
        ));
        assert!(state.set_modify_replacement(
            second_client_order_id,
            second_replacement_venue_order_id,
            Quantity::from("22"),
            Quantity::from("20"),
            Price::from("0.6"),
        ));

        let second = state
            .claim_modify_replacement(second_replacement_venue_order_id)
            .expect("second replacement must be selected");
        assert_eq!(second.client_order_id, second_client_order_id);
        assert_eq!(second.old_venue_order_id, second_old_venue_order_id);
        assert_eq!(second.venue_order_id, second_replacement_venue_order_id);
        assert_eq!(second.quantity, Quantity::from("22"));
        assert_eq!(second.leg_quantity, Quantity::from("20"));
        assert_eq!(second.price, Price::from("0.6"));

        let first = state
            .pending_modify_promotion(first_replacement_venue_order_id)
            .expect("first replacement must remain pending");
        assert_eq!(first.client_order_id, first_client_order_id);
        assert_eq!(first.old_venue_order_id, first_old_venue_order_id);
        assert_eq!(first.venue_order_id, first_replacement_venue_order_id);
        assert_eq!(first.quantity, Quantity::from("11"));
        assert_eq!(first.leg_quantity, Quantity::from("10"));
        assert_eq!(first.price, Price::from("0.4"));
    }

    #[rstest]
    fn test_begin_cancels_is_atomic_when_one_order_conflicts() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let available_client_order_id = ClientOrderId::from("O-CANCEL-AVAILABLE");
        let conflicting_client_order_id = ClientOrderId::from("O-CANCEL-CONFLICT");
        let mut state = WsDispatchState::default();

        assert!(state.begin_modify(
            conflicting_client_order_id,
            VenueOrderId::from("0xmodify-conflict"),
            instrument_id,
        ));
        assert!(!state.begin_cancels(&[
            (available_client_order_id, instrument_id),
            (conflicting_client_order_id, instrument_id),
        ]));
        assert!(state.begin_modify(
            available_client_order_id,
            VenueOrderId::from("0xmodify-available"),
            instrument_id,
        ));
    }

    #[rstest]
    fn test_begin_available_cancels_skips_existing_cancel() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let pending_client_order_id = ClientOrderId::from("O-CANCEL-PENDING");
        let available_client_order_id = ClientOrderId::from("O-CANCEL-AVAILABLE");
        let modifying_client_order_id = ClientOrderId::from("O-MODIFY-PENDING");
        let unreserved_client_order_id = ClientOrderId::from("O-CANCEL-UNRESERVED");
        let mut state = WsDispatchState::default();

        assert!(state.begin_cancels(&[(pending_client_order_id, instrument_id)]));
        assert_eq!(
            state
                .begin_available_cancels(&[
                    (pending_client_order_id, instrument_id),
                    (available_client_order_id, instrument_id),
                ])
                .unwrap(),
            vec![available_client_order_id],
        );
        assert!(!state.begin_modify(
            pending_client_order_id,
            VenueOrderId::from("0xcancel-pending"),
            instrument_id,
        ));
        assert!(!state.begin_modify(
            available_client_order_id,
            VenueOrderId::from("0xcancel-available"),
            instrument_id,
        ));

        state.finish_cancels(&[pending_client_order_id, available_client_order_id]);
        assert!(state.begin_modify(
            modifying_client_order_id,
            VenueOrderId::from("0xmodify-pending"),
            instrument_id,
        ));
        assert!(
            state
                .begin_available_cancels(&[
                    (unreserved_client_order_id, instrument_id),
                    (modifying_client_order_id, instrument_id),
                ])
                .is_none()
        );
        assert!(state.begin_modify(
            unreserved_client_order_id,
            VenueOrderId::from("0xcancel-unreserved"),
            instrument_id,
        ));
    }

    #[rstest]
    fn test_cancel_and_modify_are_mutually_exclusive() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let client_order_id = ClientOrderId::from("O-MODIFY-CANCEL-ALL");
        let other_client_order_id = ClientOrderId::from("O-MODIFY-MARKET-CANCEL");
        let venue_order_id = VenueOrderId::from("0xmodify-cancel-all");
        let mut state = WsDispatchState::default();

        assert!(state.begin_cancels(&[(client_order_id, instrument_id)]));
        assert!(!state.begin_modify(client_order_id, venue_order_id, instrument_id));
        assert!(state.begin_market_cancel(instrument_id));
        assert!(!state.begin_modify(
            other_client_order_id,
            VenueOrderId::from("0xmodify-market-cancel"),
            instrument_id,
        ));
        state.finish_market_cancel(instrument_id);
        state.finish_cancels(&[client_order_id]);
        assert!(state.begin_modify(client_order_id, venue_order_id, instrument_id));
        assert!(!state.begin_cancels(&[(client_order_id, instrument_id)]));
        assert!(!state.begin_market_cancel(instrument_id));
        assert!(state.set_modify_replacement(
            client_order_id,
            VenueOrderId::from("0xmodify-cancel-all-new"),
            Quantity::from("12"),
            Quantity::from("12"),
            Price::from("0.5"),
        ));
        assert!(!state.begin_cancels(&[(client_order_id, instrument_id)]));
        assert!(!state.begin_market_cancel(instrument_id));
        assert!(
            state
                .finish_modify_without_replacement(
                    client_order_id,
                    venue_order_id,
                    false,
                    UnixNanos::default(),
                )
                .is_some()
        );
        assert!(state.begin_market_cancel(instrument_id));
        assert!(!state.begin_modify(client_order_id, venue_order_id, instrument_id));
        state.finish_market_cancel(instrument_id);
    }

    #[rstest]
    fn test_reset_preserves_modify_recovery_and_stale_leg_safety() {
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let pending_client_order_id = ClientOrderId::from("O-PENDING-RESET");
        let pending_old_venue_order_id = VenueOrderId::from("0xpending-old-reset");
        let pending_new_venue_order_id = VenueOrderId::from("0xpending-new-reset");
        let replaced_client_order_id = ClientOrderId::from("O-REPLACED-RESET");
        let replaced_old_venue_order_id = VenueOrderId::from("0xreplaced-old-reset");
        let replaced_new_venue_order_id = VenueOrderId::from("0xreplaced-new-reset");
        let closed_client_order_id = ClientOrderId::from("O-CLOSED-RESET");
        let closed_venue_order_id = VenueOrderId::from("0xclosed-reset");
        let cancel_client_order_id = ClientOrderId::from("O-CANCEL-RESET");
        let trade_id = TradeId::from("T-RESET");
        let mut state = WsDispatchState::default();

        assert!(state.begin_modify(
            pending_client_order_id,
            pending_old_venue_order_id,
            instrument_id,
        ));
        assert!(state.set_modify_replacement(
            pending_client_order_id,
            pending_new_venue_order_id,
            Quantity::from("12"),
            Quantity::from("10"),
            Price::from("0.5"),
        ));
        assert!(state.begin_modify(
            replaced_client_order_id,
            replaced_old_venue_order_id,
            instrument_id,
        ));
        assert!(state.set_modify_replacement(
            replaced_client_order_id,
            replaced_new_venue_order_id,
            Quantity::from("12"),
            Quantity::from("10"),
            Price::from("0.5"),
        ));
        assert!(
            state
                .claim_modify_replacement(replaced_new_venue_order_id)
                .is_some()
        );
        assert!(state.begin_modify(closed_client_order_id, closed_venue_order_id, instrument_id,));
        assert!(
            state
                .finish_modify_without_replacement(
                    closed_client_order_id,
                    closed_venue_order_id,
                    true,
                    UnixNanos::from(1),
                )
                .is_some()
        );
        state.record_reconciled_fill(trade_id, pending_old_venue_order_id);
        assert!(state.begin_cancels(&[(cancel_client_order_id, instrument_id)]));

        state.reset_session();

        assert_eq!(
            state
                .pending_modify_promotion(pending_new_venue_order_id)
                .unwrap()
                .client_order_id,
            pending_client_order_id,
        );
        assert!(state.suppress_modify_cancel(replaced_old_venue_order_id));
        assert!(state.suppress_modify_cancel(closed_venue_order_id));
        assert!(state.suppress_modify_cancel_reemit(pending_old_venue_order_id));
        assert!(state.suppress_modify_cancel_reemit(replaced_old_venue_order_id));
        assert!(!state.suppress_modify_cancel_reemit(closed_venue_order_id));
        assert!(
            state
                .reconciled_fills
                .contains(&(trade_id, pending_old_venue_order_id))
        );
        assert!(state.begin_cancels(&[(cancel_client_order_id, instrument_id)]));
    }

    #[rstest]
    fn test_reconciled_fill_is_not_reapplied_from_websocket() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let token_instruments = AtomicMap::new();
        token_instruments.insert(trade.asset_id, instrument.clone());
        let fill_tracker = OrderFillTrackerMap::new();
        fill_tracker.restore_order(
            venue_order_id,
            Quantity::from("100"),
            Quantity::from("25"),
            OrderSide::Buy,
        );
        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-RECONCILED-FILL",
        );
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);
        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
        };

        let mut state = WsDispatchState::default();
        state.record_reconciled_fill(TradeId::from(trade.id.as_str()), venue_order_id);

        dispatch_user_message(&UserWsMessage::Trade(trade), &ctx, &mut state);

        assert_eq!(
            fill_tracker.get_cumulative_filled(&venue_order_id),
            Some(Quantity::from("25")),
        );
        assert!(receiver.try_recv().is_err());
        // The reconciliation report already delivered the leg, so it does not block reports
        assert!(settlement.ensure_resolved(None, "mass status").is_ok());
    }

    #[rstest]
    fn test_cancel_not_reemitted_when_fill_completes_order() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);

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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-CANCEL-FULL",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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
    fn test_cancel_saved_before_acceptance_and_retained_for_modify_reset() {
        let cancel_order: PolymarketUserOrder = load("ws_user_order_cancellation.json");
        let instrument = test_instrument();
        let instrument_id = instrument.id();

        let token_instruments = AtomicMap::new();
        token_instruments.insert(cancel_order.asset_id, instrument);

        // Fill tracker has NO registration (simulates HTTP still in-flight)
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(cancel_order.id.as_str());

        let pending_submits = PendingSubmitTracker::default();
        let order_contexts = OrderContextRegistry::default();
        let emitter = test_emitter();

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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

        let client_order_id = ClientOrderId::from("O-CANCEL-RESET");
        assert!(state.begin_modify(client_order_id, venue_order_id, instrument_id));
        state.reset_session();
        assert!(
            state
                .finish_modify_without_replacement(
                    client_order_id,
                    venue_order_id,
                    false,
                    UnixNanos::default(),
                )
                .unwrap()
                .1
                .is_some()
        );
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
        terminal_order.status = Some(status.into());
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = instrument_for_trade(&trade);

        let token_instruments = AtomicMap::new();
        token_instruments.insert(terminal_order.asset_id, instrument.clone());

        // No registration: the submit response has not landed
        let fill_tracker = OrderFillTrackerMap::new();
        let venue_order_id = VenueOrderId::from(terminal_order.id.as_str());
        let client_order_id = ClientOrderId::from("O-BUFFERED");

        let pending_submits = PendingSubmitTracker::default();
        pending_submits.insert(venue_order_id, client_order_id);
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            client_order_id.as_str(),
        );
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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

        let instrument = bind_instrument(test_instrument(), "0x4134", PolymarketOutcome::yes());
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
        let order_contexts = OrderContextRegistry::default();
        register_context(&order_contexts, venue_order_id, instrument.id(), "O-3797");
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xabc",
            user_api_key: "xxx",
        };
        let mut state = WsDispatchState::default();

        let make_order =
            |size_matched: &str, ts: &str, event_type: PolymarketEventType| PolymarketUserOrder {
                asset_id,
                associate_trades: None,
                created_at: Some("1775074735".to_string()),
                expiration: Some("0".to_string()),
                id: order_id.clone(),
                maker_address: Some(Ustr::from("0xabc")),
                market: Ustr::from("0x4134"),
                order_owner: Some(Ustr::from("xxx")),
                order_type: Some(PolymarketOrderType::GTC),
                original_size: "20".to_string(),
                outcome: Some(PolymarketOutcome::yes()),
                owner: Ustr::from("xxx"),
                price: "0.18".to_string(),
                side: PolymarketOrderSide::Buy,
                size_matched: size_matched.to_string(),
                status: Some(PolymarketOrderStatus::Canceled.into()),
                timestamp: ts.to_string(),
                event_type,
            };

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
                matched_amount: Decimal::from_str_exact(&matched_amount.to_string())
                    .unwrap_or(Decimal::ZERO),
                order_id: order_id.clone(),
                outcome: PolymarketOutcome::yes(),
                owner: "xxx".to_string(),
                price: Decimal::from_str_exact("0.18").unwrap_or(Decimal::ZERO),
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

        let instrument = bind_instrument(test_instrument(), "0xmarket", PolymarketOutcome::yes());
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-OVERFILL",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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

        let instrument = bind_instrument(test_instrument(), "0xmarket", PolymarketOutcome::yes());
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
        let order_contexts = OrderContextRegistry::default();
        order_contexts.register_context(
            venue_order_id,
            OrderContext {
                identity: OrderIdentity {
                    client_order_id: ClientOrderId::from("O-ONE-SHOT"),
                    strategy_id: StrategyId::from("S-001"),
                    instrument_id: instrument.id(),
                    order_side,
                    order_type,
                },
                quantity: submitted,
                price: Some(Price::from("0.50")),
                trigger_price: None,
                trigger_type: None,
                time_in_force,
                is_post_only: false,
                is_reduce_only: false,
                is_quote_quantity: false,
            },
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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

        let instrument = bind_instrument(test_instrument(), "0xmarket", PolymarketOutcome::yes());
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-GROSS-OVERFILL",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "00000000-0000-0000-0000-000000000001",
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

    // Unmatched -> Rejected (placement never became live); CanceledMarketResolved -> Expired
    // (market settled). Both are tracked own-order terminal states emitted as order events.
    #[rstest]
    #[case(
        crate::common::enums::PolymarketOrderStatus::Unmatched,
        Some("invalid post-only order: order crosses book"),
        "Rejected"
    )]
    #[case(
        crate::common::enums::PolymarketOrderStatus::CanceledMarketResolved,
        None,
        "Expired"
    )]
    fn test_dispatch_order_terminal_status_emits_event(
        #[case] status: crate::common::enums::PolymarketOrderStatus,
        #[case] reason: Option<&str>,
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
        let order_contexts = OrderContextRegistry::default();
        register_context(
            &order_contexts,
            venue_order_id,
            instrument.id(),
            "O-TERMINAL",
        );
        order_contexts.mark_accepted(venue_order_id);
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

        let settlement = SettlementRegistry::new(AccountId::from("POLY-001"));
        settlement.mark_live();
        settlement.note_order_submitted(venue_order_id);

        let ctx = WsDispatchContext {
            signer_type: PolymarketSignerType::Owner,
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            settlement: &settlement,
            pending_submits: &pending_submits,
            order_contexts: &order_contexts,
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
            created_at: Some("1775074735".to_string()),
            expiration: Some("0".to_string()),
            id: order_id,
            maker_address: Some(Ustr::from("0xabc")),
            market: Ustr::from("0x4134"),
            order_owner: Some(Ustr::from("xxx")),
            order_type: Some(PolymarketOrderType::FOK),
            original_size: "10".to_string(),
            outcome: Some(PolymarketOutcome::yes()),
            owner: Ustr::from("xxx"),
            price: "0.50".to_string(),
            side: PolymarketOrderSide::Buy,
            size_matched: "0".to_string(),
            status: Some(PolymarketUserOrderStatus::new(status, reason)),
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

                if let OrderEventAny::Rejected(rejected) = order_event {
                    assert_eq!(
                        rejected.reason,
                        "invalid post-only order: order crosses book"
                    );
                    assert!(rejected.due_post_only);
                }
            }
            other => panic!("expected order event, was {other:?}"),
        }
    }
}
