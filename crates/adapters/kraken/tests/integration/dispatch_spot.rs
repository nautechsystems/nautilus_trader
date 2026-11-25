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

//! Integration tests for the Kraken Spot v2 WebSocket execution dispatch.
//!
//! Validates the two-tier routing contract from
//! `docs/developer_guide/adapters.md#tracked-and-external-execution-updates`:
//! tracked orders for the spot product (registered at submission via `OrderIdentity`) emit typed
//! [`OrderEventAny`] events; untracked / external orders fall back to
//! [`ExecutionReport`] variants.

use std::sync::Arc;

use nautilus_common::messages::{ExecutionEvent, ExecutionReport};
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_kraken::{
    common::enums::{KrakenOrderSide, KrakenOrderType, KrakenTimeInForce},
    websocket::{
        dispatch::{self, WsDispatchState},
        spot_v2::{
            enums::{KrakenExecType, KrakenLiquidityInd, KrakenWsOrderStatus},
            messages::{KrakenWsExecutionData, KrakenWsExecutionTriggers},
        },
    },
};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType},
    events::OrderEventAny,
    identifiers::{ClientOrderId, InstrumentId, Symbol, TradeId},
    instruments::{Instrument, InstrumentAny, currency_pair::CurrencyPair},
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use rust_decimal_macros::dec;

use crate::common::{
    CAPTURED_MARKET_ORDER_ID, CAPTURED_STOP_ORDER_ID, account_id, captured_spot_frames,
    dispatch_spot_frames, drain_events, empty_decimal_map, empty_string_map, make_identity,
    test_emitter, usdc_eur_pair,
};

const SPOT_SYMBOL: &str = "BTC/USDT";
const SPOT_INSTRUMENT_ID: &str = "BTC/USDT.KRAKEN";

fn make_spot_pair() -> InstrumentAny {
    InstrumentAny::CurrencyPair(
        CurrencyPair::builder()
            .instrument_id(InstrumentId::from(SPOT_INSTRUMENT_ID))
            .raw_symbol(Symbol::from(SPOT_SYMBOL))
            .base_currency(Currency::BTC())
            .quote_currency(Currency::from("USDT"))
            .price_precision(1)
            .size_precision(8)
            .price_increment(Price::from("0.1"))
            .size_increment(Quantity::from("0.00000001"))
            .ts_event(UnixNanos::default())
            .ts_init(UnixNanos::default())
            .build()
            .unwrap(),
    )
}

fn instruments_with(instrument: InstrumentAny) -> Arc<AtomicMap<InstrumentId, InstrumentAny>> {
    let map = Arc::new(AtomicMap::new());
    map.insert(instrument.id(), instrument);
    map
}

fn make_spot_execution(
    exec_type: KrakenExecType,
    cl_ord_id: Option<&str>,
    venue_order_id: &str,
    exec_id: Option<&str>,
) -> KrakenWsExecutionData {
    KrakenWsExecutionData {
        exec_type,
        order_id: venue_order_id.to_string(),
        cl_ord_id: cl_ord_id.map(str::to_string),
        symbol: Some(SPOT_SYMBOL.to_string()),
        side: Some(KrakenOrderSide::Buy),
        order_type: Some(KrakenOrderType::Limit),
        order_qty: Some(dec!(0.0001)),
        display_qty: None,
        limit_price: Some(dec!(70000)),
        order_status: match exec_type {
            KrakenExecType::Filled => Some(KrakenWsOrderStatus::Filled),
            KrakenExecType::Canceled => Some(KrakenWsOrderStatus::Canceled),
            KrakenExecType::Expired => Some(KrakenWsOrderStatus::Expired),
            KrakenExecType::New => Some(KrakenWsOrderStatus::New),
            _ => None,
        },
        cum_qty: None,
        cum_cost: None,
        avg_price: None,
        time_in_force: Some(KrakenTimeInForce::GoodTilCancelled),
        expire_time: None,
        post_only: Some(true),
        reduce_only: Some(false),
        triggers: None,
        timestamp: "2026-04-11T00:00:00.000Z".parse().unwrap(),
        exec_id: exec_id.map(str::to_string),
        last_qty: exec_id.map(|_| dec!(0.0001)),
        last_price: exec_id.map(|_| dec!(70000)),
        cost: exec_id.map(|_| dec!(7)),
        liquidity_ind: exec_id.map(|_| KrakenLiquidityInd::Maker),
        fees: None,
        fee_usd_equiv: None,
        reason: None,
    }
}

#[rstest]
fn test_spot_execution_new_tracked_emits_order_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-1");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let exec = make_spot_execution(KrakenExecType::New, Some("uuid-spot-1"), "v-spot-1", None);
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
}

#[rstest]
fn test_spot_execution_canceled_tracked_synthesizes_accepted_then_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-2");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let exec = make_spot_execution(
        KrakenExecType::Canceled,
        Some("uuid-spot-2"),
        "v-spot-2",
        None,
    );
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(matches!(
        events[1],
        ExecutionEvent::Order(OrderEventAny::Canceled(_))
    ));
    assert!(state.lookup_identity(&cid).is_none());
}

