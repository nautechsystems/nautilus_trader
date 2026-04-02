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
//! Routes user-channel WS messages (order updates and trades) into Nautilus
//! order status reports and fill reports. Reports are emitted immediately if
//! the order is already accepted, otherwise buffered until acceptance.
//! Trade fills are deduped via a FIFO cache. Maker and taker fills are
//! handled separately to account for multi-leg maker order matching.

use std::sync::Mutex;

use nautilus_common::cache::fifo::{FifoCache, FifoCacheMap};
use nautilus_core::{MUTEX_POISONED, UUID4, UnixNanos, collections::AtomicMap, time::AtomicTime};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderType, TimeInForce},
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
    pub token_instruments: &'a AtomicMap<Ustr, InstrumentAny>,
    pub fill_tracker: &'a OrderFillTrackerMap,
    pub pending_fills: &'a Mutex<FifoCacheMap<VenueOrderId, Vec<FillReport>, 1_000>>,
    pub pending_order_reports: &'a Mutex<FifoCacheMap<VenueOrderId, Vec<OrderStatusReport>, 1_000>>,
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
            dispatch_order_update(order, ctx);
            None
        }
        UserWsMessage::Trade(trade) => dispatch_trade_update(trade, ctx, state),
    }
}

fn dispatch_order_update(order: &PolymarketUserOrder, ctx: &WsDispatchContext<'_>) {
    let instrument = match ctx.token_instruments.get_cloned(&order.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in order update: {}", order.asset_id);
            return;
        }
    };

    let ts_event = parse_timestamp_ms(&order.timestamp).unwrap_or_else(|_| ctx.clock.get_time_ns());
    let venue_order_id = VenueOrderId::from(order.id.as_str());

    let ts_init = ctx.clock.get_time_ns();
    let mut report =
        build_ws_order_status_report(order, &instrument, ctx.account_id, ts_event, ts_init);
    let is_accepted = ctx.fill_tracker.contains(&venue_order_id);

    // Order updates can race ahead of trade messages, so cap filled_qty
    // to what the fill tracker has recorded to prevent duplicate inferred fills
    if let Some(tracked_filled) = ctx.fill_tracker.get_cumulative_filled(&venue_order_id) {
        let tracked_qty = Quantity::new(tracked_filled, instrument.size_precision());
        if report.filled_qty > tracked_qty {
            log::debug!(
                "Capping filled_qty for {venue_order_id} from {} to {} (awaiting trade messages)",
                report.filled_qty,
                tracked_qty,
            );
            report.filled_qty = tracked_qty;
        }
    }

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
            ts_init,
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
    let ts_init = ctx.clock.get_time_ns();

    if is_maker {
        dispatch_maker_fills(trade, ctx, liquidity_side, ts_event, ts_init);
    } else {
        dispatch_taker_fill(trade, ctx, liquidity_side, ts_event, ts_init);
    }

    if needs_refresh {
        Some(AccountRefreshRequest)
    } else {
        None
    }
}

