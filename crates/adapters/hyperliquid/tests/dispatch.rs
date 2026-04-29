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

//! Integration tests for the Hyperliquid WebSocket execution dispatch.
//!
//! Covers the two-tier routing contract from
//! `docs/developer_guide/adapters.md` lines 1232-1296 plus the GH-3827
//! cancel-replace handling:
//!
//! * Tracked orders emit typed [`OrderEventAny`] events (`OrderAccepted`,
//!   `OrderCanceled`, `OrderUpdated`, `OrderFilled`, `OrderExpired`,
//!   `OrderRejected`, `OrderTriggered`).
//! * External / untracked orders fall through to
//!   [`DispatchOutcome::External`] so the caller can forward the raw
//!   [`OrderStatusReport`] / [`FillReport`].
//! * Stale / race legs (replay, cancel-before-accept, cancel leg of a
//!   cancel-replace) return [`DispatchOutcome::Skip`].

use std::sync::Arc;

use nautilus_common::messages::ExecutionEvent;
use nautilus_core::{UUID4, UnixNanos, time::get_atomic_clock_realtime};
use nautilus_hyperliquid::websocket::dispatch::{
    DispatchOutcome, OrderIdentity, WsDispatchState, dispatch_fill_report,
    dispatch_order_status_report,
};
use nautilus_live::ExecutionEventEmitter;
use nautilus_model::{
    enums::{
        AccountType, LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce, TriggerType,
    },
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    reports::{FillReport, OrderStatusReport},
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;

const INSTRUMENT_ID: &str = "BTC-USD-PERP.HYPERLIQUID";

fn test_emitter() -> (
    ExecutionEventEmitter,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) {
    let clock = get_atomic_clock_realtime();
    let mut emitter = ExecutionEventEmitter::new(
        clock,
        TraderId::from("TESTER-001"),
        account_id(),
        AccountType::Margin,
        None,
    );
    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    emitter.set_sender(tx);
    (emitter, rx)
}

fn drain_events(
    rx: &mut tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
) -> Vec<ExecutionEvent> {
    let mut events = Vec::new();
    while let Ok(e) = rx.try_recv() {
        events.push(e);
    }
    events
}

fn account_id() -> AccountId {
    AccountId::from("HYPERLIQUID-001")
}

fn identity(order_type: OrderType) -> OrderIdentity {
    OrderIdentity {
        strategy_id: StrategyId::from("S-001"),
        instrument_id: InstrumentId::from(INSTRUMENT_ID),
        order_side: OrderSide::Buy,
        order_type,
        quantity: Quantity::from("0.00020"),
        price: Some(Price::from("56730.0")),
    }
}

fn make_status_report(
    client_order_id: Option<&str>,
    venue_order_id: &str,
    status: OrderStatus,
    price: Option<&str>,
    quantity: &str,
) -> OrderStatusReport {
    let mut report = OrderStatusReport::new(
        account_id(),
        InstrumentId::from(INSTRUMENT_ID),
        client_order_id.map(ClientOrderId::new),
        VenueOrderId::new(venue_order_id),
        OrderSide::Buy,
        OrderType::Limit,
        TimeInForce::Gtc,
        status,
        Quantity::from(quantity),
        Quantity::from("0"),
        UnixNanos::default(),
        UnixNanos::default(),
        UnixNanos::default(),
        Some(UUID4::new()),
    );

    if let Some(px) = price {
        report = report.with_price(Price::from(px));
    }

    report
}

fn make_fill_report(
    client_order_id: Option<&str>,
    venue_order_id: &str,
    trade_id: &str,
    last_qty: &str,
    last_px: &str,
) -> FillReport {
    FillReport::new(
        account_id(),
        InstrumentId::from(INSTRUMENT_ID),
        VenueOrderId::new(venue_order_id),
        TradeId::new(trade_id),
        OrderSide::Buy,
        Quantity::from(last_qty),
        Price::from(last_px),
        Money::new(0.0, Currency::USD()),
        LiquiditySide::Taker,
        client_order_id.map(ClientOrderId::new),
        None,
        UnixNanos::default(),
        UnixNanos::default(),
        Some(UUID4::new()),
    )
}

fn assert_event_types(events: &[ExecutionEvent], expected: &[&str]) {
    let kinds: Vec<&str> = events
        .iter()
        .map(|e| match e {
            ExecutionEvent::Order(OrderEventAny::Accepted(_)) => "Accepted",
            ExecutionEvent::Order(OrderEventAny::Canceled(_)) => "Canceled",
            ExecutionEvent::Order(OrderEventAny::Updated(_)) => "Updated",
            ExecutionEvent::Order(OrderEventAny::Filled(_)) => "Filled",
            ExecutionEvent::Order(OrderEventAny::Expired(_)) => "Expired",
            ExecutionEvent::Order(OrderEventAny::Rejected(_)) => "Rejected",
            ExecutionEvent::Order(OrderEventAny::Triggered(_)) => "Triggered",
            ExecutionEvent::Order(_) => "OtherOrder",
            ExecutionEvent::OrderSubmittedBatch(_) => "OrderSubmittedBatch",
            ExecutionEvent::OrderAcceptedBatch(_) => "OrderAcceptedBatch",
            ExecutionEvent::OrderCanceledBatch(_) => "OrderCanceledBatch",
            ExecutionEvent::Report(_) => "Report",
            ExecutionEvent::Account(_) => "Account",
        })
        .collect();
    assert_eq!(
        kinds, expected,
        "event sequence mismatch: got {kinds:?}, expected {expected:?}",
    );
}

#[rstest]
fn test_dispatch_accepted_tracked_emits_order_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-001");
    state.register_identity(cid, identity(OrderType::Limit));

    let report = make_status_report(
        Some("O-001"),
        "v-100",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    assert_eq!(outcome, DispatchOutcome::Tracked);
    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted"]);
    assert_eq!(
        state.cached_venue_order_id(&cid),
        Some(VenueOrderId::new("v-100")),
    );
}

#[rstest]
fn test_dispatch_accepted_external_falls_back() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let report = make_status_report(
        Some("EXT-001"),
        "v-200",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    assert_eq!(outcome, DispatchOutcome::External);
    let events = drain_events(&mut rx);
    // External path emits nothing from dispatch; the caller forwards the report.
    assert!(events.is_empty(), "expected no dispatch-emitted events");
}

#[rstest]
fn test_dispatch_canceled_tracked_synthesizes_accepted_then_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-002");
    state.register_identity(cid, identity(OrderType::Limit));

    let report = make_status_report(
        Some("O-002"),
        "v-200",
        OrderStatus::Canceled,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    assert_eq!(outcome, DispatchOutcome::Tracked);
    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted", "Canceled"]);
    // Terminal cleanup retains the filled-orders marker.
    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_dispatch_expired_tracked_synthesizes_accepted_then_expired() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-003");
    state.register_identity(cid, identity(OrderType::Limit));

    let report = make_status_report(
        Some("O-003"),
        "v-300",
        OrderStatus::Expired,
        Some("56730.0"),
        "0.00020",
    );
    dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted", "Expired"]);
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_dispatch_rejected_tracked_emits_rejected_and_cleans_up() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-004");
    state.register_identity(cid, identity(OrderType::Limit));

    let mut report = make_status_report(
        Some("O-004"),
        "v-400",
        OrderStatus::Rejected,
        Some("56730.0"),
        "0.00020",
    );
    report = report.with_cancel_reason("Insufficient margin".to_string());
    dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Rejected"]);
    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
#[case::stop_limit(OrderType::StopLimit, &["Accepted", "Triggered"])]
#[case::trailing_stop_limit(OrderType::TrailingStopLimit, &["Accepted", "Triggered"])]
#[case::limit_if_touched(OrderType::LimitIfTouched, &["Accepted", "Triggered"])]
#[case::plain_limit_is_ignored(OrderType::Limit, &[])]
fn test_dispatch_triggered_per_order_type(
    #[case] order_type: OrderType,
    #[case] expected: &[&str],
) {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-005");
    state.register_identity(cid, identity(order_type));

    let mut report = make_status_report(
        Some("O-005"),
        "v-500",
        OrderStatus::Triggered,
        Some("56730.0"),
        "0.00020",
    );
    report = report.with_trigger_price(Price::from("56700.0"));
    report.trigger_type = Some(TriggerType::LastPrice);
    dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, expected);
}

#[rstest]
fn test_dispatch_fill_tracked_synthesizes_accepted_then_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-007");
    state.register_identity(cid, identity(OrderType::Limit));

    let fill = make_fill_report(Some("O-007"), "v-700", "trade-1", "0.00020", "56730.0");
    let outcome = dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());

    assert_eq!(outcome, DispatchOutcome::Tracked);
    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted", "Filled"]);

    // Commission currency on the emitted OrderFilled must come from the
    // FillReport so the engine books the fee in the instrument's settlement
    // currency rather than defaulting elsewhere.
    if let ExecutionEvent::Order(OrderEventAny::Filled(filled)) = &events[1] {
        assert_eq!(filled.currency, Currency::USD());
        assert_eq!(filled.last_qty, Quantity::from("0.00020"));
        assert_eq!(filled.last_px, Price::from("56730.0"));
        assert_eq!(filled.venue_order_id, VenueOrderId::new("v-700"));
    } else {
        panic!("expected OrderEventAny::Filled at index 1");
    }

    // Terminal fill cleans identity and records filled marker.
    assert!(state.lookup_identity(&cid).is_none());
    assert!(state.filled_orders.contains(&cid));
}

