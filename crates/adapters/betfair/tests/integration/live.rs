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

//! End-to-end seam tests: ExecTester -> RiskEngine -> ExecutionEngine ->
//! `BetfairExecutionClient` -> mock venue -> `AsyncRunner` routing fork -> `Cache`.

use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use nautilus_betfair::common::consts::{
    METHOD_CANCEL_ORDERS, METHOD_LIST_CURRENT_ORDERS, METHOD_PLACE_ORDERS, METHOD_REPLACE_ORDERS,
};
use nautilus_common::{actor::DataActor, cache::Cache};
use nautilus_model::{
    enums::OrderStatus,
    events::OrderEventAny,
    identifiers::{TradeId, VenueOrderId},
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;
use serde_json::Value;

use crate::{
    common::{MockResponseGate, MockState, betting_api_error, load_fixture, load_json_fixture},
    harness,
};

const DEADLINE: Duration = Duration::from_secs(5);

fn order_reached(cache: &Cache, order: &OrderAny, status: OrderStatus) -> bool {
    cache
        .order(&order.client_order_id())
        .is_some_and(|cached| cached.status() == status)
}

async fn wait_for_request_count(state: &MockState, method: &str, expected: usize) {
    nautilus_common::testing::wait_until_async(
        || {
            let methods = state.betting_methods.clone();
            async move {
                methods
                    .lock()
                    .iter()
                    .filter(|candidate| candidate.as_str() == method)
                    .count()
                    >= expected
            }
        },
        DEADLINE,
    )
    .await;
}

fn customer_ref(params: &Value) -> &str {
    let value = params["customerRef"]
        .as_str()
        .expect("mutating request must include customerRef");
    assert_eq!(value.len(), 32);
    assert!(
        value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte)),
        "customerRef must be lowercase hexadecimal: {value}",
    );
    value
}

fn assert_applied_once_with_identical_retry(state: &MockState, method: &str) -> String {
    let requests: Vec<Value> = state
        .betting_request_params
        .lock()
        .iter()
        .filter(|(candidate, _)| candidate == method)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(requests.len(), 2, "expected one request and one retry");
    assert_eq!(requests[0], requests[1], "retry params must be identical");

    let applied: Vec<Value> = state
        .betting_applied_request_params
        .lock()
        .iter()
        .filter(|(candidate, _)| candidate == method)
        .map(|(_, params)| params.clone())
        .collect();
    assert_eq!(applied, vec![requests[0].clone()]);

    customer_ref(&requests[0]).to_string()
}

fn clear_mutation_observations(state: &MockState) {
    state.betting_methods.lock().clear();
    state.betting_request_params.lock().clear();
    state.betting_applied_request_params.lock().clear();
}

fn set_timeout_report(state: &MockState, method: &str, fixture_path: &str) {
    let fixture = load_fixture(fixture_path);
    let mut response: Value = serde_json::from_str(&fixture).unwrap();
    response["result"]["status"] = Value::String("TIMEOUT".to_string());
    state
        .betting_overrides
        .lock()
        .insert(method.to_string(), response["result"].clone());
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

    assert!(h.exec_engine.borrow().get_client(&h.client_id()).is_some());
    assert!(h.cache.borrow().instrument(&h.instrument_id).is_some());
}

#[rstest]
#[tokio::test]
async fn submit_routes_to_accepted_in_cache() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;

    assert!(accepted, "order did not reach Accepted via routed events");
    harness::invariants::assert_tracked_used_events(&h.routed);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Accepted,
    );
}

