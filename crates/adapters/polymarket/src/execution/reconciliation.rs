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

//! Reconciliation report generation for the Polymarket execution client.

use ahash::{AHashMap, AHashSet};
use anyhow::Context;
use nautilus_core::{
    UnixNanos, collections::AtomicMap, datetime::NANOSECONDS_IN_SECOND, time::AtomicTime,
};
use nautilus_model::{
    enums::{LiquiditySide, OrderStatus, PositionSideSpecified},
    identifiers::{AccountId, ClientId, ClientOrderId, InstrumentId, TradeId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::{Currency, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    identity::OrderIdentity,
    order_fill_tracker::{AppliedPendingFill, OrderFillTrackerMap},
    parse::{
        build_maker_fill_report, instrument_fee_exponent, instrument_taker_fee, parse_fill_report,
        parse_fill_values, parse_order_status_report, parse_timestamp,
    },
};
use crate::{
    common::{
        consts::{DUST_POSITION_THRESHOLD, DUST_SNAP_THRESHOLD_DEC, USDC_DECIMALS},
        enums::{PolymarketLiquiditySide, PolymarketTradeStatus},
    },
    http::{
        clob::PolymarketClobHttpClient,
        data_api::PolymarketDataApiHttpClient,
        models::{DataApiPosition, PolymarketOpenOrder, PolymarketTradeReport},
        query::{GetOrdersParams, GetTradesParams},
    },
};

/// Shared context for trade-to-fill-report conversion.
pub(crate) struct FillContext<'a> {
    pub account_id: AccountId,
    pub user_address: &'a str,
    pub api_key: &'a str,
    pub pusd: Currency,
    pub clock: &'static AtomicTime,
}

#[derive(Clone, Copy)]
pub(crate) enum PendingFillPolicy<'a> {
    /// Mass status: emit pending-trade fills only when the fill's
    /// `venue_order_id` is in the paired order-report set; count the rest.
    PairedWith(&'a AHashSet<VenueOrderId>),
    /// Runtime singular checks: confirmed trades only (pending settlement must
    /// not create an inferred fill).
    ConfirmedOnly,
}

#[derive(Debug, Default)]
pub(crate) struct FillBuildOutput {
    pub fills: Vec<FillReport>,
    pub discards: FillBuildDiscards,
    /// Summed `last_qty` of emitted pending-sourced fills per order.
    pub pending_filled_by_order: AHashMap<VenueOrderId, Decimal>,
    /// Raw venue trade ID paired with each emitted pending fill's report index.
    pub pending_fill_sources: Vec<(usize, TradeId)>,
}

/// Counts of trade evidence dropped while building fill reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FillBuildDiscards {
    /// Fill entries dropped because their instrument is not loaded.
    pub unmapped_instruments: usize,
    /// Fill entries dropped because a quantity or price cannot be represented.
    pub unrepresentable_fills: usize,
    /// Duplicate non-failed venue trades dropped by raw trade ID.
    pub duplicate_trades: usize,
    /// Confirmed maker trades dropped because no maker order in the match is
    /// owned by the account.
    pub unowned_maker_trades: usize,
    /// Settlement-pending fills dropped because no order report pairs with them.
    pub unpaired_pending_fills: usize,
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    pending_policy: PendingFillPolicy<'_>,
    ts_init: UnixNanos,
) -> FillBuildOutput {
    let mut output = FillBuildOutput::default();
    let mut seen_raw_trade_ids = AHashSet::new();

    for trade in trades {
        if trade.status == PolymarketTradeStatus::Failed {
            continue;
        }

        let raw_trade_id = TradeId::from(trade.id.as_str());

        if seen_raw_trade_ids.contains(&raw_trade_id) {
            log::debug!("Skipping duplicate venue trade {}", trade.id);
            output.discards.duplicate_trades += 1;
            continue;
        }

        let is_pending = trade.status.is_pending_settlement();

        if is_pending && matches!(pending_policy, PendingFillPolicy::ConfirmedOnly) {
            log::debug!(
                "Skipping settlement-pending trade {} with status {:?} under confirmed-only policy",
                trade.id,
                trade.status,
            );
            continue;
        }

        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            if !trade
                .maker_orders
                .iter()
                .any(|mo| mo.is_owned_by(ctx.user_address, ctx.api_key))
            {
                if trade.status == PolymarketTradeStatus::Confirmed {
                    output.discards.unowned_maker_trades += 1;
                }

                log::debug!(
                    "Maker trade {} with status {:?} holds no maker order owned by the account",
                    trade.id,
                    trade.status,
                );
                continue;
            }

            for mo in &trade.maker_orders {
                if !mo.is_owned_by(ctx.user_address, ctx.api_key) {
                    continue;
                }
                let token_id = Ustr::from(mo.asset_id.as_str());
                let instrument = instruments.get_cloned(&token_id);
                let (instrument_id, price_prec, size_prec) = match instrument {
                    Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                    None => {
                        output.discards.unmapped_instruments += 1;
                        continue;
                    }
                };

                if let Some(filter_id) = instrument_filter
                    && instrument_id != filter_id
                {
                    continue;
                }

                if parse_fill_values(
                    &trade.id,
                    mo.matched_amount,
                    mo.price,
                    price_prec,
                    size_prec,
                )
                .is_none()
                {
                    output.discards.unrepresentable_fills += 1;
                    continue;
                }

                let ts_event =
                    parse_timestamp(&trade.match_time).unwrap_or(ctx.clock.get_time_ns());
                let report = build_maker_fill_report(
                    mo,
                    &trade.id,
                    trade.trader_side,
                    trade.side,
                    trade.asset_id.as_str(),
                    ctx.account_id,
                    instrument_id,
                    price_prec,
                    size_prec,
                    ctx.pusd,
                    LiquiditySide::Maker,
                    ts_event,
                    ts_init,
                );
                push_fill_report(
                    report,
                    raw_trade_id,
                    is_pending,
                    &pending_policy,
                    &mut seen_raw_trade_ids,
                    &mut output,
                );
            }
        } else {
            let token_id = Ustr::from(trade.asset_id.as_str());
            let instrument = instruments.get_cloned(&token_id);
            let (instrument_id, price_prec, size_prec, taker_fee_rate, fee_exponent) =
                match instrument {
                    Some(i) => (
                        i.id(),
                        i.price_precision(),
                        i.size_precision(),
                        instrument_taker_fee(&i),
                        instrument_fee_exponent(&i),
                    ),
                    None => {
                        output.discards.unmapped_instruments += 1;
                        continue;
                    }
                };

            if let Some(filter_id) = instrument_filter
                && instrument_id != filter_id
            {
                continue;
            }

            let Some(report) = parse_fill_report(
                trade,
                instrument_id,
                ctx.account_id,
                None,
                price_prec,
                size_prec,
                ctx.pusd,
                taker_fee_rate,
                fee_exponent,
                ts_init,
            ) else {
                output.discards.unrepresentable_fills += 1;
                continue;
            };
            push_fill_report(
                report,
                raw_trade_id,
                is_pending,
                &pending_policy,
                &mut seen_raw_trade_ids,
                &mut output,
            );
        }
    }

    output
}

