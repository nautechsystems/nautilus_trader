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

use std::sync::Mutex;

use ahash::AHashMap;
use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
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
        order_fill_tracker::OrderFillTrackerMap,
        parse::{
            build_maker_fill_report, compute_commission, determine_order_side,
            make_composite_trade_id, parse_liquidity_side,
        },
    },
};

/// Signal returned when a finalized trade requires an async account refresh.
#[derive(Debug)]
pub(crate) struct AccountRefreshRequest;

/// Mutable state owned by the WS message loop (not shared via Arc).
#[derive(Debug, Default)]
pub(crate) struct WsDispatchState {
    pub processed_fills: FifoCache<String, 10_000>,
}

/// Immutable context borrowed from the async block's owned values.
#[derive(Debug)]
pub(crate) struct WsDispatchContext<'a> {
    pub token_instruments: &'a AHashMap<Ustr, InstrumentAny>,
    pub fill_tracker: &'a OrderFillTrackerMap,
    pub pending_fills: &'a Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>,
    pub pending_order_reports: &'a Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>,
    pub emitter: &'a ExecutionEventEmitter,
    pub account_id: AccountId,
    pub clock: &'static AtomicTime,
    pub user_address: &'a str,
    pub user_api_key: &'a str,
}

/// Top-level router — synchronous, returns signal for async account refresh.
pub(crate) fn dispatch_user_message(
    message: &UserWsMessage,
    ctx: &WsDispatchContext<'_>,
    state: &mut WsDispatchState,
) -> Option<AccountRefreshRequest> {
    match message {
        UserWsMessage::Order(order) => {
            dispatch_order_update(order, ctx);
            None
        }
        UserWsMessage::Trade(trade) => dispatch_trade_update(trade, ctx, state),
    }
}

/// Dispatches an order status update, emitting or buffering the report.
fn dispatch_order_update(order: &PolymarketUserOrder, ctx: &WsDispatchContext<'_>) {
    let instrument = match ctx.token_instruments.get(&order.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in order update: {}", order.asset_id);
            return;
        }
    };

    let ts_event = parse_timestamp_ms(&order.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let venue_order_id = VenueOrderId::from(order.id.as_str());

    let report = build_ws_order_status_report(order, instrument, ctx.account_id, ts_event);
    let is_accepted = ctx.fill_tracker.contains(&venue_order_id);

    emit_or_buffer_order_report(
        report,
        venue_order_id,
        is_accepted,
        ctx.emitter,
        ctx.pending_order_reports,
    );

    // MATCHED convergence: check for dust residual
    if order.status == PolymarketOrderStatus::Matched {
        let price = Price::new(
            order.price.parse::<f64>().unwrap_or(0.0),
            instrument.price_precision(),
        );

        if let Some(dust_fill) = ctx.fill_tracker.check_dust_and_build_fill(
            &venue_order_id,
            ctx.account_id,
            &order.id,
            price.as_f64(),
            crate::execution::get_usdc_currency(),
            ts_event,
        ) {
            emit_or_buffer_fill_report(
                dust_fill,
                venue_order_id,
                is_accepted,
                ctx.emitter,
                ctx.pending_fills,
            );
        }
    }
}

/// Dispatches a trade update with dedup, returning a refresh signal if finalized.
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

    if is_maker {
        dispatch_maker_fills(trade, ctx, liquidity_side, ts_event);
    } else {
        dispatch_taker_fill(trade, ctx, liquidity_side, ts_event);
    }

    if needs_refresh {
        Some(AccountRefreshRequest)
    } else {
        None
    }
}

/// Processes maker-side fills from a trade, one per matched maker order.
fn dispatch_maker_fills(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
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

    for mo in user_orders {
        let asset_id = Ustr::from(mo.asset_id.as_str());
        let instrument = match ctx.token_instruments.get(&asset_id) {
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
            crate::execution::get_usdc_currency(),
            liquidity_side,
            ts_event,
            ts_event,
        );
        let maker_venue_order_id = report.venue_order_id;
        report.last_qty = ctx
            .fill_tracker
            .snap_fill_qty(&maker_venue_order_id, report.last_qty);
        let is_accepted = ctx.fill_tracker.contains(&maker_venue_order_id);

        if is_accepted {
            ctx.fill_tracker.record_fill(
                &maker_venue_order_id,
                report.last_qty.as_f64(),
                report.last_px.as_f64(),
                report.ts_event,
            );
        }

        emit_or_buffer_fill_report(
            report,
            maker_venue_order_id,
            is_accepted,
            ctx.emitter,
            ctx.pending_fills,
        );
    }
}

