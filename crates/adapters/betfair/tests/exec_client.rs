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

use std::{cell::RefCell, net::SocketAddr, rc::Rc, time::Duration};

use nautilus_betfair::{config::BetfairExecConfig, execution::BetfairExecutionClient};
use nautilus_common::{
    cache::Cache,
    clients::ExecutionClient,
    live::runner::{set_data_event_sender, set_exec_event_sender},
    messages::{DataEvent, ExecutionEvent, execution::cancel::CancelOrder},
};
use nautilus_core::{UUID4, UnixNanos};
use nautilus_live::ExecutionClientCore;
use nautilus_model::{
    data::Data,
    enums::{AccountType, OmsType},
    events::OrderEventAny,
    identifiers::{
        AccountId, ClientId, ClientOrderId, InstrumentId, StrategyId, TraderId, Venue, VenueOrderId,
    },
    types::Currency,
};
use rstest::rstest;
use serde_json::Value;

use crate::common::*;

#[allow(clippy::type_complexity)]
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

    let server = tokio::spawn(async move {
        let (mut reader, write_half) = accept_and_auth(&listener).await;

        // Skip auth line from subscribe_orders combined write
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        line.clear();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();

        let json: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(json["op"], "orderSubscription");

        tokio::time::sleep(Duration::from_secs(2)).await;
        drop(write_half);
    });

    client.connect().await.unwrap();

    assert!(client.is_connected());
    assert!(state.login_count.load(std::sync::atomic::Ordering::Relaxed) > 0);

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

        // Wait for the subscribe_orders combined write (auth + subscription)
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        line.clear();
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

        // Wait for the subscribe_orders combined write (auth + subscription)
        let mut line = String::new();
        tokio::io::AsyncBufReadExt::read_line(&mut reader, &mut line)
            .await
            .unwrap();
        line.clear();
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
    state.betting_overrides.lock().unwrap().insert(
        "SportsAPING/v1.0/cancelOrders".to_string(),
        v["result"].clone(),
    );

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
    client.cancel_order(&cmd).unwrap();

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
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().unwrap().insert(
        "SportsAPING/v1.0/cancelOrders".to_string(),
        v["result"].clone(),
    );

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
    client.cancel_order(&cmd).unwrap();

    let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
        .await
        .expect("timeout waiting for cancel rejected")
        .expect("channel closed");

    match event {
        ExecutionEvent::Order(OrderEventAny::CancelRejected(rejected)) => {
            assert_eq!(rejected.client_order_id, ClientOrderId::from("O-002"));
            assert!(
                rejected.reason.as_str().contains("ErrorInOrder"),
                "Expected ErrorInOrder reason, found: {}",
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
async fn test_cancel_order_result_failure_no_instructions_emits_rejected() {
    let (addr, state) = start_mock_http().await;

    let fixture = load_fixture("rest/betting_cancel_orders_result_failure.json");
    let v: Value = serde_json::from_str(&fixture).unwrap();
    state.betting_overrides.lock().unwrap().insert(
        "SportsAPING/v1.0/cancelOrders".to_string(),
        v["result"].clone(),
    );

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
    client.cancel_order(&cmd).unwrap();

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
    state.betting_overrides.lock().unwrap().insert(
        "SportsAPING/v1.0/cancelOrders".to_string(),
        v["result"].clone(),
    );

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
    client.cancel_order(&cmd).unwrap();

    tokio::time::sleep(Duration::from_millis(500)).await;

    let event = rx.try_recv();
    assert!(
        event.is_err(),
        "Successful cancel should not emit rejected event, found: {event:?}"
    );

    client.disconnect().await.unwrap();
    let _ = server.await;
}
