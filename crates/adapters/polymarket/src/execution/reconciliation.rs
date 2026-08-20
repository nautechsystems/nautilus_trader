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
    identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId},
    instruments::{Instrument, InstrumentAny},
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::Quantity,
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    order_fill_tracker::OrderFillTrackerMap,
    parse::{build_maker_fill_report, parse_fill_report, parse_order_status_report},
    report_validation::{ensure_instrument_binding, non_negative_quantity, parse_match_time},
};
use crate::{
    common::{
        consts::{DUST_POSITION_THRESHOLD, DUST_SNAP_THRESHOLD_DEC},
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
    pub clock: &'static AtomicTime,
}

/// Counts of confirmed trade evidence dropped while building fill reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FillBuildDiscards {
    /// Fill entries dropped because their instrument is not loaded.
    pub unmapped_instruments: usize,
    /// In-scope historical fills dropped because their instrument is not loaded.
    pub in_scope_historical: usize,
    /// Confirmed maker trades dropped because no maker order in the match is
    /// owned by the account.
    pub unowned_maker_trades: usize,
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<(Vec<FillReport>, FillBuildDiscards)> {
    let mut reports = Vec::new();
    let mut discards = FillBuildDiscards::default();

    for trade in trades {
        if trade.status != PolymarketTradeStatus::Confirmed {
            continue;
        }

        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            if !trade
                .maker_orders
                .iter()
                .any(|mo| mo.is_owned_by(ctx.user_address, ctx.api_key))
            {
                discards.unowned_maker_trades += 1;
                log::debug!(
                    "Confirmed maker trade {} holds no maker order owned by the account",
                    trade.id,
                );
                continue;
            }

            let mut ts_event = None;

            for mo in &trade.maker_orders {
                if !mo.is_owned_by(ctx.user_address, ctx.api_key) {
                    continue;
                }
                let token_id = mo.asset_id;
                let instrument = instruments.get_cloned(&token_id);
                let instrument = match instrument {
                    Some(instrument) => instrument,
                    None => {
                        classify_unmapped_historical(
                            &mut discards,
                            load_ids,
                            &trade.market,
                            token_id.as_str(),
                        );
                        continue;
                    }
                };
                let instrument_id = instrument.id();

                if let Some(filter_id) = instrument_filter
                    && instrument_id != filter_id
                {
                    continue;
                }

                ensure_instrument_binding(
                    &instrument,
                    trade.market.as_str(),
                    mo.asset_id.as_str(),
                    Some(mo.outcome.as_str()),
                    "Polymarket maker fill",
                )?;
                let ts_event = match ts_event {
                    Some(ts_event) => ts_event,
                    None => {
                        let parsed = parse_match_time(&trade.match_time, "maker fill match_time")?;
                        ts_event = Some(parsed);
                        parsed
                    }
                };

                let report = build_maker_fill_report(
                    mo,
                    &trade.id,
                    trade.trader_side,
                    trade.side,
                    trade.asset_id.as_str(),
                    trade.market.as_str(),
                    ctx.account_id,
                    &instrument,
                    LiquiditySide::Maker,
                    ts_event,
                    ts_init,
                )
                .with_context(|| {
                    format!(
                        "failed to build maker fill report for trade {} and order {}",
                        trade.id, mo.order_id,
                    )
                })?;
                reports.push(report);
            }
        } else {
            let token_id = trade.asset_id;
            let instrument = instruments.get_cloned(&token_id);
            let instrument = match instrument {
                Some(instrument) => instrument,
                None => {
                    classify_unmapped_historical(
                        &mut discards,
                        load_ids,
                        &trade.market,
                        token_id.as_str(),
                    );
                    continue;
                }
            };
            let instrument_id = instrument.id();

            if let Some(filter_id) = instrument_filter
                && instrument_id != filter_id
            {
                continue;
            }

            let report = parse_fill_report(trade, &instrument, ctx.account_id, None, ts_init)
                .with_context(|| {
                    format!("failed to build taker fill report for trade {}", trade.id)
                })?;
            reports.push(report);
        }
    }

    Ok((reports, discards))
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<(Vec<OrderStatusReport>, usize)> {
    let mut reports = Vec::new();
    let mut filtered = 0usize;

    for order in orders {
        let token_id = order.asset_id;
        let instrument = instruments.get_cloned(&token_id);
        let instrument = match instrument {
            Some(instrument) => instrument,
            None => {
                let instrument_id =
                    instrument_id_from_market_token(order.market.as_str(), token_id.as_str());

                if instrument_in_load_ids_scope(instrument_id, load_ids) {
                    anyhow::bail!(unmapped_in_scope_message(
                        "open order",
                        instrument_id,
                        Some(&format!("token {token_id}")),
                        load_ids,
                    ));
                }
                log::debug!("Dropping out-of-scope unmapped open order instrument {instrument_id}");
                filtered += 1;
                continue;
            }
        };
        let instrument_id = instrument.id();

        if let Some(filter_id) = instrument_filter
            && instrument_id != filter_id
        {
            continue;
        }

        let report = parse_order_status_report(order, &instrument, account_id, None, ts_init)?;
        reports.push(report);
    }

    Ok((reports, filtered))
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

/// Builds position reports after binding every relevant row to its loaded instrument.
///
/// Binding and exact quantity construction deliberately precede zero/dust exclusion so malformed
/// evidence cannot disappear as an empty position.
pub(crate) fn build_position_reports_scoped(
    positions: &[DataApiPosition],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    load_ids: Option<&[InstrumentId]>,
    ts: UnixNanos,
) -> anyhow::Result<Vec<PositionStatusReport>> {
    let mut reports = Vec::with_capacity(positions.len());

    for position in positions {
        let token_id = Ustr::from(position.asset.as_str());
        let Some(instrument) = instruments.get_cloned(&token_id) else {
            let instrument_id =
                instrument_id_from_market_token(&position.condition_id, &position.asset);
            let in_scope = instrument_filter.map_or_else(
                || instrument_in_load_ids_scope(instrument_id, load_ids),
                |filter_id| filter_id == instrument_id,
            );

            if in_scope {
                anyhow::bail!(unmapped_in_scope_message(
                    "position",
                    instrument_id,
                    Some(&format!("token {}", position.asset)),
                    load_ids,
                ));
            }
            log::debug!("Dropping out-of-scope unmapped position instrument {instrument_id}");
            continue;
        };
        let instrument_id = instrument.id();

        if instrument_filter.is_some_and(|filter_id| filter_id != instrument_id) {
            continue;
        }

        ensure_instrument_binding(
            &instrument,
            &position.condition_id,
            &position.asset,
            None,
            "Data API position",
        )?;
        let quantity =
            non_negative_quantity(position.size, instrument.size_precision(), "position size")?;

        if position.size > Decimal::ZERO && position.size < DUST_POSITION_THRESHOLD {
            log::debug!(
                "Filtering dust position: {}-{}, size={}",
                position.condition_id,
                position.asset,
                position.size,
            );
        }

        if position.size < DUST_POSITION_THRESHOLD {
            continue;
        }

        reports.push(PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSideSpecified::Long,
            quantity,
            ts,
            ts,
            None,
            None,
            position.avg_price,
        ));
    }

    Ok(reports)
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
    lookback_mins: Option<u64>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let ts_init = ctx.clock.get_time_ns();
    let lookback_start = lookback_mins.map(|mins| {
        UnixNanos::from(
            ts_init.as_u64().saturating_sub(
                mins.saturating_mul(60)
                    .saturating_mul(NANOSECONDS_IN_SECOND),
            ),
        )
    });

    let orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;

    let (mut order_reports, orders_filtered) = build_order_reports_from_orders(
        &orders,
        instruments,
        ctx.account_id,
        None,
        ts_init,
        load_ids,
    )?;

    let mut trades = http_client
        .get_trades(trades_params_for_window(
            lookback_start,
            lookback_start.map(|_| ts_init),
        ))
        .await
        .context("failed to fetch trades for mass status")?;

    let mut untimestamped_trades = 0usize;

    if let Some(cutoff) = lookback_start {
        (trades, untimestamped_trades) =
            trades_in_lookback_scope(trades, cutoff, ctx, instruments, None, None, load_ids)?;
    }

    let (mut fill_reports, fill_discards) =
        build_fill_reports_from_trades(&trades, ctx, instruments, None, ts_init, load_ids)?;

    if fill_discards.unowned_maker_trades > 0 {
        log::error!(
            "Mass status is missing {} confirmed maker trade(s) holding no maker order owned by \
             the account; executed quantity may be understated",
            fill_discards.unowned_maker_trades,
        );
    }

    fill_tracker.snap_fill_reports(&mut fill_reports);
    validate_known_order_fill_aggregates(&fill_reports, fill_tracker)?;

    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let position_reports = build_position_reports_scoped(
        &positions,
        instruments,
        ctx.account_id,
        None,
        load_ids,
        ts_init,
    )?;

    log::debug!(
        "Generated mass status: {} orders ({} filtered), {} fills ({} instrument-filtered, \
         {} in-scope historical misses, {} unowned maker trades), {} positions",
        order_reports.len(),
        orders_filtered,
        fill_reports.len(),
        fill_discards.unmapped_instruments,
        fill_discards.in_scope_historical,
        fill_discards.unowned_maker_trades,
        position_reports.len(),
    );

    if lookback_start.is_none() {
        cap_order_reports_to_confirmed_fills(&mut order_reports, &fill_reports)?;
    }

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    if let Some(lookback_start) = lookback_start {
        let reported_orders: AHashSet<VenueOrderId> = order_reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect();
        let reports_complete = fill_discards.in_scope_historical == 0
            && fill_discards.unowned_maker_trades == 0
            && untimestamped_trades == 0
            && fill_reports
                .iter()
                .all(|report| reported_orders.contains(&report.venue_order_id));
        mass_status.set_report_window(Some(lookback_start), reports_complete);
    }

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
}

pub(crate) fn trades_params_for_window(
    start: Option<UnixNanos>,
    end: Option<UnixNanos>,
) -> GetTradesParams {
    GetTradesParams {
        // CLOB `after` is exclusive of the given Unix second
        after: start.map(|ts| unix_secs(ts).saturating_sub(1)),
        before: end.map(unix_secs),
        ..Default::default()
    }
}

fn unix_secs(ts: UnixNanos) -> u64 {
    ts.as_u64() / NANOSECONDS_IN_SECOND
}

fn instrument_id_from_market_token(market: &str, token_id: &str) -> InstrumentId {
    InstrumentId::from(format!("{market}-{token_id}.POLYMARKET").as_str())
}

fn instrument_in_load_ids_scope(
    instrument_id: InstrumentId,
    load_ids: Option<&[InstrumentId]>,
) -> bool {
    match load_ids {
        Some(ids) if !ids.is_empty() => ids.contains(&instrument_id),
        _ => true,
    }
}

fn historical_instrument_in_scope(
    instrument_id: InstrumentId,
    instrument_filter: Option<InstrumentId>,
    load_ids: Option<&[InstrumentId]>,
) -> bool {
    instrument_filter.map_or_else(
        || instrument_in_load_ids_scope(instrument_id, load_ids),
        |filter_id| filter_id == instrument_id,
    )
}

fn venue_order_in_scope(venue_order_id: &str, venue_order_filter: Option<VenueOrderId>) -> bool {
    venue_order_filter.is_none_or(|filter_id| venue_order_id == filter_id.as_str())
}

/// Determines whether a confirmed trade can affect the requested/loaded static scope without
/// parsing its timestamp or economic values.
pub(crate) fn confirmed_trade_in_static_scope(
    trade: &PolymarketTradeReport,
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    venue_order_filter: Option<VenueOrderId>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<bool> {
    if trade.status != PolymarketTradeStatus::Confirmed {
        return Ok(false);
    }

    let instrument_in_scope = |raw_token_id: &str| -> anyhow::Result<bool> {
        if let Some(instrument) = instruments.get_cloned(&Ustr::from(raw_token_id)) {
            return Ok(instrument_filter.is_none_or(|filter_id| instrument.id() == filter_id));
        }

        let instrument_id = instrument_id_from_market_token(trade.market.as_str(), raw_token_id);
        Ok(historical_instrument_in_scope(
            instrument_id,
            instrument_filter,
            load_ids,
        ))
    };

    if trade.trader_side == PolymarketLiquiditySide::Maker {
        for order in &trade.maker_orders {
            if !venue_order_in_scope(&order.order_id, venue_order_filter)
                || !order.is_owned_by(ctx.user_address, ctx.api_key)
            {
                continue;
            }

            if instrument_in_scope(order.asset_id.as_str())? {
                return Ok(true);
            }
        }
        Ok(false)
    } else if venue_order_in_scope(&trade.taker_order_id, venue_order_filter) {
        instrument_in_scope(trade.asset_id.as_str())
    } else {
        Ok(false)
    }
}

fn trades_in_lookback_scope(
    trades: Vec<PolymarketTradeReport>,
    cutoff: UnixNanos,
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    venue_order_filter: Option<VenueOrderId>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<(Vec<PolymarketTradeReport>, usize)> {
    let mut retained = Vec::with_capacity(trades.len());
    let mut untimestamped = 0usize;

    for trade in trades {
        if !confirmed_trade_in_static_scope(
            &trade,
            ctx,
            instruments,
            instrument_filter,
            venue_order_filter,
            load_ids,
        )? {
            log::debug!(
                "Dropping confirmed trade {} outside lookback scope",
                trade.id
            );
            continue;
        }

        match parse_match_time(&trade.match_time, "trade match_time") {
            Ok(ts_event) if ts_event >= cutoff => retained.push(trade),
            Ok(_) => {}
            Err(_) => untimestamped += 1,
        }
    }

    Ok((retained, untimestamped))
}

fn unmapped_in_scope_message(
    kind: &str,
    instrument_id: InstrumentId,
    detail: Option<&str>,
    load_ids: Option<&[InstrumentId]>,
) -> String {
    let hint = match load_ids {
        Some(ids) if ids.contains(&instrument_id) => {
            "this instrument is in instrument_config.load_ids but was not loaded"
        }
        _ => "set instrument_config.load_ids to the instruments this node should reconcile",
    };

    match detail {
        Some(detail) => {
            format!("unmapped in-scope {kind} instrument {instrument_id} ({detail}); {hint}")
        }
        None => format!("unmapped in-scope {kind} instrument {instrument_id}; {hint}"),
    }
}

fn classify_unmapped_historical(
    discards: &mut FillBuildDiscards,
    load_ids: Option<&[InstrumentId]>,
    market: &str,
    token_id: &str,
) {
    let instrument_id = instrument_id_from_market_token(market, token_id);
    discards.unmapped_instruments += 1;
    if instrument_in_load_ids_scope(instrument_id, load_ids) {
        discards.in_scope_historical += 1;
        log::warn!("Unmapped in-scope historical instrument {instrument_id}");
        return;
    }

    log::debug!("Dropping out-of-scope unmapped historical instrument {instrument_id}");
}

fn cap_order_reports_to_confirmed_fills(
    order_reports: &mut [OrderStatusReport],
    fill_reports: &[FillReport],
) -> anyhow::Result<()> {
    let confirmed_by_order = confirmed_filled_quantities(fill_reports)?;

    for report in order_reports {
        let local_filled = Quantity::zero(report.quantity.precision);
        cap_order_report_filled_qty(
            report,
            local_filled,
            local_filled,
            confirmed_by_order.get(&report.venue_order_id).copied(),
        )?;
    }
    Ok(())
}

pub(crate) fn confirmed_filled_quantities(
    fill_reports: &[FillReport],
) -> anyhow::Result<AHashMap<VenueOrderId, Decimal>> {
    let mut confirmed_by_order = AHashMap::new();
    for fill in fill_reports {
        let total = confirmed_by_order
            .entry(fill.venue_order_id)
            .or_insert(Decimal::ZERO);
        *total = checked_confirmed_filled_total(
            *total,
            fill.last_qty.as_decimal(),
            fill.venue_order_id,
        )?;
    }

    Ok(confirmed_by_order)
}

pub(crate) fn validate_known_order_fill_aggregates(
    fill_reports: &[FillReport],
    fill_tracker: &OrderFillTrackerMap,
) -> anyhow::Result<()> {
    for (venue_order_id, total) in confirmed_filled_quantities(fill_reports)? {
        fill_tracker.validate_confirmed_total(&venue_order_id, total)?;
    }
    Ok(())
}

fn checked_confirmed_filled_total(
    current: Decimal,
    added: Decimal,
    venue_order_id: VenueOrderId,
) -> anyhow::Result<Decimal> {
    current
        .checked_add(added)
        .with_context(|| format!("confirmed filled quantity overflow for order {venue_order_id}"))
}

pub(crate) fn cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    cached_filled: Quantity,
    tracked_filled: Quantity,
    confirmed_filled: Option<Decimal>,
) -> anyhow::Result<()> {
    let local_filled = cached_filled.max(tracked_filled);
    anyhow::ensure!(
        local_filled <= report.quantity,
        "local filled quantity {local_filled} exceeds order quantity {} for {}",
        report.quantity,
        report.venue_order_id,
    );
    let confirmed_filled = match confirmed_filled {
        Some(qty) => {
            non_negative_quantity(qty, report.quantity.precision, "confirmed filled quantity")?
        }
        None => Quantity::zero(report.quantity.precision),
    };
    anyhow::ensure!(
        confirmed_filled <= report.quantity,
        "confirmed filled quantity {confirmed_filled} exceeds order quantity {} for {}",
        report.quantity,
        report.venue_order_id,
    );
    let capped = report.filled_qty.min(local_filled.max(confirmed_filled));
    report.filled_qty = capped;
    normalize_terminal_order_report_quantity(report);
    anyhow::ensure!(
        report.filled_qty <= report.quantity,
        "filled quantity {} exceeds order quantity {} for {}",
        report.filled_qty,
        report.quantity,
        report.venue_order_id,
    );
    anyhow::ensure!(
        report.order_status != OrderStatus::Filled || report.filled_qty == report.quantity,
        "Filled order {} has filled quantity {} but order quantity {}",
        report.venue_order_id,
        report.filled_qty,
        report.quantity,
    );
    Ok(())
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
            "Normalizing terminal order report {} quantity from {} to confirmed fills {}",
            report.venue_order_id,
            report.quantity,
            report.filled_qty,
        );
        report.quantity = report.filled_qty;
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::TradeId,
        types::{Currency, Money, Price},
    };
    use rstest::rstest;
    use rust_decimal_macros::dec;

    use super::*;
    use crate::http::{
        models::GammaMarket,
        parse::{create_instrument_from_def, parse_gamma_market},
    };

    fn instrument_for_open_order(order: &PolymarketOpenOrder) -> InstrumentAny {
        let mut market: GammaMarket =
            serde_json::from_str(include_str!("../../test_data/gamma_market.json")).unwrap();
        market.condition_id = order.market.to_string();
        market.clob_token_ids =
            serde_json::to_string(&[order.asset_id.as_str(), "synthetic-other-token"]).unwrap();
        market.outcomes = serde_json::to_string(&[order.outcome.as_str(), "Other"]).unwrap();
        market.fees_enabled = Some(false);
        market.fee_schedule = None;
        let definition = parse_gamma_market(&market).unwrap().remove(0);
        create_instrument_from_def(&definition, UnixNanos::default()).unwrap()
    }

    fn instrument_for_position(position: &DataApiPosition) -> InstrumentAny {
        let mut market: GammaMarket =
            serde_json::from_str(include_str!("../../test_data/gamma_market.json")).unwrap();
        market.condition_id = position.condition_id.clone();
        market.clob_token_ids =
            serde_json::to_string(&[position.asset.as_str(), "synthetic-other-token"]).unwrap();
        market.outcomes = serde_json::to_string(&["Yes", "No"]).unwrap();
        market.fees_enabled = Some(false);
        market.fee_schedule = None;
        let definition = parse_gamma_market(&market).unwrap().remove(0);
        create_instrument_from_def(&definition, UnixNanos::default()).unwrap()
    }

    fn test_position() -> DataApiPosition {
        DataApiPosition {
            asset: "123".to_string(),
            condition_id: "0xaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
                .to_string(),
            size: Decimal::from_str_exact("1.000001").unwrap(),
            avg_price: Some(Decimal::from_str_exact("0.123456789012345678").unwrap()),
        }
    }

    fn position_map(position: &DataApiPosition) -> AtomicMap<Ustr, InstrumentAny> {
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from(position.asset.as_str()),
            instrument_for_position(position),
        );
        instruments
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
            Money::zero(Currency::pUSD()),
            LiquiditySide::Taker,
            None,
            None,
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
        )];

        cap_order_reports_to_confirmed_fills(&mut reports, &fills).unwrap();

        assert_eq!(reports[0].filled_qty, Quantity::from("4.0000"));
    }

    #[rstest]
    fn test_cumulative_cap_uses_max_for_overlapping_cache_tracker_and_confirmed_evidence() {
        let mut report = OrderStatusReport::new(
            AccountId::from("POLY-001"),
            InstrumentId::from("TEST.POLYMARKET"),
            None,
            VenueOrderId::from("V-OVERLAP"),
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
        );

        cap_order_report_filled_qty(
            &mut report,
            Quantity::from("4.0000"),
            Quantity::from("4.0000"),
            Some(dec!(4.0000)),
        )
        .unwrap();

        assert_eq!(report.filled_qty, Quantity::from("4.0000"));
    }

    #[rstest]
    fn test_confirmed_filled_quantities_returns_overflow_error() {
        let error = checked_confirmed_filled_total(
            Decimal::MAX,
            Decimal::ONE,
            VenueOrderId::from("V-OVERFLOW"),
        )
        .expect_err("overflow must be surfaced");

        assert!(error.to_string().contains("overflow"));
    }

    #[rstest]
    fn test_known_order_fill_aggregate_rejects_confirmed_overfill() {
        let account_id = AccountId::from("POLY-001");
        let instrument_id = InstrumentId::from("TEST.POLYMARKET");
        let venue_order_id = VenueOrderId::from("V-KNOWN-OVERFILL");
        let tracker = OrderFillTrackerMap::new();
        tracker.register(
            venue_order_id,
            Quantity::from("10.0000"),
            OrderSide::Sell,
            instrument_id,
            4,
            4,
        );
        let fills = ["T-KNOWN-1", "T-KNOWN-2"]
            .into_iter()
            .map(|trade_id| {
                FillReport::new(
                    account_id,
                    instrument_id,
                    venue_order_id,
                    TradeId::from(trade_id),
                    OrderSide::Sell,
                    Quantity::from("6.0000"),
                    Price::from("0.5000"),
                    Money::zero(Currency::pUSD()),
                    LiquiditySide::Taker,
                    None,
                    None,
                    UnixNanos::from(1),
                    UnixNanos::from(1),
                    None,
                )
            })
            .collect::<Vec<_>>();

        let error = validate_known_order_fill_aggregates(&fills, &tracker)
            .expect_err("known order aggregate overfill must fail closed");

        assert!(error.to_string().contains("exceeds submitted quantity"));
    }

    #[rstest]
    #[case::below_threshold("99.995", Some("99.995"))]
    #[case::at_threshold("99.990", None)]
    fn normalizes_confirmed_dust_residual_to_order_quantity(
        #[case] confirmed: &str,
        #[case] expected_quantity: Option<&str>,
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

        let result = cap_order_reports_to_confirmed_fills(&mut reports, &fills);

        if let Some(expected_quantity) = expected_quantity {
            result.unwrap();
            assert_eq!(reports[0].quantity, Quantity::from(expected_quantity));
            assert_eq!(reports[0].filled_qty, Quantity::from(confirmed));
        } else {
            let error = result.expect_err("non-dust partial evidence cannot remain Filled");
            assert!(error.to_string().contains("Filled order"));
        }
    }

    #[rstest]
    fn trades_params_for_window_uses_exclusive_after_unix_seconds() {
        let start = UnixNanos::from(100 * NANOSECONDS_IN_SECOND);
        let end = UnixNanos::from(250 * NANOSECONDS_IN_SECOND);

        let params = trades_params_for_window(Some(start), Some(end));

        assert_eq!(params.after, Some(99));
        assert_eq!(params.before, Some(250));
    }

    fn unmapped_open_order() -> crate::http::models::PolymarketOpenOrder {
        crate::http::models::PolymarketOpenOrder {
            associate_trades: None,
            id: "0xid".to_string(),
            status: crate::common::enums::PolymarketOrderStatus::Live,
            market: Ustr::from("0xmarket"),
            original_size: rust_decimal_macros::dec!(10),
            outcome: crate::common::enums::PolymarketOutcome::yes(),
            maker_address: "0xmaker".to_string(),
            owner: "owner".to_string(),
            price: rust_decimal_macros::dec!(0.5),
            side: crate::common::enums::PolymarketOrderSide::Buy,
            size_matched: rust_decimal_macros::dec!(0),
            asset_id: Ustr::from("token"),
            expiration: None,
            order_type: crate::common::enums::PolymarketOrderType::GTC,
            created_at: 1_703_875_200,
        }
    }

    #[rstest]
    fn in_scope_unmapped_open_order_errors() {
        let error = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("in-scope open-order miss must fail");

        let message = error.to_string();

        assert!(message.contains("unmapped in-scope open order"));
        assert!(message.contains("set instrument_config.load_ids"));
    }

    #[rstest]
    fn named_load_ids_unmapped_open_order_names_failed_load() {
        let instrument_id = InstrumentId::from("0xmarket-token.POLYMARKET");
        let error = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
            Some(std::slice::from_ref(&instrument_id)),
        )
        .expect_err("named in-scope open-order miss must fail");
        let message = error.to_string();

        assert!(message.contains("unmapped in-scope open order"));
        assert!(message.contains("in instrument_config.load_ids but was not loaded"));
    }

    #[rstest]
    fn out_of_scope_unmapped_open_order_is_dropped() {
        let scoped = InstrumentId::from("OTHER.POLYMARKET");

        let (reports, filtered) = build_order_reports_from_orders(
            &[unmapped_open_order()],
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
            Some(std::slice::from_ref(&scoped)),
        )
        .expect("out-of-scope open-order miss is dropped");

        assert!(reports.is_empty());
        assert_eq!(filtered, 1);
    }

    #[rstest]
    fn test_build_order_reports_valid_then_malformed_relevant_row_returns_error() {
        let valid: PolymarketOpenOrder =
            serde_json::from_str(include_str!("../../test_data/http_open_order.json")).unwrap();
        let instrument = instrument_for_open_order(&valid);
        let instruments = AtomicMap::new();
        instruments.insert(valid.asset_id, instrument);
        let mut invalid = valid.clone();
        invalid.created_at = 0;

        let result = build_order_reports_from_orders(
            &[valid, invalid],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
            None,
        );

        assert!(
            result.is_err(),
            "a malformed later row must discard the valid prefix"
        );
    }

    #[rstest]
    fn in_scope_unmapped_position_errors() {
        let position = test_position();
        let error = build_position_reports_scoped(
            &[position],
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            None,
            UnixNanos::from(1),
        )
        .expect_err("in-scope position miss must fail");

        let message = error.to_string();

        assert!(message.contains("unmapped in-scope position"));
        assert!(message.contains("set instrument_config.load_ids"));
    }

    #[rstest]
    fn test_position_binding_rejects_wrong_condition_before_zero_or_dust() {
        let mut position = test_position();
        let instruments = position_map(&position);
        position.condition_id =
            "0xbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_string();
        position.size = Decimal::ZERO;

        let error = build_position_reports_scoped(
            &[position],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            None,
            UnixNanos::from(1),
        )
        .expect_err("wrong condition must fail before zero exclusion");

        assert!(
            error
                .to_string()
                .contains("does not match instrument condition")
        );
    }

    #[rstest]
    fn test_position_binding_accepts_equivalent_condition_case() {
        let mut position = test_position();
        let instruments = position_map(&position);
        position.condition_id = position.condition_id.to_ascii_uppercase();

        let reports = build_position_reports_scoped(
            &[position],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            None,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(reports.len(), 1);
    }

    #[rstest]
    fn test_position_report_uses_loaded_instrument_id() {
        let mut position = test_position();
        let instruments = position_map(&position);
        let expected = instruments
            .get_cloned(&Ustr::from(position.asset.as_str()))
            .unwrap()
            .id();
        position.condition_id = position.condition_id.to_ascii_uppercase();

        let reports = build_position_reports_scoped(
            &[position],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            None,
            UnixNanos::from(1),
        )
        .unwrap();

        assert_eq!(reports[0].instrument_id, expected);
    }

    #[rstest]
    fn test_position_numeric_conversion_round_trips_exactly() {
        let position = test_position();
        let instruments = position_map(&position);

        let reports = build_position_reports_scoped(
            std::slice::from_ref(&position),
            &instruments,
            AccountId::from("POLY-001"),
            None,
            None,
            UnixNanos::from(1),
        )
        .unwrap();
        assert_eq!(reports[0].quantity.as_decimal(), position.size);

        let mut over_precision = position;
        over_precision.size = Decimal::from_str_exact("1.0000001").unwrap();
        assert!(
            build_position_reports_scoped(
                &[over_precision],
                &instruments,
                AccountId::from("POLY-001"),
                None,
                None,
                UnixNanos::from(1),
            )
            .is_err()
        );
    }

    fn maker_trade_for_scope(owner: &str, maker_asset: &str) -> PolymarketTradeReport {
        let mut trade: PolymarketTradeReport =
            serde_json::from_str(include_str!("../../test_data/http_trade_report.json")).unwrap();
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.asset_id = Ustr::from("999");
        trade.match_time = "not-a-timestamp".to_string();
        trade.maker_orders.truncate(1);
        trade.maker_orders[0].owner = owner.to_string();
        trade.maker_orders[0].asset_id = Ustr::from(maker_asset);
        trade
    }

    #[rstest]
    fn test_role_aware_lookback_marks_owned_cross_asset_maker_time_failure() {
        let trade = maker_trade_for_scope("owned-api-key", "123");
        let position = DataApiPosition {
            asset: "123".to_string(),
            condition_id: trade.market.to_string(),
            size: dec!(1),
            avg_price: None,
        };
        let instruments = position_map(&position);
        let instrument_id = instruments.get_cloned(&Ustr::from("123")).unwrap().id();
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xnot-the-maker",
            api_key: "owned-api-key",
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (retained, untimestamped) = trades_in_lookback_scope(
            vec![trade],
            UnixNanos::from(1),
            &ctx,
            &instruments,
            None,
            None,
            Some(std::slice::from_ref(&instrument_id)),
        )
        .unwrap();

        assert!(retained.is_empty());
        assert_eq!(untimestamped, 1);
    }

    #[rstest]
    fn test_role_aware_lookback_ignores_genuinely_unrelated_time_failure() {
        let trade = maker_trade_for_scope("foreign-api-key", "999");
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xnot-the-maker",
            api_key: "owned-api-key",
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };
        let scoped = InstrumentId::from("OTHER.POLYMARKET");

        let (retained, untimestamped) = trades_in_lookback_scope(
            vec![trade],
            UnixNanos::from(1),
            &ctx,
            &AtomicMap::new(),
            None,
            None,
            Some(std::slice::from_ref(&scoped)),
        )
        .unwrap();

        assert!(retained.is_empty());
        assert_eq!(untimestamped, 0);
    }

    #[rstest]
    fn test_maker_builder_ignores_malformed_time_for_owned_leg_outside_instrument_filter() {
        let trade = maker_trade_for_scope("owned-api-key", "123");
        let position = DataApiPosition {
            asset: "123".to_string(),
            condition_id: trade.market.to_string(),
            size: dec!(1),
            avg_price: None,
        };
        let instruments = position_map(&position);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0xnot-the-maker",
            api_key: "owned-api-key",
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let (reports, discards) = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            Some(InstrumentId::from("OTHER.POLYMARKET")),
            UnixNanos::from(1),
            None,
        )
        .expect("an owned maker leg outside the requested instrument is unrelated");

        assert!(reports.is_empty());
        assert_eq!(discards, FillBuildDiscards::default());
    }
}
