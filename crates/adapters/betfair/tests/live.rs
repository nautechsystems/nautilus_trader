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

mod common;
mod harness;

use std::time::Duration;

use nautilus_betfair::common::consts::{
    METHOD_CANCEL_ORDERS, METHOD_LIST_CURRENT_ORDERS, METHOD_PLACE_ORDERS,
};
use nautilus_common::{actor::DataActor, cache::Cache};
use nautilus_model::{
    enums::OrderStatus,
    identifiers::VenueOrderId,
    orders::{Order, OrderAny},
    types::{Price, Quantity},
};
use rstest::rstest;
use rust_decimal::Decimal;

const DEADLINE: Duration = Duration::from_secs(5);

fn order_reached(cache: &Cache, order: &OrderAny, status: OrderStatus) -> bool {
    cache
        .order(&order.client_order_id())
        .is_some_and(|cached| cached.status() == status)
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
async fn modify_price_cancel_replace_promotes_bet_id() {
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

    harness::invariants::assert_tracked_used_events(&h.routed);
    let cache = h.cache.borrow();
    let updated = cache.order(&order.client_order_id()).unwrap();
    assert_eq!(updated.venue_order_id(), Some(new_venue_order_id));
    assert_eq!(updated.price(), Some(Price::from("5.0")));
    assert_eq!(updated.status(), OrderStatus::Accepted);
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
