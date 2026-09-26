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

//! End-to-end seam tests: strategy commands -> risk engine -> execution engine ->
//! `PolymarketExecutionClient` -> mock venue -> live execution routing -> cache.

use std::{cell::RefCell, rc::Rc, time::Duration};

use axum::http::StatusCode;
use nautilus_common::{
    actor::DataActor,
    cache::Cache,
    msgbus::{self, TypedHandler},
    testing::wait_until_async,
};
use nautilus_core::UnixNanos;
use nautilus_live::{SocketReconnectRequestOutcome, testing::ExecutionHarness};
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, StrategyId, TradeId, TraderId, VenueOrderId,
    },
    orders::{Order, OrderAny, OrderTestBuilder, stubs::TestOrderEventStubs},
    reports::ExecutionMassStatus,
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::{Value, json};
use ustr::Ustr;

use crate::{
    harness,
    mock_venue::{DEFAULT_ACCEPTED_ORDER_ID, load_json},
};

const DEADLINE: Duration = Duration::from_secs(5);
const UNRESOLVED_MASS_STATUS_ERROR: &str = "cannot generate mass status: Polymarket settlement \
                                            registry holds 1 record(s) with unresolved evidence";
// Matches the user stream endpoint name the execution client registers
const USER_STREAMS_ENDPOINT: &str = "polymarket-user-streams";

fn order_reached(cache: &Cache, order: &OrderAny, status: OrderStatus) -> bool {
    cache
        .order(&order.client_order_id())
        .is_some_and(|cached| cached.status() == status)
}

fn event_count(order: &OrderAny, predicate: impl Fn(&OrderEventAny) -> bool) -> usize {
    order
        .events()
        .iter()
        .filter(|event| predicate(event))
        .count()
}

#[rstest]
#[tokio::test]
async fn harness_builds_and_connects() {
    let h = harness::Harness::build().await;

    h.assert_engine_ready();
    assert!(h.cache().borrow().instrument(&h.instrument_id()).is_some());
    assert_eq!(
        h.mock_state
            .user_socket_count
            .load(std::sync::atomic::Ordering::Acquire),
        1,
    );
}

#[rstest]
#[tokio::test]
async fn submit_routes_to_accepted_in_cache() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;

    let status = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .map(|cached| cached.status());
    let post_count = *h.mock_state.order_post_count.lock().await;
    assert!(
        accepted,
        "order did not reach Accepted: status={status:?}, routed={:?}, order_posts={post_count}",
        h.routed(),
    );
    harness::invariants::assert_tracked_used_events(h.routed());
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Accepted,
    );
    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(h.risk_command_count(), 1);
    assert_eq!(
        cached.venue_order_id(),
        Some(VenueOrderId::from(
            crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID
        )),
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        true,
    );
    harness::invariants::assert_own_book_consistent(&h.cache().borrow(), &h.instrument_id());
}

#[rstest]
#[tokio::test]
async fn exec_tester_drives_submit_to_accepted() {
    let mut h = harness::Harness::build().await;
    let instrument_id = h.instrument_id();
    let mut tester = h.register_exec_tester(
        StrategyId::from(harness::STRATEGY_ID),
        Quantity::from("100.0000"),
    );

    tester.on_start().unwrap();
    tester.on_quote(&harness::quote(instrument_id)).unwrap();

    let accepted = h
        .pump_until(DEADLINE, |cache| {
            cache
                .orders(None, Some(&instrument_id), None, None, None)
                .iter()
                .any(|order| order.status() == OrderStatus::Accepted)
        })
        .await;

    assert!(accepted, "ExecTester-driven order did not reach Accepted");
    assert_eq!(h.risk_command_count(), 1);
    harness::invariants::assert_tracked_used_events(h.routed());
}

#[rstest]
#[tokio::test]
async fn tracked_cancel_emits_event_and_shrinks_own_book() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );

    h.cancel_via_execution(&order);
    let state = h.mock_state.clone();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        DEADLINE,
    )
    .await;
    h.mock_state
        .feed_user("ws_user_order_cancellation.json")
        .await;
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;

    assert!(canceled, "order did not reach Canceled");
    assert_eq!(h.risk_command_count(), 1);
    harness::invariants::assert_tracked_used_events(h.routed());
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Canceled,
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache().borrow(), &h.instrument_id());
}

