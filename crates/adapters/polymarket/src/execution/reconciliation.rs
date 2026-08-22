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
    types::{Currency, Quantity},
};
use rust_decimal::Decimal;
use ustr::Ustr;

use super::{
    order_fill_tracker::OrderFillTrackerMap,
    parse::{
        build_maker_fill_report, instrument_fee_exponent, instrument_taker_fee, parse_fill_report,
        parse_order_status_report, parse_timestamp,
    },
};
use crate::{
    common::{
        consts::{DUST_POSITION_THRESHOLD, DUST_SNAP_THRESHOLD_DEC, USDC_DECIMALS},
        enums::{PolymarketLiquiditySide, PolymarketOutcome, PolymarketTradeStatus},
        models::is_owned_by_account,
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

fn validate_instrument_binding(
    instrument: &InstrumentAny,
    condition_id: &str,
    outcome: PolymarketOutcome,
) -> anyhow::Result<()> {
    let InstrumentAny::BinaryOption(binary) = instrument else {
        anyhow::bail!("expected Polymarket BinaryOption instrument, found {instrument:?}");
    };
    let instrument_condition = binary
        .info
        .as_ref()
        .and_then(|info| info.get_str("condition_id"))
        .context("Polymarket instrument is missing condition_id metadata")?;

    anyhow::ensure!(
        instrument_condition.eq_ignore_ascii_case(condition_id),
        "provider condition {condition_id} does not match instrument condition {instrument_condition}",
    );
    let instrument_outcome = binary
        .outcome
        .context("Polymarket instrument is missing outcome metadata")?;
    anyhow::ensure!(
        instrument_outcome.as_str() == outcome.as_str(),
        "provider outcome {outcome} does not match instrument outcome {instrument_outcome}",
    );

    Ok(())
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
    /// Confirmed trades dropped from a bounded report because their event time is invalid.
    pub untimestamped_trades: usize,
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
    lookback_start: Option<UnixNanos>,
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
                let ts_event = parse_timestamp(&trade.match_time);
                let instrument_id =
                    instrument_id_from_market_token(trade.market.as_str(), trade.asset_id.as_str());
                let in_load_ids_scope = instrument_in_load_ids_scope(instrument_id, load_ids);

                if !trade_in_lookback_window(
                    ts_event,
                    lookback_start,
                    in_load_ids_scope,
                    &trade.id,
                    &mut discards,
                ) {
                    continue;
                }
                discards.unowned_maker_trades += 1;
                log::debug!(
                    "Confirmed maker trade {} holds no maker order owned by the account",
                    trade.id,
                );
                continue;
            }

            let mut selected_maker_orders = Vec::new();

            for mo in &trade.maker_orders {
                if !mo.is_owned_by(ctx.user_address, ctx.api_key) {
                    continue;
                }
                let token_id = mo.asset_id;
                let instrument = match instruments.get_cloned(&token_id) {
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

                validate_instrument_binding(&instrument, trade.market.as_str(), mo.outcome)?;
                selected_maker_orders.push((mo, instrument));
            }

            if selected_maker_orders.is_empty() {
                continue;
            }

            let ts_event = parse_timestamp(&trade.match_time);
            let in_load_ids_scope = selected_maker_orders
                .iter()
                .any(|(_, instrument)| instrument_in_load_ids_scope(instrument.id(), load_ids));

            if !trade_in_lookback_window(
                ts_event,
                lookback_start,
                in_load_ids_scope,
                &trade.id,
                &mut discards,
            ) {
                continue;
            }
            let ts_event = ts_event.unwrap_or_else(|| ctx.clock.get_time_ns());

            for (mo, instrument) in selected_maker_orders {
                let instrument_id = instrument.id();
                let price_prec = instrument.price_precision();
                let size_prec = instrument.size_precision();

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
            if !is_owned_by_account(
                &trade.maker_address,
                &trade.owner,
                ctx.user_address,
                ctx.api_key,
            ) {
                log::debug!(
                    "Dropping confirmed taker trade {} not owned by the account",
                    trade.id
                );
                continue;
            }

            let token_id = trade.asset_id;
            let instrument = match instruments.get_cloned(&token_id) {
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

            validate_instrument_binding(&instrument, trade.market.as_str(), trade.outcome)?;
            let ts_event = parse_timestamp(&trade.match_time);
            let in_load_ids_scope = instrument_in_load_ids_scope(instrument_id, load_ids);

            if !trade_in_lookback_window(
                ts_event,
                lookback_start,
                in_load_ids_scope,
                &trade.id,
                &mut discards,
            ) {
                continue;
            }
            let price_prec = instrument.price_precision();
            let size_prec = instrument.size_precision();
            let taker_fee_rate = instrument_taker_fee(&instrument);
            let fee_exponent = instrument_fee_exponent(&instrument);

            let report = parse_fill_report(
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
            )
            .with_context(|| format!("failed to build taker fill report for trade {}", trade.id))?;
            reports.push(report);
        }
    }

    Ok((reports, discards))
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    ctx: &FillContext<'_>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<(Vec<OrderStatusReport>, usize)> {
    let mut reports = Vec::new();
    let mut filtered = 0usize;

    for order in orders {
        if !is_owned_by_account(
            &order.maker_address,
            &order.owner,
            ctx.user_address,
            ctx.api_key,
        ) {
            log::debug!("Dropping open order {} not owned by the account", order.id);
            filtered += 1;
            continue;
        }

        let token_id = order.asset_id;
        let instrument = match instruments.get_cloned(&token_id) {
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

        validate_instrument_binding(&instrument, order.market.as_str(), order.outcome)?;
        let price_prec = instrument.price_precision();
        let size_prec = instrument.size_precision();

        let report = parse_order_status_report(
            order,
            instrument_id,
            ctx.account_id,
            None,
            price_prec,
            size_prec,
            ts_init,
        );
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
            let instrument_id = instrument_id_from_market_token(&p.condition_id, &p.asset);
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

pub(crate) fn retain_mapped_position_reports(
    reports: Vec<PositionStatusReport>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    load_ids: Option<&[InstrumentId]>,
) -> anyhow::Result<Vec<PositionStatusReport>> {
    let mut kept = Vec::with_capacity(reports.len());

    for report in reports {
        if position_instrument_loaded(report.instrument_id, instruments) {
            kept.push(report);
            continue;
        }

        if instrument_in_load_ids_scope(report.instrument_id, load_ids) {
            anyhow::bail!(unmapped_in_scope_message(
                "position",
                report.instrument_id,
                None,
                load_ids,
            ));
        }
        log::debug!(
            "Dropping out-of-scope unmapped position instrument {}",
            report.instrument_id
        );
    }

    Ok(kept)
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

    let (mut order_reports, orders_filtered) =
        build_order_reports_from_orders(&orders, instruments, ctx, None, ts_init, load_ids)?;

    let trades = http_client
        .get_trades(trades_params_for_window(
            lookback_start,
            lookback_start.map(|_| ts_init),
        ))
        .await
        .context("failed to fetch trades for mass status")?;

    let (mut fill_reports, fill_discards) = build_fill_reports_from_trades(
        &trades,
        ctx,
        instruments,
        None,
        ts_init,
        load_ids,
        lookback_start,
    )?;

    if fill_discards.unowned_maker_trades > 0 {
        log::error!(
            "Mass status is missing {} confirmed maker trade(s) holding no maker order owned by \
             the account; executed quantity may be understated",
            fill_discards.unowned_maker_trades,
        );
    }

    fill_tracker.snap_fill_reports(&mut fill_reports);

    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let position_reports = retain_mapped_position_reports(
        build_position_reports(&positions, ctx.account_id, ts_init),
        instruments,
        load_ids,
    )?;

    log::debug!(
        "Generated mass status: {} orders ({} filtered), {} fills ({} instrument-filtered, \
         {} in-scope historical misses, {} unowned maker trades, {} untimestamped trades), {} \
         positions",
        order_reports.len(),
        orders_filtered,
        fill_reports.len(),
        fill_discards.unmapped_instruments,
        fill_discards.in_scope_historical,
        fill_discards.unowned_maker_trades,
        fill_discards.untimestamped_trades,
        position_reports.len(),
    );

    if lookback_start.is_none() {
        cap_order_reports_to_confirmed_fills(&mut order_reports, &fill_reports);
    }

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    if let Some(lookback_start) = lookback_start {
        let reported_orders: AHashSet<VenueOrderId> = order_reports
            .iter()
            .map(|report| report.venue_order_id)
            .collect();
        let reports_complete = fill_discards.in_scope_historical == 0
            && fill_discards.unowned_maker_trades == 0
            && fill_discards.untimestamped_trades == 0
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

fn position_instrument_loaded(
    instrument_id: InstrumentId,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
) -> bool {
    let symbol = instrument_id.symbol.as_str();
    symbol
        .rsplit_once('-')
        .is_some_and(|(_, token_id)| instruments.contains_key(&Ustr::from(token_id)))
}

fn trade_in_lookback_window(
    ts_event: Option<UnixNanos>,
    lookback_start: Option<UnixNanos>,
    in_load_ids_scope: bool,
    trade_id: &str,
    discards: &mut FillBuildDiscards,
) -> bool {
    let Some(cutoff) = lookback_start else {
        return true;
    };

    match ts_event {
        Some(ts_event) => ts_event >= cutoff,
        None => {
            if in_load_ids_scope {
                discards.untimestamped_trades += 1;
            } else {
                log::debug!(
                    "Dropping out-of-scope historical trade {trade_id} with unparsable match_time"
                );
            }
            false
        }
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
) {
    let confirmed_by_order = confirmed_filled_quantities(fill_reports);

    for report in order_reports {
        let local_filled = Quantity::zero(report.quantity.precision);
        cap_order_report_filled_qty(
            report,
            local_filled,
            confirmed_by_order.get(&report.venue_order_id).copied(),
        );
    }
}

pub(crate) fn confirmed_filled_quantities(
    fill_reports: &[FillReport],
) -> AHashMap<VenueOrderId, Decimal> {
    let mut confirmed_by_order = AHashMap::new();
    for fill in fill_reports {
        *confirmed_by_order.entry(fill.venue_order_id).or_default() += fill.last_qty.as_decimal();
    }

    confirmed_by_order
}

pub(crate) fn cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    confirmed_filled: Option<Decimal>,
) {
    let confirmed_filled = confirmed_filled
        .and_then(|qty| Quantity::from_decimal_dp(qty, report.quantity.precision).ok())
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    let capped = report.filled_qty.min(local_filled.max(confirmed_filled));
    report.filled_qty = capped;
    normalize_terminal_order_report_quantity(report);
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
        types::{Money, Price},
    };
    use rstest::rstest;

    use super::*;

    const TEST_CONDITION_ID: &str =
        "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917";
    const TEST_TOKEN_ID: &str =
        "71321045679252212594626385532706912750332728571942532289631379312455583992563";
    const TEST_USER_ADDRESS: &str = "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266";
    const TEST_API_KEY: &str = "00000000-0000-0000-0000-000000000001";

    fn test_instrument() -> InstrumentAny {
        let def = crate::http::parse::PolymarketInstrumentDef {
            symbol: Ustr::from(format!("{TEST_CONDITION_ID}-{TEST_TOKEN_ID}").as_str()),
            token_id: Ustr::from(TEST_TOKEN_ID),
            condition_id: Ustr::from(TEST_CONDITION_ID),
            market_id: "test-market".to_string(),
            question_id: None,
            outcome: crate::common::enums::PolymarketOutcome::yes(),
            question: "Test market?".to_string(),
            description: None,
            price_precision: 3,
            tick_size: Decimal::new(1, 3),
            min_size: None,
            maker_fee: None,
            taker_fee: None,
            start_date: None,
            end_date: None,
            active: true,
            closed: false,
            market_slug: None,
            neg_risk: None,
            fee_schedule: None,
            game_id: None,
        };

        crate::http::parse::create_instrument_from_def(&def, UnixNanos::from(1))
            .expect("valid test instrument")
    }

    fn test_instruments() -> AtomicMap<Ustr, InstrumentAny> {
        let instruments = AtomicMap::new();
        instruments.insert(Ustr::from(TEST_TOKEN_ID), test_instrument());
        instruments
    }

    fn test_fill_context() -> FillContext<'static> {
        FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: TEST_USER_ADDRESS,
            api_key: TEST_API_KEY,
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        }
    }

    fn confirmed_taker_trade() -> PolymarketTradeReport {
        serde_json::from_str(include_str!("../../test_data/http_trade_report.json"))
            .expect("valid trade fixture")
    }

    fn open_order() -> PolymarketOpenOrder {
        serde_json::from_str(include_str!("../../test_data/http_open_order.json"))
            .expect("valid open-order fixture")
    }

    #[rstest]
    fn foreign_confirmed_taker_trade_is_ignored() {
        let mut trade = confirmed_taker_trade();
        trade.maker_address = "0x1111111111111111111111111111111111111111".to_string();
        trade.owner = "foreign-api-key".to_string();

        let (reports, _) = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            None,
            UnixNanos::from(1),
            None,
            None,
        )
        .expect("foreign taker trade is outside local report scope");

        assert!(reports.is_empty());
    }

    #[rstest]
    fn confirmed_taker_trade_with_wrong_condition_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.market =
            Ustr::from("0x1111111111111111111111111111111111111111111111111111111111111111");

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            None,
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned trade with contradictory condition must fail");

        assert!(error.to_string().contains("condition"));
    }

    #[rstest]
    fn confirmed_taker_trade_with_wrong_outcome_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            None,
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned trade with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
    }

    #[rstest]
    fn owned_maker_leg_with_wrong_outcome_fails_binding() {
        let mut trade = confirmed_taker_trade();
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.maker_orders[0].owner = TEST_API_KEY.to_string();
        trade.maker_orders[0].outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_fill_reports_from_trades(
            &[trade],
            &test_fill_context(),
            &test_instruments(),
            None,
            UnixNanos::from(1),
            None,
            None,
        )
        .expect_err("owned maker leg with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
    }

    #[rstest]
    fn owned_open_order_with_wrong_condition_fails_binding() {
        let mut order = open_order();
        order.market =
            Ustr::from("0x1111111111111111111111111111111111111111111111111111111111111111");

        let error = build_order_reports_from_orders(
            &[order],
            &test_instruments(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("owned open order with contradictory condition must fail");

        assert!(error.to_string().contains("condition"));
    }

    #[rstest]
    fn owned_open_order_with_wrong_outcome_fails_binding() {
        let mut order = open_order();
        order.outcome = crate::common::enums::PolymarketOutcome::no();

        let error = build_order_reports_from_orders(
            &[order],
            &test_instruments(),
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            None,
        )
        .expect_err("owned open order with contradictory outcome must fail");

        assert!(error.to_string().contains("outcome"));
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

        cap_order_reports_to_confirmed_fills(&mut reports, &fills);

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

        cap_order_reports_to_confirmed_fills(&mut reports, &fills);

        assert_eq!(reports[0].quantity, Quantity::from(expected_quantity));
        assert_eq!(reports[0].filled_qty, Quantity::from(confirmed));
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
            maker_address: TEST_USER_ADDRESS.to_string(),
            owner: TEST_API_KEY.to_string(),
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
            &test_fill_context(),
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
            &test_fill_context(),
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
            &test_fill_context(),
            None,
            UnixNanos::from(1),
            Some(std::slice::from_ref(&scoped)),
        )
        .expect("out-of-scope open-order miss is dropped");

        assert!(reports.is_empty());
        assert_eq!(filtered, 1);
    }

    #[rstest]
    fn in_scope_unmapped_position_errors() {
        let reports = vec![PositionStatusReport::new(
            AccountId::from("POLY-001"),
            InstrumentId::from("0xmarket-token.POLYMARKET"),
            PositionSideSpecified::Long,
            Quantity::from("10.000000"),
            UnixNanos::from(1),
            UnixNanos::from(1),
            None,
            None,
            None,
        )];

        let error = retain_mapped_position_reports(reports, &AtomicMap::new(), None)
            .expect_err("in-scope position miss must fail");

        let message = error.to_string();

        assert!(message.contains("unmapped in-scope position"));
        assert!(message.contains("set instrument_config.load_ids"));
    }
}