#[rstest]
#[tokio::test]
async fn submit_apply_then_lost_response_resolves_from_ocm() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");
    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);
    h.mock_state.betting_error_overrides.lock().insert(
        METHOD_PLACE_ORDERS.to_string(),
        betting_api_error("NO_SESSION"),
    );

    h.submit_via_risk(&order);
    wait_for_request_count(&h.mock_state, METHOD_PLACE_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    let request_ref = assert_applied_once_with_identical_retry(&h.mock_state, METHOD_PLACE_ORDERS);
    assert!(!request_ref.is_empty());
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Submitted,
    );
    let submitted = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(
        event_count(&submitted, |event| matches!(
            event,
            OrderEventAny::Submitted(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&submitted, |event| matches!(
            event,
            OrderEventAny::Accepted(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&submitted, |event| matches!(
            event,
            OrderEventAny::Rejected(_)
        )),
        0,
    );

    h.feeder.feed("stream/ocm_harness_open.json");
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "OCM did not resolve the applied submit");
    h.pump_for(Duration::from_millis(300)).await;

    let accepted_order = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(
        event_count(&accepted_order, |event| matches!(
            event,
            OrderEventAny::Accepted(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&accepted_order, |event| matches!(
            event,
            OrderEventAny::Rejected(_)
        )),
        0,
    );
    assert_eq!(
        accepted_order.venue_order_id(),
        Some(VenueOrderId::from("228302937743")),
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );
}

#[rstest]
#[tokio::test]
async fn cancel_apply_then_lost_response_resolves_from_ocm() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");
    let setup_ref = h
        .mock_state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_PLACE_ORDERS)
        .map(|(_, params)| customer_ref(params).to_string())
        .unwrap();
    clear_mutation_observations(&h.mock_state);
    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    set_timeout_report(
        &h.mock_state,
        METHOD_CANCEL_ORDERS,
        "rest/betting_cancel_orders_success.json",
    );

    h.cancel_via_execution(&order);
    wait_for_request_count(&h.mock_state, METHOD_CANCEL_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    let cancel_ref = assert_applied_once_with_identical_retry(&h.mock_state, METHOD_CANCEL_ORDERS);
    assert_ne!(cancel_ref, setup_ref);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingCancel,
    );
    let pending = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(
        event_count(&pending, |event| matches!(
            event,
            OrderEventAny::CancelRejected(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&pending, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        0,
    );

    h.feeder.feed("stream/ocm_harness_cancel.json");
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(canceled, "OCM did not resolve the applied cancel");
    h.pump_for(Duration::from_millis(300)).await;

    let canceled_order = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(
        event_count(&canceled_order, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&canceled_order, |event| matches!(
            event,
            OrderEventAny::CancelRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[case::cancel_then_open(false)]
#[case::open_then_cancel(true)]
#[tokio::test]
async fn replace_apply_then_lost_response_resolves_from_ocm(#[case] replacement_open_first: bool) {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");
    let setup_ref = h
        .mock_state
        .betting_request_params
        .lock()
        .iter()
        .find(|(method, _)| method == METHOD_PLACE_ORDERS)
        .map(|(_, params)| customer_ref(params).to_string())
        .unwrap();
    clear_mutation_observations(&h.mock_state);
    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 502);
    set_timeout_report(
        &h.mock_state,
        METHOD_REPLACE_ORDERS,
        "rest/betting_replace_orders_success.json",
    );

    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    wait_for_request_count(&h.mock_state, METHOD_REPLACE_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    let replace_ref =
        assert_applied_once_with_identical_retry(&h.mock_state, METHOD_REPLACE_ORDERS);
    assert_ne!(replace_ref, setup_ref);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingUpdate,
    );

    if replacement_open_first {
        h.feeder.feed("stream/ocm_harness_replace_open.json");
    } else {
        h.feeder.feed("stream/ocm_harness_cancel.json");
        h.pump_for(Duration::from_millis(300)).await;
        harness::invariants::assert_order_status(
            &h.cache.borrow(),
            &order.client_order_id(),
            OrderStatus::PendingUpdate,
        );
        h.feeder.feed("stream/ocm_harness_replace_open.json");
    }

    let new_venue_order_id = VenueOrderId::from("240808766933");
    let promoted = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .and_then(|cached| cached.venue_order_id())
                == Some(new_venue_order_id)
        })
        .await;
    assert!(promoted, "replacement OCM did not promote the new bet");

    if replacement_open_first {
        h.feeder.feed("stream/ocm_harness_cancel.json");
        h.pump_for(Duration::from_millis(300)).await;
    }

    let updated = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(updated.status(), OrderStatus::Accepted);
    assert_eq!(updated.venue_order_id(), Some(new_venue_order_id));
    assert_eq!(updated.price(), Some(Price::from("5.0")));
    assert_eq!(updated.quantity(), Quantity::from("10.0"));
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );
    harness::invariants::assert_tracked_used_events(&h.routed);
}

#[rstest]
#[tokio::test]
async fn replace_filled_stream_before_rest_updates_before_fill() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");

    let waiters = Arc::new(AtomicUsize::new(0));
    let semaphore = Arc::new(tokio::sync::Semaphore::new(0));
    *h.mock_state.betting_response_gate.lock() = Some(MockResponseGate {
        method: METHOD_REPLACE_ORDERS.to_string(),
        waiters: Arc::clone(&waiters),
        semaphore: Arc::clone(&semaphore),
    });

    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    nautilus_common::testing::wait_until_async(
        || {
            let waiters = Arc::clone(&waiters);
            async move { waiters.load(Ordering::Relaxed) == 1 }
        },
        DEADLINE,
    )
    .await;
    assert_eq!(waiters.load(Ordering::Relaxed), 1);

    h.feeder.feed("stream/ocm_harness_replace_filled.json");
    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;

    semaphore.add_permits(1);
    h.mock_state.betting_response_gate.lock().take();
    assert!(filled, "filled replacement OCM did not close the order");
    h.pump_for(Duration::from_millis(500)).await;
    h.feeder.feed("stream/ocm_harness_replace_filled.json");
    h.pump_for(Duration::from_millis(300)).await;

    harness::invariants::assert_tracked_used_events(&h.routed);
    let old_venue_order_id = VenueOrderId::from("228302937743");
    let new_venue_order_id = VenueOrderId::from("240808766933");
    let client_order_id = order.client_order_id();
    let cache = h.cache.borrow();
    let updated = cache.order(&client_order_id).unwrap();
    assert_eq!(updated.status(), OrderStatus::Filled);
    assert_eq!(updated.quantity(), Quantity::from("10.0"));
    assert_eq!(updated.filled_qty(), Quantity::from("10.0"));
    assert_eq!(updated.leaves_qty(), Quantity::zero(1));
    assert_eq!(updated.venue_order_id(), Some(new_venue_order_id));
    assert_eq!(
        updated
            .venue_order_ids()
            .into_iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![old_venue_order_id, new_venue_order_id],
    );
    assert_eq!(
        cache.client_order_id(&old_venue_order_id),
        Some(&client_order_id),
    );
    assert_eq!(
        cache.client_order_id(&new_venue_order_id),
        Some(&client_order_id),
    );

    let events = updated.events();
    let updated_index = events
        .iter()
        .position(|event| matches!(event, OrderEventAny::Updated(_)))
        .expect("replacement must emit OrderUpdated");
    let filled_index = events
        .iter()
        .position(|event| matches!(event, OrderEventAny::Filled(_)))
        .expect("replacement must emit OrderFilled");
    assert!(
        updated_index < filled_index,
        "OrderUpdated must precede the replacement fill",
    );
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
                | OrderEventAny::Canceled(_)
                | OrderEventAny::Rejected(_)
        )),
        0,
    );
    let fill = events
        .iter()
        .find_map(|event| match event {
            OrderEventAny::Filled(fill) => Some(fill),
            _ => None,
        })
        .unwrap();
    assert_eq!(fill.client_order_id, client_order_id);
    assert_eq!(fill.venue_order_id, new_venue_order_id);
    assert_eq!(fill.trade_id, TradeId::from("240808766933-10.00"));
    assert_eq!(fill.last_qty, Quantity::from("10.0"));
    assert_eq!(fill.last_px, Price::from("5.0"));
    drop(updated);
    drop(cache);

    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &client_order_id,
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
    assert!(h.exec_engine.borrow().check_integrity());
    assert!(h.exec_engine.borrow().check_connected());
}

#[rstest]
#[tokio::test]
async fn tracked_cancel_emits_event_and_shrinks_own_book() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.feeder.feed("stream/ocm_harness_cancel.json");
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(canceled, "order did not reach Canceled via routed events");

    harness::invariants::assert_tracked_used_events(&h.routed);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Canceled,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn exec_tester_drives_submit_to_accepted() {
    let mut h = harness::Harness::build().await;
    let instrument_id = h.instrument_id;
    let mut tester = h.register_exec_tester("10");

    tester.on_start().unwrap();
    tester
        .on_quote(&harness::quote(&instrument_id, "3.00", "3.02"))
        .unwrap();

    let accepted = h
        .pump_until(DEADLINE, |cache| {
            cache
                .orders(None, Some(&instrument_id), None, None, None)
                .iter()
                .any(|order| order.status() == OrderStatus::Accepted)
        })
        .await;

    assert!(accepted, "ExecTester-driven order did not reach Accepted");
    harness::invariants::assert_tracked_used_events(&h.routed);
}

#[rstest]
#[tokio::test]
async fn tracked_fill_emits_event_and_closes() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.feeder.feed("stream/ocm_harness_fill.json");
    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;
    assert!(filled, "order did not reach Filled via routed events");

    harness::invariants::assert_tracked_used_events(&h.routed);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Filled,
    );
    harness::invariants::assert_filled_qty(
        &h.cache.borrow(),
        &order.client_order_id(),
        Decimal::from(10),
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn external_order_routes_as_report() {
    let mut h = harness::Harness::build().await;

    h.feeder.feed("stream/ocm_harness_external.json");
    let saw_report = h
        .pump_until_routed(DEADLINE, harness::RoutedKind::Report)
        .await;

    assert!(saw_report, "external order did not route as a report");
}

#[rstest]
#[tokio::test]
async fn tracked_partial_then_full_fill_accounts_correctly() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    // Partial fill: 4 of 10, the order stays open and tracked in the own book.
    h.feeder.feed("stream/ocm_harness_partial_fill.json");
    let partial = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PartiallyFilled)
        })
        .await;
    assert!(partial, "order did not reach PartiallyFilled");
    harness::invariants::assert_filled_qty(
        &h.cache.borrow(),
        &order.client_order_id(),
        Decimal::from(4),
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );

    // Completing fill: cumulative 10, the order closes and leaves the book.
    h.feeder.feed("stream/ocm_harness_fill.json");
    let filled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Filled)
        })
        .await;
    assert!(filled, "order did not reach Filled");

    harness::invariants::assert_tracked_used_events(&h.routed);
    harness::invariants::assert_filled_qty(
        &h.cache.borrow(),
        &order.client_order_id(),
        Decimal::from(10),
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn modify_price_replace_stream_duplicates_do_not_change_order() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    // Price cancel-replace via replaceOrders HTTP: the new bet id comes from the replace
    // fixture's placeInstructionReport, the new price from the modify command.
    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    let new_venue_order_id = VenueOrderId::from("240808766933");
    let promoted = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .and_then(|cached| cached.venue_order_id())
                == Some(new_venue_order_id)
        })
        .await;
    assert!(promoted, "order did not promote to the replacement bet id");

    h.feeder.feed("stream/ocm_harness_cancel.json");
    h.feeder.feed("stream/ocm_harness_cancel.json");
    h.feeder.feed("stream/ocm_harness_replace_open.json");
    h.feeder.feed("stream/ocm_harness_replace_open.json");
    h.pump_for(Duration::from_millis(300)).await;

    harness::invariants::assert_tracked_used_events(&h.routed);
    let cache = h.cache.borrow();
    let updated = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(updated.venue_order_id(), Some(new_venue_order_id));
    assert_eq!(
        updated
            .venue_order_ids()
            .into_iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![
            VenueOrderId::from("228302937743"),
            VenueOrderId::from("240808766933"),
        ],
    );
    assert_eq!(updated.price(), Some(Price::from("5.0")));
    assert_eq!(updated.quantity(), Quantity::from("10.0"));
    assert_eq!(updated.status(), OrderStatus::Accepted);
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Filled(_))),
        0,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &cache,
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );
    harness::invariants::assert_own_book_consistent(&cache, &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn replace_cancelled_not_placed_closes_order_once() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");
    h.override_betting_result(
        METHOD_REPLACE_ORDERS,
        "rest/betting_replace_orders_cancelled_not_placed_live.json",
    );

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.modify_via_risk(&order, Some(Price::from("2.57")), None);
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(
        canceled,
        "partial replacement failure did not close the order"
    );

    h.feeder.feed("stream/ocm_harness_cancel.json");
    h.feeder.feed("stream/ocm_harness_cancel.json");
    h.pump_for(Duration::from_millis(300)).await;

    let canceled = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(canceled.status(), OrderStatus::Canceled);
    assert_eq!(
        canceled.venue_order_id(),
        Some(VenueOrderId::from("228302937743")),
    );
    assert_eq!(canceled.price(), Some(Price::from("3.0")));
    assert_eq!(canceled.quantity(), Quantity::from("10.0"));
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Updated(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[case::rest_first(false)]
#[case::stream_first(true)]
#[tokio::test]
async fn replace_cancelled_not_placed_stays_closed_after_late_partial_fill(
    #[case] old_terminal_first: bool,
) {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");
    h.override_betting_result(
        METHOD_REPLACE_ORDERS,
        "rest/betting_replace_orders_cancelled_not_placed_live.json",
    );

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    if old_terminal_first {
        h.mock_state.betting_response_delays.lock().insert(
            METHOD_REPLACE_ORDERS.to_string(),
            Duration::from_millis(300),
        );
    }
    h.modify_via_risk(&order, Some(Price::from("2.57")), None);
    if old_terminal_first {
        wait_for_request_count(&h.mock_state, METHOD_REPLACE_ORDERS, 1).await;
        h.feeder.feed("stream/ocm_harness_cancel.json");
        h.pump_for(Duration::from_millis(100)).await;
        harness::invariants::assert_order_status(
            &h.cache.borrow(),
            &order.client_order_id(),
            OrderStatus::PendingUpdate,
        );
    }
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(canceled, "replace failure did not cancel the old bet");

    h.feeder
        .feed("stream/ocm_harness_replace_cancel_partial_fill.json");
    h.pump_for(Duration::from_millis(100)).await;
    h.feeder
        .feed("stream/ocm_harness_replace_cancel_with_fill.json");
    h.feeder
        .feed("stream/ocm_harness_replace_cancel_with_fill.json");
    h.pump_for(Duration::from_millis(300)).await;

    let canceled = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(canceled.status(), OrderStatus::Canceled);
    assert_eq!(canceled.filled_qty(), Quantity::from("2.0"));
    assert_eq!(canceled.leaves_qty(), Quantity::from("8.0"));
    assert_eq!(
        canceled.venue_order_id(),
        Some(VenueOrderId::from("228302937743")),
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        2,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Updated(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn old_cancel_before_definitive_replace_error_closes_order_once() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");
    let response: Value = serde_json::from_str(&load_fixture(
        "rest/betting_jsonrpc_error_invalid_params_live.json",
    ))
    .unwrap();
    h.mock_state
        .betting_error_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), response);
    h.mock_state.betting_response_delays.lock().insert(
        METHOD_REPLACE_ORDERS.to_string(),
        Duration::from_millis(300),
    );

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    wait_for_request_count(&h.mock_state, METHOD_REPLACE_ORDERS, 1).await;
    h.feeder.feed("stream/ocm_harness_cancel.json");
    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(
        canceled,
        "old-bet cancel was lost when the replacement request failed"
    );

    h.feeder.feed("stream/ocm_harness_cancel.json");
    h.pump_for(Duration::from_millis(300)).await;

    let canceled = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(canceled.status(), OrderStatus::Canceled);
    assert_eq!(
        canceled.venue_order_id(),
        Some(VenueOrderId::from("228302937743")),
    );
    assert_eq!(canceled.price(), Some(Price::from("3.0")));
    assert_eq!(canceled.quantity(), Quantity::from("10.0"));
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn ambiguous_replace_stays_pending_through_old_bet_cancel() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.mock_state.betting_error_overrides.lock().insert(
        METHOD_REPLACE_ORDERS.to_string(),
        betting_api_error("TIMEOUT_ERROR"),
    );
    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    let pending = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PendingUpdate)
        })
        .await;
    assert!(pending, "ambiguous replace did not remain PendingUpdate");

    h.feeder.feed("stream/ocm_harness_cancel.json");
    let canceled = h
        .pump_until(Duration::from_millis(500), |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(
        !canceled,
        "old-bet cancel from an ambiguous replace must remain suppressed",
    );

    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingUpdate,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );
}