#[rstest]
fn test_spot_execution_trade_tracked_emits_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-3");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let exec = make_spot_execution(
        KrakenExecType::Trade,
        Some("uuid-spot-3"),
        "v-spot-3",
        Some("trade-spot-3"),
    );
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    // Trade exec has no order_status (we mapped it to None) so the status
    // path emits nothing; only the fill path fires. ensure_accepted_emitted
    // synthesizes Accepted before the fill.
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(matches!(
        events[1],
        ExecutionEvent::Order(OrderEventAny::Filled(_))
    ));
}

#[rstest]
fn test_spot_execution_trade_external_emits_fill_report() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let exec = make_spot_execution(KrakenExecType::Trade, None, "v-spot-ext", Some("trade-ext"));
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    // Untracked Trade exec emits both an OrderStatusReport (status path)
    // and a FillReport (fill path) because the exec carries both kinds of
    // information; the engine reconciles them.
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    assert!(
        events
            .iter()
            .all(|e| matches!(e, ExecutionEvent::Report(_)))
    );
}

#[rstest]
fn test_spot_execution_dedup_skips_duplicate_trade_id() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    state.register_identity(
        ClientOrderId::new("uuid-spot-4"),
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let instruments = instruments_with(make_spot_pair());
    let truncated = empty_string_map();
    let qty_cache = empty_decimal_map();

    let exec = make_spot_execution(
        KrakenExecType::Trade,
        Some("uuid-spot-4"),
        "v-spot-4",
        Some("trade-spot-dup"),
    );
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments,
        &truncated,
        &qty_cache,
        account_id(),
        UnixNanos::default(),
    );
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments,
        &truncated,
        &qty_cache,
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    // Accepted + Filled from the first dispatch; the second is fully deduped.
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(matches!(
        events[1],
        ExecutionEvent::Order(OrderEventAny::Filled(_))
    ));
}

/// An execution frame whose venue order ID the model cannot hold emits nothing, tracked or not,
/// with or without a fill, and a tracked order stays tracked for a later frame or reconciliation.
#[rstest]
#[case::status_tracked(KrakenExecType::New, Some("uuid-invalid-spot"), None)]
#[case::status_external(KrakenExecType::New, None, None)]
#[case::fill_tracked(
    KrakenExecType::Trade,
    Some("uuid-invalid-spot"),
    Some("trade-invalid")
)]
#[case::fill_external(KrakenExecType::Trade, None, Some("trade-invalid"))]
fn test_spot_execution_drops_an_invalid_venue_order_id(
    #[case] exec_type: KrakenExecType,
    #[case] cl_ord_id: Option<&str>,
    #[case] exec_id: Option<&str>,
) {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-invalid-spot");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let exec = make_spot_execution(exec_type, cl_ord_id, "", exec_id);
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(drain_events(&mut rx).is_empty());
    assert!(state.lookup_identity(&cid).is_some());
}

#[rstest]
fn test_spot_execution_new_external_emits_status_report() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let exec = make_spot_execution(KrakenExecType::New, None, "v-spot-ext-2", None);
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_spot_execution_triggered_emits_order_triggered() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-trig");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::StopLimit),
    );

    let mut exec = make_spot_execution(
        KrakenExecType::Status,
        Some("uuid-spot-trig"),
        "v-spot-trig",
        None,
    );
    exec.order_status = Some(KrakenWsOrderStatus::Triggered);

    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(matches!(
        events[1],
        ExecutionEvent::Order(OrderEventAny::Triggered(_))
    ));
}

/// A tracked order already reported triggered emits no second `OrderTriggered` when a later frame,
/// such as its entry in a reconnect's open-orders snapshot, restates it triggered.
#[rstest]
fn test_spot_execution_triggered_again_emits_nothing() {
    let state = WsDispatchState::new();
    let cid = ClientOrderId::new("uuid-spot-retrig");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::StopLimit),
    );
    let mut triggered = make_spot_execution(
        KrakenExecType::New,
        Some("uuid-spot-retrig"),
        "v-spot-retrig",
        None,
    );
    triggered.order_status = Some(KrakenWsOrderStatus::Triggered);

    let events = dispatch_spot_frames(&[triggered.clone(), triggered], &state, &make_spot_pair());

    assert!(matches!(
        events[0].as_slice(),
        [
            ExecutionEvent::Order(OrderEventAny::Accepted(_)),
            ExecutionEvent::Order(OrderEventAny::Triggered(_)),
        ]
    ));
    assert!(events[1].is_empty(), "{:?}", events[1]);
}

#[rstest]
fn test_spot_execution_amended_emits_order_updated() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-amend");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_spot_pair());

    // Initial placement.
    let new_exec = make_spot_execution(
        KrakenExecType::New,
        Some("uuid-spot-amend"),
        "v-spot-amend",
        None,
    );
    dispatch::spot::execution(
        &new_exec,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Modify ack via Amended exec_type.
    let amended_exec = make_spot_execution(
        KrakenExecType::Amended,
        Some("uuid-spot-amend"),
        "v-spot-amend",
        None,
    );
    dispatch::spot::execution(
        &amended_exec,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Updated(_))
    ));
}

