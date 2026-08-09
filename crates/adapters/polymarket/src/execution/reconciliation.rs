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

/// Counts of venue orders dropped while building order reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct OrderBuildDiscards {
    /// Orders dropped because their instrument is not loaded.
    pub unmapped_instruments: usize,
    /// Orders dropped because a quantity or price cannot be represented.
    pub unrepresentable_orders: usize,
}

impl OrderBuildDiscards {
    pub(crate) const fn has_anomalies(&self) -> bool {
        self.unmapped_instruments > 0 || self.unrepresentable_orders > 0
    }
}

/// Counts of confirmed trade evidence dropped while building fill reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct FillBuildDiscards {
    /// Fill entries dropped because their instrument is not loaded.
    pub unmapped_instruments: usize,
    /// Trade rows dropped because their match timestamp cannot be parsed.
    pub invalid_timestamps: usize,
    /// Fill entries dropped because quantity or price cannot be represented.
    pub invalid_values: usize,
    /// Confirmed maker trades dropped because no maker order in the match is
    /// owned by the account.
    pub unowned_maker_trades: usize,
}

impl FillBuildDiscards {
    pub(crate) const fn has_anomalies(&self) -> bool {
        self.unmapped_instruments > 0
            || self.invalid_timestamps > 0
            || self.invalid_values > 0
            || self.unowned_maker_trades > 0
    }
}

/// Counts of venue positions dropped while building position reports.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) struct PositionBuildDiscards {
    /// Positions dropped because their positive size is below the dust threshold.
    pub dust_positions: usize,
    /// Positions dropped because their size is negative or cannot be represented as a quantity.
    pub unrepresentable_positions: usize,
    /// Non-flat positions dropped because their average price was outside (0, 1).
    pub invalid_avg_prices: usize,
    /// Flat reports withheld because the pass contains order or fill activity for the instrument.
    pub flat_withheld_activity: usize,
}

impl PositionBuildDiscards {
    pub(crate) const fn has_anomalies(&self) -> bool {
        self.unrepresentable_positions > 0 || self.invalid_avg_prices > 0
    }
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> (Vec<FillReport>, FillBuildDiscards) {
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

            let Some(ts_event) = parse_timestamp(&trade.match_time) else {
                discards.invalid_timestamps += 1;
                log::warn!(
                    "Skipping confirmed maker trade {}: invalid match_time {}",
                    trade.id,
                    trade.match_time,
                );
                continue;
            };

            for mo in &trade.maker_orders {
                if !mo.is_owned_by(ctx.user_address, ctx.api_key) {
                    continue;
                }
                let token_id = Ustr::from(mo.asset_id.as_str());
                let instrument = instruments.get_cloned(&token_id);
                let (instrument_id, price_prec, size_prec) = match instrument {
                    Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                    None => {
                        discards.unmapped_instruments += 1;
                        continue;
                    }
                };

                if let Some(filter_id) = instrument_filter
                    && instrument_id != filter_id
                {
                    continue;
                }

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

                match report {
                    Some(report) => reports.push(report),
                    None => {
                        discards.invalid_values += 1;
                        log::warn!(
                            "Skipping confirmed maker fill {}-{}: matched_amount {} or price {} is unrepresentable",
                            trade.id,
                            mo.order_id,
                            mo.matched_amount,
                            mo.price,
                        );
                    }
                }
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
                        discards.unmapped_instruments += 1;
                        continue;
                    }
                };

            if let Some(filter_id) = instrument_filter
                && instrument_id != filter_id
            {
                continue;
            }