#[rstest]
fn test_dispatch_fill_tracked_partial_then_terminal() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-008");
    state.register_identity(cid, identity(OrderType::Limit));

    let partial = make_fill_report(Some("O-008"), "v-800", "t-p1", "0.00010", "56730.0");
    let remainder = make_fill_report(Some("O-008"), "v-800", "t-p2", "0.00010", "56730.0");

    dispatch_fill_report(&partial, &state, &emitter, UnixNanos::default());
    dispatch_fill_report(&remainder, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted", "Filled", "Filled"]);
    assert!(state.filled_orders.contains(&cid));
    assert!(state.lookup_identity(&cid).is_none());
}

#[rstest]
fn test_dispatch_fill_duplicate_trade_id_is_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-009");
    // Two submissions with same quantity 0.00040 so the first fill is non-terminal.
    state.register_identity(
        cid,
        OrderIdentity {
            quantity: Quantity::from("0.00040"),
            ..identity(OrderType::Limit)
        },
    );

    let fill = make_fill_report(Some("O-009"), "v-900", "trade-dup", "0.00010", "56730.0");

    dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());
    // Second dispatch of same trade_id is deduped.
    let outcome = dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::Tracked);

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted", "Filled"]);
}

#[rstest]
fn test_dispatch_fill_external_falls_back() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let fill = make_fill_report(None, "v-ext", "trade-ext", "0.00020", "56730.0");
    let outcome = dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::External);
    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_dispatch_stale_replay_after_terminal_is_skipped() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-010");
    state.insert_filled(cid);
    state.register_identity(cid, identity(OrderType::Limit));

    let report = make_status_report(
        Some("O-010"),
        "v-1000",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::Skip);
    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_dispatch_fill_for_order_in_filled_orders_is_skipped() {
    // A late fill arriving for an order whose cid is already recorded in
    // `filled_orders` (e.g. the terminal canceled/expired/rejected path ran
    // first) must be suppressed rather than emitted: the identity has been
    // cleaned up and the engine has already observed the terminal event.
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-010a");
    state.insert_filled(cid);

    let fill = make_fill_report(
        Some("O-010a"),
        "v-1010",
        "trade-stale",
        "0.00020",
        "56730.0",
    );
    let outcome = dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());

    assert_eq!(outcome, DispatchOutcome::Skip);
    assert!(drain_events(&mut rx).is_empty());
}