#[rstest]
#[tokio::test]
async fn tracked_full_fill_emits_event_and_closes() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );

    h.mock_state.feed_user("ws_user_trade_full.json").await;
    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    assert!(filled, "order did not reach Filled");
    harness::invariants::assert_tracked_used_events(h.routed());
    harness::invariants::assert_filled_qty(
        &h.cache().borrow(),
        &order.client_order_id(),
        Decimal::from(100),
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[tokio::test]
async fn tracked_partial_then_full_fill_is_exact_and_deduplicated() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );

    h.mock_state.feed_user("ws_user_trade.json").await;
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PartiallyFilled)
        })
        .await,
        "order did not reach PartiallyFilled",
    );
    harness::invariants::assert_filled_qty(
        &h.cache().borrow(),
        &order.client_order_id(),
        Decimal::from(25),
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        true,
    );

    h.mock_state.feed_user("ws_user_trade.json").await;
    h.pump_for(Duration::from_millis(200)).await;
    harness::invariants::assert_filled_qty(
        &h.cache().borrow(),
        &order.client_order_id(),
        Decimal::from(25),
    );

    h.mock_state
        .feed_user("ws_user_trade_completion.json")
        .await;
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await,
        "order did not reach Filled",
    );
    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        2,
    );
    harness::invariants::assert_tracked_used_events(h.routed());
    harness::invariants::assert_filled_qty(
        &h.cache().borrow(),
        &order.client_order_id(),
        Decimal::from(100),
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[tokio::test]
async fn external_order_is_parsed_by_mass_status_reconciliation() {
    let h = harness::Harness::build().await;
    *h.mock_state.orders_response_override.lock().await = Some(crate::mock_venue::load_json(
        "http_open_orders_harness.json",
    ));
    *h.mock_state.trades_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_empty_page.json"));

    let mass_status = h.reconcile_from_venue().await;
    let reports = mass_status.order_reports();
    let report = reports.values().next().expect("external order report");

    assert_eq!(reports.len(), 1);
    assert_eq!(report.account_id, AccountId::from(harness::ACCOUNT_ID));
    assert_eq!(report.client_order_id, None);
    assert_eq!(report.instrument_id, h.instrument_id());
    assert_eq!(
        report.venue_order_id,
        VenueOrderId::from(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID),
    );
    assert_eq!(report.order_side, Some(OrderSide::Buy));
    assert_eq!(report.order_type, OrderType::Limit);
    assert_eq!(report.time_in_force, TimeInForce::Gtc);
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.quantity, Quantity::from("100.0000"));
    assert_eq!(report.filled_qty, Quantity::from("0.0000"));
    assert_eq!(report.price, Some(Price::from("0.5000")));
    assert_eq!(
        report.ts_accepted,
        UnixNanos::from(1_703_875_200_000_000_000),
    );
    assert_eq!(report.ts_last, report.ts_accepted);
    assert_eq!(report.expire_time, None);
    assert!(!report.post_only);
    assert!(!report.reduce_only);
}

#[rstest]
#[tokio::test]
async fn external_fill_is_parsed_by_mass_status_reconciliation() {
    let h = harness::Harness::build().await;
    *h.mock_state.orders_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_empty_page.json"));
    *h.mock_state.trades_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_trades_page.json"));

    let mass_status = h.reconcile_from_venue().await;
    let fills = mass_status.fill_reports();
    let report = fills
        .values()
        .flat_map(|reports| reports.iter())
        .next()
        .expect("external fill report");

    assert_eq!(fills.values().map(Vec::len).sum::<usize>(), 1);
    assert_eq!(report.account_id, AccountId::from(harness::ACCOUNT_ID));
    assert_eq!(report.client_order_id, None);
    assert_eq!(report.instrument_id, h.instrument_id());
    assert_eq!(
        report.venue_order_id,
        VenueOrderId::from(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID),
    );
    assert_eq!(report.trade_id, TradeId::from("trade-0x001"));
    assert_eq!(report.order_side, OrderSide::Buy);
    assert_eq!(report.last_qty, Quantity::from("10.0000"));
    assert_eq!(report.last_px, Price::from("0.5000"));
    assert_eq!(report.commission, Money::zero(Currency::pUSD()));
    assert_eq!(report.liquidity_side, LiquiditySide::Taker);
    assert_eq!(report.avg_px, None);
    assert_eq!(report.ts_event, UnixNanos::from(1_704_067_200_000_000_000));
    assert_eq!(report.venue_position_id, None);
}