            if parse_timestamp(&trade.match_time).is_none() {
                discards.invalid_timestamps += 1;
                log::warn!(
                    "Skipping confirmed taker trade {}: invalid match_time {}",
                    trade.id,
                    trade.match_time,
                );
                continue;
            }

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
            );

            match report {
                Some(report) => reports.push(report),
                None => {
                    discards.invalid_values += 1;
                    log::warn!(
                        "Skipping confirmed taker trade {}: size {} or price {} is unrepresentable",
                        trade.id,
                        trade.size,
                        trade.price,
                    );
                }
            }
        }
    }

    (reports, discards)
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> (Vec<OrderStatusReport>, OrderBuildDiscards) {
    let mut reports = Vec::new();
    let mut discards = OrderBuildDiscards::default();

    for order in orders {
        let token_id = Ustr::from(order.asset_id.as_str());
        let instrument = instruments.get_cloned(&token_id);
        let (instrument_id, price_prec, size_prec) = match instrument {
            Some(i) => (i.id(), i.price_precision(), i.size_precision()),
            None => {
                discards.unmapped_instruments += 1;
                continue;
            }
        };

        if let Some(filter_id) = instrument_filter
            && instrument_id != filter_id
        {
            continue;
        }

        let report = parse_order_status_report(
            order,
            instrument_id,
            account_id,
            None,
            price_prec,
            size_prec,
            ts_init,
        );

        match report {
            Some(report) => reports.push(report),
            None => discards.unrepresentable_orders += 1,
        }
    }

    (reports, discards)
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

/// Builds position status reports from Data API positions, filtering positive dust.
pub(crate) fn build_position_reports(
    positions: &[DataApiPosition],
    account_id: AccountId,
    ts: UnixNanos,
) -> (Vec<PositionStatusReport>, PositionBuildDiscards) {
    build_position_reports_with_activity(positions, account_id, ts, &AHashSet::new())
}

/// Builds position reports while withholding stale Flat rows for instruments with pass activity.
pub(crate) fn build_position_reports_with_activity(
    positions: &[DataApiPosition],
    account_id: AccountId,
    ts: UnixNanos,
    activity_instruments: &AHashSet<InstrumentId>,
) -> (Vec<PositionStatusReport>, PositionBuildDiscards) {
    let mut reports = Vec::new();
    let mut discards = PositionBuildDiscards::default();

    for p in positions {
        if p.size < Decimal::ZERO {
            discards.unrepresentable_positions += 1;
            log::warn!(
                "Skipping negative Data API position: condition_id={}, asset={}, size={}",
                p.condition_id,
                p.asset,
                p.size,
            );
            continue;
        }

        if p.size > Decimal::ZERO && p.size < DUST_POSITION_THRESHOLD {
            log::debug!(
                "Filtering dust position: {}-{}, size={}",
                p.condition_id,
                p.asset,
                p.size
            );
            discards.dust_positions += 1;
            continue;
        }

        let instrument_id =
            InstrumentId::from(format!("{}-{}.POLYMARKET", p.condition_id, p.asset).as_str());
        let quantity = match Quantity::from_decimal_dp(p.size, USDC_DECIMALS as u8) {
            Ok(quantity) => quantity,
            Err(e) => {
                discards.unrepresentable_positions += 1;
                log::warn!(
                    "Skipping invalid Data API position {}-{} size {}: {e}",
                    p.condition_id,
                    p.asset,
                    p.size,
                );
                continue;
            }
        };

        if p.size == Decimal::ZERO {
            if activity_instruments.contains(&instrument_id) {
                discards.flat_withheld_activity += 1;
                log::debug!(
                    "Withholding Flat Data API position {}-{} due to same-pass order or fill activity",
                    p.condition_id,
                    p.asset,
                );
                continue;
            }

            reports.push(PositionStatusReport::new(
                account_id,
                instrument_id,
                PositionSideSpecified::Flat,
                quantity,
                ts,
                ts,
                None,
                None,
                None,
            ));
            continue;
        }

        let avg_price = match p.avg_price {
            Some(avg_price) if avg_price > Decimal::ZERO && avg_price < Decimal::ONE => {
                Some(avg_price)
            }
            Some(avg_price) => {
                discards.invalid_avg_prices += 1;
                log::warn!(
                    "Skipping Data API position {}-{} with invalid avg_price {avg_price}",
                    p.condition_id,
                    p.asset,
                );
                continue;
            }
            None => None,
        };

        reports.push(PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSideSpecified::Long,
            quantity,
            ts,
            ts,
            None,
            None,
            avg_price,
        ));
    }

    (reports, discards)
}

