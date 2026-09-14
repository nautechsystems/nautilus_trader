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

use std::time::Duration;

use axum::http::StatusCode;
use nautilus_common::{actor::DataActor, cache::Cache, testing::wait_until_async};
use nautilus_core::UnixNanos;
use nautilus_model::{
    enums::{LiquiditySide, OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, StrategyId, TradeId, VenueOrderId},
    orders::{Order, OrderAny},
    types::{Currency, Money, Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;

use crate::harness;

const DEADLINE: Duration = Duration::from_secs(5);

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
