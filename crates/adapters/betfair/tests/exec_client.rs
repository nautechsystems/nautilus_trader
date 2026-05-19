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

//! Integration tests for `BetfairExecutionClient`.

mod common;

use std::{
    cell::RefCell,
    net::SocketAddr,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use nautilus_betfair::{
    common::consts::{
        BETFAIR_CLIENT_ID, BETFAIR_VENUE, METHOD_CANCEL_ORDERS, METHOD_LIST_CURRENT_ORDERS,
        METHOD_PLACE_ORDERS,
    },
    config::BetfairExecConfig,
    execution::BetfairExecutionClient,
};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{set_data_event_sender, set_exec_event_sender},
    messages::{
        DataEvent, ExecutionEvent,
        execution::{
            ExecutionReport,
            cancel::{BatchCancelOrders, CancelAllOrders, CancelOrder},
            modify::ModifyOrder,
            query::QueryOrder,
            report::{GenerateFillReportsBuilder, GenerateOrderStatusReportsBuilder},
            submit::{SubmitOrder, SubmitOrderList},
        },
    },
    testing::wait_until_async,
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    data::Data,
    enums::{AccountType, OmsType, OrderSide, OrderType, TimeInForce},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientOrderId, InstrumentId, OrderListId, StrategyId, TraderId, VenueOrderId,
    },
    orders::{Order, OrderAny, OrderList, builder::OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

fn create_test_execution_client_with_config(
    addr: SocketAddr,
    stream_port: u16,
    config: BetfairExecConfig,
) -> (
    BetfairExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BETFAIR-001");
    let client_id = *BETFAIR_CLIENT_ID;
    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        *BETFAIR_VENUE,
        OmsType::Netting,
        account_id,
        AccountType::Betting,
        None,
        cache.clone(),
    );

    let http_client = create_test_http_client(addr);
    let currency = Currency::GBP();

    let (tx, rx) = tokio::sync::mpsc::unbounded_channel();
    set_exec_event_sender(tx);

    let (data_tx, data_rx) = tokio::sync::mpsc::unbounded_channel();
    set_data_event_sender(data_tx);

    let mut client = BetfairExecutionClient::new(
        core,
        http_client,
        test_credential(),
        plain_stream_config(stream_port),
        config,
        currency,
    );
    client.start().unwrap();

    (client, rx, data_rx, cache)
}

fn create_test_execution_client(
    addr: SocketAddr,
    stream_port: u16,
) -> (
    BetfairExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    Rc<RefCell<Cache>>,
) {
    create_test_execution_client_with_config(addr, stream_port, BetfairExecConfig::default())
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, _listener) = start_mock_stream().await;
    let (client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    assert_eq!(client.client_id(), *BETFAIR_CLIENT_ID);
    assert_eq!(client.account_id(), AccountId::from("BETFAIR-001"));
    assert_eq!(client.venue(), *BETFAIR_VENUE);
    assert_eq!(client.oms_type(), OmsType::Netting);
    assert!(!client.is_connected());
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_disconnect() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);
    let subscription_received = Arc::new(AtomicBool::new(false));
    let subscription_received_server = Arc::clone(&subscription_received);

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;

        // Capture the order subscription sent after the initial auth handshake
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        let json: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(json["op"], "orderSubscription");
        subscription_received_server.store(true, Ordering::Relaxed);

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    assert!(client.is_connected());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);

    wait_until_async(
        || {
            let subscription_received = Arc::clone(&subscription_received);
            async move { subscription_received.load(Ordering::Relaxed) }
        },
        Duration::from_secs(2),
    )
    .await;

    client.disconnect().await.unwrap();
    assert!(!client.is_connected());

    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_exec_client_connect_emits_account_state() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let mut found_account_state = false;

    while let Ok(event) = rx.try_recv() {
        if matches!(event, ExecutionEvent::Account(_)) {
            found_account_state = true;
            break;
        }
    }

    assert!(
        found_account_state,
        "Expected AccountState event during connect"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_emits_order_status_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        // Wait for the order subscription after the initial auth handshake
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for OCM event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Report(_)),
        "Expected Report event from OCM, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_voided_order_emits_data_event() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_VOIDED.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        // Wait for the order subscription after the initial auth handshake
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while data_rx.try_recv().is_ok() {}

    let event = tokio::time::timeout(Duration::from_secs(5), data_rx.recv())
        .await
        .expect("timeout waiting for voided data event")
        .expect("channel closed");

    assert!(
        matches!(event, DataEvent::Data(Data::Custom(_))),
        "Expected Custom data event for voided order, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

fn make_cancel_order(
    instrument_id: &str,
    client_order_id: &str,
    venue_order_id: &str,
) -> CancelOrder {
    CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        ClientOrderId::from(client_order_id),
        Some(VenueOrderId::from(venue_order_id)),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_bet_taken_or_lapsed_treated_as_success() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_bet_taken_or_lapsed.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-001", "1");
    client.cancel_order(cmd).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "BetTakenOrLapsed should not emit cancel rejected, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_instruction_failure_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_error.json");
    let mut v: Value = serde_json::from_str(&fixture).unwrap();
    v["result"]["instructionReports"][0]["errorMessage"] =
        Value::String("Betfair returned a detailed cancel validation error".to_string());
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-002", "1");
    client.cancel_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for cancel rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) => {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-002"));
            assert!(
                rejected
                    .reason
                    .as_str()
                    .contains("Betfair returned a detailed cancel validation error"),
                "Expected detailed Betfair error message, found: {}",
                rejected.reason,
            );
            assert!(rejected.reason.as_str().contains("ErrorInOrder"));
        }
        other => panic!("Expected CancelRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_result_failure_no_instructions_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_result_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-003", "1");
    client.cancel_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for cancel rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) => {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-003"));
            assert!(
                rejected.reason.as_str().contains("MarketSuspended"),
                "Expected MarketSuspended reason, found: {}",
                rejected.reason,
            );
        }
        other => panic!("Expected CancelRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_order_success_no_rejected_event() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_success.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-004", "1");
    client.cancel_order(cmd).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "Successful cancel should not emit rejected event, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

fn make_test_order(
    instrument_id: &str,
    client_order_id: &str,
    price: &str,
    quantity: &str,
) -> OrderAny {
    OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from(instrument_id))
        .client_order_id(ClientOrderId::from(client_order_id))
        .side(OrderSide::Sell)
        .price(Price::from(price))
        .quantity(Quantity::from(quantity))
        .time_in_force(TimeInForce::Gtc)
        .build()
}

fn add_order_to_cache(cache: &Rc<RefCell<Cache>>, order: OrderAny) {
    cache
        .borrow_mut()
        .add_order(order, None, Some(*BETFAIR_CLIENT_ID), false)
        .unwrap();
}

fn make_submit_order_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        None,
        UUID4::new(),
        UnixNanos::default(),
    )
}

