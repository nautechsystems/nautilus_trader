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

//! Execution client tests against a mock of the Kalshi order API.

use std::{rc::Rc, time::Duration};

use axum::http::StatusCode;
use nautilus_common::{
    cache::CacheView,
    clients::ExecutionClient,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{CancelOrder, ModifyOrder, QueryAccount},
    },
};
use nautilus_core::UUID4;
use nautilus_model::{
    enums::{OrderSide, OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientOrderId, StrategyId, TradeId, TraderId, VenueOrderId},
    orders::builder::OrderTestBuilder,
    types::{Money, Price, Quantity},
};
use rust_decimal_macros::dec;

use crate::{
    harness::{
        CLIENT_ORDER_ID, FILL_JSON, TICKER, VENUE_ORDER_ID, add_ask, cache_handle, cached_order,
        decimal, events_within, execution_client, fill_json, instrument, instrument_id,
        limit_order, next_event, next_order_event, next_report, order_json, reported_fill_ids,
        start_client, status_reports, submit, ts_init,
    },
    mock_venue::{MockVenue, spawn_mock},
};

#[tokio::test]
async fn test_submit_reports_accepted_and_reads_back_its_fills() {
    let venue = MockVenue::new(
        order_json("executed", "100.00", "0.00"),
        vec![FILL_JSON.to_string()],
    );
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();

    client.submit_order(submit(&order)).unwrap();

    let OrderEventAny::Submitted(submitted) = next_order_event(&mut rx).await else {
        panic!("expected an order submitted event");
    };

    assert_eq!(
        submitted.client_order_id,
        ClientOrderId::from(CLIENT_ORDER_ID)
    );

    let OrderEventAny::Accepted(accepted) = next_order_event(&mut rx).await else {
        panic!("expected an order accepted event");
    };

    assert_eq!(accepted.venue_order_id, VenueOrderId::from(VENUE_ORDER_ID));

    // The exchange reports counts rather than fills, so the fills are read back and reported as order
    // events the engine can apply.
    let OrderEventAny::Filled(filled) = next_order_event(&mut rx).await else {
        panic!("expected an order filled event");
    };

    assert_eq!(filled.trade_id, TradeId::from("fill-1"));
    assert_eq!(filled.last_qty, Quantity::from("100.00"));
    assert_eq!(filled.last_px, Price::from("0.34"));
    assert_eq!(filled.commission, Some(Money::from("0.10 USD")));

    let creates = venue.creates();

    assert_eq!(creates.len(), 1);

    let request = &creates[0];

    assert_eq!(request["ticker"], TICKER);
    assert_eq!(request["side"], "bid");
    assert_eq!(request["time_in_force"], "good_till_canceled");
    assert_eq!(request["client_order_id"], CLIENT_ORDER_ID);
    assert_eq!(decimal(&request["count"]), dec!(100));
    assert_eq!(decimal(&request["price"]), dec!(0.34));
    assert!(venue.signed("POST /trade-api/v2/portfolio/events/orders"));
}

#[tokio::test]
async fn test_market_order_takes_the_far_side_of_the_book_and_never_rests() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    add_ask(&cache, "0.55", "100.00");
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id())
        .client_order_id(ClientOrderId::from(CLIENT_ORDER_ID))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100.00"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();

    client.submit_order(submit(&order)).unwrap();

    loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(_)) = next_event(&mut rx).await {
            break;
        }
    }

    // The exchange has no market order, so one is sent at the far side of the book: it must be
    // immediately-or-canceled, because whatever the far side does not fill would otherwise work.
    let creates = venue.creates();

    assert_eq!(creates.len(), 1);
    assert_eq!(creates[0]["time_in_force"], "immediate_or_cancel");
    assert_eq!(decimal(&creates[0]["price"]), dec!(0.55));
}