fn mass_status_omission_error(
    order_discards: &OrderBuildDiscards,
    fill_discards: &FillBuildDiscards,
    position_discards: &PositionBuildDiscards,
) -> Option<String> {
    if !order_discards.has_anomalies()
        && !fill_discards.has_anomalies()
        && !position_discards.has_anomalies()
    {
        return None;
    }

    Some(format!(
        "Mass status omitted {} unmapped order(s), {} unrepresentable order(s), {} fill(s) with \
         unmapped instruments, {} fill(s) with invalid timestamps, {} fill(s) with invalid \
         values, {} unowned maker trade(s), {} unrepresentable position(s), and {} position(s) \
         with invalid average prices; the returned report carries no count of these omissions",
        order_discards.unmapped_instruments,
        order_discards.unrepresentable_orders,
        fill_discards.unmapped_instruments,
        fill_discards.invalid_timestamps,
        fill_discards.invalid_values,
        fill_discards.unowned_maker_trades,
        position_discards.unrepresentable_positions,
        position_discards.invalid_avg_prices,
    ))
}

fn mass_status_dust_warning(position_discards: &PositionBuildDiscards) -> Option<String> {
    (position_discards.dust_positions > 0).then(|| {
        format!(
            "{} position(s) below the dust threshold were omitted by policy",
            position_discards.dust_positions,
        )
    })
}

fn mass_status_flat_withheld_warning(position_discards: &PositionBuildDiscards) -> Option<String> {
    (position_discards.flat_withheld_activity > 0).then(|| {
        format!(
            "Mass status withheld {} Flat position report(s) due to same-pass order or fill activity",
            position_discards.flat_withheld_activity,
        )
    })
}

