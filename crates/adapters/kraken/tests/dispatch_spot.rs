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
//! `docs/developer_guide/adapters.md` lines 1232-1296 for the spot product:
//! tracked orders (registered at submission via `OrderIdentity`) emit typed
//! [`OrderEventAny`] events; untracked / external orders fall back to
//! [`ExecutionReport`] variants.

mod common;

use std::sync::Arc;

use common::{
    account_id, drain_events, empty_f64_map, empty_string_map, make_identity, test_emitter,
};
use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_kraken::{
    common::enums::{KrakenOrderSide, KrakenOrderType, KrakenTimeInForce},
    websocket::{
        dispatch::{self, WsDispatchState},
        spot_v2::{
            enums::{KrakenExecType, KrakenLiquidityInd, KrakenWsOrderStatus},
            messages::KrakenWsExecutionData,
        },
    },
};
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{ClientOrderId, InstrumentId, Symbol},
    instruments::{Instrument, InstrumentAny, currency_pair::CurrencyPair},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;

const SPOT_SYMBOL: &str = "BTC/USDT";
const SPOT_INSTRUMENT_ID: &str = "BTC/USDT.KRAKEN";

fn make_spot_pair() -> InstrumentAny {
    InstrumentAny::CurrencyPair(CurrencyPair::new(
        InstrumentId::from(SPOT_INSTRUMENT_ID),
        Symbol::from(SPOT_SYMBOL),
        Currency::BTC(),
        Currency::from("USDT"),
        1,
        8,
        Price::from("0.1"),
        Quantity::from("0.00000001"),
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        None,
        UnixNanos::default(),
        UnixNanos::default(),
    ))
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
        order_qty: Some(0.0001),
        limit_price: Some(70_000.0),
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
        post_only: Some(true),
        reduce_only: Some(false),
        timestamp: "2026-04-11T00:00:00.000Z".parse().unwrap(),
        exec_id: exec_id.map(str::to_string),
        last_qty: exec_id.map(|_| 0.0001),
        last_price: exec_id.map(|_| 70_000.0),
        cost: exec_id.map(|_| 7.0),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
    let qty_cache = empty_f64_map();

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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
/// populated — every other field, including `symbol`, is `None`.
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
        limit_price: None,
        order_status: Some(order_status),
        cum_qty: None,
        cum_cost: None,
        avg_price: None,
        time_in_force: None,
        post_only: None,
        reduce_only: None,
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
    // skipped (existing behaviour, no panic, no event).
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
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
        &empty_f64_map(),
        account_id(),
        UnixNanos::default(),
    );
    assert!(state.lookup_order_symbol("v-spot-evict").is_none());
    assert!(state.lookup_order_client_id("v-spot-evict").is_none());
}