#[tokio::test]
async fn test_market_order_without_a_book_is_denied_without_a_request() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(instrument_id())
        .client_order_id(ClientOrderId::from(CLIENT_ORDER_ID))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("100.00"))
        .time_in_force(TimeInForce::Ioc)
        .build();
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();

    client.submit_order(submit(&order)).unwrap();

    // A market order is sent at the far side of the book, and an empty book leaves that price
    // unknown: the order is refused here rather than sent at a price the exchange rejects.
    let OrderEventAny::Denied(denied) = next_order_event(&mut rx).await else {
        panic!("expected an order denied event");
    };

    assert!(denied.reason.contains("far side"), "{}", denied.reason);
    assert!(venue.creates().is_empty());
    assert_eq!(client.tracked_orders(), 0);
}

#[tokio::test]
async fn test_day_order_is_denied_without_a_request() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Day, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();

    client.submit_order(submit(&order)).unwrap();

    // The exchange has no day order: a resting order ends when it is canceled or its market closes.
    let OrderEventAny::Denied(denied) = next_order_event(&mut rx).await else {
        panic!("expected an order denied event");
    };

    assert!(denied.reason.contains("DAY"), "{}", denied.reason);
    assert!(venue.creates().is_empty());
}

#[tokio::test]
async fn test_an_unanswered_submission_is_not_reported_as_rejected() {
    let venue = MockVenue::resting();
    venue
        .create_responses
        .lock()
        .push_back(StatusCode::INTERNAL_SERVER_ERROR);
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();

    client.submit_order(submit(&order)).unwrap();

    // The exchange did not answer, which is not the same as refusing: a rejection would tell the
    // engine the order never existed while it can still be working at the venue.
    let events = events_within(&mut rx, Duration::from_millis(200)).await;

    assert!(
        !events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Order(OrderEventAny::Rejected(_)))),
        "an unanswered submission must not be reported as rejected: {events:?}"
    );
}

#[tokio::test]
async fn test_cancel_order_reports_canceled_and_reads_back_a_fill_before_it() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 20, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.submit_order(submit(&order)).unwrap();

    let accepted = loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) = next_event(&mut rx).await
        {
            break accepted;
        }
    };

    assert_eq!(accepted.venue_order_id, VenueOrderId::from(VENUE_ORDER_ID));
    assert_eq!(client.tracked_orders(), 1);

    client
        .cancel_order(CancelOrder::new(
            TraderId::from("TESTER-001"),
            None,
            StrategyId::from("S-001"),
            instrument_id(),
            ClientOrderId::from(CLIENT_ORDER_ID),
            Some(VenueOrderId::from(VENUE_ORDER_ID)),
            UUID4::new(),
            ts_init(),
            None,
            None,
        ))
        .unwrap();

    let OrderEventAny::Canceled(canceled) = next_order_event(&mut rx).await else {
        panic!("expected an order canceled event");
    };

    assert_eq!(
        canceled.venue_order_id,
        Some(VenueOrderId::from(VENUE_ORDER_ID))
    );
    assert!(venue.signed("DELETE /trade-api/v2/portfolio/events/orders/order-1"));

    // Ten contracts fill before the cancellation reaches the exchange. The cancellation receipt only
    // reports what it reduced, so the fill has to come from the poll that follows it.
    venue.add_fill(&fill_json("fill-1", "10.00", "2025-01-01T12:00:05Z"));
    venue.set_order(order_json("canceled", "10.00", "90.00"));

    let events = events_within(&mut rx, Duration::from_millis(300)).await;

    assert_eq!(
        reported_fill_ids(&events),
        vec![TradeId::from("fill-1")],
        "the fill that preceded the cancellation is reported once"
    );
    assert!(
        status_reports(&events)
            .iter()
            .any(|report| report.order_status == OrderStatus::Canceled),
        "the terminal state is reported after the cancellation"
    );
    assert_eq!(client.tracked_orders(), 0);
}