#[rstest]
fn test_spot_execution_filled_marker_cleans_up_state() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-term");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_spot_pair());

    // Drive an Accepted first so emitted_accepted is set.
    let new_exec = make_spot_execution(KrakenExecType::New, Some("uuid-spot-term"), "v-term", None);
    dispatch::spot::execution(
        &new_exec,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Filled exec_type is the terminal marker (the trade-side of the same
    // execution emits the actual OrderFilled event in production; here we
    // just exercise the cleanup path).
    let filled_exec = make_spot_execution(
        KrakenExecType::Filled,
        Some("uuid-spot-term"),
        "v-term",
        None,
    );
    dispatch::spot::execution(
        &filled_exec,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_spot_execution_stale_after_terminal_is_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-stale");
    state.insert_filled(cid);

    let exec = make_spot_execution(
        KrakenExecType::New,
        Some("uuid-spot-stale"),
        "v-spot-stale",
        None,
    );
    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_spot_filled_with_fill_payload_defers_cleanup_until_after_fill() {
    // Filled status arriving in the same execution as the fill payload must
    // not cleanup before the fill side runs.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-final-trade");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let mut exec = make_spot_execution(
        KrakenExecType::Trade,
        Some("uuid-spot-final-trade"),
        "v-spot-final",
        Some("trade-final"),
    );
    exec.order_status = Some(KrakenWsOrderStatus::Filled);

    dispatch::spot::execution(
        &exec,
        &state,
        &emitter,
        &instruments_with(make_spot_pair()),
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    // The status side sees Filled but defers because exec_id is present;
    // the fill side then synthesizes Accepted and emits Filled, and only
    // *then* terminal-cleanup runs.
    assert_eq!(events.len(), 2);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(matches!(
        events[1],
        ExecutionEvent::Order(OrderEventAny::Filled(_))
    ));
    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_spot_restated_emits_order_updated() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-restate");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_spot_pair());

    // Placement.
    let new_exec = make_spot_execution(
        KrakenExecType::New,
        Some("uuid-spot-restate"),
        "v-spot-restate",
        None,
    );
    dispatch::spot::execution(
        &new_exec,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Engine-initiated restatement.
    let restated = make_spot_execution(
        KrakenExecType::Restated,
        Some("uuid-spot-restate"),
        "v-spot-restate",
        None,
    );
    dispatch::spot::execution(
        &restated,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Updated(_))
    ));
}

/// Builds a delta-style execution frame matching what Kraken sends as a
/// follow-up to `pending_new` (and for `amended` / `restated` / `status`):
/// only `order_id`, `exec_type`, `order_status`, and `timestamp` are
/// populated - every other field, including `symbol`, is `None`.
fn make_spot_execution_delta(
    exec_type: KrakenExecType,
    venue_order_id: &str,
    order_status: KrakenWsOrderStatus,
) -> KrakenWsExecutionData {
    KrakenWsExecutionData {
        exec_type,
        order_id: venue_order_id.to_string(),
        cl_ord_id: None,
        symbol: None,
        side: None,
        order_type: None,
        order_qty: None,
        display_qty: None,
        limit_price: None,
        order_status: Some(order_status),
        cum_qty: None,
        cum_cost: None,
        avg_price: None,
        time_in_force: None,
        expire_time: None,
        post_only: None,
        reduce_only: None,
        triggers: None,
        timestamp: "2026-04-11T00:00:00.001Z".parse().unwrap(),
        exec_id: None,
        last_qty: None,
        last_price: None,
        cost: None,
        liquidity_ind: None,
        fees: None,
        fee_usd_equiv: None,
        reason: None,
    }
}

#[rstest]
#[case::new(KrakenExecType::New, KrakenWsOrderStatus::New)]
#[case::amended(KrakenExecType::Amended, KrakenWsOrderStatus::New)]
#[case::restated(KrakenExecType::Restated, KrakenWsOrderStatus::New)]
#[case::status(KrakenExecType::Status, KrakenWsOrderStatus::New)]
fn test_spot_pending_new_then_symbolless_delta_resolves_via_cache(
    #[case] delta_exec_type: KrakenExecType,
    #[case] delta_order_status: KrakenWsOrderStatus,
) {
    // Untracked / external order: `pending_new` arrives with full data and
    // emits an initial report, then a delta frame (no symbol) arrives and
    // must still emit a report via the cached symbol populated from the
    // first frame. Covers every delta exec type that omits `symbol`
    // (`new` / `amended` / `restated` / `status`) per Kraken's executions
    // docs.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let instruments = instruments_with(make_spot_pair());

    let pending = make_spot_execution(KrakenExecType::PendingNew, None, "v-spot-delta", None);
    dispatch::spot::execution(
        &pending,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    assert_eq!(drain_events(&mut rx).len(), 1);

    let delta = make_spot_execution_delta(delta_exec_type, "v-spot-delta", delta_order_status);
    dispatch::spot::execution(
        &delta,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(
        events.len(),
        1,
        "delta frame should resolve via the cached symbol and emit a report"
    );
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_spot_delta_without_cached_symbol_is_dropped() {
    // No prior `pending_new` -> no cached symbol -> the delta frame must be
    // skipped (existing behavior, no panic, no event).
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let instruments = instruments_with(make_spot_pair());

    let new_delta = make_spot_execution_delta(
        KrakenExecType::New,
        "v-spot-no-prior",
        KrakenWsOrderStatus::New,
    );
    dispatch::spot::execution(
        &new_delta,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_spot_tracked_pending_new_seeds_caches_no_event_yet() {
    // Realistic `pending_new` for a tracked order: carries `cl_ord_id` and
    // `symbol`, parses as `OrderStatus::Submitted` (which `status_tracked`
    // treats as a no-op since the engine already has the order Submitted
    // locally). Both venue-id caches must be seeded so the follow-up delta
    // can resolve the identity.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-tracked");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_spot_pair());

    let mut pending = make_spot_execution(
        KrakenExecType::PendingNew,
        Some("uuid-spot-tracked"),
        "v-spot-tracked",
        None,
    );
    pending.order_status = Some(KrakenWsOrderStatus::PendingNew);
    dispatch::spot::execution(
        &pending,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(
        drain_events(&mut rx).is_empty(),
        "tracked pending_new parses as Submitted; status_tracked emits nothing"
    );
    assert_eq!(
        state.lookup_order_symbol("v-spot-tracked").as_deref(),
        Some(SPOT_SYMBOL)
    );
    assert_eq!(state.lookup_order_client_id("v-spot-tracked"), Some(cid));
}

#[rstest]
fn test_spot_tracked_delta_new_without_cl_ord_id_emits_order_accepted() {
    // Issue #4051: Kraken's `new` delta lacks both `symbol` and `cl_ord_id`.
    // With both venue-id caches seeded from the prior `pending_new` the
    // dispatch must still resolve the tracked identity and emit
    // `OrderAccepted`. Without the `order_client_id_cache` lookup this
    // delta falls through to the untracked report path and the strategy
    // never transitions out of `Submitted`.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-spot-tracked-delta");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_spot_pair());

    let mut pending = make_spot_execution(
        KrakenExecType::PendingNew,
        Some("uuid-spot-tracked-delta"),
        "v-spot-tracked-delta",
        None,
    );
    pending.order_status = Some(KrakenWsOrderStatus::PendingNew);
    dispatch::spot::execution(
        &pending,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    let delta = make_spot_execution_delta(
        KrakenExecType::New,
        "v-spot-tracked-delta",
        KrakenWsOrderStatus::New,
    );
    dispatch::spot::execution(
        &delta,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(
        events.len(),
        1,
        "delta `new` without cl_ord_id must resolve via the venue-id cache and emit OrderAccepted"
    );
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
}

#[rstest]
fn test_spot_terminal_eviction_runs_on_missing_instrument_early_return() {
    // Terminal cleanup must run regardless of which early return the inner
    // dispatch hits. Here `lookup_instrument` bails because no instruments
    // are registered, but the outer eviction still clears both venue-id
    // caches for the terminal exec type.
    let state = Arc::new(WsDispatchState::new());
    let (emitter, _rx) = test_emitter();
    let cid = ClientOrderId::new("uuid-spot-orphan");

    state.cache_order_symbol("v-spot-orphan", "FOO/BAR");
    state.cache_order_client_id("v-spot-orphan", cid);

    let empty_instruments: Arc<AtomicMap<InstrumentId, InstrumentAny>> = Arc::new(AtomicMap::new());
    let canceled = make_spot_execution_delta(
        KrakenExecType::Canceled,
        "v-spot-orphan",
        KrakenWsOrderStatus::Canceled,
    );
    dispatch::spot::execution(
        &canceled,
        &state,
        &emitter,
        &empty_instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(state.lookup_order_symbol("v-spot-orphan").is_none());
    assert!(state.lookup_order_client_id("v-spot-orphan").is_none());
}

#[rstest]
fn test_spot_terminal_exec_type_evicts_symbol_cache() {
    let state = Arc::new(WsDispatchState::new());
    let (emitter, _rx) = test_emitter();
    let instruments = instruments_with(make_spot_pair());

    let pending = make_spot_execution(
        KrakenExecType::PendingNew,
        Some("uuid-spot-evict"),
        "v-spot-evict",
        None,
    );
    dispatch::spot::execution(
        &pending,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    assert!(state.lookup_order_symbol("v-spot-evict").is_some());
    assert!(state.lookup_order_client_id("v-spot-evict").is_some());

    let canceled = make_spot_execution_delta(
        KrakenExecType::Canceled,
        "v-spot-evict",
        KrakenWsOrderStatus::Canceled,
    );
    dispatch::spot::execution(
        &canceled,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_decimal_map(),
        account_id(),
        UnixNanos::default(),
    );
    assert!(state.lookup_order_symbol("v-spot-evict").is_none());
    assert!(state.lookup_order_client_id("v-spot-evict").is_none());
}

/// An untracked order's `pending_new` frame: a buy limit of 0.0002 at 70000.0.
fn external_pending_new(venue_order_id: &str) -> KrakenWsExecutionData {
    let mut exec = make_spot_execution(KrakenExecType::PendingNew, None, venue_order_id, None);
    exec.order_status = Some(KrakenWsOrderStatus::PendingNew);
    exec.order_qty = Some(dec!(0.0002));
    exec
}

/// A `trade` frame filling 0.0001 of `external_pending_new`'s order at 70000.0, stating what
/// Kraken's trade frames state.
fn external_trade(venue_order_id: &str, exec_id: &str, cum_qty: Decimal) -> KrakenWsExecutionData {
    let mut exec = make_spot_execution_delta(
        KrakenExecType::Trade,
        venue_order_id,
        KrakenWsOrderStatus::PartiallyFilled,
    );
    exec.symbol = Some(SPOT_SYMBOL.to_string());
    exec.side = Some(KrakenOrderSide::Buy);
    exec.order_type = Some(KrakenOrderType::Limit);
    exec.exec_id = Some(exec_id.to_string());
    exec.last_qty = Some(dec!(0.0001));
    exec.last_price = Some(dec!(70000));
    exec.cum_qty = Some(cum_qty);
    exec.avg_price = Some(dec!(70000));
    exec.liquidity_ind = Some(KrakenLiquidityInd::Maker);
    exec
}

/// A stop-loss-limit buy's `pending_new`, its trigger 69000.0 on last and its limit 70000.0.
fn external_stop_pending_new(venue_order_id: &str) -> KrakenWsExecutionData {
    let mut exec = external_pending_new(venue_order_id);
    exec.order_type = Some(KrakenOrderType::StopLossLimit);
    exec.post_only = None;
    exec.triggers = Some(KrakenWsExecutionTriggers {
        reference: Some("last".to_string()),
        price: Some(dec!(69000)),
        price_type: Some("static".to_string()),
    });
    exec
}

/// Returns `exec` with the fields of the JSON object `patch` replaced.
fn patched(exec: &KrakenWsExecutionData, patch: &str) -> KrakenWsExecutionData {
    let mut value = serde_json::to_value(exec).unwrap();
    let fields: serde_json::Map<String, serde_json::Value> = serde_json::from_str(patch).unwrap();
    for (key, field) in fields {
        value[key] = field;
    }
    serde_json::from_value(value).unwrap()
}

/// The order status report of a frame that emitted one, and the fills bundled with it.
fn single_report(events: &[ExecutionEvent]) -> (&OrderStatusReport, &[FillReport]) {
    match events {
        [ExecutionEvent::Report(ExecutionReport::Order(report))] => (report, &[]),
        [ExecutionEvent::Report(ExecutionReport::OrderWithFills(report, fills))] => (report, fills),
        other => panic!("expected one order status report, received {other:?}"),
    }
}

/// Each captured frame of an order placed outside the node reports the order's whole shape:
/// `pending_new` reports it accepted, a `new` frame after the trigger reports it triggered, and
/// the trade reports its status and fill together, filled at the order quantity.
#[rstest]
#[case::stop_loss_limit_buy(
    CAPTURED_STOP_ORDER_ID,
    OrderSide::Buy,
    OrderType::StopLimit,
    Some("0.9132"),
    Some(("0.9121", TriggerType::LastPrice)),
    &[
        OrderStatus::Accepted,
        OrderStatus::Triggered,
        OrderStatus::Triggered,
        OrderStatus::Filled,
        OrderStatus::Filled,
    ],
    ("TCAP14-STLMT-000014", "0.9122", dec!(0.009122)),
)]
#[case::market_sell(
    CAPTURED_MARKET_ORDER_ID,
    OrderSide::Sell,
    OrderType::Market,
    None,
    None,
    &[
        OrderStatus::Accepted,
        OrderStatus::Accepted,
        OrderStatus::Filled,
        OrderStatus::Filled,
    ],
    ("TCAP15-MARKT-000015", "0.9121", dec!(0.009121)),
)]
fn test_spot_external_captured_orders_report_each_frame(
    #[case] order_id: &str,
    #[case] side: OrderSide,
    #[case] order_type: OrderType,
    #[case] price: Option<&str>,
    #[case] trigger: Option<(&str, TriggerType)>,
    #[case] statuses: &[OrderStatus],
    #[case] fill: (&str, &str, Decimal),
) {
    let frames = captured_spot_frames(order_id);
    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &usdc_eur_pair());

    let reports: Vec<_> = events.iter().map(|e| single_report(e)).collect();
    let quantity = Quantity::from("5.00000000");

    for (report, _) in &reports {
        assert_eq!(
            (
                report.order_side,
                report.order_type,
                report.quantity,
                report.price,
                report.trigger_price,
                report.trigger_type,
                report.time_in_force,
            ),
            (
                Some(side),
                order_type,
                quantity,
                price.map(Price::from),
                trigger.map(|(px, _)| Price::from(px)),
                trigger.map(|(_, trigger_type)| trigger_type),
                TimeInForce::Gtc,
            ),
        );
    }
    let reported: Vec<_> = reports.iter().map(|(r, _)| r.order_status).collect();
    assert_eq!(reported, statuses);

    let bundled: Vec<_> = reports.iter().map(|(_, fills)| fills.len()).collect();
    let trade_index = frames
        .iter()
        .position(|exec| exec.exec_type == KrakenExecType::Trade)
        .unwrap();
    let mut expected_bundled = vec![0; frames.len()];
    expected_bundled[trade_index] = 1;
    assert_eq!(bundled, expected_bundled);

    let (trade_report, fills) = &reports[trade_index];
    assert_eq!(trade_report.filled_qty, quantity);
    let (trade_id, last_px, fee) = fill;
    assert_eq!(
        (
            fills[0].trade_id,
            fills[0].last_qty,
            fills[0].last_px,
            fills[0].commission,
            fills[0].liquidity_side,
        ),
        (
            TradeId::from(trade_id),
            quantity,
            Price::from(last_px),
            Money::from_decimal(fee, Currency::EUR()).unwrap(),
            LiquiditySide::Taker,
        ),
    );
}

/// A trade on an order seen placed reports its status and fill together, the status carrying the
/// order quantity from `pending_new`, so the engine never infers the fill the trade carries. Both
/// carry the client order ID the other client gave the order, if any.
#[rstest]
fn test_spot_external_partial_trade_reports_status_with_its_fill(
    #[values(None, Some("other-client-1"))] cl_ord_id: Option<&str>,
) {
    let state = WsDispatchState::new();
    let mut pending = external_pending_new("v-ext-partial");
    pending.cl_ord_id = cl_ord_id.map(str::to_string);
    let frames = [
        pending,
        external_trade("v-ext-partial", "T-ext-partial", dec!(0.0001)),
    ];

    let events = dispatch_spot_frames(&frames, &state, &make_spot_pair());

    let (report, fills) = single_report(&events[1]);
    let client_order_id = cl_ord_id.map(ClientOrderId::new);
    assert_eq!(
        (
            report.order_status,
            report.quantity,
            report.filled_qty,
            report.price,
            report.client_order_id,
        ),
        (
            OrderStatus::PartiallyFilled,
            Quantity::from("0.00020000"),
            Quantity::from("0.00010000"),
            Some(Price::from("70000.0")),
            client_order_id,
        ),
    );
    assert_eq!(fills.len(), 1);
    assert_eq!(
        (fills[0].trade_id, fills[0].client_order_id),
        (TradeId::from("T-ext-partial"), client_order_id)
    );
}

/// A replayed trade on an order seen placed emits nothing: its fill is applied, and its
/// cumulative quantity is stale.
#[rstest]
fn test_spot_external_trade_replay_emits_nothing() {
    let trade = external_trade("v-ext-replay", "T-ext-replay", dec!(0.0001));
    let frames = [external_pending_new("v-ext-replay"), trade.clone(), trade];

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    assert_eq!(events[1].len(), 1);
    assert!(events[2].is_empty());
}

/// A terminal frame after a partial fill reports the order quantity from `pending_new` and the
/// cumulative quantity the frame states, or the last one stated when it states none.
#[rstest]
fn test_spot_external_terminal_after_partial_fill_keeps_order_quantity(
    #[values(
        (KrakenExecType::Canceled, KrakenWsOrderStatus::Canceled, OrderStatus::Canceled),
        (KrakenExecType::Expired, KrakenWsOrderStatus::Expired, OrderStatus::Expired)
    )]
    terminal: (KrakenExecType, KrakenWsOrderStatus, OrderStatus),
    #[values(Some(dec!(0.0001)), None)] terminal_cum_qty: Option<Decimal>,
) {
    let (exec_type, order_status, expected_status) = terminal;
    let mut terminal = make_spot_execution_delta(exec_type, "v-ext-terminal", order_status);
    terminal.cum_qty = terminal_cum_qty;
    let frames = [
        external_pending_new("v-ext-terminal"),
        external_trade("v-ext-terminal", "T-ext-terminal", dec!(0.0001)),
        terminal,
    ];

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    let (report, _) = single_report(&events[2]);
    assert_eq!(
        (
            report.order_status,
            report.order_side,
            report.quantity,
            report.filled_qty
        ),
        (
            expected_status,
            Some(OrderSide::Buy),
            Quantity::from("0.00020000"),
            Quantity::from("0.00010000"),
        ),
    );
}

/// An amendment's quantity and limit reach every later report of the order, filled or not.
#[rstest]
fn test_spot_external_amend_carries_new_terms_forward(
    #[values(KrakenExecType::Amended, KrakenExecType::Restated)] amend_type: KrakenExecType,
    #[values(false, true)] partially_filled: bool,
) {
    let mut amend = make_spot_execution_delta(amend_type, "v-ext-amend", KrakenWsOrderStatus::New);
    amend.order_qty = Some(dec!(0.0003));
    amend.limit_price = Some(dec!(71000));
    let mut frames = vec![external_pending_new("v-ext-amend")];

    if partially_filled {
        frames.push(external_trade("v-ext-amend", "T-ext-amend", dec!(0.0001)));
    }
    frames.push(amend);
    frames.push(make_spot_execution_delta(
        KrakenExecType::New,
        "v-ext-amend",
        KrakenWsOrderStatus::New,
    ));

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    let filled = if partially_filled {
        "0.00010000"
    } else {
        "0.00000000"
    };

    for frame_events in &events[events.len() - 2..] {
        let (report, _) = single_report(frame_events);
        assert_eq!(
            (
                report.order_status,
                report.order_side,
                report.quantity,
                report.price,
                report.filled_qty,
            ),
            (
                OrderStatus::Accepted,
                Some(OrderSide::Buy),
                Quantity::from("0.00030000"),
                Some(Price::from("71000.0")),
                Quantity::from(filled),
            ),
        );
    }
}

/// Once an order is reported triggered, a `new` frame reports it triggered until it fills: the
/// engine holds a filled order partially filled, which it cannot move to triggered.
#[rstest]
#[case::unfilled(
    false,
    Some(KrakenWsOrderStatus::New),
    OrderStatus::Triggered,
    "0.00000000"
)]
#[case::partially_filled(true, None, OrderStatus::Accepted, "0.00010000")]
fn test_spot_external_new_frame_after_trigger_reports_triggered_until_filled(
    #[case] partially_filled: bool,
    #[case] new_status: Option<KrakenWsOrderStatus>,
    #[case] expected_status: OrderStatus,
    #[case] expected_filled: &str,
) {
    let mut frames = vec![
        external_stop_pending_new("v-ext-stop"),
        make_spot_execution_delta(
            KrakenExecType::New,
            "v-ext-stop",
            KrakenWsOrderStatus::Triggered,
        ),
    ];

    if partially_filled {
        let mut trade = external_trade("v-ext-stop", "T-ext-stop", dec!(0.0001));
        trade.order_type = Some(KrakenOrderType::StopLossLimit);
        frames.push(trade);
    }
    let mut new =
        make_spot_execution_delta(KrakenExecType::New, "v-ext-stop", KrakenWsOrderStatus::New);
    new.order_status = new_status;
    frames.push(new);

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    assert_eq!(
        single_report(&events[1]).0.order_status,
        OrderStatus::Triggered
    );
    let (report, _) = single_report(events.last().unwrap());
    assert_eq!(
        (report.order_status, report.filled_qty),
        (expected_status, Quantity::from(expected_filled)),
    );
}

/// An order the engine cannot build from what its frames state reports each frame alone, and a
/// trade's fill arrives as its own report.
#[rstest]
#[case::trailing_stop_pct(
    r#"{"order_type":"trailing-stop","triggers":{"reference":"last","price":1.0,"price_type":"pct"}}"#
)]
#[case::stop_loss_quote(
    r#"{"order_type":"stop-loss","triggers":{"reference":"last","price":150.0,"price_type":"quote"}}"#
)]
#[case::stop_loss_without_price_type(
    r#"{"order_type":"stop-loss","triggers":{"reference":"last","price":69000.0}}"#
)]
#[case::gtd_limit_without_expiry(r#"{"time_in_force":"GTD","post_only":false}"#)]
#[case::post_only_gtd_without_expiry(r#"{"time_in_force":"GTD","post_only":true}"#)]
#[case::take_profit_limit_inverted(
    r#"{"order_type":"take-profit-limit","triggers":{"reference":"last","price":71000.0,"price_type":"static"}}"#
)]
#[case::market_without_quantity(r#"{"order_type":"market","order_qty":null,"limit_price":0.0}"#)]
fn test_spot_external_unbuildable_order_reports_each_frame_alone(#[case] placed: &str) {
    let pending = patched(&external_pending_new("v-ext-unbuildable"), placed);
    let mut trade = external_trade("v-ext-unbuildable", "T-ext-unbuildable", dec!(0.0001));
    trade.order_type = pending.order_type;

    let events = dispatch_spot_frames(
        &[pending, trade],
        &WsDispatchState::new(),
        &make_spot_pair(),
    );

    assert_eq!(
        single_report(&events[0]).0.order_status,
        OrderStatus::Submitted
    );
    assert!(matches!(
        events[1].as_slice(),
        [
            ExecutionEvent::Report(ExecutionReport::Order(_)),
            ExecutionEvent::Report(ExecutionReport::Fill(_)),
        ]
    ));
}