#[rstest]
#[tokio::test]
async fn test_submit_order_success_emits_accepted() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-SUBMIT-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = make_submit_order_cmd(&order);
    client.submit_order(cmd).unwrap();

    // First event should be OrderSubmitted (emitted synchronously)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for submitted event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Order(OrderEventAny::Submitted(_))),
        "Expected OrderSubmitted event, found: {event:?}"
    );

    // Second event should be OrderAccepted (emitted after HTTP response)
    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for accepted event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Accepted(accepted)) => {
            assert_eq!(accepted.venue_order_id, VenueOrderId::from("228302937743"));
        }
        other => panic!("Expected OrderAccepted event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_submit_order_error_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_place_order_error.json");
    let mut v: Value = serde_json::from_str(&fixture).unwrap();
    v["result"]["instructionReports"][0]["errorMessage"] =
        Value::String("Betfair returned a detailed submit validation error".to_string());
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181106170-235-0.BETFAIR", "O-SUBMIT-002", "1.80", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = make_submit_order_cmd(&order);
    client.submit_order(cmd).unwrap();

    let _ = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for submitted")
        .expect("channel closed");

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for rejected event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Rejected(rejected)) => {
            assert_eq!(
                rejected.client_order_id,
                ClientOrderId::from("O-SUBMIT-002")
            );
            assert!(
                rejected
                    .reason
                    .as_str()
                    .contains("Betfair returned a detailed submit validation error"),
                "Expected detailed Betfair error message, found: {}",
                rejected.reason,
            );
            assert!(rejected.reason.as_str().contains("ErrorInOrder"));
        }
        other => panic!("Expected OrderRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_price_and_quantity_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-001"),
        Some(VenueOrderId::from("123")),
        Some(Quantity::from("5")),
        Some(Price::from("3.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for modify rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-MOD-001"));
            assert!(
                rejected
                    .reason
                    .as_str()
                    .contains("cannot modify price and quantity simultaneously"),
                "Expected simultaneous modify reason, found: {}",
                rejected.reason,
            );
        }
        other => panic!("Expected ModifyRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_modify_order_no_effective_change_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-002", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-002"),
        Some(VenueOrderId::from("123")),
        None,
        Some(Price::from("2.58")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for modify rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(rejected)) => {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-MOD-002"));
            assert!(
                rejected.reason.as_str().contains("no effective change"),
                "Expected no effective change reason, found: {}",
                rejected.reason,
            );
        }
        other => panic!("Expected ModifyRejected event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_all_orders_sends_request() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = CancelAllOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.cancel_all_orders(cmd).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_CANCEL_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "Cancel all should not emit rejected events on success, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_emits_cancel_event() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_CANCEL.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for OCM cancel event")
        .expect("channel closed");

    assert!(
        matches!(event, ExecutionEvent::Report(_)),
        "Expected Report event from OCM cancel, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_handles_mixed_updates() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_MIXED.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_count = 0;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(_))) => {
                report_count += 1;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        report_count >= 2,
        "Expected at least 2 Report events from MIXED OCM, found: {report_count}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_handler_handles_full_image() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FULL_IMAGE.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut found_report = false;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(_))) => {
                found_report = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(found_report, "Expected Report event from FULL_IMAGE OCM");

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_voided_partial_emits_both_fill_and_void() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_VOIDED_partial.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    while data_rx.try_recv().is_ok() {}

    // Should receive execution report (fill + status for sm=60)
    let mut found_report = false;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(_))) => {
                found_report = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    assert!(
        found_report,
        "Expected Report event for partially voided order"
    );

    // Should also receive Custom data event for BetfairOrderVoided (sv=40)
    let data_event = tokio::time::timeout(Duration::from_secs(3), data_rx.recv())
        .await
        .expect("timeout waiting for voided data event")
        .expect("channel closed");

    assert!(
        matches!(data_event, DataEvent::Data(Data::Custom(_))),
        "Expected Custom data event for voided portion, found: {data_event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_no_void_event_when_sv_zero() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, mut data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED_sv_zero.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}
    while data_rx.try_recv().is_ok() {}

    // Should receive execution report for the fill
    let mut found_report = false;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(_))) => {
                found_report = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    assert!(found_report, "Expected Report event for filled order");

    // Should NOT receive a void data event (sv=0)
    tokio::time::sleep(Duration::from_millis(500)).await;
    let data_event = data_rx.try_recv();
    assert!(
        data_event.is_err(),
        "Should not emit void event when sv=0, found: {data_event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_submit_order_registers_customer_order_ref() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-RFO-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = make_submit_order_cmd(&order);
    client.submit_order(cmd).unwrap();

    // Wait for submitted + accepted
    let _ = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;
    let _ = tokio::time::timeout(Duration::from_secs(5), rx.recv()).await;

    // Verify the mock server received the placeOrders call
    let has_place_orders = state
        .betting_methods
        .lock()
        .unwrap()
        .iter()
        .any(|m| m == METHOD_PLACE_ORDERS);
    assert!(has_place_orders, "Expected placeOrders call");

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_ocm_filled_no_avp_uses_order_price() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FILLED_no_avp.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    // Expect execution report (fill and/or status report)
    let mut found_report = false;

    for _ in 0..10 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(_))) => {
                found_report = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert!(
        found_report,
        "Expected Report event for no-avp filled order"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports() {
    let (addr, state) = start_mock_http().await;

    // Override listCurrentOrders to return executable orders
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();

    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert!(
        !reports.is_empty(),
        "Expected at least one order status report"
    );

    for report in &reports {
        assert!(!report.venue_order_id.to_string().is_empty());
        assert!(report.price.is_some());
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_generate_fill_reports() {
    let (addr, state) = start_mock_http().await;

    // Override listCurrentOrders to return executed orders with fills
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();

    let reports = client.generate_fill_reports(cmd).await.unwrap();

    assert!(
        !reports.is_empty(),
        "Expected at least one fill report from executed orders"
    );

    for report in &reports {
        assert!(report.last_qty.as_f64() > 0.0);
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_query_order_emits_order_status_report() {
    let (addr, state) = start_mock_http().await;

    // The fixture contains two executable orders on different markets.
    // query_order filters to the one matching the command's instrument_id.
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    // Drain connection events (account state, subscription acks)
    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let client_order_id = ClientOrderId::from("O-20260418-QUERY-001");
    let instrument_id = InstrumentId::from("1.180575118-39980.BETFAIR");
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("228059754671")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for query_order event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(report.venue_order_id.as_str(), "228059754671");
            assert_eq!(report.client_order_id, Some(client_order_id));
            assert_eq!(report.instrument_id, instrument_id);
        }
        other => panic!("Expected OrderStatusReport, was {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_query_order_no_match_emits_nothing() {
    let (addr, state) = start_mock_http().await;

    // Empty response: none of the lookups (ref, legacy ref, bet_id) return
    // any orders, so query_order must log-and-skip without emitting.
    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(3)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.180575118-39980.BETFAIR"),
        ClientOrderId::from("O-20260418-MISS"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_order(cmd).unwrap();

    // Nothing should be emitted. Give the spawned task time to run and
    // confirm no Report event lands.
    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(ExecutionReport::Order(_))) {
            report_seen = true;
            break;
        }
    }
    assert!(
        !report_seen,
        "query_order should not emit a report when no orders match",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

fn make_submit_order_list_cmd(
    instrument_id: &str,
    orders: &[OrderAny],
) -> (SubmitOrderList, OrderList) {
    let order_list = OrderList::new(
        OrderListId::from("OL-001"),
        InstrumentId::from(instrument_id),
        StrategyId::from("S-001"),
        orders.iter().map(OrderAny::client_order_id).collect(),
        UnixNanos::default(),
    );
    let order_inits = orders.iter().map(|o| o.init_event().clone()).collect();
    let cmd = SubmitOrderList::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        order_list.clone(),
        order_inits,
        None,
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None, // correlation_id
    );
    (cmd, order_list)
}

/// `submit_order_list` with the batch-success fixture must emit
/// OrderSubmitted + OrderAccepted for every leg, with each leg's
/// venue order id taken from the matching instruction report.
#[rstest]
#[tokio::test]
async fn test_submit_order_list_success_emits_accepted_for_each_leg() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_place_order_batch_success.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order1 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-LIST-001"))
        .order_list_id(OrderListId::from("OL-001"))
        .side(OrderSide::Sell)
        .price(Price::from("2.58"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-LIST-002"))
        .order_list_id(OrderListId::from("OL-001"))
        .side(OrderSide::Sell)
        .price(Price::from("3.00"))
        .quantity(Quantity::from("5"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    add_order_to_cache(&cache, order1.clone());
    add_order_to_cache(&cache, order2.clone());

    let (cmd, _order_list) = make_submit_order_list_cmd(
        "1.181005744-86362-0.BETFAIR",
        &[order1.clone(), order2.clone()],
    );
    client.submit_order_list(cmd).unwrap();

    let mut accepted_ids: Vec<(ClientOrderId, VenueOrderId)> = Vec::new();
    let mut submitted = 0;

    for _ in 0..6 {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Submitted(_)))) => submitted += 1,
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Accepted(a)))) => {
                accepted_ids.push((a.client_order_id, a.venue_order_id));
            }
            Ok(Some(_)) => {}
            _ => break,
        }

        if submitted >= 2 && accepted_ids.len() >= 2 {
            break;
        }
    }

    assert_eq!(submitted, 2, "expected one OrderSubmitted per leg");
    assert_eq!(accepted_ids.len(), 2, "expected one OrderAccepted per leg");

    accepted_ids.sort_by(|a, b| a.0.as_str().cmp(b.0.as_str()));
    assert_eq!(accepted_ids[0].0, ClientOrderId::from("O-LIST-001"));
    assert_eq!(accepted_ids[0].1, VenueOrderId::from("228302937743"));
    assert_eq!(accepted_ids[1].0, ClientOrderId::from("O-LIST-002"));
    assert_eq!(accepted_ids[1].1, VenueOrderId::from("228302937744"));

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `submit_order_list` with a partial-failure fixture must emit
/// OrderAccepted for the success leg and OrderRejected for the failure leg.
#[rstest]
#[tokio::test]
async fn test_submit_order_list_partial_failure_emits_mixed_events() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_place_order_batch_partial_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_PLACE_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order1 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-LIST-OK"))
        .order_list_id(OrderListId::from("OL-002"))
        .side(OrderSide::Sell)
        .price(Price::from("2.58"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-LIST-FAIL"))
        .order_list_id(OrderListId::from("OL-002"))
        .side(OrderSide::Sell)
        .price(Price::from("3.00"))
        .quantity(Quantity::from("5"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    add_order_to_cache(&cache, order1.clone());
    add_order_to_cache(&cache, order2.clone());

    let (cmd, _order_list) = make_submit_order_list_cmd(
        "1.181005744-86362-0.BETFAIR",
        &[order1.clone(), order2.clone()],
    );
    client.submit_order_list(cmd).unwrap();

    let mut accepted: Vec<ClientOrderId> = Vec::new();
    let mut rejected: Vec<ClientOrderId> = Vec::new();

    for _ in 0..6 {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Accepted(a)))) => {
                accepted.push(a.client_order_id);
            }
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Rejected(r)))) => {
                rejected.push(r.client_order_id);
            }
            Ok(Some(_)) => {}
            _ => break,
        }

        if !accepted.is_empty() && !rejected.is_empty() {
            break;
        }
    }

    assert_eq!(accepted, vec![ClientOrderId::from("O-LIST-OK")]);
    assert_eq!(rejected, vec![ClientOrderId::from("O-LIST-FAIL")]);

    client.disconnect().await.unwrap();
    let _ = server.await;
}