/// Processes a taker-side fill from a trade.
fn dispatch_taker_fill(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
) {
    let instrument = match ctx.token_instruments.get(&trade.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in trade: {}", trade.asset_id);
            return;
        }
    };

    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

    let mut report =
        build_ws_taker_fill_report(trade, instrument, ctx.account_id, liquidity_side, ts_event);
    report.last_qty = ctx
        .fill_tracker
        .snap_fill_qty(&venue_order_id, report.last_qty);

    let is_accepted = ctx.fill_tracker.contains(&venue_order_id);

    if is_accepted {
        ctx.fill_tracker.record_fill(
            &venue_order_id,
            report.last_qty.as_f64(),
            report.last_px.as_f64(),
            report.ts_event,
        );
    }

    emit_or_buffer_fill_report(
        report,
        venue_order_id,
        is_accepted,
        ctx.emitter,
        ctx.pending_fills,
    );
}

/// Builds an [`OrderStatusReport`] from a WS order update.
fn build_ws_order_status_report(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
) -> OrderStatusReport {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_status = OrderStatus::from(order.status);
    let order_side = OrderSide::from(order.side);
    let time_in_force = TimeInForce::from(order.order_type);
    let quantity = Quantity::new(
        order.original_size.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let filled_qty = Quantity::new(
        order.size_matched.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let price = Price::new(
        order.price.parse::<f64>().unwrap_or(0.0),
        instrument.price_precision(),
    );

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
        ts_event,
        None,
    );
    report.price = Some(price);
    report
}

/// Builds a [`FillReport`] from a WS taker trade update.
fn build_ws_taker_fill_report(
    trade: &PolymarketUserTrade,
    instrument: &InstrumentAny,
    account_id: AccountId,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
) -> FillReport {
    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
    let trade_id = make_composite_trade_id(&trade.id, &trade.taker_order_id);
    let order_side = determine_order_side(
        trade.trader_side,
        trade.side,
        trade.asset_id.as_str(),
        trade.asset_id.as_str(),
    );

    let last_qty = Quantity::new(
        trade.size.parse::<f64>().unwrap_or(0.0),
        instrument.size_precision(),
    );
    let last_px = Price::new(
        trade.price.parse::<f64>().unwrap_or(0.0),
        instrument.price_precision(),
    );

    let fee_bps: Decimal = trade.fee_rate_bps.parse().unwrap_or_default();
    let size: Decimal = trade.size.parse().unwrap_or_default();
    let price_dec: Decimal = trade.price.parse().unwrap_or_default();
    let commission_value = compute_commission(fee_bps, size, price_dec);
    let usdc = crate::execution::get_usdc_currency();

    FillReport {
        account_id,
        instrument_id: instrument.id(),
        venue_order_id,
        trade_id,
        order_side,
        last_qty,
        last_px,
        commission: Money::new(commission_value, usdc),
        liquidity_side,
        report_id: UUID4::new(),
        ts_event,
        ts_init: ts_event,
        client_order_id: None,
        venue_position_id: None,
    }
}

// ---------------------------------------------------------------------------
// Emission helpers (bifurcated: emit immediately or buffer for pending accept)
// ---------------------------------------------------------------------------

/// Emits an order report immediately if accepted, otherwise buffers it.
fn emit_or_buffer_order_report(
    report: OrderStatusReport,
    venue_order_id: VenueOrderId,
    is_accepted: bool,
    emitter: &ExecutionEventEmitter,
    pending: &Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>,
) {
    if is_accepted {
        emitter.send_order_status_report(report);
    } else {
        let mut guard = pending.lock().expect(MUTEX_POISONED);
        if let Some(reports) = guard.get_mut(&venue_order_id) {
            reports.push(report);
        } else {
            guard.insert(venue_order_id, vec![report]);
        }
    }
}

/// Emits a fill report immediately if accepted, otherwise buffers it.
fn emit_or_buffer_fill_report(
    report: FillReport,
    venue_order_id: VenueOrderId,
    is_accepted: bool,
    emitter: &ExecutionEventEmitter,
    pending: &Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>,
) {
    if is_accepted {
        emitter.send_fill_report(report);
    } else {
        let mut guard = pending.lock().expect(MUTEX_POISONED);
        if let Some(fills) = guard.get_mut(&venue_order_id) {
            fills.push(report);
        } else {
            guard.insert(venue_order_id, vec![report]);
        }
    }
}

#[cfg(test)]
mod tests {
    use nautilus_model::{
        enums::{AccountType, CurrencyType},
        identifiers::TraderId,
        types::Currency,
    };
    use rstest::rstest;

    use super::*;

    fn load<T: serde::de::DeserializeOwned>(filename: &str) -> T {
        let path = format!("test_data/{filename}");
        let content = std::fs::read_to_string(path).expect("Failed to read test data");
        serde_json::from_str(&content).expect("Failed to parse test data")
    }

    fn test_instrument() -> InstrumentAny {
        use crate::http::parse::{create_instrument_from_def, parse_gamma_market};
        let market: crate::http::models::GammaMarket = load("gamma_market.json");
        let defs = parse_gamma_market(&market).unwrap();
        create_instrument_from_def(&defs[0], UnixNanos::from(1_000_000_000u64)).unwrap()
    }

    fn test_emitter() -> ExecutionEventEmitter {
        ExecutionEventEmitter::new(
            nautilus_core::time::get_atomic_clock_realtime(),
            TraderId::from("TESTER-001"),
            AccountId::from("POLY-001"),
            AccountType::Cash,
            Some(Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto)),
        )
    }

    #[rstest]
    fn test_build_ws_order_status_report() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);

        let report = build_ws_order_status_report(
            &order,
            &instrument,
            AccountId::from("POLY-001"),
            ts_event,
        );

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert!(report.price.is_some());
    }

    #[rstest]
    fn test_build_ws_taker_fill_report() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();
        let ts_event = UnixNanos::from(1_000_000_000u64);

        let report = build_ws_taker_fill_report(
            &trade,
            &instrument,
            AccountId::from("POLY-001"),
            LiquiditySide::Taker,
            ts_event,
        );

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.liquidity_side, LiquiditySide::Taker);
        assert_eq!(report.ts_event, ts_event);
    }

    #[rstest]
    fn test_dispatch_order_message_buffers_when_not_accepted() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();

        let mut token_instruments = AHashMap::new();
        token_instruments.insert(order.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_fills = Mutex::new(FifoCacheMap::default());
        let pending_order_reports = Mutex::new(FifoCacheMap::default());
        let emitter = test_emitter();

        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_fills: &pending_fills,
            pending_order_reports: &pending_order_reports,
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
        let guard = pending_order_reports.lock().unwrap();
        let venue_order_id = VenueOrderId::from(order.id.as_str());
        assert!(guard.get(&venue_order_id).is_some());
    }

    #[rstest]
    fn test_dispatch_trade_dedup() {
        let trade: PolymarketUserTrade = load("ws_user_trade.json");
        let instrument = test_instrument();

        let mut token_instruments = AHashMap::new();
        token_instruments.insert(trade.asset_id, instrument);

        let fill_tracker = OrderFillTrackerMap::new();
        let pending_fills = Mutex::new(FifoCacheMap::default());
        let pending_order_reports = Mutex::new(FifoCacheMap::default());
        let emitter = test_emitter();

        let ctx = WsDispatchContext {
            token_instruments: &token_instruments,
            fill_tracker: &fill_tracker,
            pending_fills: &pending_fills,
            pending_order_reports: &pending_order_reports,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock: nautilus_core::time::get_atomic_clock_realtime(),
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        // First dispatch processes the trade
        let _ = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let fills_count = {
            let guard = pending_fills.lock().unwrap();
            let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
            guard.get(&venue_order_id).map_or(0, |v| v.len())
        };
        assert_eq!(fills_count, 1);

        // Second dispatch should be deduped — no additional fill
        let _ = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let fills_count_after = {
            let guard = pending_fills.lock().unwrap();
            let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
            guard.get(&venue_order_id).map_or(0, |v| v.len())
        };
        assert_eq!(fills_count_after, 1);
    }
}