#[rstest]
#[case::rest_first(false, false)]
#[case::stream_first(true, false)]
#[case::bet_taken_or_lapsed(true, true)]
#[tokio::test]
async fn successful_cancel_resolves_ambiguous_replace(
    #[case] stream_first: bool,
    #[case] bet_taken_or_lapsed: bool,
) {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.mock_state.betting_error_overrides.lock().insert(
        METHOD_REPLACE_ORDERS.to_string(),
        betting_api_error("TIMEOUT_ERROR"),
    );
    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    let pending = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PendingUpdate)
        })
        .await;
    assert!(pending, "ambiguous replace did not remain PendingUpdate");

    if stream_first {
        h.mock_state
            .betting_response_delays
            .lock()
            .insert(METHOD_CANCEL_ORDERS.to_string(), Duration::from_millis(300));
    }

    if bet_taken_or_lapsed {
        h.override_betting_result(
            METHOD_CANCEL_ORDERS,
            "rest/betting_cancel_orders_bet_taken_or_lapsed.json",
        );
    }
    h.cancel_via_execution(&order);
    wait_for_request_count(&h.mock_state, METHOD_CANCEL_ORDERS, 1).await;

    if stream_first {
        h.feeder.feed("stream/ocm_harness_cancel.json");
        h.pump_for(Duration::from_millis(100)).await;
        harness::invariants::assert_order_status(
            &h.cache.borrow(),
            &order.client_order_id(),
            OrderStatus::PendingCancel,
        );
    } else {
        h.pump_for(Duration::from_millis(100)).await;
        h.feeder.feed("stream/ocm_harness_cancel.json");
    }

    let canceled = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Canceled)
        })
        .await;
    assert!(
        canceled,
        "successful cancel did not resolve pending replace"
    );

    let canceled = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(canceled.status(), OrderStatus::Canceled);
    assert_eq!(
        canceled.venue_order_id(),
        Some(VenueOrderId::from("228302937743"))
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        1,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::Updated(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&canceled, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_) | OrderEventAny::CancelRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[case::cancel_then_open(false, false)]
#[case::open_then_cancel(true, false)]
#[case::partial_fill_then_open(false, true)]
#[tokio::test]
async fn ambiguous_replace_accounts_old_fill_across_stream_orderings(
    #[case] replacement_open_first: bool,
    #[case] partial_fill_first: bool,
) {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    h.mock_state.betting_error_overrides.lock().insert(
        METHOD_REPLACE_ORDERS.to_string(),
        betting_api_error("TIMEOUT_ERROR"),
    );
    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    let pending = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::PendingUpdate)
        })
        .await;
    assert!(pending, "ambiguous replace did not remain PendingUpdate");

    if partial_fill_first {
        h.feeder
            .feed("stream/ocm_harness_replace_cancel_partial_fill.json");
        let filled = h
            .pump_until(DEADLINE, |cache| {
                cache
                    .order(&order.client_order_id())
                    .is_some_and(|cached| cached.filled_qty().as_decimal() == Decimal::from(2))
            })
            .await;
        assert!(filled, "old-bet partial fill was not applied");
        harness::invariants::assert_order_status(
            &h.cache.borrow(),
            &order.client_order_id(),
            OrderStatus::PendingUpdate,
        );
        h.feeder
            .feed("stream/ocm_harness_replace_open_after_fill.json");
    } else if replacement_open_first {
        h.feeder
            .feed("stream/ocm_harness_replace_open_after_fill.json");
        h.feeder
            .feed("stream/ocm_harness_replace_cancel_with_fill.json");
    } else {
        h.feeder
            .feed("stream/ocm_harness_replace_cancel_with_fill.json");
        h.feeder
            .feed("stream/ocm_harness_replace_open_after_fill.json");
    }

    let filled = h
        .pump_until(DEADLINE, |cache| {
            cache.order(&order.client_order_id()).is_some_and(|cached| {
                cached.filled_qty().as_decimal() == Decimal::from(2)
                    && cached.venue_order_id() == Some(VenueOrderId::from("240808766933"))
            })
        })
        .await;
    assert!(filled, "replacement and old-bet fill were not both applied");
    h.feeder
        .feed("stream/ocm_harness_replace_cancel_with_fill.json");
    h.feeder
        .feed("stream/ocm_harness_replace_open_after_fill.json");
    h.pump_for(Duration::from_millis(300)).await;

    let updated = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(updated.status(), OrderStatus::PartiallyFilled);
    assert_eq!(updated.quantity(), Quantity::from("10.0"));
    assert_eq!(updated.filled_qty().as_decimal(), Decimal::from(2));
    assert_eq!(updated.leaves_qty(), Quantity::from("8.0"));
    assert_eq!(
        updated.venue_order_id(),
        Some(VenueOrderId::from("240808766933"))
    );
    assert_eq!(
        updated
            .venue_order_ids()
            .into_iter()
            .copied()
            .collect::<Vec<_>>(),
        vec![
            VenueOrderId::from("228302937743"),
            VenueOrderId::from("240808766933"),
        ],
    );
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(event, OrderEventAny::Filled(_))),
        1,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::Canceled(_)
        )),
        0,
    );
    assert_eq!(
        event_count(&updated, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        true,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn modify_quantity_reduction_updates_qty() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    // Quantity reduction drives a partial cancel. The request reduces 10 to 6 (cancel 4),
    // but the venue cancels only 3 (a fill raced the reduction), so the working quantity is
    // derived from the actual size_cancelled as 10 - 3 = 7, not the requested target of 6.
    h.override_betting_result(
        METHOD_CANCEL_ORDERS,
        "rest/betting_cancel_orders_size_reduction.json",
    );
    h.modify_via_risk(&order, None, Some(Quantity::from("6.0")));
    let reduced = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .map(|cached| cached.quantity().as_decimal())
                == Some(Decimal::from(7))
        })
        .await;
    assert!(
        reduced,
        "order quantity was not reduced to the size_cancelled-derived 7"
    );

    harness::invariants::assert_tracked_used_events(&h.routed);
    let cache = h.cache.borrow();
    let updated = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(updated.quantity().as_decimal(), Decimal::from(7));
    assert_eq!(
        updated.venue_order_id(),
        Some(VenueOrderId::from("228302937743"))
    );
    assert_eq!(updated.status(), OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn reduction_that_closes_the_bet_settles_on_the_reduced_size() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");

    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    set_timeout_report(
        &h.mock_state,
        METHOD_CANCEL_ORDERS,
        "rest/betting_cancel_orders_success.json",
    );

    h.modify_via_risk(&order, None, Some(Quantity::from("4")));
    wait_for_request_count(&h.mock_state, METHOD_CANCEL_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    // Cancel the unmatched six after four match
    let mut closed =
        load_json_fixture("rest/list_current_orders_harness_open.json")["result"]["currentOrders"]
            [0]
        .clone();
    closed["status"] = Value::from("EXECUTION_COMPLETE");
    closed["sizeMatched"] = Value::from(4.0);
    closed["averagePriceMatched"] = Value::from(3.0);
    closed["sizeRemaining"] = Value::from(0.0);
    closed["sizeCancelled"] = Value::from(6.0);
    h.mock_state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [closed],
            "moreAvailable": false,
        }),
    );

    h.reconcile_from_venue().await;
    let resolved = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .is_some_and(|cached| cached.quantity() == Quantity::from("4"))
        })
        .await;
    assert!(resolved, "reconciliation did not apply the lost reduction");

    // Reconcile the terminal record on the following pass
    h.reconcile_from_venue().await;
    h.pump_for(Duration::from_millis(300)).await;

    let cache = h.cache.borrow();
    let settled = cache.order(&order.client_order_id()).unwrap().clone();
    assert_eq!(
        settled.quantity(),
        Quantity::from("4"),
        "the terminal record must not restore the stake the order never had",
    );
    assert_eq!(settled.filled_qty(), Quantity::from("4"));
    assert_eq!(settled.status(), OrderStatus::Filled);
    assert_eq!(
        event_count(&settled, |event| matches!(event, OrderEventAny::Updated(_))),
        1,
        "the reduction must resolve exactly once",
    );
    harness::invariants::assert_own_book_consistent(&cache, &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn replace_apply_then_lost_response_resolves_from_reconciliation() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");

    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_REPLACE_ORDERS.to_string(), 502);
    set_timeout_report(
        &h.mock_state,
        METHOD_REPLACE_ORDERS,
        "rest/betting_replace_orders_success.json",
    );

    h.modify_via_risk(&order, Some(Price::from("5.0")), None);
    wait_for_request_count(&h.mock_state, METHOD_REPLACE_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingUpdate,
    );

    let old_bet_id = "228302937743";
    let new_bet_id = "240808766933";
    let mut old_leg = load_json_fixture("rest/list_current_orders_harness_canceled.json")["result"]
        ["currentOrders"][0]
        .clone();
    old_leg["betId"] = Value::from(old_bet_id);
    let mut new_leg =
        load_json_fixture("rest/list_current_orders_harness_open.json")["result"]["currentOrders"]
            [0]
        .clone();
    new_leg["betId"] = Value::from(new_bet_id);
    new_leg["priceSize"]["price"] = Value::from(5.0);
    h.mock_state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [old_leg, new_leg],
            "moreAvailable": false,
        }),
    );

    let mass_status = h.reconcile_from_venue().await;
    assert!(
        mass_status.order_reports().is_empty(),
        "the resolving pass must leave the promotion to the direct event: {:?}",
        mass_status.order_reports(),
    );

    let promoted = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .and_then(|cached| cached.venue_order_id())
                == Some(VenueOrderId::from(new_bet_id))
        })
        .await;
    assert!(promoted, "reconciliation did not promote the replacement");
    h.pump_for(Duration::from_millis(300)).await;

    {
        let cache = h.cache.borrow();
        let updated = cache.order(&order.client_order_id()).unwrap().clone();
        assert_eq!(updated.status(), OrderStatus::Accepted);
        assert_eq!(updated.quantity(), Quantity::from("10"));
        assert_eq!(updated.price(), Some(Price::from("5.0")));
        assert_eq!(
            updated.venue_order_id(),
            Some(VenueOrderId::from(new_bet_id))
        );
        assert_eq!(
            event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
            1,
            "the replace must resolve exactly once",
        );
        assert_eq!(
            event_count(&updated, |event| matches!(
                event,
                OrderEventAny::Canceled(_)
            )),
            0,
            "the superseded leg must not cancel the live order",
        );
        assert_eq!(
            event_count(&updated, |event| matches!(
                event,
                OrderEventAny::Accepted(_)
            )),
            1,
            "resolution must not re-accept the order",
        );
        harness::invariants::assert_tracked_used_events(&h.routed);
        harness::invariants::assert_own_book_consistent(&cache, &h.instrument_id);
    }

    let repeated = h.reconcile_from_venue().await;
    let reported_bet_ids: Vec<String> = repeated
        .order_reports()
        .values()
        .map(|report| report.venue_order_id.to_string())
        .collect();
    assert_eq!(reported_bet_ids, vec![new_bet_id.to_string()]);
    h.pump_for(Duration::from_millis(300)).await;

    let settled = h.cache.borrow();
    let settled_order = settled.order(&order.client_order_id()).unwrap().clone();
    assert_eq!(
        event_count(&settled_order, |event| matches!(
            event,
            OrderEventAny::Updated(_)
        )),
        1,
        "a resolved replace must not be promoted again",
    );
    assert_eq!(settled_order.status(), OrderStatus::Accepted);
}