/// The engine creates an order from its `pending_new` report, so a trade reports its status and
/// fill together even after an amendment leaves a status the engine could not create the order
/// from: sent apart, the status would make the engine infer the fill the trade carries.
#[rstest]
#[case::trigger_past_limit(
    r#"{"order_type":"take-profit-limit","triggers":{"reference":"last","price":69000.0,"price_type":"static"}}"#,
    r#"{"triggers":{"price":71000.0}}"#
)]
#[case::relative_trigger(
    r#"{"order_type":"stop-loss-limit","triggers":{"reference":"last","price":69000.0,"price_type":"static"}}"#,
    r#"{"triggers":{"price":1.0,"price_type":"pct"}}"#
)]
fn test_spot_external_amended_to_unbuildable_reports_trade_with_its_fill(
    #[case] placed: &str,
    #[case] amended: &str,
) {
    let pending = patched(&external_pending_new("v-ext-amended"), placed);
    let amend = make_spot_execution_delta(
        KrakenExecType::Amended,
        "v-ext-amended",
        KrakenWsOrderStatus::New,
    );
    let mut trade = external_trade("v-ext-amended", "T-ext-amended", dec!(0.0001));
    trade.order_type = pending.order_type;
    let frames = [pending, patched(&amend, amended), trade];

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    assert_eq!(
        single_report(&events[0]).0.order_status,
        OrderStatus::Accepted
    );
    let (report, fills) = single_report(&events[2]);
    assert_eq!(
        (report.order_status, report.filled_qty, fills.len()),
        (
            OrderStatus::PartiallyFilled,
            Quantity::from("0.00010000"),
            1
        )
    );
    assert_eq!(fills[0].trade_id, TradeId::from("T-ext-amended"));
}