fn make_batch_cancel_cmd(
    instrument_id: &str,
    cancels: Vec<(ClientOrderId, Option<VenueOrderId>)>,
) -> BatchCancelOrders {
    let cancel_orders: Vec<CancelOrder> = cancels
        .into_iter()
        .map(|(client_oid, venue_oid)| {
            CancelOrder::new(
                TraderId::from("TESTER-001"),
                Some(*BETFAIR_CLIENT_ID),
                StrategyId::from("S-001"),
                InstrumentId::from(instrument_id),
                client_oid,
                venue_oid,
                UUID4::new(),
                UnixNanos::default(),
                None,
                None, // correlation_id
            )
        })
        .collect();
    BatchCancelOrders::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        cancel_orders,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    )
}

/// A batch-cancel that succeeds for every leg must not emit any
/// CancelRejected events; the venue acknowledges via the OCM stream.
#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_success_no_rejected_events() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_batch_success.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (ClientOrderId::from("O-BC-1"), Some(VenueOrderId::from("1"))),
            (ClientOrderId::from("O-BC-2"), Some(VenueOrderId::from("2"))),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_CANCEL_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "Successful batch-cancel should not emit rejected events, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A batch-cancel where one leg fails must emit CancelRejected for the
/// failing leg only, leaving the successful leg alone.
#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_partial_failure_emits_rejected_for_failing_leg() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_batch_partial_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![
            (
                ClientOrderId::from("O-BC-OK"),
                Some(VenueOrderId::from("1")),
            ),
            (
                ClientOrderId::from("O-BC-FAIL"),
                Some(VenueOrderId::from("2")),
            ),
        ],
    );
    client.batch_cancel_orders(cmd).unwrap();

    let mut rejected_ids: Vec<ClientOrderId> = Vec::new();

    for _ in 0..4 {
        match tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::CancelRejected(r)))) => {
                rejected_ids.push(r.client_order_id);
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert_eq!(
        rejected_ids,
        vec![ClientOrderId::from("O-BC-FAIL")],
        "Only the failing leg must be rejected"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `batch_cancel_orders` synthesises a CancelRejected immediately for any
/// leg missing a `venue_order_id` and skips that leg in the venue request.
#[rstest]
#[tokio::test]
async fn test_batch_cancel_orders_missing_venue_id_emits_rejected_locally() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let cmd = make_batch_cancel_cmd(
        "1.179082386-235-0.BETFAIR",
        vec![(ClientOrderId::from("O-BC-NO-ID"), None)],
    );
    client.batch_cancel_orders(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(2), rx.recv())
        .await
        .expect("timeout waiting for CancelRejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(rej)) => {
            assert_eq!(rej.client_order_id, ClientOrderId::from("O-BC-NO-ID"));
            assert!(
                rej.reason.as_str().contains("no venue_order_id"),
                "expected missing venue_order_id reason, was: {}",
                rej.reason,
            );
        }
        other => panic!("Expected CancelRejected, was {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A modify with quantity reduction (no price change) must succeed without
/// emitting a ModifyRejected; the venue acknowledges via the OCM stream.
#[rstest]
#[tokio::test]
async fn test_modify_order_quantity_reduction_does_not_reject() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-QTY", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-QTY"),
        Some(VenueOrderId::from("123")),
        Some(Quantity::from("4")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_CANCEL_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    tokio::time::sleep(Duration::from_millis(200)).await;
    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "successful quantity reduction must not emit a modify event, was: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// Modify cannot proceed without a `venue_order_id`. The command must
/// surface a synchronous error rather than silently dropping the request.
#[rstest]
#[tokio::test]
async fn test_modify_order_without_venue_id_returns_error() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-NOID", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-NOID"),
        None,
        Some(Quantity::from("5")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.modify_order(cmd);
    assert!(result.is_err(), "modify without venue_order_id must error");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("venue_order_id"),
        "expected venue_order_id in error message, was: {err}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// An empty full-image OCM (no `orc`) must clear state without producing
/// any execution Reports: the venue uses this to mark "no open orders".
#[rstest]
#[tokio::test]
async fn test_ocm_empty_image_emits_no_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_EMPTY_IMAGE.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(_)) {
            report_seen = true;
            break;
        }
    }

    assert!(
        !report_seen,
        "EMPTY_IMAGE OCM must not emit any execution Reports"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

fn load_fixture_frames(path: &str) -> Vec<String> {
    let body = load_fixture(path);
    let value: Value = serde_json::from_str(&body).expect("fixture is not valid JSON");
    let frames = value
        .as_array()
        .cloned()
        .unwrap_or_else(|| vec![value.clone()]);
    frames.into_iter().map(|v| v.to_string()).collect()
}

async fn write_lines(write_half: &mut tokio::net::tcp::OwnedWriteHalf, lines: &[String]) {
    for line in lines {
        tokio::io::AsyncWriteExt::write_all(write_half, format!("{line}\r\n").as_bytes())
            .await
            .unwrap();
    }
}

/// Three OCMs for the same bet with monotonically increasing `sm` must produce
/// three incremental fill reports, one per `sm` step, because the trade id
/// (`bet_id-sm`) is unique per state.
#[rstest]
#[tokio::test]
async fn test_ocm_multiple_incremental_fills_emits_one_report_per_step() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let frames = load_fixture_frames("stream/ocm_multiple_fills.json");
    assert_eq!(frames.len(), 3, "expected 3 incremental fill frames");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &frames).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut fill_reports = 0;

    for _ in 0..20 {
        match tokio::time::timeout(Duration::from_secs(3), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(ExecutionReport::Fill(_)))) => {
                fill_reports += 1;
                if fill_reports >= 3 {
                    break;
                }
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert_eq!(
        fill_reports, 3,
        "expected exactly one fill report per incremental sm step"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// Replaying the same OCM frame twice must emit a single fill report; the
/// second frame is deduped by trade-id (`bet_id-sm` is identical).
#[rstest]
#[tokio::test]
async fn test_ocm_duplicate_frame_dedupes_fill_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let frames = load_fixture_frames("stream/ocm_multiple_fills.json");
    let single = frames.into_iter().next().unwrap();
    let duplicated = vec![single.clone(), single];

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &duplicated).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut fill_reports = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(ExecutionReport::Fill(_))) {
            fill_reports += 1;
        }
    }

    assert_eq!(
        fill_reports, 1,
        "duplicate OCM frame must not produce a second fill report"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// With `ignore_external_orders=true`, an unmatched order with no `rfo`
/// (no customer order ref, e.g. placed via the venue web UI) must be
/// silently skipped: no execution report, no fill.
#[rstest]
#[tokio::test]
async fn test_ocm_ignore_external_orders_skips_orders_without_rfo() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecConfig::builder()
        .ignore_external_orders(true)
        .build();
    let (mut client, mut rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);

    // OCM frame with an unmatched order missing `rfo` (no customer order ref),
    // simulating an external order placed outside the bot.
    let external_ocm = r#"{
        "op": "ocm",
        "id": 2,
        "clk": "AOQXAPMdAJQWANAfAIQd",
        "pt": 1618710654660,
        "oc": [{
            "id": "1.180604981",
            "orc": [{
                "id": 1209555,
                "uo": [{
                    "id": "999000111",
                    "p": 1.75,
                    "s": 10,
                    "side": "L",
                    "status": "E",
                    "pt": "P",
                    "ot": "L",
                    "pd": 1618710649000,
                    "md": 1618710654000,
                    "avp": 1.73,
                    "sm": 1.12,
                    "sr": 8.88,
                    "sl": 0,
                    "sc": 0,
                    "sv": 0,
                    "rac": "",
                    "rc": "REG_LGA"
                }]
            }]
        }]
    }"#;
    let external_line: String = serde_json::from_str::<Value>(external_ocm)
        .unwrap()
        .to_string();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &[external_line]).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(_)) {
            report_seen = true;
            break;
        }
    }

    assert!(
        !report_seen,
        "external order (no rfo) must be skipped under ignore_external_orders"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// Regression: an empty `rfo` string must be treated identically to a missing
/// `rfo`. Parsers elsewhere normalise `""` to `None`; the
/// `ignore_external_orders` skip must do the same so externally-placed orders
/// (the venue sometimes emits `"rfo": ""`) are silently ignored.
#[rstest]
#[tokio::test]
async fn test_ocm_ignore_external_orders_skips_empty_string_rfo() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecConfig::builder()
        .ignore_external_orders(true)
        .build();
    let (mut client, mut rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);

    // Same shape as the missing-rfo fixture but with `rfo: ""` explicit.
    let external_ocm = r#"{
        "op": "ocm",
        "id": 2,
        "clk": "AOQXAPMdAJQWANAfAIQd",
        "pt": 1618710654660,
        "oc": [{
            "id": "1.180604981",
            "orc": [{
                "id": 1209555,
                "uo": [{
                    "id": "999000222",
                    "p": 1.75,
                    "s": 10,
                    "side": "L",
                    "status": "E",
                    "pt": "P",
                    "ot": "L",
                    "pd": 1618710649000,
                    "md": 1618710654000,
                    "avp": 1.73,
                    "sm": 1.12,
                    "sr": 8.88,
                    "sl": 0,
                    "sc": 0,
                    "sv": 0,
                    "rac": "",
                    "rc": "REG_LGA",
                    "rfo": "",
                    "rfs": ""
                }]
            }]
        }]
    }"#;
    let external_line: String = serde_json::from_str::<Value>(external_ocm)
        .unwrap()
        .to_string();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &[external_line]).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(_)) {
            report_seen = true;
            break;
        }
    }

    assert!(
        !report_seen,
        "external order with empty-string rfo must be skipped under ignore_external_orders"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `stream_market_ids_filter` must drop OCMs for markets outside the filter
/// so multi-strategy deployments can isolate per-instance order streams.
#[rstest]
#[tokio::test]
async fn test_ocm_market_ids_filter_skips_unrelated_markets() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;

    let config = BetfairExecConfig::builder()
        .stream_market_ids_filter(vec!["1.OTHER".to_string()])
        .build();
    let (mut client, mut rx, _data_rx, _cache) =
        create_test_execution_client_with_config(addr, stream_port, config);

    // Multi-fill fixture targets market "1.179082386"; with the filter set to
    // "1.OTHER" the handler must drop every frame.
    let frames = load_fixture_frames("stream/ocm_multiple_fills.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &frames).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(_)) {
            report_seen = true;
            break;
        }
    }

    assert!(
        !report_seen,
        "OCMs for markets outside stream_market_ids_filter must be dropped"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `cancel_order` cannot proceed without a `venue_order_id`. The command must
/// surface a synchronous error rather than silently dropping the request.
#[rstest]
#[tokio::test]
async fn test_cancel_order_without_venue_id_returns_error() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let cmd = CancelOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-CN-NOID"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    let result = client.cancel_order(cmd);
    assert!(result.is_err(), "cancel without venue_order_id must error");
    assert!(
        result.unwrap_err().to_string().contains("venue_order_id"),
        "expected venue_order_id in error message"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A modify with a quantity *increase* (not allowed on Betfair) must emit a
/// ModifyRejected explaining the constraint. Only reductions are valid.
#[rstest]
#[tokio::test]
async fn test_modify_order_quantity_increase_rejects() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-INC", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-INC"),
        Some(VenueOrderId::from("123")),
        Some(Quantity::from("20")),
        None,
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for modify rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::ModifyRejected(rej)) => {
            assert_eq!(rej.client_order_id, ClientOrderId::from("O-MOD-INC"));
            assert!(
                rej.reason.as_str().contains("can only reduce quantity"),
                "expected reduce-only reason, was: {}",
                rej.reason,
            );
        }
        other => panic!("Expected ModifyRejected, was {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A handicap-bearing instrument id (e.g. `1.M-S-1.5.BETFAIR`) must round-trip
/// the handicap into the place instruction so Betfair routes to the correct
/// runner (handicap markets are keyed by selection_id + handicap).
#[rstest]
#[tokio::test]
async fn test_submit_order_with_handicap_includes_handicap_in_instruction() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-1.5.BETFAIR", "O-HCAP", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_PLACE_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let params = state
        .betting_request_params
        .lock()
        .unwrap()
        .iter()
        .find(|(m, _)| m == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;

    let instr = &params["instructions"][0];
    assert_eq!(instr["selectionId"], 86362);
    // Decimals serialise as JSON strings; Betfair accepts the string form.
    assert_eq!(instr["handicap"], "1.5");

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A price modify dispatches `replaceOrders` (Betfair's atomic price update)
/// with the new price attached to the existing bet id; it does NOT call
/// cancelOrders + placeOrders.
#[rstest]
#[tokio::test]
async fn test_modify_price_dispatches_replace_orders_with_new_price() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.179082386-235-0.BETFAIR", "O-MOD-PX", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    let cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        ClientOrderId::from("O-MOD-PX"),
        Some(VenueOrderId::from("228000000111")),
        None,
        Some(Price::from("3.50")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(cmd).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == "SportsAPING/v1.0/replaceOrders")
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let params = state
        .betting_request_params
        .lock()
        .unwrap()
        .iter()
        .find(|(m, _)| m == "SportsAPING/v1.0/replaceOrders")
        .cloned()
        .expect("replaceOrders call must be recorded")
        .1;

    let instr = &params["instructions"][0];
    assert_eq!(instr["betId"], "228000000111");
    // Decimals serialise as JSON strings.
    assert_eq!(instr["newPrice"], "3.50");

    let methods = state.betting_methods.lock().unwrap().clone();
    assert!(
        !methods.iter().any(|m| m == METHOD_PLACE_ORDERS),
        "price modify must not place a new order, only replace; saw: {methods:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `generate_order_status_reports` must transparently recover from a stale
/// session: when `listCurrentOrders` first returns `NO_SESSION`, the client
/// refreshes credentials (keep-alive or full re-login) and retries the same
/// request. The strategy never sees the failure; it just gets the reports.
/// This contract is replicated in three sites in `execution.rs`; covering one
/// of them protects the shared error-classification path.
#[rstest]
#[tokio::test]
async fn test_generate_order_status_reports_recovers_from_no_session() {
    let (addr, state) = start_mock_http().await;

    // Make `listCurrentOrders` fail once with NO_SESSION; the next call
    // (the in-line retry in execution.rs) must succeed with the executable fixture.
    state
        .betting_error_one_shot_overrides
        .lock()
        .unwrap()
        .insert(
            METHOD_LIST_CURRENT_ORDERS.to_string(),
            (-1, "NO_SESSION".to_string()),
        );
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let keep_alives_before = state
        .keep_alive_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let cmd = GenerateOrderStatusReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .open_only(true)
        .build()
        .unwrap();
    let reports = client.generate_order_status_reports(&cmd).await.unwrap();

    assert!(
        !reports.is_empty(),
        "post-recovery listCurrentOrders must yield reports"
    );

    let listcalls = state
        .betting_methods
        .lock()
        .unwrap()
        .iter()
        .filter(|m| *m == METHOD_LIST_CURRENT_ORDERS)
        .count();
    assert_eq!(
        listcalls, 2,
        "session-recovery must retry the same listCurrentOrders call exactly once"
    );

    let keep_alives_after = state
        .keep_alive_count
        .load(std::sync::atomic::Ordering::Relaxed);
    assert!(
        keep_alives_after > keep_alives_before,
        "session-recovery must call keep_alive before retrying (the path under test
         calls keep_alive first, only falling back to a full re-login on its failure)"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `generate_fill_reports` has its own copy of the NO_SESSION recovery
/// branch (`execution.rs:1311`). The duplicated logic means a regression in
/// only the fill-reports path could pass while the order-status-reports test
/// still goes green; cover it with the same one-shot setup.
#[rstest]
#[tokio::test]
async fn test_generate_fill_reports_recovers_from_no_session() {
    let (addr, state) = start_mock_http().await;

    state
        .betting_error_one_shot_overrides
        .lock()
        .unwrap()
        .insert(
            METHOD_LIST_CURRENT_ORDERS.to_string(),
            (-1, "NO_SESSION".to_string()),
        );
    let fixture = load_fixture("rest/list_current_orders_execution_complete.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let keep_alives_before = state
        .keep_alive_count
        .load(std::sync::atomic::Ordering::Relaxed);

    let cmd = GenerateFillReportsBuilder::default()
        .ts_init(UnixNanos::default())
        .build()
        .unwrap();
    let reports = client.generate_fill_reports(cmd).await.unwrap();

    assert!(
        !reports.is_empty(),
        "post-recovery listCurrentOrders must yield fill reports"
    );

    let listcalls = state
        .betting_methods
        .lock()
        .unwrap()
        .iter()
        .filter(|m| *m == METHOD_LIST_CURRENT_ORDERS)
        .count();
    assert_eq!(listcalls, 2, "fill-report recovery must retry once");

    assert!(
        state
            .keep_alive_count
            .load(std::sync::atomic::Ordering::Relaxed)
            > keep_alives_before,
        "fill-report recovery must call keep_alive before retrying"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `query_order` runs through `list_current_orders_with_retry`
/// (`execution.rs:2522`), which is the third copy of the NO_SESSION recovery
/// branch. Verify that path also recovers transparently.
#[rstest]
#[tokio::test]
async fn test_query_order_recovers_from_no_session() {
    let (addr, state) = start_mock_http().await;

    state
        .betting_error_one_shot_overrides
        .lock()
        .unwrap()
        .insert(
            METHOD_LIST_CURRENT_ORDERS.to_string(),
            (-1, "NO_SESSION".to_string()),
        );
    let fixture = load_fixture("rest/list_current_orders_executable.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while tokio::time::timeout(Duration::from_millis(200), rx.recv())
        .await
        .is_ok()
    {}

    let keep_alives_before = state
        .keep_alive_count
        .load(std::sync::atomic::Ordering::Relaxed);

    // query_order issues an rfo lookup and (when venue_order_id is set) a
    // bet_id lookup, both via list_current_orders_with_retry. The NO_SESSION
    // override consumes the first call, so the breakdown is:
    //   rfo  -> NO_SESSION + retry (2 calls)
    //   bet_id lookup       (1 call)
    // Total: 3 listCurrentOrders calls; the recovery happens exactly once.
    let client_order_id = ClientOrderId::from("O-20260418-QUERY-RECOVER");
    let instrument_id = InstrumentId::from("1.180575118-39980.BETFAIR");
    let cmd = QueryOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("228059754671")),
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );

    client.query_order(cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timed out waiting for query_order recovery event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Report(ExecutionReport::Order(report)) => {
            assert_eq!(report.venue_order_id.as_str(), "228059754671");
        }
        other => panic!("Expected OrderStatusReport after recovery, was {other:?}"),
    }

    let listcalls = state
        .betting_methods
        .lock()
        .unwrap()
        .iter()
        .filter(|m| *m == METHOD_LIST_CURRENT_ORDERS)
        .count();
    assert_eq!(
        listcalls, 3,
        "query_order makes rfo + bet_id lookups; recovery on the first adds a single retry"
    );

    assert!(
        state
            .keep_alive_count
            .load(std::sync::atomic::Ordering::Relaxed)
            > keep_alives_before,
        "query_order recovery must call keep_alive before retrying"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// Replace-flow reconciliation: after a successful `replaceOrders`, the OCM
/// will publish a cancel for the *old* bet id (Betfair models a price modify
/// as cancel-old + place-new). The handler must recognise that cancel as part
/// of the replace and suppress it; emitting a CancelRejected or Canceled
/// here would make the strategy think its order was killed even though a
/// fresh bet has just been placed.
#[rstest]
#[tokio::test]
async fn test_replace_flow_suppresses_ocm_cancel_for_old_bet_id() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    // Channel the test uses to push OCM frames into the live stream socket.
    let (ocm_tx, mut ocm_rx) = tokio::sync::mpsc::unbounded_channel::<String>();

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        // Drain the order subscription line.
        let mut sub_line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut sub_line)
            .await
            .unwrap();

        // Forward OCM frames pushed by the test until the test drops `ocm_tx`.
        while let Some(line) = ocm_rx.recv().await {
            tokio::io::AsyncWriteExt::write_all(&mut write_half, format!("{line}\r\n").as_bytes())
                .await
                .unwrap();
        }
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-RPL-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    // Confirm the order was accepted with the expected venue id from the
    // place fixture before kicking off the modify.
    let mut accepted_seen = false;

    for _ in 0..4 {
        match tokio::time::timeout(Duration::from_secs(5), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Accepted(a)))) => {
                assert_eq!(a.venue_order_id, VenueOrderId::from("228302937743"));
                accepted_seen = true;
                break;
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }
    assert!(accepted_seen, "order must be accepted before modify");

    // Modify with a new price -> dispatches replaceOrders. On success the
    // spawned task inserts the old bet id into replaced_venue_order_ids.
    let modify_cmd = ModifyOrder::new(
        TraderId::from("TESTER-001"),
        Some(*BETFAIR_CLIENT_ID),
        StrategyId::from("S-001"),
        InstrumentId::from("1.181005744-86362-0.BETFAIR"),
        ClientOrderId::from("O-RPL-001"),
        Some(VenueOrderId::from("228302937743")),
        None,
        Some(Price::from("3.00")),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
        None, // correlation_id
    );
    client.modify_order(modify_cmd).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == "SportsAPING/v1.0/replaceOrders")
            }
        },
        Duration::from_secs(5),
    )
    .await;

    // The replace task locks ocm_state after the response lands. Brief grace
    // period for the cross-task state update before sending the OCM cancel.
    tokio::time::sleep(Duration::from_millis(150)).await;

    // OCM cancel frame for the OLD bet id with cancel quantity, shaped how
    // the venue emits it as part of a replace.
    let cancel_old_bet_ocm = r#"{
        "op": "ocm",
        "id": 2,
        "clk": "AOQXAPMdAJQWANAfAIQd",
        "pt": 1700000001000,
        "oc": [{
            "id": "1.181005744",
            "orc": [{
                "id": 86362,
                "uo": [{
                    "id": "228302937743",
                    "p": 2.58,
                    "s": 10,
                    "side": "L",
                    "status": "EC",
                    "pt": "P",
                    "ot": "L",
                    "pd": 1700000000000,
                    "md": 1700000001000,
                    "avp": 0.0,
                    "sm": 0,
                    "sr": 0,
                    "sl": 0,
                    "sc": 10,
                    "sv": 0,
                    "rac": "",
                    "rc": "REG_LGA",
                    "rfo": "O-RPL-001",
                    "rfs": "S-001"
                }]
            }]
        }]
    }"#;
    let cancel_line: String = serde_json::from_str::<Value>(cancel_old_bet_ocm)
        .unwrap()
        .to_string();
    ocm_tx.send(cancel_line).unwrap();

    // Suppression must produce zero events for that bet. Drain briefly and
    // assert nothing cancel-shaped lands.
    let mut cancel_event_seen = false;
    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        match event {
            ExecutionEvent::Order(
                OrderEventAny::CancelRejected(_) | OrderEventAny::Canceled(_),
            ) => {
                cancel_event_seen = true;
            }
            ExecutionEvent::Report(_) => {
                report_seen = true;
            }
            _ => {}
        }
    }
    assert!(
        !cancel_event_seen,
        "OCM cancel for replaced bet must not emit a Cancel event"
    );
    assert!(
        !report_seen,
        "OCM cancel for replaced bet must not emit a Report"
    );

    drop(ocm_tx);
    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A FOK limit order must serialise with `timeInForce=FILL_OR_KILL` and no
/// `persistenceType` so Betfair rejects unmatched residue rather than parking
/// it on the book.
#[rstest]
#[tokio::test]
async fn test_submit_order_fok_sends_fill_or_kill_payload() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-FOK"))
        .side(OrderSide::Sell)
        .price(Price::from("2.58"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Fok)
        .build();
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_PLACE_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let params = state
        .betting_request_params
        .lock()
        .unwrap()
        .iter()
        .find(|(m, _)| m == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;

    let limit_order = &params["instructions"][0]["limitOrder"];
    assert_eq!(
        limit_order["timeInForce"], "FILL_OR_KILL",
        "FOK payload must request fill-or-kill semantics"
    );
    assert!(
        limit_order.get("persistenceType").is_none() || limit_order["persistenceType"].is_null(),
        "FOK must not also send a persistenceType",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A Market AtTheClose order must serialise as a `marketOnCloseOrder` (BSP)
/// with the order quantity used as `liability`, not as a regular limit.
#[rstest]
#[tokio::test]
async fn test_submit_order_market_on_close_sends_bsp_instruction() {
    let (addr, state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = OrderTestBuilder::new(OrderType::Market)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-MOC"))
        .side(OrderSide::Buy)
        .quantity(Quantity::from("12"))
        .time_in_force(TimeInForce::AtTheClose)
        .build();
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_PLACE_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let params = state
        .betting_request_params
        .lock()
        .unwrap()
        .iter()
        .find(|(m, _)| m == METHOD_PLACE_ORDERS)
        .cloned()
        .expect("placeOrders call must be recorded")
        .1;

    let instr = &params["instructions"][0];
    assert_eq!(instr["orderType"], "MARKET_ON_CLOSE");
    // Betfair's `Decimal` serialiser emits liability as a JSON string.
    assert_eq!(instr["marketOnCloseOrder"]["liability"], "12");
    assert!(
        instr.get("limitOrder").is_none() || instr["limitOrder"].is_null(),
        "MOC must not include a limitOrder body",
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A submit failure that is "ambiguous" (5xx, network error, timeout) leaves
/// the order in SUBMITTED rather than emitting OrderRejected, because the
/// venue may have processed the order and OCM will reconcile it.
#[rstest]
#[tokio::test]
async fn test_submit_order_ambiguous_5xx_does_not_emit_rejected() {
    let (addr, state) = start_mock_http().await;
    state
        .betting_status_overrides
        .lock()
        .unwrap()
        .insert(METHOD_PLACE_ORDERS.to_string(), 502);

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (_reader, write_half) = accept_and_auth(&listener).await;
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-AMB", "2.58", "10");
    add_order_to_cache(&cache, order.clone());

    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    // Wait for the placeOrders dispatch to actually hit the mock so the
    // no-Rejected assertion below is grounded in the 5xx path having fired.
    // Without this, a regression that stops dispatching placeOrders after
    // local submit would still pass.
    wait_until_async(
        || {
            let methods = Arc::clone(&state.betting_methods);
            async move {
                methods
                    .lock()
                    .unwrap()
                    .iter()
                    .any(|m| m == METHOD_PLACE_ORDERS)
            }
        },
        Duration::from_secs(5),
    )
    .await;

    let mut submitted_seen = false;
    let mut rejected_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
        match event {
            ExecutionEvent::Order(OrderEventAny::Submitted(_)) => submitted_seen = true,
            ExecutionEvent::Order(OrderEventAny::Rejected(_)) => rejected_seen = true,
            _ => {}
        }
    }

    assert!(
        submitted_seen,
        "OrderSubmitted must still be emitted synchronously"
    );
    assert!(
        !rejected_seen,
        "ambiguous 5xx error must NOT emit Rejected; OCM reconciles"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// `FULL_IMAGE_STRATEGY` OCMs carry only matched-order history (`mb`/`ml`) and
/// per-strategy buckets (`smc`), no `uo`. The handler must accept the frame
/// without panicking, but emit no Reports because there are no open orders.
#[rstest]
#[tokio::test]
async fn test_ocm_full_image_strategy_emits_no_report() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let ocm_fixture = load_fixture("stream/ocm_FULL_IMAGE_STRATEGY.json");

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            format!("{}\r\n", ocm_fixture.trim()).as_bytes(),
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut report_seen = false;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
        if matches!(event, ExecutionEvent::Report(_)) {
            report_seen = true;
            break;
        }
    }

    assert!(
        !report_seen,
        "FULL_IMAGE_STRATEGY without `uo` must not emit Reports"
    );
    assert!(
        client.is_connected(),
        "client must remain connected after FULL_IMAGE_STRATEGY"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

/// A second EC (terminal) OCM for the same `bet_id` must be deduped: the first
/// one fully reports the order, the replay must not produce additional Reports.
#[rstest]
#[tokio::test]
async fn test_ocm_duplicate_terminal_event_is_deduped() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    // Frames 0+1 of the duplicate-execution fixture both target bet 230486317487:
    // frame 0 is status=E (sm=1.12), frame 1 is status=EC with sc=8.88 (terminal).
    // Sending frame 1 a second time must emit no further reports.
    let mut frames = load_fixture_frames("stream/ocm_DUPLICATE_EXECUTION.json");
    let terminal_frame = frames.remove(1);
    let initial_frame = frames.remove(0);
    let lines = vec![initial_frame, terminal_frame.clone(), terminal_frame];

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        write_lines(&mut write_half, &lines).await;

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    while rx.try_recv().is_ok() {}

    let mut order_status_reports = 0;
    let mut fill_reports = 0;

    while let Ok(Some(event)) = tokio::time::timeout(Duration::from_secs(2), rx.recv()).await {
        match event {
            ExecutionEvent::Report(ExecutionReport::Order(_)) => order_status_reports += 1,
            ExecutionEvent::Report(ExecutionReport::Fill(_)) => fill_reports += 1,
            _ => {}
        }
    }

    assert_eq!(
        fill_reports, 1,
        "only the first incremental sm should yield a fill"
    );
    assert_eq!(
        order_status_reports, 2,
        "expected one status report per non-deduped frame (first frame + first terminal)"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}

const RECONNECT_CONNECTION_MSG: &[u8] =
    b"{\"op\":\"connection\",\"connectionId\":\"reconnect\"}\r\n";

#[rstest]
#[tokio::test]
async fn test_post_reconnect_dispatches_mass_status() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;

        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        // A second `Connection` message is what the OCM handler treats as a reconnect.
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();

        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let mut saw_mass_status = false;
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    while tokio::time::Instant::now() < deadline {
        if let Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(_)))) =
            tokio::time::timeout(Duration::from_millis(500), rx.recv()).await
        {
            saw_mass_status = true;
            break;
        }
    }
    assert!(
        saw_mass_status,
        "expected ExecutionReport::MassStatus dispatch after reconnect",
    );

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { !halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(!client.is_reconciling());

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_submit_denied_during_reconciliation() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    // Wide window so a submit can land while `is_reconciling` is still set.
    state.betting_response_delays.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(800),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(client.is_reconciling());

    while rx.try_recv().is_ok() {}

    let order = make_test_order("1.181005744-86362-0.BETFAIR", "O-HALT-001", "2.58", "10");
    add_order_to_cache(&cache, order.clone());
    client.submit_order(make_submit_order_cmd(&order)).unwrap();

    let event = tokio::time::timeout(Duration::from_millis(500), rx.recv())
        .await
        .expect("timeout waiting for denied event")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::Denied(denied)) => {
            assert_eq!(denied.client_order_id, ClientOrderId::from("O-HALT-001"));
            assert!(
                denied.reason.as_str().contains("STREAM_RECONCILING"),
                "Expected STREAM_RECONCILING reason, found: {}",
                denied.reason,
            );
        }
        other => panic!("Expected OrderDenied event, found: {other:?}"),
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_queued_reconnect_re_asserts_halt() {
    // Without the per-iteration store(true), iter#2 runs with the flag cleared
    // by iter#1 and submits slip through during the second reconciliation.
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    // Slow each reconcile so the second Connection lands while iter#1 runs.
    state.betting_response_delays.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(500),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();

        // Land the second reconnect mid-flight to exercise the queue race.
        tokio::time::sleep(Duration::from_millis(150)).await;

        tokio::io::AsyncWriteExt::write_all(
            &mut write_half,
            b"{\"op\":\"connection\",\"connectionId\":\"reconnect-2\"}\r\n",
        )
        .await
        .unwrap();

        tokio::time::sleep(Duration::from_secs(10)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    let mut mass_status_count = 0usize;
    let mut iter1_dispatched_at_halt_state: Option<bool> = None;

    let deadline = tokio::time::Instant::now() + Duration::from_secs(10);
    while mass_status_count < 2 && tokio::time::Instant::now() < deadline {
        match tokio::time::timeout(Duration::from_millis(200), rx.recv()).await {
            Ok(Some(ExecutionEvent::Report(ExecutionReport::MassStatus(_)))) => {
                mass_status_count += 1;
                if mass_status_count == 1 {
                    // iter#2 should re-assert the halt at the top of its iteration.
                    wait_until_async(
                        || {
                            let halted = client.is_reconciling();
                            async move { halted }
                        },
                        Duration::from_secs(1),
                    )
                    .await;
                    iter1_dispatched_at_halt_state = Some(client.is_reconciling());
                }
            }
            Ok(Some(_)) => {}
            _ => {}
        }
    }

    assert_eq!(
        mass_status_count, 2,
        "expected one MassStatus per queued reconnect signal, found {mass_status_count}",
    );
    assert_eq!(
        iter1_dispatched_at_halt_state,
        Some(true),
        "expected iter#2 to re-assert is_reconciling after iter#1 cleared it",
    );

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { !halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(!client.is_reconciling());

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_submit_order_list_denied_during_reconciliation() {
    // The list path has its own halt branch and must emit one OrderDenied per leg.
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    state.betting_response_delays.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(800),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(client.is_reconciling());

    while rx.try_recv().is_ok() {}

    let order1 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-HLT-LIST-001"))
        .order_list_id(OrderListId::from("OL-HLT"))
        .side(OrderSide::Sell)
        .price(Price::from("2.58"))
        .quantity(Quantity::from("10"))
        .time_in_force(TimeInForce::Gtc)
        .build();
    let order2 = OrderTestBuilder::new(OrderType::Limit)
        .trader_id(TraderId::from("TESTER-001"))
        .strategy_id(StrategyId::from("S-001"))
        .instrument_id(InstrumentId::from("1.181005744-86362-0.BETFAIR"))
        .client_order_id(ClientOrderId::from("O-HLT-LIST-002"))
        .order_list_id(OrderListId::from("OL-HLT"))
        .side(OrderSide::Sell)
        .price(Price::from("3.00"))
        .quantity(Quantity::from("5"))
        .time_in_force(TimeInForce::Gtc)
        .build();

    add_order_to_cache(&cache, order1.clone());
    add_order_to_cache(&cache, order2.clone());

    let (cmd, _order_list) = make_submit_order_list_cmd(
        "1.181005744-86362-0.BETFAIR",
        &[order1.clone(), order2.clone()],
    );
    client.submit_order_list(cmd).unwrap();

    let mut denied_ids: Vec<ClientOrderId> = Vec::new();
    while denied_ids.len() < 2 {
        match tokio::time::timeout(Duration::from_millis(500), rx.recv()).await {
            Ok(Some(ExecutionEvent::Order(OrderEventAny::Denied(denied)))) => {
                assert!(
                    denied.reason.as_str().contains("STREAM_RECONCILING"),
                    "expected STREAM_RECONCILING reason, found: {}",
                    denied.reason,
                );
                denied_ids.push(denied.client_order_id);
            }
            Ok(Some(_)) => {}
            _ => break,
        }
    }

    assert_eq!(denied_ids.len(), 2, "expected one OrderDenied per leg");
    assert!(denied_ids.contains(&ClientOrderId::from("O-HLT-LIST-001")));
    assert!(denied_ids.contains(&ClientOrderId::from("O-HLT-LIST-002")));

    client.disconnect().await.unwrap();
    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_disconnect_during_reconciliation_clears_halt() {
    // If the client disconnects while the reconnect task is still in flight,
    // clear_resync_state must reset is_reconciling so a future connect/submit
    // cycle isn't permanently halted with STREAM_RECONCILING.
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/list_current_orders_empty.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_LIST_CURRENT_ORDERS.to_string(), v["result"].clone());
    // Slow enough that the disconnect aborts an in-flight reconciliation.
    state.betting_response_delays.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_secs(5),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(15)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(client.is_reconciling());

    // Disconnecting mid-reconcile aborts the reconnect task before it can clear
    // the flag itself; the cleanup in clear_resync_state must do it.
    client.disconnect().await.unwrap();

    assert!(
        !client.is_reconciling(),
        "is_reconciling must be cleared on disconnect even when reconnect task is aborted",
    );

    let _ = server.await;
}

#[rstest]
#[tokio::test]
async fn test_cancel_allowed_during_reconciliation() {
    let (addr, state) = start_mock_http().await;

    let list_fixture = load_fixture("rest/list_current_orders_empty.json");
    let list_v: Value = serde_json::from_str(&list_fixture).unwrap();
    state.betting_overrides.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        list_v["result"].clone(),
    );
    let cancel_fixture = load_fixture("rest/betting_cancel_orders_success.json");
    let cancel_v: Value = serde_json::from_str(&cancel_fixture).unwrap();
    state
        .betting_overrides
        .lock()
        .unwrap()
        .insert(METHOD_CANCEL_ORDERS.to_string(), cancel_v["result"].clone());
    state.betting_response_delays.lock().unwrap().insert(
        METHOD_LIST_CURRENT_ORDERS.to_string(),
        Duration::from_millis(800),
    );

    let (stream_port, listener) = start_mock_stream().await;
    let (mut client, mut rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    let server = tokio::spawn(async move {
        let (mut reader, mut write_half) = accept_and_auth(&listener).await;
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        tokio::io::AsyncWriteExt::write_all(&mut write_half, RECONNECT_CONNECTION_MSG)
            .await
            .unwrap();
        tokio::time::sleep(Duration::from_secs(5)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    wait_until_async(
        || {
            let halted = client.is_reconciling();
            async move { halted }
        },
        Duration::from_secs(2),
    )
    .await;
    assert!(client.is_reconciling());

    while rx.try_recv().is_ok() {}

    let cmd = make_cancel_order("1.179082386-235-0.BETFAIR", "O-CANCEL-001", "1");
    client.cancel_order(cmd).unwrap();

    // Allow the HTTP cancel round-trip to complete, then assert no halt-denied
    // event was emitted (cancels must pass through during reconciliation).
    tokio::time::sleep(Duration::from_millis(200)).await;

    while let Ok(event) = rx.try_recv() {
        if let ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) = event {
            assert!(
                !rejected.reason.as_str().contains("STREAM_RECONCILING"),
                "Cancel must not be denied with STREAM_RECONCILING during reconciliation",
            );
        }
    }

    client.disconnect().await.unwrap();
    let _ = server.await;
}
