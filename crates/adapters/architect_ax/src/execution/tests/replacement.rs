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

#[path = "../../../tests/integration/common/execution.rs"]
mod execution;
#[path = "../../../tests/integration/common/server.rs"]
mod server;

use std::time::Duration;

use execution::{add_test_account_to_cache, create_test_execution_client, drain_rx};
use nautilus_common::{
    clients::ExecutionClient,
    messages::{
        ExecutionEvent, ExecutionReport,
        execution::{CancelOrder, GenerateOrderStatusReport, ModifyOrder, QueryOrder},
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_model::{
    enums::{OrderStatus, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{AccountId, ClientOrderId, InstrumentId, StrategyId, TraderId, VenueOrderId},
    types::{Price, Quantity},
};
use rstest::rstest;
use server::{load_test_data, start_test_server};

use crate as adapter;
use crate::common::consts::AX_CLIENT_ID;

#[rstest]
#[case::missing_stream(true, false, false, "ACCEPTED", Some(OrderStatus::Accepted), false)]
#[case::late_stream(true, true, false, "ACCEPTED", Some(OrderStatus::Accepted), false)]
#[case::missing_child(false, true, false, "ACCEPTED", None, false)]
#[case::parent_changes_during_query(
    true,
    false,
    true,
    "ACCEPTED",
    Some(OrderStatus::Accepted),
    false
)]
#[case::filled(true, false, false, "FILLED", Some(OrderStatus::Filled), false)]
#[case::canceled(true, false, false, "CANCELED", Some(OrderStatus::Canceled), false)]
#[case::expired(true, false, false, "EXPIRED", Some(OrderStatus::Expired), false)]
#[case::rejected_proposal(true, true, false, "REJECTED", None, false)]
#[case::retired_proposal(true, true, false, "REPLACED", None, false)]
#[case::report_then_reconnect(true, false, false, "ACCEPTED", Some(OrderStatus::Accepted), true)]
#[tokio::test]
async fn test_replacement_resolution_preserves_child(
    #[case] child_found: bool,
    #[case] late_stream: bool,
    #[case] changing_parent: bool,
    #[case] candidate_state: &str,
    #[case] expected: Option<OrderStatus>,
    #[case] reconnect: bool,
) {
    let (addr, state) = start_test_server().await.unwrap();
    let mut replacement = load_test_data("ws_order_replaced_live.json");
    let parent = VenueOrderId::from("OID-QUERY-PARENT");
    let proposed = VenueOrderId::from("OID-QUERY-PROPOSAL");
    replacement["ro"]["oid"] = parent.to_string().into();
    replacement["noid"] = proposed.to_string().into();
    replacement["no"]["oid"] = proposed.to_string().into();
    let mut candidate_order = replacement["no"].clone();

    let filled = if expected == Some(OrderStatus::Filled) {
        100
    } else {
        0
    };

    let remaining = if candidate_state == "ACCEPTED" {
        100
    } else {
        0
    };

    candidate_order["o"] = candidate_state.into();
    candidate_order["xq"] = filled.into();
    candidate_order["rq"] = remaining.into();

    if child_found && expected.is_none() {
        replacement["noid"] = "OID-ACTUAL-CHILD".into();
        replacement["no"]["oid"] = "OID-ACTUAL-CHILD".into();
        replacement["no"]["p"] = "1.0927".into();
    }

    let child = VenueOrderId::from(replacement["noid"].as_str().unwrap());
    let expected_price = Price::from(replacement["no"]["p"].as_str().unwrap());
    *state.replace_order_oid.lock().await = Some(proposed.to_string());
    let mut parent_status = load_test_data("captured/order-status-replaced.json");
    parent_status["status"]["order_id"] = parent.to_string().into();
    let mut child_status = load_test_data("http_get_order_status.json");
    child_status["status"]["order_id"] = proposed.to_string().into();
    child_status["status"]["state"] = candidate_state.into();
    child_status["status"]["filled_quantity"] = filled.into();
    child_status["status"]["remaining_quantity"] = remaining.into();
    if changing_parent {
        let mut accepted = child_status.clone();
        accepted["status"]["order_id"] = parent.to_string().into();
        state
            .order_status_responses
            .lock()
            .await
            .push((parent.to_string(), 200, accepted));
    }

    state.order_status_responses.lock().await.extend(vec![
        (parent.to_string(), 200, parent_status),
        (
            proposed.to_string(),
            if child_found { 200 } else { 404 },
            if child_found {
                child_status
            } else {
                serde_json::json!({"error":"no matching orders"})
            },
        ),
    ]);
    *state.orders_payload.lock().await =
        Some(serde_json::json!({"orders":[replacement["ro"].clone(), candidate_order]}));
    *state.fills_payload.lock().await = Some(serde_json::json!({"fills":[]}));
    let (mut client, mut rx, cache) = create_test_execution_client(addr);
    add_test_account_to_cache(&cache, AccountId::from("AX-001"));
    client.start().unwrap();
    client.connect().await.unwrap();
    drain_rx(&mut rx);
    let client_order_id = ClientOrderId::from("O-QUERY-REPLACE");
    let instrument_id = InstrumentId::from("EURUSD-PERP.AX");
    let strategy_id = StrategyId::from("S-001");
    client.register_external_order(
        client_order_id,
        parent,
        instrument_id,
        strategy_id,
        UnixNanos::default(),
    );
    client
        .modify_order(ModifyOrder::new(
            TraderId::from("TESTER-001"),
            Some(*AX_CLIENT_ID),
            strategy_id,
            instrument_id,
            client_order_id,
            Some(parent),
            None,
            Some(Price::from("1.0926")),
            None,
            UUID4::new(),
            UnixNanos::default(),
            None,
            None,
        ))
        .unwrap();
    wait_until_async(
        || async { client.pending_tasks.all_finished() },
        Duration::from_secs(5),
    )
    .await;
    assert_eq!(
        client
            .ws_orders
            .caches()
            .venue_to_client_id
            .get(&proposed)
            .map(|id| *id),
        Some(client_order_id),
    );

    if reconnect {
        let command = GenerateOrderStatusReport::new(
            UUID4::new(),
            UnixNanos::default(),
            Some(instrument_id),
            Some(client_order_id),
            Some(parent),
            None,
            None,
        );
        let report = client
            .generate_order_status_report(&command)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(report.venue_order_id, parent);
        assert_eq!(report.order_status, OrderStatus::Canceled);
        client.disconnect().await.unwrap();
        client.connect().await.unwrap();
        drain_rx(&mut rx);
    }

    let command = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*AX_CLIENT_ID),
        strategy_id,
        instrument_id,
        client_order_id,
        Some(parent),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None,
    );

    client.query_order(command.clone()).unwrap();

    if changing_parent {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("expected parent report, was {event:?}");
        };

        assert_eq!(report.venue_order_id, parent);
        assert_eq!(report.order_status, OrderStatus::Accepted);
        client.query_order(command).unwrap();
    }

    if let Some(expected) = expected {
        let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
            .await
            .unwrap()
            .unwrap();

        let ExecutionEvent::Report(ExecutionReport::Order(report)) = event else {
            panic!("expected replacement report, was {event:?}");
        };

        assert_eq!(report.account_id, AccountId::from("AX-001"));
        assert_eq!(report.client_order_id, Some(client_order_id));
        assert_eq!(report.venue_order_id, child);
        assert_eq!(report.instrument_id, instrument_id);
        assert_eq!(report.order_status, expected);
        assert_eq!(report.price, Some(expected_price));
        assert_eq!(report.quantity, Quantity::from("100"));
        assert_eq!(
            report.filled_qty,
            Quantity::from(filled.to_string().as_str())
        );
    } else {
        wait_until_async(
            || async { client.pending_tasks.all_finished() },
            Duration::from_secs(5),
        )
        .await;
        let report = client
            .http_client
            .request_order_status(
                AccountId::from("AX-001"),
                instrument_id,
                Some(client_order_id),
                Some(parent),
                None,
                OrderType::Limit,
                TimeInForce::Gtc,
            )
            .await
            .unwrap();
        let resolved = super::super::resolve_order_status_report(
            &client.http_client,
            client.ws_orders.caches(),
            client_order_id,
            report,
        )
        .await
        .unwrap();

        assert!(resolved.is_none());
        assert_eq!(
            client
                .ws_orders
                .orders_metadata()
                .get(&client_order_id)
                .unwrap()
                .venue_order_id,
            Some(parent),
        );
        assert!(matches!(
            rx.try_recv(),
            Err(tokio::sync::mpsc::error::TryRecvError::Empty)
        ));
    }

    assert!(
        state
            .order_status_queries
            .lock()
            .await
            .iter()
            .any(|query| { query.order_id.as_deref() == Some(proposed.as_str()) })
    );

    if late_stream {
        state.order_injected_events.lock().await.push(replacement);

        let update = tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                let event = rx.recv().await.unwrap();
                match event {
                    ExecutionEvent::Order(OrderEventAny::Updated(update)) => break update,
                    ExecutionEvent::Report(ExecutionReport::Order(report)) => {
                        assert_eq!(report.order_status, OrderStatus::Accepted);
                    }
                    other => panic!("unexpected event {other:?}"),
                }
            }
        })
        .await
        .unwrap();

        assert_eq!(update.client_order_id, client_order_id);
        assert_eq!(update.venue_order_id, Some(child));
        assert_eq!(update.price, Some(expected_price));
        assert_eq!(update.quantity, Quantity::from("100"));
    }

    let closed = expected.is_some_and(OrderStatus::is_closed);
    if !closed {
        client
            .cancel_order(CancelOrder::new(
                TraderId::from("TESTER-001"),
                Some(*AX_CLIENT_ID),
                strategy_id,
                instrument_id,
                client_order_id,
                Some(parent),
                UUID4::new(),
                UnixNanos::default(),
                None,
                None,
            ))
            .unwrap();
        wait_until_async(
            || async {
                state
                    .get_messages()
                    .await
                    .iter()
                    .any(|message| message["t"] == "x")
            },
            Duration::from_secs(5),
        )
        .await;
    }

    let messages = state.get_messages().await;
    client.disconnect().await.unwrap();
    let cancels: Vec<_> = messages
        .iter()
        .filter(|message| message["t"] == "x")
        .collect();
    assert_eq!(cancels.len(), usize::from(!closed));

    if !closed {
        assert_eq!(cancels[0]["oid"], child.as_str());
    }

    while let Ok(event) = rx.try_recv() {
        if let ExecutionEvent::Report(ExecutionReport::Order(report)) = event {
            if report.venue_order_id == child {
                assert_eq!(
                    report.order_status,
                    expected.unwrap_or(OrderStatus::Accepted)
                );
            } else {
                assert!(closed);
                assert_eq!(report.venue_order_id, parent);
                assert_eq!(report.order_status, OrderStatus::Canceled);
            }
        } else {
            panic!("unexpected event {event:?}");
        }
    }
}