fn push_fill_report(
    report: FillReport,
    raw_trade_id: TradeId,
    is_pending: bool,
    pending_policy: &PendingFillPolicy<'_>,
    seen_raw_trade_ids: &mut AHashSet<TradeId>,
    output: &mut FillBuildOutput,
) {
    if is_pending {
        let PendingFillPolicy::PairedWith(paired_order_ids) = pending_policy else {
            return;
        };

        if !paired_order_ids.contains(&report.venue_order_id) {
            output.discards.unpaired_pending_fills += 1;
            log::debug!(
                "Settlement-pending fill {} for order {} omitted because no order report pairs with it",
                report.trade_id,
                report.venue_order_id,
            );
            return;
        }

        *output
            .pending_filled_by_order
            .entry(report.venue_order_id)
            .or_default() += report.last_qty.as_decimal();
        output
            .pending_fill_sources
            .push((output.fills.len(), raw_trade_id));
    }

    seen_raw_trade_ids.insert(raw_trade_id);
    output.fills.push(report);
}

fn record_rest_applied_pending_fills(
    fill_tracker: &OrderFillTrackerMap,
    fill_reports: &[FillReport],
    pending_fill_sources: &[(usize, TradeId)],
    resolve_order_identity: &impl Fn(&VenueOrderId) -> Option<OrderIdentity>,
) -> AHashSet<TradeId> {
    let applied_fills = pending_fill_sources
        .iter()
        .map(|(report_index, trade_id)| {
            let mut applied_fill = AppliedPendingFill::from(&fill_reports[*report_index]);

            if let Some(identity) = resolve_order_identity(&applied_fill.venue_order_id) {
                applied_fill.client_order_id = Some(identity.client_order_id);
                applied_fill.strategy_id = Some(identity.strategy_id);
                applied_fill.order_type = Some(identity.order_type);
            }
            (*trade_id, applied_fill)
        })
        .collect::<Vec<_>>();

    fill_tracker.record_rest_applied_pending_fills(&applied_fills)
}