#[rstest]
#[tokio::test]
async fn submit_venue_error_rejects_and_stays_out_of_book() {
    let mut h = harness::Harness::build().await;
    *h.mock_state.order_response.lock().await = Some(crate::mock_venue::load_json(
        "http_order_response_failed.json",
    ));
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    let rejected = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Rejected)
        })
        .await;

    assert!(rejected, "order did not reach Rejected");
    assert_eq!(h.risk_command_count(), 1);
    harness::invariants::assert_tracked_used_events(h.routed());
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Rejected,
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[tokio::test]
async fn cancel_replace_rotates_venue_id_and_updates_exact_values() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );
    let old_venue_order_id = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .and_then(|cached| cached.venue_order_id())
        .unwrap();
    *h.mock_state.single_order_response.lock().await =
        Some(crate::mock_venue::load_json("http_canceled_orders_harness.json")["data"][0].clone());
    *h.mock_state.trades_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_empty_page.json"));
    h.mock_state
        .order_response_uses_request_hash
        .store(true, std::sync::atomic::Ordering::Release);

    h.modify_via_risk(
        &order,
        Some(Price::from("0.6000")),
        Some(Quantity::from("120.0000")),
    );
    let updated = h
        .pump_until(DEADLINE, |cache| {
            cache.order(&order.client_order_id()).is_some_and(|cached| {
                cached.status() == OrderStatus::Accepted
                    && cached.venue_order_id() != Some(old_venue_order_id)
                    && cached.price() == Some(Price::from("0.6000"))
                    && cached.quantity() == Quantity::from("120.0000")
            })
        })
        .await;
    let open_order_ids = h.mock_state.open_order_ids.lock().await.clone();

    assert!(updated, "replacement did not update the cached order");
    assert_eq!(h.risk_command_count(), 2);
    harness::invariants::assert_tracked_used_events(h.routed());
    assert_eq!(open_order_ids.len(), 1);
    let replacement_venue_order_id = VenueOrderId::from(
        open_order_ids
            .iter()
            .next()
            .expect("mock should retain the replacement venue order ID")
            .as_str(),
    );
    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(cached.client_order_id(), order.client_order_id());
    assert_eq!(cached.venue_order_id(), Some(replacement_venue_order_id));
    assert_eq!(cached.price(), Some(Price::from("0.6000")));
    assert_eq!(cached.quantity(), Quantity::from("120.0000"));
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        true,
    );
    harness::invariants::assert_own_book_consistent(&h.cache().borrow(), &h.instrument_id());
}

#[rstest]
#[tokio::test]
async fn startup_reconciliation_correlates_tracked_open_order() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );
    let tracked_event_count = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .expect("tracked order should be cached")
        .events()
        .len();
    *h.mock_state.orders_response_override.lock().await = Some(crate::mock_venue::load_json(
        "http_open_orders_harness.json",
    ));
    *h.mock_state.trades_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_empty_page.json"));

    let mass_status = h.reconcile_from_venue().await;
    let reports = mass_status.order_reports();
    let report = reports.values().next().expect("tracked order report");

    assert_eq!(reports.len(), 1);
    assert_eq!(report.client_order_id, None);
    assert_eq!(
        report.venue_order_id,
        VenueOrderId::from(crate::mock_venue::DEFAULT_ACCEPTED_ORDER_ID)
    );
    assert_eq!(report.order_status, OrderStatus::Accepted);
    let cache = h.cache().borrow();
    assert_eq!(
        cache
            .orders(None, Some(&h.instrument_id()), None, None, None)
            .len(),
        1,
    );
    let tracked = cache
        .order(&order.client_order_id())
        .expect("tracked order should remain cached");
    assert_eq!(tracked.status(), OrderStatus::Accepted);
    assert_eq!(tracked.events().len(), tracked_event_count);
}

#[rstest]
#[tokio::test]
async fn reconciliation_applies_missed_terminal_cancel() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );
    h.mark_pending_cancel(&order);
    *h.mock_state.orders_response_override.lock().await = Some(crate::mock_venue::load_json(
        "http_canceled_orders_harness.json",
    ));
    *h.mock_state.trades_response_override.lock().await =
        Some(crate::mock_venue::load_json("http_empty_page.json"));

    let mass_status = h.reconcile_from_venue().await;
    let reports = mass_status.order_reports();
    let report = reports.values().next().expect("canceled order report");

    assert_eq!(report.client_order_id, None);
    assert_eq!(report.order_status, OrderStatus::Canceled);
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Canceled,
    );
    harness::invariants::assert_in_own_book(
        &h.cache().borrow(),
        &h.instrument_id(),
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[tokio::test]
async fn ambiguous_submit_is_resolved_by_signed_hash_websocket_order() {
    let mut h = harness::Harness::build().await;
    *h.mock_state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    h.mock_state
        .order_response_uses_request_hash
        .store(true, std::sync::atomic::Ordering::Release);
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    h.pump_for(Duration::from_millis(200)).await;
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Submitted,
    );
    let venue_order_id = h
        .mock_state
        .open_order_ids
        .lock()
        .await
        .iter()
        .next()
        .cloned()
        .expect("mock should retain the signed order hash");
    let mut websocket_order = crate::mock_venue::load_json("ws_user_order_placement.json");
    websocket_order["id"] = serde_json::Value::String(venue_order_id.clone());
    websocket_order["event_type"] = serde_json::Value::String("order".to_string());
    h.mock_state.send_user(websocket_order).await;
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    assert!(accepted, "WebSocket order did not resolve ambiguous submit");
    harness::invariants::assert_tracked_used_events(h.routed());
    assert_eq!(
        cached.venue_order_id(),
        Some(VenueOrderId::from(venue_order_id.as_str())),
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Accepted(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Rejected(_))),
        0,
    );
}