/// A terminal frame forgets the order's shape, so a later frame reports only what it states.
#[rstest]
#[case::filled(KrakenExecType::Filled, KrakenWsOrderStatus::Filled)]
#[case::canceled(KrakenExecType::Canceled, KrakenWsOrderStatus::Canceled)]
#[case::expired(KrakenExecType::Expired, KrakenWsOrderStatus::Expired)]
fn test_spot_external_terminal_frame_forgets_order_shape(
    #[case] exec_type: KrakenExecType,
    #[case] order_status: KrakenWsOrderStatus,
) {
    let mut late = make_spot_execution_delta(
        KrakenExecType::New,
        "v-ext-forget",
        KrakenWsOrderStatus::New,
    );
    late.symbol = Some(SPOT_SYMBOL.to_string());
    let frames = [
        external_pending_new("v-ext-forget"),
        make_spot_execution_delta(exec_type, "v-ext-forget", order_status),
        late,
    ];

    let events = dispatch_spot_frames(&frames, &WsDispatchState::new(), &make_spot_pair());

    let (report, _) = single_report(&events[2]);
    assert_eq!(
        (report.order_side, report.quantity),
        (None, Quantity::from("0.00000000"))
    );
}

/// A tracked order's frames keep no shape, so once its tracking ends a later frame reports only
/// what it states.
#[rstest]
fn test_spot_tracked_frames_seed_no_order_shape() {
    let state = WsDispatchState::new();
    let cid = ClientOrderId::new("uuid-spot-unshaped");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let mut pending = external_pending_new("v-spot-unshaped");
    pending.cl_ord_id = Some("uuid-spot-unshaped".to_string());
    let instrument = make_spot_pair();

    assert!(dispatch_spot_frames(&[pending], &state, &instrument)[0].is_empty());
    state.cleanup_terminal(&cid);
    let late = make_spot_execution_delta(
        KrakenExecType::New,
        "v-spot-unshaped",
        KrakenWsOrderStatus::New,
    );
    let events = dispatch_spot_frames(&[late], &state, &instrument);

    let (report, _) = single_report(&events[0]);
    assert_eq!(
        (report.client_order_id, report.order_side, report.quantity),
        (Some(cid), None, Quantity::from("0.00000000"))
    );
}