#[tokio::test]
async fn test_poll_reports_a_state_change_and_its_fill_once() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 20, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.submit_order(submit(&order)).unwrap();

    // The submission response carries counts rather than fills, so wait for the accepted answer
    // before the exchange has anything to report.
    loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(_)) = next_event(&mut rx).await {
            break;
        }
    }

    // The submission path reads the fills behind an accepted order, so let that read happen while
    // the exchange has none before introducing a fill the order read does not account for.
    let _ = events_within(&mut rx, Duration::from_millis(150)).await;
    venue.add_fill(FILL_JSON);
    let early = events_within(&mut rx, Duration::from_millis(200)).await;

    assert_eq!(
        reported_fill_ids(&early),
        Vec::<TradeId>::new(),
        "a fill the order read does not account for is not bundled with it"
    );

    // The exchange now reports the execution, which covers the fill.
    venue.set_order(order_json("executed", "100.00", "0.00"));
    let events = events_within(&mut rx, Duration::from_millis(400)).await;
    let reports = status_reports(&events);

    assert_eq!(
        reported_fill_ids(&events),
        vec![TradeId::from("fill-1")],
        "the fill is reported once, not on every poll"
    );
    assert!(
        reports
            .iter()
            .any(|report| report.order_status == OrderStatus::Filled),
        "the order is reported as filled"
    );
    // A bundled report is read as a snapshot, so it may never carry more than it accounts for: a
    // consumer voids the difference.
    for event in &events {
        if let ExecutionEvent::Report(ExecutionReport::OrderWithFills(status, fills)) = event {
            let bundled: Quantity = fills.iter().fold(
                Quantity::zero(status.filled_qty.precision),
                |total, fill| total + fill.last_qty,
            );

            assert!(
                bundled <= status.filled_qty,
                "a snapshot bundles {bundled} of {} filled contracts",
                status.filled_qty
            );
        }
    }
    assert_eq!(client.tracked_orders(), 0);
}

#[tokio::test]
async fn test_poll_tolerates_an_order_the_venue_has_not_published_yet() {
    let venue = MockVenue::resting();
    venue
        .order_responses
        .lock()
        .push_back(StatusCode::NOT_FOUND);
    venue
        .order_responses
        .lock()
        .push_back(StatusCode::NOT_FOUND);
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 20, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.submit_order(submit(&order)).unwrap();

    // The venue's read path lags its write path, so an order it accepted can be missing from a read:
    // the client keeps polling rather than dropping the order and its fills.
    let report = next_report(&mut rx).await;

    assert!(matches!(report, ExecutionReport::Order(_)));
    assert_eq!(client.tracked_orders(), 1);

    // An order that never appears is eventually dropped, because polling it forever is not tracking.
    venue.order_responses.lock().clear();
    for _ in 0..8 {
        venue
            .order_responses
            .lock()
            .push_back(StatusCode::NOT_FOUND);
    }
    let _ = events_within(&mut rx, Duration::from_millis(400)).await;

    assert_eq!(client.tracked_orders(), 0);
}

#[tokio::test]
async fn test_poll_retries_a_fill_read_that_failed() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 20, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.submit_order(submit(&order)).unwrap();

    loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(_)) = next_event(&mut rx).await {
            break;
        }
    }

    // The order fills and the read of its fills fails. The state must stay open, so the next poll
    // reads the fills again rather than leaving the engine to infer a fill without its metadata.
    venue
        .fill_responses
        .lock()
        .push_back(StatusCode::INTERNAL_SERVER_ERROR);
    venue.add_fill(FILL_JSON);
    venue.set_order(order_json("executed", "100.00", "0.00"));

    let events = events_within(&mut rx, Duration::from_millis(500)).await;

    assert_eq!(
        reported_fill_ids(&events),
        vec![TradeId::from("fill-1")],
        "the fill is reported by the retry"
    );
    let fills = events
        .iter()
        .filter_map(|event| match event {
            ExecutionEvent::Report(ExecutionReport::Fill(fill)) => Some(fill.as_ref().clone()),
            ExecutionEvent::Report(ExecutionReport::OrderWithFills(_, fills)) => {
                Some(fills[0].clone())
            }
            _ => None,
        })
        .collect::<Vec<_>>();

    if let Some(fill) = fills.first() {
        assert_eq!(fill.commission, Money::from("0.10 USD"));
    }
}