#[rstest]
#[tokio::test]
async fn ambiguous_submit_is_resolved_by_targeted_rest_read() {
    let mut h = harness::Harness::build().await;
    *h.mock_state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    h.mock_state
        .order_response_uses_request_hash
        .store(true, std::sync::atomic::Ordering::Release);
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    h.pump_for(Duration::from_millis(200)).await;
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::Submitted,
    );
    let venue_order_id = h
        .mock_state
        .open_order_ids
        .lock()
        .await
        .iter()
        .next()
        .cloned()
        .expect("mock should retain the signed order hash");
    let mut venue_order = crate::mock_venue::load_json("http_open_order.json");
    venue_order["id"] = serde_json::Value::String(venue_order_id.clone());
    venue_order["asset_id"] = serde_json::Value::from(crate::mock_venue::TEST_TOKEN_ID);
    venue_order["market"] = serde_json::Value::from(crate::mock_venue::TEST_CONDITION_ID);
    venue_order["size_matched"] = serde_json::Value::from("0.0000");
    *h.mock_state.single_order_response.lock().await = Some(venue_order);

    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;

    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    assert!(
        accepted,
        "targeted REST read did not resolve ambiguous submit"
    );
    harness::invariants::assert_tracked_used_events(h.routed());
    assert_eq!(
        cached.venue_order_id(),
        Some(VenueOrderId::from(venue_order_id.as_str())),
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Accepted(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Rejected(_))),
        0,
    );
}

#[rstest]
#[tokio::test]
async fn ambiguous_submit_resolution_retries_failed_order_reads() {
    let mut h = harness::Harness::build().await;
    *h.mock_state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    h.mock_state
        .order_response_uses_request_hash
        .store(true, std::sync::atomic::Ordering::Release);
    h.mock_state
        .single_order_response_statuses
        .lock()
        .await
        .extend([
            StatusCode::INTERNAL_SERVER_ERROR,
            StatusCode::INTERNAL_SERVER_ERROR,
        ]);
    serve_rest_trades(&h, &[]).await;
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    h.pump_for(Duration::from_millis(200)).await;
    let status_while_unknown = cached_order(&h, &order).status();
    let gate_while_unknown = generate_mass_status(&h).await;
    let venue_order_id = h
        .mock_state
        .open_order_ids
        .lock()
        .await
        .iter()
        .next()
        .cloned()
        .expect("mock should retain the signed order hash");
    let mut venue_order = load_json("http_open_order.json");
    venue_order["id"] = json!(venue_order_id);
    venue_order["asset_id"] = json!(crate::mock_venue::TEST_TOKEN_ID);
    venue_order["market"] = json!(crate::mock_venue::TEST_CONDITION_ID);
    venue_order["size_matched"] = json!("0.0000");
    *h.mock_state.single_order_response.lock().await = Some(venue_order);

    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert_eq!(status_while_unknown, OrderStatus::Submitted);
    assert_eq!(
        gate_while_unknown.unwrap_err().to_string(),
        "cannot generate mass status: 1 Polymarket order(s) have an unknown submit outcome"
    );
    assert!(accepted, "resolver did not retry after failed order reads");
    assert!(resumed, "reports stayed blocked after the order resolved");
    assert_eq!(
        h.mock_state
            .single_order_get_count
            .load(std::sync::atomic::Ordering::Acquire),
        3,
    );
    let cached = cached_order(&h, &order);
    assert_eq!(
        cached.venue_order_id(),
        Some(VenueOrderId::from(venue_order_id.as_str())),
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Accepted(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Rejected(_))),
        0,
    );
}