fn dispatch_maker_fills(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
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
        let instrument = match ctx.token_instruments.get_cloned(&asset_id) {
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
            ts_init,
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

fn dispatch_taker_fill(
    trade: &PolymarketUserTrade,
    ctx: &WsDispatchContext<'_>,
    liquidity_side: LiquiditySide,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) {
    let instrument = match ctx.token_instruments.get_cloned(&trade.asset_id) {
        Some(i) => i,
        None => {
            log::warn!("Unknown asset_id in trade: {}", trade.asset_id);
            return;
        }
    };

    let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());

    let mut report = build_ws_taker_fill_report(
        trade,
        &instrument,
        ctx.account_id,
        liquidity_side,
        ts_event,
        ts_init,
    );
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

fn build_ws_order_status_report(
    order: &PolymarketUserOrder,
    instrument: &InstrumentAny,
    account_id: AccountId,
    ts_event: UnixNanos,
    ts_init: UnixNanos,
) -> OrderStatusReport {
    let venue_order_id = VenueOrderId::from(order.id.as_str());
    let order_status =
        crate::execution::parse::resolve_order_status(order.status, order.event_type);
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
        ts_init,
        None,
    );
    report.price = Some(price);
    report
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
        ts_init,
        client_order_id: None,
        venue_position_id: None,
    }
}

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
    use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
    use nautilus_core::time::AtomicTime;
    use nautilus_model::{
        enums::{AccountType, CurrencyType, OrderStatus},
        identifiers::TraderId,
        types::Currency,
    };
    use rstest::rstest;

    use super::*;
    use crate::http::{
        models::GammaMarket,
        parse::{create_instrument_from_def, parse_gamma_market},
    };

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
            Some(Currency::new("USDC", 6, 0, "USDC", CurrencyType::Crypto)),
        )
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
        );

        assert_eq!(report.order_side, OrderSide::Buy);
        assert_eq!(report.order_type, OrderType::Limit);
        assert!(report.price.is_some());
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
        );

        assert_eq!(report.order_status, OrderStatus::Canceled);
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
        assert_eq!(report.ts_event, ts_event);
        assert_eq!(report.ts_init, ts_init);
    }

    #[rstest]
    fn test_dispatch_order_message_buffers_when_not_accepted() {
        let order: PolymarketUserOrder = load("ws_user_order_placement.json");
        let instrument = test_instrument();

        let token_instruments = AtomicMap::new();
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

        let token_instruments = AtomicMap::new();
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

        // Second dispatch should be deduped, no additional fill
        let _ = dispatch_user_message(&UserWsMessage::Trade(trade.clone()), &ctx, &mut state);
        let fills_count_after = {
            let guard = pending_fills.lock().unwrap();
            let venue_order_id = VenueOrderId::from(trade.taker_order_id.as_str());
            guard.get(&venue_order_id).map_or(0, |v| v.len())
        };
        assert_eq!(fills_count_after, 1);
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

        let pending_fills = Mutex::new(FifoCacheMap::default());
        let pending_order_reports = Mutex::new(FifoCacheMap::default());
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
        fill_tracker.record_fill(&venue_order_id, 50.0, 0.5, UnixNanos::from(1_000u64));

        let pending_fills = Mutex::new(FifoCacheMap::default());
        let pending_order_reports = Mutex::new(FifoCacheMap::default());
        let mut emitter = test_emitter();
        let (sender, mut receiver) = tokio::sync::mpsc::unbounded_channel();
        emitter.set_sender(sender);

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
    fn test_dispatch_order_matched_dust_fill_uses_local_ts_init() {
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
        fill_tracker.record_fill(&venue_order_id, 99.995, 0.5, UnixNanos::from(1_000u64));

        let pending_fills = Mutex::new(FifoCacheMap::default());
        let pending_order_reports = Mutex::new(FifoCacheMap::default());
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
            pending_fills: &pending_fills,
            pending_order_reports: &pending_order_reports,
            emitter: &emitter,
            account_id: AccountId::from("POLY-001"),
            clock,
            user_address: "0xtest",
            user_api_key: "test-key",
        };
        let mut state = WsDispatchState::default();

        dispatch_user_message(&UserWsMessage::Order(order), &ctx, &mut state);

        let first = receiver.try_recv().expect("Expected order report");
        let second = receiver.try_recv().expect("Expected dust fill report");

        match first {
            ExecutionEvent::Report(ExecutionReport::Order(_)) => {}
            other => panic!("Expected order report, was {other:?}"),
        }

        match second {
            ExecutionEvent::Report(ExecutionReport::Fill(fill_report)) => {
                assert_eq!(
                    fill_report.ts_event,
                    UnixNanos::from(1_703_875_201_000_000_000u64)
                );
                assert_eq!(fill_report.ts_init, UnixNanos::from(2_000_000_000u64));
            }
            other => panic!("Expected fill report, was {other:?}"),
        }
    }
}
