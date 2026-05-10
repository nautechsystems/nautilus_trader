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

//! Integration tests for the Kraken Futures WebSocket execution dispatch.
//!
//! Validates the two-tier routing contract from
//! `docs/developer_guide/adapters.md` lines 1232-1296 for the futures product:
//! tracked orders (registered at submission via `OrderIdentity`) emit typed
//! [`OrderEventAny`] events; untracked / external orders fall back to
//! [`ExecutionReport`] variants.

mod common;

use std::sync::Arc;

use common::{
    account_id, drain_events, empty_instrument_id_map, empty_quantity_map, empty_string_map,
    make_identity, test_emitter,
};
use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{AtomicMap, UnixNanos};
use nautilus_kraken::{
    common::enums::{KrakenFillType, KrakenFuturesOrderType},
    websocket::{
        dispatch::{self, OrderIdentity, WsDispatchState},
        futures::messages::{
            KrakenFuturesFeed, KrakenFuturesFill, KrakenFuturesFillsDelta, KrakenFuturesOpenOrder,
            KrakenFuturesOpenOrdersCancel, KrakenFuturesOpenOrdersDelta,
        },
    },
};
use nautilus_model::{
    enums::{OrderSide, OrderType},
    events::OrderEventAny,
    identifiers::{ClientOrderId, InstrumentId, StrategyId, Symbol},
    instruments::{Instrument, InstrumentAny, crypto_perpetual::CryptoPerpetual},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use ustr::Ustr;

const FUTURES_PRODUCT: &str = "PF_XBTUSD";
const FUTURES_INSTRUMENT_ID: &str = "PF_XBTUSD.KRAKEN";

fn make_futures_perpetual() -> InstrumentAny {
    InstrumentAny::CryptoPerpetual(CryptoPerpetual::new(
        InstrumentId::from(FUTURES_INSTRUMENT_ID),
        Symbol::from(FUTURES_PRODUCT),
        Currency::BTC(),
        Currency::USD(),
        Currency::USD(),
        false,
        1,
        4,
        Price::from("0.5"),
        Quantity::from("0.0001"),
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

fn make_open_order(
    qty: f64,
    filled: f64,
    cli_ord_id: Option<&str>,
    venue_order_id: &str,
) -> KrakenFuturesOpenOrder {
    KrakenFuturesOpenOrder {
        instrument: Ustr::from(FUTURES_PRODUCT),
        time: 0,
        last_update_time: 0,
        qty,
        filled,
        limit_price: Some(70_000.0),
        stop_price: None,
        order_type: KrakenFuturesOrderType::Limit,
        order_id: venue_order_id.to_string(),
        cli_ord_id: cli_ord_id.map(str::to_string),
        direction: 0,
        reduce_only: false,
        trigger_signal: None,
    }
}

fn make_open_orders_delta(
    is_cancel: bool,
    reason: Option<&str>,
    qty: f64,
    filled: f64,
    cli_ord_id: Option<&str>,
    venue_order_id: &str,
) -> KrakenFuturesOpenOrdersDelta {
    KrakenFuturesOpenOrdersDelta {
        feed: KrakenFuturesFeed::OpenOrders,
        order: make_open_order(qty, filled, cli_ord_id, venue_order_id),
        is_cancel,
        reason: reason.map(str::to_string),
    }
}

fn make_open_orders_cancel(
    reason: Option<&str>,
    cli_ord_id: Option<&str>,
    venue_order_id: &str,
) -> KrakenFuturesOpenOrdersCancel {
    KrakenFuturesOpenOrdersCancel {
        feed: KrakenFuturesFeed::OpenOrders,
        order_id: venue_order_id.to_string(),
        cli_ord_id: cli_ord_id.map(str::to_string),
        is_cancel: true,
        reason: reason.map(str::to_string),
    }
}

fn make_fills_delta(
    cli_ord_id: Option<&str>,
    venue_order_id: &str,
    trade_id: &str,
) -> KrakenFuturesFillsDelta {
    KrakenFuturesFillsDelta {
        feed: KrakenFuturesFeed::Fills,
        username: None,
        fills: vec![KrakenFuturesFill {
            instrument: Some(Ustr::from(FUTURES_PRODUCT)),
            time: 0,
            price: 70_000.0,
            qty: 0.0001,
            order_id: venue_order_id.to_string(),
            cli_ord_id: cli_ord_id.map(str::to_string),
            fill_id: trade_id.to_string(),
            fill_type: KrakenFillType::Maker,
            buy: true,
            fee_paid: Some(0.05),
            fee_currency: Some("USD".to_string()),
        }],
    }
}

#[rstest]
fn test_futures_delta_tracked_emits_order_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-tracked-1");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let delta = make_open_orders_delta(false, None, 0.0001, 0.0, Some("uuid-tracked-1"), "v-1");
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(
        events[0],
        ExecutionEvent::Order(OrderEventAny::Accepted(_))
    ));
    assert!(state.emitted_accepted.contains(&cid));
}

#[rstest]
fn test_futures_delta_tracked_cancel_synthesizes_accepted_then_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-tracked-2");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    // Cancel arrives without a prior placement delta (fast cancel race).
    let delta = make_open_orders_delta(
        true,
        Some("cancelled_by_user"),
        0.0001,
        0.0,
        Some("uuid-tracked-2"),
        "v-2",
    );
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
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
    // Terminal cleanup removed the identity.
    assert!(state.lookup_identity(&cid).is_none());
}

#[rstest]
fn test_futures_delta_fill_driven_cancel_is_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    state.register_identity(
        ClientOrderId::new("uuid-tracked-3"),
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let delta = make_open_orders_delta(
        true,
        Some("full_fill"),
        0.0,
        0.0001,
        Some("uuid-tracked-3"),
        "v-3",
    );
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert!(
        events.is_empty(),
        "fill-driven cancel should not emit events"
    );
}

#[rstest]
fn test_futures_fills_delta_tracked_emits_filled_after_synthesized_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-tracked-4");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let fills = make_fills_delta(Some("uuid-tracked-4"), "v-4", "trade-4");
    dispatch::futures::fills_delta(
        &fills,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_string_map(),
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
        ExecutionEvent::Order(OrderEventAny::Filled(_))
    ));
}