#[tokio::test]
async fn test_modify_order_amends_the_total_count_and_price() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    client.on_instrument(instrument());
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.submit_order(submit(&order)).unwrap();

    loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(_)) = next_event(&mut rx).await {
            break;
        }
    }

    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            None,
            StrategyId::from("S-001"),
            instrument_id(),
            ClientOrderId::from(CLIENT_ORDER_ID),
            Some(VenueOrderId::from(VENUE_ORDER_ID)),
            Some(Quantity::from("50.00")),
            Some(Price::from("0.36")),
            None,
            UUID4::new(),
            ts_init(),
            None,
            None,
        ))
        .unwrap();

    let OrderEventAny::Updated(updated) = next_order_event(&mut rx).await else {
        panic!("expected an order updated event");
    };

    assert_eq!(updated.quantity, Quantity::from("50.00"));
    assert_eq!(updated.price, Some(Price::from("0.36")));
    assert!(venue.signed("POST /trade-api/v2/portfolio/events/orders/order-1/amend"));
}

#[tokio::test]
async fn test_query_account_reports_cash_plus_positions_as_the_total() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue).await;
    let cache = cache_handle();
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    // Connecting reports the balance it read to prove the credential, so the account exists before
    // any strategy trades against it.
    let ExecutionEvent::Account(connected) = next_event(&mut rx).await else {
        panic!("expected the account state read at connect");
    };

    assert_eq!(connected.balances[0].free, Money::from("4125.00 USD"));

    client
        .query_account(QueryAccount::new(
            TraderId::from("TESTER-001"),
            None,
            AccountId::from("KALSHI-001"),
            UUID4::new(),
            ts_init(),
            None,
            None,
        ))
        .unwrap();

    let ExecutionEvent::Account(state) = next_event(&mut rx).await else {
        panic!("expected an account state event");
    };
    let balance = state.balances[0];

    // 4125.00 USD of cash and 5000.00 USD of open positions, with the cash free to trade.
    assert_eq!(balance.total, Money::from("9125.00 USD"));
    assert_eq!(balance.free, Money::from("4125.00 USD"));
    assert_eq!(balance.locked, Money::from("5000.00 USD"));
}

#[tokio::test]
async fn test_connect_adopts_the_members_working_orders_and_positions() {
    let venue = MockVenue::new(
        order_json("resting", "50.00", "50.00"),
        vec![fill_json("fill-1", "50.00", "2025-01-01T12:00:05Z")],
    );
    venue
        .cutoff
        .lock()
        .replace("2025-01-01T00:00:00Z".to_string());
    let addr = spawn_mock(venue).await;
    let cache = cache_handle();
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, true);
    let mut rx = start_client(&mut client);

    client.connect().await.unwrap();

    let events = events_within(&mut rx, Duration::from_millis(300)).await;
    // The account already holds a working order, a fill behind it, and a position: a session that
    // starts against it has to adopt all three.
    let adopted = status_reports(&events)
        .into_iter()
        .find(|report| report.venue_order_id == VenueOrderId::from(VENUE_ORDER_ID))
        .expect("the working order is reported");

    assert_eq!(adopted.order_status, OrderStatus::PartiallyFilled);
    assert_eq!(adopted.filled_qty, Quantity::from("50.00"));

    assert_eq!(adopted.venue_order_id, VenueOrderId::from(VENUE_ORDER_ID));
    // A limit order cannot be built without its price, so the report carries the venue's.
    assert_eq!(adopted.price, Some(Price::from("0.34")));
    assert_eq!(reported_fill_ids(&events), vec![TradeId::from("fill-1")]);
    assert!(
        events
            .iter()
            .any(|event| matches!(event, ExecutionEvent::Report(ExecutionReport::Position(_)))),
        "the position is reported"
    );
    // A working order the platform placed carries a client order identifier, so it is tracked and
    // the poll task keeps it current.
    assert_eq!(client.tracked_orders(), 1);
}