#[rstest]
#[tokio::test]
async fn reduction_apply_then_lost_response_resolves_from_reconciliation() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "setup order did not reach Accepted");

    h.mock_state
        .betting_apply_then_status_one_shot_overrides
        .lock()
        .insert(METHOD_CANCEL_ORDERS.to_string(), 502);
    set_timeout_report(
        &h.mock_state,
        METHOD_CANCEL_ORDERS,
        "rest/betting_cancel_orders_success.json",
    );

    h.modify_via_risk(&order, None, Some(Quantity::from("4")));
    wait_for_request_count(&h.mock_state, METHOD_CANCEL_ORDERS, 2).await;
    h.pump_for(Duration::from_millis(300)).await;

    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingUpdate,
    );
    let inflight = h
        .cache
        .borrow()
        .order(&order.client_order_id())
        .unwrap()
        .clone();
    assert_eq!(inflight.quantity(), Quantity::from("10"));
    assert_eq!(
        event_count(&inflight, |event| matches!(
            event,
            OrderEventAny::ModifyRejected(_)
        )),
        0,
    );

    let mut reduced =
        load_json_fixture("rest/list_current_orders_harness_open.json")["result"]["currentOrders"]
            [0]
        .clone();
    reduced["sizeRemaining"] = Value::from(4.0);
    reduced["sizeCancelled"] = Value::from(6.0);
    h.mock_state.betting_overrides.lock().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        serde_json::json!({
            "currentOrders": [reduced],
            "moreAvailable": false,
        }),
    );

    let mass_status = h.reconcile_from_venue().await;
    assert!(
        mass_status.order_reports().is_empty(),
        "the resolving pass must leave the reduction to the direct event: {:?}",
        mass_status.order_reports(),
    );

    let resolved = h
        .pump_until(DEADLINE, |cache| {
            cache
                .order(&order.client_order_id())
                .is_some_and(|cached| cached.quantity() == Quantity::from("4"))
        })
        .await;
    assert!(resolved, "reconciliation did not apply the lost reduction");
    h.pump_for(Duration::from_millis(300)).await;

    {
        let cache = h.cache.borrow();
        let updated = cache.order(&order.client_order_id()).unwrap().clone();
        assert_eq!(updated.quantity(), Quantity::from("4"));
        assert_eq!(updated.status(), OrderStatus::Accepted);
        assert_eq!(
            updated.venue_order_id(),
            Some(VenueOrderId::from("228302937743"))
        );
        assert_eq!(
            event_count(&updated, |event| matches!(event, OrderEventAny::Updated(_))),
            1,
            "the reduction must resolve exactly once",
        );
        assert_eq!(
            event_count(&updated, |event| matches!(
                event,
                OrderEventAny::ModifyRejected(_)
            )),
            0,
        );
        assert_eq!(
            event_count(&updated, |event| matches!(
                event,
                OrderEventAny::Accepted(_)
            )),
            1,
            "resolution must not re-accept the order",
        );
        harness::invariants::assert_tracked_used_events(&h.routed);
        harness::invariants::assert_own_book_consistent(&cache, &h.instrument_id);
    }

    let repeated = h.reconcile_from_venue().await;
    let reports = repeated.order_reports();
    assert_eq!(reports.len(), 1);
    assert_eq!(
        reports.values().next().unwrap().quantity,
        Quantity::from("4"),
        "the report must carry the reduced size, not the venue's original stake",
    );
    h.pump_for(Duration::from_millis(300)).await;

    let settled = h.cache.borrow();
    let settled_order = settled.order(&order.client_order_id()).unwrap().clone();
    assert_eq!(settled_order.quantity(), Quantity::from("4"));
    assert_eq!(
        event_count(&settled_order, |event| matches!(
            event,
            OrderEventAny::Updated(_)
        )),
        1,
        "a resolved reduction must not update again",
    );
}