#[rstest]
#[tokio::test]
async fn ambiguous_submit_resolves_trade_matched_before_order_creation() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    *h.mock_state.order_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;
    h.mock_state
        .order_response_uses_request_hash
        .store(true, std::sync::atomic::Ordering::Release);
    h.mock_state
        .trades_filter_after
        .store(true, std::sync::atomic::Ordering::Release);
    let order = harness::limit_order(h.instrument_id(), "O-1");

    h.submit_via_risk(&order);
    h.pump_for(Duration::from_millis(200)).await;
    let venue_order_id = h
        .mock_state
        .open_order_ids
        .lock()
        .await
        .iter()
        .next()
        .cloned()
        .expect("mock should retain the signed order hash");

    // The venue can stamp an order that matches on submit after its trade's match time
    let mut trade = user_trade("ws_user_trade_full.json", "CONFIRMED");
    trade["taker_order_id"] = json!(venue_order_id);
    trade["match_time"] = json!("1704067260");
    serve_rest_trades(&h, &[trade]).await;
    let mut venue_order = load_json("http_open_order.json");
    venue_order["id"] = json!(venue_order_id);
    venue_order["asset_id"] = json!(crate::mock_venue::TEST_TOKEN_ID);
    venue_order["market"] = json!(crate::mock_venue::TEST_CONDITION_ID);
    venue_order["status"] = json!("MATCHED");
    venue_order["size_matched"] = json!("100.0000");
    venue_order["created_at"] = json!(1_704_067_261_u64);
    *h.mock_state.single_order_response.lock().await = Some(venue_order);

    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert!(
        filled,
        "resolver missed a trade matched before the order's creation time"
    );
    assert!(resumed, "reports stayed blocked after the order resolved");
    let cached = cached_order(&h, &order);
    assert_eq!(
        cached.venue_order_id(),
        Some(VenueOrderId::from(venue_order_id.as_str())),
    );
    assert_eq!(cached.filled_qty(), Quantity::from("100.0000"));

    let fill_trade_ids: Vec<TradeId> = cached
        .events()
        .iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.trade_id),
            _ => None,
        })
        .collect();

    assert_eq!(fill_trade_ids, vec![TradeId::from("trade-0xfull")]);
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn ambiguous_cancel_is_resolved_by_websocket_cancel() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(h.instrument_id(), "O-1");
    h.submit_via_risk(&order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );
    *h.mock_state.cancel_response_status.lock().await = StatusCode::INTERNAL_SERVER_ERROR;

    h.cancel_via_execution(&order);
    let state = h.mock_state.clone();
    wait_until_async(
        || {
            let state = state.clone();
            async move { *state.cancel_delete_count.lock().await == 1 }
        },
        DEADLINE,
    )
    .await;
    h.pump_for(Duration::from_millis(200)).await;
    harness::invariants::assert_order_status(
        &h.cache().borrow(),
        &order.client_order_id(),
        OrderStatus::PendingCancel,
    );

    h.mock_state
        .feed_user("ws_user_order_cancellation.json")
        .await;
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    let cached = h
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();

    assert!(
        canceled,
        "WebSocket cancel did not resolve ambiguous cancel"
    );
    harness::invariants::assert_tracked_used_events(h.routed());
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Canceled(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(
            event,
            OrderEventAny::CancelRejected(_)
        )),
        0,
    );
}