#[tokio::test]
async fn test_restart_resumes_polling_and_commands() {
    let venue = MockVenue::resting();
    let addr = spawn_mock(venue.clone()).await;
    let cache = cache_handle();
    let order = limit_order(OrderSide::Buy, TimeInForce::Gtc, CLIENT_ORDER_ID);
    cached_order(&cache, &order);
    let mut client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);
    let mut rx = start_client(&mut client);
    client.connect().await.unwrap();
    client.stop().unwrap();
    client.start().unwrap();

    // A stopped client closes its task group; a restarted one has to issue requests again.
    client.submit_order(submit(&order)).unwrap();

    loop {
        if let ExecutionEvent::Order(OrderEventAny::Accepted(_)) = next_event(&mut rx).await {
            break;
        }
    }

    assert_eq!(venue.creates().len(), 1);
}

#[tokio::test]
async fn test_generate_mass_status_reports_the_venue_state() {
    let venue = MockVenue::new(
        order_json("resting", "0.00", "100.00"),
        vec![FILL_JSON.to_string()],
    );
    venue
        .cutoff
        .lock()
        .replace("2024-01-01T00:00:00Z".to_string());
    let addr = spawn_mock(venue).await;
    let cache = cache_handle();
    let client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);

    // A window inside the venue's live data tier is covered in full.
    let mass_status = client
        .generate_mass_status(Some(1_000_000))
        .await
        .unwrap()
        .expect("a mass status");

    assert_eq!(mass_status.order_reports().len(), 1);
    assert_eq!(mass_status.fill_reports().len(), 1);
    assert_eq!(mass_status.position_reports().len(), 1);
    assert_eq!(
        mass_status.order_reports()[&VenueOrderId::from(VENUE_ORDER_ID)].order_status,
        OrderStatus::Accepted
    );
    assert_eq!(
        mass_status.order_reports()[&VenueOrderId::from(VENUE_ORDER_ID)].price,
        Some(Price::from("0.34"))
    );
    assert!(mass_status.lookback_start().is_some());
    assert!(mass_status.reports_complete());
}

#[tokio::test]
async fn test_mass_status_before_the_venue_cutoff_is_not_complete() {
    let venue = MockVenue::new(order_json("canceled", "0.00", "100.00"), Vec::new());
    // A cutoff in the future stands for any window the live endpoints cannot cover.
    venue
        .cutoff
        .lock()
        .replace("2100-01-01T00:00:00Z".to_string());
    let addr = spawn_mock(venue).await;
    let cache = cache_handle();
    let client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);

    let mass_status = client
        .generate_mass_status(Some(1_000_000))
        .await
        .unwrap()
        .expect("a mass status");

    // The venue holds orders and fills older than the window's start in its historical tier, which
    // this client does not read, so the report set is not a full picture of the window.
    assert!(!mass_status.reports_complete());
    // A completed order is still reported, so an order the client held while it was offline can be
    // reconciled.
    assert_eq!(
        mass_status.order_reports()[&VenueOrderId::from(VENUE_ORDER_ID)].order_status,
        OrderStatus::Canceled
    );
}

#[tokio::test]
async fn test_unbounded_mass_status_is_not_complete() {
    let venue = MockVenue::new(
        order_json("resting", "0.00", "100.00"),
        vec![FILL_JSON.to_string()],
    );
    venue
        .cutoff
        .lock()
        .replace("2025-01-01T00:00:00Z".to_string());
    let addr = spawn_mock(venue).await;
    let cache = cache_handle();
    let client = execution_client(addr, CacheView::new(Rc::clone(&cache)), 60_000, false);

    let mass_status = client
        .generate_mass_status(None)
        .await
        .unwrap()
        .expect("a mass status");

    // Without a window the request is for the whole history, which the live endpoints cannot return.
    assert!(mass_status.lookback_start().is_none());
    assert!(!mass_status.reports_complete());
}