/// GH-3827: `ACCEPTED(new_voi)` followed by `CANCELED(old_voi)` under the
/// same `client_order_id` must emit a single `OrderUpdated` (with the new
/// venue order id) and suppress the stale cancel.
#[rstest]
fn test_cancel_replace_emits_updated_not_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-CR-001");
    state.register_identity(cid, identity(OrderType::Limit));

    // Prime state as if the first ACCEPTED had already flowed through.
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("375273671786"));

    let accepted_new = make_status_report(
        Some("O-CR-001"),
        "375273716474",
        OrderStatus::Accepted,
        Some("53893.0"),
        "0.00020",
    );
    let canceled_old = make_status_report(
        Some("O-CR-001"),
        "375273671786",
        OrderStatus::Canceled,
        Some("56730.0"),
        "0.00020",
    );

    dispatch_order_status_report(&accepted_new, &state, &emitter, UnixNanos::default());
    let canceled_outcome =
        dispatch_order_status_report(&canceled_old, &state, &emitter, UnixNanos::default());

    assert_eq!(canceled_outcome, DispatchOutcome::Skip);

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Updated"]);

    // The cached venue_order_id has advanced to the replacement leg.
    assert_eq!(
        state.cached_venue_order_id(&cid),
        Some(VenueOrderId::new("375273716474")),
    );
    // Identity is still tracked (the order was not terminal).
    assert!(state.lookup_identity(&cid).is_some());
    // No stale `filled_orders` marker was written for the order.
    assert!(!state.filled_orders.contains(&cid));

    if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = &events[0] {
        assert_eq!(
            updated.venue_order_id,
            Some(VenueOrderId::new("375273716474"))
        );
        assert_eq!(updated.price, Some(Price::from("53893.0")));
        assert_eq!(updated.quantity, Quantity::from("0.00020"));
    } else {
        panic!("expected OrderEventAny::Updated");
    }
}