#[rstest]
fn test_futures_fills_delta_external_emits_fill_report() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let fills = make_fills_delta(None, "v-external", "trade-external");
    dispatch::futures::fills_delta(
        &fills,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_string_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_futures_fills_delta_dedup_skips_duplicate_trade_id() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    state.register_identity(
        ClientOrderId::new("uuid-tracked-5"),
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let fills = make_fills_delta(Some("uuid-tracked-5"), "v-5", "trade-dup");
    let instruments = instruments_with(make_futures_perpetual());
    let truncated = empty_string_map();
    let venue_client = empty_string_map();

    // First dispatch — accepted + filled.
    dispatch::futures::fills_delta(
        &fills,
        &state,
        &emitter,
        &instruments,
        &truncated,
        &venue_client,
        account_id(),
        UnixNanos::default(),
    );
    // Second dispatch with the same trade id — must be deduped.
    dispatch::futures::fills_delta(
        &fills,
        &state,
        &emitter,
        &instruments,
        &truncated,
        &venue_client,
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(
        events.len(),
        2,
        "second fill should be deduped, not re-emitted",
    );
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
fn test_futures_open_orders_cancel_tracked_via_venue_client_map() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-tracked-6");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    // Simulate a prior delta having populated venue_client_map for v-6 -> cid.
    let venue_client = empty_string_map();
    venue_client.insert("v-6".to_string(), cid);

    let cancel = make_open_orders_cancel(Some("cancelled_by_user"), None, "v-6");
    dispatch::futures::open_orders_cancel(
        &cancel,
        &state,
        &emitter,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &venue_client,
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    // Synthesized Accepted then Canceled.
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
fn test_futures_open_orders_cancel_external_falls_back_to_report() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    // Side caches populated by an earlier delta on the wire (the order is
    // not registered as one we submitted).
    let order_instrument_map = empty_instrument_id_map();
    order_instrument_map.insert(
        "v-ext".to_string(),
        InstrumentId::from(FUTURES_INSTRUMENT_ID),
    );
    let venue_order_qty = empty_quantity_map();
    venue_order_qty.insert("v-ext".to_string(), Quantity::new(0.0001, 4));

    let cancel = make_open_orders_cancel(Some("cancelled_by_user"), None, "v-ext");
    dispatch::futures::open_orders_cancel(
        &cancel,
        &state,
        &emitter,
        &empty_string_map(),
        &order_instrument_map,
        &empty_string_map(),
        &venue_order_qty,
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_futures_open_orders_cancel_fill_driven_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    state.register_identity(
        ClientOrderId::new("uuid-tracked-7"),
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );

    let cancel = make_open_orders_cancel(Some("partial_fill"), Some("uuid-tracked-7"), "v-7");
    dispatch::futures::open_orders_cancel(
        &cancel,
        &state,
        &emitter,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_futures_delta_external_emits_status_report() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let delta = make_open_orders_delta(false, None, 0.0001, 0.0, None, "v-ext-2");
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    assert!(matches!(events[0], ExecutionEvent::Report(_)));
}

#[rstest]
fn test_futures_delta_modify_ack_emits_order_updated() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-modify-ack");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_futures_perpetual());

    // Initial placement at 70_000.
    let placement = make_open_orders_delta(
        false,
        Some("new_placed_order_by_user"),
        0.0001,
        0.0,
        Some("uuid-modify-ack"),
        "v-modify",
    );
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Follow-up delta with the limit price moved to 71_000 (genuine modify).
    let mut amended = make_open_orders_delta(
        false,
        None,
        0.0001,
        0.0,
        Some("uuid-modify-ack"),
        "v-modify",
    );
    amended.order.limit_price = Some(71_000.0);

    dispatch::futures::open_orders_delta(
        &amended,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
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
fn test_futures_delta_no_op_repeat_does_not_emit_updated() {
    // Two identical deltas — nothing changed — must not emit OrderUpdated.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-noop");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_futures_perpetual());

    let placement = make_open_orders_delta(
        false,
        Some("new_placed_order_by_user"),
        0.0001,
        0.0,
        Some("uuid-noop"),
        "v-noop",
    );
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Identical second delta.
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert!(
        events.is_empty(),
        "no-op delta should not emit; saw {events:?}"
    );
}

#[rstest]
fn test_futures_delta_partial_fill_does_not_emit_updated() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-partial-fill");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_futures_perpetual());

    // Placement.
    let placement = make_open_orders_delta(
        false,
        None,
        0.0002,
        0.0,
        Some("uuid-partial-fill"),
        "v-partial",
    );
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Follow-up delta announcing a partial fill (filled > previous).
    let partial = make_open_orders_delta(
        false,
        None,
        0.0002,
        0.0001,
        Some("uuid-partial-fill"),
        "v-partial",
    );
    dispatch::futures::open_orders_delta(
        &partial,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert!(
        events.is_empty(),
        "partial-fill delta should not emit OrderUpdated; saw {events:?}",
    );
}

#[rstest]
fn test_futures_fills_delta_terminal_cleanup_on_full_fill() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-terminal");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let instruments = instruments_with(make_futures_perpetual());

    let fills = make_fills_delta(Some("uuid-terminal"), "v-term", "trade-term");
    dispatch::futures::fills_delta(
        &fills,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_string_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // After full fill: identity removed, filled_orders set.
    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
    assert!(state.previous_filled_qty(&cid).is_none());
}

#[rstest]
fn test_futures_delta_stale_after_terminal_is_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-stale");
    // Pretend the order was filled and cleaned up earlier.
    state.insert_filled(cid);

    let delta = make_open_orders_delta(
        false,
        Some("status_update"),
        0.0001,
        0.0001,
        Some("uuid-stale"),
        "v-stale",
    );
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_futures_partial_fill_does_not_double_count_via_delta_and_fill() {
    // A 0.5/1.0 partial fill that arrives as both an OpenOrdersDelta
    // (filled=0.5) and a FillsDelta (last_qty=0.5) must result in cumulative
    // 0.5, not 1.0. Otherwise the order would be marked terminal early and
    // remaining fills would be stale-suppressed.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-no-double");
    state.register_identity(
        cid,
        OrderIdentity {
            strategy_id: StrategyId::from("EXEC_TESTER-001"),
            instrument_id: InstrumentId::from(FUTURES_INSTRUMENT_ID),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: Quantity::from("0.001"),
        },
    );
    let instruments = instruments_with(make_futures_perpetual());

    // Placement delta — filled=0
    let placement = make_open_orders_delta(
        false,
        Some("new_placed_order_by_user"),
        0.001,
        0.0,
        Some("uuid-no-double"),
        "v-no-double",
    );
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    // Partial-fill delta — filled=0.0005
    let partial = make_open_orders_delta(
        false,
        None,
        0.001,
        0.0005,
        Some("uuid-no-double"),
        "v-no-double",
    );
    dispatch::futures::open_orders_delta(
        &partial,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    // Matching FillsDelta — last_qty=0.0005 (the same fill)
    let fill = KrakenFuturesFillsDelta {
        feed: KrakenFuturesFeed::Fills,
        username: None,
        fills: vec![KrakenFuturesFill {
            instrument: Some(Ustr::from(FUTURES_PRODUCT)),
            time: 0,
            price: 70_000.0,
            qty: 0.0005,
            order_id: "v-no-double".to_string(),
            cli_ord_id: Some("uuid-no-double".to_string()),
            fill_id: "trade-half".to_string(),
            fill_type: KrakenFillType::Maker,
            buy: true,
            fee_paid: Some(0.0),
            fee_currency: Some("USD".to_string()),
        }],
    };
    dispatch::futures::fills_delta(
        &fill,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_string_map(),
        account_id(),
        UnixNanos::default(),
    );

    // Drain placement Accepted (1) + the fill OrderFilled (1).
    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 2);

    // Identity must still exist (order is half-filled, not terminal).
    assert!(
        state.lookup_identity(&cid).is_some(),
        "identity should not be cleaned up after a half fill",
    );
    assert!(
        !state.filled_orders.contains(&cid),
        "filled_orders should not contain a partially-filled order",
    );

    // The fill side recorded cumulative = 0.0005 (not 0.0010).
    let cumulative = state.previous_filled_qty(&cid).expect("cumulative tracked");
    assert_eq!(cumulative, Quantity::from("0.0005"));
}

#[rstest]
fn test_futures_modify_ack_refreshes_tracked_quantity() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("uuid-modify-qty");
    state.register_identity(
        cid,
        OrderIdentity {
            strategy_id: StrategyId::from("EXEC_TESTER-001"),
            instrument_id: InstrumentId::from(FUTURES_INSTRUMENT_ID),
            order_side: OrderSide::Buy,
            order_type: OrderType::Limit,
            quantity: Quantity::from("0.001"),
        },
    );
    let instruments = instruments_with(make_futures_perpetual());

    // Placement at qty=0.001
    let placement = make_open_orders_delta(
        false,
        None,
        0.001,
        0.0,
        Some("uuid-modify-qty"),
        "v-modify-qty",
    );
    dispatch::futures::open_orders_delta(
        &placement,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Modify ack: qty grew to 0.002
    let modify = make_open_orders_delta(
        false,
        None,
        0.002,
        0.0,
        Some("uuid-modify-qty"),
        "v-modify-qty",
    );
    dispatch::futures::open_orders_delta(
        &modify,
        &state,
        &emitter,
        &instruments,
        &empty_string_map(),
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );
    let _ = drain_events(&mut rx);

    // Identity now reflects the new size.
    let updated_identity = state.lookup_identity(&cid).expect("identity present");
    assert_eq!(updated_identity.quantity, Quantity::from("0.002"));
}

#[rstest]
fn test_cleanup_terminal_clears_all_dispatch_state() {
    // Cleanup is what the rejection paths in submit_single_order /
    // submit_order_list call when a REST submit fails — the order will
    // never appear on the wire so the dispatch entry must not leak.
    let state = WsDispatchState::new();
    let cid = ClientOrderId::new("uuid-rejected");
    state.register_identity(
        cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    state.insert_accepted(cid);
    state.record_filled_qty(cid, Quantity::from("0.0001"));

    state.cleanup_terminal(&cid);

    assert!(state.lookup_identity(&cid).is_none());
    assert!(!state.emitted_accepted.contains(&cid));
    assert!(state.previous_filled_qty(&cid).is_none());
}

#[rstest]
fn test_truncated_id_map_resolves_full_client_order_id() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let full_cid = ClientOrderId::new("full-uuid-aaaaaaaaaaaaaaaaaaaa");
    state.register_identity(
        full_cid,
        make_identity(FUTURES_INSTRUMENT_ID, OrderSide::Buy, OrderType::Limit),
    );
    let truncated_id_map = empty_string_map();
    truncated_id_map.insert("trunc-aaa".to_string(), full_cid);

    // The wire message carries the truncated id; dispatch must resolve it
    // to the full id and find the registered identity.
    let delta = make_open_orders_delta(false, None, 0.0001, 0.0, Some("trunc-aaa"), "v-trunc");
    dispatch::futures::open_orders_delta(
        &delta,
        &state,
        &emitter,
        &instruments_with(make_futures_perpetual()),
        &truncated_id_map,
        &empty_instrument_id_map(),
        &empty_string_map(),
        &empty_quantity_map(),
        account_id(),
        UnixNanos::default(),
    );

    let events = drain_events(&mut rx);
    assert_eq!(events.len(), 1);
    let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = &events[0] else {
        panic!("expected OrderAccepted, was {:?}", events[0]);
    };
    assert_eq!(accepted.client_order_id, full_cid);
}