fn mass_status_lookback_warning(orders_removed: usize, trades_removed: usize) -> Option<String> {
    (orders_removed > 0 || trades_removed > 0).then(|| {
        format!(
            "Mass-status lookback removed {orders_removed} order row(s) and {trades_removed} trade row(s)"
        )
    })
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
    lookback_mins: Option<u64>,
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let ts_init = ctx.clock.get_time_ns();
    let cutoff = mass_status_cutoff(ts_init, lookback_mins);

    // Fetch orders
    let mut orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;
    let orders_before = orders.len();
    let orders_removed = cutoff.map_or(0, |cutoff| filter_orders_to_lookback(&mut orders, cutoff));

    let (mut order_reports, order_discards) =
        build_order_reports_from_orders(&orders, instruments, ctx.account_id, None, ts_init);

    // Fetch and parse fill reports
    let mut trades = http_client
        .get_trades(GetTradesParams::default())
        .await
        .context("failed to fetch trades for mass status")?;
    let trades_before = trades.len();
    let trades_removed = cutoff.map_or(0, |cutoff| filter_trades_to_lookback(&mut trades, cutoff));

    let (mut fill_reports, fill_discards) =
        build_fill_reports_from_trades(&trades, ctx, instruments, None, ts_init);

    // Snap dust drift on REST fills the same way the WS path does.
    // Commission stays as venue-reported.
    fill_tracker.snap_fill_reports(&mut fill_reports);

    // Position reports from Data API
    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let activity_instruments = order_reports
        .iter()
        .map(|report| report.instrument_id)
        .chain(fill_reports.iter().map(|report| report.instrument_id))
        .collect();
    let (position_reports, position_discards) = build_position_reports_with_activity(
        &positions,
        ctx.account_id,
        ts_init,
        &activity_instruments,
    );

    if let Some(message) =
        mass_status_omission_error(&order_discards, &fill_discards, &position_discards)
    {
        log::error!("{message}");
    }

    if let Some(message) = mass_status_dust_warning(&position_discards) {
        log::warn!("{message}");
    }

    if let Some(message) = mass_status_flat_withheld_warning(&position_discards) {
        log::warn!("{message}");
    }

    if let Some(message) = mass_status_lookback_warning(orders_removed, trades_removed) {
        log::warn!("{message}");
    }

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
            "Generated mass status: {} orders ({} instrument-filtered, {} unrepresentable), {} \
            fills ({} instrument-filtered, {} invalid timestamps, {} invalid values, {} unowned \
             maker trades), {} positions ({} dust-filtered, {} unrepresentable, {} invalid average \
             prices, {} Flat reports withheld for activity)",
            order_reports.len(),
            order_discards.unmapped_instruments,
            order_discards.unrepresentable_orders,
            fill_reports.len(),
            fill_discards.unmapped_instruments,
            fill_discards.invalid_timestamps,
            fill_discards.invalid_values,
            fill_discards.unowned_maker_trades,
            position_reports.len(),
            position_discards.dust_positions,
            position_discards.unrepresentable_positions,
            position_discards.invalid_avg_prices,
            position_discards.flat_withheld_activity,
        );
    }

    cap_order_reports_to_confirmed_fills(&mut order_reports, &fill_reports);

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
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
    use nautilus_core::time::get_atomic_clock_realtime;
    use nautilus_model::{
        enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
        identifiers::TradeId,
        types::{Money, Price},
    };
    use rstest::rstest;

    use super::*;

    #[derive(Clone, Copy)]
    enum UnrepresentableOrderField {
        OriginalSize,
        SizeMatched,
        Price,
    }

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn data_api_position(size: Decimal) -> DataApiPosition {
        DataApiPosition {
            asset: "123".to_string(),
            condition_id: "0xabc".to_string(),
            size,
            avg_price: Some(Decimal::new(5, 1)),
        }
    }

    #[rstest]
    fn dust_position_is_counted_when_report_is_omitted() {
        let position = data_api_position(DUST_POSITION_THRESHOLD / Decimal::from(2));

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.dust_positions, 1);
        assert_eq!(discards.unrepresentable_positions, 0);
    }

    #[rstest]
    fn position_at_dust_threshold_is_kept() {
        let position = data_api_position(DUST_POSITION_THRESHOLD);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(discards, PositionBuildDiscards::default());
    }

    #[rstest]
    #[case::negative(Decimal::NEGATIVE_ONE)]
    #[case::zero(Decimal::ZERO)]
    #[case::above_one(Decimal::new(15, 1))]
    fn invalid_avg_price_is_rejected_and_counted(#[case] avg_price: Decimal) {
        let mut position = data_api_position(Decimal::ONE);
        position.avg_price = Some(avg_price);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.invalid_avg_prices, 1);
        assert_eq!(discards.unrepresentable_positions, 0);
    }

    #[rstest]
    fn valid_avg_price_is_retained() {
        let position = data_api_position(Decimal::ONE);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].avg_px_open, Some(Decimal::new(5, 1)));
        assert_eq!(discards.invalid_avg_prices, 0);
    }

    #[rstest]
    fn unrepresentable_position_is_counted_when_report_is_omitted() {
        let position = data_api_position(Decimal::MAX);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.dust_positions, 0);
        assert_eq!(discards.unrepresentable_positions, 1);
    }

    #[rstest]
    fn negative_position_is_unrepresentable_not_dust() {
        let position = data_api_position(Decimal::NEGATIVE_ONE);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.dust_positions, 0);
        assert_eq!(discards.unrepresentable_positions, 1);
    }

    #[rstest]
    fn position_discards_accumulate_in_one_build() {
        let positions = [
            data_api_position(Decimal::new(5, 3)),
            data_api_position(Decimal::new(1, 3)),
            data_api_position(Decimal::NEGATIVE_ONE),
            data_api_position(Decimal::MAX),
        ];

        let (reports, discards) = build_position_reports(
            &positions,
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.dust_positions, 2);
        assert_eq!(discards.unrepresentable_positions, 2);
    }

    #[rstest]
    fn flat_position_is_reported_without_discard_count() {
        let mut position = data_api_position(Decimal::ZERO);
        position.avg_price = Some(Decimal::ZERO);
        let account_id = AccountId::from("POLYMARKET-001");
        let ts = UnixNanos::from(1);

        let (reports, discards) = build_position_reports(&[position], account_id, ts);

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].account_id, account_id);
        assert_eq!(
            reports[0].instrument_id,
            InstrumentId::from("0xabc-123.POLYMARKET"),
        );
        assert_eq!(reports[0].position_side, PositionSideSpecified::Flat);
        assert!(reports[0].quantity.is_zero());
        assert_eq!(reports[0].quantity.precision, USDC_DECIMALS as u8);
        assert_eq!(discards, PositionBuildDiscards::default());
    }

    #[rstest]
    fn flat_position_is_withheld_for_same_instrument_activity() {
        let position = data_api_position(Decimal::ZERO);
        let activity_instruments =
            AHashSet::from_iter([InstrumentId::from("0xabc-123.POLYMARKET")]);

        let (reports, discards) = build_position_reports_with_activity(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
            &activity_instruments,
        );

        assert!(reports.is_empty());
        assert_eq!(discards.flat_withheld_activity, 1);
        assert_eq!(discards.invalid_avg_prices, 0);
    }

    #[rstest]
    fn flat_position_is_reported_when_only_other_instrument_has_activity() {
        let position = data_api_position(Decimal::ZERO);
        let activity_instruments =
            AHashSet::from_iter([InstrumentId::from("0xother-456.POLYMARKET")]);

        let (reports, discards) = build_position_reports_with_activity(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
            &activity_instruments,
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].position_side, PositionSideSpecified::Flat);
        assert_eq!(discards, PositionBuildDiscards::default());
    }

    #[rstest]
    fn normal_position_has_no_build_discards() {
        let position = data_api_position(Decimal::ONE);

        let (reports, discards) = build_position_reports(
            &[position],
            AccountId::from("POLYMARKET-001"),
            UnixNanos::from(1),
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(discards, PositionBuildDiscards::default());
    }

    #[rstest]
    #[case::original_size(UnrepresentableOrderField::OriginalSize)]
    #[case::size_matched(UnrepresentableOrderField::SizeMatched)]
    #[case::price(UnrepresentableOrderField::Price)]
    fn unrepresentable_order_field_is_counted_without_dropping_normal_order(
        #[case] field: UnrepresentableOrderField,
    ) {
        let normal_order: PolymarketOpenOrder = load("http_open_order.json");
        let normal_venue_order_id = VenueOrderId::from(normal_order.id.as_str());
        let mut unrepresentable_order = normal_order.clone();
        unrepresentable_order.id = "unrepresentable-order".to_string();

        match field {
            UnrepresentableOrderField::OriginalSize => {
                unrepresentable_order.original_size = Decimal::MAX;
            }
            UnrepresentableOrderField::SizeMatched => {
                unrepresentable_order.size_matched = Decimal::MAX;
            }
            UnrepresentableOrderField::Price => {
                unrepresentable_order.price = Decimal::MAX;
            }
        }

        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(normal_order.asset_id, instrument);

        let (reports, discards) = build_order_reports_from_orders(
            &[unrepresentable_order, normal_order],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1),
        );

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].venue_order_id, normal_venue_order_id);
        assert_eq!(discards.unmapped_instruments, 0);
        assert_eq!(discards.unrepresentable_orders, 1);
    }

    #[rstest]
    fn filled_order_with_raw_zero_fill_below_snap_threshold_is_counted() {
        let mut order: PolymarketOpenOrder = load("http_open_order.json");
        order.original_size = DUST_SNAP_THRESHOLD_DEC / Decimal::from(2);
        order.size_matched = Decimal::ZERO;
        order.status = crate::common::enums::PolymarketOrderStatus::Matched;
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(order.asset_id, instrument);

        let (reports, discards) = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.unmapped_instruments, 0);
        assert_eq!(discards.unrepresentable_orders, 1);
    }

    #[rstest]
    #[case::maker(PolymarketLiquiditySide::Maker)]
    #[case::taker(PolymarketLiquiditySide::Taker)]
    fn invalid_trade_timestamp_is_counted_and_omitted(
        #[case] trader_side: PolymarketLiquiditySide,
    ) {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.trader_side = trader_side;
        trade.match_time = "invalid-match-time".to_string();
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000002",
            pusd: Currency::pUSD(),
            clock: get_atomic_clock_realtime(),
        };

        let (reports, discards) = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            UnixNanos::from(1_000_000_000u64),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.invalid_timestamps, 1);
        assert_eq!(discards.unmapped_instruments, 0);
        assert_eq!(discards.unowned_maker_trades, 0);
    }

    #[rstest]
    fn unrepresentable_maker_fill_values_are_omitted() {
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.maker_orders[0].matched_amount = Decimal::MAX;
        trade.maker_orders[0].price = Decimal::MAX;
        let token_id = trade.maker_orders[0].asset_id;
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(token_id, instrument);
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000002",
            pusd: Currency::pUSD(),
            clock: get_atomic_clock_realtime(),
        };

        let (reports, discards) = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            None,
            UnixNanos::from(1_000_000_000u64),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.invalid_values, 1);
        assert_eq!(discards.invalid_timestamps, 0);
        assert_eq!(discards.unmapped_instruments, 0);
    }

    #[rstest]
    fn mass_status_lookback_excludes_old_inputs_from_omission_counts() {
        let cutoff_secs = 2_000_000_000_u64;
        let cutoff = UnixNanos::from(cutoff_secs * NANOSECONDS_IN_SECOND);
        let mut order: PolymarketOpenOrder = load("http_open_order.json");
        order.created_at = cutoff_secs - 1;
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.match_time = (cutoff_secs - 1).to_string();
        let mut orders = vec![order];
        let mut trades = vec![trade];

        assert_eq!(filter_orders_to_lookback(&mut orders, cutoff), 1);
        assert_eq!(filter_trades_to_lookback(&mut trades, cutoff), 1);

        let (order_reports, order_discards) = build_order_reports_from_orders(
            &orders,
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            cutoff,
        );
        let (fill_reports, fill_discards) = build_fill_reports_from_trades(
            &trades,
            &FillContext {
                account_id: AccountId::from("POLY-001"),
                user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                api_key: "00000000-0000-0000-0000-000000000001",
                pusd: Currency::pUSD(),
                clock: get_atomic_clock_realtime(),
            },
            &AtomicMap::new(),
            None,
            cutoff,
        );

        assert!(order_reports.is_empty());
        assert!(fill_reports.is_empty());
        assert_eq!(order_discards, OrderBuildDiscards::default());
        assert_eq!(fill_discards, FillBuildDiscards::default());
    }

    #[rstest]
    fn mass_status_lookback_keeps_unknown_age_inputs_visible_to_builders() {
        let cutoff = UnixNanos::from(2_000_000_000_u64 * NANOSECONDS_IN_SECOND);
        let mut order: PolymarketOpenOrder = load("http_open_order.json");
        order.created_at = u64::MAX;
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.match_time = "invalid-match-time".to_string();
        let mut overflowing_trade = trade.clone();
        overflowing_trade.match_time = u64::MAX.to_string();
        let mut orders = vec![order];
        let mut trades = vec![trade, overflowing_trade];

        assert_eq!(filter_orders_to_lookback(&mut orders, cutoff), 0);
        assert_eq!(filter_trades_to_lookback(&mut trades, cutoff), 0);

        let (_, order_discards) = build_order_reports_from_orders(
            &orders,
            &AtomicMap::new(),
            AccountId::from("POLY-001"),
            None,
            cutoff,
        );
        let (_, fill_discards) = build_fill_reports_from_trades(
            &trades,
            &FillContext {
                account_id: AccountId::from("POLY-001"),
                user_address: "0xf39fd6e51aad88f6f4ce6ab8827279cfffb92266",
                api_key: "00000000-0000-0000-0000-000000000001",
                pusd: Currency::pUSD(),
                clock: get_atomic_clock_realtime(),
            },
            &AtomicMap::new(),
            None,
            cutoff,
        );

        assert_eq!(order_discards.unmapped_instruments, 1);
        assert_eq!(fill_discards.unmapped_instruments, 2);
    }

    #[rstest]
    #[case::maximum(u64::MAX)]
    #[case::milliseconds(1_722_000_000_000)]
    fn overflowing_order_timestamp_is_counted_as_unrepresentable(#[case] created_at: u64) {
        let mut order: PolymarketOpenOrder = load("http_open_order.json");
        order.created_at = created_at;
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = crate::http::parse::parse_gamma_market(&market).unwrap();
        let instrument =
            crate::http::parse::create_instrument_from_def(&defs[0], UnixNanos::from(1)).unwrap();
        let instruments = AtomicMap::new();
        instruments.insert(order.asset_id, instrument);

        let (reports, discards) = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLY-001"),
            None,
            UnixNanos::from(1),
        );

        assert!(reports.is_empty());
        assert_eq!(discards.unmapped_instruments, 0);
        assert_eq!(discards.unrepresentable_orders, 1);
    }

    #[rstest]
    fn mass_status_cutoff_saturates_absurd_lookback_to_zero() {
        assert_eq!(
            mass_status_cutoff(UnixNanos::from(1_000_000_000), Some(u64::MAX)),
            Some(UnixNanos::from(0)),
        );

        // This is the smallest minute count whose nanosecond product exceeds u64::MAX.
        assert_eq!(
            mass_status_cutoff(UnixNanos::from(100_000_000_000), Some(307_445_735)),
            Some(UnixNanos::from(0)),
        );

        assert_eq!(
            mass_status_cutoff(UnixNanos::from(100_000_000_000), Some(1_u64 << 62)),
            Some(UnixNanos::from(0)),
        );
    }

    #[rstest]
    fn mass_status_cutoff_subtracts_normal_lookback_exactly() {
        let ts_init = UnixNanos::from(3_700 * NANOSECONDS_IN_SECOND);

        assert_eq!(
            mass_status_cutoff(ts_init, Some(60)),
            Some(UnixNanos::from(100 * NANOSECONDS_IN_SECOND)),
        );
        assert_eq!(mass_status_cutoff(ts_init, None), None);
    }

    #[rstest]
    fn mass_status_omission_error_is_none_without_anomalies() {
        assert_eq!(
            mass_status_omission_error(
                &OrderBuildDiscards::default(),
                &FillBuildDiscards::default(),
                &PositionBuildDiscards::default(),
            ),
            None,
        );
    }

    #[rstest]
    #[case::unmapped_order(
        OrderBuildDiscards {
            unmapped_instruments: 1,
            ..OrderBuildDiscards::default()
        },
        FillBuildDiscards::default(),
        PositionBuildDiscards::default(),
        "1 unmapped order(s)",
    )]
    #[case::unrepresentable_order(
        OrderBuildDiscards {
            unrepresentable_orders: 1,
            ..OrderBuildDiscards::default()
        },
        FillBuildDiscards::default(),
        PositionBuildDiscards::default(),
        "1 unrepresentable order(s)",
    )]
    #[case::unmapped_fill_instrument(
        OrderBuildDiscards::default(),
        FillBuildDiscards {
            unmapped_instruments: 1,
            ..FillBuildDiscards::default()
        },
        PositionBuildDiscards::default(),
        "1 fill(s) with unmapped instruments",
    )]
    #[case::invalid_fill_timestamp(
        OrderBuildDiscards::default(),
        FillBuildDiscards {
            invalid_timestamps: 1,
            ..FillBuildDiscards::default()
        },
        PositionBuildDiscards::default(),
        "1 fill(s) with invalid timestamps",
    )]
    #[case::invalid_fill_values(
        OrderBuildDiscards::default(),
        FillBuildDiscards {
            invalid_values: 1,
            ..FillBuildDiscards::default()
        },
        PositionBuildDiscards::default(),
        "1 fill(s) with invalid values",
    )]
    #[case::unowned_maker_trade(
        OrderBuildDiscards::default(),
        FillBuildDiscards {
            unowned_maker_trades: 1,
            ..FillBuildDiscards::default()
        },
        PositionBuildDiscards::default(),
        "1 unowned maker trade(s)",
    )]
    #[case::unrepresentable_position(
        OrderBuildDiscards::default(),
        FillBuildDiscards::default(),
        PositionBuildDiscards {
            unrepresentable_positions: 1,
            ..PositionBuildDiscards::default()
        },
        "1 unrepresentable position(s)",
    )]
    #[case::invalid_position_avg_price(
        OrderBuildDiscards::default(),
        FillBuildDiscards::default(),
        PositionBuildDiscards {
            invalid_avg_prices: 1,
            ..PositionBuildDiscards::default()
        },
        "1 position(s) with invalid average prices",
    )]
    fn each_mass_status_anomaly_produces_an_error_with_its_own_count(
        #[case] order_discards: OrderBuildDiscards,
        #[case] fill_discards: FillBuildDiscards,
        #[case] position_discards: PositionBuildDiscards,
        #[case] expected: &str,
    ) {
        let message =
            mass_status_omission_error(&order_discards, &fill_discards, &position_discards)
                .expect("a single anomaly must produce an error message");

        assert!(message.contains(expected), "message was: {message}");
    }

    #[rstest]
    fn mass_status_omission_error_keeps_counts_with_their_labels() {
        let order_discards = OrderBuildDiscards {
            unmapped_instruments: 1,
            unrepresentable_orders: 2,
        };
        let fill_discards = FillBuildDiscards {
            unmapped_instruments: 3,
            invalid_timestamps: 4,
            ..FillBuildDiscards::default()
        };
        let position_discards = PositionBuildDiscards {
            unrepresentable_positions: 5,
            ..PositionBuildDiscards::default()
        };

        let message =
            mass_status_omission_error(&order_discards, &fill_discards, &position_discards)
                .expect("nonzero anomalies must produce an error message");

        assert!(message.contains("1 unmapped order(s)"));
        assert!(message.contains("2 unrepresentable order(s)"));
        assert!(message.contains("3 fill(s) with unmapped instruments"));
        assert!(message.contains("4 fill(s) with invalid timestamps"));
        assert!(message.contains("5 unrepresentable position(s)"));
        assert!(message.contains("returned report carries no count of these omissions"));
    }

    #[rstest]
    #[case::none(0, None)]
    #[case::one(
        1,
        Some(
            "Mass status withheld 1 Flat position report(s) due to same-pass order or fill activity"
        )
    )]
    fn mass_status_flat_warning_depends_only_on_withheld_count(
        #[case] flat_withheld_activity: usize,
        #[case] expected: Option<&str>,
    ) {
        let position_discards = PositionBuildDiscards {
            flat_withheld_activity,
            invalid_avg_prices: 7,
            ..PositionBuildDiscards::default()
        };

        assert_eq!(
            mass_status_flat_withheld_warning(&position_discards).as_deref(),
            expected,
        );
    }

    #[rstest]
    #[case::none(0, 0, None)]
    #[case::orders(
        2,
        0,
        Some("Mass-status lookback removed 2 order row(s) and 0 trade row(s)")
    )]
    #[case::trades(
        0,
        3,
        Some("Mass-status lookback removed 0 order row(s) and 3 trade row(s)")
    )]
    fn mass_status_lookback_warning_depends_on_removed_counts(
        #[case] orders_removed: usize,
        #[case] trades_removed: usize,
        #[case] expected: Option<&str>,
    ) {
        assert_eq!(
            mass_status_lookback_warning(orders_removed, trades_removed).as_deref(),
            expected,
        );
    }

    #[rstest]
    fn dust_alone_does_not_produce_an_omission_error() {
        let position_discards = PositionBuildDiscards {
            dust_positions: 1,
            ..PositionBuildDiscards::default()
        };

        assert_eq!(
            mass_status_omission_error(
                &OrderBuildDiscards::default(),
                &FillBuildDiscards::default(),
                &position_discards,
            ),
            None,
        );
    }

    #[rstest]
    #[case::none(0, None)]
    #[case::one(
        1,
        Some("1 position(s) below the dust threshold were omitted by policy")
    )]
    fn mass_status_dust_warning_depends_only_on_dust_count(
        #[case] dust_positions: usize,
        #[case] expected: Option<&str>,
    ) {
        let position_discards = PositionBuildDiscards {
            dust_positions,
            unrepresentable_positions: 7,
            ..PositionBuildDiscards::default()
        };

        assert_eq!(
            mass_status_dust_warning(&position_discards).as_deref(),
            expected,
        );
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
}