/// GH-3827: an `ACCEPTED(new_voi)` that omits `price` must fall back to the
/// cached `OrderIdentity::price` so the emitted `OrderUpdated` still carries
/// an accurate price. If neither the report nor the identity carries a price
/// the dispatch skips the leg rather than emitting an `OrderUpdated` with
/// `None`.
#[rstest]
#[case::report_has_price(
    Some("53893.0"),
    Some(Price::from("56730.0")),
    Some("Updated"),
    Some(Price::from("53893.0"))
)]
#[case::identity_fallback(
    None,
    Some(Price::from("56730.0")),
    Some("Updated"),
    Some(Price::from("56730.0"))
)]
#[case::both_missing_is_skipped(None, None, None, None)]
fn test_cancel_replace_price_sources(
    #[case] report_price: Option<&str>,
    #[case] identity_price: Option<Price>,
    #[case] expected_event: Option<&str>,
    #[case] expected_updated_price: Option<Price>,
) {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-CR-002");
    state.register_identity(
        cid,
        OrderIdentity {
            price: identity_price,
            ..identity(OrderType::Limit)
        },
    );
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("5000"));

    let accepted_new = make_status_report(
        Some("O-CR-002"),
        "5001",
        OrderStatus::Accepted,
        report_price,
        "0.00020",
    );
    let outcome =
        dispatch_order_status_report(&accepted_new, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);

    match expected_event {
        Some(kind) => {
            assert_eq!(outcome, DispatchOutcome::Tracked);
            assert_event_types(&events, &[kind]);
            if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = &events[0] {
                assert_eq!(updated.venue_order_id, Some(VenueOrderId::new("5001")));
                assert_eq!(updated.price, expected_updated_price);
            } else {
                panic!("expected OrderEventAny::Updated");
            }
        }
        None => {
            // No price anywhere: dispatch must skip rather than emit a bogus Updated.
            assert_eq!(outcome, DispatchOutcome::Skip);
            assert_event_types(&events, &[]);
            // The cached venue_order_id is not advanced on skip, so later
            // events for the old leg still match.
            assert_eq!(
                state.cached_venue_order_id(&cid),
                Some(VenueOrderId::new("5000"))
            );
        }
    }
}

/// GH-3827: a modify that completed via WS (cached venue_order_id already
/// advanced) emits `OrderUpdated` even if the modify HTTP call itself
/// failed with a transport error.
#[rstest]
fn test_cancel_replace_recovers_after_timed_out_modify() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-CR-003");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("2222"));

    // Transport-timeout path: no pending marker is set. When the replacement
    // ACCEPTED arrives via WS, the cached-voi mismatch alone drives the
    // OrderUpdated promotion.
    let accepted_new = make_status_report(
        Some("O-CR-003"),
        "3333",
        OrderStatus::Accepted,
        Some("53893.0"),
        "0.00020",
    );
    dispatch_order_status_report(&accepted_new, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Updated"]);
    if let ExecutionEvent::Order(OrderEventAny::Updated(updated)) = &events[0] {
        assert_eq!(updated.venue_order_id, Some(VenueOrderId::new("3333")));
    }
    assert_eq!(
        state.cached_venue_order_id(&cid),
        Some(VenueOrderId::new("3333"))
    );
}

