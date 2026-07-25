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
        instrument_taker_fee, try_build_maker_fill_report, try_parse_fill_report,
        try_parse_order_status_report, try_parse_timestamp,
    },
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
    pub pusd: Currency,
    pub clock: &'static AtomicTime,
}

#[derive(Clone, Debug)]
pub(crate) enum FillReconciliationScope {
    All,
    Instrument(InstrumentId),
    VenueOrder(VenueOrderId),
    Order {
        instrument_id: InstrumentId,
        venue_order_id: VenueOrderId,
    },
    Orders {
        instrument_id: Option<InstrumentId>,
        venue_order_ids: AHashSet<VenueOrderId>,
    },
}

pub(crate) fn resolve_requested_instrument(
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    instrument_id: InstrumentId,
) -> anyhow::Result<(Ustr, InstrumentAny)> {
    instruments
        .load()
        .iter()
        .find(|(_, instrument)| instrument.id() == instrument_id)
        .map(|(token_id, instrument)| (*token_id, instrument.clone()))
        .with_context(|| {
            format!(
                "Polymarket reconciliation cannot resolve requested NT instrument {instrument_id}"
            )
        })
}

/// Converts trade reports into fill reports: single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    scope: &FillReconciliationScope,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<FillReport>> {
    let mut reports = Vec::new();
    let instrument_filter = match scope {
        FillReconciliationScope::All | FillReconciliationScope::VenueOrder(_) => None,
        FillReconciliationScope::Instrument(instrument_id)
        | FillReconciliationScope::Order { instrument_id, .. } => Some(*instrument_id),
        FillReconciliationScope::Orders { instrument_id, .. } => *instrument_id,
    };
    let filter_token = instrument_filter
        .map(|instrument_id| resolve_requested_instrument(instruments, instrument_id))
        .transpose()?
        .map(|(token_id, _)| token_id);
    let target_order_ids = match scope {
        FillReconciliationScope::All | FillReconciliationScope::Instrument(_) => None,
        FillReconciliationScope::VenueOrder(venue_order_id) => {
            Some(AHashSet::from_iter([*venue_order_id]))
        }
        FillReconciliationScope::Order { venue_order_id, .. } => {
            Some(AHashSet::from_iter([*venue_order_id]))
        }
        FillReconciliationScope::Orders {
            venue_order_ids, ..
        } => Some(venue_order_ids.clone()),
    };

    for trade in trades {
        if trade.status != PolymarketTradeStatus::Confirmed {
            continue;
        }

        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            let mut relevant_maker_order_seen = matches!(scope, FillReconciliationScope::All);
            let mut owned_maker_order_seen = false;
            for mo in &trade.maker_orders {
                let venue_order_id = VenueOrderId::from(mo.order_id.as_str());

                if target_order_ids
                    .as_ref()
                    .is_some_and(|target_ids| !target_ids.contains(&venue_order_id))
                {
                    continue;
                }
                if target_order_ids.is_some() {
                    relevant_maker_order_seen = true;
                }

                if filter_token.is_some_and(|token_id| mo.asset_id != token_id) {
                    if target_order_ids.is_some() {
                        anyhow::bail!(
                            "Polymarket reconciliation target maker fill {} asset {} \
                             does not match requested NT instrument",
                            trade.id,
                            mo.asset_id
                        );
                    }
                    continue;
                }
                if filter_token.is_some() {
                    relevant_maker_order_seen = true;
                }
                if mo.maker_address != ctx.user_address && mo.owner != ctx.api_key {
                    continue;
                }
                owned_maker_order_seen = true;
                let token_id = Ustr::from(mo.asset_id.as_str());
                let instrument = instruments.get_cloned(&token_id);
                let (instrument_id, price_prec, size_prec) = match instrument {
                    Some(i) => (i.id(), i.price_precision(), i.size_precision()),
                    None => {
                        anyhow::bail!(
                            "Polymarket reconciliation cannot map confirmed maker fill {} \
                             asset {} to an NT instrument",
                            trade.id,
                            mo.asset_id
                        );
                    }
                };

                let ts_event = try_parse_timestamp(&trade.match_time).map_err(|e| {
                    anyhow::anyhow!(
                        "confirmed maker fill {} has invalid match_time: {e}",
                        trade.id
                    )
                })?;
                let report = try_build_maker_fill_report(
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
                )?;
                reports.push(report);
            }
            if relevant_maker_order_seen && !owned_maker_order_seen {
                anyhow::bail!(
                    "Polymarket reconciliation confirmed maker trade {} has no owned maker order",
                    trade.id
                );
            }
        } else {
            let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

            if target_order_ids
                .as_ref()
                .is_some_and(|target_ids| !target_ids.contains(&venue_order_id))
            {
                continue;
            }

            if filter_token.is_some_and(|token_id| trade.asset_id != token_id) {
                if target_order_ids.is_some() {
                    anyhow::bail!(
                        "Polymarket reconciliation target taker fill {} asset {} \
                         does not match requested NT instrument",
                        trade.id,
                        trade.asset_id
                    );
                }
                continue;
            }
            let token_id = Ustr::from(trade.asset_id.as_str());
            let instrument = instruments.get_cloned(&token_id);
            let (instrument_id, price_prec, size_prec, taker_fee_rate) = match instrument {
                Some(i) => (
                    i.id(),
                    i.price_precision(),
                    i.size_precision(),
                    instrument_taker_fee(&i),
                ),
                None => {
                    anyhow::bail!(
                        "Polymarket reconciliation cannot map confirmed taker fill {} \
                         asset {} to an NT instrument",
                        trade.id,
                        trade.asset_id
                    );
                }
            };

            let report = try_parse_fill_report(
                trade,
                instrument_id,
                ctx.account_id,
                None,
                price_prec,
                size_prec,
                ctx.pusd,
                taker_fee_rate,
                ts_init,
            )?;
            reports.push(report);
        }
    }

    Ok(reports)
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> anyhow::Result<Vec<OrderStatusReport>> {
    let mut reports = Vec::new();
    let filter_token = instrument_filter
        .map(|instrument_id| resolve_requested_instrument(instruments, instrument_id))
        .transpose()?
        .map(|(token_id, _)| token_id);

    for order in orders {
        if filter_token.is_some_and(|token_id| order.asset_id != token_id) {
            continue;
        }
        let token_id = Ustr::from(order.asset_id.as_str());
        let instrument = instruments.get_cloned(&token_id);
        let (instrument_id, price_prec, size_prec) = match instrument {
            Some(i) => (i.id(), i.price_precision(), i.size_precision()),
            None => {
                anyhow::bail!(
                    "Polymarket reconciliation cannot map venue open order asset {} \
                     to an NT instrument",
                    order.asset_id
                );
            }
        };

        let report = try_parse_order_status_report(
            order,
            instrument_id,
            account_id,
            None,
            price_prec,
            size_prec,
            ts_init,
        )?;
        reports.push(report);
    }

    Ok(reports)
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
    instruments: &AtomicMap<Ustr, InstrumentAny>,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts: UnixNanos,
) -> anyhow::Result<Vec<PositionStatusReport>> {
    let mut reports = Vec::new();
    let filter_token = instrument_filter
        .map(|instrument_id| resolve_requested_instrument(instruments, instrument_id))
        .transpose()?
        .map(|(token_id, _)| token_id);

    for position in positions {
        if filter_token.is_some_and(|token_id| position.asset != token_id.as_str()) {
            continue;
        }

        if position.size.is_sign_negative() {
            anyhow::bail!(
                "Polymarket reconciliation received negative Data API position \
                 {}-{} size {}",
                position.condition_id,
                position.asset,
                position.size
            );
        }

        if position.size > Decimal::ZERO && position.size < DUST_POSITION_THRESHOLD {
            log::debug!(
                "Filtering dust position: {}-{}, size={}",
                position.condition_id,
                position.asset,
                position.size
            );
        }

        if position.size < DUST_POSITION_THRESHOLD {
            continue;
        }

        let instrument = instruments
            .get_cloned(&Ustr::from(position.asset.as_str()))
            .with_context(|| {
                format!(
                    "Polymarket reconciliation cannot map Data API position \
                     {}-{} to an NT instrument",
                    position.condition_id, position.asset
                )
            })?;
        let quantity = position_quantity(
            position.size,
            instrument.size_precision(),
            position.condition_id.as_str(),
            position.asset.as_str(),
        )?;
        reports.push(PositionStatusReport::new(
            account_id,
            instrument.id(),
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

fn position_quantity(
    size: Decimal,
    precision: u8,
    condition_id: &str,
    asset: &str,
) -> anyhow::Result<Quantity> {
    Quantity::from_decimal_dp(size, precision).with_context(|| {
        format!(
            "Polymarket reconciliation cannot represent Data API position \
             {condition_id}-{asset} size {size}"
        )
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

    // Fetch orders
    let orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;

    let mut order_reports =
        build_order_reports_from_orders(&orders, instruments, ctx.account_id, None, ts_init)?;

    // Fetch and parse fill reports
    let trades = http_client
        .get_trades(GetTradesParams::default())
        .await
        .context("failed to fetch trades for mass status")?;

    let mut fill_reports = build_fill_reports_from_trades(
        &trades,
        ctx,
        instruments,
        &FillReconciliationScope::All,
        ts_init,
    )?;

    // Snap dust drift on REST fills the same way the WS path does.
    // Commission stays as venue-reported.
    fill_tracker.snap_fill_reports(&mut fill_reports);

    // Position reports from Data API
    let positions = data_api_client
        .get_positions(ctx.user_address)
        .await
        .context("failed to fetch positions for mass status")?;

    let position_reports =
        build_position_reports(&positions, instruments, ctx.account_id, None, ts_init)?;

    // Apply lookback filter
    if let Some(mins) = lookback_mins {
        let now_ns = ctx.clock.get_time_ns();
        let lookback_ns = mins
            .checked_mul(60)
            .and_then(|seconds| seconds.checked_mul(1_000_000_000))
            .with_context(|| format!("Polymarket reconciliation lookback {mins}min overflows"))?;
        let cutoff_ns = now_ns.as_u64().saturating_sub(lookback_ns);
        let cutoff = UnixNanos::from(cutoff_ns);

        let orders_before = order_reports.len();
        order_reports.retain(|r| r.ts_last >= cutoff);
        let orders_removed = orders_before - order_reports.len();

        let fills_before = fill_reports.len();
        fill_reports.retain(|r| r.ts_event >= cutoff);
        let fills_removed = fills_before - fill_reports.len();

        log::debug!(
            "Lookback filter ({}min): orders {}->{} (removed {}), fills {}->{} (removed {})",
            mins,
            orders_before,
            order_reports.len(),
            orders_removed,
            fills_before,
            fill_reports.len(),
            fills_removed,
        );
    } else {
        log::debug!(
            "Generated mass status: {} orders, {} fills, {} positions",
            order_reports.len(),
            fill_reports.len(),
            position_reports.len(),
        );
    }

    cap_order_reports_to_confirmed_fills(&mut order_reports, &fill_reports)?;

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
}

fn cap_order_reports_to_confirmed_fills(
    order_reports: &mut [OrderStatusReport],
    fill_reports: &[FillReport],
) -> anyhow::Result<()> {
    let confirmed_by_order = confirmed_filled_quantities(fill_reports)?;

    for report in order_reports {
        let local_filled = Quantity::zero(report.quantity.precision);
        try_cap_order_report_filled_qty(
            report,
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
        *total = total
            .checked_add(fill.last_qty.as_decimal())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "confirmed fill quantity overflow for venue order {}",
                    fill.venue_order_id
                )
            })?;
    }

    Ok(confirmed_by_order)
}

pub(crate) fn cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    confirmed_filled: Option<Decimal>,
) {
    let confirmed_filled = confirmed_filled
        .and_then(|qty| Quantity::from_decimal_dp(qty, report.quantity.precision).ok())
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    cap_order_report_with_quantities(report, local_filled, confirmed_filled);
}

pub(crate) fn try_cap_order_report_filled_qty(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    confirmed_filled: Option<Decimal>,
) -> anyhow::Result<()> {
    let confirmed_filled = confirmed_filled
        .map(|qty| {
            Quantity::from_decimal_dp(qty, report.quantity.precision).map_err(|e| {
                anyhow::anyhow!(
                    "cannot represent confirmed fill quantity for venue order {}: {e}",
                    report.venue_order_id
                )
            })
        })
        .transpose()?
        .unwrap_or_else(|| Quantity::zero(report.quantity.precision));
    cap_order_report_with_quantities(report, local_filled, confirmed_filled);
    Ok(())
}

fn cap_order_report_with_quantities(
    report: &mut OrderStatusReport,
    local_filled: Quantity,
    confirmed_filled: Quantity,
) {
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
        instruments::{Instrument, InstrumentAny, stubs::binary_option},
        types::{Money, Price},
    };
    use rstest::rstest;

    use super::*;

    fn mapped_instrument() -> (AtomicMap<Ustr, InstrumentAny>, InstrumentAny) {
        let instrument = InstrumentAny::BinaryOption(binary_option());
        let instruments = AtomicMap::new();
        instruments.insert(
            Ustr::from(instrument.raw_symbol().as_str()),
            instrument.clone(),
        );
        (instruments, instrument)
    }

    fn open_order_for(instrument: &InstrumentAny) -> PolymarketOpenOrder {
        let content = std::fs::read_to_string("test_data/http_open_order.json")
            .expect("open-order fixture should load");
        let mut order: PolymarketOpenOrder =
            serde_json::from_str(&content).expect("open-order fixture should decode");
        order.asset_id = Ustr::from(instrument.raw_symbol().as_str());
        order
    }

    fn confirmed_trade_for(instrument: &InstrumentAny) -> PolymarketTradeReport {
        let content = std::fs::read_to_string("test_data/http_trade_report.json")
            .expect("trade fixture should load");
        let mut trade: PolymarketTradeReport =
            serde_json::from_str(&content).expect("trade fixture should decode");
        trade.status = PolymarketTradeStatus::Confirmed;
        trade.asset_id = Ustr::from(instrument.raw_symbol().as_str());
        for maker_order in &mut trade.maker_orders {
            maker_order.asset_id = Ustr::from(instrument.raw_symbol().as_str());
        }
        trade
    }

    fn fill_context() -> FillContext<'static> {
        FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        }
    }

    #[rstest]
    fn rejects_unmapped_open_order() {
        let content = std::fs::read_to_string("test_data/http_open_order.json")
            .expect("open-order fixture should load");
        let order: PolymarketOpenOrder =
            serde_json::from_str(&content).expect("open-order fixture should decode");
        let instruments = AtomicMap::new();

        let error = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("an unmapped venue open order must fail reconciliation");

        assert!(error.to_string().contains("venue open order asset"));
    }

    #[rstest]
    fn rejects_unmapped_confirmed_fill() {
        let content = std::fs::read_to_string("test_data/http_trade_report.json")
            .expect("trade fixture should load");
        let mut trade: PolymarketTradeReport =
            serde_json::from_str(&content).expect("trade fixture should decode");
        trade.status = PolymarketTradeStatus::Confirmed;
        let instruments = AtomicMap::new();
        let ctx = FillContext {
            account_id: AccountId::from("POLY-001"),
            user_address: "0x70997970c51812dc3a010c7d01b50e0d17dc79c8",
            api_key: "00000000-0000-0000-0000-000000000001",
            pusd: Currency::pUSD(),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
        };

        let error = build_fill_reports_from_trades(
            &[trade],
            &ctx,
            &instruments,
            &FillReconciliationScope::All,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("an unmapped confirmed fill must fail reconciliation");

        assert!(error.to_string().contains("confirmed taker fill"));
    }

    #[rstest]
    #[case::quantity(|order: &mut PolymarketOpenOrder| order.original_size = Decimal::MAX)]
    #[case::filled(|order: &mut PolymarketOpenOrder| order.size_matched = Decimal::MAX)]
    #[case::price(|order: &mut PolymarketOpenOrder| order.price = Decimal::MAX)]
    fn rejects_unrepresentable_open_order_numeric(#[case] mutate: fn(&mut PolymarketOpenOrder)) {
        let (instruments, instrument) = mapped_instrument();
        let mut order = open_order_for(&instrument);
        mutate(&mut order);

        let error = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("unrepresentable open-order numerics must fail reconciliation");

        assert!(
            error
                .to_string()
                .contains("cannot represent venue open order")
        );
    }

    #[rstest]
    fn rejects_overflowed_open_order_timestamp() {
        let (instruments, instrument) = mapped_instrument();
        let mut order = open_order_for(&instrument);
        order.created_at = u64::MAX;

        let error = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("overflowed open-order timestamps must fail reconciliation");

        assert!(error.to_string().contains("created_at"));
    }

    #[rstest]
    fn rejects_malformed_open_order_expiration() {
        let (instruments, instrument) = mapped_instrument();
        let mut order = open_order_for(&instrument);
        order.expiration = Some("not-a-timestamp".to_string());

        let error = build_order_reports_from_orders(
            &[order],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("malformed open-order expiration must fail reconciliation");

        assert!(error.to_string().contains("expiration"));
    }

    #[rstest]
    #[case::quantity(|trade: &mut PolymarketTradeReport| trade.size = Decimal::MAX)]
    #[case::price(|trade: &mut PolymarketTradeReport| trade.price = Decimal::MAX)]
    fn rejects_unrepresentable_confirmed_taker_fill_numeric(
        #[case] mutate: fn(&mut PolymarketTradeReport),
    ) {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        mutate(&mut trade);

        let error = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::All,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("unrepresentable confirmed-fill numerics must fail reconciliation");

        assert!(
            error
                .to_string()
                .contains("cannot represent confirmed taker fill")
        );
    }

    #[rstest]
    fn rejects_unrepresentable_confirmed_maker_fill_numeric() {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        trade.trader_side = PolymarketLiquiditySide::Maker;
        trade.maker_orders[0].matched_amount = Decimal::MAX;

        let error = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::All,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("unrepresentable maker-fill numerics must fail reconciliation");

        assert!(
            error
                .to_string()
                .contains("cannot represent confirmed maker fill")
        );
    }

    #[rstest]
    fn rejects_confirmed_maker_fill_without_owned_maker_order() {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        trade.trader_side = PolymarketLiquiditySide::Maker;
        for maker_order in &mut trade.maker_orders {
            maker_order.maker_address = "0x-unrelated-maker".to_string();
            maker_order.owner = "unrelated-owner".to_string();
        }

        let error = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::All,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("a confirmed maker trade must contain an owned maker order");

        assert!(error.to_string().contains("no owned maker order"));
    }

    #[rstest]
    fn targeted_maker_fill_rejects_target_order_ownership_mismatch() {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        trade.trader_side = PolymarketLiquiditySide::Maker;
        let target_order_id = VenueOrderId::from(trade.maker_orders[0].order_id.as_str());
        for maker_order in &mut trade.maker_orders {
            maker_order.maker_address = "0x-unrelated-maker".to_string();
            maker_order.owner = "unrelated-owner".to_string();
        }

        let error = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::VenueOrder(target_order_id),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("a targeted maker order with mismatched ownership must fail");

        assert!(error.to_string().contains("no owned maker order"));
    }

    #[rstest]
    fn targeted_maker_fill_ignores_unrelated_trade_without_owned_order() {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        trade.trader_side = PolymarketLiquiditySide::Maker;
        for maker_order in &mut trade.maker_orders {
            maker_order.maker_address = "0x-unrelated-maker".to_string();
            maker_order.owner = "unrelated-owner".to_string();
        }

        let reports = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::VenueOrder(VenueOrderId::from("unrelated-target")),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("a targeted query must ignore a trade that does not contain its order");

        assert!(reports.is_empty());
    }

    #[rstest]
    #[case::malformed("not-a-timestamp")]
    #[case::overflow("18446744073709551615")]
    fn rejects_invalid_confirmed_fill_timestamp(#[case] match_time: &str) {
        let (instruments, instrument) = mapped_instrument();
        let mut trade = confirmed_trade_for(&instrument);
        trade.match_time = match_time.to_string();

        let error = build_fill_reports_from_trades(
            &[trade],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::All,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("invalid confirmed-fill timestamps must fail reconciliation");

        assert!(error.to_string().contains("match_time"));
    }

    #[rstest]
    fn scoped_order_reconciliation_ignores_unrelated_unmapped_order() {
        let (instruments, instrument) = mapped_instrument();
        let mapped = open_order_for(&instrument);
        let mut unrelated = mapped.clone();
        unrelated.asset_id = Ustr::from("UNRELATED-UNMAPPED-TOKEN");

        let reports = build_order_reports_from_orders(
            &[unrelated, mapped],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            Some(instrument.id()),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("scoped reconciliation must ignore unrelated venue orders");

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].instrument_id, instrument.id());
    }

    #[rstest]
    fn scoped_fill_reconciliation_ignores_unrelated_unmapped_fill() {
        let (instruments, instrument) = mapped_instrument();
        let mapped = confirmed_trade_for(&instrument);
        let mut unrelated = mapped.clone();
        unrelated.asset_id = Ustr::from("UNRELATED-UNMAPPED-TOKEN");

        let reports = build_fill_reports_from_trades(
            &[unrelated, mapped],
            &fill_context(),
            &instruments,
            &FillReconciliationScope::Instrument(instrument.id()),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("scoped reconciliation must ignore unrelated venue fills");

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].instrument_id, instrument.id());
    }

    #[rstest]
    fn rejects_negative_current_position_before_dust_filtering() {
        let position = DataApiPosition {
            asset: "token-1".to_string(),
            condition_id: "condition-1".to_string(),
            size: Decimal::NEGATIVE_ONE,
            avg_price: None,
        };

        let error = build_position_reports(
            &[position],
            &AtomicMap::new(),
            AccountId::from("POLYMARKET-001"),
            None,
            UnixNanos::from(1_000_000_000u64),
        )
        .expect_err("negative current positions must fail reconciliation");

        assert!(error.to_string().contains("negative Data API position"));
    }

    #[rstest]
    fn scoped_position_reconciliation_ignores_unrelated_invalid_position() {
        let (instruments, instrument) = mapped_instrument();
        let unrelated = DataApiPosition {
            asset: "UNRELATED-UNMAPPED-TOKEN".to_string(),
            condition_id: "condition-unrelated".to_string(),
            size: Decimal::NEGATIVE_ONE,
            avg_price: None,
        };
        let mapped = DataApiPosition {
            asset: instrument.raw_symbol().to_string(),
            condition_id: "condition-mapped".to_string(),
            size: Decimal::ONE,
            avg_price: None,
        };

        let reports = build_position_reports(
            &[unrelated, mapped],
            &instruments,
            AccountId::from("POLYMARKET-001"),
            Some(instrument.id()),
            UnixNanos::from(1_000_000_000u64),
        )
        .expect("scoped reconciliation must ignore unrelated positions");

        assert_eq!(reports.len(), 1);
        assert_eq!(reports[0].instrument_id, instrument.id());
    }

    #[rstest]
    fn rejects_unrepresentable_current_position() {
        let error = position_quantity(Decimal::MAX, 6, "condition-1", "token-1")
            .expect_err("an unrepresentable current position must fail reconciliation");

        assert!(
            error
                .to_string()
                .contains("cannot represent Data API position")
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

        cap_order_reports_to_confirmed_fills(&mut reports, &fills).unwrap();

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

        cap_order_reports_to_confirmed_fills(&mut reports, &fills).unwrap();

        assert_eq!(reports[0].quantity, Quantity::from(expected_quantity));
        assert_eq!(reports[0].filled_qty, Quantity::from(confirmed));
    }
}