fn remove_withheld_pending_fills(
    fill_reports: &mut Vec<FillReport>,
    pending_fill_sources: &[(usize, TradeId)],
    pending_filled_by_order: &mut AHashMap<VenueOrderId, Decimal>,
    withheld_trade_ids: &AHashSet<TradeId>,
) {
    let withheld_report_indices = pending_fill_sources
        .iter()
        .filter(|(_, trade_id)| withheld_trade_ids.contains(trade_id))
        .map(|(report_index, _)| *report_index)
        .collect::<AHashSet<_>>();
    pending_filled_by_order.clear();

    for (report_index, _) in pending_fill_sources
        .iter()
        .filter(|(_, trade_id)| !withheld_trade_ids.contains(trade_id))
    {
        let report = &fill_reports[*report_index];
        *pending_filled_by_order
            .entry(report.venue_order_id)
            .or_default() += report.last_qty.as_decimal();
    }

    let mut report_index = 0;
    fill_reports.retain(|_| {
        let retain = !withheld_report_indices.contains(&report_index);
        report_index += 1;
        retain
    });
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    resolve_client_order_id: impl Fn(&VenueOrderId) -> Option<ClientOrderId>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> (Vec<OrderStatusReport>, usize) {
    let mut reports = Vec::new();
    let mut filtered = 0usize;

    for order in orders {
        let token_id = Ustr::from(order.asset_id.as_str());
        let instrument = instruments.get_cloned(&token_id);
        let (instrument_id, price_prec, size_prec) = match instrument {
            Some(i) => (i.id(), i.price_precision(), i.size_precision()),
            None => {
                filtered += 1;
                continue;
            }
        };

        if let Some(filter_id) = instrument_filter
            && instrument_id != filter_id
        {
            continue;
        }

        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let report = parse_order_status_report(
            order,
            instrument_id,
            account_id,
            resolve_client_order_id(&venue_order_id),
            price_prec,
            size_prec,
            ts_init,
        );
        reports.push(report);
    }

    (reports, filtered)
}

/// Applies venue_order_id and time-range filters to fill reports.
pub(crate) fn apply_fill_filters(
    mut reports: Vec<FillReport>,
    venue_order_id: Option<VenueOrderId>,
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> Vec<FillReport> {
    if let Some(vid) = venue_order_id {
        reports.retain(|r| r.venue_order_id == vid);
    }

    match (start, end) {
        (Some(s), Some(e)) => reports.retain(|r| r.ts_event >= s && r.ts_event <= e),
        (Some(s), None) => reports.retain(|r| r.ts_event >= s),
        (None, Some(e)) => reports.retain(|r| r.ts_event <= e),
        (None, None) => {}
    }

    reports
}

/// Builds position status reports from Data API positions, filtering dust.
pub(crate) fn build_position_reports(
    positions: &[DataApiPosition],
    account_id: AccountId,
    ts: UnixNanos,
) -> Vec<PositionStatusReport> {
    positions
        .iter()
        .filter(|p| {
            if p.size > Decimal::ZERO && p.size < DUST_POSITION_THRESHOLD {
                log::debug!(
                    "Filtering dust position: {}-{}, size={}",
                    p.condition_id,
                    p.asset,
                    p.size
                );
            }
            p.size >= DUST_POSITION_THRESHOLD
        })
        .filter_map(|p| {
            let instrument_id =
                InstrumentId::from(format!("{}-{}.POLYMARKET", p.condition_id, p.asset).as_str());
            let quantity = match Quantity::from_decimal_dp(p.size, USDC_DECIMALS as u8) {
                Ok(quantity) => quantity,
                Err(e) => {
                    log::warn!(
                        "Skipping invalid Data API position {}-{} size {}: {e}",
                        p.condition_id,
                        p.asset,
                        p.size,
                    );
                    return None;
                }
            };
            Some(PositionStatusReport::new(
                account_id,
                instrument_id,
                PositionSideSpecified::Long,
                quantity,
                ts,
                ts,
                None,
                None,
                p.avg_price,
            ))
        })
        .collect()
}

fn filter_orders_to_lookback(orders: &mut Vec<PolymarketOpenOrder>, cutoff: UnixNanos) -> usize {
    let before = orders.len();
    orders.retain(|order| {
        order
            .created_at
            .checked_mul(NANOSECONDS_IN_SECOND)
            .map(UnixNanos::from)
            .is_none_or(|ts| ts >= cutoff)
    });
    before - orders.len()
}

fn filter_trades_to_lookback(trades: &mut Vec<PolymarketTradeReport>, cutoff: UnixNanos) -> usize {
    let before = trades.len();
    trades.retain(|trade| {
        parse_timestamp(&trade.match_time).is_none_or(|ts_event| ts_event >= cutoff)
    });
    before - trades.len()
}

fn mass_status_cutoff(ts_init: UnixNanos, lookback_mins: Option<u64>) -> Option<UnixNanos> {
    lookback_mins.map(|mins| {
        let lookback_ns = mins
            .saturating_mul(60)
            .saturating_mul(NANOSECONDS_IN_SECOND);
        UnixNanos::from(ts_init.as_u64().saturating_sub(lookback_ns))
    })
}

/// Full reconciliation mass status generation.
#[expect(clippy::too_many_arguments)]
pub(crate) async fn generate_mass_status(
    http_client: &PolymarketClobHttpClient,
    data_api_client: &PolymarketDataApiHttpClient,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    fill_tracker: &OrderFillTrackerMap,
    ctx: &FillContext<'_>,
    client_id: ClientId,
    venue: Venue,
    resolve_order_identity: impl Fn(&VenueOrderId) -> Option<OrderIdentity>,
    is_fill_applied: impl Fn(&FillReport, &[PolymarketTradeReport]) -> bool,
    lookback_mins: Option<u64>,
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    fill_tracker.begin_rest_applied_pending_pass();
    let ts_init = ctx.clock.get_time_ns();
    let cutoff = mass_status_cutoff(ts_init, lookback_mins);

    // Fetch orders
    let mut orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;
    let orders_before = orders.len();
    let orders_removed = cutoff.map_or(0, |cutoff| filter_orders_to_lookback(&mut orders, cutoff));

    let (mut order_reports, orders_filtered) = build_order_reports_from_orders(
        &orders,
        instruments,
        ctx.account_id,
        |venue_order_id| {
            resolve_order_identity(venue_order_id).map(|identity| identity.client_order_id)
        },
        None,
        ts_init,
    );
    let paired_order_ids = order_reports
        .iter()
        .map(|report| report.venue_order_id)
        .collect::<AHashSet<_>>();

    // Fetch and parse fill reports
    let mut trades = http_client
        .get_trades(GetTradesParams::default())
        .await
        .context("failed to fetch trades for mass status")?;
    let trades_before = trades.len();
    let trades_removed = cutoff.map_or(0, |cutoff| filter_trades_to_lookback(&mut trades, cutoff));

    let FillBuildOutput {
        fills: mut fill_reports,
        discards: fill_discards,
        mut pending_filled_by_order,
        pending_fill_sources,
    } = build_fill_reports_from_trades(
        &trades,
        ctx,
        instruments,
        None,
        PendingFillPolicy::PairedWith(&paired_order_ids),
        ts_init,
    );

    if fill_discards.unowned_maker_trades > 0 {
        log::error!(
            "Mass status is missing {} confirmed maker trade(s) holding no maker order owned by \
             the account; executed quantity may be understated",
            fill_discards.unowned_maker_trades,
        );
    }

    if fill_discards.unpaired_pending_fills > 0 {
        log::warn!(
            "{} settlement-pending fill(s) were omitted because no order report pairs with them; quantities may be understated until the trades confirm",
            fill_discards.unpaired_pending_fills,
        );
    }

    // Snap dust drift on REST fills the same way the WS path does.
    // Commission stays as venue-reported.
    fill_tracker.snap_fill_reports(&mut fill_reports);

    // Position reports from Data API
    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let position_reports = build_position_reports(&positions, ctx.account_id, ts_init);

    if let Some(mins) = lookback_mins {
        log::debug!(
            "Lookback filter ({}min): orders {}->{} (removed {}), trades {}->{} (removed {})",
            mins,
            orders_before,
            orders.len(),
            orders_removed,
            trades_before,
            trades.len(),
            trades_removed,
        );
    } else {
        log::debug!(
            "Generated mass status: {} orders ({} filtered), {} fills ({} instrument-filtered, \
             {} unrepresentable, {} duplicate venue trades, {} unowned maker trades, {} unpaired \
             settlement-pending fills), {} positions",
            order_reports.len(),
            orders_filtered,
            fill_reports.len(),
            fill_discards.unmapped_instruments,
            fill_discards.unrepresentable_fills,
            fill_discards.duplicate_trades,
            fill_discards.unowned_maker_trades,
            fill_discards.unpaired_pending_fills,
            position_reports.len(),
        );
    }

    let withheld_trade_ids = record_rest_applied_pending_fills(
        fill_tracker,
        &fill_reports,
        &pending_fill_sources,
        &resolve_order_identity,
    );
    remove_withheld_pending_fills(
        &mut fill_reports,
        &pending_fill_sources,
        &mut pending_filled_by_order,
        &withheld_trade_ids,
    );
    cap_order_reports_to_reported_fills(
        &mut order_reports,
        &fill_reports,
        &pending_filled_by_order,
    );

    let fills_before_applied_check = fill_reports.len();
    fill_reports.retain(|fill| !is_fill_applied(fill, &trades));
    let applied_fills = fills_before_applied_check - fill_reports.len();

    if applied_fills > 0 {
        log::debug!("Skipped {applied_fills} REST fill report(s) already applied to cached orders",);
    }

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
}

fn cap_order_reports_to_reported_fills(
    order_reports: &mut [OrderStatusReport],
    fill_reports: &[FillReport],
    pending_filled_by_order: &AHashMap<VenueOrderId, Decimal>,
) {
    let reported_by_order = reported_filled_quantities(fill_reports);

    for report in order_reports {
        let local_filled = Quantity::zero(report.quantity.precision);
        cap_order_report_filled_qty_without_normalization(
            report,
            local_filled,
            reported_by_order.get(&report.venue_order_id).copied(),
        );

        if !pending_filled_by_order
            .get(&report.venue_order_id)
            .is_some_and(|quantity| !quantity.is_zero())
        {
            normalize_terminal_order_report_quantity(report);
        }
    }
}

pub(crate) fn reported_filled_quantities(
    fill_reports: &[FillReport],
) -> AHashMap<VenueOrderId, Decimal> {
    let mut reported_by_order = AHashMap::new();

    for fill in fill_reports {
        *reported_by_order.entry(fill.venue_order_id).or_default() += fill.last_qty.as_decimal();
    }

    reported_by_order
}

pub(crate) fn cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    reported_filled: Option<Decimal>,
) {
    cap_order_report_filled_qty_without_normalization(report, local_filled, reported_filled);
    normalize_terminal_order_report_quantity(report);
}

fn cap_order_report_filled_qty_without_normalization(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    reported_filled: Option<Decimal>,
) {
    let reported_filled = reported_filled
        .and_then(|qty| Quantity::from_decimal_dp(qty, report.quantity.precision).ok())
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    let capped = report.filled_qty.min(local_filled.max(reported_filled));
    report.filled_qty = capped;
}

pub(crate) fn normalize_terminal_order_report_quantity(report: &mut OrderStatusReport) {
    if report.order_status != OrderStatus::Filled
        || report.filled_qty.is_zero()
        || report.filled_qty >= report.quantity
    {
        return;
    }

    let leaves = report.quantity.as_decimal() - report.filled_qty.as_decimal();
    if leaves < DUST_SNAP_THRESHOLD_DEC {
        log::debug!(
            "Normalizing terminal order report {} quantity from {} to reported fills {}",
            report.venue_order_id,
            report.quantity,
            report.filled_qty,
        );
        report.quantity = report.filled_qty;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::{ClientOrderId, StrategyId, TradeId},
        types::{Money, Price},
    };
    use rstest::rstest;

    use super::*;

    fn instruments_with_asset(asset_id: Ustr) -> AtomicMap<Ustr, InstrumentAny> {
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(asset_id, instrument);
        instruments
    }

    fn fill_context() -> FillContext<'static> {
        FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: get_atomic_clock_realtime(),
        }
    }

    fn order_identity(client_order_id: ClientOrderId) -> OrderIdentity {
        OrderIdentity {
            client_order_id,
            strategy_id: StrategyId::from("S-EVIDENCE-RESOLVED"),
            instrument_id: InstrumentId::from("TEST.POLYMARKET"),
            order_side: OrderSide::Buy,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::Fok,
        }
    }

    #[rstest]
    fn order_reports_include_resolved_client_order_id() {
        let order: PolymarketOpenOrder = load("http_open_order.json");
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        let client_order_id = ClientOrderId::from("O-RESOLVED");
        let mut external_order = order.clone();
        external_order.id = "V-EXTERNAL".to_string();
        let instruments = instruments_with_asset(order.asset_id);

        let (reports, filtered) = build_order_reports_from_orders(
            &[order, external_order],
            &instruments,
            AccountId::from("POLY-001"),
            |candidate| (candidate == &venue_order_id).then_some(client_order_id),
            None,
            UnixNanos::from(1),
        );

        assert_eq!(filtered, 0);
        assert_eq!(reports.len(), 2);
        assert_eq!(reports[0].client_order_id, Some(client_order_id));
        assert_eq!(reports[1].client_order_id, None);
    }

    #[rstest]
    fn mass_status_records_post_snap_pending_fill_evidence() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.size = Decimal::from_str_exact("714.285714").unwrap();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let instruments = instruments_with_asset(trade.asset_id);
        let paired_order_ids = AHashSet::from_iter([venue_order_id]);
        let mut output = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&paired_order_ids),
            UnixNanos::from(1),
        );
        let fill_tracker = OrderFillTrackerMap::new();
        let applied_qty = Quantity::from("714.285710");
        fill_tracker.register(
            venue_order_id,
            applied_qty,
            OrderSide::Buy,
            output.fills[0].instrument_id,
            6,
            2,
        );
        fill_tracker.begin_rest_applied_pending_pass();

        fill_tracker.snap_fill_reports(&mut output.fills);
        let withheld = record_rest_applied_pending_fills(
            &fill_tracker,
            &output.fills,
            &output.pending_fill_sources,
            &|_| None,
        );

        assert!(withheld.is_empty());
        assert_eq!(output.fills[0].last_qty, applied_qty);
        assert_eq!(
            fill_tracker.rest_applied_pending_fills(&trade_id),
            vec![AppliedPendingFill::from(&output.fills[0])],
        );
    }

    #[rstest]
    fn mass_status_records_resolved_order_identity_in_pending_fill_evidence() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let client_order_id = ClientOrderId::from("O-EVIDENCE-RESOLVED");
        let instruments = instruments_with_asset(trade.asset_id);
        let paired_order_ids = AHashSet::from_iter([venue_order_id]);
        let output = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&paired_order_ids),
            UnixNanos::from(1),
        );
        let fill_tracker = OrderFillTrackerMap::new();
        let identity = order_identity(client_order_id);

        let withheld = record_rest_applied_pending_fills(
            &fill_tracker,
            &output.fills,
            &output.pending_fill_sources,
            &|candidate| (candidate == &venue_order_id).then_some(identity),
        );

        assert!(withheld.is_empty());
        let recorded = &fill_tracker.rest_applied_pending_fills(&trade_id)[0];
        assert_eq!(recorded.client_order_id, Some(client_order_id));
        assert_eq!(recorded.strategy_id, Some(identity.strategy_id));
        assert_eq!(recorded.order_type, Some(identity.order_type));
    }

    #[rstest]
    fn mass_status_withholds_failed_pending_fill_before_order_cap() {
        let order: PolymarketOpenOrder = load("http_open_order.json");
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.size = Decimal::from_str_exact("714.285714").unwrap();
        let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
        let trade_id = TradeId::from(trade.id.as_str());
        let instruments = instruments_with_asset(trade.asset_id);
        let (mut order_reports, _) = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLY-001"),
            |_| None,
            None,
            UnixNanos::from(1),
        );
        let paired_order_ids = order_reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect::<AHashSet<_>>();
        let mut output = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&paired_order_ids),
            UnixNanos::from(1),
        );
        let fill_tracker = OrderFillTrackerMap::new();
        let applied_qty = Quantity::from("714.285710");
        fill_tracker.register(
            venue_order_id,
            applied_qty,
            OrderSide::Buy,
            output.fills[0].instrument_id,
            6,
            2,
        );
        fill_tracker.note_failed_trade(&trade_id);
        fill_tracker.begin_rest_applied_pending_pass();

        fill_tracker.snap_fill_reports(&mut output.fills);
        let withheld = record_rest_applied_pending_fills(
            &fill_tracker,
            &output.fills,
            &output.pending_fill_sources,
            &|_| None,
        );
        remove_withheld_pending_fills(
            &mut output.fills,
            &output.pending_fill_sources,
            &mut output.pending_filled_by_order,
            &withheld,
        );
        cap_order_reports_to_reported_fills(
            &mut order_reports,
            &output.fills,
            &output.pending_filled_by_order,
        );

        assert_eq!(withheld, AHashSet::from_iter([trade_id]));
        assert!(output.fills.is_empty());
        assert!(output.pending_filled_by_order.is_empty());
        assert!(
            fill_tracker
                .rest_applied_pending_fills(&trade_id)
                .is_empty()
        );
        assert_eq!(order_reports.len(), 1);
        assert_eq!(order_reports[0].filled_qty, Quantity::zero(4));
    }

    #[rstest]
    fn caps_order_report_to_confirmed_companion_fills() {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-1");
        let mut reports = vec![OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::PartiallyFilled,
            Quantity::from("10.0000"),
            Quantity::from("10.0000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let fills = vec![FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-1"),
            OrderSide::Buy,
            Quantity::from("4.0000"),
            Price::from("0.5000"),
            Money::new(0.0, Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_reported_fills(&mut reports, &fills, &AHashMap::new());

        assert_eq!(reports[0].filled_qty, Quantity::from("4.0000"));
    }

    #[rstest]
    #[case::below_threshold("99.995", "99.995")]
    #[case::at_threshold("99.990", "100.000")]
    fn normalizes_confirmed_dust_residual_to_order_quantity(
        #[case] confirmed: &str,
        #[case] expected_quantity: &str,
    ) {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-DUST");
        let mut reports = vec![OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("100.000"),
            Quantity::from("100.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let fills = vec![FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-DUST"),
            OrderSide::Buy,
            Quantity::from(confirmed),
            Price::from("0.5000"),
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_reported_fills(&mut reports, &fills, &AHashMap::new());

        assert_eq!(reports[0].quantity, Quantity::from(expected_quantity));
        assert_eq!(reports[0].filled_qty, Quantity::from(confirmed));
    }

    #[rstest]
    fn pending_fill_caps_filled_quantity_without_normalizing_order_quantity() {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-PENDING-DUST");
        let mut reports = vec![OrderStatusReport::new(
            account_id,
            instrument_id,
            None,
            venue_order_id,
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            Quantity::from("100.000"),
            Quantity::from("100.000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let fills = vec![FillReport::new(
            account_id,
            instrument_id,
            venue_order_id,
            TradeId::from("T-PENDING-DUST"),
            OrderSide::Buy,
            Quantity::from("99.995"),
            Price::from("0.5000"),
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];
        let pending_filled_by_order =
            AHashMap::from_iter([(venue_order_id, Decimal::from_str_exact("99.995").unwrap())]);

        cap_order_reports_to_reported_fills(&mut reports, &fills, &pending_filled_by_order);

        assert_eq!(reports[0].filled_qty, Quantity::from("99.995"));
        assert_eq!(reports[0].quantity, Quantity::from("100.000"));
    }

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    #[rstest]
    #[case(PolymarketTradeStatus::Matched)]
    #[case(PolymarketTradeStatus::Mined)]
    #[case(PolymarketTradeStatus::Retrying)]
    fn paired_pending_taker_trade_reaches_fills_and_cap(#[case] status: PolymarketTradeStatus) {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = status;

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let paired_order_ids =
            AHashSet::from_iter([VenueOrderId::from(trade.taker_order_id.as_str())]);
        let output = build_fill_reports_from_trades(
            &[trade.clone()],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&paired_order_ids),
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.fills[0].trade_id, TradeId::from(trade.id.as_str()),);

        let mut reports = vec![OrderStatusReport::new(
            ctx.account_id,
            output.fills[0].instrument_id,
            None,
            VenueOrderId::from(trade.taker_order_id.as_str()),
            OrderSide::Buy,
            OrderType::Limit,
            TimeInForce::Gtc,
            OrderStatus::Filled,
            output.fills[0].last_qty,
            output.fills[0].last_qty,
            UnixNanos::from(1),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_reported_fills(
            &mut reports,
            &output.fills,
            &output.pending_filled_by_order,
        );

        assert_eq!(reports[0].filled_qty, output.fills[0].last_qty);
    }

    #[rstest]
    fn paired_pending_owned_maker_trade_emits_venue_identity() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.id = "123456789012345678901234567-12345678".to_string();
        trade.maker_orders[0].order_id = "12345678".to_string();
        let owned_maker = trade.maker_orders[0].clone();

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(owned_maker.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: &owned_maker.maker_address,
            api_key: "ffffffff-ffff-ffff-ffff-ffffffffffff",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };
        let paired_order_ids =
            AHashSet::from_iter([VenueOrderId::from(owned_maker.order_id.as_str())]);

        let output = build_fill_reports_from_trades(
            &[trade.clone()],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&paired_order_ids),
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(
            output.fills[0].trade_id,
            TradeId::from("1234567890123456789-864e38ffec393d64"),
        );
    }

    #[rstest]
    fn unpaired_pending_fill_is_counted_and_omitted() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let output = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&AHashSet::new()),
            UnixNanos::from(1),
        );

        assert!(output.fills.is_empty());
        assert_eq!(output.discards.unpaired_pending_fills, 1);
        assert!(output.pending_filled_by_order.is_empty());
    }

    #[rstest]
    fn confirmed_fill_is_not_pairing_gated() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let output = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&AHashSet::new()),
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.discards.unpaired_pending_fills, 0);
        assert!(output.pending_filled_by_order.is_empty());
    }

    #[rstest]
    fn confirmed_only_omits_pending_fill_without_a_discard() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let output = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::ConfirmedOnly,
            UnixNanos::from(1),
        );

        assert!(output.fills.is_empty());
        assert_eq!(output.discards, FillBuildDiscards::default());
        assert!(output.pending_filled_by_order.is_empty());
    }

    #[rstest]
    fn confirmed_redelivery_after_pending_skip_is_emitted() {
        let mut pending: PolymarketTradeReport = load("http_trade_report.json");
        pending.status = PolymarketTradeStatus::Matched;
        let mut confirmed = pending.clone();
        confirmed.status = PolymarketTradeStatus::Confirmed;
        let instruments = instruments_with_asset(pending.asset_id);

        let output = build_fill_reports_from_trades(
            &[pending, confirmed],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::ConfirmedOnly,
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.discards.duplicate_trades, 0);
    }

    #[rstest]
    fn duplicate_trade_id_is_counted_and_emitted_once() {
        let trade: PolymarketTradeReport = load("http_trade_report.json");
        let trade_id = TradeId::from(trade.id.as_str());
        let instruments = instruments_with_asset(trade.asset_id);

        let output = build_fill_reports_from_trades(
            &[trade.clone(), trade],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::ConfirmedOnly,
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.fills[0].trade_id, trade_id);
        assert_eq!(output.discards.duplicate_trades, 1);
    }

    #[rstest]
    fn valid_row_after_unrepresentable_row_is_emitted() {
        let mut corrupt: PolymarketTradeReport = load("http_trade_report.json");
        let valid = corrupt.clone();
        corrupt.size = Decimal::NEGATIVE_ONE;
        let instruments = instruments_with_asset(valid.asset_id);

        let output = build_fill_reports_from_trades(
            &[corrupt, valid],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::ConfirmedOnly,
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.discards.unrepresentable_fills, 1);
        assert_eq!(output.discards.duplicate_trades, 0);
    }

    #[rstest]
    fn confirmed_redelivery_after_unpaired_pending_is_emitted() {
        let mut pending: PolymarketTradeReport = load("http_trade_report.json");
        pending.status = PolymarketTradeStatus::Matched;
        let mut confirmed = pending.clone();
        confirmed.status = PolymarketTradeStatus::Confirmed;
        let instruments = instruments_with_asset(pending.asset_id);

        let output = build_fill_reports_from_trades(
            &[pending, confirmed],
            &fill_context(),
            &instruments,
            None,
            PendingFillPolicy::PairedWith(&AHashSet::new()),
            UnixNanos::from(1),
        );

        assert_eq!(output.fills.len(), 1);
        assert_eq!(output.discards.unpaired_pending_fills, 1);
        assert_eq!(output.discards.duplicate_trades, 0);
    }

    #[rstest]
    #[case(PolymarketTradeStatus::Matched)]
    #[case(PolymarketTradeStatus::Mined)]
    #[case(PolymarketTradeStatus::Retrying)]
    #[case(PolymarketTradeStatus::Failed)]
    fn unowned_non_confirmed_maker_trade_is_not_counted(#[case] status: PolymarketTradeStatus) {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = status;
        trade.trader_side = PolymarketLiquiditySide::Maker;

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x000000000000000000000000000000000000dead",
            api_key: "ffffffff-ffff-ffff-ffff-ffffffffffff",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let output = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            PendingFillPolicy::ConfirmedOnly,
            UnixNanos::from(1),
        );

        assert!(output.fills.is_empty());
        assert_eq!(output.discards.unowned_maker_trades, 0);
    }
}
