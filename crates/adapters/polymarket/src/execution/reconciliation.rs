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

use std::fmt::Display;

use ahash::AHashMap;
use anyhow::Context;
use indexmap::IndexMap;
use nautilus_core::{UnixNanos, collections::AtomicMap, time::AtomicTime};
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
        ReportParseError, build_maker_fill_report, instrument_fee_exponent, instrument_taker_fee,
        parse_fill_report, parse_order_status_report, parse_timestamp,
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

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum PositionOmission {
    Dust,
    Zero,
    InvalidSize,
    InvalidAveragePrice,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum ReconciliationOmission {
    PendingTrade,
    FailedTrade,
    UnmappedOrder,
    InvalidOrder(ReportParseError),
    UnmappedFill,
    InvalidFill(ReportParseError),
    UnownedMakerTrade,
    Position(PositionOmission),
    LookbackOrder,
    LookbackFill,
}

impl ReconciliationOmission {
    const fn invalidates_snapshot(self) -> bool {
        match self {
            Self::PendingTrade
            | Self::UnmappedOrder
            | Self::InvalidOrder(_)
            | Self::UnmappedFill
            | Self::InvalidFill(_)
            | Self::UnownedMakerTrade
            | Self::Position(
                PositionOmission::InvalidSize | PositionOmission::InvalidAveragePrice,
            ) => true,
            Self::FailedTrade
            | Self::Position(PositionOmission::Dust | PositionOmission::Zero)
            | Self::LookbackOrder
            | Self::LookbackFill => false,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub(crate) struct ReconciliationOmissions {
    counts: IndexMap<ReconciliationOmission, usize>,
}

impl ReconciliationOmissions {
    pub(crate) fn record(&mut self, reason: ReconciliationOmission) {
        self.record_n(reason, 1);
    }

    pub(crate) fn record_n(&mut self, reason: ReconciliationOmission, count: usize) {
        if count > 0 {
            *self.counts.entry(reason).or_default() += count;
        }
    }

    #[must_use]
    #[cfg(test)]
    pub(crate) fn count(&self, reason: ReconciliationOmission) -> usize {
        self.counts.get(&reason).copied().unwrap_or_default()
    }

    #[must_use]
    pub(crate) fn is_empty(&self) -> bool {
        self.counts.is_empty()
    }

    pub(crate) fn merge(&mut self, other: Self) {
        for (reason, count) in other.counts {
            self.record_n(reason, count);
        }
    }

    fn ensure_authoritative(&self) -> anyhow::Result<()> {
        let invalidating = self
            .counts
            .iter()
            .filter(|(reason, _)| reason.invalidates_snapshot())
            .map(|(reason, count)| format!("{reason:?}={count}"))
            .collect::<Vec<_>>();
        anyhow::ensure!(
            invalidating.is_empty(),
            "Mass status is not authoritative: {}",
            invalidating.join(", "),
        );
        Ok(())
    }
}

impl Display for ReconciliationOmissions {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let mut separator = "";
        for (reason, count) in &self.counts {
            write!(f, "{separator}{reason:?}={count}")?;
            separator = ", ";
        }

        if separator.is_empty() {
            f.write_str("none")?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ReportSet<T> {
    pub reports: Vec<T>,
    pub omissions: ReconciliationOmissions,
}

impl<T> ReportSet<T> {
    fn new() -> Self {
        Self {
            reports: Vec::new(),
            omissions: ReconciliationOmissions::default(),
        }
    }

    fn omit(&mut self, reason: ReconciliationOmission) {
        self.omissions.record(reason);
    }
}

#[derive(Debug)]
struct ReconciliationSnapshot {
    orders: Vec<OrderStatusReport>,
    fills: Vec<FillReport>,
    positions: Vec<PositionStatusReport>,
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> ReportSet<FillReport> {
    let mut output = ReportSet::new();

    for trade in trades {
        match trade.status {
            PolymarketTradeStatus::Confirmed => {}
            PolymarketTradeStatus::Failed => {
                output.omit(ReconciliationOmission::FailedTrade);
                continue;
            }
            PolymarketTradeStatus::Matched
            | PolymarketTradeStatus::Mined
            | PolymarketTradeStatus::Retrying => {
                output.omit(ReconciliationOmission::PendingTrade);
                continue;
            }
        }

        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            if !trade
                .maker_orders
                .iter()
                .any(|mo| mo.is_owned_by(ctx.user_address, ctx.api_key))
            {
                output.omit(ReconciliationOmission::UnownedMakerTrade);
                log::debug!(
                    "Confirmed maker trade {} holds no maker order owned by the account",
                    trade.id,
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
                        output.omit(ReconciliationOmission::UnmappedFill);
                        continue;
                    }
                };

                if let Some(filter_id) = instrument_filter
                    && instrument_id != filter_id
                {
                    continue;
                }

                let Some(ts_event) = parse_timestamp(&trade.match_time) else {
                    output.omit(ReconciliationOmission::InvalidFill(
                        ReportParseError::Timestamp,
                    ));
                    continue;
                };
                let report = match build_maker_fill_report(
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
                ) {
                    Ok(report) => report,
                    Err(e) => {
                        output.omit(ReconciliationOmission::InvalidFill(e));
                        continue;
                    }
                };
                output.reports.push(report);
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
                        output.omit(ReconciliationOmission::UnmappedFill);
                        continue;
                    }
                };

            if let Some(filter_id) = instrument_filter
                && instrument_id != filter_id
            {
                continue;
            }

            let report = match parse_fill_report(
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
            ) {
                Ok(report) => report,
                Err(e) => {
                    output.omit(ReconciliationOmission::InvalidFill(e));
                    continue;
                }
            };
            output.reports.push(report);
        }
    }

    output
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> ReportSet<OrderStatusReport> {
    let mut output = ReportSet::new();

    for order in orders {
        let token_id = Ustr::from(order.asset_id.as_str());
        let instrument = instruments.get_cloned(&token_id);
        let (instrument_id, price_prec, size_prec) = match instrument {
            Some(i) => (i.id(), i.price_precision(), i.size_precision()),
            None => {
                output.omit(ReconciliationOmission::UnmappedOrder);
                continue;
            }
        };

        if let Some(filter_id) = instrument_filter
            && instrument_id != filter_id
        {
            continue;
        }

        let report = match parse_order_status_report(
            order,
            instrument_id,
            account_id,
            None,
            price_prec,
            size_prec,
            ts_init,
        ) {
            Ok(report) => report,
            Err(e) => {
                output.omit(ReconciliationOmission::InvalidOrder(e));
                continue;
            }
        };
        output.reports.push(report);
    }

    output
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
) -> ReportSet<PositionStatusReport> {
    let mut output = ReportSet::new();

    for position in positions {
        if position.size == Decimal::ZERO {
            output.omit(ReconciliationOmission::Position(PositionOmission::Zero));
            continue;
        }

        if position.size > Decimal::ZERO && position.size < DUST_POSITION_THRESHOLD {
            output.omit(ReconciliationOmission::Position(PositionOmission::Dust));
            continue;
        }

        let quantity = match Quantity::from_decimal_dp(position.size, USDC_DECIMALS as u8) {
            Ok(quantity) => quantity,
            Err(_) => {
                output.omit(ReconciliationOmission::Position(
                    PositionOmission::InvalidSize,
                ));
                continue;
            }
        };
        let Some(avg_price) = position
            .avg_price
            .filter(|price| *price > Decimal::ZERO && *price < Decimal::ONE)
        else {
            output.omit(ReconciliationOmission::Position(
                PositionOmission::InvalidAveragePrice,
            ));
            continue;
        };
        let instrument_id = InstrumentId::from(
            format!("{}-{}.POLYMARKET", position.condition_id, position.asset).as_str(),
        );
        output.reports.push(PositionStatusReport::new(
            account_id,
            instrument_id,
            PositionSideSpecified::Long,
            quantity,
            ts,
            ts,
            None,
            None,
            Some(avg_price),
        ));
    }

    output
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
    let orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;
    let trades = http_client
        .get_trades(GetTradesParams::default())
        .await
        .context("failed to fetch trades for mass status")?;
    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;
    let snapshot = build_reconciliation_snapshot(
        &orders,
        &trades,
        &positions,
        instruments,
        fill_tracker,
        ctx,
        lookback_mins,
        ts_init,
    )?;

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);
    mass_status.add_order_reports(snapshot.orders);
    mass_status.add_position_reports(snapshot.positions);
    mass_status.add_fill_reports(snapshot.fills);

    Ok(Some(mass_status))
}

#[expect(clippy::too_many_arguments)]
fn build_reconciliation_snapshot(
    orders: &[PolymarketOpenOrder],
    trades: &[PolymarketTradeReport],
    positions: &[DataApiPosition],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    fill_tracker: &OrderFillTrackerMap,
    ctx: &FillContext<'_>,
    lookback_mins: Option<u64>,
    ts_init: UnixNanos,
) -> anyhow::Result<ReconciliationSnapshot> {
    let mut order_set =
        build_order_reports_from_orders(orders, instruments, ctx.account_id, None, ts_init);
    let mut fill_set = build_fill_reports_from_trades(trades, ctx, instruments, None, ts_init);
    let position_set = build_position_reports(positions, ctx.account_id, ts_init);

    fill_tracker.snap_fill_reports(&mut fill_set.reports);

    if let Some(mins) = lookback_mins {
        let lookback_ns = mins.saturating_mul(60).saturating_mul(1_000_000_000);
        let cutoff = UnixNanos::from(ts_init.as_u64().saturating_sub(lookback_ns));
        let order_count = order_set.reports.len();
        let fill_count = fill_set.reports.len();
        order_set.reports.retain(|report| report.ts_last >= cutoff);
        fill_set.reports.retain(|report| report.ts_event >= cutoff);
        order_set.omissions.record_n(
            ReconciliationOmission::LookbackOrder,
            order_count - order_set.reports.len(),
        );
        fill_set.omissions.record_n(
            ReconciliationOmission::LookbackFill,
            fill_count - fill_set.reports.len(),
        );
    }

    let invalid_orders =
        cap_order_reports_to_confirmed_fills(&mut order_set.reports, &fill_set.reports);
    order_set.omissions.record_n(
        ReconciliationOmission::InvalidOrder(ReportParseError::FilledQuantity),
        invalid_orders,
    );

    let mut omissions = order_set.omissions;
    omissions.merge(fill_set.omissions);
    omissions.merge(position_set.omissions);
    log_reconciliation_summary(
        "mass status",
        order_set.reports.len(),
        fill_set.reports.len(),
        position_set.reports.len(),
        &omissions,
    );
    omissions.ensure_authoritative()?;

    Ok(ReconciliationSnapshot {
        orders: order_set.reports,
        fills: fill_set.reports,
        positions: position_set.reports,
    })
}

pub(crate) fn log_reconciliation_summary(
    route: &str,
    order_reports: usize,
    fill_reports: usize,
    position_reports: usize,
    omissions: &ReconciliationOmissions,
) {
    let message = format!(
        "Polymarket {route}: reports(order={order_reports}, fill={fill_reports}, position={position_reports}); omissions={omissions}"
    );

    if omissions.is_empty() {
        log::debug!("{message}");
    } else {
        log::warn!("{message}");
    }
}

fn cap_order_reports_to_confirmed_fills(
    order_reports: &mut Vec<OrderStatusReport>,
    fill_reports: &[FillReport],
) -> usize {
    let confirmed_by_order = confirmed_filled_quantities(fill_reports);
    let before = order_reports.len();

    order_reports.retain_mut(|report| {
        let local_filled = Quantity::zero(report.quantity.precision);
        cap_order_report_filled_qty(
            report,
            local_filled,
            confirmed_by_order.get(&report.venue_order_id).copied(),
        )
        .is_ok()
    });

    before - order_reports.len()
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
) -> Result<(), ReportParseError> {
    let confirmed_filled = confirmed_filled
        .and_then(|qty| Quantity::from_decimal_dp(qty, report.quantity.precision).ok())
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    let capped = report.filled_qty.min(local_filled.max(confirmed_filled));
    report.filled_qty = capped;
    if report.order_status == OrderStatus::Filled && report.filled_qty.is_zero() {
        return Err(ReportParseError::FilledQuantity);
    }

    normalize_terminal_order_report_quantity(report);
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
        types::{Money, Price},
    };
    use rstest::rstest;

    use super::*;
    use crate::{
        common::enums::PolymarketTradeStatus,
        http::{
            models::GammaMarket,
            parse::{create_instrument_from_def, parse_gamma_market},
        },
    };

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("failed to read test data");
        serde_json::from_str(&content).expect("failed to parse test data")
    }

    fn test_instrument() -> InstrumentAny {
        let market: GammaMarket = load("gamma_market.json");
        let defs = parse_gamma_market(&market).expect("market should parse");
        create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64))
            .expect("instrument should parse")
    }

    fn test_fill_context() -> FillContext<'static> {
        FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        }
    }

    #[rstest]
    fn reconciliation_snapshot_rejects_pending_settlement() {
        let instrument = test_instrument();
        let mut trade: PolymarketTradeReport = load("http_trade_report.json");
        trade.status = PolymarketTradeStatus::Matched;
        let position = DataApiPosition {
            asset: trade.asset_id.to_string(),
            condition_id: "0xabc".to_string(),
            size: Decimal::ZERO,
            avg_price: None,
        };

        let instruments = AtomicMap::new();
        instruments.insert(trade.asset_id, instrument);
        let error = build_reconciliation_snapshot(
            &[],
            &[trade],
            &[position],
            &instruments,
            &OrderFillTrackerMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("pending settlement must make the snapshot non-authoritative");

        assert_eq!(
            error.to_string(),
            "Mass status is not authoritative: PendingTrade=1",
        );
    }

    #[rstest]
    fn reconciliation_snapshot_omits_zero_position_evidence() {
        let position = DataApiPosition {
            asset: "123".to_string(),
            condition_id: "0xabc".to_string(),
            size: Decimal::ZERO,
            avg_price: Some(Decimal::new(5, 1)),
        };
        let snapshot = build_reconciliation_snapshot(
            &[],
            &[],
            &[position],
            &AtomicMap::new(),
            &OrderFillTrackerMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("zero rows should not be treated as position-close evidence");

        assert!(snapshot.positions.is_empty());
    }

    #[rstest]
    fn reconciliation_snapshot_rejects_invalid_open_position() {
        let position = DataApiPosition {
            asset: "123".to_string(),
            condition_id: "0xabc".to_string(),
            size: Decimal::TEN,
            avg_price: None,
        };
        let error = build_reconciliation_snapshot(
            &[],
            &[],
            &[position],
            &AtomicMap::new(),
            &OrderFillTrackerMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("invalid open position must make the snapshot non-authoritative");

        assert_eq!(
            error.to_string(),
            "Mass status is not authoritative: Position(InvalidAveragePrice)=1",
        );
    }

    #[rstest]
    fn reconciliation_snapshot_rejects_filled_order_without_confirmed_fill() {
        let instrument = test_instrument();
        let mut order: PolymarketOpenOrder = load("http_open_order.json");
        order.status = crate::common::enums::PolymarketOrderStatus::Matched;
        order.size_matched = order.original_size;

        let instruments = AtomicMap::new();
        instruments.insert(order.asset_id, instrument);
        let error = build_reconciliation_snapshot(
            &[order],
            &[],
            &[],
            &instruments,
            &OrderFillTrackerMap::new(),
            &test_fill_context(),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("filled order without confirmed fill must not emit malformed state");

        assert_eq!(
            error.to_string(),
            "Mass status is not authoritative: InvalidOrder(FilledQuantity)=1",
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