/// GH-3827: a `CANCELED(old_voi)` arriving before the replacement
/// `ACCEPTED(new_voi)` is suppressed via the pending-modify marker so the
/// later ACCEPTED still routes through the `OrderUpdated` path.
#[rstest]
fn test_cancel_before_accept_is_suppressed() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-CR-004");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("1000"));

    // Successful modify HTTP round-trip populated the pending marker.
    state.mark_pending_modify(cid, VenueOrderId::new("1000"));

    let canceled_old = make_status_report(
        Some("O-CR-004"),
        "1000",
        OrderStatus::Canceled,
        Some("56730.0"),
        "0.00020",
    );
    let cancel_outcome =
        dispatch_order_status_report(&canceled_old, &state, &emitter, UnixNanos::default());
    assert_eq!(cancel_outcome, DispatchOutcome::Skip);

    let accepted_new = make_status_report(
        Some("O-CR-004"),
        "2000",
        OrderStatus::Accepted,
        Some("53893.0"),
        "0.00020",
    );
    dispatch_order_status_report(&accepted_new, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Updated"]);
    // Pending marker cleared on the replacement ACCEPTED; tracked state alive.
    assert!(state.pending_modify(&cid).is_none());
    assert!(state.lookup_identity(&cid).is_some());
    assert_eq!(
        state.cached_venue_order_id(&cid),
        Some(VenueOrderId::new("2000"))
    );
}

/// GH-3827: when a modify failed at HTTP level, no pending marker is set, so
/// a later cancel for the unchanged order still emits `OrderCanceled`.
#[rstest]
fn test_cancel_after_failed_modify_still_emits_canceled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-CR-005");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("9999"));
    // Intentionally no `mark_pending_modify`: the failed modify leaves no
    // state behind.

    let canceled = make_status_report(
        Some("O-CR-005"),
        "9999",
        OrderStatus::Canceled,
        Some("56730.0"),
        "0.00020",
    );
    dispatch_order_status_report(&canceled, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Canceled"]);
    assert!(state.filled_orders.contains(&cid));
    assert!(state.lookup_identity(&cid).is_none());
}

#[rstest]
fn test_partial_fill_status_emits_nothing_from_status_path() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-011");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);

    let report = make_status_report(
        Some("O-011"),
        "v-1100",
        OrderStatus::PartiallyFilled,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::Tracked);
    assert!(drain_events(&mut rx).is_empty());
}

#[rstest]
fn test_filled_status_marker_is_noop_without_fill() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-012");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("v-1200"));

    let report = make_status_report(
        Some("O-012"),
        "v-1200",
        OrderStatus::Filled,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::Tracked);

    // No events from the status-only marker; the fill side emits the actual
    // `OrderFilled` when the matching trade arrives.
    assert!(drain_events(&mut rx).is_empty());

    // `filled_orders` must NOT be set here, otherwise the follow-up fill
    // would be classified as a stale replay and dropped before it can
    // emit OrderFilled.
    assert!(!state.filled_orders.contains(&cid));
    assert!(state.lookup_identity(&cid).is_some());
}

#[rstest]
fn test_filled_status_marker_then_fill_emits_filled() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-012a");
    state.register_identity(cid, identity(OrderType::Limit));
    state.insert_accepted(cid);
    state.record_venue_order_id(cid, VenueOrderId::new("v-1210"));

    let status = make_status_report(
        Some("O-012a"),
        "v-1210",
        OrderStatus::Filled,
        Some("56730.0"),
        "0.00020",
    );
    dispatch_order_status_report(&status, &state, &emitter, UnixNanos::default());

    // Status-only marker arrived first; the real fill must still be routed.
    let fill = make_fill_report(Some("O-012a"), "v-1210", "trade-012a", "0.00020", "56730.0");
    let outcome = dispatch_fill_report(&fill, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::Tracked);

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Filled"]);
    assert!(state.filled_orders.contains(&cid));
    assert!(state.lookup_identity(&cid).is_none());
}

#[rstest]
fn test_accepted_dedup_skips_second_accepted() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());
    let cid = ClientOrderId::new("O-013");
    state.register_identity(cid, identity(OrderType::Limit));

    let first = make_status_report(
        Some("O-013"),
        "v-1300",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    let second = make_status_report(
        Some("O-013"),
        "v-1300",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    dispatch_order_status_report(&first, &state, &emitter, UnixNanos::default());
    dispatch_order_status_report(&second, &state, &emitter, UnixNanos::default());

    let events = drain_events(&mut rx);
    assert_event_types(&events, &["Accepted"]);
}

#[rstest]
fn test_report_without_client_order_id_is_external() {
    let (emitter, mut rx) = test_emitter();
    let state = Arc::new(WsDispatchState::new());

    let report = make_status_report(
        None,
        "v-1400",
        OrderStatus::Accepted,
        Some("56730.0"),
        "0.00020",
    );
    let outcome = dispatch_order_status_report(&report, &state, &emitter, UnixNanos::default());
    assert_eq!(outcome, DispatchOutcome::External);
    assert!(drain_events(&mut rx).is_empty());
}