#[rstest]
#[tokio::test]
async fn stream_failed_trade_confirmed_by_rest_keeps_fill_and_resumes_reports() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[]).await;
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "MATCHED"))
        .await;
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await,
        "order did not reach Filled",
    );

    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "FAILED"))
        .await;
    h.pump_for(Duration::from_millis(200)).await;
    let quarantined = generate_mass_status(&h).await;
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "CONFIRMED")]).await;
    let resumed = reports_resume(&mut h).await;

    assert_eq!(
        quarantined.unwrap_err().to_string(),
        UNRESOLVED_MASS_STATUS_ERROR,
    );
    assert!(resumed, "reports stayed blocked after REST confirmation");
    let cached = cached_order(&h, &order);
    assert_eq!(cached.status(), OrderStatus::Filled);
    assert_eq!(cached.filled_qty(), Quantity::from("100.0000"));
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(
            event,
            OrderEventAny::FillVoided(_)
        )),
        0,
    );
    let cache = h.cache().borrow();
    let positions = cache.positions_open(None, Some(&h.instrument_id()), None, None, None);
    assert_eq!(positions.len(), 1);
    assert_eq!(positions[0].quantity, Quantity::from("100.0000"));
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn failed_multi_maker_trade_voids_only_owned_maker_leg() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[]).await;
    h.mock_state.send_user(owned_maker_trade("MATCHED")).await;
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PartiallyFilled)
        })
        .await,
        "order did not reach PartiallyFilled",
    );

    h.mock_state.send_user(owned_maker_trade("FAILED")).await;
    serve_rest_trades(&h, &[owned_maker_trade("FAILED")]).await;

    let voided = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .is_some_and(|cached| !cached.voided_qty().is_zero())
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert!(voided, "owned maker leg was not voided");
    assert!(resumed, "reports stayed blocked after the REST failure");
    let cached = cached_order(&h, &order);

    let fills: Vec<_> = cached
        .events()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.clone()),
            _ => None,
        })
        .collect();

    let voids: Vec<_> = cached
        .events()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::FillVoided(voided) => Some(voided.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 1);
    assert_eq!(voids.len(), 1);
    assert_eq!(fills[0].liquidity_side, LiquiditySide::Maker);
    assert_eq!(fills[0].last_qty, Quantity::from("60.0000"));
    assert_eq!(voids[0].trade_id, fills[0].trade_id);
    assert_eq!(voids[0].voided_qty, Quantity::from("60.0000"));
    assert_eq!(cached.status(), OrderStatus::Accepted);
    assert_eq!(cached.filled_qty(), Quantity::from("0.0000"));
    assert_eq!(cached.voided_qty(), Quantity::from("60.0000"));
    assert!(
        h.cache()
            .borrow()
            .positions_open(None, Some(&h.instrument_id()), None, None, None)
            .is_empty(),
        "voided maker fill left an open position",
    );
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn fill_applied_after_rest_failure_is_voided_once() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "FAILED")]).await;

    // The fill waits unrouted in the event channel while REST settles the trade as failed, so
    // core applies it only after the failure is final and the observer owes it a void
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "MATCHED"))
        .await;
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "FAILED"))
        .await;
    tokio::time::sleep(Duration::from_millis(800)).await;
    let status_before_routing = cached_order(&h, &order).status();

    let voided = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Voided)
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert_eq!(status_before_routing, OrderStatus::Accepted);
    assert!(voided, "late applied fill was not voided");
    assert!(resumed, "reports stayed blocked after the void applied");
    let cached = cached_order(&h, &order);
    assert_eq!(cached.filled_qty(), Quantity::from("0.0000"));
    assert_eq!(cached.voided_qty(), Quantity::from("100.0000"));
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(
            event,
            OrderEventAny::FillVoided(_)
        )),
        1,
    );
    assert!(
        h.cache()
            .borrow()
            .positions_open(None, Some(&h.instrument_id()), None, None, None)
            .is_empty(),
        "voided fill left an open position",
    );
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn reconnect_resolves_provisional_trade_from_rest() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "MATCHED"))
        .await;
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await,
        "order did not reach Filled",
    );

    // The stream never delivers the terminal status; only REST knows the trade failed
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "FAILED")]).await;
    h.pump_for(Duration::from_millis(600)).await;
    let status_before_reconnect = cached_order(&h, &order).status();

    // Withhold the REST result so the unresolved interval after the reconnect is observable
    serve_rest_trades(&h, &[]).await;
    let handle = h
        .sockets
        .handle(h.client_id(), Ustr::from(USER_STREAMS_ENDPOINT))
        .expect("user stream should register a reconnect handle");
    let outcome = handle.request_reconnect();
    let blocked = reports_block(&mut h).await;
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "FAILED")]).await;

    let voided = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Voided)
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert_eq!(status_before_reconnect, OrderStatus::Filled);
    assert_eq!(outcome, SocketReconnectRequestOutcome::Accepted);
    assert_eq!(
        blocked.map(|e| e.to_string()),
        Some(UNRESOLVED_MASS_STATUS_ERROR.to_string()),
    );
    assert!(voided, "reconnect did not resolve the provisional trade");
    assert!(resumed, "reports stayed blocked after the REST failure");
    let cached = cached_order(&h, &order);
    assert_eq!(cached.filled_qty(), Quantity::from("0.0000"));
    assert_eq!(cached.voided_qty(), Quantity::from("100.0000"));
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&cached, |event| matches!(
            event,
            OrderEventAny::FillVoided(_)
        )),
        1,
    );
    assert!(
        h.cache()
            .borrow()
            .positions_open(None, Some(&h.instrument_id()), None, None, None)
            .is_empty(),
        "voided fill left an open position",
    );
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn rest_confirmation_applies_quarantined_unapplied_trade_once() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[]).await;

    // The stream missed the match and first reports the trade as failed
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "FAILED"))
        .await;
    h.pump_for(Duration::from_millis(200)).await;
    let quarantined = generate_mass_status(&h).await;
    let status_while_quarantined = cached_order(&h, &order).status();
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "CONFIRMED")]).await;

    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "CONFIRMED"))
        .await;
    h.pump_for(Duration::from_millis(200)).await;
    let resumed = reports_resume(&mut h).await;

    assert_eq!(
        quarantined.unwrap_err().to_string(),
        UNRESOLVED_MASS_STATUS_ERROR,
    );
    assert_eq!(status_while_quarantined, OrderStatus::Accepted);
    assert!(filled, "REST confirmation did not apply the fill");
    assert!(resumed, "reports stayed blocked after REST confirmation");
    let cached = cached_order(&h, &order);

    let fills: Vec<_> = cached
        .events()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.clone()),
            _ => None,
        })
        .collect();

    assert_eq!(fills.len(), 1);
    assert_eq!(fills[0].trade_id, TradeId::from("trade-0xfull"));
    assert_eq!(fills[0].last_qty, Quantity::from("100.0000"));
    assert_eq!(fills[0].last_px, Price::from("0.5000"));
    assert_eq!(fills[0].liquidity_side, LiquiditySide::Taker);
    assert_eq!(cached.filled_qty(), Quantity::from("100.0000"));
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn engine_declined_fill_fails_reports_closed() {
    let mut h = harness::Harness::build().await;
    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from(harness::TRADER_ID))
        .strategy_id(StrategyId::from(harness::STRATEGY_ID))
        .instrument_id(h.instrument_id())
        .client_order_id(ClientOrderId::from("O-1"))
        .side(OrderSide::Sell)
        .price(Price::from("0.5000"))
        .quantity(Quantity::from("100.0000"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[]).await;
    let declined = record_declined_fills();
    let before_trade = generate_mass_status(&h).await;

    // A SELL fill past the order quantity is declined by the engine as an overfill
    let mut trade = user_trade("ws_user_trade_full.json", "CONFIRMED");
    trade["side"] = json!("SELL");
    trade["size"] = json!("150.0");
    trade["maker_orders"][0]["matched_amount"] = json!("150.0000");
    h.mock_state.send_user(trade).await;
    h.pump_for(Duration::from_millis(300)).await;
    let after_decline = generate_mass_status(&h).await;

    assert!(
        before_trade.is_ok(),
        "reports were blocked before the trade"
    );
    assert_eq!(
        after_decline.unwrap_err().to_string(),
        UNRESOLVED_MASS_STATUS_ERROR,
    );
    let declined = declined.borrow();
    assert_eq!(declined.len(), 1);

    let OrderEventAny::Filled(declined_fill) = &declined[0] else {
        panic!("expected a declined fill, was {:?}", declined[0]);
    };

    assert_eq!(declined_fill.trade_id, TradeId::from("trade-0xfull"));
    assert_eq!(
        declined_fill.account_id,
        AccountId::from(harness::ACCOUNT_ID)
    );
    assert_eq!(declined_fill.instrument_id, h.instrument_id());
    assert_eq!(declined_fill.last_qty, Quantity::from("150.0000"));
    let cached = cached_order(&h, &order);
    assert_eq!(cached.status(), OrderStatus::Accepted);
    assert_eq!(cached.filled_qty(), Quantity::from("0.0000"));
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        0,
    );
}

