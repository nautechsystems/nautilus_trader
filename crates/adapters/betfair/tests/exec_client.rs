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
    common::consts::{METHOD_CANCEL_ORDERS, METHOD_LIST_CURRENT_ORDERS, METHOD_PLACE_ORDERS},
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
            cancel::{CancelAllOrders, CancelOrder},
            modify::ModifyOrder,
            query::QueryOrder,
            report::{GenerateFillReportsBuilder, GenerateOrderStatusReportsBuilder},
            submit::SubmitOrder,
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
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    orders::{OrderAny, builder::OrderTestBuilder},
    types::{Currency, Price, Quantity},
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

fn create_test_execution_client(
    addr: SocketAddr,
    stream_port: u16,
) -> (
    BetfairExecutionClient,
    tokio::sync::mpsc::UnboundedReceiver<ExecutionEvent>,
    tokio::sync::mpsc::UnboundedReceiver<DataEvent>,
    Rc<RefCell<Cache>>,
) {
    let trader_id = TraderId::from("TESTER-001");
    let account_id = AccountId::from("BETFAIR-001");
    let client_id = ClientId::from("BETFAIR");
    let cache = Rc::new(RefCell::new(Cache::default()));

    let core = ExecutionClientCore::new(
        trader_id,
        client_id,
        Venue::from("BETFAIR"),
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
        BetfairExecConfig::default(),
        currency,
    );
    client.start().unwrap();

    (client, rx, data_rx, cache)
}

#[rstest]
#[tokio::test]
async fn test_exec_client_creation() {
    let (addr, _state) = start_mock_http().await;
    let (stream_port, _listener) = start_mock_stream().await;
    let (client, _rx, _data_rx, _cache) = create_test_execution_client(addr, stream_port);

    assert_eq!(client.client_id(), ClientId::from("BETFAIR"));
    assert_eq!(client.account_id(), AccountId::from("BETFAIR-001"));
    assert_eq!(client.venue(), Venue::from("BETFAIR"));
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
        Some(ClientId::from("BETFAIR")),
        StrategyId::from("S-001"),
        InstrumentId::from(instrument_id),
        ClientOrderId::from(client_order_id),
        Some(VenueOrderId::from(venue_order_id)),
        UUID4::new(),
        UnixNanos::default(),
        None,
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
        .add_order(order, None, Some(ClientId::from("BETFAIR")), false)
        .unwrap();
}

fn make_submit_order_cmd(order: &OrderAny) -> SubmitOrder {
    SubmitOrder::from_order(
        order,
        TraderId::from("TESTER-001"),
        Some(ClientId::from("BETFAIR")),
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
        Some(ClientId::from("BETFAIR")),
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
        Some(ClientId::from("BETFAIR")),
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
        Some(ClientId::from("BETFAIR")),
        StrategyId::from("S-001"),
        InstrumentId::from("1.179082386-235-0.BETFAIR"),
        OrderSide::NoOrderSide,
        UUID4::new(),
        UnixNanos::default(),
        None,
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
        Some(ClientId::from("BETFAIR")),
        StrategyId::from("S-001"),
        instrument_id,
        client_order_id,
        Some(VenueOrderId::from("228059754671")),
        UUID4::new(),
        UnixNanos::default(),
        None,
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
        Some(ClientId::from("BETFAIR")),
        StrategyId::from("S-001"),
        InstrumentId::from("1.180575118-39980.BETFAIR"),
        ClientOrderId::from("O-20260418-MISS"),
        None,
        UUID4::new(),
        UnixNanos::default(),
        None,
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