/// A tracked conditional order's amendment updates its trigger to the static price the frame
/// states, whether or not the frame states the order's type.
#[rstest]
#[case::frame_states_type(Some(KrakenOrderType::StopLossLimit))]
#[case::frame_omits_type(None)]
fn test_spot_tracked_amended_carries_static_trigger(
    #[case] frame_order_type: Option<KrakenOrderType>,
) {
    let state = WsDispatchState::new();
    let cid = ClientOrderId::new("uuid-spot-stop-amend");
    state.register_identity(
        cid,
        make_identity(SPOT_INSTRUMENT_ID, OrderSide::Buy, OrderType::StopLimit),
    );
    let new = make_spot_execution(
        KrakenExecType::New,
        Some("uuid-spot-stop-amend"),
        "v-spot-stop-amend",
        None,
    );
    let mut amended = make_spot_execution(
        KrakenExecType::Amended,
        Some("uuid-spot-stop-amend"),
        "v-spot-stop-amend",
        None,
    );
    amended.order_type = frame_order_type;
    amended.triggers = external_stop_pending_new("v-spot-stop-amend").triggers;

    let events = dispatch_spot_frames(&[new, amended], &state, &make_spot_pair());

    let [ExecutionEvent::Order(OrderEventAny::Updated(updated))] = events[1].as_slice() else {
        panic!("expected one OrderUpdated, received {:?}", events[1]);
    };
    assert_eq!(updated.trigger_price, Some(Price::from("69000.0")));
}