#[rstest]
#[tokio::test]
async fn matched_not_broadcasted_trade_applies_provisionally_once() {
    let mut h = harness::Harness::build().await;
    let declined = record_declined_fills();
    let order = harness::limit_order(h.instrument_id(), "O-1");
    submit_until_accepted(&mut h, &order).await;
    serve_rest_trades(&h, &[user_trade("ws_user_trade_full.json", "CONFIRMED")]).await;

    h.mock_state
        .send_user(user_trade(
            "ws_user_trade_full.json",
            "MATCHED_NOT_BROADCASTED",
        ))
        .await;

    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    let provisional = generate_mass_status(&h).await;
    h.mock_state
        .send_user(user_trade("ws_user_trade_full.json", "CONFIRMED"))
        .await;
    h.pump_for(Duration::from_millis(200)).await;
    let confirmed = generate_mass_status(&h).await;

    assert!(filled, "MATCHED_NOT_BROADCASTED trade did not apply");
    assert!(provisional.is_ok(), "provisional fill blocked reports");
    assert!(confirmed.is_ok(), "stream confirmation blocked reports");
    let cached = cached_order(&h, &order);
    assert_eq!(cached.filled_qty(), Quantity::from("100.0000"));
    assert_eq!(
        event_count(&cached, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

#[rstest]
#[tokio::test]
async fn restart_hydrates_cached_fill_and_ignores_replayed_trade() {
    let order = harness::limit_order(InstrumentId::from(harness::INSTRUMENT_ID), "O-1");
    let mut h =
        harness::Harness::build_with_cache(|execution| seed_partially_filled(execution, &order))
            .await;
    serve_rest_trades(&h, &[]).await;
    let declined = record_declined_fills();

    h.mock_state.feed_user("ws_user_trade.json").await;
    h.pump_for(Duration::from_millis(200)).await;
    let after_replay = generate_mass_status(&h).await;
    let filled_after_replay = cached_order(&h, &order).filled_qty();

    // New trades on a restored order wait for a targeted REST read
    h.mock_state
        .feed_user("ws_user_trade_completion.json")
        .await;
    h.pump_for(Duration::from_millis(200)).await;
    let while_gated = generate_mass_status(&h).await;
    let filled_while_gated = cached_order(&h, &order).filled_qty();
    serve_rest_trades(
        &h,
        &[user_trade("ws_user_trade_completion.json", "CONFIRMED")],
    )
    .await;

    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    h.pump_for(Duration::from_millis(200)).await;

    assert!(after_replay.is_ok(), "replayed trade blocked reports");
    assert_eq!(filled_after_replay, Quantity::from("25.0000"));
    assert_eq!(
        while_gated.unwrap_err().to_string(),
        UNRESOLVED_MASS_STATUS_ERROR,
    );
    assert_eq!(filled_while_gated, Quantity::from("25.0000"));
    assert!(filled, "REST confirmation did not fill the restored order");
    let cached = cached_order(&h, &order);

    let trade_ids: Vec<_> = cached
        .events()
        .into_iter()
        .filter_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill.trade_id),
            _ => None,
        })
        .collect();

    assert_eq!(
        trade_ids,
        vec![
            TradeId::from("trade-0xabcdef1234"),
            TradeId::from("trade-0xcompletion"),
        ],
    );
    assert_eq!(cached.filled_qty(), Quantity::from("100.0000"));
    assert_eq!(*declined.borrow(), Vec::<OrderEventAny>::new());
}