#[rstest]
#[tokio::test]
async fn submit_venue_error_rejects_and_stays_out_of_book() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    // The venue rejects the placement: the instruction report fails, so the adapter emits
    // OrderRejected and the order never enters the own order book.
    h.override_betting_result(METHOD_PLACE_ORDERS, "rest/betting_place_order_error.json");
    h.submit_via_risk(&order);
    let rejected = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Rejected)
        })
        .await;
    assert!(rejected, "order did not reach Rejected via routed events");

    harness::invariants::assert_tracked_used_events(&h.routed);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Rejected,
    );
    harness::invariants::assert_in_own_book(
        &h.cache.borrow(),
        &h.instrument_id,
        &order.client_order_id(),
        false,
    );
}

#[rstest]
#[tokio::test]
async fn startup_reconcile_correlates_open_order() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    // Startup reconcile: listCurrentOrders shows the order still open. The report path
    // parses it end-to-end and correlates to the cached order without spurious events.
    h.override_betting_result(
        METHOD_LIST_CURRENT_ORDERS,
        "rest/list_current_orders_harness_open.json",
    );
    let mass_status = h.reconcile_from_venue().await;

    let reports = mass_status.order_reports();
    assert_eq!(reports.len(), 1, "expected one order status report");
    let report = reports.values().next().unwrap();
    assert_eq!(report.venue_order_id, VenueOrderId::from("228302937743"));
    assert_eq!(report.order_status, OrderStatus::Accepted);
    assert_eq!(report.client_order_id, Some(order.client_order_id()));

    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Accepted,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}

