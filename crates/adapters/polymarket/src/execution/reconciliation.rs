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

use anyhow::Context;
use nautilus_core::{UnixNanos, time::AtomicTime};
use nautilus_model::{
    enums::LiquiditySide,
    identifiers::{AccountId, ClientId, InstrumentId, Venue, VenueOrderId},
    instruments::Instrument,
    reports::{ExecutionMassStatus, FillReport, OrderStatusReport, PositionStatusReport},
    types::Currency,
};
use ustr::Ustr;

use super::parse::{
    build_maker_fill_report, parse_fill_report, parse_order_status_report, parse_timestamp,
};
use crate::{
    common::enums::PolymarketLiquiditySide,
    http::{
        clob::PolymarketClobHttpClient,
        models::{PolymarketOpenOrder, PolymarketTradeReport},
        query::{GetOrdersParams, GetTradesParams},
    },
    providers::PolymarketInstrumentProvider,
};

/// Shared context for trade-to-fill-report conversion.
pub(crate) struct FillContext<'a> {
    pub account_id: AccountId,
    pub user_address: &'a str,
    pub api_key: &'a str,
    pub usdc: Currency,
    pub clock: &'static AtomicTime,
}

/// Converts trade reports into fill reports — single implementation of maker/taker
/// parsing used by both `generate_fill_reports()` and `generate_mass_status()`.
pub(crate) fn build_fill_reports_from_trades(
    trades: &[PolymarketTradeReport],
    ctx: &FillContext<'_>,
    provider: &PolymarketInstrumentProvider,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> (Vec<FillReport>, usize) {
    let mut reports = Vec::new();
    let mut filtered = 0usize;

    for trade in trades {
        let is_maker = trade.trader_side == PolymarketLiquiditySide::Maker;

        if is_maker {
            for mo in &trade.maker_orders {
                if mo.maker_address != ctx.user_address && mo.owner != ctx.api_key {
                    continue;
                }
                let token_id = Ustr::from(mo.asset_id.as_str());
                let instrument = provider.get_by_token_id(&token_id);
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
                    ctx.usdc,
                    LiquiditySide::Maker,
                    ts_event,
                    ts_init,
                );
                reports.push(report);
            }
        } else {
            let token_id = Ustr::from(trade.asset_id.as_str());
            let instrument = provider.get_by_token_id(&token_id);
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

            let report = parse_fill_report(
                trade,
                instrument_id,
                ctx.account_id,
                None,
                price_prec,
                size_prec,
                ctx.usdc,
                ts_init,
            );
            reports.push(report);
        }
    }

    (reports, filtered)
}

/// Converts open orders into order status reports.
pub(crate) fn build_order_reports_from_orders(
    orders: &[PolymarketOpenOrder],
    provider: &PolymarketInstrumentProvider,
    account_id: AccountId,
    instrument_filter: Option<InstrumentId>,
    ts_init: UnixNanos,
) -> (Vec<OrderStatusReport>, usize) {
    let mut reports = Vec::new();
    let mut filtered = 0usize;

    for order in orders {
        let token_id = Ustr::from(order.asset_id.as_str());
        let instrument = provider.get_by_token_id(&token_id);
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

        let report = parse_order_status_report(
            order,
            instrument_id,
            account_id,
            None,
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

/// Full reconciliation mass status generation.
pub(crate) async fn generate_mass_status(
    http_client: &PolymarketClobHttpClient,
    provider: &PolymarketInstrumentProvider,
    ctx: &FillContext<'_>,
    client_id: ClientId,
    venue: Venue,
    lookback_mins: Option<u64>,
) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let ts_init = UnixNanos::default();

    // Fetch orders
    let orders = http_client
        .get_orders(GetOrdersParams::default())
        .await
        .context("failed to fetch orders for mass status")?;

    let (mut order_reports, orders_filtered) =
        build_order_reports_from_orders(&orders, provider, ctx.account_id, None, ts_init);

    // Fetch and parse fill reports
    let trades = http_client
        .get_trades(GetTradesParams::default())
        .await
        .context("failed to fetch trades for mass status")?;

    let (mut fill_reports, fills_filtered) =
        build_fill_reports_from_trades(&trades, ctx, provider, None, ts_init);

    // Position reports: empty for cash/prediction markets
    let position_reports: Vec<PositionStatusReport> = vec![];

    // Apply lookback filter
    if let Some(mins) = lookback_mins {
        let now_ns = ctx.clock.get_time_ns();
        let cutoff_ns = now_ns.as_u64().saturating_sub(mins * 60 * 1_000_000_000);
        let cutoff = UnixNanos::from(cutoff_ns);

        let orders_before = order_reports.len();
        order_reports.retain(|r| r.ts_last >= cutoff);
        let orders_removed = orders_before - order_reports.len();

        let fills_before = fill_reports.len();
        fill_reports.retain(|r| r.ts_event >= cutoff);
        let fills_removed = fills_before - fill_reports.len();

        log::info!(
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
            "Generated mass status: {} orders ({} filtered), {} fills ({} filtered)",
            order_reports.len(),
            orders_filtered,
            fill_reports.len(),
            fills_filtered,
        );
    }

    let mut mass_status = ExecutionMassStatus::new(client_id, ctx.account_id, venue, ts_init, None);

    mass_status.add_order_reports(order_reports);
    mass_status.add_position_reports(position_reports);
    mass_status.add_fill_reports(fill_reports);

    Ok(Some(mass_status))
}