async fn submit_until_accepted(h: &mut harness::Harness, order: &OrderAny) {
    h.submit_via_risk(order);
    assert!(
        h.pump_until(DEADLINE, |cache| {
            order_reached(cache, order, OrderStatus::Accepted)
        })
        .await,
        "order did not reach Accepted",
    );
}

fn cached_order(h: &harness::Harness, order: &OrderAny) -> OrderAny {
    h.cache()
        .borrow()
        .order(&order.client_order_id())
        .expect("order should be cached")
        .clone()
}

fn user_trade(fixture: &str, status: &str) -> Value {
    let mut trade = load_json(fixture);
    trade["status"] = json!(status);
    trade["event_type"] = json!("trade");
    trade
}

// The tracked order is the one owned maker among the two makers of the full-fill trade
fn owned_maker_trade(status: &str) -> Value {
    let mut trade = user_trade("ws_user_trade_full.json", status);
    let mut unowned = trade["maker_orders"][0].clone();
    unowned["matched_amount"] = json!("40.0000");
    unowned["side"] = json!("BUY");
    let mut owned = unowned.clone();
    owned["order_id"] = json!(DEFAULT_ACCEPTED_ORDER_ID);
    owned["owner"] = trade["owner"].clone();
    owned["maker_address"] = trade["maker_address"].clone();
    owned["matched_amount"] = json!("60.0000");

    trade["trader_side"] = json!("MAKER");
    trade["side"] = json!("SELL");
    trade["taker_order_id"] =
        json!("0xtaker05taker05taker05taker05taker05taker05taker05taker05taker05");
    trade["owner"] = unowned["owner"].clone();
    trade["maker_address"] = unowned["maker_address"].clone();
    trade["maker_orders"] = json!([unowned, owned]);
    trade
}

async fn serve_rest_trades(h: &harness::Harness, trades: &[Value]) {
    *h.mock_state.orders_response_override.lock().await = Some(load_json("http_empty_page.json"));
    *h.mock_state.trades_response_override.lock().await =
        Some(json!({ "data": trades, "next_cursor": "LTE=" }));
}

#[allow(
    clippy::await_holding_refcell_ref,
    reason = "single-threaded test harness only runs mock venue tasks during the await"
)]
async fn generate_mass_status(h: &harness::Harness) -> anyhow::Result<Option<ExecutionMassStatus>> {
    let client_id = h.client_id();
    h.exec_engine()
        .borrow_mut()
        .generate_mass_status(&client_id, None)
        .await
}

fn record_declined_fills() -> Rc<RefCell<Vec<OrderEventAny>>> {
    let declined = Rc::new(RefCell::new(Vec::new()));
    let recorder = declined.clone();
    msgbus::subscribe_order_events(
        "events.order_fill_declined.*".into(),
        TypedHandler::from(move |event: &OrderEventAny| recorder.borrow_mut().push(event.clone())),
        None,
    );
    declined
}

async fn reports_resume(h: &mut harness::Harness) -> bool {
    let deadline = tokio::time::Instant::now() + DEADLINE;

    while tokio::time::Instant::now() < deadline {
        if generate_mass_status(h).await.is_ok() {
            return true;
        }

        h.pump_for(Duration::from_millis(100)).await;
    }

    false
}

async fn reports_block(h: &mut harness::Harness) -> Option<anyhow::Error> {
    let deadline = tokio::time::Instant::now() + DEADLINE;

    while tokio::time::Instant::now() < deadline {
        if let Err(e) = generate_mass_status(h).await {
            return Some(e);
        }

        h.pump_for(Duration::from_millis(100)).await;
    }

    None
}

// Leaves the order as a previous run would: accepted, then filled 25 by `ws_user_trade.json`
fn seed_partially_filled(execution: &ExecutionHarness, order: &OrderAny) {
    let account_id = AccountId::from(harness::ACCOUNT_ID);
    execution
        .cache()
        .borrow_mut()
        .add_order(order.clone(), None, Some(execution.client_id()), false)
        .unwrap();
    let mut engine = execution.exec_engine().borrow_mut();
    engine.process(&TestOrderEventStubs::submitted(order, account_id));
    engine.process(&TestOrderEventStubs::accepted(
        order,
        account_id,
        VenueOrderId::from(DEFAULT_ACCEPTED_ORDER_ID),
    ));
    let accepted = execution
        .cache()
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    engine.process(&TestOrderEventStubs::filled(
        &accepted,
        &harness::instrument(),
        Some(TradeId::from("trade-0xabcdef1234")),
        None,
        Some(Price::from("0.5000")),
        Some(Quantity::from("25.0000")),
        Some(LiquiditySide::Taker),
        Some(Money::zero(Currency::pUSD())),
        None,
        Some(account_id),
    ));
}