#[rstest]
#[tokio::test]
async fn reconcile_applies_canceled_while_pending_cancel() {
    let mut h = harness::Harness::build().await;
    let order = harness::limit_order(&h.instrument_id, "O-1");

    h.submit_via_risk(&order);
    let accepted = h
        .pump_until(DEADLINE, |cache| {
            order_reached(cache, &order, OrderStatus::Accepted)
        })
        .await;
    assert!(accepted, "order did not reach Accepted");

    // Stage the missed cancel: the order is locally PendingCancel, but the live cancel
    // event is withheld (no OCM frame is fed).
    h.mark_pending_cancel(&order);
    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::PendingCancel,
    );

    // Reconciliation returns the venue Canceled report for the order's current bet id. A
    // confirmed Canceled is authoritative and applies even while locally PendingCancel.
    h.override_betting_result(
        METHOD_LIST_CURRENT_ORDERS,
        "rest/list_current_orders_harness_canceled.json",
    );
    let mass_status = h.reconcile_from_venue().await;

    let reports = mass_status.order_reports();
    let report = reports.values().next().unwrap();
    assert_eq!(report.order_status, OrderStatus::Canceled);

    harness::invariants::assert_order_status(
        &h.cache.borrow(),
        &order.client_order_id(),
        OrderStatus::Canceled,
    );
    harness::invariants::assert_own_book_consistent(&h.cache.borrow(), &h.instrument_id);
}
